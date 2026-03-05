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

use axum::{routing::get, routing::post, Json, Router};
use serde::Serialize;
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
    ws_auth_mode: &'static str,
    help_enabled: bool,
}

async fn config() -> Json<ConfigResponse> {
    Json(ConfigResponse {
        ws_auth_mode: "oidc_broker",
        help_enabled: true,
    })
}
