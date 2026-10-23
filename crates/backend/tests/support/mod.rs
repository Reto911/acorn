//! MySQL, HTTP and status fixtures with explicit production event processing.

pub mod broker;
pub mod real;

use std::{
    net::SocketAddr,
    process::{ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use acorn_backend::{
    config::HealthConfig,
    health::Health,
    http,
    llm_review::ReviewService,
    membership::Membership,
    scheduler::Scheduler,
    submissions::{CreateSubmission, CreationService, Submission},
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
use sqlx::Connection;
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

pub mod database;
pub use database::Fixture;

impl Fixture {
    /// Build a child-process TOML value without serializing the password.
    pub fn process_config(&self) -> toml::Value {
        let mut config: toml::Value =
            toml::from_str(include_str!("../../../../config/backend.example.toml")).unwrap();
        let database = &self.config.database;
        config["database"]["endpoint"] = database.endpoint.clone().into();
        config["database"]["name"] = database.name.clone().into();
        config["database"]["username"] = database.username.clone().into();
        config["database"]["max_connections"] = i64::from(database.max_connections).into();
        config["database"]["acquire_timeout_ms"] = (database.acquire_timeout_ms as i64).into();
        config["database"]["lock_wait_timeout_secs"] =
            i64::from(database.lock_wait_timeout_secs).into();
        config
    }

    /// Publish a second immutable version while retaining all original version data.
    pub async fn publish_next(&self) {
        let mut connection = self.connection().await;
        sqlx::raw_sql("START TRANSACTION;
            INSERT INTO problem_versions (problem_id, version_no, title, statement, comparison_mode, published_at_ms) SELECT problem_id, 2, 'New title', statement, comparison_mode, published_at_ms + 1 FROM problem_versions WHERE problem_id = 1 AND version_no = 1;
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

/// Supply a fresh controlled group sample for tests that do not exercise broker observation.
pub fn observed_membership(config: &acorn_backend::config::Config) -> Membership {
    let membership = Membership::new(&config.worker_membership).unwrap();
    membership.record(Ok(1)).unwrap();
    membership
}

/// Real backend executable with captured logs and deployment-style termination.
pub struct BackendProcess {
    /// Child is killed if a test fails before explicit cleanup.
    child: Child,
    /// Keep the child deployment TOML alive until the process has ended.
    _directory: tempfile::TempDir,
    /// Pipe drained during event waits and final process collection.
    lines: Lines<BufReader<ChildStderr>>,
    /// All collected startup, shutdown and error messages.
    captured: String,
}

impl BackendProcess {
    /// Write a temporary deployment config and start the actual backend executable.
    pub fn spawn(fixture: &Fixture) -> Self {
        let mut config = fixture.process_config();
        config["http"]["listen"] = "127.0.0.1:0".into();
        config["logging"]["level"] = fixture.config.logging.level.clone().into();
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
        config["scheduler"]["retry_interval_ms"] =
            (fixture.config.scheduler.retry_interval_ms as i64).into();
        config["health"]["interval_ms"] = (fixture.config.health.interval_ms as i64).into();
        config["health"]["timeout_ms"] = (fixture.config.health.timeout_ms as i64).into();
        config["kafka"]["bootstrap_servers"] = toml::Value::Array(
            fixture
                .config
                .kafka
                .bootstrap_servers
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
        );
        config["kafka"]["tasks_topic"] = fixture.config.kafka.tasks_topic.clone().into();
        config["kafka"]["events_topic"] = fixture.config.kafka.events_topic.clone().into();
        config["kafka"]["group_id"] = fixture.config.kafka.group_id.clone().into();
        config["worker_membership"]["tasks_group_id"] = fixture
            .config
            .worker_membership
            .tasks_group_id
            .clone()
            .into();
        config["worker_membership"]["interval_ms"] =
            (fixture.config.worker_membership.interval_ms as i64).into();
        config["worker_membership"]["timeout_ms"] =
            (fixture.config.worker_membership.timeout_ms as i64).into();
        config["kafka"]["shutdown_timeout_ms"] =
            (fixture.config.kafka.shutdown_timeout_ms as i64).into();
        let review = &fixture.config.llm_review;
        config["llm_review"]["enabled"] = review.enabled.into();
        config["llm_review"]["endpoint"] = review.endpoint.clone().into();
        config["llm_review"]["api_key"] = review.api_key.clone().into();
        config["llm_review"]["model"] = review.model.clone().into();
        config["llm_review"]["reasoning_effort"] = review.reasoning_effort.clone().into();
        config["llm_review"]["timeout_ms"] = (review.timeout_ms as i64).into();
        config["llm_review"]["max_concurrent"] = i64::from(review.max_concurrent).into();
        config["llm_review"]["pending_max_count"] = i64::from(review.pending_max_count).into();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backend.toml");
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_acorn-backend"))
            .arg("--config")
            .arg(path)
            .env("MYSQL_PASSWORD", std::env::var("MYSQL_PASSWORD").unwrap())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let lines = BufReader::new(child.stderr.take().unwrap()).lines();
        Self {
            child,
            _directory: directory,
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

/// Controlled read-only status service; it never receives judging tasks.
pub struct Mock {
    /// Number of status requests used to observe collection rounds.
    pub status_calls: AtomicUsize,
    /// Controlled response, or None to wait for the caller's deadline.
    pub status_reply: Mutex<Option<Result<proto::GetWorkerStatusReply, Status>>>,
}

#[tonic::async_trait]
impl JudgeWorker for Mock {
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
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let mock = Arc::new(Mock {
            status_calls: AtomicUsize::new(0),
            status_reply: Mutex::new(Some(Ok(proto::GetWorkerStatusReply {
                stopping: false,
                execution_state: proto::WorkerExecutionState::Idle as i32,
                unacknowledged_results: 0,
                unacknowledged_capacity: 128,
            }))),
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

    /// Stop the status server after its health collector has drained.
    pub async fn close(self) {
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

/// HTTP and MySQL assembly with controlled events and real status transport.
pub struct Harness {
    /// Database files and configuration.
    pub fixture: Fixture,
    /// Production Scheduler handle.
    pub scheduler: Scheduler,
    /// Production owned creation service.
    pub creation: CreationService,
    /// Controlled observations isolate creation checks from a real broker.
    pub membership: Membership,
    /// The same passive cache exposed by the production router.
    pub health: Health,
    /// Production independent review service, shared with Scheduler.
    pub review: ReviewService,
    /// Production HTTP routes invoked as Tower requests.
    pub app: Router,
    /// Independently controlled Worker endpoints.
    pub servers: Vec<RpcServer>,
    /// Collector observed once, including tests that stop it before full teardown.
    health_task: Option<JoinHandle<anyhow::Result<()>>>,
    /// Review cancellation and persistence must finish before closing the pool.
    review_task: JoinHandle<anyhow::Result<()>>,
}

impl Harness {
    /// Assemble HTTP, storage, review and controlled status observations.
    pub async fn start(workers: usize, capacity: u32) -> Self {
        Self::start_with_health(workers, capacity, HealthConfig::default()).await
    }

    /// Assemble the same services with a test-selected collection cadence and budget.
    pub async fn start_with_health(
        workers: usize,
        capacity: u32,
        health_config: HealthConfig,
    ) -> Self {
        let mut fixture = Fixture::new().await;
        fixture.config.health = health_config;
        fixture.config.scheduler.pending_max_count = capacity;
        Self::start_fixture(fixture, workers).await
    }

    /// Assemble production services using a fixture with explicitly selected review settings.
    pub async fn start_fixture(mut fixture: Fixture, workers: usize) -> Self {
        let mut servers = Vec::new();
        for _ in 0..workers {
            servers.push(RpcServer::start().await);
        }
        fixture.config.scheduler.workers = servers
            .iter()
            .map(|server| server.address.clone())
            .collect();
        let (review, review_task) =
            ReviewService::start(&fixture.config.llm_review, fixture.database.clone()).unwrap();
        let scheduler = Scheduler::restore(&fixture.config, Default::default()).unwrap();
        let membership = observed_membership(&fixture.config);
        let creation = CreationService::new(
            &fixture.config,
            fixture.database.clone(),
            scheduler.clone(),
            membership.clone(),
        );
        let (health, health_task) =
            Health::start(&fixture.config.health, fixture.database.clone(), &scheduler).unwrap();
        let app = http::router(
            &fixture.config,
            fixture.database.clone(),
            creation.clone(),
            scheduler.clone(),
            health.clone(),
            review.clone(),
        );
        Self {
            fixture,
            scheduler,
            creation,
            membership,
            health,
            review,
            app,
            servers,
            health_task: Some(health_task),
            review_task,
        }
    }

    /// Create a fixture task and deliver its controlled terminal through EventProcessor.
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
        self.terminal(&id).await;
        id
    }

    /// Deliver a controlled terminal through the production event processor and return storage.
    pub async fn terminal(&self, id: &str) -> Submission {
        let stored = self.fixture.database.submission(id).await.unwrap().unwrap();
        let context = self
            .fixture
            .database
            .judging_context(id)
            .await
            .unwrap()
            .unwrap();
        let reply = terminal_reply(id, &stored.source, context.total_cases);
        let encoded = acorn_protocol::records::encode_event(
            &reply,
            self.fixture.config.kafka.max_event_record_bytes,
        )
        .unwrap();
        acorn_backend::scheduler::kafka::EventProcessor::new(
            &self.fixture.config,
            self.scheduler.clone(),
            self.fixture.database.clone(),
            self.review.clone(),
        )
        .process(encoded.payload(), || Ok(()))
        .await
        .unwrap();
        self.fixture.database.submission(id).await.unwrap().unwrap()
    }

    /// Drain owned creation and review work before closing status transport and storage.
    pub async fn close(mut self) {
        self.creation.stop();
        self.scheduler.pause();
        self.review.stop();
        self.creation.wait_idle().await;
        (&mut self.review_task).await.unwrap().unwrap();
        self.stop_health().await;
        for server in self.servers {
            server.close().await;
        }
        self.fixture.close().await;
    }

    /// Stop and observe the collector before closing its shared database or endpoints.
    pub async fn stop_health(&mut self) {
        self.health.stop();
        if let Some(task) = self.health_task.take() {
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
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

/// Controlled terminal shapes for event and HTTP contract checks; no execution is simulated.
pub fn terminal_reply(id: &str, source: &[u8], total: u32) -> proto::GetJobReply {
    let mut reply = proto::GetJobReply {
        job_id: id.into(),
        state: proto::JobState::Finished as i32,
        phase: proto::JobPhase::Done as i32,
        total_cases: total,
        completed_cases: total,
        terminal: Some(proto::get_job_reply::Terminal::Result(proto::JudgeResult {
            verdict: proto::Verdict::Accepted as i32,
            ..Default::default()
        })),
    };
    if source == b"ce" {
        reply.completed_cases = 0;
        reply.terminal = Some(proto::get_job_reply::Terminal::Result(proto::JudgeResult {
            verdict: proto::Verdict::CompileError as i32,
            compile_diagnostic: vec![0, 255, 10],
            compile_diagnostic_truncated: true,
            ..Default::default()
        }));
    } else if source == b"wa" {
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
    reply
}
