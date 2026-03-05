use base64::Engine;
use prost::Message;
use thiserror::Error;

use crate::{
    contracts,
    pb::{messages_v1, types_v1},
};

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("protobuf decode error: {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("missing payload in {0} envelope")]
    MissingPayload(&'static str),
    #[error("unknown enum value for {field}: {value}")]
    UnknownEnum { field: &'static str, value: i32 },
    #[error("invalid base64 payload for {field}")]
    InvalidBase64 { field: &'static str },
}

pub fn encode_client_message(
    message: &contracts::ClientToBackendMessage,
) -> Result<Vec<u8>, TransportError> {
    let payload = match message {
        contracts::ClientToBackendMessage::MultimodalFrame(value) => {
            messages_v1::client_envelope::Payload::MultimodalFrame(multimodal_to_proto(value)?)
        }
        contracts::ClientToBackendMessage::AudioChunk(value) => {
            messages_v1::client_envelope::Payload::AudioChunk(audio_chunk_to_proto(value))
        }
        contracts::ClientToBackendMessage::UserCommand(value) => {
            messages_v1::client_envelope::Payload::UserCommand(user_command_to_proto(value))
        }
        contracts::ClientToBackendMessage::EdgeHazard(value) => {
            messages_v1::client_envelope::Payload::EdgeHazard(edge_hazard_to_proto(value))
        }
    };

    let envelope = messages_v1::ClientEnvelope {
        payload: Some(payload),
    };
    Ok(envelope.encode_to_vec())
}

pub fn decode_client_message(
    bytes: &[u8],
) -> Result<contracts::ClientToBackendMessage, TransportError> {
    let envelope = messages_v1::ClientEnvelope::decode(bytes)?;
    let payload = envelope
        .payload
        .ok_or(TransportError::MissingPayload("client"))?;

    let message = match payload {
        messages_v1::client_envelope::Payload::MultimodalFrame(value) => {
            contracts::ClientToBackendMessage::MultimodalFrame(multimodal_from_proto(value)?)
        }
        messages_v1::client_envelope::Payload::AudioChunk(value) => {
            contracts::ClientToBackendMessage::AudioChunk(audio_chunk_from_proto(value))
        }
        messages_v1::client_envelope::Payload::UserCommand(value) => {
            contracts::ClientToBackendMessage::UserCommand(user_command_from_proto(value))
        }
        messages_v1::client_envelope::Payload::EdgeHazard(value) => {
            contracts::ClientToBackendMessage::EdgeHazard(edge_hazard_from_proto(value)?)
        }
    };

    Ok(message)
}

pub fn encode_server_message(
    message: &contracts::BackendToClientMessage,
) -> Result<Vec<u8>, TransportError> {
    let payload = match message {
        contracts::BackendToClientMessage::AssistantText(value) => {
            messages_v1::server_envelope::Payload::AssistantText(assistant_text_to_proto(value))
        }
        contracts::BackendToClientMessage::AssistantAudio(value) => {
            messages_v1::server_envelope::Payload::AssistantAudio(assistant_audio_to_proto(value)?)
        }
        contracts::BackendToClientMessage::HardStop(value) => {
            messages_v1::server_envelope::Payload::HardStop(hard_stop_to_proto(value))
        }
        contracts::BackendToClientMessage::ConnectionState(value) => {
            messages_v1::server_envelope::Payload::ConnectionState(connection_state_to_proto(value))
        }
        contracts::BackendToClientMessage::SemanticCue(value) => {
            messages_v1::server_envelope::Payload::SemanticCue(semantic_cue_to_proto(value))
        }
        contracts::BackendToClientMessage::SafetyState(value) => {
            messages_v1::server_envelope::Payload::SafetyState(safety_state_to_proto(value))
        }
        contracts::BackendToClientMessage::HumanHelpSession(value) => {
            messages_v1::server_envelope::Payload::HumanHelpSession(human_help_to_proto(value))
        }
    };

    let envelope = messages_v1::ServerEnvelope {
        payload: Some(payload),
    };
    Ok(envelope.encode_to_vec())
}

pub fn decode_server_message(
    bytes: &[u8],
) -> Result<contracts::BackendToClientMessage, TransportError> {
    let envelope = messages_v1::ServerEnvelope::decode(bytes)?;
    let payload = envelope
        .payload
        .ok_or(TransportError::MissingPayload("server"))?;

    let message = match payload {
        messages_v1::server_envelope::Payload::AssistantText(value) => {
            contracts::BackendToClientMessage::AssistantText(assistant_text_from_proto(value))
        }
        messages_v1::server_envelope::Payload::AssistantAudio(value) => {
            contracts::BackendToClientMessage::AssistantAudio(assistant_audio_from_proto(value))
        }
        messages_v1::server_envelope::Payload::HardStop(value) => {
            contracts::BackendToClientMessage::HardStop(hard_stop_from_proto(value)?)
        }
        messages_v1::server_envelope::Payload::ConnectionState(value) => {
            contracts::BackendToClientMessage::ConnectionState(connection_state_from_proto(value)?)
        }
        messages_v1::server_envelope::Payload::SemanticCue(value) => {
            contracts::BackendToClientMessage::SemanticCue(semantic_cue_from_proto(value)?)
        }
        messages_v1::server_envelope::Payload::SafetyState(value) => {
            contracts::BackendToClientMessage::SafetyState(safety_state_from_proto(value)?)
        }
        messages_v1::server_envelope::Payload::HumanHelpSession(value) => {
            contracts::BackendToClientMessage::HumanHelpSession(human_help_from_proto(value)?)
        }
    };

    Ok(message)
}

fn multimodal_to_proto(
    value: &contracts::MultimodalFrameMessage,
) -> Result<messages_v1::MultimodalFrameMessage, TransportError> {
    Ok(messages_v1::MultimodalFrameMessage {
        session_id: value.session_id.clone(),
        timestamp: value.timestamp.clone(),
        frame_jpeg: value
            .frame_jpeg_base64
            .as_deref()
            .map(|raw| decode_base64(raw, "frame_jpeg_base64"))
            .transpose()?,
        motion_state: motion_state_to_proto(value.motion_state) as i32,
        pitch: value.pitch,
        velocity: value.velocity,
        user_text: value.user_text.clone(),
        yaw_delta_deg: value.yaw_delta_deg,
        carry_mode: value
            .carry_mode
            .map(|mode| carry_mode_to_proto(mode) as i32),
        sensor_unavailable: value.sensor_unavailable,
        lat: value.lat,
        lng: value.lng,
        heading_deg: value.heading_deg,
        location_accuracy_m: value.location_accuracy_m,
        location_age_ms: value.location_age_ms,
        sensor_health: value.sensor_health.as_ref().map(sensor_health_to_proto),
    })
}

fn multimodal_from_proto(
    value: messages_v1::MultimodalFrameMessage,
) -> Result<contracts::MultimodalFrameMessage, TransportError> {
    Ok(contracts::MultimodalFrameMessage {
        session_id: value.session_id,
        timestamp: value.timestamp,
        frame_jpeg_base64: value
            .frame_jpeg
            .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)),
        motion_state: motion_state_from_proto(value.motion_state)?,
        pitch: value.pitch,
        velocity: value.velocity,
        user_text: value.user_text,
        yaw_delta_deg: value.yaw_delta_deg,
        carry_mode: value.carry_mode.map(carry_mode_from_proto).transpose()?,
        sensor_unavailable: value.sensor_unavailable,
        lat: value.lat,
        lng: value.lng,
        heading_deg: value.heading_deg,
        location_accuracy_m: value.location_accuracy_m,
        location_age_ms: value.location_age_ms,
        sensor_health: value.sensor_health.map(sensor_health_from_proto),
    })
}

fn audio_chunk_to_proto(value: &contracts::AudioChunkMessage) -> messages_v1::AudioChunkMessage {
    messages_v1::AudioChunkMessage {
        session_id: value.session_id.clone(),
        timestamp: value.timestamp.clone(),
        audio_chunk_pcm16: value.audio_chunk_pcm16.clone(),
    }
}

fn audio_chunk_from_proto(value: messages_v1::AudioChunkMessage) -> contracts::AudioChunkMessage {
    contracts::AudioChunkMessage {
        session_id: value.session_id,
        timestamp: value.timestamp,
        audio_chunk_pcm16: value.audio_chunk_pcm16,
    }
}

fn user_command_to_proto(value: &contracts::UserCommandMessage) -> messages_v1::UserCommandMessage {
    messages_v1::UserCommandMessage {
        session_id: value.session_id.clone(),
        timestamp: value.timestamp.clone(),
        command: value.command.clone(),
    }
}

fn user_command_from_proto(
    value: messages_v1::UserCommandMessage,
) -> contracts::UserCommandMessage {
    contracts::UserCommandMessage {
        session_id: value.session_id,
        timestamp: value.timestamp,
        command: value.command,
    }
}

fn edge_hazard_to_proto(value: &contracts::EdgeHazardMessage) -> messages_v1::EdgeHazardMessage {
    messages_v1::EdgeHazardMessage {
        session_id: value.session_id.clone(),
        timestamp: value.timestamp.clone(),
        hazard_type: value.hazard_type.clone(),
        position_x: value.position_x,
        distance: value
            .distance
            .map(|distance| distance_to_proto(distance) as i32),
        confidence: value.confidence,
        suppress_seconds: value.suppress_seconds,
    }
}

fn edge_hazard_from_proto(
    value: messages_v1::EdgeHazardMessage,
) -> Result<contracts::EdgeHazardMessage, TransportError> {
    Ok(contracts::EdgeHazardMessage {
        session_id: value.session_id,
        timestamp: value.timestamp,
        hazard_type: value.hazard_type,
        position_x: value.position_x,
        distance: value.distance.map(distance_from_proto).transpose()?,
        confidence: value.confidence,
        suppress_seconds: value.suppress_seconds,
    })
}

fn assistant_text_to_proto(
    value: &contracts::AssistantTextMessage,
) -> messages_v1::AssistantTextMessage {
    messages_v1::AssistantTextMessage {
        session_id: value.session_id.clone(),
        timestamp: value.timestamp.clone(),
        text: value.text.clone(),
    }
}

fn assistant_text_from_proto(
    value: messages_v1::AssistantTextMessage,
) -> contracts::AssistantTextMessage {
    contracts::AssistantTextMessage {
        session_id: value.session_id,
        timestamp: value.timestamp,
        text: value.text,
    }
}

fn assistant_audio_to_proto(
    value: &contracts::AssistantAudioMessage,
) -> Result<messages_v1::AssistantAudioMessage, TransportError> {
    Ok(messages_v1::AssistantAudioMessage {
        session_id: value.session_id.clone(),
        timestamp: value.timestamp.clone(),
        pcm24: value
            .pcm24
            .as_deref()
            .map(|raw| decode_base64(raw, "pcm24"))
            .transpose()?,
        pcm16: value
            .pcm16
            .as_deref()
            .map(|raw| decode_base64(raw, "pcm16"))
            .transpose()?,
        hazard_position_x: value.hazard_position_x,
    })
}

fn assistant_audio_from_proto(
    value: messages_v1::AssistantAudioMessage,
) -> contracts::AssistantAudioMessage {
    contracts::AssistantAudioMessage {
        session_id: value.session_id,
        timestamp: value.timestamp,
        pcm24: value
            .pcm24
            .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)),
        pcm16: value
            .pcm16
            .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)),
        hazard_position_x: value.hazard_position_x,
    }
}

