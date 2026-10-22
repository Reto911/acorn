//! Submission identities, task ownership and bounded typed terminal details.

mod creation;

pub use creation::{CreationError, CreationService};

use acorn_protocol::worker::v1::{self as proto, SubmitJobRequest};
use base64::{Engine, prelude::BASE64_STANDARD};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    config::ResultsConfig,
    database::{Error, Result},
    llm_review::{ReviewAttempt, ReviewMode, ReviewStatus},
};

/// Persisted business states; Worker phases are not stored here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubmissionState {
    /// Saved but not yet dispatched.
    Queued,
    /// Submission is being offered to a selected Worker.
    Dispatching,
    /// Worker has acknowledged admission.
    Running,
    /// A judging verdict has been persisted.
    Finished,
    /// An execution or communication failure has been persisted.
    Failed,
}

impl SubmissionState {
    /// Stable SQLite and HTTP representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Failed => "failed",
        }
    }

    /// Decode the schema's fixed state enumeration.
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "dispatching" => Ok(Self::Dispatching),
            "running" => Ok(Self::Running),
            "finished" => Ok(Self::Finished),
            "failed" => Ok(Self::Failed),
            _ => Err(Error::Data(anyhow::anyhow!(
                "Unknown submission state: {value}"
            ))),
        }
    }
}

/// Judging verdict stored separately from typed terminal details.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Accepted.
    AC,
    /// Wrong answer.
    WA,
    /// Compile error, including compiler resource limits.
    CE,
    /// Runtime error.
    RE,
    /// CPU or wall-clock limit exceeded.
    TLE,
    /// Confirmed memory limit exceeded.
    MLE,
    /// Output limit exceeded.
    OLE,
}

impl Verdict {
    /// Stable SQL filter and JSON representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AC => "AC",
            Self::WA => "WA",
            Self::CE => "CE",
            Self::RE => "RE",
            Self::TLE => "TLE",
            Self::MLE => "MLE",
            Self::OLE => "OLE",
        }
    }

    /// Decode the schema's fixed verdict enumeration.
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "AC" => Ok(Self::AC),
            "WA" => Ok(Self::WA),
            "CE" => Ok(Self::CE),
            "RE" => Ok(Self::RE),
            "TLE" => Ok(Self::TLE),
            "MLE" => Ok(Self::MLE),
            "OLE" => Ok(Self::OLE),
            _ => Err(Error::Data(anyhow::anyhow!("Unknown verdict: {value}"))),
        }
    }
}

/// Identity, fixed execution target and result summary returned by list queries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmissionInfo {
    /// API-generated UUID and Worker Job ID.
    pub id: String,
    /// Fixed problem number.
    pub problem_id: i64,
    /// Fixed published version.
    pub version_no: i64,
    /// Fixed language identifier.
    pub language_id: String,
    /// Persisted business state.
    pub state: SubmissionState,
    /// Judging conclusion, present only when finished.
    pub verdict: Option<Verdict>,
    /// Independent latest review state, without loading its payload.
    pub review_status: ReviewStatus,
    /// UTC Unix creation time in milliseconds.
    pub created_at_ms: i64,
    /// Backend terminal-save time, not proof that the remote process stopped.
    pub terminal_at_ms: Option<i64>,
}

/// Full stored submission; HTTP lists deliberately use only SubmissionInfo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    /// Identity, fixed target, business state and timestamps.
    pub info: SubmissionInfo,
    /// Original owner, determined by backend configuration at creation.
    pub user_id: i64,
    /// Original nonempty source bytes.
    pub source: Vec<u8>,
    /// Client-generated creation key, scoped to the owner.
    pub create_request_key: String,
    /// Parsed result details, absent for AC and nonterminal states.
    pub terminal_details: Option<TerminalDetails>,
    /// Latest review attempt, absent when review_status is DISABLED.
    pub review: Option<ReviewAttempt>,
}

/// Caller-provided content for a short, idempotent creation transaction.
#[derive(Clone, Debug)]
pub struct CreateSubmission {
    /// Problem whose current published version must be selected.
    pub problem_id: i64,
    /// Requested built-in language.
    pub language_id: String,
    /// Original bytes, already UTF-8 encoded by the HTTP boundary.
    pub source: Vec<u8>,
    /// Parsed and canonicalized Idempotency-Key UUID.
    pub request_key: Uuid,
}

