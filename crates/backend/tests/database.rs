//! P06 acceptance using the shipped schema, seed user and example problem in real MySQL.

use acorn_backend::{
    config::Config,
    database::{Database, Error},
    scheduler::{AdmissionError, Scheduler},
    submissions::{
        CreateSubmission, SubmissionFilter, SubmissionState, TerminalDetails, TerminalOutcome,
        Verdict,
    },
};
use acorn_protocol::worker::v1::{self as proto, failed_case::Exit};
use base64::{Engine, prelude::BASE64_STANDARD};
use sqlx::{Connection, Row};
use uuid::Uuid;

#[path = "support/database.rs"]
mod database_fixture;
use database_fixture::Fixture;

/// Each application connection receives the same session contract and shares a finite pool.
#[tokio::test]
async fn mysql_pool_sessions_and_acquisition() {
    let fixture = Fixture::new().await;
    let mut config = fixture.config.database.clone();
    config.max_connections = 2;
    config.acquire_timeout_ms = 500;
    let pool = acorn_backend::database::connect_pool(&config)
        .await
        .unwrap();
    let mut first = pool.acquire().await.unwrap();
    let mut second = pool.acquire().await.unwrap();
    let mut ids = Vec::new();
    for connection in [&mut first, &mut second] {
        let row = sqlx::query("SELECT CAST(CONNECTION_ID() AS SIGNED) AS connection_id, @@character_set_connection AS charset, @@collation_connection AS collation, @@transaction_isolation AS isolation_level, @@sql_mode AS sql_mode, CAST(@@innodb_lock_wait_timeout AS SIGNED) AS row_wait, CAST(@@lock_wait_timeout AS SIGNED) AS metadata_wait")
            .fetch_one(&mut **connection).await.unwrap();
        ids.push(row.try_get::<i64, _>("connection_id").unwrap());
        assert_eq!(row.try_get::<String, _>("charset").unwrap(), "utf8mb4");
        assert_eq!(
            row.try_get::<String, _>("collation").unwrap(),
            "utf8mb4_0900_bin"
        );
        assert_eq!(
            row.try_get::<String, _>("isolation_level").unwrap(),
            "READ-COMMITTED"
        );
        let modes = row.try_get::<String, _>("sql_mode").unwrap();
        assert!(modes.split(',').any(|mode| mode == "STRICT_ALL_TABLES"));
        assert!(
            modes
                .split(',')
                .any(|mode| mode == "NO_ENGINE_SUBSTITUTION")
        );
        assert_eq!(
            row.try_get::<i64, _>("row_wait").unwrap(),
            i64::from(config.lock_wait_timeout_secs)
        );
        assert_eq!(
            row.try_get::<i64, _>("metadata_wait").unwrap(),
            i64::from(config.lock_wait_timeout_secs)
        );
    }
    assert_ne!(ids[0], ids[1]);
    assert!(matches!(
        pool.acquire().await,
        Err(sqlx::Error::PoolTimedOut)
    ));
    drop(first);
    let restored = pool.acquire().await.unwrap();
    drop(restored);
    drop(second);
    pool.close().await;
    fixture.close().await;
}

/// Create raw source without trimming or newline normalization.
fn request(language: &str) -> CreateSubmission {
    CreateSubmission {
        problem_id: 1,
        language_id: language.into(),
        source: b"// source\r\n\t\n".to_vec(),
        request_key: Uuid::new_v4(),
    }
}

/// Binary-collated text remains a checked String, while true binary data stays bytes.
#[tokio::test]
async fn mysql_binary_collation_preserves_text_type_checks() {
    let fixture = Fixture::new().await;
    let submitted = request("c");
    let created = fixture
        .database
        .create_submission(submitted.clone())
        .await
        .unwrap();
    let mut connection = fixture.connection().await;
    let row = sqlx::query("SELECT v.title, v.statement, s.id, s.source, s.verdict FROM submissions s JOIN problem_versions v USING (problem_id, version_no) WHERE s.id = ?")
        .bind(&created.info.id).fetch_one(&mut connection).await.unwrap();
    assert!(!row.try_get::<String, _>("title").unwrap().is_empty());
    assert!(!row.try_get::<String, _>("statement").unwrap().is_empty());
    assert_eq!(row.try_get::<String, _>("id").unwrap(), created.info.id);
    assert!(
        row.try_get::<Option<String>, _>("verdict")
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        row.try_get::<String, _>("verdict"),
        Err(sqlx::Error::ColumnDecode { .. })
    ));
    assert_eq!(
        row.try_get::<Vec<u8>, _>("source").unwrap(),
        submitted.source
    );
    assert!(matches!(
        row.try_get::<String, _>("source"),
        Err(sqlx::Error::ColumnDecode { .. })
    ));
    connection.close().await.unwrap();
    fixture.close().await;
}

/// Valid AC terminal with no duplicate payload.
fn accepted(config: &Config) -> TerminalOutcome {
    TerminalOutcome::judged(
        &proto::JudgeResult {
            verdict: proto::Verdict::Accepted as i32,
            ..Default::default()
        },
        104,
        &config.results,
    )
    .unwrap()
}

