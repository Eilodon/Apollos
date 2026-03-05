from __future__ import annotations

import asyncio
import inspect
import logging
import os
import time
from datetime import datetime, timezone
from typing import Any, Callable

from .live_bridge import GeminiLiveBridge
from .session_manager import SessionStore
from .human_fallback import HumanFallbackManager
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

    def __init__(
        self,
        session_store: SessionStore,
        websocket_registry: WebSocketRegistry,
        human_fallback_manager: HumanFallbackManager | None = None,
    ) -> None:
        self._session_store = session_store
        self._websocket_registry = websocket_registry
        self._human_fallback_manager = human_fallback_manager
        self._live_enabled = _env_flag('ENABLE_GEMINI_LIVE', True)

        self._bridges: dict[str, GeminiLiveBridge] = {}
        self._bridge_lock = asyncio.Lock()
        self._last_safety_state_signature: dict[str, str] = {}
        self._last_guard_notice_at: dict[str, float] = {}
        self._last_help_frame_sent_ms: dict[str, int] = {}
        self._max_frame_b64_chars = max(1_024, int(os.getenv('MAX_FRAME_B64_CHARS', '1800000') or 1800000))
        self._max_audio_chunk_b64_chars = max(256, int(os.getenv('MAX_AUDIO_CHUNK_B64_CHARS', '240000') or 240000))
        self._max_user_command_chars = max(32, int(os.getenv('MAX_USER_COMMAND_CHARS', '400') or 400))
        self._max_sensor_health_flags = max(1, int(os.getenv('MAX_SENSOR_HEALTH_FLAGS', '8') or 8))
        self._help_frame_min_interval_ms = max(120, int(os.getenv('HELP_FRAME_MIN_INTERVAL_MS', '350') or 350))

        self._tools: dict[str, ToolFn] = {
            'log_hazard_event': log_hazard_event,
            'set_navigation_mode': set_navigation_mode,
            'log_emotion_event': log_emotion_event,
            'escalate_mode_if_stressed': escalate_mode_if_stressed,
            'identify_location': identify_location,
            'get_context_summary': get_context_summary,
            'request_human_help': request_human_help,
        }

        self._configure_tool_runtime()

    def set_human_fallback_manager(self, manager: HumanFallbackManager | None) -> None:
        self._human_fallback_manager = manager
        self._configure_tool_runtime()

    def _configure_tool_runtime(self) -> None:
        configure_runtime(
            session_store=self._session_store,
            websocket_registry=self._websocket_registry,
            human_fallback_manager=self._human_fallback_manager,
        )

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
        self._last_safety_state_signature.pop(session_id, None)
        self._last_help_frame_sent_ms.pop(session_id, None)
        guard_prefix = f'{session_id}:'
        for key in list(self._last_guard_notice_at):
            if key.startswith(guard_prefix):
                self._last_guard_notice_at.pop(key, None)

    async def shutdown(self) -> None:
        async with self._bridge_lock:
            bridges = list(self._bridges.values())
            self._bridges.clear()
        if not bridges:
            return
        await asyncio.gather(*(bridge.close() for bridge in bridges), return_exceptions=True)
        self._last_safety_state_signature.clear()
        self._last_guard_notice_at.clear()

    async def stats(self) -> dict[str, Any]:
        async with self._bridge_lock:
            bridge_count = len(self._bridges)
        return {
            'live_enabled': self._live_enabled,
            'active_live_sessions': bridge_count,
        }

    async def _handle_multimodal_frame(self, session_id: str, payload: dict[str, Any]) -> None:
        frame_b64 = str(payload.get('frame_jpeg_base64', '') or '').strip()
        if frame_b64 and len(frame_b64) > self._max_frame_b64_chars:
            await self._emit_guard_warning(
                session_id,
                key='frame_too_large',
                message='Input frame dropped: payload exceeds safety size limits.',
            )
            return

        sensor_health_score, sensor_health_flags = self._extract_sensor_health(payload)
        localization_uncertainty_m = self._extract_localization_uncertainty(payload)
        observability = await self._session_store.update_observability(
            session_id,
            sensor_health_score=sensor_health_score,
            sensor_health_flags=sensor_health_flags,
            localization_uncertainty_m=localization_uncertainty_m,
        )
        await self._emit_safety_state_if_changed(session_id, observability)
        await self._forward_help_frame(session_id, payload)

        if not self._live_enabled:
            return

        bridge = await self._ensure_bridge(session_id)
        await bridge.send_multimodal_frame(payload)

        user_text = str(payload.get('user_text', '') or '').strip()
        if user_text:
            await bridge.send_text(user_text, turn_complete=True)

    async def _handle_audio_chunk(self, session_id: str, payload: dict[str, Any]) -> None:
        audio_chunk = str(payload.get('audio_chunk_pcm16', '') or '').strip()
        if not audio_chunk:
            return
        if len(audio_chunk) > self._max_audio_chunk_b64_chars:
            await self._emit_guard_warning(
                session_id,
                key='audio_too_large',
                message='Audio chunk dropped: payload exceeds safety size limits.',
            )
            return
        await self._forward_help_audio(session_id, audio_chunk=audio_chunk, timestamp=payload.get('timestamp'))
        if not self._live_enabled:
            return

        bridge = await self._ensure_bridge(session_id)
        await bridge.send_audio_chunk(audio_chunk)

    async def _handle_user_command(self, session_id: str, payload: dict[str, Any]) -> None:
        command = str(payload.get('command', '') or '').strip()
        if not command:
            return
        if len(command) > self._max_user_command_chars:
            await self._emit_guard_warning(
                session_id,
                key='command_too_large',
                message='Command rejected: too long for safe realtime handling.',
            )
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
            help_link = str(result.get('help_link') or result.get('value') or result.get('message') or '')
            rtc = result.get('rtc')
            if isinstance(rtc, dict):
                await self._emit_human_help_session(session_id, rtc=rtc, help_link=help_link)
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

        if lower in {'sos', 'cứu tôi', 'cuu toi', 'help me', 'emergency'}:
            result = await self.dispatch_tool_call('request_human_help', {}, session_id)
            help_link = str(result.get('help_link') or result.get('value') or result.get('message') or '')
            rtc = result.get('rtc')
            if isinstance(rtc, dict):
                await self._emit_human_help_session(session_id, rtc=rtc, help_link=help_link)
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

    async def _emit_human_help_session(self, session_id: str, rtc: dict[str, Any], help_link: str) -> None:
        provider = str(rtc.get('provider', '')).strip().lower()
        token = str(rtc.get('token', '')).strip()
        room_name = str(rtc.get('room_name', '')).strip()
        if provider not in {'twilio', 'livekit'} or not token or not room_name:
            return
        await self._websocket_registry.send_live(
            session_id,
            {
                'type': 'human_help_session',
                'session_id': session_id,
                'timestamp': self._now(),
                'help_link': help_link,
                'rtc': {
                    'provider': provider,
                    'room_name': room_name,
                    'identity': str(rtc.get('identity', '')).strip(),
                    'token': token,
                    'expires_in': int(rtc.get('expires_in', 0) or 0),
                },
            },
        )

    async def _emit_guard_warning(self, session_id: str, key: str, message: str, cooldown_s: float = 10.0) -> None:
        now = time.monotonic()
        bucket = f'{session_id}:{key}'
        previous = self._last_guard_notice_at.get(bucket, 0.0)
        if now - previous < cooldown_s:
            return
        self._last_guard_notice_at[bucket] = now
        await self._send_assistant_text(session_id, message)

    def _extract_sensor_health(self, payload: dict[str, Any]) -> tuple[float, list[str]]:
        score = 0.9
        flags: list[str] = []

        raw = payload.get('sensor_health')
        if isinstance(raw, dict):
            raw_score = raw.get('score')
            if isinstance(raw_score, (int, float)):
                score = max(0.0, min(1.0, float(raw_score)))
            raw_flags = raw.get('flags')
            if isinstance(raw_flags, list):
                for item in raw_flags[: self._max_sensor_health_flags]:
                    normalized = str(item).strip().lower().replace(' ', '_')
                    if normalized and normalized not in flags:
                        flags.append(normalized)

        if bool(payload.get('sensor_unavailable', False)) and 'sensor_unavailable' not in flags:
            flags.append('sensor_unavailable')
            score = min(score, 0.45)

        return score, flags

    async def _forward_help_frame(self, session_id: str, payload: dict[str, Any]) -> None:
        frame_b64 = str(payload.get('frame_jpeg_base64', '') or '').strip()
        if not frame_b64:
            return

        now_ms = int(time.time() * 1000)
        last_sent = self._last_help_frame_sent_ms.get(session_id, 0)
        if now_ms - last_sent < self._help_frame_min_interval_ms:
            return
        self._last_help_frame_sent_ms[session_id] = now_ms

        await self._websocket_registry.send_help(
            session_id,
            {
                'type': 'help_frame',
                'session_id': session_id,
                'timestamp': str(payload.get('timestamp') or self._now()),
                'frame_jpeg_base64': frame_b64,
                'motion_state': str(payload.get('motion_state', '') or ''),
                'carry_mode': str(payload.get('carry_mode', '') or ''),
            },
        )

    async def _forward_help_audio(self, session_id: str, audio_chunk: str, timestamp: Any) -> None:
        await self._websocket_registry.send_help(
            session_id,
            {
                'type': 'help_audio',
                'session_id': session_id,
                'timestamp': str(timestamp or self._now()),
                'audio_chunk_pcm16': audio_chunk,
                'sample_rate_hz': 16000,
            },
        )

    @staticmethod
    def _extract_localization_uncertainty(payload: dict[str, Any]) -> float:
        raw_accuracy = payload.get('location_accuracy_m')
        if isinstance(raw_accuracy, (int, float)) and raw_accuracy >= 0:
            uncertainty = float(raw_accuracy)
        elif isinstance(payload.get('lat'), (int, float)) and isinstance(payload.get('lng'), (int, float)):
            uncertainty = 30.0
        else:
            uncertainty = 120.0

        raw_age_ms = payload.get('location_age_ms')
        if isinstance(raw_age_ms, (int, float)) and raw_age_ms > 0:
            uncertainty += min(90.0, float(raw_age_ms) / 1000.0)

        return max(0.0, min(500.0, uncertainty))

    async def _emit_safety_state_if_changed(self, session_id: str, observability: dict[str, Any]) -> None:
        degraded_mode = bool(observability.get('degraded_mode', False))
        degraded_reason = str(observability.get('degraded_reason', '') or '')
        sensor_health_score = float(observability.get('sensor_health_score', 1.0) or 1.0)
        localization_uncertainty_m = float(observability.get('localization_uncertainty_m', 120.0) or 120.0)
        last_safety_tier = str(observability.get('last_safety_tier', 'silent') or 'silent')
        sensor_health_flags = observability.get('sensor_health_flags', [])

        signature = (
            f'{int(degraded_mode)}|{round(sensor_health_score, 2)}|'
            f'{round(localization_uncertainty_m, 1)}|{last_safety_tier}|{degraded_reason}'
        )
        if self._last_safety_state_signature.get(session_id) == signature:
            return
        self._last_safety_state_signature[session_id] = signature

        await self._websocket_registry.send_live(
            session_id,
            {
                'type': 'safety_state',
                'session_id': session_id,
                'timestamp': self._now(),
                'degraded': degraded_mode,
                'reason': degraded_reason,
                'sensor_health_score': sensor_health_score,
                'sensor_health_flags': sensor_health_flags if isinstance(sensor_health_flags, list) else [],
                'localization_uncertainty_m': localization_uncertainty_m,
                'tier': last_safety_tier,
            },
        )

        if degraded_mode and bool(observability.get('degraded_changed', False)):
            await self._send_assistant_text(
                session_id,
                'Degraded-safe mode active. Please slow down and verify surroundings.',
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
