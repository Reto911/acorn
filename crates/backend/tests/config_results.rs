//! Focused configuration and terminal-shape boundary checks.

use acorn_backend::{config::Config, database::Error, submissions::TerminalOutcome};
use acorn_protocol::worker::v1::{FailedCase, JudgeResult, Verdict};

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
