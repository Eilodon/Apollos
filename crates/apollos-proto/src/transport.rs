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
        contracts::ClientToBackendMessage::HazardObservation(value) => {
            messages_v1::client_envelope::Payload::HazardObservation(hazard_observation_to_proto(
                value,
            ))
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
        messages_v1::client_envelope::Payload::HazardObservation(value) => {
            contracts::ClientToBackendMessage::HazardObservation(hazard_observation_from_proto(
                value,
            )?)
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
        contracts::BackendToClientMessage::SafetyDirective(value) => {
            messages_v1::server_envelope::Payload::SafetyDirective(safety_directive_to_proto(value))
        }
        contracts::BackendToClientMessage::ConnectionState(value) => {
            messages_v1::server_envelope::Payload::ConnectionState(connection_state_to_proto(value))
        }
        contracts::BackendToClientMessage::SemanticCue(value) => {
            messages_v1::server_envelope::Payload::SemanticCue(semantic_cue_to_proto(value))
        }
        contracts::BackendToClientMessage::HumanHelpSession(value) => {
            messages_v1::server_envelope::Payload::HumanHelpSession(human_help_to_proto(value))
        }
        contracts::BackendToClientMessage::CognitionState(value) => {
            messages_v1::server_envelope::Payload::CognitionState(cognition_state_to_proto(value))
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
        messages_v1::server_envelope::Payload::SafetyDirective(value) => {
            contracts::BackendToClientMessage::SafetyDirective(safety_directive_from_proto(value)?)
        }
        messages_v1::server_envelope::Payload::ConnectionState(value) => {
            contracts::BackendToClientMessage::ConnectionState(connection_state_from_proto(value)?)
        }
        messages_v1::server_envelope::Payload::SemanticCue(value) => {
            contracts::BackendToClientMessage::SemanticCue(semantic_cue_from_proto(value)?)
        }
        messages_v1::server_envelope::Payload::HumanHelpSession(value) => {
            contracts::BackendToClientMessage::HumanHelpSession(human_help_from_proto(value)?)
        }
        messages_v1::server_envelope::Payload::CognitionState(value) => {
            contracts::BackendToClientMessage::CognitionState(cognition_state_from_proto(value)?)
        }
    };

    Ok(message)
}

fn multimodal_to_proto(
    value: &contracts::MultimodalFrameMessage,
) -> Result<messages_v1::MultimodalFrameMessage, TransportError> {
    Ok(messages_v1::MultimodalFrameMessage {
        session_id: value.session_id.clone(),
        timestamp_ms: value.timestamp_ms,
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
        sensor_uncertainty: value
            .sensor_uncertainty
            .as_ref()
            .map(sensor_uncertainty_to_proto),
        vision_odometry: value.vision_odometry.as_ref().map(vision_odometry_to_proto),
        cloud_link: value.cloud_link.as_ref().map(cloud_link_to_proto),
        edge_semantic_cues: value
            .edge_semantic_cues
            .iter()
            .map(edge_semantic_cue_to_proto)
            .collect(),
    })
}

fn multimodal_from_proto(
    value: messages_v1::MultimodalFrameMessage,
) -> Result<contracts::MultimodalFrameMessage, TransportError> {
    Ok(contracts::MultimodalFrameMessage {
        session_id: value.session_id,
        timestamp_ms: value.timestamp_ms,
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
        sensor_uncertainty: value.sensor_uncertainty.map(sensor_uncertainty_from_proto),
        vision_odometry: value.vision_odometry.map(vision_odometry_from_proto),
        cloud_link: value.cloud_link.map(cloud_link_from_proto),
        edge_semantic_cues: value
            .edge_semantic_cues
            .into_iter()
            .map(edge_semantic_cue_from_proto)
            .collect(),
    })
}

fn audio_chunk_to_proto(value: &contracts::AudioChunkMessage) -> messages_v1::AudioChunkMessage {
    messages_v1::AudioChunkMessage {
        session_id: value.session_id.clone(),
        timestamp_ms: value.timestamp_ms,
        audio_chunk_pcm16: value.audio_chunk_pcm16.clone(),
    }
}

fn audio_chunk_from_proto(value: messages_v1::AudioChunkMessage) -> contracts::AudioChunkMessage {
    contracts::AudioChunkMessage {
        session_id: value.session_id,
        timestamp_ms: value.timestamp_ms,
        audio_chunk_pcm16: value.audio_chunk_pcm16,
    }
}

fn user_command_to_proto(value: &contracts::UserCommandMessage) -> messages_v1::UserCommandMessage {
    messages_v1::UserCommandMessage {
        session_id: value.session_id.clone(),
        timestamp_ms: value.timestamp_ms,
        command: value.command.clone(),
    }
}

fn user_command_from_proto(
    value: messages_v1::UserCommandMessage,
) -> contracts::UserCommandMessage {
    contracts::UserCommandMessage {
        session_id: value.session_id,
        timestamp_ms: value.timestamp_ms,
        command: value.command,
    }
}

fn hazard_observation_to_proto(
    value: &contracts::HazardObservationMessage,
) -> messages_v1::HazardObservationMessage {
    messages_v1::HazardObservationMessage {
        session_id: value.session_id.clone(),
        timestamp_ms: value.timestamp_ms,
        hazard_type: hazard_type_to_proto(value.hazard_type) as i32,
        bearing_x: value.bearing_x,
        distance_m: value.distance_m,
        relative_velocity_mps: value.relative_velocity_mps,
        confidence: value.confidence,
        source: value.source.clone(),
        suppress_ms: value.suppress_ms,
    }
}

fn hazard_observation_from_proto(
    value: messages_v1::HazardObservationMessage,
) -> Result<contracts::HazardObservationMessage, TransportError> {
    Ok(contracts::HazardObservationMessage {
        session_id: value.session_id,
        timestamp_ms: value.timestamp_ms,
        hazard_type: hazard_type_from_proto(value.hazard_type)?,
        bearing_x: value.bearing_x,
        distance_m: value.distance_m,
        relative_velocity_mps: value.relative_velocity_mps,
        confidence: value.confidence,
        source: value.source,
        suppress_ms: value.suppress_ms,
    })
}

fn assistant_text_to_proto(
    value: &contracts::AssistantTextMessage,
) -> messages_v1::AssistantTextMessage {
    messages_v1::AssistantTextMessage {
        session_id: value.session_id.clone(),
        timestamp_ms: value.timestamp_ms,
        text: value.text.clone(),
    }
}

fn assistant_text_from_proto(
    value: messages_v1::AssistantTextMessage,
) -> contracts::AssistantTextMessage {
    contracts::AssistantTextMessage {
        session_id: value.session_id,
        timestamp_ms: value.timestamp_ms,
        text: value.text,
    }
}

fn assistant_audio_to_proto(
    value: &contracts::AssistantAudioMessage,
) -> Result<messages_v1::AssistantAudioMessage, TransportError> {
    Ok(messages_v1::AssistantAudioMessage {
        session_id: value.session_id.clone(),
        timestamp_ms: value.timestamp_ms,
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
        timestamp_ms: value.timestamp_ms,
        pcm24: value
            .pcm24
            .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)),
        pcm16: value
            .pcm16
            .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)),
        hazard_position_x: value.hazard_position_x,
    }
}

