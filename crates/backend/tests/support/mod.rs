//! Real SQLite and gRPC fixtures with controlled, deterministic Worker responses.

pub mod real;

use std::{
    collections::HashMap,
    net::SocketAddr,
    process::{ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use acorn_backend::{
    config::Config,
    database::Database,
    http,
    scheduler::Scheduler,
    submissions::{CreateSubmission, CreationService, Submission, SubmissionState},
};
use acorn_protocol::worker::v1::{
    self as proto,
    judge_worker_server::{JudgeWorker, JudgeWorkerServer},
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use tokio::{
    io::{AsyncBufReadExt, BufReader, Lines},
    net::TcpListener,
    process::{Child, ChildStderr},
    sync::oneshot,
    task::JoinHandle,
};
use tonic::{
    Request as RpcRequest, Response, Status, codegen::tokio_stream::wrappers::TcpListenerStream,
    transport::Server,
};
use tower::ServiceExt;
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

    /// Deployment-side connection for controlled publication and lock contention.
    pub async fn connection(&self) -> SqliteConnection {
        SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&self.config.database.path)
                .foreign_keys(true),
        )
        .await
        .unwrap()
    }

    /// Publish a second immutable version while retaining all original version data.
    pub async fn publish_next(&self) {
        let mut connection = self.connection().await;
        sqlx::raw_sql("BEGIN IMMEDIATE;
            INSERT INTO problem_versions SELECT problem_id, 2, 'New title', statement, comparison_mode, published_at_ms + 1 FROM problem_versions WHERE problem_id = 1 AND version_no = 1;
            INSERT INTO problem_version_languages SELECT problem_id, 2, language_id, cpu_time_ms + 1, wall_time_ms, memory_kib FROM problem_version_languages WHERE problem_id = 1 AND version_no = 1;
            INSERT INTO test_cases SELECT problem_id, 2, case_index, input, expected_output FROM test_cases WHERE problem_id = 1 AND version_no = 1;
            UPDATE problems SET current_version_no = 2 WHERE id = 1; COMMIT;").execute(&mut connection).await.unwrap();
        connection.close().await.unwrap();
    }

    /// Persist a queued task without handing it to a running Scheduler.
    pub async fn queued(&self, language: &str) -> String {
        self.database
            .create_submission(CreateSubmission {
                problem_id: 1,
                language_id: language.into(),
                source: b"ac".to_vec(),
                request_key: Uuid::new_v4(),
            })
            .await
            .unwrap()
            .info
            .id
    }
}

/// Real backend executable with captured logs and deployment-style termination.
pub struct BackendProcess {
    /// Child is killed if a test fails before explicit cleanup.
    child: Child,
    /// Pipe drained during event waits and final process collection.
    lines: Lines<BufReader<ChildStderr>>,
    /// All collected startup, shutdown and error messages.
    captured: String,
}

impl BackendProcess {
    /// Write a temporary deployment config and start the actual backend executable.
    pub fn spawn(fixture: &Fixture) -> Self {
        let mut config: toml::Value =
            toml::from_str(include_str!("../../../../config/backend.example.toml")).unwrap();
        config["http"]["listen"] = "127.0.0.1:0".into();
        config["database"]["path"] = fixture.config.database.path.to_str().unwrap().into();
        config["database"]["busy_timeout_ms"] =
            (fixture.config.database.busy_timeout_ms as i64).into();
        config["scheduler"]["pending_max_count"] =
            i64::from(fixture.config.scheduler.pending_max_count).into();
        config["scheduler"]["workers"] = toml::Value::Array(
            fixture
                .config
                .scheduler
                .workers
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
        );
        config["scheduler"]["poll_interval_ms"] =
            (fixture.config.scheduler.poll_interval_ms as i64).into();
        config["scheduler"]["retry_interval_ms"] =
            (fixture.config.scheduler.retry_interval_ms as i64).into();
        let path = fixture
            .directory
            .path()
            .join(format!("backend-{}.toml", Uuid::new_v4()));
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_acorn-backend"))
            .arg("--config")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let lines = BufReader::new(child.stderr.take().unwrap()).lines();
        Self {
            child,
            lines,
            captured: String::new(),
        }
    }

    /// Observe a process event without relying on a fixed sleep.
    pub async fn until_log(&mut self, event: &str) -> String {
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(line) = self.lines.next_line().await.unwrap() {
                self.captured.push_str(&line);
                self.captured.push('\n');
                if line.contains(event) {
                    return line;
                }
            }
            panic!("Backend exited before {event}: {}", self.captured);
        })
        .await
        .unwrap()
    }

    /// Return the actual random HTTP address after all startup checks finish.
    pub async fn listening(&mut self) -> SocketAddr {
        self.until_log("Backend HTTP listening")
            .await
            .split("listen=")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap()
    }

    /// Send the deployment service manager's normal shutdown signal.
    pub fn terminate(&self) {
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(self.child.id().unwrap() as i32),
            nix::sys::signal::Signal::SIGTERM,
        )
        .unwrap();
    }

    /// Collect exit status and all remaining logs while draining the stderr pipe.
    pub async fn finish(mut self) -> (ExitStatus, String) {
        let (status, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(self.child.wait(), async {
                while let Some(line) = self.lines.next_line().await.unwrap() {
                    self.captured.push_str(&line);
                    self.captured.push('\n');
                }
            })
        })
        .await
        .unwrap();
        (status.unwrap(), self.captured)
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
    /// Accepted Job IDs in observed submission order, for startup FIFO assertions.
    pub submission_order: Mutex<Vec<String>>,
    /// Number of GetJob calls, including failures.
    pub gets: AtomicUsize,
    /// Number of AcknowledgeResult calls, including failed ACKs.
    pub acks: AtomicUsize,
    /// Number of status checks, distinct from Job queries and ACKs.
    pub status_calls: AtomicUsize,
    /// Controlled status outcome; None waits until the caller's deadline or cancellation.
    pub status_reply: Mutex<Option<Result<proto::GetWorkerStatusReply, Status>>>,
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
        self.submission_order.lock().unwrap().push(id.clone());
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

    /// Return the selected health outcome without changing the controlled judging scenario.
    async fn get_worker_status(
        &self,
        _request: RpcRequest<proto::GetWorkerStatusRequest>,
    ) -> Result<Response<proto::GetWorkerStatusReply>, Status> {
        self.status_calls.fetch_add(1, Ordering::SeqCst);
        let reply = self.status_reply.lock().unwrap().clone();
        match reply {
            Some(reply) => reply.map(Response::new),
            None => std::future::pending().await,
        }
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
            submission_order: Mutex::new(Vec::new()),
            gets: AtomicUsize::new(0),
            acks: AtomicUsize::new(0),
            status_calls: AtomicUsize::new(0),
            status_reply: Mutex::new(Some(Ok(proto::GetWorkerStatusReply {
                stopping: false,
                execution_state: proto::WorkerExecutionState::Idle as i32,
                unacknowledged_results: 0,
                unacknowledged_capacity: 128,
            }))),
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

/// P07/P08 assembly with real transport and SQLite and replaceable Worker behavior.
pub struct Harness {
    /// Database files and configuration.
    pub fixture: Fixture,
    /// Production Scheduler handle.
    pub scheduler: Scheduler,
    /// Production owned creation service.
    pub creation: CreationService,
    /// Production HTTP routes invoked as Tower requests.
    pub app: Router,
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
            Scheduler::start(&fixture.config, fixture.database.clone(), Vec::new()).unwrap();
        let creation =
            CreationService::new(&fixture.config, fixture.database.clone(), scheduler.clone());
        let app = http::router(&fixture.config, fixture.database.clone(), creation.clone());
        Self {
            fixture,
            scheduler,
            creation,
            app,
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

    /// Drain owned creation and tracking before closing transport and database.
    pub async fn close(self) {
        self.creation.stop();
        self.scheduler.pause();
        self.creation.wait_idle().await;
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
        self.creation.stop();
        self.creation.wait_idle().await;
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

/// Make an HTTP request and decode the API response as JSON.
pub async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    key: Option<Uuid>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(key) = key {
        builder = builder.header("idempotency-key", key.to_string());
    }
    let body = if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}

/// Create a fixture submission and return its identity after checking the immediate response.
pub async fn create(app: &Router, language: &str, source: &str) -> String {
    let (status, headers, body) = call(
        app,
        "POST",
        "/api/v1/submissions",
        Some(json!({"problem_id": 1, "language_id": language, "source": source})),
        Some(Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let id = body["submission_id"].as_str().unwrap().to_owned();
    assert_eq!(headers["location"], format!("/api/v1/submissions/{id}"));
    id
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

/// Use actual HTTP/1.0 over a loopback socket for real end-to-end acceptance.
pub async fn network_call(
    address: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: Option<Value>,
    key: Option<Uuid>,
) -> (u16, Value) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let payload = body.map(|body| body.to_string()).unwrap_or_default();
    let key = key
        .map(|key| format!("Idempotency-Key: {key}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.0\r\nHost: localhost\r\nContent-Type: application/json\r\n{key}Content-Length: {}\r\n\r\n{payload}",
        payload.len()
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await.unwrap();
        let boundary = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (
            status,
            serde_json::from_slice(&bytes[boundary + 4..]).unwrap(),
        )
    })
    .await
    .unwrap()
}