fn hard_stop_to_proto(value: &contracts::HardStopMessage) -> messages_v1::HardStopMessage {
    messages_v1::HardStopMessage {
        position_x: value.position_x,
        distance: distance_to_proto(value.distance) as i32,
        hazard_type: value.hazard_type.clone(),
        confidence: value.confidence,
        ts: value.ts.clone(),
    }
}

fn hard_stop_from_proto(
    value: messages_v1::HardStopMessage,
) -> Result<contracts::HardStopMessage, TransportError> {
    Ok(contracts::HardStopMessage {
        position_x: value.position_x,
        distance: distance_from_proto(value.distance)?,
        hazard_type: value.hazard_type,
        confidence: value.confidence,
        ts: value.ts,
    })
}

fn connection_state_to_proto(
    value: &contracts::ConnectionStateMessage,
) -> messages_v1::ConnectionStateMessage {
    messages_v1::ConnectionStateMessage {
        state: connection_state_to_proto_enum(value.state) as i32,
        detail: value.detail.clone(),
    }
}

fn connection_state_from_proto(
    value: messages_v1::ConnectionStateMessage,
) -> Result<contracts::ConnectionStateMessage, TransportError> {
    Ok(contracts::ConnectionStateMessage {
        state: connection_state_from_proto_enum(value.state)?,
        detail: value.detail,
    })
}

