//! L01 review data contract; scheduling and external API calls are added separately.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::database::{Error, Result};

/// Serialized opinion and optional diagnostic object budget.
pub const OUTPUT_MAX_BYTES: usize = 16_384;
/// Serialized envelope budget, including all metadata and errors.
pub const DETAILS_MAX_BYTES: usize = 32_768;
/// Maximum fixed English failure message size.
pub const ERROR_MESSAGE_MAX_BYTES: usize = 512;

/// Automatic review policy belonging to an immutable problem version.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewMode {
    /// Never automatically review.
    #[default]
    #[serde(rename = "DISABLED")]
    Disabled,
    /// Review after at least one actually executed case passed.
    #[serde(rename = "ENABLE-ANY")]
    EnableAny,
    /// Review only an accepted solution.
    #[serde(rename = "ENABLE-ALL")]
    EnableAll,
}

impl ReviewMode {
    /// Decode the persisted fixed enumeration without accepting other modes.
    pub(crate) fn parse(value: &str) -> Result<Self> {
        serde_json::from_value(Value::String(value.into()))
            .map_err(|error| Error::Data(error.into()))
    }
}

/// Review state independent of the machine judging state and verdict.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewStatus {
    /// No review has been admitted.
    #[default]
    Disabled,
    /// Waiting in the queue or calling the model.
    Pending,
    /// Concrete findings need human review.
    Doubtful,
    /// No finding within the agreed scope was found.
    Pass,
    /// The review could not produce a validated opinion.
    Failed,
}

impl ReviewStatus {
    /// Stable SQLite representation, also used for history filters.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "DISABLED",
            Self::Pending => "PENDING",
            Self::Doubtful => "DOUBTFUL",
            Self::Pass => "PASS",
            Self::Failed => "FAILED",
        }
    }

    /// Decode a stored state without inventing a fallback.
    pub(crate) fn parse(value: &str) -> Result<Self> {
        serde_json::from_value(Value::String(value.into()))
            .map_err(|error| Error::Data(error.into()))
    }
}

/// Origin of one admitted review attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewTrigger {
    /// First machine terminal persistence met the automatic policy.
    Auto,
    /// A user explicitly requested a review or rerun.
    Manual,
}

/// Only the two decisions that a model may produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    /// No scoped finding.
    Pass,
    /// At least one scoped finding.
    Doubtful,
}

/// Allowed anti-cheating categories; ordinary correctness errors are outside scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FindingCategory {
    /// Guessing or hardcoding particular tests rather than solving the problem.
    TestTargeting,
    /// Exploiting the judging environment.
    JudgeEnvironmentAbuse,
    /// Concrete evasion of a public statement constraint.
    StatementConstraintViolation,
}

/// One finding located in the original source using a closed line interval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFinding {
    /// Scoped behavior category.
    pub category: FindingCategory,
    /// One-based first line, inclusive.
    pub line_start: usize,
    /// One-based final line, inclusive.
    pub line_end: usize,
    /// Executable behavior and its connection to the suspicion.
    pub evidence: String,
    /// A possible legitimate explanation, or an explicit absence of one.
    pub normal_explanation: String,
}

/// Fixed model-generated opinion; it contains no execution metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewResult {
    /// Opinion independent of the machine verdict.
    pub decision: ReviewDecision,
    /// Bounded Chinese summary.
    pub summary: String,
    /// Empty for PASS and nonempty for DOUBTFUL.
    pub findings: Vec<ReviewFinding>,
}

