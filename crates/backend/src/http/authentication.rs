//! Authentication routes and a shared session, expected-identity and Origin boundary.

use axum::{
    Extension, Json, Router,
    extract::{Request, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Map, Value};

use super::{
    AppState,
    dto::{AuthResponse, LoginRequest, RegisterRequest},
    error::ApiError,
};
use crate::auth::{self, AuthenticatedUser, User};

/// Route-specific access contract, kept beside explicit router groups.
#[derive(Clone, Copy)]
pub(super) enum Access {
    /// Anonymous identity inspection, login or registration.
    Optional,
    /// A valid session, without a page identity requirement.
    Required,
    /// A valid session whose identity matches the page's expected user.
    Personal,
    /// Anonymous logout is allowed; a valid session requires the expected identity.
    Logout,
}

/// Request-local session snapshot; never debug-formatted or serialized.
#[derive(Clone)]
struct CurrentSession {
    /// Opaque browser token, also retained for login replacement and logout.
    token: Option<String>,
    /// Identity authenticated before any business extraction or query.
    user: Option<User>,
}

/// Compose all four auth routes with their documented anonymous and logout policies.
pub(super) fn routes(state: &AppState) -> Router<AppState> {
    guarded(
        Router::new()
            .route("/api/v1/auth/register", post(register))
            .route("/api/v1/auth/login", post(login))
            .route("/api/v1/auth/me", get(me)),
        state,
        Access::Optional,
    )
    .merge(guarded(
        Router::new().route("/api/v1/auth/logout", post(logout)),
        state,
        Access::Logout,
    ))
}

/// Apply one shared boundary before extractors, with a route-specific access requirement.
pub(super) fn guarded(
    routes: Router<AppState>,
    state: &AppState,
    access: Access,
) -> Router<AppState> {
    routes.route_layer(middleware::from_fn_with_state(
        (state.clone(), access),
        boundary,
    ))
}

/// Authenticate once, authorize before business work, then uniformly renew and prevent caching.
async fn boundary(
    State((state, access)): State<(AppState, Access)>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = state.cookies.read(request.headers());
    let user = match token
        .as_deref()
        .map(|token| state.sessions.authenticate(token))
        .transpose()
    {
        Ok(user) => user.flatten(),
        Err(error) => {
            let mut response = ApiError::internal(error).into_response();
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-store"),
            );
            return response;
        }
    };
    let decision = authorize(
        &state,
        access,
        request.headers(),
        request.method() == axum::http::Method::POST,
        user.as_ref(),
    );
    let renew = user.is_some();
    let mut response = match decision {
        Err(error) => error.into_response(),
        Ok(()) => {
            if let Some(user) = &user {
                request
                    .extensions_mut()
                    .insert(AuthenticatedUser { user: user.clone() });
            }
            request.extensions_mut().insert(CurrentSession {
                token: token.clone(),
                user,
            });
            next.run(request).await
        }
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    if renew {
        // A successful login or logout already selected its replacement or deletion cookie.
        response
            .headers_mut()
            .entry(header::SET_COOKIE)
            .or_insert_with(|| state.cookies.header(token.as_deref()));
    }
    response
}

/// Enforce the page identity and JSON write Origin without trusting either as authentication.
fn authorize(
    state: &AppState,
    access: Access,
    headers: &HeaderMap,
    write: bool,
    user: Option<&User>,
) -> Result<(), ApiError> {
    if matches!(access, Access::Required | Access::Personal) && user.is_none() {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "AuthenticationRequired",
            "A valid session is required.",
        ));
    }
    if write
        && (headers.get_all(header::ORIGIN).iter().count() != 1
            || headers
                .get(header::ORIGIN)
                .and_then(|value| value.to_str().ok())
                != Some(state.config.http.public_origin.as_str()))
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "RequestOriginRejected",
            "The request Origin is not allowed.",
        ));
    }
    if matches!(access, Access::Personal | Access::Logout)
        && let Some(user) = user
    {
        let expected = headers
            .get("x-acorn-user-id")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok());
        if headers.get_all("x-acorn-user-id").iter().count() != 1 || expected != Some(user.id) {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "SessionChanged",
                "The session account has changed. Refresh the current identity.",
            ));
        }
    }
    Ok(())
}

/// Validate and persist a new account, leaving login to a separate user action.
async fn register(
    State(state): State<AppState>,
    body: Result<Json<RegisterRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let input = body?.0.validate().map_err(|_| ApiError::invalid())?;
    let hash = auth::hash_password(input.password)
        .await
        .map_err(ApiError::internal)?;
    let user = state
        .database
        .create_user(&input.username, &input.display_name, &hash)
        .await?;
    Ok((StatusCode::CREATED, Json(AuthResponse::from(Some(user)))).into_response())
}

/// Verify after lookup has released its connection, then atomically replace this browser's session.
async fn login(
    State(state): State<AppState>,
    Extension(current): Extension<CurrentSession>,
    body: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let input = body?.0.validate().map_err(|_| ApiError::invalid())?;
    let invalid = || {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "InvalidCredentials",
            "The username or password is incorrect.",
        )
    };
    let credentials = state
        .database
        .user_by_username(&input.username)
        .await?
        .ok_or_else(invalid)?;
    if !auth::verify_password(input.password, credentials.password_hash)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(invalid());
    }
    let token = state
        .sessions
        .create(credentials.user.clone(), current.token.as_deref())
        .map_err(ApiError::internal)?;
    Ok((
        [(header::SET_COOKIE, state.cookies.header(Some(&token)))],
        Json(AuthResponse::from(Some(credentials.user))),
    )
        .into_response())
}

/// Return the already authenticated snapshot without querying account storage.
async fn me(Extension(current): Extension<CurrentSession>) -> Json<AuthResponse> {
    Json(AuthResponse::from(current.user))
}

/// Delete only the admitted current token and clear its browser cookie; repeated logout succeeds.
async fn logout(
    State(state): State<AppState>,
    Extension(current): Extension<CurrentSession>,
    body: Result<Json<Map<String, Value>>, JsonRejection>,
) -> Result<Response, ApiError> {
    if !body?.0.is_empty() {
        return Err(ApiError::invalid());
    }
    if let Some(token) = current.token {
        state.sessions.remove(&token).map_err(ApiError::internal)?;
    }
    Ok((
        [(header::SET_COOKIE, state.cookies.header(None))],
        Json(AuthResponse::from(None)),
    )
        .into_response())
}
