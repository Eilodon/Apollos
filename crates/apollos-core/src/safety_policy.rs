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
}

pub fn tier_for_sensor_score(score: f32) -> SafetyTier {
    if score >= 0.85 {
        SafetyTier::Voice
    } else if score >= 0.70 {
        SafetyTier::Ping
    } else if score >= 0.45 {
        SafetyTier::HardStop
    } else {
        SafetyTier::HumanEscalation
    }
}

pub fn evaluate_safety_policy(payload: SafetyPolicyInput) -> SafetyPolicyDecision {
    let confidence = payload.hazard_confidence.clamp(0.0, 1.0);
    let sensor_health = payload.sensor_health_score.clamp(0.0, 1.0);
    let loc_uncertainty = payload.localization_uncertainty_m.clamp(0.0, 300.0);

    let mut risk_score = confidence * 3.2;
    risk_score += match payload.distance_category {
        DistanceCategory::VeryClose => 2.4,
        DistanceCategory::Mid => 1.4,
        DistanceCategory::Far => 0.5,
    };
    risk_score += match payload.motion_state {
        MotionState::Running => 1.2,
        MotionState::WalkingFast => 0.8,
        MotionState::WalkingSlow => 0.35,
        MotionState::Stationary => 0.0,
    };
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
        && matches!(tier, SafetyTier::HardStop | SafetyTier::HumanEscalation)
    {
        tier = SafetyTier::Voice;
    }

    SafetyPolicyDecision {
        tier,
        risk_score,
        reason: format!("risk={risk_score:.2};conf={confidence:.2};sensor={sensor_health:.2}"),
    }
}

pub fn max_tier(a: SafetyTier, b: SafetyTier) -> SafetyTier {
    if tier_rank(a) >= tier_rank(b) {
        a
    } else {
        b
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn very_close_hazard_biases_to_hard_stop() {
        let decision = evaluate_safety_policy(SafetyPolicyInput {
            hazard_confidence: 0.58,
            distance_category: DistanceCategory::VeryClose,
            motion_state: MotionState::WalkingFast,
            sensor_health_score: 0.92,
            localization_uncertainty_m: 20.0,
            edge_reflex_active: false,
        });
        assert!(decision.should_emit_hard_stop());
    }
}
