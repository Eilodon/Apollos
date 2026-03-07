use apollos_proto::contracts::{
    ClientToBackendMessage, MotionState, MultimodalFrameMessage, SensorHealthSnapshot,
};
use chrono::Utc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SessionState {
    pub session_id: String,
    pub motion_state: MotionState,
    pub sensor_health: Option<SensorHealthSnapshot>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            motion_state: MotionState::Stationary,
            sensor_health: None,
        }
    }

    pub fn bootstrap_message(&self) -> ClientToBackendMessage {
        ClientToBackendMessage::MultimodalFrame(MultimodalFrameMessage {
            session_id: self.session_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            frame_jpeg_base64: None,
            motion_state: self.motion_state,
            pitch: 0.0,
            velocity: 0.0,
            user_text: None,
            yaw_delta_deg: Some(0.0),
            carry_mode: None,
            sensor_unavailable: Some(false),
            lat: None,
            lng: None,
            heading_deg: None,
            location_accuracy_m: None,
            location_age_ms: None,
            sensor_health: self.sensor_health.clone(),
            sensor_uncertainty: None,
            vision_odometry: None,
            cloud_link: None,
            edge_semantic_cues: Vec::new(),
        })
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}
