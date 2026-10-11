//! Startup and language-plan acceptance checks without sandbox execution.

use acorn_worker::{
    config::Config,
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
    let too_small = EXAMPLE.replace("max_response_bytes = 1_048_576", "max_response_bytes = 100");
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