impl ReviewResult {
    /// Check content budgets and closed line intervals against the original source.
    pub fn validate(&self, source_lines: usize) -> Result<()> {
        if self.summary.len() > 2048 || self.findings.len() > 8 {
            return Err(Error::InvalidReview("Opinion exceeds content limits"));
        }
        if (self.decision == ReviewDecision::Pass) != self.findings.is_empty() {
            return Err(Error::InvalidReview("Decision and findings disagree"));
        }
        for finding in &self.findings {
            if finding.line_start == 0
                || finding.line_end < finding.line_start
                || finding.line_end > source_lines
                || finding.evidence.len() > 1024
                || finding.normal_explanation.len() > 512
            {
                return Err(Error::InvalidReview(
                    "Finding violates line or content limits",
                ));
            }
        }
        if json_size(self)? > OUTPUT_MAX_BYTES {
            return Err(Error::InvalidReview("Serialized opinion exceeds limit"));
        }
        Ok(())
    }
}

/// Provider-reported token counts; missing values remain null and are never estimated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewUsage {
    /// Actual reported input tokens.
    pub input_tokens: Option<u64>,
    /// Actual reported output tokens, including reasoning where the provider does so.
    pub output_tokens: Option<u64>,
    /// Actual reported reasoning tokens, if available.
    pub reasoning_tokens: Option<u64>,
}

/// Stable review failure codes, separate from HTTP operation errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewErrorCode {
    /// Service is disabled or cannot accept handoff.
    ReviewUnavailable,
    /// No independent pending slot was available.
    ReviewQueueFull,
    /// Stored source or fixed statement could not be obtained.
    ReviewInputUnavailable,
    /// Complete serialized request exceeds its budget.
    ReviewInputTooLarge,
    /// Request and complete response reading exceeded the deadline.
    ReviewTimeout,
    /// Transport or API operation failed.
    ReviewApiFailed,
    /// Provider response did not complete.
    ReviewIncomplete,
    /// Output, opinion or complete envelope failed validation or exceeded a budget.
    ReviewInvalidOutput,
    /// Backend shutdown or restart interrupted this attempt.
    ReviewInterrupted,
}

impl ReviewErrorCode {
    /// Fixed English description; provider-controlled errors are never persisted here.
    pub fn message(self) -> &'static str {
        match self {
            Self::ReviewUnavailable => "The review service is unavailable.",
            Self::ReviewQueueFull => "The review queue is full.",
            Self::ReviewInputUnavailable => "The stored review input is unavailable.",
            Self::ReviewInputTooLarge => "The complete review request exceeds the byte limit.",
            Self::ReviewTimeout => "The review request timed out.",
            Self::ReviewApiFailed => "The review API request failed.",
            Self::ReviewIncomplete => "The review response did not complete.",
            Self::ReviewInvalidOutput => {
                "The review output is invalid or exceeds the content limit."
            }
            Self::ReviewInterrupted => "The backend interrupted the review.",
        }
    }
}

/// Failure summary assembled locally, never copied from an API response body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewError {
    /// Stable failure classification.
    pub code: ReviewErrorCode,
    /// Fixed English message corresponding to the code.
    pub message: String,
}

/// Latest review envelope; fixed optional keys serialize as explicit nulls.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDetails {
    /// Admission origin.
    pub trigger: ReviewTrigger,
    /// Admission UTC Unix milliseconds.
    pub requested_at_ms: i64,
    /// Completion UTC Unix milliseconds, absent while pending.
    pub completed_at_ms: Option<i64>,
    /// Actual requested model.
    pub model: String,
    /// Actual requested reasoning effort.
    pub reasoning_effort: String,
    /// Backend policy identifier, excluded from frontend DTOs.
    pub prompt_version: String,
    /// Actual provider response identity, if retained within the total budget.
    pub response_id: Option<String>,
    /// Actual reported usage, without inferred values.
    pub usage: Option<ReviewUsage>,
    /// Validated opinion, absent for pending and failed attempts.
    pub result: Option<ReviewResult>,
    /// Fixed local failure summary, present only on failure.
    pub error: Option<ReviewError>,
    /// Optional complete diagnostic object, excluded from frontend DTOs and logs.
    pub raw_output_json: Option<Value>,
}

