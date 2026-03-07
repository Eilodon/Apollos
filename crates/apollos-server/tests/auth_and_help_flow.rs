use apollos_server::{build_router, AppState};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use tower::ServiceExt;

fn configure_test_env() {
    std::env::set_var("APP_ENV", "development");
    std::env::set_var("OIDC_ALLOW_INSECURE_DEV_TOKENS", "1");
    std::env::set_var("TWILIO_ACCOUNT_SID", "AC123");
    std::env::set_var("TWILIO_VIDEO_API_KEY_SID", "SK123");
    std::env::set_var("TWILIO_VIDEO_API_KEY_SECRET", "secret");
    std::env::set_var("TWILIO_REQUIRED", "0");
}

async fn post_json(app: axum::Router, path: &str, payload: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .expect("request should build");

    let response = app.oneshot(request).await.expect("request should complete");
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body should decode");

    let parsed = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice::<Value>(&body).expect("body should be json")
    };

    (status, parsed)
}

#[tokio::test]
async fn oidc_broker_flow_revokes_session_tokens() {
    configure_test_env();
    let app = build_router(AppState::default());

    let (exchange_status, exchange_body) = post_json(
        app.clone(),
        "/auth/oidc/exchange",
        json!({ "id_token": "abcdefghijklmnopqrstuvwxyz-1234567890" }),
    )
    .await;
    assert_eq!(exchange_status, StatusCode::OK);

    let session_token = exchange_body
        .get("session_token")
        .and_then(Value::as_str)
        .expect("session token expected")
        .to_string();

    let (ws_status, ws_body) = post_json(
        app.clone(),
        "/auth/ws-ticket",
        json!({ "session_token": session_token }),
    )
    .await;
    assert_eq!(ws_status, StatusCode::OK);
    assert!(ws_body.get("access_token").is_some());

    let (logout_status, logout_body) = post_json(
        app.clone(),
        "/auth/logout",
        json!({ "session_token": session_token }),
    )
    .await;
    assert_eq!(logout_status, StatusCode::OK);
    assert_eq!(logout_body.get("ok").and_then(Value::as_bool), Some(true));

    let (retry_status, _) = post_json(
        app,
        "/auth/ws-ticket",
        json!({ "session_token": session_token }),
    )
    .await;
    assert_eq!(retry_status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn help_ticket_exchange_is_one_time() {
    configure_test_env();
    let state = AppState::default();
    let help_session = state
        .fallback
        .create_help_session("s1", "manual")
        .await
        .expect("should create help session");
    let app = build_router(state.clone());

    let link = help_session.help_link.expect("help link should exist");
    let help_ticket = link
        .split("help_ticket=")
        .nth(1)
        .expect("help ticket should exist")
        .to_string();

    let (first_status, first_body) = post_json(
        app.clone(),
        "/auth/help-ticket/exchange",
        json!({ "help_ticket": help_ticket }),
    )
    .await;
    assert_eq!(first_status, StatusCode::OK);
    assert!(first_body.get("viewer_token").is_some());

    let (second_status, _) = post_json(
        app,
        "/auth/help-ticket/exchange",
        json!({ "help_ticket": help_ticket }),
    )
    .await;
    assert_eq!(second_status, StatusCode::UNAUTHORIZED);
}
