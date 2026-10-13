//! Automatic review contracts over the real queue, SQLite, gRPC and backend lifecycle.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU16, Ordering},
    },
    time::Duration,
};

use acorn_backend::{
    config::LlmReviewConfig,
    database::Database,
    llm_review::{ReviewAttempt, ReviewErrorCode, ReviewStatus, ReviewTrigger},
    scheduler::Scheduler,
    submissions::{CreateSubmission, Submission, SubmissionState, TerminalOutcome},
};
use acorn_protocol::worker::v1::JudgeResult;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use sqlx::Connection;
use tokio::{
    net::TcpListener,
    sync::{oneshot, watch},
    task::JoinHandle,
};
use uuid::Uuid;

use super::support::{
    BackendProcess, Fixture, Harness, Mock, Mode, RpcServer, network_call, wait_for,
};

/// Controlled request captures and an explicit response gate, with no model credentials.
struct Responses {
    /// Public model inputs in actual POST order.
    requests: Mutex<Vec<Value>>,
    /// Release all responses when the test allows completion or shuts down.
    release: watch::Sender<bool>,
    /// One representative API failure can be selected without network timing.
    status: AtomicU16,
    /// When present, verify ACK completion and local release at the actual model POST.
    observer: Mutex<Option<(Scheduler, Arc<Mock>)>>,
}

/// Receive one Responses request, assert its release boundary and wait on the test gate.
async fn respond(State(state): State<Arc<Responses>>, Json(body): Json<Value>) -> Response {
    let input: Value = serde_json::from_str(body["input"][0]["content"].as_str().unwrap()).unwrap();
    if let Some((scheduler, worker)) = state.observer.lock().unwrap().as_ref() {
        let jobs = worker.submitted.lock().unwrap();
        let id = jobs
            .iter()
            .find(|(_, task)| task.source == input["source"].as_str().unwrap().as_bytes())
            .map(|(id, _)| id)
            .unwrap();
        assert!(
            !scheduler.contains(id),
            "Model POST preceded Scheduler release"
        );
        assert!(
            worker.acks.load(Ordering::SeqCst) > 0,
            "Model POST preceded ACK"
        );
    }
    state.requests.lock().unwrap().push(input);
    let mut release = state.release.subscribe();
    if !*release.borrow_and_update() {
        let _ = release.changed().await;
    }
    let status = StatusCode::from_u16(state.status.load(Ordering::SeqCst)).unwrap();
    (status, Json(json!({
        "id":"controlled-response", "status":"completed",
        "output":[{"type":"message", "role":"assistant", "content":[{
            "type":"output_text", "text":"{\"decision\":\"PASS\",\"summary\":\"未发现疑点\",\"findings\":[]}"
        }]}]
    }))).into_response()
}

/// Ephemeral local HTTP service using the production client's actual Responses transport.
struct Endpoint {
    /// Shared captures and response control.
    state: Arc<Responses>,
    /// Enabled production settings with a dummy key and long cancellation budget.
    config: LlmReviewConfig,
    /// Listener shutdown after releasing all held responses.
    stop: oneshot::Sender<()>,
    /// Observe listener completion at cleanup.
    task: JoinHandle<()>,
}

impl Endpoint {
    /// Start a held or immediately completing endpoint on a random loopback port.
    async fn start(held: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release, _) = watch::channel(!held);
        let state = Arc::new(Responses {
            requests: Mutex::new(Vec::new()),
            release,
            status: AtomicU16::new(200),
            observer: Mutex::new(None),
        });
        let app = Router::new()
            .route("/responses", post(respond))
            .with_state(state.clone());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        Self {
            state,
            config: LlmReviewConfig {
                enabled: true,
                endpoint: format!("http://{address}/responses"),
                api_key: "controlled-review-key".into(),
                max_concurrent: 1,
                pending_max_count: 1,
                timeout_ms: 60_000,
                ..Default::default()
            },
            stop,
            task,
        }
    }

    /// Initialize an immutable fixture version with the chosen automatic policy.
    async fn fixture(&self, mode: &str) -> Fixture {
        let mut fixture = Fixture::new().await;
        fixture.config.llm_review = self.config.clone();
        fixture.config.scheduler.poll_interval_ms = 10;
        let mut connection = fixture.connection().await;
        sqlx::query(
            "UPDATE problem_versions SET review_mode = ? WHERE problem_id = 1 AND version_no = 1",
        )
        .bind(mode)
        .execute(&mut connection)
        .await
        .unwrap();
        connection.close().await.unwrap();
        fixture
    }

    /// Return the exact number of observed model POSTs.
    fn count(&self) -> usize {
        self.state.requests.lock().unwrap().len()
    }

    /// Release held responses and wait for listener shutdown.
    async fn close(self) {
        self.state.release.send_replace(true);
        let _ = self.stop.send(());
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap();
    }
}

