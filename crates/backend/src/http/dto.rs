//! Public JSON shapes; private source, test data and Worker progress never appear.

use crate::{
    problems,
    submissions::{self, SubmissionState, TerminalDetails, Verdict},
};
use serde::{Deserialize, Serialize};

/// Allowed create fields only; execution data and owner are server-controlled.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateRequest {
    /// Problem whose latest published version is selected.
    pub problem_id: i64,
    /// Built-in language identifier.
    pub language_id: String,
    /// Original UTF-8 source, with whitespace and newlines preserved.
    pub source: String,
}

/// Minimal response acknowledging persistence and local handoff.
#[derive(Serialize)]
pub(super) struct CreateResponse {
    /// API-generated UUID, also used as the Worker Job ID.
    pub submission_id: String,
    /// Persisted business state observed during creation.
    pub state: SubmissionState,
}

/// Current-version identity for a problem list.
#[derive(Serialize)]
pub(super) struct ProblemSummary {
    /// Positive display and lookup number.
    problem_id: i64,
    /// Latest published version number.
    version_no: i64,
    /// Published title.
    title: String,
}

impl From<problems::ProblemSummary> for ProblemSummary {
    /// Map list fields explicitly, excluding private database content.
    fn from(value: problems::ProblemSummary) -> Self {
        Self {
            problem_id: value.problem_id,
            version_no: value.version_no,
            title: value.title,
        }
    }
}

/// One permitted language with per-case budgets in milliseconds and KiB.
#[derive(Serialize)]
struct Language {
    /// Built-in c or python identifier.
    language_id: String,
    /// CPU limit per case in milliseconds.
    cpu_time_ms: u64,
    /// Wall-clock limit per case in milliseconds.
    wall_time_ms: u64,
    /// Memory limit per case in KiB.
    memory_kib: u64,
}

/// Latest or explicit-version public problem detail.
#[derive(Serialize)]
pub(super) struct Problem {
    /// Flat identity and title, shared with the list.
    #[serde(flatten)]
    summary: ProblemSummary,
    /// Immutable statement belonging to this version.
    statement: String,
    /// Allowed languages and their limits, excluding all tests and answers.
    languages: Vec<Language>,
}

impl From<problems::ProblemVersion> for Problem {
    /// Convert only the documented public fields.
    fn from(value: problems::ProblemVersion) -> Self {
        Self {
            summary: value.summary.into(),
            statement: value.statement,
            languages: value
                .languages
                .into_iter()
                .map(|language| Language {
                    language_id: language.language_id,
                    cpu_time_ms: language.limits.cpu_time_ms,
                    wall_time_ms: language.limits.wall_time_ms,
                    memory_kib: language.limits.memory_kib,
                })
                .collect(),
        }
    }
}

/// Submission history fields omit source and terminal diagnostics.
#[derive(Serialize)]
pub(super) struct SubmissionSummary {
    /// UUID exposed with the HTTP contract's field name.
    submission_id: String,
    /// Fixed problem number.
    problem_id: i64,
    /// Version fixed at creation, unaffected by later publication.
    version_no: i64,
    /// Fixed language.
    language_id: String,
    /// Persisted business state, never a transient Worker phase.
    state: SubmissionState,
    /// Independent verdict, or null before a judging terminal.
    verdict: Option<Verdict>,
    /// UTC Unix creation milliseconds.
    created_at_ms: i64,
    /// UTC Unix terminal-save milliseconds, or null until terminal.
    terminal_at_ms: Option<i64>,
}

impl From<submissions::SubmissionInfo> for SubmissionSummary {
    /// Rename the database identity without returning stored source or ownership data.
    fn from(value: submissions::SubmissionInfo) -> Self {
        Self {
            submission_id: value.id,
            problem_id: value.problem_id,
            version_no: value.version_no,
            language_id: value.language_id,
            state: value.state,
            verdict: value.verdict,
            created_at_ms: value.created_at_ms,
            terminal_at_ms: value.terminal_at_ms,
        }
    }
}

/// Parsed terminal details remain an object rather than a JSON-encoded string.
#[derive(Serialize)]
pub(super) struct Submission {
    /// Flat identity, fixed target, status and timestamps.
    #[serde(flatten)]
    summary: SubmissionSummary,
    /// Typed Base64-bearing details; null for AC and nonterminal records.
    terminal_details: Option<TerminalDetails>,
}

impl From<submissions::Submission> for Submission {
    /// Preserve typed terminal shape and optional resource statistics.
    fn from(value: submissions::Submission) -> Self {
        Self {
            summary: value.info.into(),
            terminal_details: value.terminal_details,
        }
    }
}

/// Shared pagination envelope, deliberately without an extra total-count query.
#[derive(Serialize)]
pub(super) struct Page<T> {
    /// Items in the documented deterministic order.
    pub items: Vec<T>,
    /// Effective configured or explicitly requested page length.
    pub limit: u32,
    /// Requested zero-based item offset.
    pub offset: u64,
}

/// Problem-list pagination parameters; unrecognized fields are rejected.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageQuery {
    /// Optional page length, defaulting to the deployment configuration.
    pub limit: Option<u32>,
    /// Optional nonnegative item offset, defaulting to zero.
    pub offset: Option<u64>,
}

/// Submission history pagination and the three documented filters.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HistoryQuery {
    /// Optional page length.
    pub limit: Option<u32>,
    /// Optional nonnegative offset.
    pub offset: Option<u64>,
    /// Optional positive problem number.
    pub problem_id: Option<i64>,
    /// Optional persisted business state.
    pub state: Option<SubmissionState>,
    /// Optional judging conclusion.
    pub verdict: Option<Verdict>,
}