fn semantic_cue_to_proto(value: &contracts::SemanticCueMessage) -> messages_v1::SemanticCueMessage {
    messages_v1::SemanticCueMessage {
        cue: semantic_cue_to_proto_enum(value.cue) as i32,
        position_x: value.position_x,
    }
}

fn semantic_cue_from_proto(
    value: messages_v1::SemanticCueMessage,
) -> Result<contracts::SemanticCueMessage, TransportError> {
    Ok(contracts::SemanticCueMessage {
        cue: semantic_cue_from_proto_enum(value.cue)?,
        position_x: value.position_x,
    })
}

fn safety_state_to_proto(value: &contracts::SafetyStateMessage) -> messages_v1::SafetyStateMessage {
    messages_v1::SafetyStateMessage {
        session_id: value.session_id.clone(),
        timestamp: value.timestamp.clone(),
        degraded: value.degraded,
        reason: value.reason.clone(),
        sensor_health_score: value.sensor_health_score,
        sensor_health_flags: value.sensor_health_flags.clone().unwrap_or_default(),
        localization_uncertainty_m: value.localization_uncertainty_m,
        tier: safety_tier_to_proto(value.tier) as i32,
    }
}

fn safety_state_from_proto(
    value: messages_v1::SafetyStateMessage,
) -> Result<contracts::SafetyStateMessage, TransportError> {
    Ok(contracts::SafetyStateMessage {
        session_id: value.session_id,
        timestamp: value.timestamp,
        degraded: value.degraded,
        reason: value.reason,
        sensor_health_score: value.sensor_health_score,
        sensor_health_flags: if value.sensor_health_flags.is_empty() {
            None
        } else {
            Some(value.sensor_health_flags)
        },
        localization_uncertainty_m: value.localization_uncertainty_m,
        tier: safety_tier_from_proto(value.tier)?,
    })
}

