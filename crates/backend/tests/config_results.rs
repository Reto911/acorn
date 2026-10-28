//! Focused configuration and terminal-shape boundary checks.

use acorn_backend::scheduler::kafka;
use acorn_backend::{config::Config, database::Error, submissions::TerminalOutcome};
use acorn_protocol::worker::v1::{FailedCase, JudgeResult, Verdict};

/// Cookie deployment settings and session ceilings enforce the documented startup contract.
#[test]
fn authentication_configuration_contract() {
    let example = include_str!("../../../config/backend.example.toml");
    let defaulted = Config::parse(&example.replace("cookie_secure = false\n", "").replace(
        "[auth]\nmax_sessions_per_user = 10\nmax_sessions = 10_000\n",
        "",
    ))
    .unwrap();
    assert!(!defaulted.http.cookie_secure);
    assert_eq!(defaulted.auth.max_sessions_per_user, 10);
    assert_eq!(defaulted.auth.max_sessions, 10_000);
    let https = example
        .replace(
            "public_origin = \"http://127.0.0.1:8080\"",
            "public_origin = \"https://acorn.example\"",
        )
        .replace("cookie_secure = false", "cookie_secure = true");
    assert!(Config::parse(&https).unwrap().http.cookie_secure);
    for origin in [
        "null",
        "file:///tmp/page",
        "https://user@acorn.example",
        "https://acorn.example/",
        "https://acorn.example/path",
        "https://acorn.example?x=1",
        "https://acorn.example#fragment",
    ] {
        assert!(
            Config::parse(&example.replace(
                "public_origin = \"http://127.0.0.1:8080\"",
                &format!("public_origin = {origin:?}")
            ))
            .is_err()
        );
    }
    for field in ["max_sessions_per_user = 10", "max_sessions = 10_000"] {
        assert!(
            Config::parse(&example.replace(
                field,
                &format!("{} = 0", field.split_once(" = ").unwrap().0)
            ))
            .is_err()
        );
    }
    assert!(
        Config::parse(&example.replace("public_origin = \"http://127.0.0.1:8080\"\n", "")).is_err()
    );
}

/// Membership freshness is configured independently from health and has finite SDK budgets.
#[test]
fn worker_membership_defaults_and_bounds() {
    use acorn_backend::config::WorkerMembershipConfig;
    let example = include_str!("../../../config/backend.example.toml");
    let config = Config::parse(example.split("[worker_membership]").next().unwrap()).unwrap();
    assert_eq!(
        config.worker_membership.stale_after().unwrap().as_millis(),
        12_000
    );
    let mut config = config;
    config.health.interval_ms = 99;
    assert_eq!(config.worker_membership.interval_ms, 5_000);
    for settings in [
        WorkerMembershipConfig {
            tasks_group_id: String::new(),
            ..Default::default()
        },
        WorkerMembershipConfig {
            interval_ms: 0,
            timeout_ms: 1,
            ..Default::default()
        },
        WorkerMembershipConfig {
            interval_ms: 1,
            timeout_ms: 0,
            ..Default::default()
        },
        WorkerMembershipConfig {
            interval_ms: 1,
            timeout_ms: i32::MAX as u64 + 1,
            ..Default::default()
        },
        WorkerMembershipConfig {
            interval_ms: u64::MAX,
            timeout_ms: 1,
            ..Default::default()
        },
    ] {
        assert!(settings.validate().is_err());
    }
}

/// The example loads; invalid JSON budgets and duplicate endpoints fail at startup.
#[test]
fn configuration_relationships() {
    let example = include_str!("../../../config/backend.example.toml");
    Config::parse(example).unwrap();
    assert!(
        Config::parse(&example.replace("max_request_bytes = 524_288", "max_request_bytes = 1"))
            .is_err()
    );
    assert!(
        Config::parse(&example.replace(
            "workers = [\"http://127.0.0.1:50051\"]",
            "workers = [\"http://127.0.0.1:50051\", \"http://127.0.0.1:50051\"]"
        ))
        .is_err()
    );
}

