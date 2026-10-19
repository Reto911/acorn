//! Automatic and manual review contracts over SQLite, HTTP, gRPC and backend lifecycle.

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
    llm_review::{
        ReviewAttempt, ReviewDecision, ReviewErrorCode, ReviewResult, ReviewStatus, ReviewTrigger,
    },
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
    BackendProcess, Fixture, Harness, Mock, Mode, RpcServer, call, network_call, wait_for,
};

/// Controlled request captures and an explicit response gate, with no model credentials.
struct Responses {
    /// Public model inputs in actual POST order.
    requests: Mutex<Vec<Value>>,
    /// Release all responses when the test allows completion or shuts down.
    release: watch::Sender<bool>,
    /// One representative API failure can be selected without network timing.
    status: AtomicU16,
    /// Complete model body selected by the test, including representative invalid output.
    output: Mutex<Value>,
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
    let output = state.output.lock().unwrap().to_string();
    (
        status,
        Json(json!({
            "id":"controlled-response", "status":"completed",
            "output":[{"type":"message", "role":"assistant", "content":[{
                "type":"output_text", "text":output
            }]}]
        })),
    )
        .into_response()
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
            output: Mutex::new(json!({"decision":"PASS", "summary":"未发现疑点", "findings":[]})),
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

/// Send a manual request and check the common persisted-detail Location on successful responses.
async fn manual(app: &Router, id: &str, expected: Option<&str>) -> (StatusCode, Value) {
    let (status, headers, body) = call(
        app,
        "POST",
        &format!("/api/v1/submissions/{id}/review"),
        Some(json!({"expected_review_id":expected})),
        None,
    )
    .await;
    if status.is_success() {
        assert_eq!(headers["location"], format!("/api/v1/submissions/{id}"));
    }
    (status, body)
}

/// One bounded finding with an inclusive single-line location.
fn doubtful_output() -> Value {
    json!({"decision":"DOUBTFUL", "summary":"需要人工复核", "findings":[{
        "category":"TEST_TARGETING", "line_start":1, "line_end":1,
        "evidence":"受控测试疑点", "normal_explanation":"受控测试正常解释"
    }]})
}

/// Manual CE and first-case WA bypass automatic gates; pending reuse works even when full.
#[tokio::test]
async fn manual_http_admission_pending_merge_and_queue_full() {
    let endpoint = Endpoint::start(true).await;
    *endpoint.state.output.lock().unwrap() = doubtful_output();
    let harness = Harness::start_fixture(endpoint.fixture("DISABLED").await, &[Mode::Finish]).await;
    let ce = harness.submit("c", "ce").await;
    let original = harness.terminal(&ce).await;
    let (_, _, initial) = call(
        &harness.app,
        "GET",
        &format!("/api/v1/submissions/{ce}"),
        None,
        None,
    )
    .await;
    assert_eq!(initial["review_status"], "DISABLED");
    assert!(initial.get("review").unwrap().is_null());
    let (status, first) = manual(&harness.app, &ce, None).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(first["review_status"], "PENDING");
    assert_eq!(first["review"]["trigger"], "manual");
    let review_id = first["review"]["review_id"].as_str().unwrap().to_owned();
    wait_for(|| endpoint.count() == 1).await;

    let wa = harness.submit("python", "wa").await;
    harness.terminal(&wa).await;
    assert_eq!(
        manual(&harness.app, &wa, None).await.0,
        StatusCode::ACCEPTED
    );
    let ac = harness.submit("python", "ac").await;
    let before_full = harness.terminal(&ac).await;
    let (status, full) = manual(&harness.app, &ac, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(full["error"]["code"], "ReviewQueueFull");
    assert_eq!(
        harness
            .fixture
            .database
            .submission(&ac)
            .await
            .unwrap()
            .unwrap(),
        before_full
    );

    let mut duplicates = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let app = harness.app.clone();
        let id = ce.clone();
        duplicates.spawn(async move { manual(&app, &id, Some(&Uuid::new_v4().to_string())).await });
    }
    while let Some(result) = duplicates.join_next().await {
        let (status, reused) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(reused["review"]["review_id"], review_id);
    }
    assert_eq!(endpoint.count(), 1);
    for invalid in [json!({}), json!({"expected_review_id":null,"user_id":1})] {
        assert_eq!(
            call(
                &harness.app,
                "POST",
                &format!("/api/v1/submissions/{ce}/review"),
                Some(invalid),
                None
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    endpoint.state.release.send_replace(true);
    review_state(&harness.fixture.database, &ce, ReviewStatus::Doubtful).await;
    review_state(&harness.fixture.database, &wa, ReviewStatus::Doubtful).await;
    let (status, _, detail) = call(
        &harness.app,
        "GET",
        &format!("/api/v1/submissions/{ce}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["verdict"], "CE");
    assert_eq!(detail["review"]["result"], doubtful_output());
    let after = harness
        .fixture
        .database
        .submission(&ce)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.terminal_details, original.terminal_details);
    assert_eq!(after.info.terminal_at_ms, original.info.terminal_at_ms);
    assert_eq!(endpoint.count(), 2);
    harness.close().await;
    endpoint.close().await;
}

/// Concurrent first requests commit one attempt and reuse its pending identity without extra POSTs.
#[tokio::test]
async fn concurrent_first_manual_requests_share_one_attempt() {
    let endpoint = Endpoint::start(true).await;
    let mut fixture = endpoint.fixture("DISABLED").await;
    fixture.config.llm_review.pending_max_count = 8;
    let harness = Harness::start_fixture(fixture, &[Mode::Finish]).await;
    let id = harness.submit("python", "ac").await;
    harness.terminal(&id).await;
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let app = harness.app.clone();
        let id = id.clone();
        requests.spawn(async move { manual(&app, &id, None).await });
    }
    let mut created = 0;
    let mut identities = std::collections::HashSet::new();
    while let Some(result) = requests.join_next().await {
        let (status, body) = result.unwrap();
        assert!(
            matches!(status, StatusCode::OK | StatusCode::ACCEPTED),
            "{body}"
        );
        created += usize::from(status == StatusCode::ACCEPTED);
        identities.insert(body["review"]["review_id"].as_str().unwrap().to_owned());
    }
    assert_eq!(created, 1);
    assert_eq!(identities.len(), 1);
    wait_for(|| endpoint.count() == 1).await;
    harness.close().await;
    assert_eq!(endpoint.count(), 1);
    endpoint.close().await;
}

/// Nonfinished, nonexistent, disabled and stopping cases reject without replacing review data.
#[tokio::test]
async fn manual_http_rejections_preserve_records() {
    let endpoint = Endpoint::start(false).await;
    let harness = Harness::start_fixture(endpoint.fixture("DISABLED").await, &[Mode::Finish]).await;
    let id = harness.fixture.queued("python").await;
    for state in ["queued", "dispatching", "running", "failed"] {
        match state {
            "dispatching" => {
                harness
                    .fixture
                    .database
                    .mark_dispatching(&id)
                    .await
                    .unwrap();
            }
            "running" => {
                harness.fixture.database.mark_running(&id).await.unwrap();
            }
            "failed" => {
                let failure = TerminalOutcome::failed(
                    &acorn_protocol::worker::v1::JobFailure {
                        code: "ControlledFailure".into(),
                        diagnostic: String::new(),
                    },
                    &harness.fixture.config.results,
                )
                .unwrap();
                harness
                    .fixture
                    .database
                    .save_terminal(&id, &failure, None)
                    .await
                    .unwrap();
            }
            _ => {}
        }
        let before = harness
            .fixture
            .database
            .submission(&id)
            .await
            .unwrap()
            .unwrap();
        let (status, body) = manual(&harness.app, &id, None).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "JudgingNotFinished");
        assert_eq!(
            harness
                .fixture
                .database
                .submission(&id)
                .await
                .unwrap()
                .unwrap(),
            before
        );
    }
    let (status, body) = manual(&harness.app, &Uuid::new_v4().to_string(), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "SubmissionNotFound");
    let finished = harness.submit("python", "ac").await;
    harness.terminal(&finished).await;
    let (status, body) = manual(&harness.app, &finished, Some(&Uuid::new_v4().to_string())).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "ReviewChanged");
    harness.review.stop();
    let (status, body) = manual(&harness.app, &finished, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"]["code"], "BackendStopping");
    assert_eq!(endpoint.count(), 0);
    harness.close().await;
    let harness = Harness::start(&[Mode::Finish], 1).await;
    let id = harness.submit("python", "ac").await;
    let before = harness.terminal(&id).await;
    let (status, body) = manual(&harness.app, &id, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"]["code"], "ReviewUnavailable");
    assert_eq!(
        harness
            .fixture
            .database
            .submission(&id)
            .await
            .unwrap()
            .unwrap(),
        before
    );
    harness.close().await;
    endpoint.close().await;
}

/// Reruns replace only the latest review, reject delayed snapshots and preserve old opinions on full.
#[tokio::test]
async fn manual_rerun_expected_identity_and_preservation() {
    let endpoint = Endpoint::start(false).await;
    let harness = Harness::start_fixture(endpoint.fixture("DISABLED").await, &[Mode::Finish]).await;
    let id = harness.submit("python", "ac").await;
    harness.terminal(&id).await;
    assert_eq!(
        manual(&harness.app, &id, None).await.0,
        StatusCode::ACCEPTED
    );
    let first = review_state(&harness.fixture.database, &id, ReviewStatus::Pass)
        .await
        .review
        .unwrap();
    assert_eq!(
        manual(&harness.app, &id, Some(&first.id)).await.0,
        StatusCode::ACCEPTED
    );
    let second = review_state(&harness.fixture.database, &id, ReviewStatus::Pass)
        .await
        .review
        .unwrap();
    assert_ne!(first.id, second.id);
    let (status, changed) = manual(&harness.app, &id, Some(&first.id)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(changed["error"]["code"], "ReviewChanged");
    assert_eq!(endpoint.count(), 2);
    endpoint.state.status.store(503, Ordering::SeqCst);
    assert_eq!(
        manual(&harness.app, &id, Some(&second.id)).await.0,
        StatusCode::ACCEPTED
    );
    let failed = review_state(&harness.fixture.database, &id, ReviewStatus::Failed)
        .await
        .review
        .unwrap();
    assert_eq!(
        failed.details.error.as_ref().unwrap().code,
        ReviewErrorCode::ReviewApiFailed
    );
    endpoint.state.status.store(200, Ordering::SeqCst);
    assert_eq!(
        manual(&harness.app, &id, Some(&failed.id)).await.0,
        StatusCode::ACCEPTED
    );
    let previous = review_state(&harness.fixture.database, &id, ReviewStatus::Pass).await;

    endpoint.state.release.send_replace(false);
    for (index, source) in ["held-model", "queued-model"].into_iter().enumerate() {
        let other = harness.submit("python", source).await;
        harness.terminal(&other).await;
        assert_eq!(
            manual(&harness.app, &other, None).await.0,
            StatusCode::ACCEPTED
        );
        if index == 0 {
            wait_for(|| endpoint.count() == 5).await;
        }
    }
    let (status, full) = manual(
        &harness.app,
        &id,
        Some(&previous.review.as_ref().unwrap().id),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(full["error"]["code"], "ReviewQueueFull");
    assert_eq!(
        harness
            .fixture
            .database
            .submission(&id)
            .await
            .unwrap()
            .unwrap(),
        previous
    );
    endpoint.state.release.send_replace(true);
    harness.close().await;
    endpoint.close().await;
}

/// A new attempt survives while its predecessor still owns a local execution record.
#[tokio::test]
async fn manual_rerun_survives_previous_attempt_cleanup() {
    let endpoint = Endpoint::start(true).await;
    let harness = Harness::start_fixture(endpoint.fixture("DISABLED").await, &[Mode::Finish]).await;
    let id = harness.submit("python", "ac").await;
    harness.terminal(&id).await;
    let (_, first) = manual(&harness.app, &id, None).await;
    wait_for(|| endpoint.count() == 1).await;
    let previous = harness
        .fixture
        .database
        .submission(&id)
        .await
        .unwrap()
        .unwrap()
        .review
        .unwrap();
    // Keep the old execution active while formal persistence exposes its completed state.
    let completed = previous
        .succeeded(
            2_000,
            ReviewResult {
                decision: ReviewDecision::Pass,
                summary: "受控完成".into(),
                findings: vec![],
            },
            None,
            None,
            1,
        )
        .unwrap();
    harness
        .fixture
        .database
        .finish_review(&id, &completed)
        .await
        .unwrap();
    let (status, next) = manual(
        &harness.app,
        &id,
        Some(first["review"]["review_id"].as_str().unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_ne!(next["review"]["review_id"], first["review"]["review_id"]);
    endpoint.state.release.send_replace(true);
    let saved = review_state(&harness.fixture.database, &id, ReviewStatus::Pass)
        .await
        .review
        .unwrap();
    assert_eq!(saved.id, next["review"]["review_id"].as_str().unwrap());
    assert_eq!(endpoint.count(), 2);
    harness.close().await;
    endpoint.close().await;
}

/// GET exposes only public review fields and combines review filters with the existing query.
#[tokio::test]
async fn review_query_projection_and_history_filters() {
    let harness = Harness::start(&[Mode::Finish], 8).await;
    for status in [
        ReviewStatus::Disabled,
        ReviewStatus::Pending,
        ReviewStatus::Pass,
        ReviewStatus::Doubtful,
        ReviewStatus::Failed,
    ] {
        let id = harness.submit("python", status.as_str()).await;
        harness.terminal(&id).await;
        if status != ReviewStatus::Disabled {
            let pending = ReviewAttempt::pending(
                ReviewTrigger::Manual,
                1_000,
                "test-model".into(),
                "max".into(),
                "private-policy".into(),
            )
            .unwrap();
            harness
                .fixture
                .database
                .start_review(&id, None, &pending)
                .await
                .unwrap();
            let completed = match status {
                ReviewStatus::Pass | ReviewStatus::Doubtful => Some(
                    pending
                        .succeeded(
                            2_000,
                            if status == ReviewStatus::Pass {
                                ReviewResult {
                                    decision: ReviewDecision::Pass,
                                    summary: "无疑点".into(),
                                    findings: vec![],
                                }
                            } else {
                                serde_json::from_value(doubtful_output()).unwrap()
                            },
                            Some("private-response-id".into()),
                            None,
                            1,
                        )
                        .unwrap(),
                ),
                ReviewStatus::Failed => Some(
                    pending
                        .failed(
                            2_000,
                            ReviewErrorCode::ReviewInvalidOutput,
                            Some("private-response-id".into()),
                            None,
                            Some(json!({"diagnostic":"private-diagnostic"})),
                        )
                        .unwrap(),
                ),
                _ => None,
            };
            if let Some(completed) = completed {
                harness
                    .fixture
                    .database
                    .finish_review(&id, &completed)
                    .await
                    .unwrap();
            }
        }
        let (code, _, detail) = call(
            &harness.app,
            "GET",
            &format!("/api/v1/submissions/{id}"),
            None,
            None,
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(detail["review_status"], status.as_str());
        if status == ReviewStatus::Disabled {
            assert!(detail.get("review").unwrap().is_null());
        } else {
            assert_eq!(detail["review"].as_object().unwrap().len(), 7);
            for private in [
                "prompt_version",
                "raw_output_json",
                "response_id",
                "usage",
                "endpoint",
                "api_key",
            ] {
                assert!(detail["review"].get(private).is_none());
            }
            assert!(!detail.to_string().contains("private-"));
        }
        let (code, _, history) = call(&harness.app, "GET", &format!("/api/v1/submissions?problem_id=1&state=finished&verdict=AC&review_status={}&limit=1&offset=0", status.as_str()), None, None).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(history["items"].as_array().unwrap().len(), 1);
        assert_eq!(history["items"][0]["submission_id"], id);
        assert_eq!(history["items"][0]["review_status"], status.as_str());
        assert!(history["items"][0].get("review").is_none());
    }
    let (_, _, empty) = call(
        &harness.app,
        "GET",
        "/api/v1/submissions?verdict=WA&review_status=PASS",
        None,
        None,
    )
    .await;
    assert!(empty["items"].as_array().unwrap().is_empty());
    assert_eq!(
        call(
            &harness.app,
            "GET",
            "/api/v1/submissions?review_status=INVALID",
            None,
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(harness.servers[0].mock.submits.load(Ordering::SeqCst), 5);
    harness.close().await;
}

/// Canceling an HTTP waiter while the write is locked does not cancel its registered admission.
#[tokio::test]
async fn disconnected_manual_http_finishes_admission() {
    use std::{future::Future, task::Poll};

    let endpoint = Endpoint::start(false).await;
    let harness = Harness::start_fixture(endpoint.fixture("DISABLED").await, &[Mode::Finish]).await;
    let id = harness.submit("python", "ac").await;
    harness.terminal(&id).await;
    let mut connection = harness.fixture.connection().await;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut connection)
        .await
        .unwrap();
    {
        let mut waiting = Box::pin(manual(&harness.app, &id, None));
        std::future::poll_fn(|context| match waiting.as_mut().poll(context) {
            Poll::Pending => Poll::Ready(()),
            Poll::Ready(_) => panic!("A locked manual admission unexpectedly completed"),
        })
        .await;
    }
    sqlx::query("COMMIT")
        .execute(&mut connection)
        .await
        .unwrap();
    review_state(&harness.fixture.database, &id, ReviewStatus::Pass).await;
    assert_eq!(endpoint.count(), 1);
    connection.close().await.unwrap();
    harness.close().await;
    endpoint.close().await;
}

/// Process shutdown waits for a registered manual write and interruption after its client leaves.
#[tokio::test]
async fn process_shutdown_drains_registered_manual_admission() {
    let endpoint = Endpoint::start(false).await;
    let mut fixture = endpoint.fixture("DISABLED").await;
    let id = fixture.queued("python").await;
    fixture.database.mark_dispatching(&id).await.unwrap();
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
        .save_terminal(&id, &terminal, None)
        .await
        .unwrap();
    let server = RpcServer::start(Mode::Finish, fixture.database.clone()).await;
    fixture.config.scheduler.workers = vec![server.address.clone()];
    let mut backend = BackendProcess::spawn(&fixture);
    let address = backend.listening().await;
    let mut connection = fixture.connection().await;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut connection)
        .await
        .unwrap();
    let target = id.clone();
    let waiting = tokio::spawn(async move {
        network_call(
            address,
            "POST",
            &format!("/api/v1/submissions/{target}/review"),
            Some(json!({"expected_review_id":null})),
            None,
        )
        .await
    });
    backend
        .until_log("Manual review admission registered")
        .await;
    waiting.abort();
    assert!(waiting.await.unwrap_err().is_cancelled());
    backend.terminate();
    backend
        .until_log("Backend stopping admission and dispatch")
        .await;
    assert_eq!(
        network_call(address, "GET", "/api/v1/health", None, None)
            .await
            .0,
        200
    );
    let (status, body) = network_call(
        address,
        "POST",
        &format!("/api/v1/submissions/{id}/review"),
        Some(json!({"expected_review_id":null})),
        None,
    )
    .await;
    assert_eq!(status, 503);
    assert_eq!(body["error"]["code"], "BackendStopping");
    sqlx::query("COMMIT")
        .execute(&mut connection)
        .await
        .unwrap();
    let (status, logs) = backend.finish().await;
    assert!(status.success(), "{logs}");
    let interrupted = review_state(&fixture.database, &id, ReviewStatus::Failed).await;
    assert_eq!(
        interrupted.review.unwrap().details.error.unwrap().code,
        ReviewErrorCode::ReviewInterrupted
    );
    assert_eq!(interrupted.info.state, SubmissionState::Finished);
    assert_eq!(endpoint.count(), 0);
    connection.close().await.unwrap();
    server.close().await;
    fixture.close().await;
    endpoint.close().await;
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
    fixture.close().await;
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
    fixture.close().await;
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
    fixture.close().await;
    endpoint.close().await;
}
