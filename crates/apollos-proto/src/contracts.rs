use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionState {
    Stationary,
    WalkingSlow,
    WalkingFast,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistanceCategory {
    VeryClose,
    Mid,
    Far,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavigationMode {
    #[serde(rename = "NAVIGATION")]
    Navigation,
    #[serde(rename = "EXPLORE")]
    Explore,
    #[serde(rename = "READ")]
    Read,
    #[serde(rename = "QUIET")]
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarryMode {
    HandHeld,
    Necklace,
    ChestClip,
    Pocket,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionSnapshot {
    pub state: MotionState,
    pub pitch: f32,
    pub velocity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorHealthSnapshot {
    pub score: f32,
    pub flags: Vec<String>,
    pub degraded: bool,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorUncertaintySnapshot {
    pub covariance_3x3: Vec<f32>,
    pub innovation_norm: f32,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultimodalFrameMessage {
    pub session_id: String,
    pub timestamp: String,
    pub frame_jpeg_base64: Option<String>,
    pub motion_state: MotionState,
    pub pitch: f32,
    pub velocity: f32,
    pub user_text: Option<String>,
    pub yaw_delta_deg: Option<f32>,
    pub carry_mode: Option<CarryMode>,
    pub sensor_unavailable: Option<bool>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub heading_deg: Option<f32>,
    pub location_accuracy_m: Option<f32>,
    pub location_age_ms: Option<u64>,
    pub sensor_health: Option<SensorHealthSnapshot>,
    pub sensor_uncertainty: Option<SensorUncertaintySnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioChunkMessage {
    pub session_id: String,
    pub timestamp: String,
    pub audio_chunk_pcm16: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserCommandMessage {
    pub session_id: String,
    pub timestamp: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HazardObservationMessage {
    pub session_id: String,
    pub timestamp: String,
    pub hazard_type: String,
    pub bearing_x: Option<f32>,
    pub distance_m: Option<f32>,
    pub relative_velocity_mps: Option<f32>,
    pub confidence: Option<f32>,
    pub source: Option<String>,
    pub suppress_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantTextMessage {
    pub session_id: String,
    pub timestamp: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantAudioMessage {
    pub session_id: String,
    pub timestamp: String,
    pub pcm24: Option<String>,
    pub pcm16: Option<String>,
    pub hazard_position_x: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticCue {
    ApproachingObject,
    SoftObstacle,
    TurningRecommended,
    DestinationNear,
    PocketModeActive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticCueMessage {
    pub cue: SemanticCue,
    pub position_x: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetyDirectiveMessage {
    pub session_id: String,
    pub timestamp: String,
    pub hazard_type: Option<String>,
    pub hazard_score: f32,
    pub hard_stop: bool,
    pub haptic_intensity: f32,
    pub spatial_audio_pitch_hz: f32,
    pub spatial_audio_pan: f32,
    pub needs_human_assistance: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Connected,
    Reconnecting,
    Disconnected,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionStateMessage {
    pub state: ConnectionState,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanHelpProvider {
    Twilio,
    Livekit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HumanHelpRtcSession {
    pub provider: HumanHelpProvider,
    pub room_name: String,
    pub identity: Option<String>,
    pub token: String,
    pub expires_in: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HumanHelpSessionMessage {
    pub session_id: String,
    pub timestamp: String,
    pub help_link: Option<String>,
    pub rtc: HumanHelpRtcSession,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientToBackendMessage {
    #[serde(rename = "multimodal_frame")]
    MultimodalFrame(MultimodalFrameMessage),
    #[serde(rename = "audio_chunk")]
    AudioChunk(AudioChunkMessage),
    #[serde(rename = "user_command")]
    UserCommand(UserCommandMessage),
    #[serde(rename = "hazard_observation")]
    HazardObservation(HazardObservationMessage),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BackendToClientMessage {
    #[serde(rename = "assistant_text")]
    AssistantText(AssistantTextMessage),
    #[serde(rename = "audio_chunk")]
    AssistantAudio(AssistantAudioMessage),
    #[serde(rename = "safety_directive")]
    SafetyDirective(SafetyDirectiveMessage),
    #[serde(rename = "connection_state")]
    ConnectionState(ConnectionStateMessage),
    #[serde(rename = "semantic_cue")]
    SemanticCue(SemanticCueMessage),
    #[serde(rename = "human_help_session")]
    HumanHelpSession(HumanHelpSessionMessage),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_multimodal_frame_with_type_tag() {
        let payload = ClientToBackendMessage::MultimodalFrame(MultimodalFrameMessage {
            session_id: "s1".to_string(),
            timestamp: "2026-03-05T10:00:00Z".to_string(),
            frame_jpeg_base64: None,
            motion_state: MotionState::WalkingFast,
            pitch: 10.0,
            velocity: 2.2,
            user_text: None,
            yaw_delta_deg: Some(6.0),
            carry_mode: Some(CarryMode::Necklace),
            sensor_unavailable: Some(false),
            lat: None,
            lng: None,
            heading_deg: None,
            location_accuracy_m: None,
            location_age_ms: None,
            sensor_health: None,
            sensor_uncertainty: None,
        });

        let json = serde_json::to_string(&payload).expect("should serialize");
        assert!(json.contains("\"type\":\"multimodal_frame\""));
        assert!(json.contains("\"motion_state\":\"walking_fast\""));
    }
}
