use apollos_server::safety_policy::{evaluate_safety_policy, SafetyPolicyInput};

#[derive(Debug, Clone, Copy)]
struct HazardTrace {
    name: &'static str,
    input: SafetyPolicyInput,
    expect_hard_stop: bool,
    expect_human_assistance: bool,
}

#[test]
fn replay_hazard_traces_matches_tuned_thresholds() {
    let traces = [
        HazardTrace {
            name: "stair_drop_imminent",
            input: SafetyPolicyInput {
                hazard_confidence: 0.92,
                distance_m: 0.9,
                relative_velocity_mps: -2.2,
                bearing_x: 0.0,
                sensor_health_score: 0.90,
                localization_uncertainty_m: 12.0,
                edge_reflex_active: false,
            },
            expect_hard_stop: true,
            expect_human_assistance: false,
        },
        HazardTrace {
            name: "bike_crossing_fast",
            input: SafetyPolicyInput {
                hazard_confidence: 0.88,
                distance_m: 1.6,
                relative_velocity_mps: -2.6,
                bearing_x: 0.2,
                sensor_health_score: 0.86,
                localization_uncertainty_m: 18.0,
                edge_reflex_active: false,
            },
            expect_hard_stop: true,
            expect_human_assistance: false,
        },
        HazardTrace {
            name: "narrow_pole_closing",
            input: SafetyPolicyInput {
                hazard_confidence: 0.76,
                distance_m: 1.2,
                relative_velocity_mps: -1.4,
                bearing_x: -0.1,
                sensor_health_score: 0.88,
                localization_uncertainty_m: 10.0,
                edge_reflex_active: false,
            },
            expect_hard_stop: true,
            expect_human_assistance: false,
        },
        HazardTrace {
            name: "moderate_close_slow_closing",
            input: SafetyPolicyInput {
                hazard_confidence: 0.60,
                distance_m: 1.2,
                relative_velocity_mps: -0.8,
                bearing_x: 0.1,
                sensor_health_score: 0.90,
                localization_uncertainty_m: 10.0,
                edge_reflex_active: false,
            },
            expect_hard_stop: true,
            expect_human_assistance: false,
        },
        HazardTrace {
            name: "sensor_blind_critical",
            input: SafetyPolicyInput {
                hazard_confidence: 0.95,
                distance_m: 1.1,
                relative_velocity_mps: -2.4,
                bearing_x: -0.3,
                sensor_health_score: 0.22,
                localization_uncertainty_m: 180.0,
                edge_reflex_active: true,
            },
            expect_hard_stop: true,
            expect_human_assistance: true,
        },
        HazardTrace {
            name: "prolonged_sensor_blindness",
            input: SafetyPolicyInput {
                hazard_confidence: 0.84,
                distance_m: 1.0,
                relative_velocity_mps: -1.8,
                bearing_x: 0.0,
                sensor_health_score: 0.28,
                localization_uncertainty_m: 220.0,
                edge_reflex_active: false,
            },
            expect_hard_stop: true,
            expect_human_assistance: true,
        },
        HazardTrace {
            name: "static_far_sign",
            input: SafetyPolicyInput {
                hazard_confidence: 0.68,
                distance_m: 5.0,
                relative_velocity_mps: 0.0,
                bearing_x: 0.4,
                sensor_health_score: 0.90,
                localization_uncertainty_m: 8.0,
                edge_reflex_active: false,
            },
            expect_hard_stop: false,
            expect_human_assistance: false,
        },
        HazardTrace {
            name: "receding_object",
            input: SafetyPolicyInput {
                hazard_confidence: 0.80,
                distance_m: 2.2,
                relative_velocity_mps: 0.9,
                bearing_x: -0.2,
                sensor_health_score: 0.85,
                localization_uncertainty_m: 12.0,
                edge_reflex_active: false,
            },
            expect_hard_stop: false,
            expect_human_assistance: false,
        },
        HazardTrace {
            name: "low_confidence_noise",
            input: SafetyPolicyInput {
                hazard_confidence: 0.25,
                distance_m: 1.8,
                relative_velocity_mps: -0.4,
                bearing_x: 0.0,
                sensor_health_score: 0.92,
                localization_uncertainty_m: 15.0,
                edge_reflex_active: false,
            },
            expect_hard_stop: false,
            expect_human_assistance: false,
        },
    ];

    for trace in traces {
        let decision = evaluate_safety_policy(trace.input);

        assert_eq!(
            decision.hard_stop, trace.expect_hard_stop,
            "hard_stop mismatch for {}: score={:.2} reason={}",
            trace.name, decision.hazard_score, decision.reason
        );
        assert_eq!(
            decision.human_assistance, trace.expect_human_assistance,
            "human_assistance mismatch for {}: score={:.2} reason={}",
            trace.name, decision.hazard_score, decision.reason
        );
    }
}
