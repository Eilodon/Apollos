import unittest

try:
    from agent.session_manager import SessionStore
except ModuleNotFoundError:  # pragma: no cover - package-style fallback
    from backend.agent.session_manager import SessionStore


class ObservabilityStateTests(unittest.IsolatedAsyncioTestCase):
    async def test_low_sensor_health_enables_degraded_mode(self) -> None:
        store = SessionStore(use_firestore=False)
        snapshot = await store.update_observability(
            'session-observe-1',
            sensor_health_score=0.32,
            sensor_health_flags=['depth_error'],
            localization_uncertainty_m=55,
        )
        self.assertTrue(snapshot['degraded_mode'])
        self.assertIn('low_sensor_health', snapshot['degraded_reason'])

    async def test_recovery_disables_degraded_mode(self) -> None:
        store = SessionStore(use_firestore=False)
        await store.update_observability(
            'session-observe-2',
            sensor_health_score=0.30,
            sensor_health_flags=['location_missing'],
            localization_uncertainty_m=90,
        )
        recovered = await store.update_observability(
            'session-observe-2',
            sensor_health_score=0.92,
            sensor_health_flags=[],
            localization_uncertainty_m=12,
        )
        self.assertFalse(recovered['degraded_mode'])
        self.assertEqual(recovered['degraded_reason'], '')


if __name__ == '__main__':
    unittest.main()