/// Complete owned task handed to the Scheduler without another database read.
#[derive(Clone, Debug, PartialEq)]
pub struct JudgingTask {
    /// Fixed problem number, used to interpret results.
    pub problem_id: i64,
    /// Fixed published version, never replaced by a newer one.
    pub version_no: i64,
    /// Job identity, source, ordered cases and budgets sent to the Worker.
    pub request: SubmitJobRequest,
    /// Fixed-version review policy, never included in the Worker request.
    pub review_mode: ReviewMode,
}

/// Persisted work to restore before starting Kafka publishing or event consumption.
/// State groups are disjoint, as returned by Database::judging_recovery.
#[derive(Default)]
pub struct JudgingRecovery {
    /// Complete fixed-version tasks that still need publication, in creation order.
    pub queued: Vec<JudgingTask>,
    /// Confirmed deliveries awaiting admission; restore permits without task payloads.
    pub dispatching: Vec<String>,
    /// Admitted jobs awaiting terminal events, without waiting-admission permits.
    pub running: Vec<String>,
}

/// Fixed submission metadata needed to validate an event without loading task payloads.
pub struct JudgingContext {
    /// Current persisted state, used to skip repeated running writes and review preparation.
    pub state: SubmissionState,
    /// Original language, used for execution-phase validation.
    pub language_id: String,
    /// Case count from the submission's immutable problem version.
    pub total_cases: u32,
    /// Automatic review policy from that same fixed version.
    pub review_mode: ReviewMode,
}

/// Creation result returned only after the transaction and connection are released.
#[derive(Debug)]
pub struct CreatedSubmission {
    /// Persisted identity and current state.
    pub info: SubmissionInfo,
    /// True only for a newly inserted record.
    pub created: bool,
    /// Complete task for a queued record; ended or dispatched records have none.
    pub task: Option<JudgingTask>,
}

/// Optional historical filters; ownership always comes from backend configuration.
#[derive(Clone, Debug, Default)]
pub struct SubmissionFilter {
    /// Optional problem number.
    pub problem_id: Option<i64>,
    /// Optional business state.
    pub state: Option<SubmissionState>,
    /// Optional independent verdict column.
    pub verdict: Option<Verdict>,
    /// Optional independent review-state filter.
    pub review_status: Option<ReviewStatus>,
}

/// Compiler diagnostics encoded once as standard Base64.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileDetails {
    /// Combined compiler stdout/stderr prefix.
    pub compile_diagnostic_base64: String,
    /// Whether any diagnostic bytes were discarded.
    pub compile_diagnostic_truncated: bool,
}

/// Measurements retain absence and explicit zero as different values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceUsage {
    /// Actual process-tree CPU time in microseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_time_us: Option<u64>,
    /// Actual wall time in microseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wall_time_us: Option<u64>,
    /// Actual peak process-tree memory in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_memory_bytes: Option<u64>,
}

/// The unique first failed case; no test input or answer is duplicated here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailedCase {
    /// One-based index into the submission's fixed version.
    pub case_index: u32,
    /// Bounded stdout prefix as standard Base64.
    pub stdout_base64: String,
    /// Bounded stderr prefix as standard Base64.
    pub stderr_base64: String,
    /// Whether stdout was truncated.
    pub stdout_truncated: bool,
    /// Whether stderr was truncated.
    pub stderr_truncated: bool,
    /// Optional actual measurements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceUsage>,
    /// Optional program exit code, mutually exclusive with signal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
    /// Optional terminating signal, mutually exclusive with exit_code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<u32>,
}

/// The JSON wrapper for a case-level verdict.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseDetails {
    /// Sole first failed case.
    pub failed_case: FailedCase,
}

/// Bounded execution failure, independent of HTTP errors and judging verdicts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureDetails {
    /// Stable operation or Worker failure code.
    pub code: String,
    /// Bounded UTF-8 diagnostic.
    pub diagnostic: String,
}

/// Untagged JSON shape shared by database persistence and HTTP results.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TerminalDetails {
    /// Compiler error diagnostics.
    Compile(CompileDetails),
    /// One failed test case.
    Case(CaseDetails),
    /// Infrastructure, communication or interruption failure.
    Failure(FailureDetails),
}

/// Validated terminal data; private fields prevent callers bypassing its constructors.
#[derive(Clone, Debug)]
pub struct TerminalOutcome {
    /// Finished for judging results or failed for execution failures.
    pub(crate) state: SubmissionState,
    /// Independent SQL verdict column.
    pub(crate) verdict: Option<Verdict>,
    /// Optional typed JSON body.
    pub(crate) details: Option<TerminalDetails>,
}

