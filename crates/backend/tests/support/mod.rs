//! Real SQLite and gRPC fixtures with controlled, deterministic Worker responses.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use acorn_backend::{
    config::Config,
    database::Database,
    scheduler::Scheduler,
    submissions::{CreateSubmission, Submission, SubmissionState},
};
use acorn_protocol::worker::v1::{
    self as proto,
    judge_worker_server::{JudgeWorker, JudgeWorkerServer},
};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tonic::{
    Request as RpcRequest, Response, Status, codegen::tokio_stream::wrappers::TcpListenerStream,
    transport::Server,
};
use uuid::Uuid;

/// Actual initialized database, never mocked SQL behavior.
pub struct Fixture {
    /// Temporary database and process-test files.
    pub directory: tempfile::TempDir,
    /// Deployment-aligned limits, changed by each test before assembling services.
    pub config: Config,
    /// Shared single-connection application pool.
    pub database: Database,
}

impl Fixture {
    /// Initialize through the shipped SQL files and check the same startup contract.
    pub async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut config =
            Config::parse(include_str!("../../../../config/backend.example.toml")).unwrap();
        config.database.path = directory.path().join("acorn.db");
        let mut connection = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&config.database.path)
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
        for script in [
            include_str!("../../../../sql/001-initial-schema.sql"),
            include_str!("../../../../examples/seed-user.sql"),
            include_str!("../../../../sql/002-example-problem.sql"),
        ] {
            sqlx::raw_sql(script)
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let database = Database::open(&config).await.unwrap();
        Self {
            directory,
            config,
            database,
        }
    }
}

/// Explicit protocol behavior; failures require no unreliable network timing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Refuse admission without registering any task.
    Busy,
    /// Return valid terminals according to a trusted fixture source marker.
    Finish,
    /// Remain running until the test sets the release flag.
    Running,
    /// Register a Job but return unavailable from SubmitJob.
    AmbiguousFound,
    /// Return unavailable from SubmitJob and not found from GetJob.
    AmbiguousMissing,
    /// Accept, then report the Job missing during ordinary polling.
    PollMissing,
    /// Return a terminal with the wrong Job ID.
    Malformed,
    /// Return a terminal but fail the single ACK call.
    AckFailure,
    /// Wait on a release flag before responding to SubmitJob.
    AwaitAdmission,
    /// Reject task data with the documented deterministic error.
    Invalid,
}

/// Controlled service retains observable calls for assertions.
pub struct Mock {
    /// The one scenario selected for this endpoint.
    pub mode: Mode,
    /// Original submissions observed at this endpoint.
    pub submitted: Mutex<HashMap<String, proto::SubmitJobRequest>>,
    /// Number of SubmitJob calls, including refusals.
    pub submits: AtomicUsize,
    /// Number of GetJob calls, including failures.
    pub gets: AtomicUsize,
    /// Number of AcknowledgeResult calls, including failed ACKs.
    pub acks: AtomicUsize,
    /// Test-controlled completion or admission gate.
    pub released: AtomicBool,
    /// Real database used to assert commit happens before ACK.
    database: Database,
}

#[tonic::async_trait]
impl JudgeWorker for Mock {
    /// Accept or explicitly refuse according to the chosen controlled behavior.
    async fn submit_job(
        &self,
        request: RpcRequest<proto::SubmitJobRequest>,
    ) -> Result<Response<proto::SubmitJobReply>, Status> {
        self.submits.fetch_add(1, Ordering::SeqCst);
        if self.mode == Mode::Busy {
            return Err(Status::resource_exhausted("WorkerBusy"));
        }
        if self.mode == Mode::Invalid {
            return Err(Status::invalid_argument("InvalidJob"));
        }
        let task = request.into_inner();
        let id = task.job_id.clone();
        self.submitted.lock().unwrap().insert(id.clone(), task);
        if matches!(self.mode, Mode::AmbiguousFound | Mode::AmbiguousMissing) {
            return Err(Status::unavailable("Controlled ambiguous submit"));
        }
        if self.mode == Mode::AwaitAdmission {
            wait_for(|| self.released.load(Ordering::SeqCst)).await;
        }
        Ok(Response::new(proto::SubmitJobReply {
            job_id: id,
            state: proto::JobState::Running as i32,
            duplicate: false,
        }))
    }