/// One latest attempt; DISABLED is represented by absence of this object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewAttempt {
    /// Fresh UUID string for each newly admitted attempt.
    pub id: String,
    /// Independent persisted status.
    pub status: ReviewStatus,
    /// Bounded typed envelope.
    pub details: ReviewDetails,
}

impl ReviewAttempt {
    /// Create pending metadata and verify that a fixed failure can also be retained.
    pub fn pending(
        trigger: ReviewTrigger,
        requested_at_ms: i64,
        model: String,
        reasoning_effort: String,
        prompt_version: String,
    ) -> Result<Self> {
        let review = Self {
            id: Uuid::new_v4().to_string(),
            status: ReviewStatus::Pending,
            details: ReviewDetails {
                trigger,
                requested_at_ms,
                completed_at_ms: None,
                model,
                reasoning_effort,
                prompt_version,
                response_id: None,
                usage: None,
                result: None,
                error: None,
                raw_output_json: None,
            },
        };
        review.encode(0)?;
        // Reserve the actual largest fixed failure and maximum serialized usage.
        review.clone().failed(
            i64::MIN,
            ReviewErrorCode::ReviewInvalidOutput,
            None,
            Some(ReviewUsage {
                input_tokens: Some(u64::MAX),
                output_tokens: Some(u64::MAX),
                reasoning_tokens: Some(u64::MAX),
            }),
            None,
        )?;
        Ok(review)
    }

    /// Assemble a successful opinion, converting invalid or oversized output into FAILED.
    pub fn succeeded(
        mut self,
        completed_at_ms: i64,
        result: ReviewResult,
        response_id: Option<String>,
        usage: Option<ReviewUsage>,
        source_lines: usize,
    ) -> Result<Self> {
        if result.validate(source_lines).is_err() {
            return self.failed(
                completed_at_ms,
                ReviewErrorCode::ReviewInvalidOutput,
                response_id,
                usage,
                Some(serde_json::to_value(result).map_err(|error| Error::Data(error.into()))?),
            );
        }
        self.status = match result.decision {
            ReviewDecision::Pass => ReviewStatus::Pass,
            ReviewDecision::Doubtful => ReviewStatus::Doubtful,
        };
        self.details.completed_at_ms = Some(completed_at_ms);
        self.details.response_id = response_id;
        self.details.usage = usage;
        self.details.result = Some(result);
        self.details.error = None;
        self.details.raw_output_json = None;
        if json_size(&self.details)? > DETAILS_MAX_BYTES {
            let raw = serde_json::to_value(self.details.result.take().expect("Opinion exists"))
                .map_err(|error| Error::Data(error.into()))?;
            let usage = self.details.usage.clone();
            return self.failed(
                completed_at_ms,
                ReviewErrorCode::ReviewInvalidOutput,
                None,
                usage,
                Some(raw),
            );
        }
        self.encode(source_lines)?;
        Ok(self)
    }

    /// Retain bounded complete JSON diagnostics without changing the failure classification.
    pub fn failed(
        mut self,
        completed_at_ms: i64,
        code: ReviewErrorCode,
        response_id: Option<String>,
        usage: Option<ReviewUsage>,
        raw_output_json: Option<Value>,
    ) -> Result<Self> {
        self.status = ReviewStatus::Failed;
        self.details.completed_at_ms = Some(completed_at_ms);
        self.details.result = None;
        self.details.error = Some(ReviewError {
            code,
            message: code.message().into(),
        });
        self.details.usage = usage;
        self.details.response_id = response_id;
        self.details.raw_output_json = None;
        if json_size(&self.details)? > DETAILS_MAX_BYTES {
            self.details.response_id = None;
        }
        if let Some(raw) = raw_output_json
            && raw.is_object()
            && json_size(&raw)? <= OUTPUT_MAX_BYTES
        {
            self.details.raw_output_json = Some(raw);
            if json_size(&self.details)? > DETAILS_MAX_BYTES {
                self.details.raw_output_json = None;
            }
        }
        self.encode(0)?;
        Ok(self)
    }

