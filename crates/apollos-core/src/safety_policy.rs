const HARD_STOP_THRESHOLD: f32 = 3.2;
const HUMAN_ASSIST_THRESHOLD: f32 = 6.2;
const HUMAN_ASSIST_SENSOR_HEALTH_THRESHOLD: f32 = 0.35;
const SAFE_SILENCE_DEADZONE: f32 = 0.1;

#[derive(Debug, Clone, Copy)]
pub struct FluidSafetyInput {
    pub hazard_confidence: f32,
    pub distance_m: f32,
    pub relative_velocity_mps: f32,
    pub sensor_health_score: f32,
    pub localization_uncertainty_m: f32,
    pub edge_reflex_active: bool,
}

#[derive(Debug, Clone)]
pub struct SafetyPolicyDecision {
    pub hazard_score: f32,
    pub haptic_intensity: f32,
    pub spatial_audio_pitch_hz: f32,
    pub needs_hard_stop: bool,
    pub needs_human_assistance: bool,
    pub reason: String,
}

pub fn evaluate_fluid_safety(payload: FluidSafetyInput) -> SafetyPolicyDecision {
    let confidence = payload.hazard_confidence.clamp(0.0, 1.0);
    let distance_m = payload.distance_m.max(0.0);
    let closing_speed = (-payload.relative_velocity_mps).max(0.0);
    let sensor_health = payload.sensor_health_score.clamp(0.0, 1.0);
    let loc_uncertainty = payload.localization_uncertainty_m.clamp(0.0, 300.0);

    // H(d, v) = alpha*exp(-lambda*d) + beta*max(0, -v) + gamma*kappa
    let alpha = 1.5_f32;
    let lambda = 0.8_f32;
    let beta = 2.5_f32;
    let gamma = 1.2_f32;

    let distance_risk = alpha * (-lambda * distance_m).exp();
    let velocity_risk = beta * closing_speed;
    let confidence_risk = gamma * confidence;

    let mut hazard_score = distance_risk + velocity_risk + confidence_risk;

    if sensor_health < 0.5 {
        hazard_score *= 1.3;
    }

    let uncertainty_scale = 1.0 + (loc_uncertainty / 8.0).min(1.5) * 0.35;
    hazard_score *= uncertainty_scale;

    if payload.edge_reflex_active {
        hazard_score += 5.0;
    }

    let activation =
        ((hazard_score - SAFE_SILENCE_DEADZONE) / (HARD_STOP_THRESHOLD + 1.0)).clamp(0.0, 1.0);
    let haptic_intensity = activation.powf(0.75);
    let spatial_audio_pitch_hz = if activation <= 0.0 {
        0.0
    } else {
        330.0 + activation * 770.0
    };
    let needs_hard_stop = hazard_score > HARD_STOP_THRESHOLD || payload.edge_reflex_active;
    let needs_human_assistance = hazard_score > HUMAN_ASSIST_THRESHOLD
        && sensor_health < HUMAN_ASSIST_SENSOR_HEALTH_THRESHOLD;

    SafetyPolicyDecision {
        hazard_score,
        haptic_intensity,
        spatial_audio_pitch_hz,
        needs_hard_stop,
        needs_human_assistance,
        reason: format!(
            "score={hazard_score:.2};conf={confidence:.2};distance_m={distance_m:.2};closing_speed={closing_speed:.2};sensor={sensor_health:.2};loc_unc_m={loc_uncertainty:.1};edge_reflex={};silence={}",
            if payload.edge_reflex_active { "1" } else { "0" },
            if hazard_score < SAFE_SILENCE_DEADZONE { "1" } else { "0" },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_fast_hazard_triggers_hard_stop() {
        let decision = evaluate_fluid_safety(FluidSafetyInput {
            hazard_confidence: 0.8,
            distance_m: 1.2,
            relative_velocity_mps: -1.8,
            sensor_health_score: 0.9,
            localization_uncertainty_m: 12.0,
            edge_reflex_active: false,
        });

        assert!(decision.needs_hard_stop);
        assert!(decision.haptic_intensity > 0.5);
    }

    #[test]
    fn low_sensor_health_pushes_human_assistance() {
        let decision = evaluate_fluid_safety(FluidSafetyInput {
            hazard_confidence: 0.95,
            distance_m: 0.8,
            relative_velocity_mps: -2.2,
            sensor_health_score: 0.2,
            localization_uncertainty_m: 160.0,
            edge_reflex_active: true,
        });

        assert!(decision.needs_hard_stop);
        assert!(decision.needs_human_assistance);
    }

    #[test]
    fn safe_deadzone_keeps_audio_and_haptics_silent() {
        let decision = evaluate_fluid_safety(FluidSafetyInput {
            hazard_confidence: 0.02,
            distance_m: 20.0,
            relative_velocity_mps: 0.8,
            sensor_health_score: 1.0,
            localization_uncertainty_m: 0.5,
            edge_reflex_active: false,
        });

        assert!(decision.hazard_score < SAFE_SILENCE_DEADZONE);
        assert_eq!(decision.haptic_intensity, 0.0);
        assert_eq!(decision.spatial_audio_pitch_hz, 0.0);
    }
}