/// Current queries move forward while old submissions and duplicate keys stay on their fixed version.
#[tokio::test]
async fn published_queries_and_fixed_version_creation() {
    let fixture = Fixture::new().await;
    let database = &fixture.database;
    let list = database.list_problems(20, 0).await.unwrap();
    assert_eq!(list.len(), 1);
    let original = database.problem(1).await.unwrap().unwrap();
    assert_eq!(original.summary.version_no, 1);
    assert_eq!(
        original
            .languages
            .iter()
            .map(|language| language.language_id.as_str())
            .collect::<Vec<_>>(),
        ["c", "python"]
    );
    assert_eq!(original.languages[0].limits.cpu_time_ms, 1000);
    assert_eq!(original.languages[0].limits.memory_kib, 256 * 1024);
    assert!(database.problem_version(2, 1).await.unwrap().is_none());
    let submission_request = request("c");
    let created = database
        .create_submission(submission_request.clone())
        .await
        .unwrap();
    let task = created.task.unwrap();
    assert_eq!(task.request.job_id, created.info.id);
    assert_eq!(task.request.source, submission_request.source);
    assert_eq!(task.request.cases.len(), 104);
    assert_eq!(
        task.request.run_limits.as_ref().unwrap().memory_kib,
        256 * 1024
    );
    assert_eq!(task.request.cases[2].input, b"-3 5\n");
    let stored = database
        .submission(&created.info.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.user_id, 1);
    assert_eq!(stored.source, submission_request.source);
    assert_eq!(stored.info.state, SubmissionState::Queued);

    let mut connection = fixture.connection().await;
    sqlx::raw_sql("START TRANSACTION;
        INSERT INTO problem_versions (problem_id, version_no, title, statement, comparison_mode, published_at_ms) SELECT problem_id, 2, 'Version two', statement, comparison_mode, published_at_ms + 1 FROM problem_versions WHERE problem_id = 1 AND version_no = 1;
        INSERT INTO problem_version_languages SELECT problem_id, 2, language_id, cpu_time_ms, wall_time_ms, 1 FROM problem_version_languages WHERE problem_id = 1 AND version_no = 1 AND language_id = 'c';
        INSERT INTO test_cases VALUES (1, 2, 1, X'00FF010A', X'FF000A');
        UPDATE problems SET current_version_no = 2 WHERE id = 1;
        INSERT INTO problems VALUES (2, NULL, 0);
        COMMIT;").execute(&mut connection).await.unwrap();
    connection.close().await.unwrap();
    assert_eq!(
        database
            .problem(1)
            .await
            .unwrap()
            .unwrap()
            .summary
            .version_no,
        2
    );
    assert_eq!(
        database.problem_version(1, 1).await.unwrap().unwrap(),
        original
    );
    assert_eq!(database.list_problems(20, 0).await.unwrap().len(), 1);
    assert!(database.problem(2).await.unwrap().is_none());
    let repeated = database
        .create_submission(submission_request)
        .await
        .unwrap();
    assert!(!repeated.created);
    assert_eq!(repeated.info.id, created.info.id);
    assert_eq!(repeated.info.version_no, 1);
    assert_eq!(repeated.task.unwrap().request.cases.len(), 104);
    let new = database.create_submission(request("c")).await.unwrap();
    assert_eq!(new.info.version_no, 2);
    let task = new.task.unwrap();
    assert_eq!(task.request.run_limits.as_ref().unwrap().memory_kib, 1);
    assert_eq!(task.request.cases[0].input, [0, 255, 1, 10]);
    assert_eq!(task.request.cases[0].expected_output, [255, 0, 10]);
    assert_eq!(
        database
            .queued_task(&created.info.id)
            .await
            .unwrap()
            .unwrap()
            .version_no,
        1
    );
    assert!(matches!(
        database.create_submission(request("python")).await,
        Err(Error::LanguageNotAllowed)
    ));
    let mut unpublished = request("c");
    unpublished.problem_id = 2;
    assert!(matches!(
        database.create_submission(unpublished).await,
        Err(Error::ProblemNotFound)
    ));
    fixture.close().await;
}

/// Concurrent requests share one identity, and different content cannot claim its key.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_idempotency_and_conflict() {
    let fixture = Fixture::new().await;
    assert!(fixture.config.database.max_connections >= 2);
    let mut connection = fixture.connection().await;
    let mut gate = connection.begin().await.unwrap();
    sqlx::query("SELECT id FROM users WHERE id = 1 FOR UPDATE")
        .execute(&mut *gate)
        .await
        .unwrap();
    let submitted = request("python");
    let mut tasks = vec![];
    for index in 0..8 {
        let database = fixture.database.clone();
        let mut request = submitted.clone();
        if index % 2 == 1 {
            request.source.push(b'x');
        }
        tasks.push(tokio::spawn(async move {
            database.create_submission(request).await
        }));
    }
    fixture
        .wait_for_queries(
            &mut gate,
            "INSERT INTO submissions",
            fixture.config.database.max_connections.min(8) as usize,
        )
        .await;
    gate.commit().await.unwrap();
    connection.close().await.unwrap();
    let mut inserted = 0;
    let mut conflicts = 0;
    let mut ids = vec![];
    for task in tasks {
        match task.await.unwrap() {
            Ok(result) => {
                inserted += usize::from(result.created);
                ids.push(result.info.id);
            }
            Err(Error::IdempotencyConflict) => conflicts += 1,
            Err(error) => panic!("Unexpected concurrent creation error: {error}"),
        }
    }
    assert_eq!(inserted, 1);
    assert_eq!(ids.len(), 4);
    assert_eq!(conflicts, 4);
    assert!(ids.iter().all(|id| id == &ids[0]));
    let stored = fixture
        .database
        .submission_by_key(submitted.request_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.info.id, ids[0]);
    let mut conflict = submitted;
    conflict.source = stored.source;
    conflict.source.push(b'x');
    assert!(matches!(
        fixture.database.create_submission(conflict).await,
        Err(Error::IdempotencyConflict)
    ));
    assert_eq!(
        fixture
            .database
            .list_submissions(&SubmissionFilter::default(), 100, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    tokio::select! {
        biased;
        error = fixture.database.failure() => panic!("Idempotency must not notify fatal failure: {error}"),
        () = std::future::ready(()) => {}
    }
    fixture.close().await;
}

/// A duplicate on an unrelated unique key preserves error 1062 and notifies supervision.
#[tokio::test]
async fn unrelated_unique_conflict_remains_fatal() {
    let fixture = Fixture::new().await;
    let original = fixture
        .database
        .create_submission(request("c"))
        .await
        .unwrap();
    let mut connection = fixture.connection().await;
    sqlx::query("CREATE UNIQUE INDEX test_source_unique ON submissions (source(64))")
        .execute(&mut connection)
        .await
        .unwrap();
    let Error::Sql(error) = fixture
        .database
        .create_submission(request("c"))
        .await
        .unwrap_err()
    else {
        panic!("An unrelated duplicate must remain a storage error");
    };
    assert_eq!(
        error
            .as_database_error()
            .unwrap()
            .downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
            .number(),
        1062
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        fixture.database.failure(),
    )
    .await
    .unwrap();
    let stored = fixture
        .database
        .list_submissions(&SubmissionFilter::default(), 100, 0)
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].id, original.info.id);
    connection.close().await.unwrap();
    fixture.close().await;
}