fn human_help_to_proto(
    value: &contracts::HumanHelpSessionMessage,
) -> messages_v1::HumanHelpSessionMessage {
    messages_v1::HumanHelpSessionMessage {
        session_id: value.session_id.clone(),
        timestamp: value.timestamp.clone(),
        help_link: value.help_link.clone(),
        rtc: Some(human_help_rtc_to_proto(&value.rtc)),
    }
}

fn human_help_from_proto(
    value: messages_v1::HumanHelpSessionMessage,
) -> Result<contracts::HumanHelpSessionMessage, TransportError> {
    let rtc = value
        .rtc
        .ok_or(TransportError::MissingPayload("human_help_rtc"))?;

    Ok(contracts::HumanHelpSessionMessage {
        session_id: value.session_id,
        timestamp: value.timestamp,
        help_link: value.help_link,
        rtc: human_help_rtc_from_proto(rtc)?,
    })
}

fn sensor_health_to_proto(
    value: &contracts::SensorHealthSnapshot,
) -> types_v1::SensorHealthSnapshot {
    types_v1::SensorHealthSnapshot {
        score: value.score,
        flags: value.flags.clone(),
        degraded: value.degraded,
        source: value.source.clone(),
    }
}

fn sensor_health_from_proto(
    value: types_v1::SensorHealthSnapshot,
) -> contracts::SensorHealthSnapshot {
    contracts::SensorHealthSnapshot {
        score: value.score,
        flags: value.flags,
        degraded: value.degraded,
        source: value.source,
    }
}

