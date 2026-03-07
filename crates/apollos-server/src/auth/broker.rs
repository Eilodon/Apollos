use std::{collections::HashMap, sync::Arc};

use axum::{extract::State, http::StatusCode, Json};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Clone)]
struct SessionRecord {
    subject: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct WsTicketRecord {
    subject: String,
    session_token: String,
    expires_at: DateTime<Utc>,
}

// TODO: KRONOS-CRITICAL: Di chuyển BrokerState sang Redis
#[derive(Debug, Default)]
struct BrokerState {
    sessions: HashMap<String, SessionRecord>,
    ws_tickets: HashMap<String, WsTicketRecord>,
    revoked_sessions: HashMap<String, DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct BrokerService {
    session_ttl_seconds: i64,
    ws_ttl_seconds: i64,
    state: Arc<RwLock<BrokerState>>,
}

#[derive(Debug, Clone)]
pub struct WsClaims {
    pub subject: String,
    pub session_token: String,
    pub expires_at: DateTime<Utc>,
}

impl Default for BrokerService {
    fn default() -> Self {
        let session_ttl_seconds = std::env::var("OIDC_BROKER_SESSION_TTL_S")
            .ok()
            .and_then(|raw| raw.parse::<i64>().ok())
            .unwrap_or(3600)
            .max(60);
        let ws_ttl_seconds = std::env::var("OIDC_BROKER_WS_TTL_S")
            .ok()
            .and_then(|raw| raw.parse::<i64>().ok())
            .unwrap_or(120)
            .max(30);

        Self {
            session_ttl_seconds,
            ws_ttl_seconds,
            state: Arc::new(RwLock::new(BrokerState::default())),
        }
    }
}

impl BrokerService {
    pub fn session_ttl_seconds(&self) -> i64 {
        self.session_ttl_seconds
    }

    pub fn ws_ttl_seconds(&self) -> i64 {
        self.ws_ttl_seconds
    }

    pub async fn create_session(&self, subject: String) -> String {
        self.prune().await;

        let token = mint_token("sess", &subject);
        let expires_at = Utc::now() + Duration::seconds(self.session_ttl_seconds);

        let mut guard = self.state.write().await;
        guard.sessions.insert(
            token.clone(),
            SessionRecord {
                subject,
                expires_at,
            },
        );
        token
    }

    pub async fn revoke_session(&self, session_token: &str) {
        let mut guard = self.state.write().await;

        if let Some(record) = guard.sessions.remove(session_token) {
            guard
                .revoked_sessions
                .insert(session_token.to_string(), record.expires_at);
        }

        guard
            .ws_tickets
            .retain(|_, ticket| ticket.session_token != session_token);
    }

    pub async fn issue_ws_ticket(&self, session_token: &str) -> Option<(String, u64)> {
        self.prune().await;

        let (subject, expires_at) = {
            let guard = self.state.read().await;

            if guard.revoked_sessions.contains_key(session_token) {
                return None;
            }

            let session = guard.sessions.get(session_token)?;
            if Utc::now() > session.expires_at {
                return None;
            }

            (session.subject.clone(), session.expires_at)
        };

        if Utc::now() > expires_at {
            return None;
        }

        let ws_token = mint_token("ws", &subject);
        let ws_expires = Utc::now() + Duration::seconds(self.ws_ttl_seconds);

        let mut guard = self.state.write().await;
        guard.ws_tickets.insert(
            ws_token.clone(),
            WsTicketRecord {
                subject,
                session_token: session_token.to_string(),
                expires_at: ws_expires,
            },
        );

        Some((ws_token, self.ws_ttl_seconds as u64))
    }

    pub async fn verify_ws_ticket(&self, ws_token: &str) -> Option<WsClaims> {
        self.prune().await;

        let guard = self.state.read().await;
        let record = guard.ws_tickets.get(ws_token)?;

        if Utc::now() > record.expires_at {
            return None;
        }

        if guard.revoked_sessions.contains_key(&record.session_token) {
            return None;
        }

        Some(WsClaims {
            subject: record.subject.clone(),
            session_token: record.session_token.clone(),
            expires_at: record.expires_at,
        })
    }

    async fn prune(&self) {
        let mut guard = self.state.write().await;
        let now = Utc::now();

        guard.sessions.retain(|_, session| session.expires_at > now);
        guard.ws_tickets.retain(|_, ticket| ticket.expires_at > now);
        guard.revoked_sessions.retain(|_, expiry| *expiry > now);
    }
}

fn mint_token(prefix: &str, subject: &str) -> String {
    let raw = format!(
        "{prefix}:{subject}:{}:{}",
        Utc::now().timestamp(),
        Uuid::new_v4()
    );
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

#[derive(Debug, Deserialize)]
pub struct OidcExchangeRequest {
    pub id_token: String,
}

#[derive(Debug, Serialize)]
pub struct OidcExchangeResponse {
    pub session_token: String,
    pub expires_in: u64,
}

pub async fn oidc_exchange_handler(
    State(state): State<AppState>,
    Json(payload): Json<OidcExchangeRequest>,
) -> Result<Json<OidcExchangeResponse>, StatusCode> {
    tracing::debug!("oidc_exchange_handler: received id_token={}", payload.id_token);
    let Some(identity) = crate::auth::oidc::verify_id_token(&payload.id_token).await else {
        tracing::warn!(
            "oidc_exchange_handler: verify_id_token failed for token={}",
            payload.id_token
        );
        return Err(StatusCode::UNAUTHORIZED);
    };

    let session_token = state.broker.create_session(identity.subject).await;
    Ok(Json(OidcExchangeResponse {
        session_token,
        expires_in: state.broker.session_ttl_seconds() as u64,
    }))
}

#[derive(Debug, Deserialize)]
pub struct WsTicketRequest {
    pub session_token: String,
}

#[derive(Debug, Serialize)]
pub struct WsTicketResponse {
    pub access_token: String,
    pub expires_in: u64,
}

pub async fn issue_ws_ticket_handler(
    State(state): State<AppState>,
    Json(payload): Json<WsTicketRequest>,
) -> Result<Json<WsTicketResponse>, StatusCode> {
    let Some((access_token, expires_in)) =
        state.broker.issue_ws_ticket(&payload.session_token).await
    else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    Ok(Json(WsTicketResponse {
        access_token,
        expires_in,
    }))
}

#[derive(Debug, Deserialize)]
pub struct LogoutRequest {
    pub session_token: String,
}

#[derive(Debug, Serialize)]
pub struct LogoutResponse {
    pub ok: bool,
}

pub async fn logout_handler(
    State(state): State<AppState>,
    Json(payload): Json<LogoutRequest>,
) -> Json<LogoutResponse> {
    state.broker.revoke_session(&payload.session_token).await;
    Json(LogoutResponse { ok: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn revoked_session_cannot_issue_ws_ticket() {
        let broker = BrokerService::default();
        let session = broker.create_session("user-1".to_string()).await;
        broker.revoke_session(&session).await;

        let ws = broker.issue_ws_ticket(&session).await;
        assert!(ws.is_none());
    }

    #[tokio::test]
    async fn valid_ws_ticket_verifies() {
        let broker = BrokerService::default();
        let session = broker.create_session("user-2".to_string()).await;
        let (ticket, _) = broker
            .issue_ws_ticket(&session)
            .await
            .expect("ticket must exist");

        let claims = broker.verify_ws_ticket(&ticket).await;
        assert!(claims.is_some());
    }
}
