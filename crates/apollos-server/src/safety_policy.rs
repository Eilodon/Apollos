use apollos_proto::contracts::{DistanceCategory, MotionState, SafetyTier};

#[derive(Debug, Clone, Copy)]
pub struct SafetyPolicyInput {
    pub hazard_confidence: f32,
    pub distance_category: DistanceCategory,
    pub motion_state: MotionState,
    pub sensor_health_score: f32,
    pub localization_uncertainty_m: f32,
    pub edge_reflex_active: bool,
}

#[derive(Debug, Clone)]
pub struct SafetyPolicyDecision {
    pub tier: SafetyTier,
    pub risk_score: f32,
    pub reason: String,
}

impl SafetyPolicyDecision {
    pub fn should_emit_hard_stop(&self) -> bool {
        tier_rank(self.tier) >= tier_rank(SafetyTier::HardStop)
    }

    pub fn should_escalate_human(&self) -> bool {
        self.tier == SafetyTier::HumanEscalation
    }
}

fn clamp(value: f32, low: f32, high: f32) -> f32 {
    value.max(low).min(high)
}

fn distance_weight(distance: DistanceCategory) -> f32 {
    match distance {
        DistanceCategory::VeryClose => 2.4,
        DistanceCategory::Mid => 1.4,
        DistanceCategory::Far => 0.5,
    }
}

fn motion_weight(motion_state: MotionState) -> f32 {
    match motion_state {
        MotionState::Running => 1.2,
        MotionState::WalkingFast => 0.8,
        MotionState::WalkingSlow => 0.35,
        MotionState::Stationary => 0.0,
    }
}

fn tier_rank(tier: SafetyTier) -> u8 {
    match tier {
        SafetyTier::Silent => 0,
        SafetyTier::Ping => 1,
        SafetyTier::Voice => 2,
        SafetyTier::HardStop => 3,
        SafetyTier::HumanEscalation => 4,
    }
}

pub fn max_tier(a: SafetyTier, b: SafetyTier) -> SafetyTier {
    if tier_rank(a) >= tier_rank(b) {
        a
    } else {
        b
    }
}

pub fn evaluate_safety_policy(payload: SafetyPolicyInput) -> SafetyPolicyDecision {
    let confidence = clamp(payload.hazard_confidence, 0.0, 1.0);
    let sensor_health = clamp(payload.sensor_health_score, 0.0, 1.0);
    let loc_uncertainty = clamp(payload.localization_uncertainty_m, 0.0, 300.0);

    let mut risk_score = confidence * 3.2;
    risk_score += distance_weight(payload.distance_category);
    risk_score += motion_weight(payload.motion_state);
    risk_score += (1.0 - sensor_health) * 1.8;
    risk_score += (loc_uncertainty / 100.0).min(1.0) * 0.8;

    if payload.edge_reflex_active {
        risk_score += 1.5;
    }

    let mut tier = if risk_score >= 6.0 {
        if sensor_health < 0.30 {
            SafetyTier::HumanEscalation
        } else {
            SafetyTier::HardStop
        }
    } else if risk_score >= 4.2 {
        SafetyTier::HardStop
    } else if risk_score >= 3.0 {
        SafetyTier::Voice
    } else if risk_score >= 2.0 {
        SafetyTier::Ping
    } else {
        SafetyTier::Silent
    };

    if payload.distance_category == DistanceCategory::VeryClose {
        tier = max_tier(tier, SafetyTier::Voice);
        if confidence >= 0.55 || payload.edge_reflex_active {
            tier = max_tier(tier, SafetyTier::HardStop);
        }
    }

    if payload.distance_category == DistanceCategory::Far
        && confidence < 0.40
        && !payload.edge_reflex_active
        && (tier == SafetyTier::HardStop || tier == SafetyTier::HumanEscalation)
    {
        tier = SafetyTier::Voice;
    }

    let reason = format!(
        "conf={confidence:.2}; distance={}; motion={}; sensor_health={sensor_health:.2}; loc_uncertainty_m={loc_uncertainty:.1}; edge_reflex={}; risk={risk_score:.2}",
        distance_str(payload.distance_category),
        motion_str(payload.motion_state),
        if payload.edge_reflex_active { "1" } else { "0" },
    );

    SafetyPolicyDecision {
        tier,
        risk_score,
        reason,
    }
}

fn distance_str(distance: DistanceCategory) -> &'static str {
    match distance {
        DistanceCategory::VeryClose => "very_close",
        DistanceCategory::Mid => "mid",
        DistanceCategory::Far => "far",
    }
}

fn motion_str(state: MotionState) -> &'static str {
    match state {
        MotionState::Stationary => "stationary",
        MotionState::WalkingSlow => "walking_slow",
        MotionState::WalkingFast => "walking_fast",
        MotionState::Running => "running",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn very_close_medium_confidence_emits_hard_stop() {
        let decision = evaluate_safety_policy(SafetyPolicyInput {
            hazard_confidence: 0.60,
            distance_category: DistanceCategory::VeryClose,
            motion_state: MotionState::WalkingFast,
            sensor_health_score: 0.85,
            localization_uncertainty_m: 18.0,
            edge_reflex_active: false,
        });

        assert!(decision.should_emit_hard_stop());
    }

    #[test]
    fn low_confidence_far_hazard_is_downgraded() {
        let decision = evaluate_safety_policy(SafetyPolicyInput {
            hazard_confidence: 0.20,
            distance_category: DistanceCategory::Far,
            motion_state: MotionState::Running,
            sensor_health_score: 0.10,
            localization_uncertainty_m: 200.0,
            edge_reflex_active: false,
        });

        assert_eq!(decision.tier, SafetyTier::Voice);
    }

    #[test]
    fn critical_risk_with_bad_sensors_escalates_human() {
        let decision = evaluate_safety_policy(SafetyPolicyInput {
            hazard_confidence: 0.95,
            distance_category: DistanceCategory::VeryClose,
            motion_state: MotionState::Running,
            sensor_health_score: 0.15,
            localization_uncertainty_m: 250.0,
            edge_reflex_active: true,
        });

        assert!(decision.should_escalate_human());
    }
}