/// MySQL endpoint syntax, connection budgets and removal of SQLite fields are enforced.
#[test]
fn mysql_configuration_contract() {
    let example = include_str!("../../../config/backend.example.toml");
    let defaulted = Config::parse(&example.replace("max_connections = 4\n", "")).unwrap();
    assert_eq!(defaulted.database.max_connections, 4);
    for endpoint in ["db.internal:3306", "[::1]:3306"] {
        let config = Config::parse(&example.replace("127.0.0.1:3306", endpoint)).unwrap();
        assert_eq!(config.database.address().unwrap().1, 3306);
    }
    for endpoint in [
        "localhost",
        "localhost:0",
        "localhost:65536",
        "::1:3306",
        "mysql://localhost:3306",
        ":3306",
    ] {
        assert!(
            Config::parse(&example.replace("127.0.0.1:3306", endpoint)).is_err(),
            "{endpoint}"
        );
    }
    for (field, invalid) in [
        ("name = \"appdb\"", "name = \"\""),
        ("username = \"example_user\"", "username = \" \""),
        ("max_connections = 4", "max_connections = 0"),
        ("acquire_timeout_ms = 5_000", "acquire_timeout_ms = 0"),
        ("lock_wait_timeout_secs = 3", "lock_wait_timeout_secs = 0"),
        (
            "lock_wait_timeout_secs = 3",
            "lock_wait_timeout_secs = 31_536_001",
        ),
        ("name = \"appdb\"", "path = \"acorn.db\""),
        ("acquire_timeout_ms = 5_000", "busy_timeout_ms = 3_000"),
    ] {
        assert!(
            Config::parse(&example.replace(field, invalid)).is_err(),
            "{invalid}"
        );
    }
    let secret = "MYSQL_TEST_SECRET_MUST_NOT_APPEAR";
    let invalid = example.replace("name = \"appdb\"", &format!("password = \"{secret}\""));
    let error = Config::parse(&invalid).unwrap_err().to_string();
    assert!(!error.contains(secret));
}

/// A backend without its password fails before trying a network connection.
#[test]
fn mysql_password_is_required_at_startup() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("backend.toml");
    std::fs::write(&path, include_str!("../../../config/backend.example.toml")).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_acorn-backend"))
        .arg("--config")
        .arg(path)
        .env_remove("MYSQL_PASSWORD")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("MYSQL_PASSWORD must be present")
    );
}

/// Old and partial health settings retain defaults; invalid budgets and age overflow fail.
#[test]
fn health_configuration_defaults_and_age_limit() {
    let example = include_str!("../../../config/backend.example.toml");
    let block = "[health]\ninterval_ms = 5_000\ntimeout_ms = 1_000\n\n";
    let legacy = example.replace(block, "");
    let config = Config::parse(&legacy).unwrap();
    assert_eq!(config.health.interval_ms, 5_000);
    assert_eq!(config.health.timeout_ms, 1_000);
    assert_eq!(config.health.stale_after().unwrap().as_millis(), 12_000);
    let partial = Config::parse(&example.replace(block, "[health]\ninterval_ms = 7\n\n")).unwrap();
    assert_eq!(partial.health.timeout_ms, 1_000);
    assert_eq!(partial.health.stale_after().unwrap().as_millis(), 2_014);
    for (field, value) in [
        ("interval_ms = 5_000", "interval_ms = 0"),
        ("timeout_ms = 1_000", "timeout_ms = 0"),
        ("timeout_ms = 1_000", "timeout_ms = 2147483648"),
        ("interval_ms = 5_000", "interval_ms = 18446744073709551615"),
        ("interval_ms = 5_000", "interval_ms = 9223372036854775807"),
    ] {
        assert!(
            Config::parse(&example.replace(field, value)).is_err(),
            "{value}"
        );
    }
}

