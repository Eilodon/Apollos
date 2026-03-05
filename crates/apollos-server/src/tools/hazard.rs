#[derive(Debug, Clone)]
pub struct HazardEvent {
    pub hazard_type: String,
    pub confidence: f32,
}

pub fn normalize_hazard_confidence(raw: f32) -> f32 {
    raw.clamp(0.0, 1.0)
}
