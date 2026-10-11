//! Real gRPC transport checks; controlled managers do not prove sandbox isolation.

mod support;

use std::{fs, time::Duration};

use acorn_protocol::worker::v1::{
    AcknowledgeResultRequest, GetJobReply, GetJobRequest, JobPhase, JobState, SubmitJobRequest,
    Verdict, get_job_reply::Terminal, judge_worker_client::JudgeWorkerClient,
};
use acorn_worker::{config::Config, rpc::WorkerService};
use support::{Fixture, job};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tonic::{
    Code, Request, Status,
    codegen::tokio_stream::wrappers::TcpListenerStream,
    transport::{Channel, Server},
};

/// Test server and client sharing an actual ephemeral loopback listener.
struct Harness {
    /// Service control for bounded shutdown and stop-path checks.
    service: WorkerService,
    /// Address used to establish independent connections.
    address: String,
    /// Generated Tonic client configured like the future Scheduler.
    client: JudgeWorkerClient<Channel>,
    /// Signal ending the transport after execution has stopped.
    stop: oneshot::Sender<()>,
    /// Transport task whose result must be observed.
    server: JoinHandle<Result<(), tonic::transport::Error>>,
}

impl Harness {
    /// Bind before starting the server so connection setup has no port-selection race.
    async fn start(config: Config) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let service = WorkerService::new(config);
        let server_service = service.clone().into_server();
        let (stop, stopped) = oneshot::channel();
        let server = tokio::spawn(
            Server::builder()
                .add_service(server_service)
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = stopped.await;
                }),
        );
        let client = JudgeWorkerClient::connect(address.clone()).await.unwrap();
        Self {
            service,
            address,
            client,
            stop,
            server,
        }
    }

    /// Await execution cleanup, stop transport, and observe both task results.
    async fn close(self) -> anyhow::Result<()> {
        let execution = self.service.shutdown().await;
        self.stop.send(()).unwrap();
        drop(self.client);
        tokio::time::timeout(Duration::from_secs(5), self.server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        execution
    }
}

/// Give each test RPC a finite deadline rather than waiting indefinitely on a failure.
fn request<T>(message: T) -> Request<T> {
    let mut request = Request::new(message);
    request.set_timeout(Duration::from_secs(5));
    request
}