/// Wait for one persisted review state while also exercising the shared pool during HTTP waits.
async fn review_state(database: &Database, id: &str, expected: ReviewStatus) -> Submission {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let submission = database.submission(id).await.unwrap().unwrap();
            if submission.info.review_status == expected {
                return submission;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

/// Queue and execution limits are independent of Worker dispatch; duplicate terminals do not POST.
#[tokio::test]
async fn automatic_fifo_capacity_and_worker_release() {
    let endpoint = Endpoint::start(true).await;
    let mut fixture = endpoint.fixture("ENABLE-ANY").await;
    fixture.config.scheduler.pending_max_count = 1;
    let harness = Harness::start_fixture(fixture, &[Mode::Finish]).await;
    *endpoint.state.observer.lock().unwrap() =
        Some((harness.scheduler.clone(), harness.servers[0].mock.clone()));
    let created = harness
        .fixture
        .database
        .create_submission(CreateSubmission {
            problem_id: 1,
            language_id: "python".into(),
            source: b"first".to_vec(),
            request_key: Uuid::new_v4(),
        })
        .await
        .unwrap();
    let first = created.info.id;
    let task = created.task.unwrap();
    harness
        .scheduler
        .enqueue(task.clone(), harness.scheduler.reserve().unwrap())
        .unwrap();
    wait_for(|| endpoint.count() == 1).await;
    assert_eq!(
        harness.terminal(&first).await.info.review_status,
        ReviewStatus::Pending
    );
    harness
        .scheduler
        .enqueue(task, harness.scheduler.reserve().unwrap())
        .unwrap();
    wait_for(|| !harness.scheduler.contains(&first)).await;
    let second = harness.submit("python", "second").await;
    assert_eq!(
        harness.terminal(&second).await.info.review_status,
        ReviewStatus::Pending
    );
    let third = harness.submit("python", "third").await;
    let full = harness.terminal(&third).await;
    assert_eq!(
        full.review.unwrap().details.error.unwrap().code,
        ReviewErrorCode::ReviewQueueFull
    );
    assert_eq!(endpoint.count(), 1);
    assert_eq!(harness.servers[0].mock.acks.load(Ordering::SeqCst), 3);
    endpoint.state.release.send_replace(true);
    review_state(&harness.fixture.database, &first, ReviewStatus::Pass).await;
    review_state(&harness.fixture.database, &second, ReviewStatus::Pass).await;
    let sources: Vec<_> = endpoint
        .state
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|input| input["source"].clone())
        .collect();
    assert_eq!(sources, [json!("first"), json!("second")]);
    harness.review.stop();
    assert_eq!(
        harness
            .fixture
            .database
            .submission(&first)
            .await
            .unwrap()
            .unwrap()
            .info
            .review_status,
        ReviewStatus::Pass
    );
    harness.close().await;
    endpoint.close().await;
}

/// A committed PENDING remains intact until ACK finishes, then stop prevents the model handoff.
#[tokio::test]
async fn stop_between_terminal_commit_and_ack_completion() {
    let endpoint = Endpoint::start(false).await;
    let mut fixture = endpoint.fixture("ENABLE-ALL").await;
    fixture.config.grpc.ack_timeout_ms = 60_000;
    let harness = Harness::start_fixture(fixture, &[Mode::Finish]).await;
    let worker = &harness.servers[0].mock;
    worker.ack_released.store(false, Ordering::SeqCst);
    let id = harness.submit("python", "ac").await;
    wait_for(|| worker.acks.load(Ordering::SeqCst) == 1).await;
    let pending = review_state(&harness.fixture.database, &id, ReviewStatus::Pending)
        .await
        .review
        .unwrap();
    harness.review.stop();
    assert_eq!(
        harness
            .fixture
            .database
            .submission(&id)
            .await
            .unwrap()
            .unwrap()
            .info
            .review_status,
        ReviewStatus::Pending
    );
    assert!(harness.scheduler.contains(&id));
    worker.ack_released.store(true, Ordering::SeqCst);
    let interrupted = review_state(&harness.fixture.database, &id, ReviewStatus::Failed).await;
    let review = interrupted.review.unwrap();
    assert_eq!(review.id, pending.id);
    assert_eq!(
        review.details.error.unwrap().code,
        ReviewErrorCode::ReviewInterrupted
    );
    assert_eq!(interrupted.info.state, SubmissionState::Finished);
    assert_eq!(endpoint.count(), 0);
    harness.close().await;
    endpoint.close().await;
}

/// A terminal already saved by another path releases the unused automatic reservation.
#[tokio::test]
async fn stale_machine_terminal_releases_review_capacity() {
    let endpoint = Endpoint::start(false).await;
    let harness =
        Harness::start_fixture(endpoint.fixture("ENABLE-ANY").await, &[Mode::Running]).await;
    let id = harness.submit("python", "stale-terminal").await;
    wait_for(|| harness.servers[0].mock.gets.load(Ordering::SeqCst) > 0).await;
    let terminal = TerminalOutcome::judged(
        &JudgeResult {
            verdict: acorn_protocol::worker::v1::Verdict::Accepted as i32,
            ..Default::default()
        },
        4,
        &harness.fixture.config.results,
    )
    .unwrap();
    assert!(
        harness
            .fixture
            .database
            .save_terminal(&id, &terminal, None)
            .await
            .unwrap()
    );
    harness.servers[0]
        .mock
        .released
        .store(true, Ordering::SeqCst);
    assert_eq!(
        harness.terminal(&id).await.info.review_status,
        ReviewStatus::Disabled
    );
    assert_eq!(endpoint.count(), 0);
    let next = harness.submit("python", "next-terminal").await;
    review_state(&harness.fixture.database, &next, ReviewStatus::Pass).await;
    assert_eq!(endpoint.count(), 1);
    harness.close().await;
    endpoint.close().await;
}

/// Disabled review and failed ACK are independent of judging persistence and review execution.
#[tokio::test]
async fn unavailable_review_and_ack_failure_keep_machine_results() {
    let endpoint = Endpoint::start(false).await;
    let mut fixture = endpoint.fixture("ENABLE-ANY").await;
    fixture.config.llm_review.enabled = false;
    let harness = Harness::start_fixture(fixture, &[Mode::Finish]).await;
    let id = harness.submit("python", "ac").await;
    let terminal = harness.terminal(&id).await;
    assert_eq!(
        terminal.review.unwrap().details.error.unwrap().code,
        ReviewErrorCode::ReviewUnavailable
    );
    assert_eq!(terminal.info.state, SubmissionState::Finished);
    assert!(terminal.terminal_details.is_none());
    assert_eq!(endpoint.count(), 0);
    harness.close().await;

    let harness =
        Harness::start_fixture(endpoint.fixture("ENABLE-ALL").await, &[Mode::AckFailure]).await;
    *endpoint.state.observer.lock().unwrap() =
        Some((harness.scheduler.clone(), harness.servers[0].mock.clone()));
    let id = harness.submit("python", "ac").await;
    let terminal = review_state(&harness.fixture.database, &id, ReviewStatus::Pass).await;
    assert_eq!(terminal.info.state, SubmissionState::Finished);
    assert_eq!(harness.servers[0].mock.acks.load(Ordering::SeqCst), 1);
    assert_eq!(endpoint.count(), 1);
    harness.close().await;
    endpoint.close().await;
}

/// Startup interrupts old reviews and uses the queued submission's original version policy.
#[tokio::test]
async fn process_review_startup_interrupts_without_reposting() {
    let endpoint = Endpoint::start(false).await;
    let mut fixture = endpoint.fixture("ENABLE-ALL").await;
    let old = fixture.queued("python").await;
    fixture.database.mark_dispatching(&old).await.unwrap();
    let terminal = TerminalOutcome::judged(
        &JudgeResult {
            verdict: acorn_protocol::worker::v1::Verdict::Accepted as i32,
            ..Default::default()
        },
        4,
        &fixture.config.results,
    )
    .unwrap();
    fixture
        .database
        .save_terminal(&old, &terminal, None)
        .await
        .unwrap();
    let pending = ReviewAttempt::pending(
        ReviewTrigger::Manual,
        1_000,
        "deepseek-flash".into(),
        "max".into(),
        "review-policy-v1".into(),
    )
    .unwrap();
    fixture
        .database
        .start_review(&old, None, &pending)
        .await
        .unwrap();
    let queued = fixture.queued("python").await;
    fixture.publish_next().await;
    let server = RpcServer::start(Mode::Finish, fixture.database.clone()).await;
    fixture.config.scheduler.workers = vec![server.address.clone()];
    let mut backend = BackendProcess::spawn(&fixture);
    backend.listening().await;
    let old_review = review_state(&fixture.database, &old, ReviewStatus::Failed)
        .await
        .review
        .unwrap();
    assert_eq!(old_review.id, pending.id);
    assert_eq!(
        old_review.details.requested_at_ms,
        pending.details.requested_at_ms
    );
    assert_eq!(
        old_review.details.error.unwrap().code,
        ReviewErrorCode::ReviewInterrupted
    );
    review_state(&fixture.database, &queued, ReviewStatus::Pass).await;
    assert_eq!(endpoint.count(), 1);
    backend.terminate();
    assert!(backend.finish().await.0.success());
    let mut restarted = BackendProcess::spawn(&fixture);
    restarted.listening().await;
    restarted.terminate();
    assert!(restarted.finish().await.0.success());
    assert_eq!(endpoint.count(), 1);
    server.close().await;
    fixture.database.close().await;
    endpoint.close().await;
}

/// Normal process shutdown interrupts queued and running requests without waiting the HTTP budget.
#[tokio::test]
async fn process_review_shutdown_cancels_and_marks_new_terminals() {
    let endpoint = Endpoint::start(true).await;
    let mut fixture = endpoint.fixture("ENABLE-ANY").await;
    let server = RpcServer::start(Mode::Running, fixture.database.clone()).await;
    fixture.config.scheduler.workers = vec![server.address.clone()];
    server.mock.released.store(true, Ordering::SeqCst);
    let mut backend = BackendProcess::spawn(&fixture);
    let address = backend.listening().await;
    let mut ids = Vec::new();
    for source in ["running-review", "queued-review", "late-machine-terminal"] {
        if ids.len() == 2 {
            server.mock.released.store(false, Ordering::SeqCst);
        }
        let (status, body) = network_call(
            address,
            "POST",
            "/api/v1/submissions",
            Some(json!({
                "problem_id":1, "language_id":"python", "source":source
            })),
            Some(Uuid::new_v4()),
        )
        .await;
        assert_eq!(status, 202);
        let id = body["submission_id"].as_str().unwrap().to_owned();
        if ids.len() < 2 {
            review_state(&fixture.database, &id, ReviewStatus::Pending).await;
            wait_for(|| server.mock.acks.load(Ordering::SeqCst) == ids.len() + 1).await;
            if ids.is_empty() {
                wait_for(|| endpoint.count() == 1).await;
            }
        }
        ids.push(id);
    }
    wait_for(|| server.mock.submits.load(Ordering::SeqCst) == 3).await;
    backend.terminate();
    backend
        .until_log("Backend stopping admission and dispatch")
        .await;
    for id in &ids[..2] {
        let review = review_state(&fixture.database, id, ReviewStatus::Failed)
            .await
            .review
            .unwrap();
        assert_eq!(
            review.details.error.unwrap().code,
            ReviewErrorCode::ReviewInterrupted
        );
    }
    assert_eq!(
        network_call(address, "GET", "/api/v1/health", None, None)
            .await
            .0,
        200
    );
    server.mock.released.store(true, Ordering::SeqCst);
    let (status, logs) = backend.finish().await;
    assert!(status.success(), "{logs}");
    assert!(logs.find("Review service stopped").unwrap() < logs.find("Backend stopped").unwrap());
    for id in &ids {
        let terminal = review_state(&fixture.database, id, ReviewStatus::Failed).await;
        assert_eq!(terminal.info.state, SubmissionState::Finished);
        assert!(terminal.terminal_details.is_none());
        assert_eq!(
            terminal.review.unwrap().details.error.unwrap().code,
            ReviewErrorCode::ReviewInterrupted
        );
    }
    assert_eq!(server.mock.acks.load(Ordering::SeqCst), 3);
    assert_eq!(endpoint.count(), 1);
    server.close().await;
    fixture.database.close().await;
    endpoint.close().await;
}

/// Ordinary API failures end one attempt; a critical completion write triggers process failure.
#[tokio::test]
async fn review_api_failure_and_critical_completion_failure() {
    let endpoint = Endpoint::start(false).await;
    endpoint.state.status.store(503, Ordering::SeqCst);
    let harness =
        Harness::start_fixture(endpoint.fixture("ENABLE-ALL").await, &[Mode::Finish]).await;
    let id = harness.submit("python", "api-failure").await;
    let failed = review_state(&harness.fixture.database, &id, ReviewStatus::Failed).await;
    assert_eq!(
        failed.review.unwrap().details.error.unwrap().code,
        ReviewErrorCode::ReviewApiFailed
    );
    endpoint.state.status.store(200, Ordering::SeqCst);
    let id = harness.submit("python", "next-review").await;
    review_state(&harness.fixture.database, &id, ReviewStatus::Pass).await;
    assert_eq!(endpoint.count(), 2);
    harness.close().await;

    endpoint.state.release.send_replace(false);
    let mut fixture = endpoint.fixture("ENABLE-ALL").await;
    let server = RpcServer::start(Mode::Finish, fixture.database.clone()).await;
    fixture.config.scheduler.workers = vec![server.address.clone()];
    let mut backend = BackendProcess::spawn(&fixture);
    let address = backend.listening().await;
    let (status, body) = network_call(
        address,
        "POST",
        "/api/v1/submissions",
        Some(json!({
            "problem_id":1, "language_id":"python", "source":"write-failure"
        })),
        Some(Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, 202);
    let id = body["submission_id"].as_str().unwrap();
    wait_for(|| endpoint.count() == 3).await;
    let mut connection = fixture.connection().await;
    sqlx::raw_sql("CREATE TRIGGER fail_review_completion BEFORE UPDATE OF review_status ON submissions WHEN OLD.review_status = 'PENDING' BEGIN SELECT RAISE(ABORT, 'Controlled review completion failure'); END;")
        .execute(&mut connection).await.unwrap();
    endpoint.state.release.send_replace(true);
    let (status, logs) = backend.finish().await;
    assert!(!status.success());
    assert!(logs.contains("Backend fault"), "{logs}");
    let stored = fixture.database.submission(id).await.unwrap().unwrap();
    assert_eq!(stored.info.state, SubmissionState::Finished);
    assert_eq!(stored.info.review_status, ReviewStatus::Pending);
    assert_eq!(server.mock.acks.load(Ordering::SeqCst), 1);
    assert_eq!(endpoint.count(), 3);
    connection.close().await.unwrap();
    server.close().await;
    fixture.database.close().await;
    endpoint.close().await;
}