/// Major result shapes round-trip without losing binary bytes, absence, or explicit zero values.
#[tokio::test]
async fn terminal_storage_and_immutability() {
    let fixture = Fixture::new().await;
    let config = &fixture.config;
    let database = &fixture.database;
    let ce_bytes = [0, 255, 128, b'\n'].repeat(16_384);
    let ce = TerminalOutcome::judged(
        &proto::JudgeResult {
            verdict: proto::Verdict::CompileError as i32,
            compile_diagnostic: ce_bytes.clone(),
            compile_diagnostic_truncated: true,
            failed_case: None,
        },
        4,
        &config.results,
    )
    .unwrap();
    let wa = TerminalOutcome::judged(
        &proto::JudgeResult {
            verdict: proto::Verdict::WrongAnswer as i32,
            failed_case: Some(proto::FailedCase {
                case_index: 2,
                stdout: vec![0, 255],
                stderr: vec![128, 10],
                resources: Some(proto::ResourceUsage {
                    cpu_time_us: Some(0),
                    wall_time_us: None,
                    peak_memory_bytes: Some(42),
                }),
                exit: Some(Exit::ExitCode(0)),
                ..Default::default()
            }),
            ..Default::default()
        },
        4,
        &config.results,
    )
    .unwrap();
    let failure = TerminalOutcome::failed(
        &proto::JobFailure {
            code: "JobDeadlineExceeded".into(),
            diagnostic: "Job deadline exceeded".into(),
        },
        &config.results,
    )
    .unwrap();
    let mut stored_results = vec![];
    for (index, terminal) in [accepted(config), ce, wa, failure.clone()]
        .into_iter()
        .enumerate()
    {
        let submitted = request("c");
        let created = database.create_submission(submitted.clone()).await.unwrap();
        if matches!(index, 1 | 2) {
            assert!(database.mark_dispatching(&created.info.id).await.unwrap());
        }
        if index == 2 {
            assert!(database.mark_running(&created.info.id).await.unwrap());
            assert!(!database.mark_running(&created.info.id).await.unwrap());
        }
        assert!(
            database
                .save_terminal(&created.info.id, &terminal, None)
                .await
                .unwrap()
        );
        let stored = database
            .submission(&created.info.id)
            .await
            .unwrap()
            .unwrap();
        assert!(stored.info.terminal_at_ms.is_some());
        assert!(
            !database
                .save_terminal(&created.info.id, &failure, None)
                .await
                .unwrap()
        );
        assert!(!database.requeue(&created.info.id).await.unwrap());
        assert!(!database.mark_dispatching(&created.info.id).await.unwrap());
        assert!(!database.mark_running(&created.info.id).await.unwrap());
        assert_eq!(
            database
                .submission(&created.info.id)
                .await
                .unwrap()
                .unwrap(),
            stored
        );
        assert!(
            database
                .queued_task(&created.info.id)
                .await
                .unwrap()
                .is_none()
        );
        let repeated = database.create_submission(submitted).await.unwrap();
        assert!(!repeated.created && repeated.task.is_none());
        stored_results.push(stored);
    }
    assert_eq!(stored_results[0].info.verdict, Some(Verdict::AC));
    assert!(stored_results[0].terminal_details.is_none());
    let Some(TerminalDetails::Compile(compile)) = &stored_results[1].terminal_details else {
        panic!("Expected CE details")
    };
    assert_eq!(
        BASE64_STANDARD
            .decode(&compile.compile_diagnostic_base64)
            .unwrap(),
        ce_bytes
    );
    assert!(compile.compile_diagnostic_truncated);
    let Some(TerminalDetails::Case(case)) = &stored_results[2].terminal_details else {
        panic!("Expected failed case")
    };
    assert_eq!(
        BASE64_STANDARD
            .decode(&case.failed_case.stdout_base64)
            .unwrap(),
        [0, 255]
    );
    assert_eq!(
        case.failed_case.resources.as_ref().unwrap().cpu_time_us,
        Some(0)
    );
    assert_eq!(
        case.failed_case.resources.as_ref().unwrap().wall_time_us,
        None
    );
    assert_eq!(case.failed_case.exit_code, Some(0));
    assert_eq!(case.failed_case.signal, None);
    let json = serde_json::to_value(&stored_results[2].terminal_details).unwrap();
    assert!(json.get("verdict").is_none());
    assert!(
        json["failed_case"]["resources"]
            .get("wall_time_us")
            .is_none()
    );
    assert!(json["failed_case"].get("signal").is_none());
    assert_eq!(stored_results[3].info.state, SubmissionState::Failed);
    assert_eq!(stored_results[3].info.verdict, None);
    assert_eq!(
        database
            .list_submissions(
                &SubmissionFilter {
                    verdict: Some(Verdict::CE),
                    ..Default::default()
                },
                20,
                0
            )
            .await
            .unwrap()
            .len(),
        1
    );
    fixture.close().await;
}

