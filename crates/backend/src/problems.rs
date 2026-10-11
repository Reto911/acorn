//! Published problem types independent of HTTP response DTOs.

use acorn_protocol::worker::v1::RunLimits;

/// Minimal current-version information for a problem list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProblemSummary {
    /// Positive problem number.
    pub problem_id: i64,
    /// Published version within this problem.
    pub version_no: i64,
    /// Version-specific title.
    pub title: String,
}

/// One allowed language and its version-specific per-case budgets.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageLimits {
    /// Built-in c or python language identifier.
    pub language_id: String,
    /// Millisecond and byte limits passed unchanged to the Worker.
    pub limits: RunLimits,
}

/// Immutable published content, excluding private test inputs and answers.
#[derive(Clone, Debug, PartialEq)]
pub struct ProblemVersion {
    /// Version identity and title.
    pub summary: ProblemSummary,
    /// Version-specific problem statement.
    pub statement: String,
    /// Supported fixed output-comparison rule.
    pub comparison_mode: String,
    /// UTC Unix publication time in milliseconds.
    pub published_at_ms: i64,
    /// Ordered allowed languages and budgets.
    pub languages: Vec<LanguageLimits>,
}
