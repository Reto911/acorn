//! Business and health JSON routes, bounded extraction and explicit public response DTOs.

mod dto;
mod error;

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use tower::ServiceBuilder;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use uuid::Uuid;

use crate::{
    config::Config,
    database::Database,
    health::{self, Health},
    llm_review::ReviewService,
    scheduler::Scheduler,
    submissions::{CreateSubmission, CreationService, SubmissionFilter},
};
use error::ApiError;

/// Shared request state for persisted business data and passive health observations.
#[derive(Clone)]
struct AppState {
    /// Deployment settings, not request-controlled execution parameters.
    config: Arc<Config>,
    /// The same SQLite handle used by Scheduler and creation.
    database: Database,
    /// Independently owned transaction-and-handoff service.
    creation: CreationService,
    /// Local lifecycle and capacity, shared with dispatch and creation.
    scheduler: Scheduler,
    /// The single collector's cache; HTTP never initiates a probe.
    health: Health,
    /// Same independently supervised review service used by Scheduler automatic handoffs.
    review: ReviewService,
}

/// Build the API with common body limits, English JSON errors and request tracing.
pub fn router(
    config: &Config,
    database: Database,
    creation: CreationService,
    scheduler: Scheduler,
    health: Health,
    review: ReviewService,
) -> Router {
    let state = AppState {
        config: Arc::new(config.clone()),
        database,
        creation,
        scheduler,
        health,
        review,
    };
    Router::new()
        .route("/api/v1/health", get(get_health))
        .route("/api/v1/problems", get(list_problems))
        .route("/api/v1/problems/{id}", get(problem))
        .route("/api/v1/problems/{id}/versions/{version_no}", get(problem_version))
        .route("/api/v1/submissions", get(list_submissions).post(create_submission))
        .route("/api/v1/submissions/{id}", get(submission))
        .route("/api/v1/submissions/{id}/review", post(start_review))
        .fallback(route_not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(config.http.max_request_bytes))
        .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()
            .make_span_with(|request: &axum::http::Request<axum::body::Body>| {
                tracing::info_span!("http", method = %request.method(), path = request.uri().path())
            })
            .on_response(DefaultOnResponse::new().level(tracing::Level::DEBUG))))
        .with_state(state)
}

/// Return passive component observations with no-store even if response assembly fails.
async fn get_health(State(state): State<AppState>) -> Response {
    (
        [(header::CACHE_CONTROL, "no-store")],
        health_snapshot(&state).map(Json),
    )
        .into_response()
}

/// Assemble the documented public fields with a real backend UTC snapshot timestamp.
fn health_snapshot(state: &AppState) -> Result<dto::HealthSnapshot, ApiError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(ApiError::internal)?;
    let snapshot_at_ms = i64::try_from(elapsed.as_millis()).map_err(ApiError::internal)?;
    Ok(dto::HealthSnapshot::new(
        snapshot_at_ms,
        health::local_snapshot(&state.creation, &state.scheduler),
        state.health.snapshot(),
    ))
}

/// Apply the configured default and reject invalid explicit pagination values.
fn pagination(
    limit: Option<u32>,
    offset: Option<u64>,
    config: &Config,
) -> Result<(u32, u64), ApiError> {
    let limit = limit.unwrap_or(config.http.default_page_size);
    let offset = offset.unwrap_or(0);
    if limit == 0 || limit > config.http.max_page_size || offset > i64::MAX as u64 {
        return Err(ApiError::invalid());
    }
    Ok((limit, offset))
}

/// List current published titles without source, statement or private test data.
async fn list_problems(
    State(state): State<AppState>,
    query: Result<Query<dto::PageQuery>, QueryRejection>,
) -> Result<Json<dto::Page<dto::ProblemSummary>>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::invalid())?;
    let (limit, offset) = pagination(query.limit, query.offset, &state.config)?;
    let items = state
        .database
        .list_problems(limit, offset)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(dto::Page {
        items,
        limit,
        offset,
    }))
}

