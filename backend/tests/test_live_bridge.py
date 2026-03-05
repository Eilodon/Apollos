import unittest
import time
from typing import Any

try:
    from agent.live_bridge import GeminiLiveBridge
except ModuleNotFoundError:  # pragma: no cover - package-style fallback
    from backend.agent.live_bridge import GeminiLiveBridge


class FakeSessionStore:
    pass


class FakeWebSocketRegistry:
    def __init__(self):
        self.messages = []

    async def send_live(self, session_id: str, payload: dict[str, Any]) -> bool:
        self.messages.append((session_id, payload))
        return True


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

    def test_risk_multiplier_increases_with_unstable_motion(self):
        calm = self.bridge._compute_risk_multiplier('walking_slow', pitch=3, velocity=0.4, yaw_delta=1)
        risky = self.bridge._compute_risk_multiplier('running', pitch=25, velocity=3.1, yaw_delta=40)
        self.assertGreater(risky, calm)
        self.assertLessEqual(risky, 4.0)

    async def test_edge_hazard_suppresses_cloud_audio_and_text(self):
        self.bridge.note_edge_hazard('DROP_AHEAD')
        await self.bridge._handle_live_response({'text': 'Stop now', 'data': 'ZmFrZV9hdWRpbw=='})
        self.assertEqual(len(self.bridge._websocket_registry.messages), 0)

    async def test_identify_location_emits_destination_near_when_within_30m(self):
        sent_messages = []

        class LocalRegistry(FakeWebSocketRegistry):
            async def send_live(self, session_id: str, payload: dict[str, Any]) -> bool:
                sent_messages.append((session_id, payload))
                return True

        async def location_dispatcher(name: str, args: dict[str, Any]) -> dict[str, Any]:
            if name == 'identify_location':
                return {'ok': True, 'distance_m': 24, 'destination_near': True}
            return {'ok': True}

        bridge = GeminiLiveBridge(
            session_id='destination-near',
            session_store=FakeSessionStore(),
            websocket_registry=LocalRegistry(),
            tool_dispatcher=location_dispatcher,
        )

        class FakeSession:
            async def send_tool_response(self, function_responses):
                pass

        bridge._session = FakeSession()
        await bridge._handle_tool_calls([{'name': 'identify_location', 'id': 'loc1', 'args': {}}])

        cues = [payload for _, payload in sent_messages if payload.get('type') == 'semantic_cue']
        self.assertTrue(any(cue.get('cue') == 'destination_near' for cue in cues))

if __name__ == '__main__':
    unittest.main()
