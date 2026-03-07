const HARD_STOP_THRESHOLD: f32 = 3.2;
const HUMAN_ASSIST_THRESHOLD: f32 = 6.2;
const HUMAN_ASSIST_SENSOR_HEALTH_THRESHOLD: f32 = 0.35;
const SAFE_SILENCE_DEADZONE: f32 = 0.1;
const PROXIMITY_OVERRIDE_DISTANCE_M: f32 = 0.6;
const PROXIMITY_OVERRIDE_CONFIDENCE: f32 = 0.6;
const PROXIMITY_OVERRIDE_MARGIN: f32 = 1.0;
const TTC_GAIN: f32 = 1.6;
const TTC_DECAY_S: f32 = 1.2;
const TTC_MIN_CLOSING_SPEED_MPS: f32 = 0.05;

#[derive(Debug, Clone, Copy)]
pub struct SafetyPolicyInput {
    pub hazard_confidence: f32,
    pub distance_m: f32,
    pub relative_velocity_mps: f32,
    pub bearing_x: f32,
    pub sensor_health_score: f32,
    pub localization_uncertainty_m: f32,
    pub edge_reflex_active: bool,
    pub continuous_hard_stop_duration_s: f32,
}

#[derive(Debug, Clone)]
pub struct SafetyPolicyDecision {
    pub hazard_score: f32,
    pub haptic_intensity: f32,
    pub spatial_audio_pitch_hz: f32,
    pub spatial_audio_pan: f32,
    pub hard_stop: bool,
    pub human_assistance: bool,
    pub reason: String,
}

impl SafetyPolicyDecision {
    pub fn should_emit_hard_stop(&self) -> bool {
        self.hard_stop
    }

    pub fn should_escalate_human(&self) -> bool {
        self.human_assistance
    }
}

