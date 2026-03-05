import unittest

try:
    from agent.websocket_handler import WebSocketRegistry
except ModuleNotFoundError:  # pragma: no cover - package-style fallback
    from backend.agent.websocket_handler import WebSocketRegistry


class FakeSocket:
    async def send_json(self, _payload):
        return None


class WebSocketRegistryTests(unittest.IsolatedAsyncioTestCase):
    async def test_live_session_rejects_different_client_owner(self) -> None:
        registry = WebSocketRegistry()
        first = FakeSocket()
        second = FakeSocket()

        ok1, _ = await registry.register_live('session-1', first, client_id='client-a')
        ok2, reason2 = await registry.register_live('session-1', second, client_id='client-b')

        self.assertTrue(ok1)
        self.assertFalse(ok2)
        self.assertIn('owned', reason2)

    async def test_live_session_rejects_missing_client_id_when_owned(self) -> None:
        registry = WebSocketRegistry()
        first = FakeSocket()
        second = FakeSocket()

        ok1, _ = await registry.register_live('session-1b', first, client_id='client-a')
        ok2, reason2 = await registry.register_live('session-1b', second, client_id=None)

        self.assertTrue(ok1)
        self.assertFalse(ok2)
        self.assertIn('owned', reason2)

    async def test_emergency_requires_same_client_owner_when_live_registered(self) -> None:
        registry = WebSocketRegistry()
        live = FakeSocket()
        emergency = FakeSocket()

        ok_live, _ = await registry.register_live('session-2', live, client_id='client-a')
        ok_emergency, reason_emergency = await registry.register_emergency(
            'session-2',
            emergency,
            client_id='client-b',
        )

        self.assertTrue(ok_live)
        self.assertFalse(ok_emergency)
        self.assertIn('mismatch', reason_emergency)

    async def test_emergency_accepts_matching_client_owner(self) -> None:
        registry = WebSocketRegistry()
        live = FakeSocket()
        emergency = FakeSocket()

        ok_live, _ = await registry.register_live('session-3', live, client_id='client-a')
        ok_emergency, reason_emergency = await registry.register_emergency(
            'session-3',
            emergency,
            client_id='client-a',
        )

        self.assertTrue(ok_live)
        self.assertTrue(ok_emergency)
        self.assertEqual(reason_emergency, '')

    async def test_emergency_rejects_missing_client_id_when_owned(self) -> None:
        registry = WebSocketRegistry()
        live = FakeSocket()
        emergency = FakeSocket()

        ok_live, _ = await registry.register_live('session-4', live, client_id='client-a')
        ok_emergency, reason_emergency = await registry.register_emergency(
            'session-4',
            emergency,
            client_id=None,
        )

        self.assertTrue(ok_live)
        self.assertFalse(ok_emergency)
        self.assertIn('mismatch', reason_emergency)


if __name__ == '__main__':
    unittest.main()
