from __future__ import annotations

import asyncio
import inspect
import logging
import os
from datetime import datetime, timezone
from typing import Any, Callable

from .live_bridge import GeminiLiveBridge
from .session_manager import SessionStore
from .tools.context_manager import get_context_summary
from .tools.emotion_escalator import escalate_mode_if_stressed
from .tools.emotion_logger import log_emotion_event
from .tools.hazard_logger import log_hazard_event
from .tools.human_help import request_human_help
from .tools.location_intel import identify_location
from .tools.mode_switcher import set_navigation_mode
from .tools.runtime import configure_runtime, reset_current_session, set_current_session
from .websocket_handler import WebSocketRegistry

logger = logging.getLogger(__name__)

ToolResult = dict[str, Any]
ToolFn = Callable[..., Any]

_DISTANCE_VALUES = {'very_close', 'mid', 'far'}


def _env_flag(name: str, default: bool) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.strip().lower() not in {'0', 'false', 'off', 'no'}


class AriaAgentOrchestrator:
    """Routes client websocket payloads into Gemini Live and local tool runtime."""

    def __init__(self, session_store: SessionStore, websocket_registry: WebSocketRegistry) -> None:
        self._session_store = session_store
        self._websocket_registry = websocket_registry
        self._live_enabled = _env_flag('ENABLE_GEMINI_LIVE', True)

        self._bridges: dict[str, GeminiLiveBridge] = {}
        self._bridge_lock = asyncio.Lock()

        self._tools: dict[str, ToolFn] = {
            'log_hazard_event': log_hazard_event,
            'set_navigation_mode': set_navigation_mode,
            'log_emotion_event': log_emotion_event,
            'escalate_mode_if_stressed': escalate_mode_if_stressed,
            'identify_location': identify_location,
            'get_context_summary': get_context_summary,
            'request_human_help': request_human_help,
        }

        configure_runtime(session_store=session_store, websocket_registry=websocket_registry)

    @property
    def live_enabled(self) -> bool:
        return self._live_enabled

    async def handle_client_message(self, session_id: str, payload: dict[str, Any]) -> None:
        message_type = str(payload.get('type', '')).strip()
        if not message_type:
            return

        if message_type == 'heartbeat':
            await self._websocket_registry.send_live(
                session_id,
                {
                    'type': 'heartbeat_ack',
                    'session_id': session_id,
                    'timestamp': self._now(),
                },
            )
            return

        motion_state = str(payload.get('motion_state', 'stationary'))
        lat = payload.get('lat')
        lng = payload.get('lng')
        heading_deg = payload.get('heading_deg')
        await self._session_store.touch_session(
            session_id,
            motion_state=motion_state,
            lat=float(lat) if isinstance(lat, (int, float)) else None,
            lng=float(lng) if isinstance(lng, (int, float)) else None,
            heading_deg=float(heading_deg) if isinstance(heading_deg, (int, float)) else None,
        )

        if message_type == 'multimodal_frame':
            await self._handle_multimodal_frame(session_id, payload)
            return

        if message_type == 'audio_chunk':
            await self._handle_audio_chunk(session_id, payload)
            return

        if message_type == 'user_command':
            await self._handle_user_command(session_id, payload)
            return

        await self._send_assistant_text(session_id, f'Unsupported message type: {message_type}')

    async def handle_emergency_message(self, session_id: str, payload: dict[str, Any]) -> None:
        message_type = str(payload.get('type', '')).strip()
        if message_type != 'EDGE_HAZARD':
            return

        hazard_type = str(payload.get('hazard_type', 'EDGE_REFLEX') or 'EDGE_REFLEX')
        suppress_seconds_raw = payload.get('suppress_seconds')
        try:
            suppress_seconds = float(suppress_seconds_raw) if suppress_seconds_raw is not None else 2.5
        except (TypeError, ValueError):
            suppress_seconds = 2.5
        suppress_seconds = min(5.0, max(0.5, suppress_seconds))

        await self._session_store.mark_edge_hazard(
            session_id=session_id,
            hazard_type=hazard_type,
            suppress_seconds=suppress_seconds,
        )

        async with self._bridge_lock:
            bridge = self._bridges.get(session_id)
        if bridge is not None:
            bridge.note_edge_hazard(hazard_type)

    async def dispatch_tool_call(self, name: str, args: dict[str, Any], session_id: str) -> ToolResult:
        tool = self._tools.get(name)
        if tool is None:
            return {'ok': False, 'tool': name, 'error': f'Unknown tool: {name}'}

        prepared = dict(args)
        if name == 'log_hazard_event':
            if 'distance_category' not in prepared and 'distance' in prepared:
                prepared['distance_category'] = prepared.pop('distance')
            prepared.setdefault('distance_category', 'mid')
            prepared.setdefault('session_id', session_id)
        if name == 'escalate_mode_if_stressed':
            prepared.setdefault('current_mode', await self._session_store.get_effective_mode(session_id))

        token = set_current_session(session_id)
        try:
            kwargs = self._filter_tool_kwargs(tool, prepared)
            raw_result = tool(**kwargs)
            if inspect.isawaitable(raw_result):
                raw_result = await raw_result

            normalized = self._normalize_tool_result(name, prepared, raw_result)
            normalized.setdefault('ok', True)
            normalized.setdefault('tool', name)
            return normalized
        except Exception as exc:
            logger.exception('Tool dispatch failed (session=%s, tool=%s): %s', session_id, name, exc)
            return {'ok': False, 'tool': name, 'error': str(exc)}
        finally:
            reset_current_session(token)

    async def close_session(self, session_id: str) -> None:
        async with self._bridge_lock:
            bridge = self._bridges.pop(session_id, None)
        if bridge is not None:
            await bridge.close()

    async def shutdown(self) -> None:
        async with self._bridge_lock:
            bridges = list(self._bridges.values())
            self._bridges.clear()
        if not bridges:
            return
        await asyncio.gather(*(bridge.close() for bridge in bridges), return_exceptions=True)

    async def stats(self) -> dict[str, Any]:
        async with self._bridge_lock:
            bridge_count = len(self._bridges)
        return {
            'live_enabled': self._live_enabled,
            'active_live_sessions': bridge_count,
        }

    async def _handle_multimodal_frame(self, session_id: str, payload: dict[str, Any]) -> None:
        if not self._live_enabled:
            return

        bridge = await self._ensure_bridge(session_id)
        await bridge.send_multimodal_frame(payload)

        user_text = str(payload.get('user_text', '') or '').strip()
        if user_text:
            await bridge.send_text(user_text, turn_complete=True)

    async def _handle_audio_chunk(self, session_id: str, payload: dict[str, Any]) -> None:
        if not self._live_enabled:
            return

        audio_chunk = str(payload.get('audio_chunk_pcm16', '') or '').strip()
        if not audio_chunk:
            return

        bridge = await self._ensure_bridge(session_id)
        await bridge.send_audio_chunk(audio_chunk)

    async def _handle_user_command(self, session_id: str, payload: dict[str, Any]) -> None:
        command = str(payload.get('command', '') or '').strip()
        if not command:
            return

        if await self._handle_local_command(session_id, command):
            return

        if not self._live_enabled:
            await self._send_assistant_text(session_id, 'Live model disabled. Command recorded locally.')
            return

        bridge = await self._ensure_bridge(session_id)
        await bridge.send_text(command, turn_complete=True)

    async def _handle_local_command(self, session_id: str, command: str) -> bool:
        normalized = command.strip()
        lower = normalized.lower()

        if lower.startswith('set_navigation_mode'):
            mode = self._extract_mode_from_command(normalized)
            if not mode:
                await self._send_assistant_text(session_id, 'Mode command missing target mode.')
                return True

            result = await self.dispatch_tool_call('set_navigation_mode', {'mode': mode}, session_id)
            await self._send_assistant_text(session_id, str(result.get('message') or result.get('value') or f'Mode switched to {mode}'))
            return True

        if lower == 'request_human_help':
            result = await self.dispatch_tool_call('request_human_help', {}, session_id)
            help_link = str(result.get('value') or result.get('message') or '')
            if help_link:
                await self._send_assistant_text(session_id, f'Human help requested: {help_link}')
            else:
                await self._send_assistant_text(session_id, 'Human help requested.')
            return True

        if lower == 'get_context_summary':
            result = await self.dispatch_tool_call('get_context_summary', {}, session_id)
            summary = str(result.get('value') or result.get('message') or '')
            await self._send_assistant_text(session_id, summary or 'No context summary available yet.')
            return True

        if lower in {'describe_detailed', 'describe_detail', 'describe'}:
            summary = await self._session_store.get_context_summary(session_id)
            await self._send_assistant_text(session_id, f'Context snapshot: {summary}')
            return True

        if lower == 'sos':
            result = await self.dispatch_tool_call('request_human_help', {}, session_id)
            help_link = str(result.get('value') or result.get('message') or '')
            await self._send_assistant_text(
                session_id,
                f'SOS acknowledged. Human help link: {help_link}' if help_link else 'SOS acknowledged. Human help requested.',
            )
            return True

        return False

    async def _ensure_bridge(self, session_id: str) -> GeminiLiveBridge:
        async with self._bridge_lock:
            bridge = self._bridges.get(session_id)
            if bridge is not None:
                return bridge

            async def tool_dispatcher(name: str, args: dict[str, Any]) -> ToolResult:
                return await self.dispatch_tool_call(name, args, session_id)

            bridge = GeminiLiveBridge(
                session_id=session_id,
                session_store=self._session_store,
                websocket_registry=self._websocket_registry,
                tool_dispatcher=tool_dispatcher,
            )
            self._bridges[session_id] = bridge
            return bridge

    async def _send_assistant_text(self, session_id: str, text: str) -> None:
        if not text.strip():
            return
        await self._websocket_registry.send_live(
            session_id,
            {
                'type': 'assistant_text',
                'session_id': session_id,
                'timestamp': self._now(),
                'text': text.strip(),
            },
        )

    @staticmethod
    def _filter_tool_kwargs(tool: ToolFn, args: dict[str, Any]) -> dict[str, Any]:
        signature = inspect.signature(tool)
        kwargs: dict[str, Any] = {}
        for name in signature.parameters:
            if name in args:
                kwargs[name] = args[name]
        return kwargs

    @staticmethod
    def _normalize_tool_result(name: str, args: dict[str, Any], raw_result: Any) -> ToolResult:
        normalized: ToolResult = {}

        if isinstance(raw_result, dict):
            normalized.update(raw_result)
        elif isinstance(raw_result, str):
            normalized['message'] = raw_result
            normalized['value'] = raw_result
        elif raw_result is not None:
            normalized['value'] = raw_result

        if name == 'log_hazard_event':
            position_x = float(args.get('position_x', 0.0) or 0.0)
            distance = str(args.get('distance_category', 'mid') or 'mid')
            if distance not in _DISTANCE_VALUES:
                distance = 'mid'

            normalized.setdefault('position_x', max(-1.0, min(1.0, position_x)))
            normalized.setdefault('distance', distance)
            normalized.setdefault('hazard_type', str(args.get('hazard_type', 'unknown') or 'unknown'))
            normalized.setdefault('confidence', float(args.get('confidence', 0.0) or 0.0))

        return normalized

    @staticmethod
    def _extract_mode_from_command(command: str) -> str:
        if ':' in command:
            _, mode = command.split(':', 1)
            return mode.strip().upper()
        if '=' in command:
            _, mode = command.split('=', 1)
            return mode.strip().upper()

        parts = command.split(maxsplit=1)
        if len(parts) == 2:
            return parts[1].strip().upper()
        return ''

    @staticmethod
    def _now() -> str:
        return datetime.now(timezone.utc).isoformat()