fn human_help_rtc_to_proto(
    value: &contracts::HumanHelpRtcSession,
) -> types_v1::HumanHelpRtcSession {
    types_v1::HumanHelpRtcSession {
        provider: human_provider_to_proto(value.provider) as i32,
        room_name: value.room_name.clone(),
        identity: value.identity.clone(),
        token: value.token.clone(),
        expires_in: value.expires_in,
    }
}

fn human_help_rtc_from_proto(
    value: types_v1::HumanHelpRtcSession,
) -> Result<contracts::HumanHelpRtcSession, TransportError> {
    Ok(contracts::HumanHelpRtcSession {
        provider: human_provider_from_proto(value.provider)?,
        room_name: value.room_name,
        identity: value.identity,
        token: value.token,
        expires_in: value.expires_in,
    })
}

fn decode_base64(raw: &str, field: &'static str) -> Result<Vec<u8>, TransportError> {
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(raw) {
        return Ok(bytes);
    }
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD_NO_PAD.decode(raw) {
        return Ok(bytes);
    }
    if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE.decode(raw) {
        return Ok(bytes);
    }
    if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(raw) {
        return Ok(bytes);
    }

    Err(TransportError::InvalidBase64 { field })
}

fn motion_state_to_proto(value: contracts::MotionState) -> types_v1::MotionState {
    match value {
        contracts::MotionState::Stationary => types_v1::MotionState::Stationary,
        contracts::MotionState::WalkingSlow => types_v1::MotionState::WalkingSlow,
        contracts::MotionState::WalkingFast => types_v1::MotionState::WalkingFast,
        contracts::MotionState::Running => types_v1::MotionState::Running,
    }
}

fn motion_state_from_proto(value: i32) -> Result<contracts::MotionState, TransportError> {
    let value =
        types_v1::MotionState::try_from(value).map_err(|_| TransportError::UnknownEnum {
            field: "MotionState",
            value,
        })?;

    Ok(match value {
        types_v1::MotionState::Stationary => contracts::MotionState::Stationary,
        types_v1::MotionState::WalkingSlow => contracts::MotionState::WalkingSlow,
        types_v1::MotionState::WalkingFast => contracts::MotionState::WalkingFast,
        types_v1::MotionState::Running => contracts::MotionState::Running,
        types_v1::MotionState::Unspecified => contracts::MotionState::Stationary,
    })
}

fn distance_to_proto(value: contracts::DistanceCategory) -> types_v1::DistanceCategory {
    match value {
        contracts::DistanceCategory::VeryClose => types_v1::DistanceCategory::VeryClose,
        contracts::DistanceCategory::Mid => types_v1::DistanceCategory::Mid,
        contracts::DistanceCategory::Far => types_v1::DistanceCategory::Far,
    }
}

fn distance_from_proto(value: i32) -> Result<contracts::DistanceCategory, TransportError> {
    let value =
        types_v1::DistanceCategory::try_from(value).map_err(|_| TransportError::UnknownEnum {
            field: "DistanceCategory",
            value,
        })?;

    Ok(match value {
        types_v1::DistanceCategory::VeryClose => contracts::DistanceCategory::VeryClose,
        types_v1::DistanceCategory::Mid => contracts::DistanceCategory::Mid,
        types_v1::DistanceCategory::Far => contracts::DistanceCategory::Far,
        types_v1::DistanceCategory::Unspecified => contracts::DistanceCategory::Mid,
    })
}

