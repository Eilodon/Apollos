use apollos_proto::contracts::{
    AssistantTextMessage, BackendToClientMessage, ClientToBackendMessage, ConnectionState,
    ConnectionStateMessage, DistanceCategory, HardStopMessage, NavigationMode, SafetyStateMessage,
};
use chrono::Utc;
use tracing::warn;

use crate::{safety_policy, AppState};

#[derive(Debug, Clone, Copy)]
pub enum Channel {
    Live,
    Emergency,
}

#[derive(Debug, Clone)]
pub struct AgentOrchestrator {
    channel: Channel,
}

impl AgentOrchestrator {
    pub(crate) fn new(channel: crate::ws_handler::Channel) -> Self {
        let channel = match channel {
            crate::ws_handler::Channel::Live => Channel::Live,
            crate::ws_handler::Channel::Emergency => Channel::Emergency,
        };
        Self { channel }
    }

    pub async fn route_message(
        &self,
        state: &AppState,
        message: ClientToBackendMessage,
    ) -> Option<BackendToClientMessage> {
        match message {
            ClientToBackendMessage::MultimodalFrame(frame) => {
                state
                    .sessions
                    .touch_session(
                        &frame.session_id,
                        Some(frame.motion_state),
                        frame.lat,
                        frame.lng,
                        frame.heading_deg,
                        true,
                    )
                    .await;

                let sensor_health_score = frame.sensor_health.as_ref().map(|s| s.score);
                let sensor_health_flags = frame.sensor_health.as_ref().map(|s| s.flags.clone());

                state
                    .sessions
                    .update_observability(
                        &frame.session_id,
                        sensor_health_score,
                        sensor_health_flags,
                        frame.location_accuracy_m,
                        None,
                        None,
                    )
                    .await;

                if let Err(error) = state.gemini.forward_multimodal_frame(state, &frame).await {
                    warn!(
                        session_id = %frame.session_id,
                        error = %error,
                        "failed to forward multimodal frame to gemini live"
                    );
                    return Some(BackendToClientMessage::ConnectionState(
                        ConnectionStateMessage {
                            state: ConnectionState::Degraded,
                            detail: Some("gemini_live_unavailable".to_string()),
                        },
                    ));
                }

                Some(BackendToClientMessage::ConnectionState(
                    ConnectionStateMessage {
                        state: ConnectionState::Connected,
                        detail: Some("frame_ingested".to_string()),
                    },
                ))
            }
            ClientToBackendMessage::AudioChunk(chunk) => {
                let allowed = state
                    .sessions
                    .should_allow_utterance(
                        &chunk.session_id,
                        Utc::now().timestamp_millis() as f64 / 1000.0,
                        0.15,
                        18,
                        12.0,
                    )
                    .await;

                if !allowed {
                    return Some(BackendToClientMessage::ConnectionState(
                        ConnectionStateMessage {
                            state: ConnectionState::Connected,
                            detail: Some("audio_rate_limited".to_string()),
                        },
                    ));
                }

                let detail = if let Err(error) = state.gemini.forward_audio_chunk(state, &chunk).await {
                    warn!(
                        session_id = %chunk.session_id,
                        error = %error,
                        "failed to forward audio chunk to gemini live"
                    );
                    "gemini_live_unavailable"
                } else {
                    "audio_ingested"
                };

                Some(BackendToClientMessage::ConnectionState(
                    ConnectionStateMessage {
                        state: ConnectionState::Connected,
                        detail: Some(detail.to_string()),
                    },
                ))
            }
            ClientToBackendMessage::UserCommand(cmd) => {
                if cmd.command.eq_ignore_ascii_case("help") {
                    let help = state
                        .fallback
                        .create_help_session(&cmd.session_id, "manual")
                        .await;

                    return Some(BackendToClientMessage::HumanHelpSession(help));
                }

                if let Some(mode) = extract_mode_from_command(&cmd.command) {
                    state.sessions.set_mode(&cmd.session_id, mode).await;
                    return Some(BackendToClientMessage::AssistantText(
                        AssistantTextMessage {
                            session_id: cmd.session_id,
                            timestamp: Utc::now().to_rfc3339(),
                            text: format!("Đã chuyển chế độ {:?}", mode),
                        },
                    ));
                }

                if state.gemini.live_enabled() {
                    match state.gemini.forward_user_command(state, &cmd).await {
                        Ok(()) => {
                            return Some(BackendToClientMessage::ConnectionState(
                                ConnectionStateMessage {
                                    state: ConnectionState::Connected,
                                    detail: Some("gemini_live_command_queued".to_string()),
                                },
                            ));
                        }
                        Err(error) => {
                            warn!(
                                session_id = %cmd.session_id,
                                error = %error,
                                "failed to forward user command to gemini live; falling back to REST"
                            );
                        }
                    }
                }

                let generated = state.gemini.infer_text(&cmd.command).await;
                let text = match generated {
                    Ok(text) => text,
                    Err(error) => format!("Xin lỗi, không thể xử lý lệnh lúc này: {error}"),
                };

                Some(BackendToClientMessage::AssistantText(
                    AssistantTextMessage {
                        session_id: cmd.session_id,
                        timestamp: Utc::now().to_rfc3339(),
                        text: format!("[{:?}] {}", self.channel, text),
                    },
                ))
            }
            ClientToBackendMessage::EdgeHazard(hazard) => {
                state
                    .sessions
                    .mark_edge_hazard(
                        &hazard.session_id,
                        hazard.hazard_type.clone(),
                        hazard.suppress_seconds,
                    )
                    .await;

                let observability = state.sessions.get_observability(&hazard.session_id).await;

                let decision =
                    safety_policy::evaluate_safety_policy(safety_policy::SafetyPolicyInput {
                        hazard_confidence: hazard.confidence.unwrap_or(0.7),
                        distance_category: hazard.distance.unwrap_or(DistanceCategory::VeryClose),
                        motion_state: observability.motion_state,
                        sensor_health_score: observability.sensor_health_score,
                        localization_uncertainty_m: observability.localization_uncertainty_m,
                        edge_reflex_active: observability.edge_reflex_active,
                    });

                state
                    .sessions
                    .update_observability(
                        &hazard.session_id,
                        None,
                        None,
                        None,
                        Some(decision.tier),
                        Some(decision.reason.clone()),
                    )
                    .await;

                if decision.should_emit_hard_stop() {
                    let hard_stop = BackendToClientMessage::HardStop(HardStopMessage {
                        position_x: hazard.position_x.unwrap_or(0.0),
                        distance: hazard.distance.unwrap_or(DistanceCategory::VeryClose),
                        hazard_type: hazard.hazard_type.clone(),
                        confidence: hazard.confidence.unwrap_or(0.7),
                        ts: Some(Utc::now().to_rfc3339()),
                    });

                    state
                        .ws_registry
                        .emit_hard_stop(&hazard.session_id, hard_stop)
                        .await;
                }

                if decision.should_escalate_human() {
                    let help = state
                        .fallback
                        .create_help_session(&hazard.session_id, "safety_policy")
                        .await;
                    let _ = state
                        .ws_registry
                        .send_live(
                            &hazard.session_id,
                            BackendToClientMessage::HumanHelpSession(help),
                        )
                        .await;
                }

                Some(BackendToClientMessage::SafetyState(SafetyStateMessage {
                    session_id: hazard.session_id,
                    timestamp: Utc::now().to_rfc3339(),
                    degraded: observability.degraded_mode,
                    reason: Some(decision.reason),
                    sensor_health_score: observability.sensor_health_score,
                    sensor_health_flags: Some(observability.sensor_health_flags),
                    localization_uncertainty_m: observability.localization_uncertainty_m,
                    tier: decision.tier,
                }))
            }
        }
    }
}

