from __future__ import annotations

import asyncio
import base64
import json
import logging
import os
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any, Awaitable, Callable

from .prompts import SYSTEM_PROMPT
from .session_manager import SessionStore
from .websocket_handler import WebSocketRegistry

logger = logging.getLogger(__name__)

ToolDispatcher = Callable[[str, dict[str, Any]], Awaitable[dict[str, Any]]]


@dataclass(slots=True)
class LiveConnectResult:
    model: str
    config_name: str


class GeminiLiveBridge:
    """Session-scoped bridge between local websocket protocol and Gemini Live API."""

    def __init__(
        self,
        session_id: str,
        session_store: SessionStore,
        websocket_registry: WebSocketRegistry,
        tool_dispatcher: ToolDispatcher,
    ) -> None:
        self._session_id = session_id
        self._session_store = session_store
        self._websocket_registry = websocket_registry
        self._tool_dispatcher = tool_dispatcher

        self._client: Any = None
        self._types: Any = None

        self._session_cm: Any = None
        self._session: Any = None

        self._receive_task: asyncio.Task[None] | None = None
        self._start_lock = asyncio.Lock()
        self._send_lock = asyncio.Lock()

        self._connected = False
        self._stopped = False
        self._consecutive_send_failures = 0

        self._last_hazard_position = 0.0
        self._last_motion_text = ''
        self._last_motion_state = 'stationary'
        self._consecutive_hazard_frames = 0
        self._last_hazard_ts = 0.0
        self._hazard_confirmation_frames = max(1, int(os.getenv('HAZARD_CONFIRMATION_FRAMES', '1')))
        self._hazard_confirmation_timeout_s = max(0.5, float(os.getenv('HAZARD_CONFIRMATION_TIMEOUT_S', '3.0')))
        self._last_pitch = 0.0
        self._last_velocity = 0.0
        self._last_yaw_delta = 0.0
        self._last_heading_deg: float | None = None
        self._last_voice_emit_ts = 0.0
        self._last_location_context = ''
        self._edge_suppress_until = 0.0
        self._edge_suppress_window_s = max(0.5, float(os.getenv('EDGE_HAZARD_SUPPRESS_SECONDS', '2.5')))

    def note_edge_hazard(self, hazard_type: str = 'EDGE_REFLEX') -> None:
        self._edge_suppress_until = time.monotonic() + self._edge_suppress_window_s
        logger.info(
            'Edge hazard suppress active (session=%s, hazard=%s, window=%.2fs)',
            self._session_id,
            hazard_type,
            self._edge_suppress_window_s,
        )

    async def ensure_started(self) -> bool:
        if self._stopped:
            return False

        if self._connected and self._session is not None:
            return True

        async with self._start_lock:
            if self._connected and self._session is not None:
                return True

            try:
                result = await self._start_session()
            except Exception as exc:
                logger.exception('Gemini live session start failed for %s: %s', self._session_id, exc)
                await self._websocket_registry.send_live(
                    self._session_id,
                    {
                        'type': 'connection_state',
                        'state': 'reconnecting',
                        'detail': f'Gemini Live unavailable: {exc}',
                    },
                )
                return False

            await self._websocket_registry.send_live(
                self._session_id,
                {
                    'type': 'connection_state',
                    'state': 'connected',
                    'detail': f'Gemini Live connected ({result.model}, {result.config_name})',
                },
            )
            return True

    async def close(self, mark_stopped: bool = True) -> None:
        if mark_stopped:
            self._stopped = True
        self._connected = False

        receive_task = self._receive_task
        self._receive_task = None

        current = asyncio.current_task()
        if receive_task is not None and receive_task is not current:
            receive_task.cancel()
            try:
                await receive_task
            except asyncio.CancelledError:
                pass
            except Exception:
                logger.exception('Live receive task shutdown error for %s', self._session_id)

        session_cm = self._session_cm
        self._session = None
        self._session_cm = None

        if session_cm is not None:
            try:
                await session_cm.__aexit__(None, None, None)
            except Exception:
                logger.exception('Live session close error for %s', self._session_id)

    async def send_audio_chunk(self, pcm16_base64: str) -> None:
        if not pcm16_base64:
            return

        started = await self.ensure_started()
        if not started or self._session is None:
            return

        audio_bytes = base64.b64decode(pcm16_base64)
        audio_blob = self._build_blob(audio_bytes, mime_type='audio/pcm;rate=16000')

        async with self._send_lock:
            try:
                await self._session.send_realtime_input(audio=audio_blob)
                self._consecutive_send_failures = 0
            except Exception as exc:
                self._consecutive_send_failures += 1
                await self._on_send_failure('audio_chunk', exc)

    async def send_multimodal_frame(self, payload: dict[str, Any]) -> None:
        started = await self.ensure_started()
        if not started or self._session is None:
            return

        frame_b64 = str(payload.get('frame_jpeg_base64', '') or '').strip()
        motion_state = str(payload.get('motion_state', 'stationary'))
        self._last_motion_state = motion_state
        pitch = float(payload.get('pitch', 0.0) or 0.0)
        velocity = float(payload.get('velocity', 0.0) or 0.0)
        yaw_delta = float(payload.get('yaw_delta_deg', 0.0) or 0.0)
        lat = payload.get('lat')
        lng = payload.get('lng')
        heading_deg = payload.get('heading_deg')
        carry_mode = str(payload.get('carry_mode', '') or '').strip()
        sensor_unavailable = bool(payload.get('sensor_unavailable', False))
        heading_value = float(heading_deg) if isinstance(heading_deg, (float, int)) else self._last_heading_deg
        self._last_heading_deg = heading_value
        self._last_pitch = pitch
        self._last_velocity = velocity
        self._last_yaw_delta = yaw_delta

        # Semantic Odometry: bơm góc xoay tích lũy để Gemini suy luận vị trí vật cản
        odometry_hint = ''
        if abs(yaw_delta) > 5.0:
            direction = 'RIGHT' if yaw_delta > 0 else 'LEFT'
            odometry_hint = (
                f' [ODOMETRY: User rotated {abs(yaw_delta):.0f}-deg {direction} since last frame.'
                f' If a hazard was detected on the {direction.lower()} previously,'
                f' warn it may now be DIRECTLY AHEAD. Do not wait for next frame to confirm.]'
            )

        risk_score = self._compute_risk_multiplier(motion_state, pitch, velocity, yaw_delta)
        effective_mode = await self._session_store.get_effective_mode(self._session_id)
        spatial_context = await self._session_store.get_spatial_context(
            self._session_id,
            current_yaw=heading_value,
            current_lat=float(lat) if isinstance(lat, (int, float)) else None,
            current_lng=float(lng) if isinstance(lng, (int, float)) else None,
        )
        edge_reflex_active = await self._session_store.is_edge_hazard_active(self._session_id, now_epoch=time.time())
        location_hint = ''
        if isinstance(lat, (int, float)) and isinstance(lng, (int, float)):
            location_hint = f' [LOCATION: lat={float(lat):.5f}, lng={float(lng):.5f}, heading={heading_value or 0:.1f}deg]'
            now_epoch = time.time()
            should_lookup = await self._session_store.should_lookup_location(self._session_id, now_epoch=now_epoch, min_interval_s=30)
            if should_lookup:
                location_result = await self._tool_dispatcher(
                    'identify_location',
                    {
                        'lat': float(lat),
                        'lng': float(lng),
                        'heading_deg': float(heading_value or 0.0),
                    },
                )
                name = str(location_result.get('name', '')).strip()
                info = str(location_result.get('relevant_info', '')).strip()
                if name:
                    self._last_location_context = f"[LOCATION INTEL: {name}. {info}]"
            elif self._last_location_context:
                location_hint += f" {self._last_location_context}"

        motion_text = (
            f"[KINEMATIC: User is {motion_state}. Pitch: {pitch:.1f}deg. "
            f"Velocity: {velocity:.2f}. Mode: {effective_mode}. RiskScore: {risk_score:.2f}. "
            f"Treat visible hazards with safety-first urgency.]{odometry_hint}{spatial_context}{location_hint}"
        )
        if carry_mode:
            motion_text += f" [CARRY_MODE: {carry_mode}]"
        if sensor_unavailable:
            motion_text += ' [POCKET_SENSOR: unavailable; manual fallback active]'
        if edge_reflex_active:
            motion_text += ' [EDGE_HAZARD: local reflex already active. Defer voice; confirm/enrich only if needed.]'


        # Avoid repeating identical motion hints too aggressively.
        include_motion_text = motion_text != self._last_motion_text
        self._last_motion_text = motion_text

        if frame_b64:
            parts: list[dict[str, Any]] = [
                {'inline_data': {'mime_type': 'image/jpeg', 'data': frame_b64}},
            ]
            if include_motion_text:
                parts.append({'text': motion_text})

            async with self._send_lock:
                try:
                    await self._session.send_client_content(
                        turns={'role': 'user', 'parts': parts},
                        turn_complete=False,
                    )
                    self._consecutive_send_failures = 0
                    return
                except Exception as exc:
                    self._consecutive_send_failures += 1
                    await self._on_send_failure('frame_content', exc)

        if include_motion_text:
            await self.send_text(motion_text, turn_complete=False)

    async def send_text(self, text: str, turn_complete: bool = True) -> None:
        if not text.strip():
            return

        started = await self.ensure_started()
        if not started or self._session is None:
            return

        async with self._send_lock:
            try:
                await self._session.send_client_content(
                    turns={
                        'role': 'user',
                        'parts': [{'text': text.strip()}],
                    },
                    turn_complete=turn_complete,
                )
                self._consecutive_send_failures = 0
            except Exception as exc:
                self._consecutive_send_failures += 1
                await self._on_send_failure('send_text', exc)

    async def _start_session(self) -> LiveConnectResult:
        self._init_client()
        assert self._client is not None

        model_candidates = self._model_candidates()
        config_candidates = self._config_candidates()

        last_error: Exception | None = None

        for model_name in model_candidates:
            for config_name, config in config_candidates:
                try:
                    session_cm = self._client.aio.live.connect(model=model_name, config=config)
                    session = await session_cm.__aenter__()
                except Exception as exc:
                    last_error = exc
                    logger.warning(
                        'Live connect failed (session=%s, model=%s, config=%s): %s',
                        self._session_id,
                        model_name,
                        config_name,
                        exc,
                    )
                    continue

                self._session_cm = session_cm
                self._session = session
                self._connected = True
                self._stopped = False
                self._receive_task = asyncio.create_task(self._receive_loop(), name=f'live-recv-{self._session_id}')

                return LiveConnectResult(model=model_name, config_name=config_name)

        if last_error is None:
            raise RuntimeError('No Live API model candidate configured.')
        raise last_error

    def _init_client(self) -> None:
        if self._client is not None:
            return

        from google import genai  # type: ignore
        from google.genai import types  # type: ignore

        use_vertex = os.getenv('GEMINI_USE_VERTEX', '0') == '1'

        if use_vertex:
            project = os.getenv('GOOGLE_CLOUD_PROJECT', '').strip()
            location = os.getenv('GOOGLE_CLOUD_LOCATION', 'us-central1').strip()
            if not project:
                raise RuntimeError('GEMINI_USE_VERTEX=1 requires GOOGLE_CLOUD_PROJECT')
            self._client = genai.Client(vertexai=True, project=project, location=location)
        else:
            api_key = os.getenv('GOOGLE_API_KEY', '').strip() or os.getenv('GEMINI_API_KEY', '').strip()
            if not api_key:
                raise RuntimeError('Missing GOOGLE_API_KEY or GEMINI_API_KEY')
            self._client = genai.Client(api_key=api_key)

        self._types = types

    def _config_candidates(self) -> list[tuple[str, dict[str, Any]]]:
        function_declarations = [
            {
                'name': 'log_hazard_event',
                'description': 'Trigger immediate HARD_STOP and log detected hazard.',
                'parameters': {
                    'type': 'object',
                    'properties': {
                        'hazard_type': {'type': 'string'},
                        'position_x': {'type': 'number', 'minimum': -1.0, 'maximum': 1.0},
                        'distance_category': {
                            'type': 'string',
                            'enum': ['very_close', 'mid', 'far'],
                        },
                        'confidence': {'type': 'number', 'minimum': 0.0, 'maximum': 1.0},
                        'description': {'type': 'string'},
                        'session_id': {'type': 'string'},
                    },
                    'required': [
                        'hazard_type',
                        'position_x',
                        'distance_category',
                        'confidence',
                        'description',
                        'session_id',
                    ],
                },
            },
            {
                'name': 'set_navigation_mode',
                'description': 'Switch navigation mode NAVIGATION/EXPLORE/READ/QUIET.',
                'parameters': {
                    'type': 'object',
                    'properties': {
                        'mode': {
                            'type': 'string',
                            'enum': ['NAVIGATION', 'EXPLORE', 'READ', 'QUIET'],
                        }
                    },
                    'required': ['mode'],
                },
            },
            {
                'name': 'log_emotion_event',
                'description': 'Log detected emotion state for analytics and adaptive tone.',
                'parameters': {
                    'type': 'object',
                    'properties': {
                        'state': {'type': 'string'},
                        'confidence': {'type': 'number', 'minimum': 0.0, 'maximum': 1.0},
                    },
                    'required': ['state', 'confidence'],
                },
            },
            {
                'name': 'escalate_mode_if_stressed',
                'description': 'Escalate mode to NAVIGATION temporarily when vocal distress is detected.',
                'parameters': {
                    'type': 'object',
                    'properties': {
                        'state': {'type': 'string'},
                        'confidence': {'type': 'number', 'minimum': 0.0, 'maximum': 1.0},
                        'current_mode': {'type': 'string'},
                    },
                    'required': ['state', 'confidence', 'current_mode'],
                },
            },
            {
                'name': 'identify_location',
                'description': 'Identify relevant nearby location context when user is stationary.',
                'parameters': {
                    'type': 'object',
                    'properties': {
                        'lat': {'type': 'number'},
                        'lng': {'type': 'number'},
                        'heading_deg': {'type': 'number'},
                    },
                    'required': ['lat', 'lng', 'heading_deg'],
                },
            },
            {
                'name': 'get_context_summary',
                'description': 'Get session context summary for reconnect/resume.',
                'parameters': {'type': 'object', 'properties': {}},
            },
            {
                'name': 'request_human_help',
                'description': 'Request human assistance and return shareable support link.',
                'parameters': {'type': 'object', 'properties': {}},
            },
        ]

        base = {
            'response_modalities': ['AUDIO'],
            'input_audio_transcription': {},
            'output_audio_transcription': {},
            'speech_config': {
                'voice_config': {'prebuilt_voice_config': {'voice_name': 'Kore'}},
            },
            'enable_affective_dialog': True,
            'proactivity': {'proactive_audio': True},
            'realtime_input_config': {
                'automatic_activity_detection': {'disabled': False},
            },
            'tools': [{'function_declarations': function_declarations}],
            'system_instruction': SYSTEM_PROMPT,
        }

        full = {
            **base,
            'session_resumption': {'transparent': True},
            'context_window_compression': {
                'trigger_tokens': 100000,
                'sliding_window': {'target_tokens': 80000},
            },
        }

        reduced = dict(base)
        minimal = {
            'response_modalities': ['AUDIO'],
            'tools': [{'function_declarations': function_declarations}],
            'system_instruction': SYSTEM_PROMPT,
        }

        return [
            ('full', full),
            ('reduced', reduced),
            ('minimal', minimal),
        ]

    def _model_candidates(self) -> list[str]:
        configured = os.getenv('GEMINI_MODEL', 'gemini-live-2.5-flash-native-audio').strip()
        extra = [
            item.strip()
            for item in os.getenv(
                'GEMINI_MODEL_FALLBACKS',
                '',
            ).split(',')
            if item.strip()
        ]

        ordered: list[str] = []
        for name in [configured, *extra]:
            if name and name not in ordered:
                ordered.append(name)
        return ordered

    async def _receive_loop(self) -> None:
        assert self._session is not None

        try:
            async for response in self._session.receive():
                await self._handle_live_response(response)
        except asyncio.CancelledError:
            raise
        except Exception as exc:
            logger.exception('Live receive error for %s: %s', self._session_id, exc)
            await self._websocket_registry.send_live(
                self._session_id,
                {
                    'type': 'connection_state',
                    'state': 'reconnecting',
                    'detail': f'Live stream dropped: {exc}',
                },
            )
        finally:
            self._connected = False
            session_cm = self._session_cm
            self._session = None
            self._session_cm = None
            if session_cm is not None:
                try:
                    await session_cm.__aexit__(None, None, None)
                except Exception:
                    logger.exception('Error closing live context in receive loop for %s', self._session_id)

    async def _handle_live_response(self, response: Any) -> None:
        texts = self._extract_texts(response)
        for text in texts:
            if self._is_edge_suppressed():
                continue
            if not self._should_emit_text(text):
                continue
            await self._websocket_registry.send_live(
                self._session_id,
                {
                    'type': 'assistant_text',
                    'session_id': self._session_id,
                    'timestamp': self._now(),
                    'text': text,
                },
            )

        audio_chunks = self._extract_audio_base64_chunks(response)
        for chunk_base64 in audio_chunks:
            if self._is_edge_suppressed():
                continue
            await self._websocket_registry.send_live(
                self._session_id,
                {
                    'type': 'audio_chunk',
                    'session_id': self._session_id,
                    'timestamp': self._now(),
                    'pcm16': chunk_base64,
                    'hazard_position_x': self._last_hazard_position,
                },
            )

        tool_calls = self._extract_tool_calls(response)
        if tool_calls:
            await self._handle_tool_calls(tool_calls)

    async def _handle_tool_calls(self, function_calls: list[dict[str, Any]]) -> None:
        if self._session is None:
            return

        function_responses: list[dict[str, Any]] = []

        for call in function_calls:
            name = str(call.get('name', '')).strip()
            call_id = str(call.get('id', '') or call.get('call_id', '')).strip()
            args = call.get('args', {})

            if isinstance(args, str):
                try:
                    args = json.loads(args)
                except json.JSONDecodeError:
                    args = {'raw': args}

            if not isinstance(args, dict):
                args = {'value': args}

            if name == 'log_hazard_event':
                now_ts = time.monotonic()
                if now_ts - self._last_hazard_ts > self._hazard_confirmation_timeout_s:
                    self._consecutive_hazard_frames = 0
                
                self._last_hazard_ts = now_ts
                self._consecutive_hazard_frames += 1
                risk_score = self._compute_risk_multiplier(
                    self._last_motion_state,
                    self._last_pitch,
                    self._last_velocity,
                    self._last_yaw_delta,
                )
                required_frames = self._hazard_confirmation_frames
                if risk_score > 2.0:
                    required_frames = 1
                if risk_score > 3.0 and args.get('confidence', 0.0) < 0.5:
                    args['confidence'] = 0.6

                should_fire = (
                    self._consecutive_hazard_frames >= required_frames or
                    self._last_motion_state in {'walking_fast', 'running'}
                )

                if not should_fire:
                    function_responses.append(
                        {
                            'id': call_id,
                            'name': name,
                            'response': {
                                'ok': True,
                                'buffered': True,
                                'message': 'Hazard detected in 1 frame. Waiting for confirmation in next frame.'
                            },
                        }
                    )
                    continue
                
                self._consecutive_hazard_frames = 0

            result = await self._tool_dispatcher(name, args)
            if name == 'log_hazard_event':
                position = float(result.get('position_x', 0.0) or 0.0)
                self._last_hazard_position = max(-1.0, min(1.0, position))
                await self._websocket_registry.send_live(
                    self._session_id,
                    {
                        'type': 'semantic_cue',
                        'cue': 'approaching_object',
                        'position_x': self._last_hazard_position,
                    },
                )
            if name == 'identify_location':
                near_flag = bool(result.get('destination_near', False))
                distance_raw = result.get('distance_m')
                distance_m = (
                    float(distance_raw)
                    if isinstance(distance_raw, (int, float))
                    else None
                )
                if near_flag or (distance_m is not None and 0 <= distance_m <= 30):
                    await self._websocket_registry.send_live(
                        self._session_id,
                        {
                            'type': 'semantic_cue',
                            'cue': 'destination_near',
                            'position_x': 0,
                        },
                    )

            response_payload = {
                'ok': bool(result.get('ok', True)),
                **result,
            }

            function_responses.append(
                {
                    'id': call_id,
                    'name': name,
                    'response': response_payload,
                }
            )

        async with self._send_lock:
            try:
                await self._session.send_tool_response(function_responses=function_responses)
            except Exception as exc:
                logger.exception('Failed to send tool response for %s: %s', self._session_id, exc)

    async def _on_send_failure(self, operation: str, error: Exception) -> None:
        logger.warning(
            'Live send failure (%s, %s, failures=%s): %s',
            self._session_id,
            operation,
            self._consecutive_send_failures,
            error,
        )

        if self._consecutive_send_failures < 3:
            return

        self._connected = False
        await self._websocket_registry.send_live(
            self._session_id,
            {
                'type': 'connection_state',
                'state': 'reconnecting',
                'detail': f'Live send unstable after {self._consecutive_send_failures} failures; reconnecting.',
            },
        )

        await self.close(mark_stopped=False)

    @staticmethod
    def _compute_risk_multiplier(motion_state: str, pitch: float, velocity: float, yaw_delta: float) -> float:
        score = 1.0
        if motion_state == 'running':
            score *= 2.0
        elif motion_state == 'walking_fast':
            score *= 1.5

        if abs(pitch) > 20:
            score *= 1.3

        if abs(yaw_delta) > 30:
            score *= 1.4

        if velocity > 2.5 and abs(pitch) > 15:
            score *= 1.5

        return min(score, 4.0)

    def _build_blob(self, data: bytes, mime_type: str) -> Any:
        if self._types is None:
            return {'mime_type': mime_type, 'data': data}
        return self._types.Blob(data=data, mime_type=mime_type)

    def _should_emit_text(self, text: str) -> bool:
        now = time.monotonic()
        normalized = text.strip().lower()
        high_priority_tokens = ('stop', 'hazard', 'danger', 'vehicle', 'stairs', 'hole', 'drop')
        if any(token in normalized for token in high_priority_tokens):
            self._last_voice_emit_ts = now
            return True
        if now - self._last_voice_emit_ts >= 8:
            self._last_voice_emit_ts = now
            return True
        return False

    def _is_edge_suppressed(self) -> bool:
        return time.monotonic() < self._edge_suppress_until

    @staticmethod
    def _now() -> str:
        return datetime.now(timezone.utc).isoformat()

    @staticmethod
    def _extract_texts(response: Any) -> list[str]:
        results: list[str] = []
        seen: set[str] = set()

        def add(text: Any) -> None:
            if not isinstance(text, str):
                return
            normalized = text.strip()
            if not normalized or normalized in seen:
                return
            seen.add(normalized)
            results.append(normalized)

        add(getattr(response, 'text', None))

        raw = GeminiLiveBridge._as_dict(response)
        add(raw.get('text'))

        server_content = raw.get('server_content') or raw.get('serverContent') or {}
        output_transcription = (
            server_content.get('output_transcription')
            or server_content.get('outputTranscription')
            or {}
        )
        add(output_transcription.get('text'))

        model_turn = server_content.get('model_turn') or server_content.get('modelTurn') or {}
        for part in model_turn.get('parts', []) or []:
            if isinstance(part, dict):
                add(part.get('text'))

        return results

    @staticmethod
    def _extract_audio_base64_chunks(response: Any) -> list[str]:
        chunks: list[str] = []

        def add_audio(value: Any) -> None:
            if value is None:
                return
            if isinstance(value, bytes):
                if not value:
                    return
                chunks.append(base64.b64encode(value).decode('ascii'))
                return
            if isinstance(value, str):
                # Some SDK versions may already deliver base64 string payload.
                if value:
                    chunks.append(value)

        add_audio(getattr(response, 'data', None))

        raw = GeminiLiveBridge._as_dict(response)
        add_audio(raw.get('data'))

        server_content = raw.get('server_content') or raw.get('serverContent') or {}
        model_turn = server_content.get('model_turn') or server_content.get('modelTurn') or {}
        parts = model_turn.get('parts', []) or []

        for part in parts:
            if not isinstance(part, dict):
                continue
            inline_data = part.get('inline_data') or part.get('inlineData') or {}
            add_audio(inline_data.get('data'))

        return chunks

    @staticmethod
    def _extract_tool_calls(response: Any) -> list[dict[str, Any]]:
        response_obj = response
        tool_call_obj = getattr(response_obj, 'tool_call', None) or getattr(response_obj, 'toolCall', None)

        function_calls: list[Any] = []
        if tool_call_obj is not None:
            function_calls = (
                getattr(tool_call_obj, 'function_calls', None)
                or getattr(tool_call_obj, 'functionCalls', None)
                or []
            )

        if not function_calls:
            raw = GeminiLiveBridge._as_dict(response_obj)
            tool_call_raw = raw.get('tool_call') or raw.get('toolCall') or {}
            function_calls = tool_call_raw.get('function_calls') or tool_call_raw.get('functionCalls') or []

        normalized: list[dict[str, Any]] = []
        for call in function_calls:
            if isinstance(call, dict):
                normalized.append(call)
                continue

            normalized.append(
                {
                    'id': getattr(call, 'id', '') or getattr(call, 'call_id', ''),
                    'name': getattr(call, 'name', ''),
                    'args': getattr(call, 'args', {}) or getattr(call, 'arguments', {}),
                }
            )

        return normalized

    @staticmethod
    def _as_dict(value: Any) -> dict[str, Any]:
        if value is None:
            return {}
        if isinstance(value, dict):
            return value

        if hasattr(value, 'model_dump'):
            try:
                dumped = value.model_dump(exclude_none=True)
                if isinstance(dumped, dict):
                    return dumped
            except Exception:
                pass

        if hasattr(value, 'to_dict'):
            try:
                dumped = value.to_dict()
                if isinstance(dumped, dict):
                    return dumped
            except Exception:
                pass

        return {}