fn carry_mode_to_proto(value: contracts::CarryMode) -> types_v1::CarryMode {
    match value {
        contracts::CarryMode::HandHeld => types_v1::CarryMode::HandHeld,
        contracts::CarryMode::Necklace => types_v1::CarryMode::Necklace,
        contracts::CarryMode::ChestClip => types_v1::CarryMode::ChestClip,
        contracts::CarryMode::Pocket => types_v1::CarryMode::Pocket,
    }
}

fn carry_mode_from_proto(value: i32) -> Result<contracts::CarryMode, TransportError> {
    let value = types_v1::CarryMode::try_from(value).map_err(|_| TransportError::UnknownEnum {
        field: "CarryMode",
        value,
    })?;

    Ok(match value {
        types_v1::CarryMode::HandHeld => contracts::CarryMode::HandHeld,
        types_v1::CarryMode::Necklace => contracts::CarryMode::Necklace,
        types_v1::CarryMode::ChestClip => contracts::CarryMode::ChestClip,
        types_v1::CarryMode::Pocket => contracts::CarryMode::Pocket,
        types_v1::CarryMode::Unspecified => contracts::CarryMode::Necklace,
    })
}

fn safety_tier_to_proto(value: contracts::SafetyTier) -> types_v1::SafetyTier {
    match value {
        contracts::SafetyTier::Silent => types_v1::SafetyTier::Silent,
        contracts::SafetyTier::Ping => types_v1::SafetyTier::Ping,
        contracts::SafetyTier::Voice => types_v1::SafetyTier::Voice,
        contracts::SafetyTier::HardStop => types_v1::SafetyTier::HardStop,
        contracts::SafetyTier::HumanEscalation => types_v1::SafetyTier::HumanEscalation,
    }
}

fn safety_tier_from_proto(value: i32) -> Result<contracts::SafetyTier, TransportError> {
    let value = types_v1::SafetyTier::try_from(value).map_err(|_| TransportError::UnknownEnum {
        field: "SafetyTier",
        value,
    })?;

    Ok(match value {
        types_v1::SafetyTier::Silent => contracts::SafetyTier::Silent,
        types_v1::SafetyTier::Ping => contracts::SafetyTier::Ping,
        types_v1::SafetyTier::Voice => contracts::SafetyTier::Voice,
        types_v1::SafetyTier::HardStop => contracts::SafetyTier::HardStop,
        types_v1::SafetyTier::HumanEscalation => contracts::SafetyTier::HumanEscalation,
        types_v1::SafetyTier::Unspecified => contracts::SafetyTier::Silent,
    })
}

fn connection_state_to_proto_enum(
    value: contracts::ConnectionState,
) -> messages_v1::ConnectionStateKind {
    match value {
        contracts::ConnectionState::Connected => messages_v1::ConnectionStateKind::Connected,
        contracts::ConnectionState::Reconnecting => messages_v1::ConnectionStateKind::Reconnecting,
        contracts::ConnectionState::Disconnected => messages_v1::ConnectionStateKind::Disconnected,
        contracts::ConnectionState::Degraded => messages_v1::ConnectionStateKind::Degraded,
    }
}

fn connection_state_from_proto_enum(
    value: i32,
) -> Result<contracts::ConnectionState, TransportError> {
    let value = messages_v1::ConnectionStateKind::try_from(value).map_err(|_| {
        TransportError::UnknownEnum {
            field: "ConnectionStateKind",
            value,
        }
    })?;

    Ok(match value {
        messages_v1::ConnectionStateKind::Connected => contracts::ConnectionState::Connected,
        messages_v1::ConnectionStateKind::Reconnecting => contracts::ConnectionState::Reconnecting,
        messages_v1::ConnectionStateKind::Disconnected => contracts::ConnectionState::Disconnected,
        messages_v1::ConnectionStateKind::Degraded => contracts::ConnectionState::Degraded,
        messages_v1::ConnectionStateKind::Unspecified => contracts::ConnectionState::Disconnected,
    })
}

