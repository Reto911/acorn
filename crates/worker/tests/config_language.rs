//! Startup and language-plan acceptance checks without sandbox execution.

use acorn_worker::{
    config::Config,
    kafka,
    language::{self, ProgramFiles},
};

/// Deployment example shared by startup and language tests.
const EXAMPLE: &str = include_str!("../../../config/worker.example.toml");

/// The example must produce one C compilation and direct isolated Python execution.
#[test]
fn example_builds_both_language_plans() {
    let config = Config::parse(EXAMPLE).unwrap();
    let c = language::plan("c", &config).unwrap();
    assert_eq!(c.source_name, "main.c");
    assert_eq!(c.program_files, ProgramFiles::CompiledArtifact);
    let compile = c.compile.unwrap();
    assert_eq!(compile.artifact_name, "program");
    assert_eq!(
        compile.command.arguments,
        ["main.c", "-std=c17", "-O2", "-o", "program"]
    );
    assert_eq!(compile.limits.process_max_count, 32);
    assert_eq!(compile.limits.memory_kib, 512 * 1024);
    assert_eq!(c.run.executable.to_str(), Some("./program"));
    let python = language::plan("python", &config).unwrap();
    assert!(python.compile.is_none());
    assert_eq!(python.source_name, "main.py");
    assert_eq!(python.program_files, ProgramFiles::Source);
    assert_eq!(python.run.arguments, ["-I", "-B", "-S", "main.py"]);
    assert_eq!(python.run.environment, config.sandbox.environment);
    assert!(language::plan("rust", &config).is_err());
}

/// An insufficient result envelope and a nonpositive execution budget block startup.
#[test]
fn invalid_resource_settings_are_explicit_errors() {
    let too_small = EXAMPLE.replace(
        "max_event_record_bytes = 1_048_576",
        "max_event_record_bytes = 100",
    );
    assert!(
        Config::parse(&too_small)
            .unwrap_err()
            .to_string()
            .contains("bounded results")
    );
    let zero = EXAMPLE.replace("job_wall_time_ms = 120_000", "job_wall_time_ms = 0");
    assert!(
        Config::parse(&zero)
            .unwrap_err()
            .to_string()
            .contains("execution.job_wall_time_ms")
    );
}

/// A missing deployment executable must fail its startup tool check.
#[test]
fn missing_tool_is_reported() {
    let mut config = Config::parse(EXAMPLE).unwrap();
    config.sandbox.executable = "/acorn/nonexistent/isolate".into();
    assert!(
        config
            .check_tools()
            .unwrap_err()
            .to_string()
            .contains("Failed to start configured tool")
    );
}

/// One KiB is a valid limit; byte-sized artifacts must still fit that file budget.
#[test]
fn kib_limits_and_artifact_bytes_use_explicit_units() {
    let mut config = Config::parse(EXAMPLE).unwrap();
    config.execution.max_run_memory_kib = 1;
    config.compile.memory_kib = 1;
    config.sandbox.file_size_max_kib = 1;
    config.sandbox.disk_quota_kib = 1;
    config.compile.artifact_max_bytes = 1024;
    config.validate().unwrap();
    config.compile.artifact_max_bytes = 1025;
    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("single-file")
    );
    config.compile.artifact_max_bytes = 1024;
    config.compile.memory_kib = 0;
    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("compile.memory_kib")
    );
    config.compile.memory_kib = i32::MAX as u64 + 1;
    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("supported KiB range")
    );
}

/// Kafka requires deployment identities and enforces finite, sufficient transport budgets.
#[test]
fn kafka_configuration_contract() {
    let example = include_str!("../../../config/worker.example.toml");
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
        ("acorn-judge-workers-v1", " "),
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
            (config.kafka.max_event_record_bytes + 1024)
                .to_string()
                .as_str()
        )
    );
    let consumer = kafka::consumer_config(&config.kafka);
    for obsolete in ["events_group_id", "tasks_group_id"] {
        let text = example.replace(
            "group_id = \"acorn-judge-workers-v1\"",
            &format!("{obsolete} = \"acorn-judge-workers-v1\""),
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
            (config.kafka.max_task_record_bytes + 1024)
                .to_string()
                .as_str()
        )
    );
}