fn safety_directive_to_proto(
    value: &contracts::SafetyDirectiveMessage,
) -> messages_v1::SafetyDirectiveMessage {
    messages_v1::SafetyDirectiveMessage {
        session_id: value.session_id.clone(),
        timestamp_ms: value.timestamp_ms,
        hazard_type: value.hazard_type.map(|hazard| hazard_type_to_proto(hazard) as i32),
        hazard_score: value.hazard_score,
        hard_stop: value.hard_stop,
        haptic_intensity: value.haptic_intensity,
        spatial_audio_pitch_hz: value.spatial_audio_pitch_hz,
        spatial_audio_pan: value.spatial_audio_pan,
        needs_human_assistance: value.needs_human_assistance,
        reason: value.reason.clone(),
        flush_audio: value.flush_audio,
    }
}

fn safety_directive_from_proto(
    value: messages_v1::SafetyDirectiveMessage,
) -> Result<contracts::SafetyDirectiveMessage, TransportError> {
    Ok(contracts::SafetyDirectiveMessage {
        session_id: value.session_id,
        timestamp_ms: value.timestamp_ms,
        hazard_type: value.hazard_type.map(hazard_type_from_proto).transpose()?,
        hazard_score: value.hazard_score,
        hard_stop: value.hard_stop,
        haptic_intensity: value.haptic_intensity,
        spatial_audio_pitch_hz: value.spatial_audio_pitch_hz,
        spatial_audio_pan: value.spatial_audio_pan,
        needs_human_assistance: value.needs_human_assistance,
        reason: value.reason,
        flush_audio: value.flush_audio,
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

fn cognition_state_to_proto(
    value: &contracts::CognitionStateMessage,
) -> messages_v1::CognitionStateMessage {
    messages_v1::CognitionStateMessage {
        session_id: value.session_id.clone(),
        timestamp_ms: value.timestamp_ms,
        active_layer: cognition_layer_to_proto(value.active_layer) as i32,
        cloud_link_healthy: value.cloud_link_healthy,
        edge_cognition_available: value.edge_cognition_available,
        cloud_rtt_ms: value.cloud_rtt_ms,
        reason: value.reason.clone(),
    }
}

fn cognition_state_from_proto(
    value: messages_v1::CognitionStateMessage,
) -> Result<contracts::CognitionStateMessage, TransportError> {
    Ok(contracts::CognitionStateMessage {
        session_id: value.session_id,
        timestamp_ms: value.timestamp_ms,
        active_layer: cognition_layer_from_proto(value.active_layer)?,
        cloud_link_healthy: value.cloud_link_healthy,
        edge_cognition_available: value.edge_cognition_available,
        cloud_rtt_ms: value.cloud_rtt_ms,
        reason: value.reason,
    })
}

fn human_help_to_proto(
    value: &contracts::HumanHelpSessionMessage,
) -> messages_v1::HumanHelpSessionMessage {
    messages_v1::HumanHelpSessionMessage {
        session_id: value.session_id.clone(),
        timestamp_ms: value.timestamp_ms,
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
        timestamp_ms: value.timestamp_ms,
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

fn sensor_uncertainty_to_proto(
    value: &contracts::SensorUncertaintySnapshot,
) -> types_v1::SensorUncertaintySnapshot {
    types_v1::SensorUncertaintySnapshot {
        covariance_3x3: value.covariance_3x3.clone(),
        innovation_norm: value.innovation_norm,
        source: value.source.clone(),
    }
}

fn sensor_uncertainty_from_proto(
    value: types_v1::SensorUncertaintySnapshot,
) -> contracts::SensorUncertaintySnapshot {
    contracts::SensorUncertaintySnapshot {
        covariance_3x3: value.covariance_3x3,
        innovation_norm: value.innovation_norm,
        source: value.source,
    }
}

fn cloud_link_to_proto(value: &contracts::CloudLinkSnapshot) -> messages_v1::CloudLinkSnapshot {
    messages_v1::CloudLinkSnapshot {
        connected: value.connected,
        rtt_ms: value.rtt_ms,
        source: value.source.clone(),
    }
}

fn cloud_link_from_proto(value: messages_v1::CloudLinkSnapshot) -> contracts::CloudLinkSnapshot {
    contracts::CloudLinkSnapshot {
        connected: value.connected,
        rtt_ms: value.rtt_ms,
        source: value.source,
    }
}

fn edge_semantic_cue_to_proto(
    value: &contracts::EdgeSemanticCueMessage,
) -> messages_v1::EdgeSemanticCueMessage {
    messages_v1::EdgeSemanticCueMessage {
        cue_type: value.cue_type.clone(),
        text: value.text.clone(),
        confidence: value.confidence,
        position_x: value.position_x,
        distance_m: value.distance_m,
        position_clock: value.position_clock.clone(),
        ttl_ms: value.ttl_ms,
        source: value.source.clone(),
    }
}

fn edge_semantic_cue_from_proto(
    value: messages_v1::EdgeSemanticCueMessage,
) -> contracts::EdgeSemanticCueMessage {
    contracts::EdgeSemanticCueMessage {
        cue_type: value.cue_type,
        text: value.text,
        confidence: value.confidence,
        position_x: value.position_x,
        distance_m: value.distance_m,
        position_clock: value.position_clock,
        ttl_ms: value.ttl_ms,
        source: value.source,
    }
}

fn vision_odometry_to_proto(
    value: &contracts::VisionOdometrySnapshot,
) -> messages_v1::VisionOdometrySnapshot {
    messages_v1::VisionOdometrySnapshot {
        source: value.source.clone(),
        applied: value.applied,
        optical_flow_score: value.optical_flow_score,
        variance_m2: value.variance_m2,
        pose_x_m: value.pose_x_m,
        pose_y_m: value.pose_y_m,
    }
}

fn vision_odometry_from_proto(
    value: messages_v1::VisionOdometrySnapshot,
) -> contracts::VisionOdometrySnapshot {
    contracts::VisionOdometrySnapshot {
        source: value.source,
        applied: value.applied,
        optical_flow_score: value.optical_flow_score,
        variance_m2: value.variance_m2,
        pose_x_m: value.pose_x_m,
        pose_y_m: value.pose_y_m,
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

fn hazard_type_to_proto(value: contracts::HazardType) -> types_v1::HazardType {
    match value {
        contracts::HazardType::Unspecified => types_v1::HazardType::Unspecified,
        contracts::HazardType::DropAhead => types_v1::HazardType::DropAhead,
        contracts::HazardType::StaticObstacle => types_v1::HazardType::StaticObstacle,
        contracts::HazardType::DynamicObstacle => types_v1::HazardType::DynamicObstacle,
        contracts::HazardType::Vehicle => types_v1::HazardType::Vehicle,
    }
}

fn hazard_type_from_proto(value: i32) -> Result<contracts::HazardType, TransportError> {
    let value = types_v1::HazardType::try_from(value).map_err(|_| TransportError::UnknownEnum {
        field: "HazardType",
        value,
    })?;

    Ok(match value {
        types_v1::HazardType::Unspecified => contracts::HazardType::Unspecified,
        types_v1::HazardType::DropAhead => contracts::HazardType::DropAhead,
        types_v1::HazardType::StaticObstacle => contracts::HazardType::StaticObstacle,
        types_v1::HazardType::DynamicObstacle => contracts::HazardType::DynamicObstacle,
        types_v1::HazardType::Vehicle => contracts::HazardType::Vehicle,
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

fn cognition_layer_to_proto(value: contracts::CognitionLayer) -> types_v1::CognitionLayer {
    match value {
        contracts::CognitionLayer::L1Survival => types_v1::CognitionLayer::L1Survival,
        contracts::CognitionLayer::L2Edge => types_v1::CognitionLayer::L2Edge,
        contracts::CognitionLayer::L3Cloud => types_v1::CognitionLayer::L3Cloud,
    }
}

fn cognition_layer_from_proto(value: i32) -> Result<contracts::CognitionLayer, TransportError> {
    let value =
        types_v1::CognitionLayer::try_from(value).map_err(|_| TransportError::UnknownEnum {
            field: "CognitionLayer",
            value,
        })?;

    Ok(match value {
        types_v1::CognitionLayer::L1Survival => contracts::CognitionLayer::L1Survival,
        types_v1::CognitionLayer::L2Edge => contracts::CognitionLayer::L2Edge,
        types_v1::CognitionLayer::L3Cloud => contracts::CognitionLayer::L3Cloud,
        types_v1::CognitionLayer::Unspecified => contracts::CognitionLayer::L3Cloud,
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
                timestamp_ms: 1_741_255_200_000,
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

    #[test]
    fn roundtrip_hazard_observation_payload() {
        let message = contracts::ClientToBackendMessage::HazardObservation(
            contracts::HazardObservationMessage {
                session_id: "haz-1".to_string(),
                timestamp_ms: 1_741_341_600_000,
                hazard_type: contracts::HazardType::DropAhead,
                bearing_x: Some(-0.2),
                distance_m: 1.4,
                relative_velocity_mps: -2.1,
                confidence: Some(0.93),
                source: Some("native_depth".to_string()),
                suppress_ms: Some(2800),
            },
        );

        let encoded = encode_client_message(&message).expect("encode should pass");
        let decoded = decode_client_message(&encoded).expect("decode should pass");
        assert_eq!(decoded, message);
    }

    #[test]
    fn roundtrip_safety_directive_payload() {
        let message =
            contracts::BackendToClientMessage::SafetyDirective(contracts::SafetyDirectiveMessage {
                session_id: "s-dir-1".to_string(),
                timestamp_ms: 1_741_341_600_000,
                hazard_type: Some(contracts::HazardType::DropAhead),
                hazard_score: 4.8,
                hard_stop: true,
                haptic_intensity: 0.92,
                spatial_audio_pitch_hz: 880.0,
                spatial_audio_pan: -0.3,
                needs_human_assistance: false,
                reason: Some("trace_replay".to_string()),
                flush_audio: true,
            });

        let encoded = encode_server_message(&message).expect("encode should pass");
        let decoded = decode_server_message(&encoded).expect("decode should pass");
        assert_eq!(decoded, message);
    }
}