    /// Validate status relationships and content at the database write boundary.
    pub(crate) fn encode(&self, source_lines: usize) -> Result<String> {
        if Uuid::parse_str(&self.id).is_err() {
            return Err(Error::InvalidReview("Review ID must be a UUID"));
        }
        let details = &self.details;
        let shape_ok = match self.status {
            ReviewStatus::Disabled => false,
            ReviewStatus::Pending => {
                details.completed_at_ms.is_none()
                    && details.result.is_none()
                    && details.error.is_none()
                    && details.raw_output_json.is_none()
                    && details.response_id.is_none()
                    && details.usage.is_none()
            }
            ReviewStatus::Pass | ReviewStatus::Doubtful => {
                details.completed_at_ms.is_some()
                    && details.error.is_none()
                    && details.raw_output_json.is_none()
                    && details.result.as_ref().is_some_and(|result| {
                        matches!(
                            (self.status, result.decision),
                            (ReviewStatus::Pass, ReviewDecision::Pass)
                                | (ReviewStatus::Doubtful, ReviewDecision::Doubtful)
                        )
                    })
            }
            ReviewStatus::Failed => {
                details.completed_at_ms.is_some()
                    && details.result.is_none()
                    && details.error.is_some()
            }
        };
        if !shape_ok {
            return Err(Error::InvalidReview("Status and details disagree"));
        }
        if let Some(result) = &details.result {
            result.validate(source_lines)?;
        }
        if let Some(error) = &details.error
            && (error.message != error.code.message()
                || error.message.len() > ERROR_MESSAGE_MAX_BYTES)
        {
            return Err(Error::InvalidReview(
                "Failure message must be fixed and bounded",
            ));
        }
        if let Some(raw) = &details.raw_output_json
            && (!raw.is_object() || json_size(raw)? > OUTPUT_MAX_BYTES)
        {
            return Err(Error::InvalidReview(
                "Diagnostic must be a bounded JSON object",
            ));
        }
        let json = serde_json::to_string(details).map_err(|error| Error::Data(error.into()))?;
        if json.len() > DETAILS_MAX_BYTES {
            return Err(Error::InvalidReview("Complete review details exceed limit"));
        }
        Ok(json)
    }
}

/// Measure serialized UTF-8 bytes, including JSON escaping rather than just text lengths.
fn json_size(value: &impl Serialize) -> Result<usize> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|error| Error::Data(error.into()))
}

#[cfg(test)]
mod tests {
    //! Contract checks for opinion validation and bounded latest-attempt assembly.

    use serde_json::json;

    use super::*;

    /// Standard local pending metadata with no remote output or estimated usage.
    fn pending() -> ReviewAttempt {
        ReviewAttempt::pending(
            ReviewTrigger::Manual,
            100,
            "deepseek-flash".into(),
            "max".into(),
            "review-policy-v1".into(),
        )
        .unwrap()
    }

    /// A concrete finding uses a one-based closed line range.
    fn doubtful() -> ReviewResult {
        ReviewResult {
            decision: ReviewDecision::Doubtful,
            summary: "需要人工复核".into(),
            findings: vec![ReviewFinding {
                category: FindingCategory::TestTargeting,
                line_start: 1,
                line_end: 2,
                evidence: "只返回固定输入的预置结果".into(),
                normal_explanation: "需要核对题目是否允许查表".into(),
            }],
        }
    }

