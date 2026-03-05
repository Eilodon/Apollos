import unittest

try:
    from agent.session_manager import SessionStore
    from agent.tools.hazard_logger import log_hazard_event
    from agent.tools.runtime import configure_runtime, reset_current_session, set_current_session
    from agent.websocket_handler import WebSocketRegistry
except ModuleNotFoundError:  # pragma: no cover - package-style fallback
    from backend.agent.session_manager import SessionStore
    from backend.agent.tools.hazard_logger import log_hazard_event
    from backend.agent.tools.runtime import configure_runtime, reset_current_session, set_current_session
    from backend.agent.websocket_handler import WebSocketRegistry


class FakeSocket:
    def __init__(self) -> None:
        self.messages = []

    async def send_json(self, payload):
        self.messages.append(payload)


class HazardLoggerTests(unittest.IsolatedAsyncioTestCase):
    async def test_hard_stop_sent_to_emergency_channel(self) -> None:
        store = SessionStore(use_firestore=False)
        registry = WebSocketRegistry()
        configure_runtime(store, registry)

        emergency = FakeSocket()
        await registry.register_emergency('session-abc', emergency)

        token = set_current_session('session-abc')
        try:
            result = await log_hazard_event(
                hazard_type='vehicle',
                position_x=0.8,
                distance_category='very_close',
                confidence=0.95,
                description='Fast moving motorbike',
                session_id='session-abc',
            )
        finally:
            reset_current_session(token)

        self.assertEqual(len(emergency.messages), 1)
        self.assertEqual(emergency.messages[0]['type'], 'HARD_STOP')
        self.assertIn('server_emit_ts_ms', emergency.messages[0])
        self.assertTrue(bool(result.get('hard_stop_emitted')))
        self.assertIn(result.get('safety_tier'), {'hard_stop', 'human_escalation'})

    async def test_hazard_type_is_normalized_to_vn_taxonomy(self) -> None:
        store = SessionStore(use_firestore=False)
        registry = WebSocketRegistry()
        configure_runtime(store, registry)

        emergency = FakeSocket()
        await registry.register_emergency('session-vn', emergency)

        token = set_current_session('session-vn')
        try:
            result = await log_hazard_event(
                hazard_type='xe máy',
                position_x=-0.3,
                distance_category='mid',
                confidence=0.8,
                description='Xe may dung tren via he',
                session_id='session-vn',
            )
        finally:
            reset_current_session(token)

        self.assertEqual(len(emergency.messages), 1)
        self.assertEqual(emergency.messages[0]['hazard_type'], 'parked_motorbike')
        self.assertIn(result.get('safety_tier'), {'voice', 'hard_stop', 'human_escalation'})

    async def test_low_confidence_far_hazard_can_avoid_hard_stop(self) -> None:
        store = SessionStore(use_firestore=False)
        registry = WebSocketRegistry()
        configure_runtime(store, registry)

        emergency = FakeSocket()
        live = FakeSocket()
        await registry.register_emergency('session-low-risk', emergency)
        await registry.register_live('session-low-risk', live)

        token = set_current_session('session-low-risk')
        try:
            result = await log_hazard_event(
                hazard_type='unknown_object',
                position_x=0.1,
                distance_category='far',
                confidence=0.22,
                description='uncertain object far away',
                session_id='session-low-risk',
            )
        finally:
            reset_current_session(token)

        self.assertFalse(bool(result.get('hard_stop_emitted')))
        self.assertEqual(len(emergency.messages), 0)
        # Can still send softer response through live channel.
        self.assertTrue(any(msg.get('type') in {'assistant_text', 'semantic_cue'} for msg in live.messages))


if __name__ == '__main__':
    unittest.main()
