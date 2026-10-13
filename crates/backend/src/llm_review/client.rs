//! One bounded non-streaming Responses POST, with no retry, protocol fallback or database access.

use std::time::{Duration, Instant};

use anyhow::Context;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::LlmReviewConfig,
    database::{Error, Result, ReviewInput, now_ms},
};

use super::{
    ReviewAttempt, ReviewErrorCode, ReviewResult, ReviewStatus, ReviewTrigger, ReviewUsage, policy,
};

/// Shared connection pool and fixed startup settings; Debug deliberately exposes no credentials.
#[derive(Clone)]
pub struct ReviewClient {
    /// One reusable asynchronous transport, with a sensitive Bearer header.
    http: reqwest::Client,
    /// Immutable request and resource settings; never logged as an entire value.
    config: LlmReviewConfig,
}

impl ReviewClient {
    /// Build enabled settings without sending a startup request; disabled settings create no client.
    pub fn new(config: &LlmReviewConfig) -> anyhow::Result<Option<Self>> {
        config.validate()?;
        if !config.enabled {
            return Ok(None);
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", config.api_key))
            .context("Invalid review authorization header")?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .retry(reqwest::retry::never())
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .context("Build review HTTP client")?;
        Ok(Some(Self {
            http,
            config: config.clone(),
        }))
    }

    /// Prepare metadata that an admission service saves before queue handoff.
    pub fn pending(&self, trigger: ReviewTrigger, requested_at_ms: i64) -> Result<ReviewAttempt> {
        ReviewAttempt::pending(
            trigger,
            requested_at_ms,
            self.config.model.clone(),
            self.config.reasoning_effort.clone(),
            policy::PROMPT_VERSION.into(),
        )
    }

    /// Complete one attempt created by this client, independently of browser and database handles.
    ///
    /// The caller obtains fixed input and releases its database connection before this call,
    /// then conditionally persists the returned attempt. Dropping this future cancels local
    /// waiting; the review service owns interruption persistence and remote billing remains unknown.
    pub async fn review(
        &self,
        submission_id: &str,
        input: &ReviewInput,
        pending: ReviewAttempt,
    ) -> Result<ReviewAttempt> {
        if pending.status != ReviewStatus::Pending {
            return Err(Error::InvalidReview("The client requires a pending review"));
        }
        let started = Instant::now();
        let mut http_status = None;
        let response = self.request(input, &mut http_status).await;
        let completed_at_ms = now_ms()?;
        let completed = match response {
            Ok(parsed) => match parsed.outcome {
                Ok(result) => pending.succeeded(
                    completed_at_ms,
                    result,
                    parsed.response_id,
                    parsed.usage,
                    input.source.lines().count(),
                )?,
                Err(code) => pending.failed(
                    completed_at_ms,
                    code,
                    parsed.response_id,
                    parsed.usage,
                    parsed.raw_output_json,
                )?,
            },
            Err(code) => pending.failed(completed_at_ms, code, None, None, None)?,
        };
        let usage = completed.details.usage.as_ref();
        tracing::info!(
            submission = submission_id,
            review = %completed.id,
            trigger = ?completed.details.trigger,
            model = %completed.details.model,
            elapsed_ms = %started.elapsed().as_millis(),
            http_status,
            status = completed.status.as_str(),
            error_code = ?completed.details.error.as_ref().map(|error| error.code),
            input_tokens = ?usage.and_then(|usage| usage.input_tokens),
            output_tokens = ?usage.and_then(|usage| usage.output_tokens),
            reasoning_tokens = ?usage.and_then(|usage| usage.reasoning_tokens),
            "Review request completed"
        );
        Ok(completed)
    }

    /// Serialize before sending and bound response accumulation while the total HTTP timer runs.
    async fn request(
        &self,
        input: &ReviewInput,
        http_status: &mut Option<u16>,
    ) -> std::result::Result<ParsedResponse, ReviewErrorCode> {
        let body = policy::request(&self.config, input)
            .map_err(|_| ReviewErrorCode::ReviewInputUnavailable)?;
        if body.len() > self.config.max_request_bytes {
            return Err(ReviewErrorCode::ReviewInputTooLarge);
        }
        let mut response = self
            .http
            .post(&self.config.endpoint)
            .body(body)
            .send()
            .await
            .map_err(transport_error)?;
        *http_status = Some(response.status().as_u16());
        if !response.status().is_success() {
            return Err(ReviewErrorCode::ReviewApiFailed);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if chunk.len() > self.config.max_response_bytes - bytes.len() {
                return Err(ReviewErrorCode::ReviewInvalidOutput);
            }
            bytes.extend_from_slice(&chunk);
        }
        parse_response(&bytes)
    }
}

/// Metadata and unvalidated complete body retained separately from the opinion classification.
struct ParsedResponse {
    /// Provider response identity, without fabricated fallback IDs.
    response_id: Option<String>,
    /// Provider-reported usage only.
    usage: Option<ReviewUsage>,
    /// Typed opinion or stable failure; business validation also runs in ReviewAttempt.
    outcome: std::result::Result<ReviewResult, ReviewErrorCode>,
    /// Only the model's complete JSON object, never reasoning or the outer API envelope.
    raw_output_json: Option<Value>,
}

/// Relevant usage subset; extra provider metadata does not enter persistence or logs.
#[derive(Deserialize)]
struct ApiUsage {
    /// Optional actual reported input tokens.
    input_tokens: Option<u64>,
    /// Optional actual reported output tokens.
    output_tokens: Option<u64>,
    /// Optional reasoning breakdown.
    output_tokens_details: Option<ApiOutputTokens>,
}

/// Relevant output-token breakdown, with absence distinct from zero.
#[derive(Deserialize)]
struct ApiOutputTokens {
    /// Actual reported reasoning tokens.
    reasoning_tokens: Option<u64>,
}

/// Accumulated assistant text and shape flags, permitting diagnostics from incomplete responses.
#[derive(Default)]
struct AssistantOutput {
    /// Concatenated output_text parts in provider order.
    text: String,
    /// Refusal, tool output or another unsupported shape was encountered.
    invalid: bool,
    /// A message explicitly reports an unfinished state.
    incomplete: bool,
}

/// Extract assistant text only, retaining invalid flags without copying reasoning or tools.
fn assistant_output(envelope: &Value) -> AssistantOutput {
    let mut output = AssistantOutput::default();
    let Some(items) = envelope.get("output").and_then(Value::as_array) else {
        output.invalid = true;
        return output;
    };
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("reasoning") => continue,
            Some("message") if item.get("role").and_then(Value::as_str) == Some("assistant") => {}
            _ => {
                output.invalid = true;
                continue;
            }
        }
        match item.get("status") {
            None => {}
            Some(Value::String(status)) if status == "completed" => {}
            Some(Value::String(status))
                if matches!(status.as_str(), "incomplete" | "in_progress") =>
            {
                output.incomplete = true
            }
            Some(_) => output.invalid = true,
        }
        let Some(parts) = item.get("content").and_then(Value::as_array) else {
            output.invalid = true;
            continue;
        };
        for part in parts {
            match (
                part.get("type").and_then(Value::as_str),
                part.get("text").and_then(Value::as_str),
            ) {
                (Some("output_text"), Some(text)) => output.text.push_str(text),
                _ => output.invalid = true,
            }
        }
    }
    output
}

