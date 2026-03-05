import unittest
import time
from typing import Any

from agent.live_bridge import GeminiLiveBridge


class FakeSessionStore:
    pass


class FakeWebSocketRegistry:
    pass


class LiveBridgeTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.dispatched_calls = []

        async def fake_dispatcher(name: str, args: dict[str, Any]) -> dict[str, Any]:
            self.dispatched_calls.append((name, args))
            return {'ok': True, 'position_x': args.get('position_x', 0.0)}

        self.bridge = GeminiLiveBridge(
            session_id='test-session',
            session_store=FakeSessionStore(),
            websocket_registry=FakeWebSocketRegistry(),
            tool_dispatcher=fake_dispatcher,
        )

        # Mock the session so it doesn't try to send responses to real Gemini
        class FakeSession:
            async def send_tool_response(self, function_responses):
                pass
        
        self.bridge._session = FakeSession()
        self.bridge._hazard_confirmation_frames = 2

    async def test_single_frame_does_not_fire(self):
        call = [{'name': 'log_hazard_event', 'id': 'call1', 'args': {'position_x': 0.5}}]
        await self.bridge._handle_tool_calls(call)

        self.assertEqual(len(self.dispatched_calls), 0)
        self.assertEqual(self.bridge._consecutive_hazard_frames, 1)

    async def test_two_frames_fire(self):
        call1 = [{'name': 'log_hazard_event', 'id': 'call1', 'args': {'position_x': 0.5}}]
        await self.bridge._handle_tool_calls(call1)
        self.assertEqual(len(self.dispatched_calls), 0)

        call2 = [{'name': 'log_hazard_event', 'id': 'call2', 'args': {'position_x': 0.6}}]
        await self.bridge._handle_tool_calls(call2)
        
        # Fire on second call
        self.assertEqual(len(self.dispatched_calls), 1)
        self.assertEqual(self.bridge._consecutive_hazard_frames, 0) # reset after fire

    async def test_fast_motion_skips_frame_check(self):
        self.bridge._last_motion_state = 'walking_fast'
        
        call = [{'name': 'log_hazard_event', 'id': 'call1', 'args': {'position_x': 0.5}}]
        await self.bridge._handle_tool_calls(call)

        # Fires immediately on first frame
        self.assertEqual(len(self.dispatched_calls), 1)
        self.assertEqual(self.bridge._consecutive_hazard_frames, 0)

    async def test_buffer_resets_after_timeout(self):
        call1 = [{'name': 'log_hazard_event', 'id': 'call1', 'args': {'position_x': 0.5}}]
        await self.bridge._handle_tool_calls(call1)
        self.assertEqual(len(self.dispatched_calls), 0)
        
        # Simulate 4 seconds passed
        self.bridge._last_hazard_ts -= 4.0

        call2 = [{'name': 'log_hazard_event', 'id': 'call2', 'args': {'position_x': 0.6}}]
        await self.bridge._handle_tool_calls(call2)

        # Should still be 0 dispatches because the first one expired
        self.assertEqual(len(self.dispatched_calls), 0)
        self.assertEqual(self.bridge._consecutive_hazard_frames, 1)

    async def test_default_config_fires_on_first_frame(self):
        dispatched_calls = []

        async def fake_dispatcher(name: str, args: dict[str, Any]) -> dict[str, Any]:
            dispatched_calls.append((name, args))
            return {'ok': True, 'position_x': args.get('position_x', 0.0)}

        bridge = GeminiLiveBridge(
            session_id='default-threshold',
            session_store=FakeSessionStore(),
            websocket_registry=FakeWebSocketRegistry(),
            tool_dispatcher=fake_dispatcher,
        )

        class FakeSession:
            async def send_tool_response(self, function_responses):
                pass

        bridge._session = FakeSession()
        self.assertEqual(bridge._hazard_confirmation_frames, 1)

        call = [{'name': 'log_hazard_event', 'id': 'call1', 'args': {'position_x': 0.2}}]
        await bridge._handle_tool_calls(call)
        self.assertEqual(len(dispatched_calls), 1)

if __name__ == '__main__':
    unittest.main()
