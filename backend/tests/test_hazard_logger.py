import unittest

from agent.session_manager import SessionStore
from agent.tools.hazard_logger import log_hazard_event
from agent.tools.runtime import configure_runtime, reset_current_session, set_current_session
from agent.websocket_handler import WebSocketRegistry


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
            await log_hazard_event(
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


if __name__ == '__main__':
    unittest.main()
