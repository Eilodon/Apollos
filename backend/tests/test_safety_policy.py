import unittest

try:
    from agent.safety_policy import SafetyPolicyInput, evaluate_safety_policy
except ModuleNotFoundError:  # pragma: no cover - package-style fallback
    from backend.agent.safety_policy import SafetyPolicyInput, evaluate_safety_policy


class SafetyPolicyTests(unittest.TestCase):
    def test_high_risk_path_emits_hard_stop(self) -> None:
        decision = evaluate_safety_policy(
            SafetyPolicyInput(
                hazard_confidence=0.92,
                distance_category='very_close',
                motion_state='walking_fast',
                sensor_health_score=0.82,
                localization_uncertainty_m=18,
                edge_reflex_active=False,
            )
        )
        self.assertIn(decision.tier, {'hard_stop', 'human_escalation'})
        self.assertTrue(decision.should_emit_hard_stop)

    def test_low_confidence_far_hazard_does_not_force_hard_stop(self) -> None:
        decision = evaluate_safety_policy(
            SafetyPolicyInput(
                hazard_confidence=0.21,
                distance_category='far',
                motion_state='stationary',
                sensor_health_score=0.95,
                localization_uncertainty_m=8,
                edge_reflex_active=False,
            )
        )
        self.assertFalse(decision.should_emit_hard_stop)
        self.assertIn(decision.tier, {'silent', 'ping', 'voice'})

    def test_very_close_hazard_biases_up_to_hard_stop(self) -> None:
        decision = evaluate_safety_policy(
            SafetyPolicyInput(
                hazard_confidence=0.60,
                distance_category='very_close',
                motion_state='walking_slow',
                sensor_health_score=0.91,
                localization_uncertainty_m=22,
                edge_reflex_active=False,
            )
        )
        self.assertTrue(decision.should_emit_hard_stop)

    def test_extreme_sensor_gap_can_trigger_human_escalation(self) -> None:
        decision = evaluate_safety_policy(
            SafetyPolicyInput(
                hazard_confidence=0.98,
                distance_category='very_close',
                motion_state='running',
                sensor_health_score=0.18,
                localization_uncertainty_m=160,
                edge_reflex_active=True,
            )
        )
        self.assertEqual(decision.tier, 'human_escalation')
        self.assertTrue(decision.should_escalate_human)


if __name__ == '__main__':
    unittest.main()
