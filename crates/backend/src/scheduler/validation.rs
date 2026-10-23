//! Shared validation of Worker snapshots, independent of RPC or Kafka transport.

use acorn_protocol::worker::v1::{
    self as proto, GetJobReply, JobPhase, JobState, get_job_reply::Terminal,
};

use crate::{config::ResultsConfig, submissions::TerminalOutcome};

/// Validated transient progress or a machine terminal ready for persistence.
pub(crate) enum Observation {
    /// Execution remains active; phase and progress stay transient.
    Running,
    /// Complete bounded result that passed the existing protocol checks.
    Terminal(TerminalOutcome),
}

/// Validate identity, state, phase, progress and the bounded terminal together.
pub(crate) fn validate(
    reply: GetJobReply,
    id: &str,
    language: &str,
    total: u32,
    limits: &ResultsConfig,
) -> Result<Observation, crate::database::Error> {
    if reply.job_id != id || reply.total_cases != total || reply.completed_cases > total {
        return Err(crate::database::Error::InvalidResult(
            "GetJob identity or case count does not match the task",
        ));
    }
    let state = JobState::try_from(reply.state).ok();
    let phase = JobPhase::try_from(reply.phase).ok();
    if state == Some(JobState::Running) {
        let valid_phase = match phase {
            Some(JobPhase::Preparing) => reply.completed_cases == 0,
            Some(JobPhase::Compiling) => language == "c" && reply.completed_cases == 0,
            Some(JobPhase::RunningCase) => reply.completed_cases < total,
            Some(JobPhase::Checking) => true,
            _ => false,
        };
        return if valid_phase && reply.terminal.is_none() {
            Ok(Observation::Running)
        } else {
            Err(crate::database::Error::InvalidResult(
                "Running Job has an invalid phase, progress or terminal",
            ))
        };
    }
    if phase != Some(JobPhase::Done) {
        return Err(crate::database::Error::InvalidResult(
            "Terminal Job must be in the done phase",
        ));
    }
    let terminal = match (state, reply.terminal) {
        (Some(JobState::Finished), Some(Terminal::Result(result))) => {
            let expected = match proto::Verdict::try_from(result.verdict).ok() {
                Some(proto::Verdict::Accepted) => total,
                Some(proto::Verdict::CompileError) => 0,
                _ => result
                    .failed_case
                    .as_ref()
                    .map_or(0, |case| case.case_index),
            };
            if reply.completed_cases != expected {
                return Err(crate::database::Error::InvalidResult(
                    "Terminal progress does not match the verdict",
                ));
            }
            TerminalOutcome::judged(&result, total, limits)
        }
        (Some(JobState::Failed), Some(Terminal::Failure(failure))) => {
            TerminalOutcome::failed(&failure, limits)
        }
        _ => {
            return Err(crate::database::Error::InvalidResult(
                "Job state does not match its terminal message",
            ));
        }
    }?;
    Ok(Observation::Terminal(terminal))
}

#[cfg(test)]
mod tests {
    //! Existing progress and terminal correspondence checks at the event boundary.
    use super::*;
    use acorn_protocol::worker::v1::{
        self as proto, GetJobReply, JobPhase, JobState, get_job_reply::Terminal,
    };
    /// Valid progress stays transient; mismatched phases and completed counts fail validation.
    #[test]
    fn progress_and_terminal_correspondence() {
        let config =
            crate::config::Config::parse(include_str!("../../../../config/backend.example.toml"))
                .unwrap();
        let mut reply = GetJobReply {
            job_id: "job".into(),
            state: JobState::Running as i32,
            phase: JobPhase::RunningCase as i32,
            total_cases: 2,
            ..Default::default()
        };
        assert!(matches!(
            validate(reply.clone(), "job", "python", 2, &config.results),
            Ok(Observation::Running)
        ));
        for (phase, completed) in [
            (JobPhase::Preparing, 0),
            (JobPhase::Compiling, 0),
            (JobPhase::RunningCase, 1),
            (JobPhase::Checking, 2),
        ] {
            let mut snapshot = reply.clone();
            snapshot.phase = phase.into();
            snapshot.completed_cases = completed;
            assert!(matches!(
                validate(snapshot, "job", "c", 2, &config.results),
                Ok(Observation::Running)
            ));
        }
        reply.phase = JobPhase::Compiling as i32;
        assert!(validate(reply.clone(), "job", "python", 2, &config.results).is_err());
        reply.phase = JobPhase::Done as i32;
        reply.state = JobState::Finished as i32;
        reply.terminal = Some(Terminal::Result(proto::JudgeResult {
            verdict: proto::Verdict::Accepted as i32,
            ..Default::default()
        }));
        reply.completed_cases = 1;
        assert!(validate(reply.clone(), "job", "c", 2, &config.results).is_err());
        reply.completed_cases = 2;
        assert!(matches!(
            validate(reply.clone(), "job", "c", 2, &config.results),
            Ok(Observation::Terminal(_))
        ));
        reply.state = JobState::Failed as i32;
        assert!(validate(reply, "job", "c", 2, &config.results).is_err());
    }
}
