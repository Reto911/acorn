//! Contracts over SQLite, backend processes, HTTP and controlled or real Workers.

#[path = "support/review_workflow.rs"]
mod review_workflow;
mod support;

use std::{sync::atomic::Ordering, time::Duration};

use acorn_backend::{
    config::HealthConfig,
    database::Database,
    health::{Health, ProbeErrorCode, ProbeState, SubmissionAdmission, local_snapshot},
    http,
    llm_review::ReviewService,
    scheduler::AdmissionError,
    scheduler::{Scheduler, SchedulerState},
    submissions::{
        CreateSubmission, CreationService, SubmissionState, TerminalDetails, TerminalOutcome,
        Verdict,
    },
};
use acorn_protocol::worker::v1::{
    GetWorkerStatusReply, JudgeResult, Verdict as RpcVerdict, WorkerExecutionState,
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::Connection;
use support::{
    BackendProcess, Fixture, Harness, Mode, RpcServer, call, create, network_call, wait_for,
};
use tower::ServiceExt;
use uuid::Uuid;

/// Check the health HTTP envelope and caching header while returning its component fields.
async fn health_response(app: &axum::Router) -> Value {
    let (status, headers, body) = call(app, "GET", "/api/v1/health", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(headers["cache-control"], "no-store");
    assert!(body["snapshot_at_ms"].as_i64().unwrap() > 0);
    body
}

/// Basic public query shapes exclude private tests and support retained old versions.
#[tokio::test]
async fn published_queries_and_history_shapes() {
    let harness = Harness::start(&[Mode::Finish], 8).await;
    assert!(harness.fixture.directory.path().exists());
    let (status, _, list) = call(&harness.app, "GET", "/api/v1/problems", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["limit"], 20);
    assert_eq!(list["offset"], 0);
    assert_eq!(list["items"][0]["problem_id"], 1);
    assert!(list["items"][0].get("statement").is_none());
    let (_, _, detail) = call(&harness.app, "GET", "/api/v1/problems/1", None, None).await;
    assert_eq!(detail["languages"][0]["memory_kib"], 262144);
    assert_eq!(detail["review_mode"], "DISABLED");
    assert!(detail.get("cases").is_none());
    harness.fixture.publish_next().await;
    let (_, _, latest) = call(&harness.app, "GET", "/api/v1/problems/1", None, None).await;
    let (_, _, old) = call(
        &harness.app,
        "GET",
        "/api/v1/problems/1/versions/1",
        None,
        None,
    )
    .await;
    assert_eq!(latest["version_no"], 2);
    assert_eq!(old["version_no"], 1);
    assert_eq!(latest["review_mode"], "DISABLED");
    assert_eq!(old["review_mode"], "DISABLED");
    let id = create(&harness.app, "python", "ac").await;
    harness.terminal(&id).await;
    let (_, _, history) = call(
        &harness.app,
        "GET",
        "/api/v1/submissions?problem_id=1&state=finished&verdict=AC&limit=1",
        None,
        None,
    )
    .await;
    assert_eq!(history["items"][0]["submission_id"], id);
    assert_eq!(history["items"][0]["version_no"], 2);
    assert_eq!(history["items"][0]["review_status"], "DISABLED");
    assert!(history["items"][0].get("source").is_none());
    assert!(history["items"][0].get("terminal_details").is_none());
    harness.close().await;
}

/// All major terminal HTTP forms preserve binary data, real zero and missing statistics.
#[tokio::test]
async fn terminal_details_and_single_ack() {
    let harness = Harness::start(&[Mode::AckFailure], 8).await;
    for (source, verdict) in [("ac", "AC"), ("ce", "CE"), ("wa", "WA")] {
        let id = create(&harness.app, "c", source).await;
        harness.terminal(&id).await;
        let (status, _, body) = call(
            &harness.app,
            "GET",
            &format!("/api/v1/submissions/{id}"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["state"], "finished");
        assert_eq!(body["verdict"], verdict);
        assert!(body["terminal_at_ms"].is_i64());
        match source {
            "ac" => assert!(body["terminal_details"].is_null()),
            "ce" => assert_eq!(
                body["terminal_details"]["compile_diagnostic_base64"],
                "AP8K"
            ),
            _ => {
                let case = &body["terminal_details"]["failed_case"];
                assert_eq!(case["stdout_base64"], "AP8=");
                assert_eq!(case["resources"]["cpu_time_us"], 0);
                assert!(case["resources"].get("wall_time_us").is_none());
                assert_eq!(case["exit_code"], 0);
                assert!(case.get("signal").is_none());
            }
        }
    }
    assert_eq!(harness.servers[0].mock.acks.load(Ordering::SeqCst), 3);
    harness.close().await;
}

/// Refusal retains the task's one permit and immediately selects another endpoint.
#[tokio::test]
async fn busy_immediately_redispatches_and_merges_handoffs() {
    let harness = Harness::start(&[Mode::Busy, Mode::Finish], 1).await;
    let created = harness
        .fixture
        .database
        .create_submission(CreateSubmission {
            problem_id: 1,
            language_id: "python".into(),
            source: b"ac".to_vec(),
            request_key: Uuid::new_v4(),
        })
        .await
        .unwrap();
    let task = created.task.unwrap();
    let id = task.request.job_id.clone();
    let permit = harness.scheduler.reserve().unwrap();
    assert!(harness.scheduler.enqueue(task.clone(), permit).unwrap());
    assert!(matches!(
        harness.scheduler.reserve(),
        Err(AdmissionError::QueueFull)
    ));
    let terminal = harness.terminal(&id).await;
    assert_eq!(terminal.info.verdict, Some(Verdict::AC));
    assert_eq!(harness.servers[0].mock.submits.load(Ordering::SeqCst), 1);
    assert_eq!(harness.servers[1].mock.submits.load(Ordering::SeqCst), 1);
    let permit = harness.scheduler.reserve().unwrap();
    harness.scheduler.enqueue(task, permit).unwrap();
    wait_for(|| !harness.scheduler.contains(&id)).await;
    assert_eq!(harness.servers[1].mock.submits.load(Ordering::SeqCst), 1);
    harness.close().await;
}

/// Two occupied endpoints continue polling independently and release admission capacity.
#[tokio::test]
async fn two_workers_track_different_submissions_concurrently() {
    let harness = Harness::start(&[Mode::Running, Mode::Running], 2).await;
    let first = harness.submit("c", "ac").await;
    let second = harness.submit("python", "ac").await;
    wait_for(|| {
        harness
            .servers
            .iter()
            .all(|server| server.mock.gets.load(Ordering::SeqCst) > 0)
    })
    .await;
    for id in [&first, &second] {
        assert_eq!(
            harness
                .fixture
                .database
                .submission(id)
                .await
                .unwrap()
                .unwrap()
                .info
                .state,
            SubmissionState::Running
        );
    }
    let permit = harness.scheduler.reserve().unwrap();
    drop(permit);
    for server in &harness.servers {
        server.mock.released.store(true, Ordering::SeqCst);
    }
    harness.terminal(&first).await;
    harness.terminal(&second).await;
    assert_eq!(
        harness
            .servers
            .iter()
            .map(|server| server.mock.submits.load(Ordering::SeqCst))
            .sum::<usize>(),
        2
    );
    harness.close().await;
}

/// Admission ambiguity queries once; a known record continues without a second submit.
#[tokio::test]
async fn ambiguous_submit_queries_once_and_never_resubmits() {
    for mode in [
        Mode::AmbiguousFound,
        Mode::AmbiguousMissing,
        Mode::PollMissing,
        Mode::Malformed,
        Mode::Invalid,
    ] {
        let harness = Harness::start(&[mode, Mode::Finish], 4).await;
        let id = harness.submit("python", "ac").await;
        let terminal = harness.terminal(&id).await;
        let mock = &harness.servers[0].mock;
        assert_eq!(mock.submits.load(Ordering::SeqCst), 1);
        assert_eq!(harness.servers[1].mock.submits.load(Ordering::SeqCst), 0);
        assert_eq!(
            mock.gets.load(Ordering::SeqCst),
            usize::from(mode != Mode::Invalid)
        );
        if mode == Mode::AmbiguousFound {
            assert_eq!(terminal.info.verdict, Some(Verdict::AC));
            assert_eq!(mock.acks.load(Ordering::SeqCst), 1);
        } else {
            assert_eq!(terminal.info.state, SubmissionState::Failed);
            assert_eq!(mock.acks.load(Ordering::SeqCst), 0);
            let expected = match mode {
                Mode::Malformed => "InvalidWorkerResponse",
                Mode::Invalid => "InvalidJob",
                _ => "WorkerJobNotFound",
            };
            let Some(TerminalDetails::Failure(details)) = terminal.terminal_details else {
                panic!("Expected persisted execution failure");
            };
            assert_eq!(details.code, expected);
            assert!(terminal.info.verdict.is_none());
        }
        harness.close().await;
    }
}

/// A pending submit owns capacity until explicit admission, without delaying local handoff.
#[tokio::test]
async fn awaiting_admission_counts_toward_capacity() {
    let harness = Harness::start(&[Mode::AwaitAdmission], 1).await;
    assert_eq!(
        local_snapshot(&harness.creation, &harness.scheduler).submission_admission,
        SubmissionAdmission::Open
    );
    let permit = harness.scheduler.reserve().unwrap();
    let reserved = local_snapshot(&harness.creation, &harness.scheduler);
    assert_eq!(reserved.scheduler.pending_used, 1);
    assert_eq!(reserved.scheduler.pending_capacity, 1);
    assert_eq!(reserved.submission_admission, SubmissionAdmission::Full);
    let body = health_response(&harness.app).await;
    assert_eq!(body["submission_admission"], "full");
    assert_eq!(
        body["scheduler"],
        json!({"state":"running", "pending_used":1, "pending_capacity":1})
    );
    drop(permit);
    let id = harness.submit("python", "ac").await;
    wait_for(|| harness.servers[0].mock.submits.load(Ordering::SeqCst) == 1).await;
    assert!(matches!(
        harness.scheduler.reserve(),
        Err(AdmissionError::QueueFull)
    ));
    assert_eq!(
        local_snapshot(&harness.creation, &harness.scheduler).submission_admission,
        SubmissionAdmission::Full
    );
    assert_eq!(
        health_response(&harness.app).await["scheduler"]["pending_used"],
        1
    );
    harness.servers[0]
        .mock
        .released
        .store(true, Ordering::SeqCst);
    harness.terminal(&id).await;
    assert!(harness.scheduler.reserve().is_ok());
    assert_eq!(harness.scheduler.snapshot().pending_used, 0);
    harness.creation.stop();
    assert!(harness.creation.is_stopping());
    assert_eq!(harness.scheduler.snapshot().state, SchedulerState::Running);
    assert_eq!(
        local_snapshot(&harness.creation, &harness.scheduler).submission_admission,
        SubmissionAdmission::Stopping
    );
    assert_eq!(
        health_response(&harness.app).await["submission_admission"],
        "stopping"
    );
    harness.scheduler.pause();
    assert_eq!(harness.scheduler.snapshot().state, SchedulerState::Stopping);
    let creation = harness.creation.clone();
    let scheduler = harness.scheduler.clone();
    let app = harness.app.clone();
    harness.close().await;
    assert_eq!(scheduler.snapshot().state, SchedulerState::Stopped);
    assert_eq!(
        local_snapshot(&creation, &scheduler).submission_admission,
        SubmissionAdmission::Unavailable
    );
    let body = health_response(&app).await;
    assert_eq!(body["submission_admission"], "unavailable");
    assert_eq!(body["scheduler"]["state"], "stopped");
}

/// Fast checks update independently, snapshots are passive, and stop cancels a pending round.
#[tokio::test]
async fn health_round_updates_independently_and_stops_pending_checks() {
    let config = HealthConfig {
        interval_ms: 60_000,
        timeout_ms: 60_000,
    };
    let mut harness = Harness::start_with_health(&[Mode::Finish, Mode::Finish], 2, config).await;
    *harness.servers[0].mock.status_reply.lock().unwrap() = None;
    let health = harness.health.clone();
    let initial = health.snapshot();
    assert_eq!(initial.database.state, ProbeState::Unknown);
    assert!(
        initial
            .workers
            .iter()
            .all(|worker| worker.probe.state == ProbeState::Unknown)
    );
    let initial_http = health_response(&harness.app).await;
    assert_eq!(
        initial_http["database"],
        json!({"state":"unknown", "checked_at_ms":null, "error_code":null})
    );
    assert_eq!(
        initial_http["workers"][0],
        json!({"id":"worker-1", "state":"unknown", "checked_at_ms":null, "error_code":null, "status":null})
    );
    wait_for(|| {
        health.snapshot().database.state == ProbeState::Ok
            && health.snapshot().workers[1].probe.state == ProbeState::Ok
            && harness
                .servers
                .iter()
                .all(|server| server.mock.status_calls.load(Ordering::SeqCst) == 1)
    })
    .await;
    let snapshot = health.snapshot();
    assert_eq!(snapshot.workers[0].id, "worker-1");
    assert_eq!(snapshot.workers[1].id, "worker-2");
    assert_eq!(snapshot.workers[0].probe.state, ProbeState::Unknown);
    assert!(snapshot.database.checked_at_ms.unwrap() > 0);
    for _ in 0..10 {
        assert_eq!(health.snapshot(), snapshot);
        let body = health_response(&harness.app).await;
        assert_eq!(body["database"]["state"], "ok");
        assert_eq!(
            body["database"]["checked_at_ms"],
            snapshot.database.checked_at_ms.unwrap()
        );
        assert_eq!(body["workers"][0]["state"], "unknown");
        assert_eq!(
            body["workers"][1]["status"],
            json!({"stopping":false, "execution_state":"idle", "unacknowledged_results":0, "unacknowledged_capacity":128})
        );
    }
    for server in &harness.servers {
        assert_eq!(server.mock.status_calls.load(Ordering::SeqCst), 1);
        assert_eq!(server.mock.submits.load(Ordering::SeqCst), 0);
        assert_eq!(server.mock.gets.load(Ordering::SeqCst), 0);
        assert_eq!(server.mock.acks.load(Ordering::SeqCst), 0);
    }
    harness.stop_health().await;
    assert_eq!(health.snapshot(), snapshot);
    harness.close().await;
}

/// The collector replaces successes with precise failures and later recovers on the same channel.
#[tokio::test]
async fn health_status_failures_and_recovery_keep_admission_open() {
    let config = HealthConfig {
        interval_ms: 20,
        timeout_ms: 1_000,
    };
    let mut harness = Harness::start_with_health(&[Mode::Finish], 2, config).await;
    let health = harness.health.clone();
    wait_for(|| health.snapshot().workers[0].probe.state == ProbeState::Ok).await;
    let valid = GetWorkerStatusReply {
        stopping: true,
        execution_state: WorkerExecutionState::Blocked as i32,
        unacknowledged_results: 2,
        unacknowledged_capacity: 2,
    };
    for (reply, expected) in [
        (
            Err(tonic::Status::unavailable("Controlled status failure")),
            ProbeErrorCode::WorkerRpcFailed,
        ),
        (
            Err(tonic::Status::cancelled("Controlled remote cancellation")),
            ProbeErrorCode::WorkerRpcFailed,
        ),
        (
            Err(tonic::Status::unimplemented("Old Worker")),
            ProbeErrorCode::WorkerStatusUnsupported,
        ),
        (
            Err(tonic::Status::deadline_exceeded("Controlled deadline")),
            ProbeErrorCode::ProbeTimeout,
        ),
        (
            Ok(GetWorkerStatusReply {
                execution_state: 0,
                ..valid
            }),
            ProbeErrorCode::InvalidWorkerStatus,
        ),
    ] {
        *harness.servers[0].mock.status_reply.lock().unwrap() = Some(reply);
        wait_for(|| health.snapshot().workers[0].probe.error_code == Some(expected)).await;
        let failed = health.snapshot();
        assert_eq!(failed.workers[0].probe.state, ProbeState::Failed);
        assert!(failed.workers[0].probe.value.is_none());
        assert!(failed.workers[0].probe.checked_at_ms.is_some());
        assert_eq!(
            local_snapshot(&harness.creation, &harness.scheduler).submission_admission,
            SubmissionAdmission::Open
        );
        let body = health_response(&harness.app).await;
        assert_eq!(body["submission_admission"], "open");
        assert_eq!(body["workers"][0]["state"], "failed");
        assert!(body["workers"][0]["status"].is_null());
        let expected_code = match expected {
            ProbeErrorCode::WorkerRpcFailed => "WorkerRpcFailed",
            ProbeErrorCode::WorkerStatusUnsupported => "WorkerStatusUnsupported",
            ProbeErrorCode::ProbeTimeout => "ProbeTimeout",
            ProbeErrorCode::InvalidWorkerStatus => "InvalidWorkerStatus",
            ProbeErrorCode::DatabaseReadFailed => unreachable!(),
        };
        assert_eq!(body["workers"][0]["error_code"], expected_code);
        let public = body.to_string();
        assert!(!public.contains(&harness.servers[0].address));
        assert!(!public.contains(harness.fixture.config.database.path.to_str().unwrap()));
        assert!(!public.contains("Controlled"));
        let permit = harness.scheduler.reserve().unwrap();
        drop(permit);
        *harness.servers[0].mock.status_reply.lock().unwrap() = Some(Ok(valid));
        wait_for(|| health.snapshot().workers[0].probe.state == ProbeState::Ok).await;
        let recovered = health.snapshot().workers[0].probe.value.unwrap();
        assert!(recovered.stopping);
        assert_eq!(
            recovered.execution_state,
            acorn_backend::health::ExecutionState::Blocked
        );
        assert_eq!(recovered.unacknowledged_results, 2);
        assert_eq!(recovered.unacknowledged_capacity, 2);
        assert_eq!(
            health_response(&harness.app).await["workers"][0]["status"],
            json!({"stopping":true, "execution_state":"blocked", "unacknowledged_results":2, "unacknowledged_capacity":2})
        );
    }
    assert_eq!(harness.servers[0].mock.submits.load(Ordering::SeqCst), 0);
    assert_eq!(harness.servers[0].mock.gets.load(Ordering::SeqCst), 0);
    assert_eq!(harness.servers[0].mock.acks.load(Ordering::SeqCst), 0);
    harness.stop_health().await;
    harness.close().await;
}

/// A nonresponding Worker consumes only the health budget; stop also interrupts the round delay.
#[tokio::test]
async fn health_worker_timeout_and_stop_between_rounds() {
    let config = HealthConfig {
        interval_ms: 60_000,
        timeout_ms: 1_000,
    };
    let mut harness = Harness::start_with_health(&[Mode::Finish], 2, config).await;
    *harness.servers[0].mock.status_reply.lock().unwrap() = None;
    let health = harness.health.clone();
    wait_for(|| harness.servers[0].mock.status_calls.load(Ordering::SeqCst) == 1).await;
    wait_for(|| {
        health.snapshot().workers[0].probe.error_code == Some(ProbeErrorCode::ProbeTimeout)
    })
    .await;
    assert_eq!(
        harness.servers[0].mock.status_calls.load(Ordering::SeqCst),
        1
    );
    assert_eq!(health.snapshot().database.state, ProbeState::Ok);
    let body = health_response(&harness.app).await;
    assert_eq!(body["workers"][0]["error_code"], "ProbeTimeout");
    assert_eq!(body["submission_admission"], "open");
    harness.stop_health().await;
    assert_eq!(
        harness.servers[0].mock.status_calls.load(Ordering::SeqCst),
        1
    );
    harness.close().await;
}

/// Empty tables succeed; actual read errors and budget expiry affect only the cached observation.
#[tokio::test]
async fn health_database_read_failure_and_timeout_recover() {
    let mut fixture = Fixture::new().await;
    fixture.database.close().await;
    fixture.config.database.busy_timeout_ms = 50;
    fixture.database = Database::open(&fixture.config).await.unwrap();
    let mut connection = fixture.connection().await;
    sqlx::query("DELETE FROM users")
        .execute(&mut connection)
        .await
        .unwrap();
    fixture.database.probe_read().await.unwrap();
    let server = RpcServer::start(Mode::Finish, fixture.database.clone()).await;
    fixture.config.scheduler.workers = vec![server.address.clone()];
    let (review, review_task) =
        ReviewService::start(&fixture.config.llm_review, fixture.database.clone()).unwrap();
    let (scheduler, scheduler_task) = Scheduler::start(
        &fixture.config,
        fixture.database.clone(),
        Vec::new(),
        review.clone(),
    )
    .unwrap();
    for (timeout_ms, expected) in [
        (1_000, ProbeErrorCode::DatabaseReadFailed),
        (10, ProbeErrorCode::ProbeTimeout),
    ] {
        let config = HealthConfig {
            interval_ms: 20,
            timeout_ms,
        };
        let (health, task) = Health::start(&config, fixture.database.clone(), &scheduler).unwrap();
        let creation =
            CreationService::new(&fixture.config, fixture.database.clone(), scheduler.clone());
        let app = http::router(
            &fixture.config,
            fixture.database.clone(),
            creation,
            scheduler.clone(),
            health.clone(),
            review.clone(),
        );
        wait_for(|| health.snapshot().database.state == ProbeState::Ok).await;
        sqlx::query("BEGIN EXCLUSIVE")
            .execute(&mut connection)
            .await
            .unwrap();
        wait_for(|| health.snapshot().database.error_code == Some(expected)).await;
        assert_eq!(health.snapshot().database.state, ProbeState::Failed);
        assert_eq!(scheduler.snapshot().state, SchedulerState::Running);
        let body = health_response(&app).await;
        assert_eq!(body["database"]["state"], "failed");
        assert_eq!(
            body["database"]["error_code"],
            if expected == ProbeErrorCode::ProbeTimeout {
                "ProbeTimeout"
            } else {
                "DatabaseReadFailed"
            }
        );
        assert_eq!(body["submission_admission"], "open");
        tokio::select! {
            biased;
            error = fixture.database.failure() => panic!("Ordinary health read triggered a fatal error: {error}"),
            _ = std::future::ready(()) => {},
        }
        sqlx::query("ROLLBACK")
            .execute(&mut connection)
            .await
            .unwrap();
        wait_for(|| health.snapshot().database.state == ProbeState::Ok).await;
        assert_eq!(health_response(&app).await["database"]["state"], "ok");
        health.stop();
        task.await.unwrap().unwrap();
    }
    scheduler.shutdown();
    scheduler_task.await.unwrap().unwrap();
    review.stop();
    review_task.await.unwrap().unwrap();
    assert_eq!(server.mock.submits.load(Ordering::SeqCst), 0);
    assert_eq!(server.mock.acks.load(Ordering::SeqCst), 0);
    server.close().await;
    connection.close().await.unwrap();
    fixture.database.close().await;
}

/// Full queues reject new records but known queued and terminal keys remain idempotent.
#[tokio::test]
async fn queue_full_and_idempotency_preserve_identity_and_version() {
    let harness = Harness::start(&[Mode::Busy], 1).await;
    let key = Uuid::new_v4();
    let body = json!({"problem_id": 1, "language_id": "c", "source": "int main(void) {}\n"});
    let (status, _, first) = call(
        &harness.app,
        "POST",
        "/api/v1/submissions",
        Some(body.clone()),
        Some(key),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    harness.fixture.publish_next().await;
    let (status, _, repeated) = call(
        &harness.app,
        "POST",
        "/api/v1/submissions",
        Some(body.clone()),
        Some(key),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated["submission_id"], first["submission_id"]);
    let id = first["submission_id"].as_str().unwrap();
    assert_eq!(
        harness
            .fixture
            .database
            .submission(id)
            .await
            .unwrap()
            .unwrap()
            .info
            .version_no,
        1
    );
    let (status, _, conflict) = call(
        &harness.app,
        "POST",
        "/api/v1/submissions",
        Some(json!({"problem_id": 1, "language_id": "python", "source": "changed"})),
        Some(key),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "IdempotencyConflict");
    let new_key = Uuid::new_v4();
    let (status, _, full) = call(
        &harness.app,
        "POST",
        "/api/v1/submissions",
        Some(body.clone()),
        Some(new_key),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(full["error"]["code"], "QueueFull");
    assert!(full["error"].get("submission_id").is_none());
    assert!(
        harness
            .fixture
            .database
            .submission_by_key(new_key)
            .await
            .unwrap()
            .is_none()
    );
    let orphan_key = Uuid::new_v4();
    let orphan = harness
        .fixture
        .database
        .create_submission(CreateSubmission {
            problem_id: 1,
            language_id: "c".into(),
            source: body["source"].as_str().unwrap().as_bytes().to_vec(),
            request_key: orphan_key,
        })
        .await
        .unwrap();
    let (status, _, full) = call(
        &harness.app,
        "POST",
        "/api/v1/submissions",
        Some(body),
        Some(orphan_key),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(full["error"]["submission_id"], orphan.info.id);
    harness.close().await;
}

/// A queued record missing from memory re-enters with its original version after publication.
#[tokio::test]
async fn queued_rehandoff_keeps_original_version() {
    let harness = Harness::start(&[Mode::Finish], 4).await;
    let key = Uuid::new_v4();
    let orphan = harness
        .fixture
        .database
        .create_submission(CreateSubmission {
            problem_id: 1,
            language_id: "python".into(),
            source: b"ac".to_vec(),
            request_key: key,
        })
        .await
        .unwrap();
    harness.fixture.publish_next().await;
    let (status, _, response) = call(
        &harness.app,
        "POST",
        "/api/v1/submissions",
        Some(json!({"problem_id":1,"language_id":"python","source":"ac"})),
        Some(key),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["submission_id"], orphan.info.id);
    assert_eq!(harness.terminal(&orphan.info.id).await.info.version_no, 1);
    let id = create(&harness.app, "python", "ac").await;
    assert_eq!(harness.terminal(&id).await.info.version_no, 2);
    harness.close().await;
}

/// Simultaneous same-key creates merge into one persisted and locally queued submission.
#[tokio::test]
async fn concurrent_creation_merges_and_rollback_releases_permits() {
    let harness = Harness::start(&[Mode::Busy], 16).await;
    let key = Uuid::new_v4();
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let app = harness.app.clone();
        requests.spawn(async move {
            call(
                &app,
                "POST",
                "/api/v1/submissions",
                Some(json!({"problem_id":1,"language_id":"python","source":"same"})),
                Some(key),
            )
            .await
        });
    }
    let mut ids = std::collections::HashSet::new();
    let mut new_count = 0;
    while let Some(response) = requests.join_next().await {
        let (status, _, body) = response.unwrap();
        assert!(matches!(status, StatusCode::OK | StatusCode::ACCEPTED));
        new_count += usize::from(status == StatusCode::ACCEPTED);
        ids.insert(body["submission_id"].as_str().unwrap().to_owned());
    }
    assert_eq!(new_count, 1);
    assert_eq!(ids.len(), 1);
    for _ in 0..16 {
        let (status, _, body) = call(
            &harness.app,
            "POST",
            "/api/v1/submissions",
            Some(json!({"problem_id":1,"language_id":"rust","source":"unsupported"})),
            Some(Uuid::new_v4()),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "UnsupportedLanguage");
    }
    let mut permits = Vec::new();
    while let Ok(permit) = harness.scheduler.reserve() {
        permits.push(permit);
    }
    assert_eq!(permits.len(), 15);
    drop(permits);
    harness.close().await;
}

/// Aborting an HTTP waiter during a locked creation does not cancel commit or handoff.
#[tokio::test]
async fn disconnected_create_finishes_transaction_and_handoff() {
    let harness = Harness::start(&[Mode::Finish], 1).await;
    let mut lock = harness.fixture.connection().await;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut lock)
        .await
        .unwrap();
    let key = Uuid::new_v4();
    let app = harness.app.clone();
    let waiting = tokio::spawn(async move {
        call(
            &app,
            "POST",
            "/api/v1/submissions",
            Some(json!({"problem_id":1,"language_id":"python","source":"ac"})),
            Some(key),
        )
        .await
    });
    wait_for(|| matches!(harness.scheduler.reserve(), Err(AdmissionError::QueueFull))).await;
    waiting.abort();
    assert!(waiting.await.unwrap_err().is_cancelled());
    sqlx::query("COMMIT").execute(&mut lock).await.unwrap();
    harness.creation.wait_idle().await;
    let saved = harness
        .fixture
        .database
        .submission_by_key(key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        harness.terminal(&saved.info.id).await.info.verdict,
        Some(Verdict::AC)
    );
    lock.close().await.unwrap();
    harness.close().await;
}

/// Known ended records remain accessible and cannot be overwritten by a late result.
#[tokio::test]
async fn terminal_replay_bypasses_capacity_and_late_result_is_acked() {
    let harness = Harness::start(&[Mode::Running], 1).await;
    let key = Uuid::new_v4();
    let body = json!({"problem_id":1,"language_id":"python","source":"ac"});
    let (_, _, response) = call(
        &harness.app,
        "POST",
        "/api/v1/submissions",
        Some(body.clone()),
        Some(key),
    )
    .await;
    let id = response["submission_id"].as_str().unwrap();
    wait_for(|| harness.servers[0].mock.gets.load(Ordering::SeqCst) > 0).await;
    let failure = acorn_protocol::worker::v1::JobFailure {
        code: "WorkerUnavailable".into(),
        diagnostic: "Previously saved failure".into(),
    };
    let terminal = TerminalOutcome::failed(&failure, &harness.fixture.config.results).unwrap();
    assert!(
        harness
            .fixture
            .database
            .save_terminal(id, &terminal, None)
            .await
            .unwrap()
    );
    let permit = harness.scheduler.reserve().unwrap();
    let (status, _, repeated) = call(
        &harness.app,
        "POST",
        "/api/v1/submissions",
        Some(body),
        Some(key),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated["submission_id"], id);
    harness.servers[0]
        .mock
        .released
        .store(true, Ordering::SeqCst);
    let saved = harness.terminal(id).await;
    assert_eq!(saved.info.state, SubmissionState::Failed);
    assert_eq!(harness.servers[0].mock.acks.load(Ordering::SeqCst), 1);
    let ac = TerminalOutcome::judged(
        &JudgeResult {
            verdict: RpcVerdict::Accepted as i32,
            ..Default::default()
        },
        4,
        &harness.fixture.config.results,
    )
    .unwrap();
    assert!(
        !harness
            .fixture
            .database
            .save_terminal(id, &ac, None)
            .await
            .unwrap()
    );
    drop(permit);
    harness.close().await;
}

/// Routing, extraction and business errors all use stable English JSON envelopes.
#[tokio::test]
async fn http_errors_and_body_limits() {
    let harness = Harness::start(&[Mode::Finish], 4).await;
    for (method, path, status, code) in [
        ("GET", "/missing", StatusCode::NOT_FOUND, "RouteNotFound"),
        (
            "DELETE",
            "/api/v1/problems",
            StatusCode::METHOD_NOT_ALLOWED,
            "MethodNotAllowed",
        ),
        (
            "POST",
            "/api/v1/health",
            StatusCode::METHOD_NOT_ALLOWED,
            "MethodNotAllowed",
        ),
        (
            "GET",
            "/api/v1/problems/not-a-number",
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
        ),
        (
            "GET",
            "/api/v1/problems?limit=0",
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
        ),
        (
            "GET",
            "/api/v1/submissions?state=unknown",
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
        ),
        (
            "GET",
            "/api/v1/problems?extra=1",
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
        ),
        (
            "GET",
            "/api/v1/problems/999",
            StatusCode::NOT_FOUND,
            "ProblemNotFound",
        ),
        (
            "GET",
            "/api/v1/problems/1/versions/999",
            StatusCode::NOT_FOUND,
            "ProblemVersionNotFound",
        ),
    ] {
        let (actual, _, body) = call(&harness.app, method, path, None, None).await;
        assert_eq!(actual, status, "{path}: {body}");
        assert_eq!(body["error"]["code"], code);
    }
    for (body, key, status, code) in [
        (
            json!({"problem_id":1,"language_id":"python","source":"ac"}),
            None,
            StatusCode::BAD_REQUEST,
            "InvalidIdempotencyKey",
        ),
        (
            json!({"problem_id":1,"language_id":"python","source":""}),
            Some(Uuid::new_v4()),
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
        ),
        (
            json!({"problem_id":1,"language_id":"python","source":"ac","user_id":2}),
            Some(Uuid::new_v4()),
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
        ),
        (
            json!({"problem_id":999,"language_id":"python","source":"ac"}),
            Some(Uuid::new_v4()),
            StatusCode::NOT_FOUND,
            "ProblemNotFound",
        ),
        (
            json!({"problem_id":1,"language_id":"python","source":"x".repeat(65537)}),
            Some(Uuid::new_v4()),
            StatusCode::PAYLOAD_TOO_LARGE,
            "SourceTooLarge",
        ),
        (
            json!({"problem_id":1,"language_id":"python","source":"x".repeat(524288)}),
            Some(Uuid::new_v4()),
            StatusCode::PAYLOAD_TOO_LARGE,
            "RequestTooLarge",
        ),
    ] {
        let (actual, _, body) =
            call(&harness.app, "POST", "/api/v1/submissions", Some(body), key).await;
        assert_eq!(actual, status, "{body}");
        assert_eq!(body["error"]["code"], code);
    }
    for (content_type, body, code) in [
        ("text/plain", "{}", "UnsupportedMediaType"),
        ("application/json", "{", "InvalidRequest"),
    ] {
        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/submissions")
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024).await.unwrap()).unwrap();
        assert_eq!(body["error"]["code"], code);
    }
    let (_, _, body) = call(
        &harness.app,
        "GET",
        &format!("/api/v1/submissions/{}", Uuid::new_v4()),
        None,
        None,
    )
    .await;
    assert_eq!(body["error"]["code"], "SubmissionNotFound");
    let mut connection = harness.fixture.connection().await;
    sqlx::query(
        "DELETE FROM problem_version_languages WHERE problem_id = 1 AND language_id = 'python'",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();
    let (status, _, body) = call(
        &harness.app,
        "POST",
        "/api/v1/submissions",
        Some(json!({"problem_id":1,"language_id":"python","source":"ac"})),
        Some(Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "LanguageNotAllowed");
    harness.creation.stop();
    let (status, _, body) = call(
        &harness.app,
        "POST",
        "/api/v1/submissions",
        Some(json!({"problem_id":1,"language_id":"python","source":"ac"})),
        Some(Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"]["code"], "BackendStopping");
    harness.close().await;
}

/// Required writes fail the dispatcher and never ACK an uncommitted result.
#[tokio::test]
async fn database_failure_ends_tracking_without_ack() {
    let harness = Harness::start(&[Mode::Running], 2).await;
    let id = harness.submit("python", "ac").await;
    wait_for(|| harness.servers[0].mock.gets.load(Ordering::SeqCst) > 0).await;
    harness.fixture.database.close().await;
    harness.servers[0]
        .mock
        .released
        .store(true, Ordering::SeqCst);
    wait_for(|| !harness.scheduler.contains(&id)).await;
    assert_eq!(harness.servers[0].mock.acks.load(Ordering::SeqCst), 0);
    assert!(matches!(
        harness.scheduler.reserve(),
        Err(AdmissionError::Stopping)
    ));
    // The dispatcher failure is deliberately observed separately from normal cleanup.
    harness.close_failed().await;
}

/// Startup loads fixed-version tasks in order, accounts for permits and never contacts old work.
#[tokio::test]
async fn process_startup_loads_queue_and_interrupts_in_flight() {
    let mut fixture = Fixture::new().await;
    fixture.config.scheduler.pending_max_count = 2;
    fixture.config.scheduler.poll_interval_ms = 10;
    let dispatching = fixture.queued("c").await;
    let running = fixture.queued("python").await;
    let finished = fixture.queued("c").await;
    for id in [&dispatching, &running, &finished] {
        assert!(fixture.database.mark_dispatching(id).await.unwrap());
    }
    assert!(fixture.database.mark_running(&running).await.unwrap());
    let terminal = TerminalOutcome::judged(
        &JudgeResult {
            verdict: RpcVerdict::Accepted as i32,
            ..Default::default()
        },
        4,
        &fixture.config.results,
    )
    .unwrap();
    fixture
        .database
        .save_terminal(&finished, &terminal, None)
        .await
        .unwrap();
    let previous_terminal = fixture.database.submission(&finished).await.unwrap();
    let first = fixture.queued("c").await;
    let second = fixture.queued("python").await;
    let expected = fixture
        .database
        .queued_tasks()
        .await
        .unwrap()
        .into_iter()
        .map(|task| task.request.job_id)
        .collect::<Vec<_>>();
    fixture.publish_next().await;
    let server = RpcServer::start(Mode::AwaitAdmission, fixture.database.clone()).await;
    fixture.config.scheduler.workers = vec![server.address.clone()];
    let mut backend = BackendProcess::spawn(&fixture);
    let address = backend.listening().await;
    wait_for(|| server.mock.submits.load(Ordering::SeqCst) == 1).await;
    let (status, body) = network_call(
        address,
        "POST",
        "/api/v1/submissions",
        Some(json!({"problem_id":1,"language_id":"c","source":"ac"})),
        Some(Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, 503);
    assert_eq!(body["error"]["code"], "QueueFull");
    for id in [&dispatching, &running] {
        let stored = fixture.database.submission(id).await.unwrap().unwrap();
        assert_eq!(stored.info.state, SubmissionState::Failed);
        let Some(TerminalDetails::Failure(failure)) = stored.terminal_details else {
            panic!("Expected startup interruption");
        };
        assert_eq!(failure.code, "SchedulerInterrupted");
    }
    assert_eq!(
        fixture.database.submission(&finished).await.unwrap(),
        previous_terminal
    );
    server.mock.released.store(true, Ordering::SeqCst);
    wait_for(|| server.mock.acks.load(Ordering::SeqCst) == 2).await;
    assert_eq!(*server.mock.submission_order.lock().unwrap(), expected);
    {
        let submitted = server.mock.submitted.lock().unwrap();
        assert_eq!(submitted.len(), 2);
        for (id, cpu_time_ms) in [(&first, 1000), (&second, 2000)] {
            assert_eq!(
                submitted[id].run_limits.as_ref().unwrap().cpu_time_ms,
                cpu_time_ms
            );
        }
    }
    backend.terminate();
    let (status, logs) = backend.finish().await;
    assert_eq!(status.code(), Some(0), "{logs}");
    server.close().await;
    fixture.database.close().await;
}

/// Excess queued work aborts startup before HTTP or any Worker RPC can begin.
#[tokio::test]
async fn process_rejects_startup_queue_overflow() {
    let mut fixture = Fixture::new().await;
    fixture.queued("c").await;
    fixture.queued("python").await;
    fixture.config.scheduler.pending_max_count = 1;
    let server = RpcServer::start(Mode::Finish, fixture.database.clone()).await;
    fixture.config.scheduler.workers = vec![server.address.clone()];
    let (status, logs) = BackendProcess::spawn(&fixture).finish().await;
    assert_eq!(status.code(), Some(1), "{logs}");
    assert!(logs.contains("increase the configured limit"), "{logs}");
    assert!(!logs.contains("Backend HTTP listening"));
    assert_eq!(server.mock.submits.load(Ordering::SeqCst), 0);
    server.close().await;
    fixture.database.close().await;
}

/// Ordinary query and preflight lock failures return 500 and recover after the external unlock.
#[tokio::test]
async fn process_read_failures_keep_serving() {
    let mut fixture = Fixture::new().await;
    fixture.config.database.busy_timeout_ms = 75;
    let mut backend = BackendProcess::spawn(&fixture);
    let address = backend.listening().await;
    let mut connection = fixture.connection().await;
    sqlx::query("BEGIN EXCLUSIVE")
        .execute(&mut connection)
        .await
        .unwrap();
    let (status, body) = network_call(address, "GET", "/api/v1/problems", None, None).await;
    assert_eq!(status, 500);
    assert_eq!(body["error"]["code"], "InternalError");
    let (status, body) = network_call(
        address,
        "POST",
        "/api/v1/submissions",
        Some(json!({"problem_id":1,"language_id":"python","source":"ac"})),
        Some(Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, 500);
    assert_eq!(body["error"]["code"], "InternalError");
    sqlx::query("ROLLBACK")
        .execute(&mut connection)
        .await
        .unwrap();
    assert_eq!(
        network_call(address, "GET", "/api/v1/problems", None, None)
            .await
            .0,
        200
    );
    backend.terminate();
    let (status, logs) = backend.finish().await;
    assert_eq!(status.code(), Some(0), "{logs}");
    assert!(logs.contains("Read problem list"));
    assert!(logs.contains("Read creation key"));
    connection.close().await.unwrap();
    fixture.database.close().await;
}

/// Failed creation writes exit promptly even while an unrelated Worker remains running.
#[tokio::test]
async fn process_creation_failure_exits_without_draining_workers() {
    use tokio::io::AsyncWriteExt;
    let mut fixture = Fixture::new().await;
    fixture.config.database.busy_timeout_ms = 75;
    let running = fixture.queued("python").await;
    let server = RpcServer::start(Mode::Running, fixture.database.clone()).await;
    fixture.config.scheduler.workers = vec![server.address.clone()];
    let mut backend = BackendProcess::spawn(&fixture);
    let address = backend.listening().await;
    wait_for(|| server.mock.gets.load(Ordering::SeqCst) > 0).await;
    let mut connection = fixture.connection().await;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut connection)
        .await
        .unwrap();
    let key = Uuid::new_v4();
    let body = json!({"problem_id":1,"language_id":"c","source":"ac"}).to_string();
    let request = format!(
        "POST /api/v1/submissions HTTP/1.0\r\nContent-Type: application/json\r\nIdempotency-Key: {key}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let (status, logs) = backend.finish().await;
    assert_eq!(status.code(), Some(1), "{logs}");
    assert!(logs.contains("Create submission transaction"), "{logs}");
    assert!(logs.contains("Backend fault"), "{logs}");
    assert_eq!(server.mock.acks.load(Ordering::SeqCst), 0);
    sqlx::query("ROLLBACK")
        .execute(&mut connection)
        .await
        .unwrap();
    assert!(
        fixture
            .database
            .submission_by_key(key)
            .await
            .unwrap()
            .is_none()
    );
    let stored = fixture
        .database
        .submission(&running)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.info.state, SubmissionState::Running);
    assert!(stored.info.terminal_at_ms.is_none());
    connection.close().await.unwrap();
    drop(stream);
    server.close().await;
    fixture.database.close().await;
}

/// Normal shutdown saves in-flight work, keeps queries available and resumes queued work on restart.
#[tokio::test]
async fn process_shutdown_drains_running_and_restart_loads_queued() {
    let mut fixture = Fixture::new().await;
    fixture.config.scheduler.poll_interval_ms = 10;
    fixture.config.health.timeout_ms = 60_000;
    fixture.queued("c").await;
    fixture.queued("python").await;
    let expected = fixture
        .database
        .queued_tasks()
        .await
        .unwrap()
        .into_iter()
        .map(|task| task.request.job_id)
        .collect::<Vec<_>>();
    let server = RpcServer::start(Mode::Running, fixture.database.clone()).await;
    *server.mock.status_reply.lock().unwrap() = None;
    fixture.config.scheduler.workers = vec![server.address.clone()];
    let mut backend = BackendProcess::spawn(&fixture);
    let address = backend.listening().await;
    wait_for(|| server.mock.gets.load(Ordering::SeqCst) > 0).await;
    wait_for(|| server.mock.status_calls.load(Ordering::SeqCst) == 1).await;
    backend.terminate();
    backend
        .until_log("Backend stopping admission and dispatch")
        .await;
    let (status, body) = network_call(
        address,
        "POST",
        "/api/v1/submissions",
        Some(json!({"problem_id":1,"language_id":"c","source":"ac"})),
        Some(Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, 503);
    assert_eq!(body["error"]["code"], "BackendStopping");
    assert_eq!(
        network_call(address, "GET", "/api/v1/problems", None, None)
            .await
            .0,
        200
    );
    let (status, health) = network_call(address, "GET", "/api/v1/health", None, None).await;
    assert_eq!(status, 200);
    assert_eq!(health["submission_admission"], "stopping");
    assert_eq!(health["scheduler"]["state"], "stopping");
    assert_eq!(health["workers"][0]["state"], "unknown");
    server.mock.released.store(true, Ordering::SeqCst);
    let (status, logs) = backend.finish().await;
    assert_eq!(status.code(), Some(0), "{logs}");
    assert!(
        logs.find("Health collector stopped").unwrap() < logs.find("Backend stopped").unwrap(),
        "{logs}"
    );
    assert_eq!(server.mock.submits.load(Ordering::SeqCst), 1);
    assert_eq!(server.mock.acks.load(Ordering::SeqCst), 1);
    let previous_terminal = fixture.database.submission(&expected[0]).await.unwrap();
    assert_eq!(
        previous_terminal.as_ref().unwrap().info.verdict,
        Some(Verdict::AC)
    );
    assert_eq!(
        fixture
            .database
            .submission(&expected[1])
            .await
            .unwrap()
            .unwrap()
            .info
            .state,
        SubmissionState::Queued
    );
    server.close().await;
    let restarted_server = RpcServer::start(Mode::Finish, fixture.database.clone()).await;
    fixture.config.scheduler.workers = vec![restarted_server.address.clone()];
    let mut restarted = BackendProcess::spawn(&fixture);
    restarted.listening().await;
    wait_for(|| restarted_server.mock.acks.load(Ordering::SeqCst) == 1).await;
    assert_eq!(
        *restarted_server.mock.submission_order.lock().unwrap(),
        [expected[1].clone()]
    );
    assert_eq!(
        fixture.database.submission(&expected[0]).await.unwrap(),
        previous_terminal
    );
    restarted.terminate();
    let (status, logs) = restarted.finish().await;
    assert_eq!(status.code(), Some(0), "{logs}");
    restarted_server.close().await;
    fixture.database.close().await;
}

/// A terminal-write fault overrides an ongoing normal shutdown without ACK or waiting other work.
#[tokio::test]
async fn process_fault_during_shutdown_does_not_wait_other_worker() {
    let mut fixture = Fixture::new().await;
    fixture.config.scheduler.poll_interval_ms = 10;
    fixture.config.health.timeout_ms = 60_000;
    fixture.queued("c").await;
    fixture.queued("python").await;
    let first = RpcServer::start(Mode::Running, fixture.database.clone()).await;
    let second = RpcServer::start(Mode::Running, fixture.database.clone()).await;
    *first.mock.status_reply.lock().unwrap() = None;
    *second.mock.status_reply.lock().unwrap() = None;
    fixture.config.scheduler.workers = vec![first.address.clone(), second.address.clone()];
    let mut backend = BackendProcess::spawn(&fixture);
    backend.listening().await;
    wait_for(|| {
        first.mock.gets.load(Ordering::SeqCst) > 0
            && second.mock.gets.load(Ordering::SeqCst) > 0
            && first.mock.status_calls.load(Ordering::SeqCst) == 1
            && second.mock.status_calls.load(Ordering::SeqCst) == 1
    })
    .await;
    let mut connection = fixture.connection().await;
    sqlx::query("CREATE TRIGGER reject_terminal BEFORE UPDATE ON submissions WHEN NEW.state = 'finished' BEGIN SELECT RAISE(ABORT, 'Controlled terminal write failure'); END")
        .execute(&mut connection).await.unwrap();
    backend.terminate();
    backend
        .until_log("Backend stopping admission and dispatch")
        .await;
    first.mock.released.store(true, Ordering::SeqCst);
    let (status, logs) = backend.finish().await;
    assert_eq!(status.code(), Some(1), "{logs}");
    assert!(logs.contains("Controlled terminal write failure"), "{logs}");
    assert!(logs.contains("Health collector stopped"), "{logs}");
    assert_eq!(first.mock.acks.load(Ordering::SeqCst), 0);
    assert_eq!(second.mock.acks.load(Ordering::SeqCst), 0);
    for server in [&first, &second] {
        let ids = server.mock.submission_order.lock().unwrap().clone();
        let stored = fixture.database.submission(&ids[0]).await.unwrap().unwrap();
        assert_eq!(stored.info.state, SubmissionState::Running);
        assert!(stored.info.terminal_at_ms.is_none());
    }
    connection.close().await.unwrap();
    first.close().await;
    second.close().await;
    fixture.database.close().await;
}

/// A creation admitted before stopping may reserve after its preflight and finish local handoff.
#[tokio::test]
async fn stopping_preserves_creation_started_before_preflight() {
    use std::{
        future::{Future, poll_fn},
        task::Poll,
    };
    let harness = Harness::start(&[Mode::Finish], 1).await;
    let creation = harness.creation.clone();
    let creating = creation.create(CreateSubmission {
        problem_id: 1,
        language_id: "python".into(),
        source: b"ac".to_vec(),
        request_key: Uuid::new_v4(),
    });
    tokio::pin!(creating);
    poll_fn(|context| {
        assert!(creating.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    harness.creation.stop();
    harness.scheduler.pause();
    let created = creating.await.unwrap();
    harness.creation.wait_idle().await;
    assert!(harness.scheduler.contains(&created.info.id));
    assert_eq!(
        harness
            .fixture
            .database
            .submission(&created.info.id)
            .await
            .unwrap()
            .unwrap()
            .info
            .state,
        SubmissionState::Queued
    );
    assert_eq!(harness.servers[0].mock.submits.load(Ordering::SeqCst), 0);
    harness.close().await;
}

/// Shared source samples through real HTTP, gRPC and Isolate, including ACK retention.
#[tokio::test]
#[ignore = "Requires host Isolate privileges, cgroups, quotas and ACORN_ISOLATE_BOX_ID"]
async fn real_http_worker_acceptance() {
    use acorn_protocol::worker::v1::{GetJobRequest, get_job_reply::Terminal};
    use base64::Engine;
    use support::real::{RealHarness, box_id};

    let harness = RealHarness::start(&[box_id("ACORN_ISOLATE_BOX_ID")]).await;
    let mut client = harness.workers[0].client().await;
    let mut first_id = None;
    for (language, source, verdict, completed) in [
        (
            "c",
            include_str!("../../../examples/submissions/a-plus-b.c"),
            "AC",
            104,
        ),
        (
            "python",
            include_str!("../../../examples/submissions/a-plus-b.py"),
            "AC",
            104,
        ),
        (
            "c",
            include_str!("../../../examples/submissions/compile-error.c"),
            "CE",
            0,
        ),
        (
            "python",
            include_str!("../../../examples/submissions/wrong-answer.py"),
            "WA",
            1,
        ),
        (
            "c",
            include_str!("../../../examples/submissions/time-limit.c"),
            "TLE",
            1,
        ),
    ] {
        let id = harness.submit(language, source).await;
        if first_id.is_none() {
            first_id = Some(id.clone());
        }
        let body = harness.terminal(&id, verdict).await;
        let job = client
            .get_job(GetJobRequest { job_id: id.clone() })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(job.completed_cases, completed);
        assert_eq!(job.total_cases, 104);
        assert!(matches!(job.terminal, Some(Terminal::Result(_))));
        match verdict {
            "CE" => {
                let diagnostic = body["terminal_details"]["compile_diagnostic_base64"]
                    .as_str()
                    .unwrap();
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(diagnostic)
                    .unwrap();
                assert!(!bytes.is_empty());
                assert!(
                    String::from_utf8_lossy(&bytes).contains("Intentional compilation failure")
                );
            }
            "WA" | "TLE" => {
                let case = &body["terminal_details"]["failed_case"];
                assert_eq!(case["case_index"], 1);
                assert_eq!(case["stdout_truncated"], false);
                assert_eq!(case["stderr_base64"], "");
                if verdict == "WA" {
                    assert_eq!(case["stdout_base64"], "MAo=");
                    assert_eq!(case["exit_code"], 0);
                } else {
                    assert_eq!(case["stdout_base64"], "");
                    assert!(case["resources"]["wall_time_us"].is_u64());
                }
            }
            _ => assert!(body["terminal_details"].is_null()),
        }
        eprintln!(
            "Real sample: submission={id} language={language} verdict={verdict} completed={completed}"
        );
    }
    let removed = client
        .get_job(GetJobRequest {
            job_id: first_id.unwrap(),
        })
        .await
        .unwrap_err();
    assert_eq!(removed.code(), tonic::Code::NotFound);
    harness.close().await;
}

/// Two real Isolate boxes execute independently and explicitly busy admission redispatches.
#[tokio::test]
#[ignore = "Requires two dedicated boxes in ACORN_ISOLATE_BOX_ID and ACORN_ISOLATE_BOX_ID_2"]
async fn real_two_workers_acceptance() {
    use acorn_protocol::worker::v1::{
        AcknowledgeResultRequest, GetJobRequest, JobPhase, JobState, RunLimits, SubmitJobRequest,
        TestCase,
    };
    use support::real::{RealHarness, box_id};

    let boxes = [
        box_id("ACORN_ISOLATE_BOX_ID"),
        box_id("ACORN_ISOLATE_BOX_ID_2"),
    ];
    assert_ne!(boxes[0], boxes[1], "Workers must own different boxes");
    let harness = RealHarness::start(&boxes).await;
    let mut a = harness.workers[0].client().await;
    let mut b = harness.workers[1].client().await;
    let source = include_str!("../../../examples/submissions/time-limit.c");
    let first = harness.submit("c", source).await;
    let second = harness.submit("c", source).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let left = a
                .get_job(GetJobRequest {
                    job_id: first.clone(),
                })
                .await;
            let right = b
                .get_job(GetJobRequest {
                    job_id: second.clone(),
                })
                .await;
            if let (Ok(left), Ok(right)) = (left, right) {
                let (left, right) = (left.into_inner(), right.into_inner());
                assert_eq!(
                    left.state,
                    JobState::Running as i32,
                    "First Job finished before overlap"
                );
                assert_eq!(
                    right.state,
                    JobState::Running as i32,
                    "Second Job finished before overlap"
                );
                if left.phase == JobPhase::RunningCase as i32
                    && right.phase == JobPhase::RunningCase as i32
                {
                    assert_eq!(left.completed_cases, 0);
                    assert_eq!(right.completed_cases, 0);
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    harness.terminal(&first, "TLE").await;
    harness.terminal(&second, "TLE").await;
    eprintln!(
        "Real overlap: submissions={first},{second} boxes={},{}",
        boxes[0], boxes[1]
    );

    // The dispatcher has selected both nodes once, so its next candidate is A again.
    let occupant = SubmitJobRequest {
        job_id: Uuid::new_v4().to_string(),
        language_id: "c".into(),
        source: source.as_bytes().to_vec(),
        cases: vec![TestCase {
            input: Vec::new(),
            expected_output: Vec::new(),
        }],
        run_limits: Some(RunLimits {
            cpu_time_ms: 5_000,
            wall_time_ms: 10_000,
            memory_kib: 262_144,
        }),
    };
    a.submit_job(occupant.clone()).await.unwrap();
    let mut probe = occupant.clone();
    probe.job_id = Uuid::new_v4().to_string();
    let busy = a.submit_job(probe).await.unwrap_err();
    assert_eq!(busy.code(), tonic::Code::ResourceExhausted);
    assert_eq!(busy.message(), "WorkerBusy");
    let moved = harness
        .submit(
            "c",
            include_str!("../../../examples/submissions/a-plus-b.c"),
        )
        .await;
    harness.terminal(&moved, "AC").await;
    assert_eq!(
        a.get_job(GetJobRequest {
            job_id: moved.clone()
        })
        .await
        .unwrap_err()
        .code(),
        tonic::Code::NotFound
    );
    assert_eq!(
        b.get_job(GetJobRequest {
            job_id: moved.clone()
        })
        .await
        .unwrap()
        .into_inner()
        .state,
        JobState::Finished as i32
    );
    assert_eq!(
        a.get_job(GetJobRequest {
            job_id: occupant.job_id.clone()
        })
        .await
        .unwrap()
        .into_inner()
        .state,
        JobState::Running as i32
    );
    eprintln!(
        "Real busy redispatch: submission={moved} worker={} occupant={}",
        harness.workers[1].address, occupant.job_id
    );
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let job = a
                .get_job(GetJobRequest {
                    job_id: occupant.job_id.clone(),
                })
                .await
                .unwrap()
                .into_inner();
            if job.state != JobState::Running as i32 {
                assert_eq!(job.state, JobState::Finished as i32, "{job:?}");
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    a.acknowledge_result(AcknowledgeResultRequest {
        job_id: occupant.job_id,
    })
    .await
    .unwrap();
    harness.close().await;
}
