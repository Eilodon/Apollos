use std::{collections::HashMap, sync::Arc};

use crate::AppState;
use apollos_proto::contracts::{HumanHelpProvider, HumanHelpRtcSession, HumanHelpSessionMessage};
use axum::{extract::State, http::StatusCode, Json};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct HelpTicketRecord {
    session_id: String,
    reason: String,
    expires_at: DateTime<Utc>,
    used: bool,
}

#[derive(Debug, Clone)]
struct ViewerTokenRecord {
    session_id: String,
    viewer_id: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
struct FallbackState {
    tickets: HashMap<String, HelpTicketRecord>,
    viewers: HashMap<String, ViewerTokenRecord>,
}

#[derive(Debug, Clone)]
struct TwilioConfig {
    account_sid: String,
    api_key_sid: String,
    api_key_secret: String,
    room_prefix: String,
    token_ttl_seconds: i64,
}

#[derive(Debug, Clone)]
pub struct HumanFallbackService {
    pub public_help_base: String,
    ticket_ttl_seconds: i64,
    viewer_ttl_seconds: i64,
    twilio: Option<TwilioConfig>,
    state: Arc<RwLock<FallbackState>>,
}

impl TwilioConfig {
    fn from_env(default_ttl_seconds: i64) -> Option<Self> {
        let account_sid = std::env::var("TWILIO_ACCOUNT_SID")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let api_key_sid = std::env::var("TWILIO_VIDEO_API_KEY_SID")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let api_key_secret = std::env::var("TWILIO_VIDEO_API_KEY_SECRET")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let room_prefix = std::env::var("TWILIO_VIDEO_ROOM_PREFIX")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "apollos-help".to_string());

        let token_ttl_seconds = std::env::var("TWILIO_VIDEO_TOKEN_TTL_SECONDS")
            .ok()
            .and_then(|raw| raw.parse::<i64>().ok())
            .unwrap_or(default_ttl_seconds)
            .max(30);

        let account_sid = account_sid?;
        let Some(api_key_sid) = api_key_sid else {
            warn!("TWILIO_ACCOUNT_SID is set but TWILIO_VIDEO_API_KEY_SID missing");
            return None;
        };
        let Some(api_key_secret) = api_key_secret else {
            warn!("TWILIO_ACCOUNT_SID is set but TWILIO_VIDEO_API_KEY_SECRET missing");
            return None;
        };