/// Startup operations preserve queued and terminal records while failing only old in-flight work.
#[tokio::test]
async fn startup_reads_and_conditional_transitions() {
    let fixture = Fixture::new().await;
    let database = &fixture.database;
    let queued = database.create_submission(request("c")).await.unwrap().info;
    let dispatching = database.create_submission(request("c")).await.unwrap().info;
    let running = database
        .create_submission(request("python"))
        .await
        .unwrap()
        .info;
    let finished = database.create_submission(request("c")).await.unwrap().info;
    assert!(database.mark_dispatching(&dispatching.id).await.unwrap());
    assert!(database.requeue(&dispatching.id).await.unwrap());
    assert!(database.mark_dispatching(&dispatching.id).await.unwrap());
    assert!(database.mark_running(&running.id).await.unwrap());
    assert!(!database.mark_running(&running.id).await.unwrap());
    assert!(!database.mark_dispatching(&running.id).await.unwrap());
    assert!(database.mark_dispatching(&finished.id).await.unwrap());
    assert!(
        database
            .save_terminal(&finished.id, &accepted(&fixture.config), None)
            .await
            .unwrap()
    );
    let old_terminal = database.submission(&finished.id).await.unwrap();
    let startup = database.queued_tasks().await.unwrap();
    assert_eq!(startup.len(), 1);
    assert_eq!(startup[0].request.job_id, queued.id);
    assert_eq!(database.interrupt_in_flight().await.unwrap(), 2);
    assert_eq!(database.interrupt_in_flight().await.unwrap(), 0);
    assert_eq!(
        database.submission(&finished.id).await.unwrap(),
        old_terminal
    );
    for id in [&dispatching.id, &running.id] {
        let stored = database.submission(id).await.unwrap().unwrap();
        let Some(TerminalDetails::Failure(failure)) = stored.terminal_details else {
            panic!("Expected interruption")
        };
        assert_eq!(failure.code, "SchedulerInterrupted");
    }
    let another = database.create_submission(request("c")).await.unwrap().info;
    let mut expected = [queued, another];
    expected.sort_by(|a, b| (a.created_at_ms, &a.id).cmp(&(b.created_at_ms, &b.id)));
    assert_eq!(
        database
            .queued_tasks()
            .await
            .unwrap()
            .iter()
            .map(|task| task.request.job_id.clone())
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|info| info.id.clone())
            .collect::<Vec<_>>()
    );
    let mut small = fixture.config.clone();
    small.scheduler.pending_max_count = 1;
    let limited = Database::open(&small).await.unwrap();
    assert!(matches!(limited.queued_tasks().await, Err(Error::Data(_))));
    limited.close().await;
    fixture.close().await;
}