/// Parse the Responses subset, preserving complete opinion JSON on failures where allowed.
fn parse_response(bytes: &[u8]) -> std::result::Result<ParsedResponse, ReviewErrorCode> {
    let envelope: Value =
        serde_json::from_slice(bytes).map_err(|_| ReviewErrorCode::ReviewInvalidOutput)?;
    if !envelope.is_object() {
        return Err(ReviewErrorCode::ReviewInvalidOutput);
    }
    let api_failed = envelope.get("error").is_some_and(|error| !error.is_null())
        || envelope.get("status").and_then(Value::as_str) == Some("failed");
    let response_id = serde_json::from_value::<Option<String>>(
        envelope.get("id").cloned().unwrap_or(Value::Null),
    );
    let usage = serde_json::from_value::<Option<ApiUsage>>(
        envelope.get("usage").cloned().unwrap_or(Value::Null),
    );
    let invalid_metadata = response_id.is_err() || usage.is_err();
    let output = if api_failed {
        AssistantOutput::default()
    } else {
        assistant_output(&envelope)
    };
    let raw_output_json = serde_json::from_str::<Value>(&output.text)
        .ok()
        .filter(Value::is_object);
    let outcome = if api_failed {
        Err(ReviewErrorCode::ReviewApiFailed)
    } else {
        match envelope.get("status").and_then(Value::as_str) {
            Some("incomplete" | "in_progress" | "queued" | "cancelled") => {
                Err(ReviewErrorCode::ReviewIncomplete)
            }
            Some("completed")
                if envelope
                    .get("incomplete_details")
                    .is_some_and(|details| !details.is_null())
                    || output.incomplete =>
            {
                Err(ReviewErrorCode::ReviewIncomplete)
            }
            Some("completed") if !output.invalid && !invalid_metadata => raw_output_json
                .as_ref()
                .ok_or(ReviewErrorCode::ReviewInvalidOutput)
                .and_then(|raw| {
                    serde_json::from_value(raw.clone())
                        .map_err(|_| ReviewErrorCode::ReviewInvalidOutput)
                }),
            _ => Err(ReviewErrorCode::ReviewInvalidOutput),
        }
    };
    Ok(ParsedResponse {
        response_id: response_id.ok().flatten(),
        usage: usage.ok().flatten().map(|usage| ReviewUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            reasoning_tokens: usage
                .output_tokens_details
                .and_then(|details| details.reasoning_tokens),
        }),
        outcome,
        raw_output_json,
    })
}

/// Classify transport errors without logging URLs, credentials or provider-controlled text.
fn transport_error(error: reqwest::Error) -> ReviewErrorCode {
    if error.is_timeout() {
        ReviewErrorCode::ReviewTimeout
    } else {
        ReviewErrorCode::ReviewApiFailed
    }
}
