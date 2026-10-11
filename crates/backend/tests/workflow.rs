//! P07 contracts over real SQLite and controlled or deployment Worker gRPC.

mod support;

use std::{sync::atomic::Ordering, time::Duration};

use acorn_backend::{
    scheduler::AdmissionError,
    submissions::{CreateSubmission, SubmissionState, TerminalDetails, Verdict},
};
use support::{Harness, Mode, wait_for};
use uuid::Uuid;

/// Terminal persistence preserves binary data, real zero and missing statistics before ACK.
#[tokio::test]
async fn terminal_details_and_single_ack() {
    let harness = Harness::start(&[Mode::AckFailure], 8).await;
    assert!(harness.fixture.directory.path().exists());
    for (source, verdict) in [
        ("ac", Verdict::AC),
        ("ce", Verdict::CE),
        ("wa", Verdict::WA),
    ] {
        let id = harness.submit("c", source).await;
        let terminal = harness.terminal(&id).await;
        assert_eq!(terminal.info.state, SubmissionState::Finished);
        assert_eq!(terminal.info.verdict, Some(verdict));
        assert!(terminal.info.terminal_at_ms.is_some());
        match terminal.terminal_details {
            None => assert_eq!(verdict, Verdict::AC),
            Some(TerminalDetails::Compile(details)) => {
                assert_eq!(verdict, Verdict::CE);
                assert_eq!(details.compile_diagnostic_base64, "AP8K");
            }
            Some(TerminalDetails::Case(details)) => {
                assert_eq!(verdict, Verdict::WA);
                let case = details.failed_case;
                assert_eq!(case.stdout_base64, "AP8=");
                let resources = case.resources.unwrap();
                assert_eq!(resources.cpu_time_us, Some(0));
                assert_eq!(resources.wall_time_us, None);
                assert_eq!(case.exit_code, Some(0));
                assert_eq!(case.signal, None);
            }
            Some(TerminalDetails::Failure(details)) => panic!("Unexpected failure: {details:?}"),
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
    let id = harness.submit("python", "ac").await;
    wait_for(|| harness.servers[0].mock.submits.load(Ordering::SeqCst) == 1).await;
    assert!(matches!(
        harness.scheduler.reserve(),
        Err(AdmissionError::QueueFull)
    ));
    harness.servers[0]
        .mock
        .released
        .store(true, Ordering::SeqCst);
    harness.terminal(&id).await;
    assert!(harness.scheduler.reserve().is_ok());
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

/// Scheduler through real gRPC and Isolate; requires a dedicated deployment box.
#[tokio::test]
#[ignore = "Requires host Isolate privileges, cgroups, quotas and ACORN_ISOLATE_BOX_ID"]
async fn real_scheduler_acceptance() {
    use acorn_backend::scheduler::Scheduler;
    use acorn_worker::{config::Config as WorkerConfig, rpc::WorkerService};
    use tokio::{net::TcpListener, sync::oneshot};
    use tonic::{codegen::tokio_stream::wrappers::TcpListenerStream, transport::Server};

    let mut fixture = support::Fixture::new().await;
    let mut worker_config =
        WorkerConfig::parse(include_str!("../../../config/worker.example.toml")).unwrap();
    worker_config.sandbox.box_id = std::env::var("ACORN_ISOLATE_BOX_ID")
        .expect("Choose a dedicated Isolate box ID")
        .parse()
        .unwrap();
    worker_config.results.max_unacknowledged_results = 1;
    worker_config.results.acknowledged_queue_length = 1;
    let worker = WorkerService::new(worker_config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    fixture.config.scheduler.workers = vec![format!("http://{}", listener.local_addr().unwrap())];
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(
        Server::builder()
            .add_service(worker.clone().into_server())
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = stopped.await;
            }),
    );
    let (scheduler, scheduler_task) =
        Scheduler::start(&fixture.config, fixture.database.clone()).unwrap();
    let mut first_id = None;
    for (language, source, verdict) in [
        (
            "c",
            "#include <stdio.h>\nint main(void) { long long a,b; if(scanf(\"%lld%lld\",&a,&b)==2) printf(\"%lld\\n\",a+b); return 0; }",
            Verdict::AC,
        ),
        (
            "python",
            "a,b=map(int,input().split());print(a+b)",
            Verdict::AC,
        ),
        ("c", "invalid C source", Verdict::CE),
        ("python", "print(0)", Verdict::WA),
    ] {
        let permit = scheduler.reserve().unwrap();
        let created = fixture
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
        assert!(scheduler.enqueue(created.task.unwrap(), permit).unwrap());
        if first_id.is_none() {
            first_id = Some(id.clone());
        }
        let terminal = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let submission = fixture.database.submission(&id).await.unwrap().unwrap();
                if matches!(
                    submission.info.state,
                    SubmissionState::Finished | SubmissionState::Failed
                ) && !scheduler.contains(&id)
                {
                    break submission;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            terminal.info.state,
            SubmissionState::Finished,
            "{terminal:?}"
        );
        assert_eq!(terminal.info.verdict, Some(verdict), "{terminal:?}");
        if verdict == Verdict::WA {
            let Some(TerminalDetails::Case(details)) = terminal.terminal_details else {
                panic!("Expected failed case");
            };
            assert_eq!(details.failed_case.case_index, 1);
        }
    }
    let mut worker_client =
        acorn_protocol::worker::v1::judge_worker_client::JudgeWorkerClient::connect(
            fixture.config.scheduler.workers[0].clone(),
        )
        .await
        .unwrap();
    let removed = worker_client
        .get_job(acorn_protocol::worker::v1::GetJobRequest {
            job_id: first_id.unwrap(),
        })
        .await
        .unwrap_err();
    assert_eq!(removed.code(), tonic::Code::NotFound);
    scheduler.shutdown();
    scheduler_task.await.unwrap().unwrap();
    worker.shutdown().await.unwrap();
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
    fixture.database.close().await;
}