fn semantic_cue_to_proto_enum(value: contracts::SemanticCue) -> messages_v1::SemanticCueKind {
    match value {
        contracts::SemanticCue::ApproachingObject => {
            messages_v1::SemanticCueKind::ApproachingObject
        }
        contracts::SemanticCue::SoftObstacle => messages_v1::SemanticCueKind::SoftObstacle,
        contracts::SemanticCue::TurningRecommended => {
            messages_v1::SemanticCueKind::TurningRecommended
        }
        contracts::SemanticCue::DestinationNear => messages_v1::SemanticCueKind::DestinationNear,
        contracts::SemanticCue::PocketModeActive => messages_v1::SemanticCueKind::PocketModeActive,
    }
}

fn semantic_cue_from_proto_enum(value: i32) -> Result<contracts::SemanticCue, TransportError> {
    let value =
        messages_v1::SemanticCueKind::try_from(value).map_err(|_| TransportError::UnknownEnum {
            field: "SemanticCueKind",
            value,
        })?;

    Ok(match value {
        messages_v1::SemanticCueKind::ApproachingObject => {
            contracts::SemanticCue::ApproachingObject
        }
        messages_v1::SemanticCueKind::SoftObstacle => contracts::SemanticCue::SoftObstacle,
        messages_v1::SemanticCueKind::TurningRecommended => {
            contracts::SemanticCue::TurningRecommended
        }
        messages_v1::SemanticCueKind::DestinationNear => contracts::SemanticCue::DestinationNear,
        messages_v1::SemanticCueKind::PocketModeActive => contracts::SemanticCue::PocketModeActive,
        messages_v1::SemanticCueKind::Unspecified => contracts::SemanticCue::ApproachingObject,
    })
}

fn human_provider_to_proto(value: contracts::HumanHelpProvider) -> types_v1::HumanRtcProvider {
    match value {
        contracts::HumanHelpProvider::Twilio => types_v1::HumanRtcProvider::Twilio,
        contracts::HumanHelpProvider::Livekit => types_v1::HumanRtcProvider::Livekit,
    }
}

fn human_provider_from_proto(value: i32) -> Result<contracts::HumanHelpProvider, TransportError> {
    let value =
        types_v1::HumanRtcProvider::try_from(value).map_err(|_| TransportError::UnknownEnum {
            field: "HumanRtcProvider",
            value,
        })?;

    Ok(match value {
        types_v1::HumanRtcProvider::Twilio => contracts::HumanHelpProvider::Twilio,
        types_v1::HumanRtcProvider::Livekit => contracts::HumanHelpProvider::Livekit,
        types_v1::HumanRtcProvider::Unspecified => contracts::HumanHelpProvider::Twilio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_client_envelope_binary() {
        let message =
            contracts::ClientToBackendMessage::UserCommand(contracts::UserCommandMessage {
                session_id: "s1".to_string(),
                timestamp: "2026-03-05T10:00:00Z".to_string(),
                command: "help".to_string(),
            });

        let encoded = encode_client_message(&message).expect("encode should pass");
        let decoded = decode_client_message(&encoded).expect("decode should pass");
        assert_eq!(decoded, message);
    }

    #[test]
    fn roundtrip_server_envelope_binary() {
        let message =
            contracts::BackendToClientMessage::ConnectionState(contracts::ConnectionStateMessage {
                state: contracts::ConnectionState::Connected,
                detail: Some("ok".to_string()),
            });

        let encoded = encode_server_message(&message).expect("encode should pass");
        let decoded = decode_server_message(&encoded).expect("decode should pass");
        assert_eq!(decoded, message);
    }
}