/// Invalid terminal shapes cannot reach database serialization through validated constructors.
#[test]
fn malformed_results_are_rejected() {
    let config = Config::parse(include_str!("../../../config/backend.example.toml")).unwrap();
    let with_case = JudgeResult {
        verdict: Verdict::Accepted as i32,
        failed_case: Some(FailedCase {
            case_index: 1,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(matches!(
        TerminalOutcome::judged(&with_case, 4, &config.results),
        Err(Error::InvalidResult(_))
    ));
    let outside = JudgeResult {
        verdict: Verdict::WrongAnswer as i32,
        failed_case: Some(FailedCase {
            case_index: 5,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(matches!(
        TerminalOutcome::judged(&outside, 4, &config.results),
        Err(Error::InvalidResult(_))
    ));
    let oversized = JudgeResult {
        verdict: Verdict::CompileError as i32,
        compile_diagnostic: vec![b'x'; config.results.compile_diagnostic_max_bytes + 1],
        ..Default::default()
    };
    assert!(matches!(
        TerminalOutcome::judged(&oversized, 4, &config.results),
        Err(Error::InvalidResult(_))
    ));
}

/// Legacy and partial review configuration retain defaults and enabled settings enforce budgets.
#[test]
fn review_configuration_defaults_and_validation() {
    use acorn_backend::{config::LlmReviewConfig, llm_review::client::ReviewClient};
    let example = include_str!("../../../config/backend.example.toml");
    let legacy = example.split("[llm_review]").next().unwrap();
    let config = Config::parse(legacy).unwrap();
    assert!(!config.llm_review.enabled);
    assert!(ReviewClient::new(&config.llm_review).unwrap().is_none());
    let partial = Config::parse(&format!("{legacy}\n[llm_review]\nenabled = false\n")).unwrap();
    assert_eq!(partial.llm_review.model, "deepseek-flash");
    assert_eq!(partial.llm_review.reasoning_effort, "max");
    assert_eq!(partial.llm_review.timeout_ms, 120_000);
    assert_eq!(partial.llm_review.max_concurrent, 2);
    assert_eq!(partial.llm_review.pending_max_count, 16);
    assert_eq!(partial.llm_review.max_output_tokens, 10_240);
    assert_eq!(partial.llm_review.max_request_bytes, 1_048_576);
    assert_eq!(partial.llm_review.max_response_bytes, 1_048_576);
    let enabled = LlmReviewConfig {
        enabled: true,
        api_key: "test-credential".into(),
        ..Default::default()
    };
    enabled.validate().unwrap();
    for invalid in [
        LlmReviewConfig {
            endpoint: "relative/path".into(),
            ..enabled.clone()
        },
        LlmReviewConfig {
            api_key: " ".into(),
            ..enabled.clone()
        },
        LlmReviewConfig {
            model: "".into(),
            ..enabled.clone()
        },
        LlmReviewConfig {
            reasoning_effort: "medium".into(),
            ..enabled.clone()
        },
        LlmReviewConfig {
            timeout_ms: 0,
            ..enabled.clone()
        },
        LlmReviewConfig {
            max_concurrent: 0,
            ..enabled.clone()
        },
        LlmReviewConfig {
            pending_max_count: 0,
            ..enabled.clone()
        },
        LlmReviewConfig {
            max_output_tokens: 0,
            ..enabled.clone()
        },
        LlmReviewConfig {
            max_request_bytes: 0,
            ..enabled.clone()
        },
        LlmReviewConfig {
            max_response_bytes: 0,
            ..enabled.clone()
        },
        LlmReviewConfig {
            model: "x".repeat(32_768),
            ..enabled.clone()
        },
    ] {
        assert!(invalid.validate().is_err());
    }
    assert!(
        ReviewClient::new(&LlmReviewConfig {
            api_key: "invalid\nheader".into(),
            ..enabled
        })
        .is_err()
    );
}

/// Credentials are absent from enclosing Debug output and malformed TOML error chains.
#[test]
fn review_configuration_diagnostics_hide_credentials() {
    let example = include_str!("../../../config/backend.example.toml");
    let text = example.replace(
        "api_key = \"\"",
        "api_key = \"test-secret-must-not-appear\"",
    );
    let config = Config::parse(&text).unwrap();
    assert!(!format!("{config:?}").contains("test-secret-must-not-appear"));
    let malformed = text.replace(
        "api_key = \"test-secret-must-not-appear\"",
        "api_key = [\"test-secret-must-not-appear\"]",
    );
    let error = Config::parse(&malformed).unwrap_err();
    let diagnostic = format!("{error:#}");
    assert!(diagnostic.contains("Invalid backend TOML configuration"));
    assert!(!diagnostic.contains("test-secret-must-not-appear"));
}

/// Kafka requires deployment identities and enforces finite, sufficient transport budgets.
#[test]
fn kafka_configuration_contract() {
    let example = include_str!("../../../config/backend.example.toml");
    let config = Config::parse(example).unwrap();
    let defaults = example
        .lines()
        .filter(|line| {
            ![
                "delivery_timeout_ms =",
                "queue_timeout_ms =",
                "shutdown_timeout_ms = 5_000",
                "max_task_record_bytes =",
                "max_event_record_bytes =",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let defaults = Config::parse(&defaults).unwrap();
    assert_eq!(defaults.kafka.delivery_timeout_ms, 30_000);
    assert_eq!(defaults.kafka.queue_timeout_ms, 1_000);
    assert_eq!(defaults.kafka.shutdown_timeout_ms, 5_000);
    assert_eq!(defaults.kafka.max_task_record_bytes, 16_777_216);
    assert_eq!(defaults.kafka.max_event_record_bytes, 1_048_576);
    for (before, after) in [
        (
            "bootstrap_servers = [\"localhost:9092\"]",
            "bootstrap_servers = []",
        ),
        ("localhost:9092", "http://localhost:9092"),
        ("localhost:9092", "localhost:0"),
        ("localhost:9092", "localhost:65536"),
        ("localhost:9092", "bad host:9092"),
        (
            "tasks_topic = \"acorn.judge.tasks.v1\"",
            "tasks_topic = \"\"",
        ),
        ("acorn.judge.events.v1", "acorn.judge.tasks.v1"),
        ("acorn-backend-judge-events-v1", " "),
        ("delivery_timeout_ms = 30_000", "delivery_timeout_ms = 0"),
        (
            "delivery_timeout_ms = 30_000",
            "delivery_timeout_ms = 2147483648",
        ),
        ("queue_timeout_ms = 1_000", "queue_timeout_ms = 0"),
        (
            "queue_timeout_ms = 1_000",
            "queue_timeout_ms = 1_000\ncommit_timeout_ms = 5_000",
        ),
        ("shutdown_timeout_ms = 5_000", "shutdown_timeout_ms = 0"),
        (
            "max_task_record_bytes = 16_777_216",
            "max_task_record_bytes = 1",
        ),
        (
            "max_task_record_bytes = 16_777_216",
            "max_task_record_bytes = 16_777_217",
        ),
        (
            "max_event_record_bytes = 1_048_576",
            "max_event_record_bytes = 1",
        ),
        (
            "max_event_record_bytes = 1_048_576",
            "max_event_record_bytes = 1_048_577",
        ),
    ] {
        assert!(
            Config::parse(&example.replace(before, after)).is_err(),
            "{after}"
        );
    }
    for required in [
        "bootstrap_servers",
        "tasks_topic",
        "events_topic",
        "group_id",
    ] {
        let missing = example
            .lines()
            .filter(|line| !line.starts_with(&format!("{required} =")))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(Config::parse(&missing).is_err(), "{required}");
    }
    assert!(Config::parse(&example.replace("localhost:9092", "[::1]:9092")).is_ok());
    for options in [
        kafka::producer_config(&config.kafka),
        kafka::consumer_config(&config.kafka),
    ] {
        assert_eq!(options.get("allow.auto.create.topics"), Some("false"));
        assert_eq!(options.get("security.protocol"), Some("PLAINTEXT"));
        options.create_native_config().unwrap();
    }
    let producer = kafka::producer_config(&config.kafka);
    assert_eq!(producer.get("enable.idempotence"), Some("true"));
    assert_eq!(producer.get("acks"), Some("all"));
    assert_eq!(
        producer.get("message.max.bytes"),
        Some(
            (config.kafka.max_task_record_bytes + 1024)
                .to_string()
                .as_str()
        )
    );
    let consumer = kafka::consumer_config(&config.kafka);
    for obsolete in ["events_group_id", "tasks_group_id"] {
        let text = example.replace(
            "group_id = \"acorn-backend-judge-events-v1\"",
            &format!("{obsolete} = \"acorn-backend-judge-events-v1\""),
        );
        assert!(Config::parse(&text).is_err(), "{obsolete}");
    }
    assert_eq!(
        consumer.get("group.id"),
        Some(config.kafka.group_id.as_str())
    );
    assert_eq!(consumer.get("enable.auto.commit"), Some("false"));
    assert_eq!(consumer.get("enable.auto.offset.store"), Some("false"));
    assert_eq!(consumer.get("auto.offset.reset"), Some("error"));
    assert_eq!(consumer.get("group.protocol"), Some("classic"));
    assert_eq!(
        consumer.get("fetch.message.max.bytes"),
        Some(
            (config.kafka.max_event_record_bytes + 1024)
                .to_string()
                .as_str()
        )
    );
}
