use apollos_proto::contracts::{
    AssistantTextMessage, BackendToClientMessage, ClientToBackendMessage, CognitionLayer,
    ConnectionState, ConnectionStateMessage, NavigationMode, SafetyDirectiveMessage, SemanticCue,
    SemanticCueMessage,
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

    async fn send_channel(
        &self,
        state: &AppState,
        session_id: &str,
        payload: BackendToClientMessage,
    ) -> bool {
        match self.channel {
            Channel::Live => state.ws_registry.send_live(session_id, payload).await,
            Channel::Emergency => {
                if state
                    .ws_registry
                    .send_emergency(session_id, payload.clone())
                    .await
                {
                    true
                } else {
                    state.ws_registry.send_live(session_id, payload).await
                }
            }
        }
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
                let localization_uncertainty_m = frame
                    .sensor_uncertainty
                    .as_ref()
                    .and_then(|snapshot| {
                        localization_uncertainty_from_covariance(&snapshot.covariance_3x3)
                    })
                    .or(frame.location_accuracy_m.map(|value| value.max(0.0)));

                state
                    .sessions
                    .update_observability(
                        &frame.session_id,
                        sensor_health_score,
                        sensor_health_flags,
                        localization_uncertainty_m,
                        None,
                        None,
                        None,
                    )
                    .await;

                let cognition_transition = state
                    .sessions
                    .update_cognition_signals(
                        &frame.session_id,
                        frame.cloud_link.as_ref(),
                        &frame.edge_semantic_cues,
                    )
                    .await;
                if let Some(transition) = cognition_transition {
                    let _ = self
                        .send_channel(
                            state,
                            &frame.session_id,
                            BackendToClientMessage::CognitionState(transition),
                        )
                        .await;
                }

                let observability = state.sessions.get_observability(&frame.session_id).await;
                if observability.active_cognition_layer == CognitionLayer::L2Edge {
                    if let Some(cue) = strongest_edge_cue(&frame.edge_semantic_cues) {
                        let text = format_edge_cue(cue);
                        let _ = self
                            .send_channel(
                                state,
                                &frame.session_id,
                                BackendToClientMessage::AssistantText(AssistantTextMessage {
                                    session_id: frame.session_id.clone(),
                                    timestamp: Utc::now().to_rfc3339(),
                                    text,
                                }),
                            )
                            .await;

                        if let Some(position_x) = cue.position_x {
                            let _ = self
                                .send_channel(
                                    state,
                                    &frame.session_id,
                                    BackendToClientMessage::SemanticCue(SemanticCueMessage {
                                        cue: SemanticCue::ApproachingObject,
                                        position_x: Some(position_x.clamp(-1.0, 1.0)),
                                    }),
                                )
                                .await;
                        }
                    }

                    return Some(BackendToClientMessage::ConnectionState(
                        ConnectionStateMessage {
                            state: ConnectionState::Degraded,
                            detail: Some("edge_cognition_active".to_string()),
                        },
                    ));
                }

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

                let detail =
                    if let Err(error) = state.gemini.forward_audio_chunk(state, &chunk).await {
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
            ClientToBackendMessage::HazardObservation(hazard) => {
                let suppress_seconds = hazard
                    .suppress_ms
                    .map(|ms| (ms.max(500).saturating_add(999)) / 1000);
                let observability = state.sessions.get_observability(&hazard.session_id).await;
                let hazard_confidence = hazard.confidence.unwrap_or(0.7).clamp(0.0, 1.0);
                let distance_m = hazard.distance_m.unwrap_or(2.0).max(0.0);
                let relative_velocity_mps = hazard.relative_velocity_mps.unwrap_or(-0.8);
                let bearing_x = hazard.bearing_x.unwrap_or(0.0).clamp(-1.0, 1.0);
                let reflex_gate =
                    hazard_confidence >= 0.85 && distance_m <= 1.5 && relative_velocity_mps <= -1.0;

                let decision =
                    safety_policy::evaluate_safety_policy(safety_policy::SafetyPolicyInput {
                        hazard_confidence,
                        distance_m,
                        relative_velocity_mps,
                        bearing_x,
                        sensor_health_score: observability.sensor_health_score,
                        localization_uncertainty_m: observability.localization_uncertainty_m,
                        edge_reflex_active: observability.edge_reflex_active || reflex_gate,
                    });

                if decision.should_emit_hard_stop() {
                    state
                        .sessions
                        .mark_edge_hazard(
                            &hazard.session_id,
                            hazard.hazard_type.clone(),
                            suppress_seconds,
                        )
                        .await;
                    let _ = state
                        .gemini
                        .interrupt_live_session(
                            &hazard.session_id,
                            &format!(
                                "hazard={};distance_m={distance_m:.2};score={:.2}",
                                hazard.hazard_type, decision.hazard_score
                            ),
                        )
                        .await;
                }

                state
                    .sessions
                    .log_hazard(
                        &hazard.session_id,
                        &hazard.hazard_type,
                        bearing_x,
                        Some(distance_m),
                        Some(relative_velocity_mps),
                        hazard_confidence,
                        Some(decision.hazard_score),
                        Some(decision.hard_stop),
                        hazard.source.as_deref(),
                        "hazard_observation_ingested",
                    )
                    .await;

                state
                    .sessions
                    .update_observability(
                        &hazard.session_id,
                        None,
                        None,
                        None,
                        Some(decision.hazard_score),
                        Some(decision.hard_stop),
                        Some(decision.reason.clone()),
                    )
                    .await;

                let directive = BackendToClientMessage::SafetyDirective(SafetyDirectiveMessage {
                    session_id: hazard.session_id.clone(),
                    timestamp: Utc::now().to_rfc3339(),
                    hazard_type: Some(hazard.hazard_type.clone()),
                    hazard_score: decision.hazard_score,
                    hard_stop: decision.hard_stop,
                    haptic_intensity: decision.haptic_intensity,
                    spatial_audio_pitch_hz: decision.spatial_audio_pitch_hz,
                    spatial_audio_pan: decision.spatial_audio_pan,
                    needs_human_assistance: decision.human_assistance,
                    reason: Some(decision.reason.clone()),
                });

                if decision.should_emit_hard_stop() {
                    state
                        .ws_registry
                        .emit_hard_stop(&hazard.session_id, directive.clone())
                        .await;
                }

                if decision.should_escalate_human() {
                    let help = state
                        .fallback
                        .create_help_session(&hazard.session_id, "safety_policy")
                        .await;
                    let _ = self
                        .send_channel(
                            state,
                            &hazard.session_id,
                            BackendToClientMessage::HumanHelpSession(help),
                        )
                        .await;
                }

                if decision.should_emit_hard_stop() {
                    None
                } else {
                    Some(directive)
                }
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

fn localization_uncertainty_from_covariance(covariance_3x3: &[f32]) -> Option<f32> {
    if covariance_3x3.len() < 9 {
        return None;
    }

    let trace = covariance_3x3[0] + covariance_3x3[4] + covariance_3x3[8];
    if !trace.is_finite() || trace < 0.0 {
        return None;
    }

    Some(trace.sqrt())
}

fn strongest_edge_cue(
    cues: &[apollos_proto::contracts::EdgeSemanticCueMessage],
) -> Option<&apollos_proto::contracts::EdgeSemanticCueMessage> {
    cues.iter().max_by(|left, right| {
        left.confidence
            .partial_cmp(&right.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn format_edge_cue(cue: &apollos_proto::contracts::EdgeSemanticCueMessage) -> String {
    let mut parts = Vec::new();
    if let Some(text) = cue.text.as_deref() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
    if let Some(distance) = cue.distance_m {
        if distance.is_finite() && distance >= 0.0 {
            parts.push(format!("{distance:.1}m"));
        }
    }
    if let Some(clock) = cue.position_clock.as_deref() {
        let trimmed = clock.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }

    if parts.is_empty() {
        format!("Edge cue: {}", cue.cue_type)
    } else {
        format!("{} ({})", cue.cue_type, parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use apollos_proto::contracts::{
        BackendToClientMessage, ClientToBackendMessage, HazardObservationMessage,
    };

    use super::*;

    #[tokio::test]
    async fn hazard_observation_triggers_hard_stop_delivery() {
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
                ClientToBackendMessage::HazardObservation(HazardObservationMessage {
                    session_id: "s1".to_string(),
                    timestamp: Utc::now().to_rfc3339(),
                    hazard_type: "EDGE_DROP_HAZARD".to_string(),
                    bearing_x: Some(0.2),
                    distance_m: Some(1.0),
                    relative_velocity_mps: Some(-2.0),
                    confidence: Some(0.92),
                    source: Some("test".to_string()),
                    suppress_ms: Some(3000),
                }),
            )
            .await;

        let delivered = emergency_rx.recv().await;
        assert!(matches!(
            delivered,
            Some(BackendToClientMessage::SafetyDirective(_))
        ));
    }

    #[test]
    fn derives_localization_uncertainty_from_covariance_trace() {
        let covariance_3x3 = vec![4.0, 0.0, 0.0, 0.0, 9.0, 0.0, 0.0, 0.0, 16.0];
        let uncertainty = localization_uncertainty_from_covariance(&covariance_3x3);

        assert_eq!(uncertainty, Some((29.0_f32).sqrt()));
    }
}