/// Read a current published version; unpublished or missing problems return 404.
async fn problem(
    State(state): State<AppState>,
    path: Result<Path<i64>, PathRejection>,
) -> Result<Json<dto::Problem>, ApiError> {
    let Path(id) = path.map_err(|_| ApiError::invalid())?;
    if id <= 0 {
        return Err(ApiError::invalid());
    }
    let problem = state.database.problem(id).await?.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "ProblemNotFound",
            "The problem was not found.",
        )
    })?;
    Ok(Json(problem.into()))
}

/// Read a retained published version without changing new-submission version selection.
async fn problem_version(
    State(state): State<AppState>,
    path: Result<Path<(i64, i64)>, PathRejection>,
) -> Result<Json<dto::Problem>, ApiError> {
    let Path((id, version)) = path.map_err(|_| ApiError::invalid())?;
    if id <= 0 || version <= 0 {
        return Err(ApiError::invalid());
    }
    let problem = state
        .database
        .problem_version(id, version)
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "ProblemVersionNotFound",
                "The problem version was not found.",
            )
        })?;
    Ok(Json(problem.into()))
}

/// Return configured-user history with typed filters and bounded pagination.
async fn list_submissions(
    State(state): State<AppState>,
    query: Result<Query<dto::HistoryQuery>, QueryRejection>,
) -> Result<Json<dto::Page<dto::SubmissionSummary>>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::invalid())?;
    if query.problem_id.is_some_and(|id| id <= 0) {
        return Err(ApiError::invalid());
    }
    let (limit, offset) = pagination(query.limit, query.offset, &state.config)?;
    let filter = SubmissionFilter {
        problem_id: query.problem_id,
        state: query.state,
        verdict: query.verdict,
        review_status: query.review_status,
    };
    let items = state
        .database
        .list_submissions(&filter, limit, offset)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(dto::Page {
        items,
        limit,
        offset,
    }))
}

/// Query only persisted state and parsed terminal details, never the Worker.
async fn submission(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<dto::Submission>, ApiError> {
    let Path(id) = path.map_err(|_| ApiError::invalid())?;
    let id = Uuid::parse_str(&id)
        .map_err(|_| ApiError::invalid())?
        .to_string();
    let submission = state.database.submission(&id).await?.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "SubmissionNotFound",
            "The submission was not found.",
        )
    })?;
    Ok(Json(submission.into()))
}

/// Validate the HTTP boundary and await a creation service that survives disconnects.
async fn create_submission(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<dto::CreateRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(body) = body?;
    let invalid_key = || {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "InvalidIdempotencyKey",
            "A valid UUID Idempotency-Key is required.",
        )
    };
    if headers.get_all("idempotency-key").iter().count() != 1 {
        return Err(invalid_key());
    }
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(invalid_key)?;
    let request_key = Uuid::parse_str(key).map_err(|_| invalid_key())?;
    let created = state
        .creation
        .create(CreateSubmission {
            problem_id: body.problem_id,
            language_id: body.language_id,
            source: body.source.into_bytes(),
            request_key,
        })
        .await?;
    let status = if created.created {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    let location = format!("/api/v1/submissions/{}", created.info.id);
    Ok((
        status,
        [(header::LOCATION, location)],
        Json(dto::CreateResponse {
            submission_id: created.info.id,
            state: created.info.state,
        }),
    )
        .into_response())
}

/// Start or reuse a manual review through an operation owned independently of the HTTP waiter.
async fn start_review(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<dto::ReviewRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Path(id) = path.map_err(|_| ApiError::invalid())?;
    let id = Uuid::parse_str(&id)
        .map_err(|_| ApiError::invalid())?
        .to_string();
    let Json(body) = body?;
    let admitted = state
        .review
        .start_manual(id, body.expected_review_id)
        .await?;
    let status = if admitted.created {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    let location = format!("/api/v1/submissions/{}", admitted.submission_id);
    Ok((
        status,
        [(header::LOCATION, location)],
        Json(dto::ReviewResponse::from(admitted)),
    )
        .into_response())
}

/// Ensure unmatched paths use the same JSON error contract as handlers.
async fn route_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "RouteNotFound",
        "The API route was not found.",
    )
}

/// Ensure a matched path with an unsupported method returns the agreed JSON error.
async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "MethodNotAllowed",
        "The HTTP method is not allowed for this route.",
    )
}
