//! Uniform English JSON errors for business, extraction and routing failures.

use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{
    database, llm_review::ManualReviewError, scheduler::AdmissionError, submissions::CreationError,
};

/// Stable JSON error payload; internal causes are logged separately.
#[derive(Serialize)]
struct Body {
    /// Frontend-readable stable error code.
    code: &'static str,
    /// English description of the failed HTTP operation.
    message: &'static str,
    /// Only a queue-full rehandoff includes the original submission ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    submission_id: Option<String>,
}

/// Envelope shared by handlers and framework rejection conversion.
#[derive(Serialize)]
struct Envelope {
    /// Operation error, distinct from a stored execution failure.
    error: Body,
}

/// HTTP status and body, with no raw storage or transport error text.
pub(super) struct ApiError {
    /// Status corresponding to the design's fixed error table.
    status: StatusCode,
    /// Stable code, safe message and optional queued identity.
    body: Body,
}

impl ApiError {
    /// Build one of the documented HTTP errors.
    pub fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            body: Body {
                code,
                message,
                submission_id: None,
            },
        }
    }

    /// Reject malformed or unallowed request data without reflecting its contents.
    pub fn invalid() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "The request is invalid.",
        )
    }

    /// Log original context while returning a stable generic internal error.
    pub fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "HTTP operation failed");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalError",
            "The operation could not be completed.",
        )
    }
}

impl IntoResponse for ApiError {
    /// Serialize all handler and framework errors through the same envelope.
    fn into_response(self) -> Response {
        (self.status, Json(Envelope { error: self.body })).into_response()
    }
}

impl From<database::Error> for ApiError {
    /// Preserve expected business errors and hide internal SQL and stored-data context.
    fn from(error: database::Error) -> Self {
        use database::Error;
        match error {
            Error::ProblemNotFound => Self::new(
                StatusCode::NOT_FOUND,
                "ProblemNotFound",
                "The problem was not found.",
            ),
            Error::LanguageNotAllowed => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "LanguageNotAllowed",
                "The language is not allowed for this problem.",
            ),
            Error::UnsupportedLanguage => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "UnsupportedLanguage",
                "The language is not supported.",
            ),
            Error::InvalidRequest(_) => Self::invalid(),
            Error::SourceTooLarge => Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "SourceTooLarge",
                "The source exceeds the byte limit.",
            ),
            Error::IdempotencyConflict => Self::new(
                StatusCode::CONFLICT,
                "IdempotencyConflict",
                "The idempotency key belongs to different content.",
            ),
            error => Self::internal(error),
        }
    }
}

impl From<CreationError> for ApiError {
    /// Expose an existing queued ID only when local rehandoff lacks capacity.
    fn from(error: CreationError) -> Self {
        match error {
            CreationError::Database(error) => error.into(),
            CreationError::WorkersUnavailable => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "WorkersUnavailable",
                "No fresh positive Worker group observation is available. Retry later.",
            ),
            CreationError::Admission {
                cause: AdmissionError::QueueFull,
                submission_id,
            } => {
                let mut error = Self::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "QueueFull",
                    "The submission queue is full.",
                );
                error.body.submission_id = submission_id;
                error
            }
            CreationError::Admission {
                cause: AdmissionError::Stopping,
                ..
            } => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "BackendStopping",
                "The backend is stopping.",
            ),
            error => Self::internal(error),
        }
    }
}

impl From<ManualReviewError> for ApiError {
    /// Map review admission failures independently of stored review results.
    fn from(error: ManualReviewError) -> Self {
        match error {
            ManualReviewError::NotFound => Self::new(
                StatusCode::NOT_FOUND,
                "SubmissionNotFound",
                "The submission was not found.",
            ),
            ManualReviewError::NotFinished => Self::new(
                StatusCode::CONFLICT,
                "JudgingNotFinished",
                "Machine judging has not finished.",
            ),
            ManualReviewError::Unavailable => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "ReviewUnavailable",
                "The review service is unavailable.",
            ),
            ManualReviewError::Stopping => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "BackendStopping",
                "The backend is stopping.",
            ),
            ManualReviewError::Changed => Self::new(
                StatusCode::CONFLICT,
                "ReviewChanged",
                "The latest review has changed. Refresh the submission.",
            ),
            ManualReviewError::QueueFull => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "ReviewQueueFull",
                "The review queue is full.",
            ),
            ManualReviewError::Database(error) => error.into(),
            error => Self::internal(error),
        }
    }
}

impl From<JsonRejection> for ApiError {
    /// Keep the JSON media type and body limit errors distinct from malformed input.
    fn from(error: JsonRejection) -> Self {
        match error.status() {
            StatusCode::PAYLOAD_TOO_LARGE => Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "RequestTooLarge",
                "The request body exceeds the byte limit.",
            ),
            StatusCode::UNSUPPORTED_MEDIA_TYPE => Self::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "UnsupportedMediaType",
                "A JSON Content-Type is required.",
            ),
            _ => Self::invalid(),
        }
    }
}