/// Recovery counts queued and delivered work together and releases only committed ownership.
#[tokio::test]
async fn judging_recovery_and_once_only_permit_release() {
    let fixture = Fixture::new().await;
    let queued = fixture
        .database
        .create_submission(request("c"))
        .await
        .unwrap();
    let dispatching = fixture
        .database
        .create_submission(request("python"))
        .await
        .unwrap();
    let running = fixture
        .database
        .create_submission(request("c"))
        .await
        .unwrap();
    assert!(
        fixture
            .database
            .mark_dispatching(&dispatching.info.id)
            .await
            .unwrap()
    );
    assert!(
        fixture
            .database
            .mark_running(&running.info.id)
            .await
            .unwrap()
    );

    let mut config = fixture.config.clone();
    config.scheduler.pending_max_count = 1;
    let limited = Database::open(&config).await.unwrap();
    assert_eq!(limited.queued_tasks().await.unwrap().len(), 1);
    assert!(matches!(
        limited.judging_recovery().await,
        Err(Error::Data(_))
    ));
    let recovery = fixture.database.judging_recovery().await.unwrap();
    assert!(Scheduler::restore(&config, recovery).is_err());
    limited.close().await;

    config.scheduler.pending_max_count = 2;
    let database = Database::open(&config).await.unwrap();
    let recovery = database.judging_recovery().await.unwrap();
    assert_eq!(recovery.queued, [queued.task.clone().unwrap()]);
    assert_eq!(
        recovery.dispatching,
        std::slice::from_ref(&dispatching.info.id)
    );
    assert_eq!(recovery.running, std::slice::from_ref(&running.info.id));
    let scheduler = Scheduler::restore(&config, recovery).unwrap();
    assert_eq!(scheduler.snapshot().pending_used, 2);
    for id in [&queued.info.id, &dispatching.info.id, &running.info.id] {
        assert!(scheduler.contains(id));
    }
    assert!(matches!(
        scheduler.reserve(),
        Err(AdmissionError::QueueFull)
    ));

    assert!(
        scheduler
            .mark_running(&database, &queued.info.id)
            .await
            .unwrap()
    );
    assert!(
        !scheduler
            .mark_running(&database, &queued.info.id)
            .await
            .unwrap()
    );
    assert!(!database.mark_dispatching(&queued.info.id).await.unwrap());
    assert_eq!(scheduler.snapshot().pending_used, 1);
    let permit = scheduler.reserve().unwrap();
    assert!(!scheduler.enqueue(queued.task.unwrap(), permit).unwrap());
    assert_eq!(scheduler.snapshot().pending_used, 1);
    assert!(scheduler.contains(&queued.info.id));

    // A failed critical write must leave the delivered job's permit owned.
    database.close().await;
    assert!(
        scheduler
            .mark_running(&database, &dispatching.info.id)
            .await
            .is_err()
    );
    assert_eq!(scheduler.snapshot().pending_used, 1);
    let terminal = accepted(&config);
    assert!(
        scheduler
            .save_terminal(&fixture.database, &dispatching.info.id, &terminal, None)
            .await
            .unwrap()
    );
    assert!(
        !scheduler
            .save_terminal(&fixture.database, &dispatching.info.id, &terminal, None)
            .await
            .unwrap()
    );
    assert_eq!(scheduler.snapshot().pending_used, 0);
    scheduler.complete(&dispatching.info.id);
    scheduler.complete(&dispatching.info.id);
    assert!(!scheduler.contains(&dispatching.info.id));
    for id in [&queued.info.id, &running.info.id] {
        assert!(
            scheduler
                .save_terminal(&fixture.database, id, &terminal, None)
                .await
                .unwrap()
        );
        scheduler.complete(id);
        assert!(!scheduler.contains(id));
    }
    assert_eq!(scheduler.snapshot().pending_used, 0);
    fixture.close().await;
}

/// Invalid deployment data rolls back creation, and application writes enforce user foreign keys.
#[tokio::test]
async fn invalid_task_data_and_foreign_keys() {
    let fixture = Fixture::new().await;
    let mut connection = fixture.connection().await;
    sqlx::query("DELETE FROM test_cases WHERE case_index = 2")
        .execute(&mut connection)
        .await
        .unwrap();
    assert!(matches!(
        fixture.database.create_submission(request("c")).await,
        Err(Error::Data(_))
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM submissions")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::query("INSERT INTO test_cases VALUES (1, 1, 2, X'', X'')")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = 1")
        .execute(&mut connection)
        .await
        .unwrap();
    let Error::Sql(sqlx::Error::Database(error)) = fixture
        .database
        .create_submission(request("c"))
        .await
        .unwrap_err()
    else {
        panic!("Expected SQL foreign-key violation")
    };
    assert!(error.is_foreign_key_violation());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM submissions")
            .fetch_one(&mut connection)
            .await
            .unwrap(),
        0
    );
    connection.close().await.unwrap();
    fixture.close().await;
}

