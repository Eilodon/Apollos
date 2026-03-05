import os
import unittest
from typing import Any

try:
    from agent.aria_agent import AriaAgentOrchestrator
    from agent.session_manager import SessionStore
    from agent.websocket_handler import WebSocketRegistry
except ModuleNotFoundError:  # pragma: no cover - package-style fallback
    from backend.agent.aria_agent import AriaAgentOrchestrator
    from backend.agent.session_manager import SessionStore
    from backend.agent.websocket_handler import WebSocketRegistry


class FakeSocket:
    def __init__(self) -> None:
        self.messages: list[dict[str, Any]] = []

    async def send_json(self, payload: dict[str, Any]) -> None:
        self.messages.append(payload)


class AriaPayloadGuardTests(unittest.IsolatedAsyncioTestCase):
    KEYS = (
        'ENABLE_GEMINI_LIVE',
        'MAX_AUDIO_CHUNK_B64_CHARS',
        'MAX_FRAME_B64_CHARS',
    )

    def setUp(self) -> None:
        self._original = {key: os.environ.get(key) for key in self.KEYS}
        os.environ['ENABLE_GEMINI_LIVE'] = '0'
        os.environ['MAX_AUDIO_CHUNK_B64_CHARS'] = '32'
        os.environ['MAX_FRAME_B64_CHARS'] = '64'

    def tearDown(self) -> None:
        for key in self.KEYS:
            value = self._original.get(key)
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value

    async def test_audio_payload_guard_rejects_oversized_chunk(self) -> None:
        store = SessionStore(use_firestore=False)
        registry = WebSocketRegistry()
        orchestrator = AriaAgentOrchestrator(session_store=store, websocket_registry=registry)
        live = FakeSocket()
        await registry.register_live('guard-audio', live)

        await orchestrator.handle_client_message(
            'guard-audio',
            {
                'type': 'audio_chunk',
                'session_id': 'guard-audio',
                'timestamp': '2026-03-05T00:00:00Z',
                'audio_chunk_pcm16': 'A' * 1024,
            },
        )

        assistant_texts = [msg for msg in live.messages if msg.get('type') == 'assistant_text']
        self.assertTrue(assistant_texts)
        self.assertIn('Audio chunk dropped', str(assistant_texts[-1].get('text', '')))

    async def test_frame_observability_emits_safety_state(self) -> None:
        store = SessionStore(use_firestore=False)
        registry = WebSocketRegistry()
        orchestrator = AriaAgentOrchestrator(session_store=store, websocket_registry=registry)
        live = FakeSocket()
        await registry.register_live('guard-frame', live)

        await orchestrator.handle_client_message(
            'guard-frame',
            {
                'type': 'multimodal_frame',
                'session_id': 'guard-frame',
                'timestamp': '2026-03-05T00:00:00Z',
                'frame_jpeg_base64': 'A' * 40,
                'motion_state': 'walking_fast',
                'pitch': 9.0,
                'velocity': 1.2,
                'sensor_health': {
                    'score': 0.3,
                    'flags': ['depth_error'],
                    'degraded': True,
                    'source': 'test',
                },
                'location_accuracy_m': 80,
                'location_age_ms': 5000,
            },
        )

        safety_messages = [msg for msg in live.messages if msg.get('type') == 'safety_state']
        self.assertTrue(safety_messages)
        self.assertTrue(bool(safety_messages[-1].get('degraded')))


if __name__ == '__main__':
    unittest.main()