    /// Unknown fields, nulls, decision relationships and actual serialized bytes are checked.
    #[test]
    fn opinion_contract_and_budgets() {
        let mut result = doubtful();
        result.validate(2).unwrap();
        assert!(result.validate(1).is_err());
        result.findings[0].line_start = 0;
        assert!(result.validate(2).is_err());
        result.findings[0].line_start = 2;
        result.findings[0].line_end = 1;
        assert!(result.validate(2).is_err());
        result = doubtful();
        result.decision = ReviewDecision::Pass;
        assert!(result.validate(2).is_err());
        result.findings.clear();
        result.validate(0).unwrap();
        result.summary = "中".repeat(683);
        assert!(result.validate(0).is_err());
        for value in [
            json!({"decision":"PASS","summary":"ok","findings":[],"extra":true}),
            json!({"decision":"FAILED","summary":"ok","findings":[]}),
            json!({"decision":"PASS","summary":null,"findings":[]}),
            json!({"decision":"PASS","findings":[]}),
        ] {
            assert!(serde_json::from_value::<ReviewResult>(value).is_err());
        }
        result = doubtful();
        result.findings[0].evidence = "\u{0000}".repeat(1024);
        result.findings[0].normal_explanation = "\u{0000}".repeat(512);
        result.findings = vec![result.findings[0].clone(); 8];
        assert!(json_size(&result).unwrap() > OUTPUT_MAX_BYTES);
        assert!(result.validate(2).is_err());
    }

    /// Output overflow becomes FAILED, while diagnostics remain optional and never truncate JSON.
    #[test]
    fn envelope_budgets_and_diagnostics() {
        let review = pending();
        let pending_json: Value = serde_json::from_str(&review.encode(0).unwrap()).unwrap();
        for key in [
            "completed_at_ms",
            "response_id",
            "usage",
            "result",
            "error",
            "raw_output_json",
        ] {
            assert!(pending_json.as_object().unwrap().contains_key(key));
            assert!(pending_json[key].is_null());
        }
        let completed = review
            .clone()
            .succeeded(200, doubtful(), Some("response-1".into()), None, 2)
            .unwrap();
        assert_eq!(completed.status, ReviewStatus::Doubtful);
        assert!(completed.details.raw_output_json.is_none());
        let oversized = review
            .clone()
            .succeeded(
                200,
                doubtful(),
                Some("x".repeat(DETAILS_MAX_BYTES)),
                None,
                2,
            )
            .unwrap();
        assert_eq!(oversized.status, ReviewStatus::Failed);
        assert_eq!(
            oversized.details.error.unwrap().code,
            ReviewErrorCode::ReviewInvalidOutput
        );
        assert!(oversized.details.result.is_none());
        assert!(oversized.details.response_id.is_none());
        let invalid = review
            .clone()
            .succeeded(200, doubtful(), None, None, 1)
            .unwrap();
        assert_eq!(invalid.status, ReviewStatus::Failed);
        assert!(invalid.details.raw_output_json.is_some());
        for raw in [json!({"body":"x".repeat(OUTPUT_MAX_BYTES)}), json!([1, 2])] {
            let failed = review
                .clone()
                .failed(
                    200,
                    ReviewErrorCode::ReviewIncomplete,
                    None,
                    None,
                    Some(raw),
                )
                .unwrap();
            assert!(failed.details.raw_output_json.is_none());
            assert_eq!(
                failed.details.error.unwrap().code,
                ReviewErrorCode::ReviewIncomplete
            );
        }
        let large = ReviewAttempt::pending(
            ReviewTrigger::Auto,
            100,
            "x".repeat(20_000),
            "max".into(),
            "v1".into(),
        )
        .unwrap();
        let failed = large
            .failed(
                200,
                ReviewErrorCode::ReviewIncomplete,
                None,
                None,
                Some(json!({"body":"x".repeat(15_000)})),
            )
            .unwrap();
        assert!(failed.details.raw_output_json.is_none());
        assert_eq!(
            failed.details.error.unwrap().code,
            ReviewErrorCode::ReviewIncomplete
        );
        assert!(
            ReviewAttempt::pending(
                ReviewTrigger::Auto,
                100,
                "x".repeat(DETAILS_MAX_BYTES),
                "max".into(),
                "v1".into()
            )
            .is_err()
        );
        let mut mutated = review;
        mutated.details.response_id = Some("unexpected".into());
        assert!(mutated.encode(0).is_err());
    }
}