/// Existing-database, schema, user and finite-lock-wait requirements report actual errors.
#[tokio::test]
async fn startup_checks_and_finite_lock_wait() {
    let fixture = Fixture::new().await;
    let mut missing = fixture.config.clone();
    missing.database.name = format!("{}missing", fixture.config.database.name);
    assert!(matches!(Database::open(&missing).await, Err(Error::Sql(_))));
    let mut connection = fixture.connection().await;
    let absent: Option<String> = sqlx::query_scalar(
        "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?",
    )
    .bind(&missing.database.name)
    .fetch_optional(&mut connection)
    .await
    .unwrap();
    assert!(absent.is_none());
    missing.database.name = fixture.config.database.name.clone();
    missing.test_user.user_id = 999;
    assert!(matches!(
        Database::open(&missing).await,
        Err(Error::Data(_))
    ));
    sqlx::query("UPDATE schema_version SET version = 1 WHERE id = 1")
        .execute(&mut connection)
        .await
        .unwrap();
    let error = match Database::open(&fixture.config).await {
        Err(error) => error,
        Ok(_) => panic!("Unexpected schema version must be rejected"),
    };
    assert!(
        error
            .to_string()
            .contains("Expected schema version 4, got 1")
    );
    sqlx::query("DELETE FROM schema_version WHERE id = 1")
        .execute(&mut connection)
        .await
        .unwrap();
    assert!(matches!(
        Database::open(&fixture.config).await,
        Err(Error::Data(_))
    ));
    sqlx::query("INSERT INTO schema_version (id, version) VALUES (1, 4)")
        .execute(&mut connection)
        .await
        .unwrap();
    let mut fast = fixture.config.clone();
    fast.database.lock_wait_timeout_secs = 1;
    let database = Database::open(&fast).await.unwrap();
    sqlx::raw_sql("START TRANSACTION")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id = 1 FOR UPDATE")
        .execute(&mut connection)
        .await
        .unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        database.create_submission(request("c")),
    )
    .await
    .unwrap();
    let Err(Error::Sql(error)) = result else {
        panic!("An expired write lock must report a storage error");
    };
    assert_eq!(
        error
            .as_database_error()
            .unwrap()
            .downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
            .number(),
        1205
    );
    tokio::time::timeout(std::time::Duration::from_secs(1), database.failure())
        .await
        .unwrap();
    sqlx::raw_sql("ROLLBACK")
        .execute(&mut connection)
        .await
        .unwrap();
    assert!(
        database
            .create_submission(request("c"))
            .await
            .unwrap()
            .created
    );
    database.close().await;
    connection.close().await.unwrap();
    fixture.close().await;
}

/// The P06 executable validates an initialized database and clearly reports pending services.
#[tokio::test]
async fn executable_serves_http_and_stops() {
    let fixture = Fixture::new().await;
    use std::{process::Stdio, time::Duration};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let mut config = fixture.process_config();
    config["http"]["listen"] = "127.0.0.1:0".into();
    let text = toml::to_string(&config).unwrap();
    let path = fixture.directory.path().join("backend.toml");
    std::fs::write(&path, text).unwrap();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_acorn-backend"))
        .arg("--config")
        .arg(path)
        .env("MYSQL_PASSWORD", std::env::var("MYSQL_PASSWORD").unwrap())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut logs = BufReader::new(child.stderr.take().unwrap()).lines();
    let address = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(line) = logs.next_line().await.unwrap() {
            if line.contains("Backend HTTP listening") {
                return line
                    .split("listen=")
                    .nth(1)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .to_owned();
            }
        }
        panic!("Backend did not start listening")
    })
    .await
    .unwrap();
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /api/v1/problems HTTP/1.0\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_string(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.starts_with("HTTP/1.0 200"));
    assert!(response.contains("\"items\""));
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id().unwrap() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .code(),
        Some(0)
    );
    let mut connection = fixture.connection().await;
    let hash: String = sqlx::query("SELECT password_hash FROM users WHERE id = 1")
        .fetch_one(&mut connection)
        .await
        .unwrap()
        .try_get("password_hash")
        .unwrap();
    assert!(hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
    let parts: Vec<_> = hash.split('$').collect();
    assert_eq!(
        base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(parts[4])
            .unwrap()
            .len(),
        16
    );
    assert_eq!(
        base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(parts[5])
            .unwrap()
            .len(),
        32
    );
    connection.close().await.unwrap();
    fixture.close().await;
}

/// Build local metadata without starting any external request or queue.
fn pending_review(
    trigger: acorn_backend::llm_review::ReviewTrigger,
) -> acorn_backend::llm_review::ReviewAttempt {
    acorn_backend::llm_review::ReviewAttempt::pending(
        trigger,
        100,
        "deepseek-flash".into(),
        "max".into(),
        "review-policy-v1".into(),
    )
    .unwrap()
}

