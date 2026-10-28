//! Real-MySQL auth exchanges and request guards over the production Tower router.

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{Connection, Row};
use tower::ServiceExt;

use crate::support::{Fixture, Harness};

/// Browser-facing Origin used by the ordinary fixture configuration.
const ORIGIN: &str = "http://127.0.0.1:8080";

/// Explicit headers let tests represent anonymous, stale-page and rejected-origin requests.
async fn request(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
    expected: Option<i64>,
    origin: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    if let Some(expected) = expected {
        builder = builder.header("x-acorn-user-id", expected);
    }
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    let body = if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(headers["cache-control"], "no-store");
    (status, headers, body)
}

/// Retain only the cookie pair a browser sends back, never its response attributes.
fn cookie(headers: &HeaderMap) -> String {
    headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

/// Registration, login replacement, errors, guarded routes and idempotent logout share one contract.
#[tokio::test]
async fn authentication_routes_and_request_guards() {
    let harness = Harness::start(1, 8).await;
    let app = &harness.raw_app;
    let (_, _, me) = request(app, "GET", "/api/v1/auth/me", None, None, None, None).await;
    assert_eq!(me, json!({"user":null}));
    for path in [
        "/api/v1/health",
        "/api/v1/submissions",
        "/api/v1/submissions/bad-id",
    ] {
        let (status, headers, body) = request(app, "GET", path, None, None, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(headers["www-authenticate"], "Session realm=\"acorn\"");
        assert_eq!(body["error"]["code"], "AuthenticationRequired");
    }
    let registration =
        json!({"username":" New_User ", "password":"Acorn-User-123!", "display_name":" 新用户 "});
    for origin in [None, Some("null"), Some("https://elsewhere.example")] {
        let (status, _, body) = request(
            app,
            "POST",
            "/api/v1/auth/register",
            Some(registration.clone()),
            None,
            None,
            origin,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "RequestOriginRejected");
    }
    let (status, headers, registered) = request(
        app,
        "POST",
        "/api/v1/auth/register",
        Some(registration.clone()),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(!headers.contains_key("set-cookie"));
    let id = registered["user"]["id"].as_i64().unwrap();
    assert_eq!(
        registered,
        json!({"user":{"id":id,"username":"new_user","display_name":"新用户"}})
    );
    assert_eq!(
        request(app, "GET", "/api/v1/auth/me", None, None, None, None)
            .await
            .2,
        json!({"user":null})
    );
    let (status, _, body) = request(
        app,
        "POST",
        "/api/v1/auth/register",
        Some(registration),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "UsernameTaken");
    let (status, _, _) = request(
        app,
        "POST",
        "/api/v1/auth/register",
        Some(json!({"username":"test","password":"12345678","role":"admin"})),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let login = json!({"username":"NEW_USER", "password":"Acorn-User-123!"});
    let (status, headers, logged_in) = request(
        app,
        "POST",
        "/api/v1/auth/login",
        Some(login.clone()),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(logged_in, registered);
    assert!(
        headers["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=86400")
    );
    let first = cookie(&headers);
    let (_, headers, me) = request(
        app,
        "GET",
        "/api/v1/auth/me",
        None,
        Some(&first),
        None,
        None,
    )
    .await;
    assert_eq!(me, registered);
    assert_eq!(cookie(&headers), first);
    for username in ["new_user", "unknown_user"] {
        let (status, headers, body) = request(
            app,
            "POST",
            "/api/v1/auth/login",
            Some(json!({"username":username,"password":"wrong-password"})),
            Some(&first),
            None,
            Some(ORIGIN),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(headers["www-authenticate"], "Session realm=\"acorn\"");
        assert_eq!(body["error"]["code"], "InvalidCredentials");
        assert_eq!(cookie(&headers), first);
    }
    for (method, path, body) in [
        ("GET", "/api/v1/submissions/bad-id", None),
        ("POST", "/api/v1/submissions", Some(json!({}))),
        ("POST", "/api/v1/submissions/bad-id/review", Some(json!({}))),
        ("POST", "/api/v1/auth/logout", Some(json!({}))),
    ] {
        let (status, _, body) = request(
            app,
            method,
            path,
            body,
            Some(&first),
            Some(id + 1),
            Some(ORIGIN),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "SessionChanged");
    }
    assert_eq!(
        request(app, "GET", "/api/v1/health", None, Some(&first), None, None)
            .await
            .0,
        StatusCode::OK
    );
    let (status, _, body) = request(
        app,
        "POST",
        "/api/v1/submissions",
        Some(json!({})),
        Some(&first),
        Some(id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "RequestOriginRejected");
    let mut connection = harness.fixture.connection().await;
    assert_eq!(
        sqlx::query("SELECT CAST(COUNT(*) AS SIGNED) AS count FROM submissions")
            .fetch_one(&mut connection)
            .await
            .unwrap()
            .try_get::<i64, _>("count")
            .unwrap(),
        0
    );
    connection.close().await.unwrap();
    let (status, headers, _) = request(
        app,
        "POST",
        "/api/v1/auth/login",
        Some(login),
        Some(&first),
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let second = cookie(&headers);
    assert_ne!(first, second);
    assert_eq!(
        request(
            app,
            "GET",
            "/api/v1/auth/me",
            None,
            Some(&first),
            None,
            None
        )
        .await
        .2,
        json!({"user":null})
    );
    assert_eq!(
        request(
            app,
            "GET",
            "/api/v1/auth/me",
            None,
            Some(&second),
            None,
            None
        )
        .await
        .2,
        registered
    );
    for invalid in [json!([]), Value::Null, json!({"unexpected":true})] {
        let (status, _, body) = request(
            app,
            "POST",
            "/api/v1/auth/logout",
            Some(invalid),
            Some(&second),
            Some(id),
            Some(ORIGIN),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "InvalidRequest");
    }
    assert_eq!(
        request(
            app,
            "GET",
            "/api/v1/auth/me",
            None,
            Some(&second),
            None,
            None
        )
        .await
        .2,
        registered
    );
    let (status, headers, body) = request(
        app,
        "POST",
        "/api/v1/auth/logout",
        Some(json!({})),
        Some(&second),
        Some(id),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"user":null}));
    assert!(
        headers["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(
        request(
            app,
            "POST",
            "/api/v1/auth/logout",
            Some(json!({})),
            Some(&second),
            None,
            Some(ORIGIN)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app,
            "GET",
            "/api/v1/auth/me",
            None,
            Some(&second),
            None,
            None
        )
        .await
        .2,
        json!({"user":null})
    );
    harness.close().await;
}

/// Secure cookies follow the external HTTPS Origin and capacity evicts old browser sessions.
#[tokio::test]
async fn secure_cookie_and_session_capacity() {
    let mut fixture = Fixture::new().await;
    fixture.config.http.public_origin = "https://acorn.example".into();
    fixture.config.http.cookie_secure = true;
    fixture.config.auth.max_sessions_per_user = 1;
    fixture.config.auth.max_sessions = 1;
    // This test uses raw routes because its deployment policy differs from the ordinary fixture.
    let harness = Harness::start_fixture(fixture, 1).await;
    let app = &harness.raw_app;
    let origin = Some("https://acorn.example");
    let credentials = json!({"username":"test","password":"Acorn-Test-Only-123!"});
    let (_, headers, _) = request(
        app,
        "POST",
        "/api/v1/auth/login",
        Some(credentials.clone()),
        None,
        None,
        origin,
    )
    .await;
    let first = cookie(&headers);
    assert!(first.starts_with("__Host-acorn_session="));
    assert!(
        headers["set-cookie"]
            .to_str()
            .unwrap()
            .ends_with("; Secure")
    );
    let (_, headers, _) = request(
        app,
        "POST",
        "/api/v1/auth/login",
        Some(credentials),
        None,
        None,
        origin,
    )
    .await;
    let second = cookie(&headers);
    assert_eq!(
        request(
            app,
            "GET",
            "/api/v1/auth/me",
            None,
            Some(&first),
            None,
            None
        )
        .await
        .2,
        json!({"user":null})
    );
    let account = json!({"username":"other","password":"Acorn-Other-123!"});
    assert_eq!(
        request(
            app,
            "POST",
            "/api/v1/auth/register",
            Some(account.clone()),
            None,
            None,
            origin
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (_, headers, _) = request(
        app,
        "POST",
        "/api/v1/auth/login",
        Some(account),
        None,
        None,
        origin,
    )
    .await;
    let third = cookie(&headers);
    assert_eq!(
        request(
            app,
            "GET",
            "/api/v1/auth/me",
            None,
            Some(&second),
            None,
            None
        )
        .await
        .2,
        json!({"user":null})
    );
    assert_eq!(
        request(
            app,
            "GET",
            "/api/v1/auth/me",
            None,
            Some(&third),
            None,
            None
        )
        .await
        .2["user"]["username"],
        "other"
    );
    let (status, headers, _) = request(
        app,
        "POST",
        "/api/v1/auth/logout",
        Some(json!({})),
        Some(&third),
        None,
        origin,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        headers["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=86400")
    );
    harness.close().await;
}