pub fn evaluate_safety_policy(payload: SafetyPolicyInput) -> SafetyPolicyDecision {
    let confidence = payload.hazard_confidence.clamp(0.0, 1.0);
    let distance_m = payload.distance_m.max(0.0);
    let closing_speed = (-payload.relative_velocity_mps).max(0.0);
    let sensor_health = payload.sensor_health_score.clamp(0.0, 1.0);
    let loc_uncertainty = payload.localization_uncertainty_m.clamp(0.0, 300.0);
    let pan = payload.bearing_x.clamp(-1.0, 1.0);

    // H(d, v) = alpha*exp(-lambda*d) + beta*max(0, -v) + gamma*kappa
    let alpha = 1.5_f32;
    let lambda = 0.8_f32;
    let beta = 2.5_f32;
    let gamma = 1.2_f32;

    let distance_risk = alpha * (-lambda * distance_m).exp();
    let velocity_risk = beta * closing_speed;
    let confidence_risk = gamma * confidence;
    let time_to_collision_s = if closing_speed > TTC_MIN_CLOSING_SPEED_MPS {
        Some(distance_m / closing_speed)
    } else {
        None
    };
    let ttc_risk = time_to_collision_s
        .map(|ttc_s| TTC_GAIN * (-(ttc_s / TTC_DECAY_S)).exp())
        .unwrap_or(0.0);
    let proximity_override =
        distance_m < PROXIMITY_OVERRIDE_DISTANCE_M && confidence > PROXIMITY_OVERRIDE_CONFIDENCE;

    let mut hazard_score = distance_risk + velocity_risk + confidence_risk + ttc_risk;

    if sensor_health < 0.5 {
        hazard_score *= 1.3;
    }

    let uncertainty_scale = 1.0 + (loc_uncertainty / 8.0).min(1.5) * 0.35;
    hazard_score *= uncertainty_scale;

    if payload.edge_reflex_active {
        hazard_score += 5.0;
    }

    if proximity_override {
        hazard_score = hazard_score.max(HARD_STOP_THRESHOLD + PROXIMITY_OVERRIDE_MARGIN);
    }

    let activation =
        ((hazard_score - SAFE_SILENCE_DEADZONE) / (HARD_STOP_THRESHOLD + 1.0)).clamp(0.0, 1.0);
    let haptic_intensity = activation.powf(0.75);
    let spatial_audio_pitch_hz = if activation <= 0.0 {
        0.0
    } else {
        330.0 + activation * 770.0
    };
    let hard_stop = hazard_score > HARD_STOP_THRESHOLD || payload.edge_reflex_active;
    let human_assistance = (hazard_score > HUMAN_ASSIST_THRESHOLD
        && sensor_health < HUMAN_ASSIST_SENSOR_HEALTH_THRESHOLD)
        || payload.continuous_hard_stop_duration_s > 5.0;

    let reason = format!(
        "score={hazard_score:.2};conf={confidence:.2};distance_m={distance_m:.2};closing_speed={closing_speed:.2};ttc_s={:.2};sensor={sensor_health:.2};loc_uncertainty_m={loc_uncertainty:.1};edge_reflex={};prox_override={};pan={pan:.2};silence={}",
        time_to_collision_s.unwrap_or(-1.0),
        if payload.edge_reflex_active { "1" } else { "0" },
        if proximity_override { "1" } else { "0" },
        if hazard_score < SAFE_SILENCE_DEADZONE { "1" } else { "0" },
    );

    SafetyPolicyDecision {
        hazard_score,
        haptic_intensity,
        spatial_audio_pitch_hz,
        spatial_audio_pan: pan,
        hard_stop,
        human_assistance,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_fast_hazard_emits_hard_stop() {
        let decision = evaluate_safety_policy(SafetyPolicyInput {
            hazard_confidence: 0.88,
            distance_m: 1.1,
            relative_velocity_mps: -1.9,
            bearing_x: 0.2,
            sensor_health_score: 0.9,
            localization_uncertainty_m: 14.0,
            edge_reflex_active: false,
            continuous_hard_stop_duration_s: 0.0,
        });

        assert!(decision.should_emit_hard_stop());
        assert!(decision.haptic_intensity > 0.5);
    }

    #[test]
    fn critical_risk_with_bad_sensors_escalates_human() {
        let decision = evaluate_safety_policy(SafetyPolicyInput {
            hazard_confidence: 0.95,
            distance_m: 0.8,
            relative_velocity_mps: -2.4,
            bearing_x: -0.3,
            sensor_health_score: 0.18,
            localization_uncertainty_m: 220.0,
            edge_reflex_active: true,
            continuous_hard_stop_duration_s: 0.0,
        });

        assert!(decision.should_escalate_human());
    }

    #[test]
    fn deadzone_silences_audio_and_haptic_output() {
        let decision = evaluate_safety_policy(SafetyPolicyInput {
            hazard_confidence: 0.01,
            distance_m: 24.0,
            relative_velocity_mps: 0.6,
            bearing_x: 0.0,
            sensor_health_score: 0.98,
            localization_uncertainty_m: 0.2,
            edge_reflex_active: false,
            continuous_hard_stop_duration_s: 0.0,
        });

        assert!(decision.hazard_score < SAFE_SILENCE_DEADZONE);
        assert_eq!(decision.haptic_intensity, 0.0);
        assert_eq!(decision.spatial_audio_pitch_hz, 0.0);
    }

    #[test]
    fn ultra_close_static_hazard_forces_hard_stop() {
        let decision = evaluate_safety_policy(SafetyPolicyInput {
            hazard_confidence: 0.95,
            distance_m: 0.1,
            relative_velocity_mps: 0.0,
            bearing_x: 0.0,
            sensor_health_score: 0.95,
            localization_uncertainty_m: 0.4,
            edge_reflex_active: false,
            continuous_hard_stop_duration_s: 0.0,
        });

        assert!(decision.hard_stop);
        assert!(decision.reason.contains("prox_override=1"));
    }

    #[test]
    fn short_ttc_increases_hazard_score() {
        let approaching = evaluate_safety_policy(SafetyPolicyInput {
            hazard_confidence: 0.7,
            distance_m: 2.4,
            relative_velocity_mps: -1.5,
            bearing_x: 0.0,
            sensor_health_score: 0.95,
            localization_uncertainty_m: 2.0,
            edge_reflex_active: false,
            continuous_hard_stop_duration_s: 0.0,
        });
        let receding = evaluate_safety_policy(SafetyPolicyInput {
            hazard_confidence: 0.7,
            distance_m: 2.4,
            relative_velocity_mps: 0.4,
            bearing_x: 0.0,
            sensor_health_score: 0.95,
            localization_uncertainty_m: 2.0,
            edge_reflex_active: false,
            continuous_hard_stop_duration_s: 0.0,
        });

        assert!(approaching.hazard_score > receding.hazard_score);
        assert!(approaching.reason.contains("ttc_s="));
    }
}