    /// Return controlled progress, missing records, or bounded binary terminal fields.
    async fn get_job(
        &self,
        request: RpcRequest<proto::GetJobRequest>,
    ) -> Result<Response<proto::GetJobReply>, Status> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        if matches!(self.mode, Mode::AmbiguousMissing | Mode::PollMissing) {
            return Err(Status::not_found("JobNotFound"));
        }
        let id = request.into_inner().job_id;
        let task = self.submitted.lock().unwrap().get(&id).cloned().unwrap();
        let total = task.cases.len() as u32;
        if self.mode == Mode::Running && !self.released.load(Ordering::SeqCst) {
            return Ok(Response::new(proto::GetJobReply {
                job_id: id,
                state: proto::JobState::Running as i32,
                phase: proto::JobPhase::RunningCase as i32,
                total_cases: total,
                ..Default::default()
            }));
        }
        let mut reply = proto::GetJobReply {
            job_id: id,
            state: proto::JobState::Finished as i32,
            phase: proto::JobPhase::Done as i32,
            total_cases: total,
            completed_cases: total,
            terminal: Some(proto::get_job_reply::Terminal::Result(proto::JudgeResult {
                verdict: proto::Verdict::Accepted as i32,
                ..Default::default()
            })),
        };
        if task.source == b"ce" {
            reply.completed_cases = 0;
            reply.terminal = Some(proto::get_job_reply::Terminal::Result(proto::JudgeResult {
                verdict: proto::Verdict::CompileError as i32,
                compile_diagnostic: vec![0, 255, 10],
                compile_diagnostic_truncated: true,
                ..Default::default()
            }));
        } else if task.source == b"wa" {
            reply.completed_cases = 1;
            reply.terminal = Some(proto::get_job_reply::Terminal::Result(proto::JudgeResult {
                verdict: proto::Verdict::WrongAnswer as i32,
                failed_case: Some(proto::FailedCase {
                    case_index: 1,
                    stdout: vec![0, 255],
                    resources: Some(proto::ResourceUsage {
                        cpu_time_us: Some(0),
                        ..Default::default()
                    }),
                    exit: Some(proto::failed_case::Exit::ExitCode(0)),
                    ..Default::default()
                }),
                ..Default::default()
            }));
        }
        if self.mode == Mode::Malformed {
            reply.job_id = "wrong-id".into();
        }
        Ok(Response::new(reply))
    }

    /// Assert the real committed terminal is already visible before accepting ACK.
    async fn acknowledge_result(
        &self,
        request: RpcRequest<proto::AcknowledgeResultRequest>,
    ) -> Result<Response<proto::AcknowledgeResultReply>, Status> {
        let submission = self
            .database
            .submission(&request.into_inner().job_id)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            submission.info.state,
            SubmissionState::Finished | SubmissionState::Failed
        ));
        self.acks.fetch_add(1, Ordering::SeqCst);
        if self.mode == Mode::AckFailure {
            return Err(Status::unavailable("Controlled ACK failure"));
        }
        Ok(Response::new(proto::AcknowledgeResultReply {}))
    }
}

/// Actual gRPC server lifecycle with a controlled service implementation.
pub struct RpcServer {
    /// Calls and release gate accessible to the test.
    pub mock: Arc<Mock>,
    /// Endpoint URI used by the production Worker client.
    pub address: String,
    /// Explicit listener shutdown signal.
    stop: oneshot::Sender<()>,
    /// Server task, observed during fixture cleanup.
    task: JoinHandle<Result<(), tonic::transport::Error>>,
}

impl RpcServer {
    /// Bind a unique loopback endpoint before starting the transport task.
    pub async fn start(mode: Mode, database: Database) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let mock = Arc::new(Mock {
            mode,
            submitted: Mutex::new(HashMap::new()),
            submits: AtomicUsize::new(0),
            gets: AtomicUsize::new(0),
            acks: AtomicUsize::new(0),
            released: AtomicBool::new(false),
            database,
        });
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(
            Server::builder()
                .add_service(JudgeWorkerServer::from_arc(mock.clone()))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = stopped.await;
                }),
        );
        Self {
            mock,
            address,
            stop,
            task,
        }
    }

    /// Stop the server only after Scheduler tracking has drained.
    pub async fn close(self) {
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

/// P07 assembly with real transport and SQLite and replaceable Worker behavior.
pub struct Harness {
    /// Database files and configuration.
    pub fixture: Fixture,
    /// Production Scheduler handle.
    pub scheduler: Scheduler,
    /// Independently controlled Worker endpoints.
    pub servers: Vec<RpcServer>,
    /// Production dispatcher task, observed at cleanup.
    task: JoinHandle<anyhow::Result<()>>,
}

impl Harness {
    /// Assemble all services with slow retry so busy-refusal assertions stay deterministic.
    pub async fn start(modes: &[Mode], capacity: u32) -> Self {
        let mut fixture = Fixture::new().await;
        fixture.config.scheduler.pending_max_count = capacity;
        fixture.config.scheduler.retry_interval_ms = 60_000;
        fixture.config.scheduler.poll_interval_ms = 10;
        let mut servers = Vec::new();
        for &mode in modes {
            servers.push(RpcServer::start(mode, fixture.database.clone()).await);
        }
        fixture.config.scheduler.workers = servers
            .iter()
            .map(|server| server.address.clone())
            .collect();
        let (scheduler, task) =
            Scheduler::start(&fixture.config, fixture.database.clone()).unwrap();
        Self {
            fixture,
            scheduler,
            servers,
            task,
        }
    }

    /// Reserve capacity before creating and handing off a task without HTTP.
    pub async fn submit(&self, language: &str, source: &str) -> String {
        let permit = self.scheduler.reserve().unwrap();
        let created = self
            .fixture
            .database
            .create_submission(CreateSubmission {
                problem_id: 1,
                language_id: language.into(),
                source: source.as_bytes().to_vec(),
                request_key: Uuid::new_v4(),
            })
            .await
            .unwrap();
        let id = created.info.id;
        assert!(
            self.scheduler
                .enqueue(created.task.unwrap(), permit)
                .unwrap()
        );
        id
    }

    /// Wait for persistence and then tracking cleanup, including the one bounded ACK.
    pub async fn terminal(&self, id: &str) -> Submission {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let submission = self.fixture.database.submission(id).await.unwrap().unwrap();
                if matches!(
                    submission.info.state,
                    SubmissionState::Finished | SubmissionState::Failed
                ) && !self.scheduler.contains(id)
                {
                    return submission;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap()
    }

    /// Drain tracking before closing transport and database.
    pub async fn close(self) {
        self.scheduler.pause();
        for server in &self.servers {
            server.mock.released.store(true, Ordering::SeqCst);
        }
        self.scheduler.shutdown();
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        for server in self.servers {
            server.close().await;
        }
        self.fixture.database.close().await;
    }

    /// Observe an expected fatal dispatcher error rather than claiming normal shutdown.
    pub async fn close_failed(self) {
        assert!(
            tokio::time::timeout(Duration::from_secs(5), self.task)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        for server in self.servers {
            server.close().await;
        }
    }
}

/// Wait on an observable fixture event with a bound, avoiding rare network race construction.
pub async fn wait_for(condition: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
