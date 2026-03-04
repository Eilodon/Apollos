from __future__ import annotations

import asyncio
import os
from datetime import datetime, timezone
from typing import Any

from .live_bridge import GeminiLiveBridge
from .prompts import SYSTEM_PROMPT
from .run_config import build_run_config
from .session_manager import SessionStore
from .tools.context_manager import get_context_summary
from .tools.emotion_logger import log_emotion_event
from .tools.hazard_logger import log_hazard_event
from .tools.human_help import request_human_help
from .tools.mode_switcher import set_navigation_mode
from .tools.runtime import configure_runtime, reset_current_session, set_current_session
from .types import MotionState
from .websocket_handler import WebSocketRegistry


class AriaAgent:
    """Session orchestrator with Gemini Live bridge and safe local fallbacks."""

    def __init__(self, session_store: SessionStore, websocket_registry: WebSocketRegistry) -> None:
        self._session_store = session_store
        self._websocket_registry = websocket_registry
        self._run_config = build_run_config()

        self._enable_live = os.getenv('ENABLE_GEMINI_LIVE', '1') == '1'
        self._bridges: dict[str, GeminiLiveBridge] = {}
        self._bridges_lock = asyncio.Lock()

        self._frame_counter: dict[str, int] = {}

        configure_runtime(session_store=session_store, websocket_registry=websocket_registry)

    @property
    def system_prompt(self) -> str:
        return SYSTEM_PROMPT

    @property
    def run_config(self) -> Any:
        return self._run_config

    async def handle_client_message(self, session_id: str, payload: dict[str, Any]) -> None:
        message_type = payload.get('type', '')

        token = set_current_session(session_id)
        try:
            if message_type == 'multimodal_frame':
                await self._handle_multimodal_frame(session_id, payload)
                return

            if message_type == 'audio_chunk':
                await self._handle_audio_chunk(session_id, payload)
                return

            if message_type == 'user_command':
                command = str(payload.get('command', '')).strip()
                await self._handle_user_command(session_id, command)
                return

            await self._send_assistant_text(session_id, 'Unsupported message type.')
        finally:
            reset_current_session(token)

    async def close_session(self, session_id: str) -> None:
        bridge: GeminiLiveBridge | None
        async with self._bridges_lock:
            bridge = self._bridges.pop(session_id, None)

        if bridge is not None:
            await bridge.close()

    async def close_all_sessions(self) -> None:
        async with self._bridges_lock:
            items = list(self._bridges.items())
            self._bridges.clear()

        for _, bridge in items:
            await bridge.close()

    async def _handle_multimodal_frame(self, session_id: str, payload: dict[str, Any]) -> None:
        motion_state = self._normalize_motion_state(str(payload.get('motion_state', 'stationary')))
        await self._session_store.touch_session(session_id, motion_state=motion_state)

        if payload.get('dev_hazard'):
            await log_hazard_event(
                hazard_type='demo_obstacle',
                position_x=float(payload.get('dev_position_x', 0.0)),
                distance_category=payload.get('dev_distance', 'very_close'),
                confidence=float(payload.get('dev_confidence', 0.9)),
                description='Developer-triggered hazard event.',
                session_id=session_id,
            )
            return

        bridge = await self._get_or_create_bridge(session_id)
        if bridge is not None:
            await bridge.send_multimodal_frame(payload)
            return

        # Local fallback behavior when Gemini Live is disabled/unavailable.
        count = self._frame_counter.get(session_id, 0) + 1
        self._frame_counter[session_id] = count

        if count % 10 == 0:
            context = await get_context_summary()
            await self._send_assistant_text(session_id, f'ARIA local check-in: {context}')

        if motion_state in {'walking_fast', 'running'} and count % 8 == 0:
            await log_emotion_event(state='focused', confidence=0.72)

    async def _handle_audio_chunk(self, session_id: str, payload: dict[str, Any]) -> None:
        await self._session_store.touch_session(session_id)

        bridge = await self._get_or_create_bridge(session_id)
        if bridge is None:
            return

        chunk = str(payload.get('audio_chunk_pcm16', '') or '').strip()
        await bridge.send_audio_chunk(chunk)

    async def _handle_user_command(self, session_id: str, command: str) -> None:
        normalized = command.strip().lower()

        if normalized.startswith('set_navigation_mode:'):
            mode = command.split(':', maxsplit=1)[1].strip().upper()
            result = await set_navigation_mode(mode)
            await self._send_assistant_text(session_id, result)

            bridge = await self._get_or_create_bridge(session_id)
            if bridge is not None:
                await bridge.send_text(f'Mode switched to {mode}.', turn_complete=False)
            return

        if normalized == 'request_human_help':
            link = await request_human_help()
            await self._send_assistant_text(session_id, f'Human help link ready: {link}')
            return

        if normalized == 'describe_detailed':
            summary = await get_context_summary()
            await self._send_assistant_text(session_id, f'Detailed scene summary: {summary}')

            bridge = await self._get_or_create_bridge(session_id)
            if bridge is not None:
                await bridge.send_text('Please describe the current scene in detail.', turn_complete=True)
            return

        if normalized == 'sos':
            await log_hazard_event(
                hazard_type='manual_sos',
                position_x=0.0,
                distance_category='very_close',
                confidence=1.0,
                description='User-triggered SOS via shake gesture.',
                session_id=session_id,
            )
            await self._send_assistant_text(session_id, 'SOS received. Stay still. Help workflow triggered.')
            return

        bridge = await self._get_or_create_bridge(session_id)
        if bridge is not None:
            await bridge.send_text(command, turn_complete=True)
            return

        await self._send_assistant_text(session_id, f'Command received: {command}')

    async def _dispatch_tool_call(self, session_id: str, name: str, args: dict[str, Any]) -> dict[str, Any]:
        token = set_current_session(session_id)
        try:
            if name == 'log_hazard_event':
                hazard_type = str(args.get('hazard_type', 'unknown_hazard'))
                position_x = float(args.get('position_x', 0.0) or 0.0)
                distance_category = str(args.get('distance_category', 'mid'))
                confidence = float(args.get('confidence', 0.0) or 0.0)
                description = str(args.get('description', ''))
                sid = str(args.get('session_id', session_id) or session_id)

                message = await log_hazard_event(
                    hazard_type=hazard_type,
                    position_x=position_x,
                    distance_category=distance_category,
                    confidence=confidence,
                    description=description,
                    session_id=sid,
                )
                return {
                    'ok': True,
                    'message': message,
                    'position_x': max(-1.0, min(1.0, position_x)),
                    'distance_category': distance_category,
                }

            if name == 'set_navigation_mode':
                mode = str(args.get('mode', 'NAVIGATION')).upper()
                message = await set_navigation_mode(mode)
                return {'ok': True, 'message': message, 'mode': mode}

            if name == 'log_emotion_event':
                state = str(args.get('state', 'unknown'))
                confidence = float(args.get('confidence', 0.0) or 0.0)
                message = await log_emotion_event(state=state, confidence=confidence)
                return {'ok': True, 'message': message, 'state': state}

            if name == 'get_context_summary':
                summary = await get_context_summary()
                return {'ok': True, 'summary': summary}

            if name == 'request_human_help':
                link = await request_human_help()
                return {'ok': True, 'link': link}

            return {'ok': False, 'error': f'Unknown tool: {name}'}
        except Exception as exc:
            return {'ok': False, 'error': str(exc), 'tool': name}
        finally:
            reset_current_session(token)

    async def _get_or_create_bridge(self, session_id: str) -> GeminiLiveBridge | None:
        if not self._enable_live:
            return None

        async with self._bridges_lock:
            bridge = self._bridges.get(session_id)
            if bridge is not None:
                return bridge

            bridge = GeminiLiveBridge(
                session_id=session_id,
                session_store=self._session_store,
                websocket_registry=self._websocket_registry,
                tool_dispatcher=lambda name, args: self._dispatch_tool_call(session_id, name, args),
            )
            self._bridges[session_id] = bridge
            return bridge

    async def _send_assistant_text(self, session_id: str, text: str) -> None:
        await self._websocket_registry.send_live(
            session_id,
            {
                'type': 'assistant_text',
                'session_id': session_id,
                'timestamp': self._now(),
                'text': text,
            },
        )

    @staticmethod
    def _normalize_motion_state(raw: str) -> MotionState:
        valid = {'stationary', 'walking_slow', 'walking_fast', 'running'}
        return raw if raw in valid else 'stationary'

    @staticmethod
    def _now() -> str:
        return datetime.now(timezone.utc).isoformat()