/// Latest-only persistence rejects old completions, preserves machine results and combines filters.
#[tokio::test]
async fn latest_review_persistence_and_rerun() {
    use acorn_backend::llm_review::{
        FindingCategory, ReviewDecision, ReviewErrorCode, ReviewFinding, ReviewResult,
        ReviewStatus, ReviewTrigger,
    };
    let fixture = Fixture::new().await;
    let database = &fixture.database;
    let created = database.create_submission(request("python")).await.unwrap();
    let id = &created.info.id;
    assert_eq!(created.info.review_status, ReviewStatus::Disabled);
    assert!(
        database
            .submission(id)
            .await
            .unwrap()
            .unwrap()
            .review
            .is_none()
    );
    assert!(database.review_input(id).await.unwrap().is_none());
    let first = pending_review(ReviewTrigger::Auto);
    assert!(
        database
            .save_terminal(id, &accepted(&fixture.config), Some(&first))
            .await
            .unwrap()
    );
    let machine = database.submission(id).await.unwrap().unwrap();
    assert!(
        !database
            .save_terminal(
                id,
                &accepted(&fixture.config),
                Some(&pending_review(ReviewTrigger::Auto))
            )
            .await
            .unwrap()
    );
    assert_eq!(machine.review.as_ref(), Some(&first));
    assert_eq!(machine.info.verdict, Some(Verdict::AC));
    assert!(machine.terminal_details.is_none());
    let input = database.review_input(id).await.unwrap().unwrap();
    assert_eq!(input.source.as_bytes(), request("python").source);
    assert_eq!(input.language_id, "python");
    assert_eq!(input.verdict, Verdict::AC);
    assert_eq!(
        input.title,
        database
            .problem_version(1, 1)
            .await
            .unwrap()
            .unwrap()
            .summary
            .title
    );
    let first_done = first
        .clone()
        .succeeded(
            200,
            ReviewResult {
                decision: ReviewDecision::Pass,
                summary: "未发现约定范围内的疑点".into(),
                findings: vec![],
            },
            Some("response-1".into()),
            None,
            input.source.lines().count(),
        )
        .unwrap();
    assert!(database.finish_review(id, &first_done).await.unwrap());
    assert!(!database.finish_review(id, &first_done).await.unwrap());
    let second = pending_review(ReviewTrigger::Manual);
    assert!(!database.start_review(id, None, &second).await.unwrap());
    assert!(
        database
            .start_review(id, Some(&first.id), &second)
            .await
            .unwrap()
    );
    assert!(
        !database
            .start_review(id, Some(&second.id), &pending_review(ReviewTrigger::Manual))
            .await
            .unwrap()
    );
    assert!(!database.finish_review(id, &first_done).await.unwrap());
    let rerun = database.submission(id).await.unwrap().unwrap();
    assert_eq!(rerun.review.as_ref(), Some(&second));
    assert_eq!(rerun.info.terminal_at_ms, machine.info.terminal_at_ms);
    assert_eq!(rerun.info.verdict, machine.info.verdict);
    assert_eq!(rerun.terminal_details, machine.terminal_details);
    let diagnostic = serde_json::json!({"decision":"PASS","missing":"findings"});
    let failed = second
        .failed(
            300,
            ReviewErrorCode::ReviewInvalidOutput,
            None,
            None,
            Some(diagnostic),
        )
        .unwrap();
    assert!(database.finish_review(id, &failed).await.unwrap());
    assert_eq!(
        database.submission(id).await.unwrap().unwrap().review,
        Some(failed.clone())
    );
    assert!(
        !database
            .save_terminal(id, &accepted(&fixture.config), None)
            .await
            .unwrap()
    );
    let filter = SubmissionFilter {
        problem_id: Some(1),
        state: Some(SubmissionState::Finished),
        verdict: Some(Verdict::AC),
        review_status: Some(ReviewStatus::Failed),
    };
    assert_eq!(
        database
            .list_submissions(&filter, 20, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        database
            .list_submissions(
                &SubmissionFilter {
                    verdict: Some(Verdict::CE),
                    ..filter
                },
                20,
                0
            )
            .await
            .unwrap()
            .is_empty()
    );
    let third = pending_review(ReviewTrigger::Manual);
    assert!(
        database
            .start_review(id, Some(&failed.id), &third)
            .await
            .unwrap()
    );
    let mut opinion = ReviewResult {
        decision: ReviewDecision::Doubtful,
        summary: "需要复核固定输出".into(),
        findings: vec![ReviewFinding {
            category: FindingCategory::TestTargeting,
            line_start: 1,
            line_end: 3,
            evidence: "待核对的源码行为".into(),
            normal_explanation: "待核对公开题意".into(),
        }],
    };
    // The write boundary must use stored source, even if a caller used an incorrect count.
    let invalid = third
        .clone()
        .succeeded(400, opinion.clone(), None, None, 3)
        .unwrap();
    assert!(matches!(
        database.finish_review(id, &invalid).await,
        Err(Error::InvalidReview(_))
    ));
    assert_eq!(
        database.submission(id).await.unwrap().unwrap().review,
        Some(third.clone())
    );
    opinion.findings[0].line_end = 2;
    let doubtful = third.succeeded(400, opinion, None, None, 2).unwrap();
    assert!(database.finish_review(id, &doubtful).await.unwrap());
    assert_eq!(
        database.submission(id).await.unwrap().unwrap().review,
        Some(doubtful)
    );
    fixture.close().await;
}

/// Fixed policies, explicit interruption and write-boundary validation obey the data contract.
#[tokio::test]
async fn review_policy_interruption_and_constraints() {
    use acorn_backend::llm_review::{
        DETAILS_MAX_BYTES, ReviewErrorCode, ReviewMode, ReviewStatus, ReviewTrigger,
    };
    let fixture = Fixture::new().await;
    let database = &fixture.database;
    let original = database.create_submission(request("c")).await.unwrap();
    assert_eq!(
        original.task.as_ref().unwrap().review_mode,
        ReviewMode::Disabled
    );
    let mut connection = fixture.connection().await;
    // Publish a new version through the existing deployment-side fixture pattern.
    sqlx::raw_sql("BEGIN; INSERT INTO problem_versions (problem_id, version_no, title, statement, published_at_ms, review_mode) VALUES (1, 2, 'New title', 'New statement', 200, 'ENABLE-ANY'); INSERT INTO problem_version_languages SELECT problem_id, 2, language_id, cpu_time_ms, wall_time_ms, memory_kib FROM problem_version_languages WHERE problem_id=1 AND version_no=1; INSERT INTO test_cases SELECT problem_id, 2, case_index, input, expected_output FROM test_cases WHERE problem_id=1 AND version_no=1; UPDATE problems SET current_version_no=2 WHERE id=1; COMMIT;")
        .execute(&mut connection).await.unwrap();
    assert_eq!(
        database.problem(1).await.unwrap().unwrap().review_mode,
        ReviewMode::EnableAny
    );
    assert_eq!(
        database
            .queued_task(&original.info.id)
            .await
            .unwrap()
            .unwrap()
            .review_mode,
        ReviewMode::Disabled
    );
    let newer = database.create_submission(request("c")).await.unwrap();
    assert_eq!(newer.task.unwrap().review_mode, ReviewMode::EnableAny);
    let pending = pending_review(ReviewTrigger::Auto);
    assert!(
        !database
            .start_review(&newer.info.id, None, &pending_review(ReviewTrigger::Manual))
            .await
            .unwrap()
    );
    let mut oversized = pending.clone();
    oversized.details.model = "x".repeat(DETAILS_MAX_BYTES);
    database.mark_dispatching(&original.info.id).await.unwrap();
    assert!(matches!(
        database
            .save_terminal(
                &original.info.id,
                &accepted(&fixture.config),
                Some(&oversized)
            )
            .await,
        Err(Error::InvalidReview(_))
    ));
    assert_eq!(
        database
            .submission(&original.info.id)
            .await
            .unwrap()
            .unwrap()
            .info
            .state,
        SubmissionState::Dispatching
    );
    database
        .save_terminal(
            &original.info.id,
            &accepted(&fixture.config),
            Some(&pending),
        )
        .await
        .unwrap();
    let input = database
        .review_input(&original.info.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(input.title, "New title");
    let machine = database
        .submission(&original.info.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(database.interrupt_pending_reviews().await.unwrap(), 1);
    assert_eq!(database.interrupt_pending_reviews().await.unwrap(), 0);
    let stored = database
        .submission(&original.info.id)
        .await
        .unwrap()
        .unwrap();
    let interrupted = stored.review.unwrap();
    assert_eq!(interrupted.id, pending.id);
    assert_eq!(interrupted.status, ReviewStatus::Failed);
    assert_eq!(
        interrupted.details.requested_at_ms,
        pending.details.requested_at_ms
    );
    assert_eq!(interrupted.details.model, pending.details.model);
    assert!(interrupted.details.completed_at_ms.is_some());
    assert_eq!(
        interrupted.details.error.unwrap().code,
        ReviewErrorCode::ReviewInterrupted
    );
    assert_eq!(stored.info.terminal_at_ms, machine.info.terminal_at_ms);
    assert_eq!(stored.info.verdict, machine.info.verdict);
    assert_eq!(stored.terminal_details, machine.terminal_details);
    for update in [
        "UPDATE submissions SET review_status='PASS' WHERE state='queued'",
        "UPDATE submissions SET review_details_json='[]' WHERE review_status='FAILED'",
        "UPDATE submissions SET review_status='DISABLED' WHERE review_status='FAILED'",
        "UPDATE problem_versions SET review_mode='UNKNOWN' WHERE version_no=1",
    ] {
        assert!(sqlx::query(update).execute(&mut connection).await.is_err());
    }
    connection.close().await.unwrap();
    fixture.close().await;
}