        Some(Self {
            account_sid,
            api_key_sid,
            api_key_secret,
            room_prefix,
            token_ttl_seconds,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HelpTicketExchangeResult {
    pub session_id: String,
    pub viewer_token: String,
    pub expires_in: u64,
    pub rtc: Option<HumanHelpRtcSession>,
}

#[derive(Debug, Deserialize)]
pub struct HelpTicketExchangeRequest {
    pub help_ticket: String,
}

#[derive(Debug, Serialize)]
pub struct HelpTicketExchangeResponse {
    pub session_id: String,
    pub viewer_token: String,
    pub expires_in: u64,
    pub rtc: Option<HumanHelpRtcSession>,
}

#[derive(Debug, Clone)]
pub struct ViewerClaims {
    pub session_id: String,
    pub viewer_id: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct TwilioVideoGrant {
    room: String,
}

#[derive(Debug, Serialize)]
struct TwilioGrants {
    identity: String,
    video: TwilioVideoGrant,
}

#[derive(Debug, Serialize)]
struct TwilioAccessClaims {
    jti: String,
    iss: String,
    sub: String,
    exp: i64,
    grants: TwilioGrants,
}

impl Default for HumanFallbackService {
    fn default() -> Self {
        let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
        let twilio_required = env_flag(
            "TWILIO_REQUIRED",
            app_env.eq_ignore_ascii_case("production"),
        );

        let viewer_ttl_seconds = std::env::var("HELP_VIEWER_TTL_S")
            .ok()
            .and_then(|raw| raw.parse::<i64>().ok())
            .unwrap_or(300)
            .max(30);
        let twilio = TwilioConfig::from_env(viewer_ttl_seconds);

        assert!(
            !(twilio_required && twilio.is_none()),
            "TWILIO_REQUIRED is enabled but Twilio config is missing"
        );

        Self {
            public_help_base: std::env::var("PUBLIC_HELP_BASE")
                .unwrap_or_else(|_| "https://help.apollos.local/live".to_string()),
            ticket_ttl_seconds: std::env::var("HELP_TICKET_TTL_S")
                .ok()
                .and_then(|raw| raw.parse::<i64>().ok())
                .unwrap_or(300)
                .max(30),
            viewer_ttl_seconds,
            twilio,
            state: Arc::new(RwLock::new(FallbackState::default())),
        }
    }
}

impl HumanFallbackService {
    pub async fn create_help_session(
        &self,
        session_id: &str,
        reason: &str,
    ) -> HumanHelpSessionMessage {
        self.prune().await;

        let help_ticket = mint_token("help", session_id);
        let expires_at = Utc::now() + Duration::seconds(self.ticket_ttl_seconds);
        let room_name = self.room_name(session_id);
        let publisher_identity = format!("patient-{session_id}");
        let publisher_token = self
            .mint_twilio_video_token(&room_name, &publisher_identity)
            .unwrap_or_else(|| "publisher-token-stub".to_string());
        let rtc_expires_in = self.twilio_token_ttl_seconds() as u32;

        let mut guard = self.state.write().await;
        guard.tickets.insert(
            help_ticket.clone(),
            HelpTicketRecord {
                session_id: session_id.to_string(),
                reason: reason.to_string(),
                expires_at,
                used: false,
            },
        );

        HumanHelpSessionMessage {
            session_id: session_id.to_string(),
            timestamp: Utc::now().to_rfc3339(),
            help_link: Some(format!(
                "{}?help_ticket={help_ticket}",
                self.public_help_base
            )),
            rtc: HumanHelpRtcSession {
                provider: HumanHelpProvider::Twilio,
                room_name,
                identity: Some(publisher_identity),
                token: publisher_token,
                expires_in: rtc_expires_in,
            },
        }
    }

    pub async fn exchange_help_ticket(&self, ticket: &str) -> Option<HelpTicketExchangeResult> {
        self.prune().await;

        let (session_id, _reason) = {
            let mut guard = self.state.write().await;
            let ticket_record = guard.tickets.get_mut(ticket)?;
            if ticket_record.used || Utc::now() > ticket_record.expires_at {
                return None;
            }

            ticket_record.used = true;
            (
                ticket_record.session_id.clone(),
                ticket_record.reason.clone(),
            )
        };

        let viewer_token = mint_token("viewer", &session_id);
        let viewer_id = format!("viewer-{}", Uuid::new_v4().simple());
        let expires_at = Utc::now() + Duration::seconds(self.viewer_ttl_seconds);
        let room_name = self.room_name(&session_id);
        let helper_identity = format!("helper-{}", Uuid::new_v4().simple());
        let viewer_rtc_token = self
            .mint_twilio_video_token(&room_name, &helper_identity)
            .unwrap_or_else(|| "viewer-token-stub".to_string());
        let rtc_expires_in = self.twilio_token_ttl_seconds() as u32;

        let mut guard = self.state.write().await;
        guard.viewers.insert(
            viewer_token.clone(),
            ViewerTokenRecord {
                session_id: session_id.clone(),
                viewer_id,
                expires_at,
            },
        );

        Some(HelpTicketExchangeResult {
            session_id: session_id.clone(),
            viewer_token,
            expires_in: self.viewer_ttl_seconds as u64,
            rtc: Some(HumanHelpRtcSession {
                provider: HumanHelpProvider::Twilio,
                room_name,
                identity: Some(helper_identity),
                token: viewer_rtc_token,
                expires_in: rtc_expires_in,
            }),
        })
    }

    pub async fn verify_viewer_token(
        &self,
        token: &str,
        expected_session_id: &str,
    ) -> Option<ViewerClaims> {
        self.prune().await;
        let guard = self.state.read().await;
        let record = guard.viewers.get(token)?;

        if record.session_id != expected_session_id || Utc::now() > record.expires_at {
            return None;
        }

        Some(ViewerClaims {
            session_id: record.session_id.clone(),
            viewer_id: record.viewer_id.clone(),
            expires_at: record.expires_at,
        })
    }

    fn room_name(&self, session_id: &str) -> String {
        let prefix = self
            .twilio
            .as_ref()
            .map(|cfg| cfg.room_prefix.as_str())
            .unwrap_or("apollos-help");
        format!("{prefix}-{session_id}")
    }

    fn twilio_token_ttl_seconds(&self) -> i64 {
        self.twilio
            .as_ref()
            .map(|cfg| cfg.token_ttl_seconds)
            .unwrap_or(self.viewer_ttl_seconds)
            .max(30)
    }

    fn mint_twilio_video_token(&self, room_name: &str, identity: &str) -> Option<String> {
        let config = self.twilio.as_ref()?;

        let now = Utc::now().timestamp();
        let exp = now + config.token_ttl_seconds.max(30);
        let claims = TwilioAccessClaims {
            jti: format!("{}-{}", config.api_key_sid, Uuid::new_v4().simple()),
            iss: config.api_key_sid.clone(),
            sub: config.account_sid.clone(),
            exp,
            grants: TwilioGrants {
                identity: identity.to_string(),
                video: TwilioVideoGrant {
                    room: room_name.to_string(),
                },
            },
        };

        let mut header = Header::new(Algorithm::HS256);
        header.typ = Some("JWT".to_string());
        header.cty = Some("twilio-fpa;v=1".to_string());

        jsonwebtoken::encode(
            &header,
            &claims,
            &EncodingKey::from_secret(config.api_key_secret.as_bytes()),
        )
        .ok()
    }

    async fn prune(&self) {
        let mut guard = self.state.write().await;
        let now = Utc::now();

        guard.tickets.retain(|_, record| record.expires_at > now);
        guard.viewers.retain(|_, record| record.expires_at > now);
    }
}

fn mint_token(prefix: &str, value: &str) -> String {
    let raw = format!(
        "{prefix}:{value}:{}:{}",
        Utc::now().timestamp(),
        Uuid::new_v4()
    );
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

fn env_flag(name: &str, default: bool) -> bool {
    let Ok(raw) = std::env::var(name) else {
        return default;
    };

    !matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "off" | "no"
    )
}

pub async fn help_ticket_exchange_handler(
    State(state): State<AppState>,
    Json(payload): Json<HelpTicketExchangeRequest>,
) -> Result<Json<HelpTicketExchangeResponse>, StatusCode> {
    let Some(result) = state
        .fallback
        .exchange_help_ticket(&payload.help_ticket)
        .await
    else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    Ok(Json(HelpTicketExchangeResponse {
        session_id: result.session_id,
        viewer_token: result.viewer_token,
        expires_in: result.expires_in,
        rtc: result.rtc,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn help_ticket_is_one_time_exchange() {
        let service = HumanFallbackService::default();
        let session = service.create_help_session("s1", "manual").await;
        let link = session.help_link.expect("must have link");
        let ticket = link
            .split("help_ticket=")
            .nth(1)
            .expect("must have ticket")
            .to_string();

        let first = service.exchange_help_ticket(&ticket).await;
        let second = service.exchange_help_ticket(&ticket).await;

        assert!(first.is_some());
        assert!(second.is_none());
    }

    #[tokio::test]
    async fn viewer_token_must_match_session() {
        let service = HumanFallbackService::default();
        let session = service.create_help_session("s2", "manual").await;
        let link = session.help_link.expect("must have link");
        let ticket = link
            .split("help_ticket=")
            .nth(1)
            .expect("must have ticket")
            .to_string();

        let exchange = service
            .exchange_help_ticket(&ticket)
            .await
            .expect("exchange works");

        let ok = service
            .verify_viewer_token(&exchange.viewer_token, "s2")
            .await;
        let bad = service
            .verify_viewer_token(&exchange.viewer_token, "other")
            .await;

        assert!(ok.is_some());
        assert!(bad.is_none());
    }
}
