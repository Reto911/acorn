//! Verify field presence and terminal variants across Protobuf encoding.

use acorn_protocol::worker::v1::{
    CheckerLimits, FailedCase, GetJobReply, JobFailure, JobPhase, JobState, JudgeResult,
    ResourceUsage, SubmitJobRequest, Verdict, failed_case::Exit, get_job_reply::Terminal,
};
use prost::Message;

/// Checker limit messages retain absence, empty messages and explicit zero without validation.
#[test]
fn checker_limit_presence_and_values_survive_encoding() {
    for checker_limits in [
        None,
        Some(CheckerLimits::default()),
        Some(CheckerLimits {
            cpu_time_ms: Some(0),
            wall_time_ms: Some(0),
            memory_kib: Some(0),
        }),
        Some(CheckerLimits {
            cpu_time_ms: Some(1),
            wall_time_ms: Some(u64::MAX),
            memory_kib: None,
        }),
    ] {
        let task = SubmitJobRequest {
            checker_limits,
            ..Default::default()
        };
        let decoded = SubmitJobRequest::decode(task.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded.checker_limits, task.checker_limits);
    }
}

/// Zero-valued statistics and exit codes must remain distinct from absent fields.
#[test]
fn failed_case_preserves_optional_zero_values() {
    let reply = GetJobReply {
        job_id: "submission-1".into(),
        state: JobState::Finished.into(),
        phase: JobPhase::Done.into(),
        completed_cases: 1,
        total_cases: 2,
        terminal: Some(Terminal::Result(JudgeResult {
            verdict: Verdict::WrongAnswer.into(),
            failed_case: Some(FailedCase {
                case_index: 1,
                resources: Some(ResourceUsage {
                    cpu_time_us: Some(0),
                    wall_time_us: None,
                    peak_memory_bytes: None,
                }),
                exit: Some(Exit::ExitCode(0)),
                ..Default::default()
            }),
            ..Default::default()
        })),
    };

    let decoded = GetJobReply::decode(reply.encode_to_vec().as_slice()).unwrap();
    let Some(Terminal::Result(result)) = decoded.terminal else {
        panic!("expected a judging result");
    };
    let failed_case = result.failed_case.unwrap();
    let resources = failed_case.resources.unwrap();
    assert_eq!(resources.cpu_time_us, Some(0));
    assert_eq!(resources.wall_time_us, None);
    assert_eq!(resources.peak_memory_bytes, None);
    assert_eq!(failed_case.exit, Some(Exit::ExitCode(0)));
}

/// Infrastructure failures must retain their distinct terminal message variant.
#[test]
fn infrastructure_failure_uses_a_separate_terminal_variant() {
    let reply = GetJobReply {
        job_id: "submission-2".into(),
        state: JobState::Failed.into(),
        phase: JobPhase::Done.into(),
        terminal: Some(Terminal::Failure(JobFailure {
            code: "SandboxInitializationFailed".into(),
            diagnostic: "Failed to initialize the sandbox.".into(),
        })),
        ..Default::default()
    };

    let decoded = GetJobReply::decode(reply.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded.state, JobState::Failed as i32);
    assert_eq!(decoded.terminal, reply.terminal);
}