/// Poll the bounded query RPC until the immutable terminal becomes visible.
async fn terminal(client: &mut JudgeWorkerClient<Channel>, id: &str) -> GetJobReply {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let reply = client
                .get_job(request(GetJobRequest { job_id: id.into() }))
                .await
                .unwrap()
                .into_inner();
            if reply.state != JobState::Running as i32 {
                assert_eq!(reply.phase, JobPhase::Done as i32);
                assert!(reply.terminal.is_some());
                return reply;
            }
            assert!(reply.terminal.is_none());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

/// Confirm a terminal through the generated client, preserving FIFO insertion order.
async fn ack(client: &mut JudgeWorkerClient<Channel>, id: &str) {
    client
        .acknowledge_result(request(AcknowledgeResultRequest { job_id: id.into() }))
        .await
        .unwrap();
}

/// Assert the stable gRPC status and reason together.
fn error(status: Status, code: Code, message: &str) {
    assert_eq!(status.code(), code);
    assert_eq!(status.message(), message);
}

/// A trusted fixture program signals its start and waits for test-owned release.
fn blocked_source(fixture: &Fixture) -> String {
    format!(
        "import pathlib,time\npathlib.Path({ready:?}).touch()\nwhile not pathlib.Path({release:?}).exists(): time.sleep(0.01)\nprint(42)\n",
        ready = fixture.directory.path().join("ready").to_str().unwrap(),
        release = fixture.directory.path().join("release").to_str().unwrap()
    )
}

/// Wait only for a deterministic fixture event, bounded by the test deadline.
async fn ready(fixture: &Fixture) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fixture.directory.path().join("ready").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

/// Concurrent same-ID calls execute once; busy rejection registers nothing and disconnect is harmless.
#[tokio::test]
async fn admission_deduplication_and_connection_independence() {
    let fixture = Fixture::new("normal");
    let mut harness = Harness::start(fixture.config.clone()).await;
    let submitted = job(
        "same-id",
        "python",
        &blocked_source(&fixture),
        &[(b"", b"42")],
    );
    let mut tasks = vec![];
    for _ in 0..8 {
        let mut client = harness.client.clone();
        let submitted = submitted.clone();
        tasks.push(tokio::spawn(async move {
            client
                .submit_job(request(submitted))
                .await
                .unwrap()
                .into_inner()
        }));
    }
    let mut accepted = 0;
    for task in tasks {
        let reply = task.await.unwrap();
        accepted += usize::from(!reply.duplicate);
        assert_eq!(reply.state, JobState::Running as i32);
    }
    assert_eq!(accepted, 1);
    ready(&fixture).await;
    let progress = harness
        .client
        .get_job(request(GetJobRequest {
            job_id: "same-id".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(progress.phase, JobPhase::RunningCase as i32);
    assert_eq!(progress.completed_cases, 0);
    assert!(
        harness
            .client
            .submit_job(request(SubmitJobRequest {
                job_id: "same-id".into(),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner()
            .duplicate
    );
    error(
        harness
            .client
            .submit_job(request(job("busy", "python", "print(42)", &[(b"", b"42")])))
            .await
            .unwrap_err(),
        Code::ResourceExhausted,
        "WorkerBusy",
    );
    error(
        harness
            .client
            .get_job(request(GetJobRequest {
                job_id: "busy".into(),
            }))
            .await
            .unwrap_err(),
        Code::NotFound,
        "JobNotFound",
    );
    error(
        harness
            .client
            .acknowledge_result(request(AcknowledgeResultRequest {
                job_id: "same-id".into(),
            }))
            .await
            .unwrap_err(),
        Code::FailedPrecondition,
        "ResultNotReady",
    );
    // Replacing the sole surviving client drops the submitting channel.
    harness.client = JudgeWorkerClient::connect(harness.address.clone())
        .await
        .unwrap();
    fs::write(fixture.directory.path().join("release"), "release").unwrap();
    let reply = terminal(&mut harness.client, "same-id").await;
    assert_eq!(reply.completed_cases, 1);
    let Some(Terminal::Result(result)) = reply.terminal else {
        panic!("Expected result")
    };
    assert_eq!(result.verdict, Verdict::Accepted as i32);
    assert_eq!(fixture.operations().matches("--run").count(), 1);
    ack(&mut harness.client, "same-id").await;
    harness.close().await.unwrap();
}

/// Queries retain unacknowledged results; repeated ACK does not reorder the bounded FIFO.
#[tokio::test]
async fn result_capacity_and_ack_fifo() {
    let mut fixture = Fixture::new("normal");
    fixture.config.results.max_unacknowledged_results = 2;
    fixture.config.results.acknowledged_queue_length = 2;
    let mut harness = Harness::start(fixture.config.clone()).await;
    for id in ["a", "b"] {
        harness
            .client
            .submit_job(request(job(id, "python", "print(42)", &[(b"", b"42")])))
            .await
            .unwrap();
        terminal(&mut harness.client, id).await;
    }
    let a = terminal(&mut harness.client, "a").await;
    assert_eq!(a, terminal(&mut harness.client, "a").await);
    assert!(
        harness
            .client
            .submit_job(request(SubmitJobRequest {
                job_id: "a".into(),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner()
            .duplicate
    );
    error(
        harness
            .client
            .submit_job(request(job("c", "python", "print(42)", &[(b"", b"42")])))
            .await
            .unwrap_err(),
        Code::ResourceExhausted,
        "ResultBufferFull",
    );
    error(
        harness
            .client
            .get_job(request(GetJobRequest { job_id: "c".into() }))
            .await
            .unwrap_err(),
        Code::NotFound,
        "JobNotFound",
    );
    ack(&mut harness.client, "a").await;
    harness
        .client
        .submit_job(request(job("c", "python", "print(42)", &[(b"", b"42")])))
        .await
        .unwrap();
    terminal(&mut harness.client, "c").await;
    ack(&mut harness.client, "b").await;
    ack(&mut harness.client, "a").await;
    ack(&mut harness.client, "c").await;
    error(
        harness
            .client
            .get_job(request(GetJobRequest { job_id: "a".into() }))
            .await
            .unwrap_err(),
        Code::NotFound,
        "JobNotFound",
    );
    terminal(&mut harness.client, "b").await;
    terminal(&mut harness.client, "c").await;
    error(
        harness
            .client
            .acknowledge_result(request(AcknowledgeResultRequest { job_id: "a".into() }))
            .await
            .unwrap_err(),
        Code::NotFound,
        "JobNotFound",
    );
    harness.close().await.unwrap();
}

/// An unconfirmed stop publishes failure while ACK and even result eviction leave the slot busy.
#[tokio::test]
async fn unconfirmed_stop_retains_slot_after_ack_and_eviction() {
    let mut fixture = Fixture::new("normal");
    fixture.config.results.acknowledged_queue_length = 1;
    let mut harness = Harness::start(fixture.config.clone()).await;
    harness
        .client
        .submit_job(request(job("old", "python", "print(42)", &[(b"", b"42")])))
        .await
        .unwrap();
    terminal(&mut harness.client, "old").await;
    fs::write(fixture.directory.path().join("mode"), "manager_failure").unwrap();
    harness
        .client
        .submit_job(request(job(
            "failed",
            "python",
            "print(42)",
            &[(b"", b"42")],
        )))
        .await
        .unwrap();
    let reply = terminal(&mut harness.client, "failed").await;
    assert_eq!(reply.state, JobState::Failed as i32);
    let Some(Terminal::Failure(failure)) = reply.terminal else {
        panic!("Expected execution failure")
    };
    assert_eq!(failure.code, "StopUnconfirmed");
    assert!(failure.diagnostic.len() <= 512);
    ack(&mut harness.client, "failed").await;
    ack(&mut harness.client, "failed").await;
    ack(&mut harness.client, "old").await;
    error(
        harness
            .client
            .get_job(request(GetJobRequest {
                job_id: "failed".into(),
            }))
            .await
            .unwrap_err(),
        Code::NotFound,
        "JobNotFound",
    );
    error(
        harness
            .client
            .submit_job(request(job("new", "python", "print(42)", &[(b"", b"42")])))
            .await
            .unwrap_err(),
        Code::ResourceExhausted,
        "WorkerBusy",
    );
    assert_eq!(fixture.operations().matches("--run").count(), 2);
    assert_eq!(fixture.operations().matches("--cleanup").count(), 1);
    assert!(
        harness
            .close()
            .await
            .unwrap_err()
            .to_string()
            .contains("unconfirmed")
    );
}

/// Normal shutdown stops execution through SIGTERM, preserves its cause, and rejects new IDs.
#[tokio::test]
async fn shutdown_preserves_failure_and_rejects_new_jobs() {
    let fixture = Fixture::new("normal");
    let mut harness = Harness::start(fixture.config.clone()).await;
    harness
        .client
        .submit_job(request(job(
            "shutdown",
            "python",
            &blocked_source(&fixture),
            &[(b"", b"42")],
        )))
        .await
        .unwrap();
    ready(&fixture).await;
    harness.service.shutdown().await.unwrap();
    let reply = terminal(&mut harness.client, "shutdown").await;
    let Some(Terminal::Failure(failure)) = reply.terminal else {
        panic!("Expected shutdown failure")
    };
    assert_eq!(failure.code, "WorkerShutdown");
    assert_eq!(fixture.operations().matches("--cleanup").count(), 1);
    assert!(
        harness
            .client
            .submit_job(request(SubmitJobRequest {
                job_id: "shutdown".into(),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner()
            .duplicate
    );
    error(
        harness
            .client
            .submit_job(request(job("new", "python", "print(42)", &[(b"", b"42")])))
            .await
            .unwrap_err(),
        Code::FailedPrecondition,
        "WorkerUnavailable",
    );
    ack(&mut harness.client, "shutdown").await;
    harness.close().await.unwrap();
}

/// Invalid requests fail before registration; empty input and answer remain valid.
#[tokio::test]
async fn validation_rejects_without_registering() {
    let mut fixture = Fixture::new("normal");
    fixture.config.request.payload_max_bytes = 32;
    let mut harness = Harness::start(fixture.config.clone()).await;
    let mut invalid_limits = job("limits", "python", "pass", &[(b"", b"")]);
    invalid_limits.run_limits.as_mut().unwrap().memory_kib = 0;
    for (invalid, reason) in [
        (
            job("language", "other", "pass", &[(b"", b"")]),
            "UnsupportedLanguage",
        ),
        (invalid_limits, "InvalidJob"),
        (
            job(
                "payload",
                "python",
                "pass",
                &[(b"12345678901234567890", b"12345678901234567890")],
            ),
            "InvalidJob",
        ),
        (job("", "python", "pass", &[(b"", b"")]), "InvalidJob"),
    ] {
        let id = invalid.job_id.clone();
        error(
            harness
                .client
                .submit_job(request(invalid))
                .await
                .unwrap_err(),
            Code::InvalidArgument,
            reason,
        );
        if !id.is_empty() {
            error(
                harness
                    .client
                    .get_job(request(GetJobRequest { job_id: id }))
                    .await
                    .unwrap_err(),
                Code::NotFound,
                "JobNotFound",
            );
        }
    }
    assert!(fixture.operations().is_empty());
    harness
        .client
        .submit_job(request(job("empty-data", "python", "pass", &[(b"", b"")])))
        .await
        .unwrap();
    let reply = terminal(&mut harness.client, "empty-data").await;
    let Some(Terminal::Result(result)) = reply.terminal else {
        panic!("Expected result")
    };
    assert_eq!(result.verdict, Verdict::Accepted as i32);
    harness.close().await.unwrap();
}

/// Submit, poll and ACK C/Python using real Isolate and an actual gRPC connection.
#[tokio::test]
#[ignore = "requires prepared Linux/Isolate and ACORN_ISOLATE_BOX_ID"]
async fn real_rpc_acceptance() {
    let mut config = Config::parse(include_str!("../../../config/worker.example.toml")).unwrap();
    config.sandbox.box_id = std::env::var("ACORN_ISOLATE_BOX_ID")
        .expect("Set a dedicated unused box ID")
        .parse()
        .unwrap();
    let mut harness = Harness::start(config).await;
    for (id, language, source) in [
        (
            "real-c",
            "c",
            "#include <stdio.h>\nint main(void){int a,b;scanf(\"%d%d\",&a,&b);printf(\"%d\\n\",a+b);}",
        ),
        (
            "real-python",
            "python",
            "a,b=map(int,input().split());print(a+b)",
        ),
    ] {
        harness
            .client
            .submit_job(request(job(
                id,
                language,
                source,
                &[(b"20 22\n", b"42"), (b"40 2\n", b"42")],
            )))
            .await
            .unwrap();
        let reply = terminal(&mut harness.client, id).await;
        assert_eq!(reply.state, JobState::Finished as i32, "{reply:?}");
        assert_eq!(reply.completed_cases, 2);
        let Some(Terminal::Result(result)) = reply.terminal else {
            panic!("Expected result")
        };
        assert_eq!(result.verdict, Verdict::Accepted as i32);
        assert!(result.failed_case.is_none());
        ack(&mut harness.client, id).await;
    }
    harness.close().await.unwrap();
}

/// The executable validates its config, serves RPCs, and stops an admitted Job on SIGTERM.
#[tokio::test]
async fn executable_serves_and_handles_sigterm() {
    let fixture = Fixture::new("normal");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let config = include_str!("../../../config/worker.example.toml")
        .replace("127.0.0.1:50051", &address.to_string())
        .replace(
            "\"/usr/bin/isolate\"",
            &format!("{:?}", fixture.config.sandbox.executable),
        );
    let config_path = fixture.directory.path().join("worker.toml");
    fs::write(&config_path, config).unwrap();
    let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_acorn-worker"))
        .arg("--config")
        .arg(config_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut client = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(client) = JudgeWorkerClient::connect(format!("http://{address}")).await {
                break client;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    client
        .submit_job(request(job(
            "signal",
            "python",
            &blocked_source(&fixture),
            &[(b"", b"42")],
        )))
        .await
        .unwrap();
    ready(&fixture).await;
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id().unwrap() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    drop(client);
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Worker stopped"));
    assert_eq!(fixture.operations().matches("--cleanup").count(), 1);
}