fn extract_mode_from_command(command: &str) -> Option<NavigationMode> {
    let normalized = command.trim().to_ascii_uppercase();

    if normalized.contains("NAVIGATION") {
        Some(NavigationMode::Navigation)
    } else if normalized.contains("EXPLORE") {
        Some(NavigationMode::Explore)
    } else if normalized.contains("READ") {
        Some(NavigationMode::Read)
    } else if normalized.contains("QUIET") {
        Some(NavigationMode::Quiet)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use apollos_proto::contracts::{
        BackendToClientMessage, ClientToBackendMessage, DistanceCategory, EdgeHazardMessage,
    };

    use super::*;

    #[tokio::test]
    async fn edge_hazard_triggers_hard_stop_delivery() {
        let state = AppState::default();
        let (emergency_tx, mut emergency_rx) = tokio::sync::mpsc::channel(8);

        state
            .ws_registry
            .register_emergency("s1", emergency_tx, Some("client-1".to_string()))
            .await
            .expect("register emergency channel");

        let orchestrator = AgentOrchestrator {
            channel: Channel::Live,
        };
        let _ = orchestrator
            .route_message(
                &state,
                ClientToBackendMessage::EdgeHazard(EdgeHazardMessage {
                    session_id: "s1".to_string(),
                    timestamp: Utc::now().to_rfc3339(),
                    hazard_type: "EDGE_DROP_HAZARD".to_string(),
                    position_x: Some(0.2),
                    distance: Some(DistanceCategory::VeryClose),
                    confidence: Some(0.92),
                    suppress_seconds: Some(3),
                }),
            )
            .await;

        let delivered = emergency_rx.recv().await;
        assert!(matches!(
            delivered,
            Some(BackendToClientMessage::HardStop(_))
        ));
    }
}