impl TerminalOutcome {
    /// Bound a locally reported transport or protocol diagnostic before constructing a failure.
    pub(crate) fn local_failure(
        code: &str,
        diagnostic: &str,
        limits: &ResultsConfig,
    ) -> Result<Self> {
        let mut end = diagnostic.len().min(limits.failure_diagnostic_max_bytes);
        while !diagnostic.is_char_boundary(end) {
            end -= 1;
        }
        Self::failed(
            &proto::JobFailure {
                code: code.into(),
                diagnostic: diagnostic[..end].into(),
            },
            limits,
        )
    }

    /// Validate result shape and raw-byte budgets before Base64 JSON serialization.
    pub fn judged(
        result: &proto::JudgeResult,
        total_cases: u32,
        limits: &ResultsConfig,
    ) -> Result<Self> {
        let verdict = match proto::Verdict::try_from(result.verdict).ok() {
            Some(proto::Verdict::Accepted) => Verdict::AC,
            Some(proto::Verdict::WrongAnswer) => Verdict::WA,
            Some(proto::Verdict::CompileError) => Verdict::CE,
            Some(proto::Verdict::RuntimeError) => Verdict::RE,
            Some(proto::Verdict::TimeLimitExceeded) => Verdict::TLE,
            Some(proto::Verdict::MemoryLimitExceeded) => Verdict::MLE,
            Some(proto::Verdict::OutputLimitExceeded) => Verdict::OLE,
            _ => return Err(Error::InvalidResult("Unknown judging verdict")),
        };
        if verdict != Verdict::CE
            && (!result.compile_diagnostic.is_empty() || result.compile_diagnostic_truncated)
        {
            return Err(Error::InvalidResult(
                "Only CE may contain compiler diagnostics",
            ));
        }
        let details = match verdict {
            Verdict::AC | Verdict::CE => {
                if result.failed_case.is_some() {
                    return Err(Error::InvalidResult("AC/CE must not contain a failed case"));
                }
                if verdict == Verdict::AC {
                    None
                } else {
                    if result.compile_diagnostic.len() > limits.compile_diagnostic_max_bytes {
                        return Err(Error::InvalidResult("Compiler diagnostic exceeds limit"));
                    }
                    Some(TerminalDetails::Compile(CompileDetails {
                        compile_diagnostic_base64: BASE64_STANDARD
                            .encode(&result.compile_diagnostic),
                        compile_diagnostic_truncated: result.compile_diagnostic_truncated,
                    }))
                }
            }
            _ => {
                let case = result
                    .failed_case
                    .as_ref()
                    .ok_or(Error::InvalidResult("Case verdict requires a failed case"))?;
                if case.case_index == 0 || case.case_index > total_cases {
                    return Err(Error::InvalidResult(
                        "Failed case index is outside this task",
                    ));
                }
                if case.stdout.len() > limits.stdout_max_bytes
                    || case.stderr.len() > limits.stderr_max_bytes
                {
                    return Err(Error::InvalidResult("Failed case output exceeds limit"));
                }
                let (exit_code, signal) = match case.exit {
                    Some(proto::failed_case::Exit::ExitCode(code)) => (Some(code), None),
                    Some(proto::failed_case::Exit::Signal(signal)) => (None, Some(signal)),
                    None => (None, None),
                };
                Some(TerminalDetails::Case(CaseDetails {
                    failed_case: FailedCase {
                        case_index: case.case_index,
                        stdout_base64: BASE64_STANDARD.encode(&case.stdout),
                        stderr_base64: BASE64_STANDARD.encode(&case.stderr),
                        stdout_truncated: case.stdout_truncated,
                        stderr_truncated: case.stderr_truncated,
                        resources: case.resources.as_ref().map(|usage| ResourceUsage {
                            cpu_time_us: usage.cpu_time_us,
                            wall_time_us: usage.wall_time_us,
                            peak_memory_bytes: usage.peak_memory_bytes,
                        }),
                        exit_code,
                        signal,
                    },
                }))
            }
        };
        Ok(Self {
            state: SubmissionState::Finished,
            verdict: Some(verdict),
            details,
        })
    }

    /// Validate a bounded execution failure without manufacturing a verdict.
    pub fn failed(failure: &proto::JobFailure, limits: &ResultsConfig) -> Result<Self> {
        if failure.code.is_empty() || failure.diagnostic.len() > limits.failure_diagnostic_max_bytes
        {
            return Err(Error::InvalidResult(
                "Invalid failure code or diagnostic size",
            ));
        }
        Ok(Self {
            state: SubmissionState::Failed,
            verdict: None,
            details: Some(TerminalDetails::Failure(FailureDetails {
                code: failure.code.clone(),
                diagnostic: failure.diagnostic.clone(),
            })),
        })
    }
}
