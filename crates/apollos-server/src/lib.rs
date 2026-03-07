pub mod agent;
pub mod auth;
pub mod config;
pub mod gemini_bridge;
pub mod human_fallback;
pub mod prompts;
pub mod safety_policy;
pub mod session;
pub mod tools;
pub mod ws_handler;
pub mod ws_registry;

use axum::{
    extract::DefaultBodyLimit,
    http::{HeaderName, HeaderValue, Method},
    routing::get,
    routing::post,
    Json, Router,
};
use serde::Serialize;
use tower_http::{cors::Any, cors::CorsLayer, trace::TraceLayer};
use tracing::info;

#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub sessions: session::SessionStore,
    pub tools: tools::ToolRuntime,
    pub broker: auth::broker::BrokerService,
    pub fallback: human_fallback::HumanFallbackService,
    pub gemini: gemini_bridge::GeminiBridge,
    pub ws_registry: ws_registry::WebSocketRegistry,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/health", get(healthz))
        .route("/ready", get(healthz))
        .route("/config", get(config))
        .route("/ws/live/{session_id}", get(ws_handler::live_ws_handler))
        .route(
            "/ws/emergency/{session_id}",
            get(ws_handler::emergency_ws_handler),
        )
        .route("/ws/help/{session_id}", get(ws_handler::help_ws_handler))
        .route(
            "/auth/oidc/exchange",
            post(auth::broker::oidc_exchange_handler),
        )
        .route(
            "/auth/ws-ticket",
            post(auth::broker::issue_ws_ticket_handler),
        )
        .route("/auth/logout", post(auth::broker::logout_handler))
        .route(
            "/auth/help-ticket/exchange",
            post(human_fallback::help_ticket_exchange_handler),
        )
        .layer(DefaultBodyLimit::max(http_max_body_bytes()))
        .layer(build_cors_layer())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

async fn healthz() -> Json<HealthResponse> {
    info!("healthz");
    Json(HealthResponse {
        status: "ok",
        service: "apollos-server",
    })
}

#[derive(Debug, Serialize)]
struct ConfigResponse {
    ws_auth_mode: String,
    help_enabled: bool,
}

async fn config() -> Json<ConfigResponse> {
    Json(ConfigResponse {
        ws_auth_mode: std::env::var("WS_AUTH_MODE").unwrap_or_else(|_| "oidc_broker".to_string()),
        help_enabled: true,
    })
}

fn build_cors_layer() -> CorsLayer {
    let raw = std::env::var("CORS_ALLOW_ORIGINS").unwrap_or_else(|_| "*".to_string());
    let origins = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    let mut cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    if origins.is_empty() || origins.contains(&"*") {
        cors = cors.allow_origin(Any);
    } else {
        let parsed = origins
            .iter()
            .filter_map(|origin| HeaderValue::from_str(origin).ok())
            .collect::<Vec<_>>();
        if parsed.is_empty() {
            cors = cors.allow_origin(Any);
        } else {
            cors = cors.allow_origin(parsed);
        }
    }

    cors.expose_headers([
        HeaderName::from_static("x-request-id"),
        HeaderName::from_static("sec-websocket-protocol"),
    ])
}

fn http_max_body_bytes() -> usize {
    std::env::var("HTTP_MAX_BODY_BYTES")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(2 * 1024 * 1024)
        .clamp(64 * 1024, 8 * 1024 * 1024)
}
