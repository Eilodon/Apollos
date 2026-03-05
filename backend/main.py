from __future__ import annotations

import json
import logging
import os
from datetime import datetime, timezone
from typing import Literal

from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from pydantic import BaseModel, Field

try:
    from agent.aria_agent import AriaAgentOrchestrator
    from agent.session_manager import SessionStore
    from agent.websocket_handler import WebSocketRegistry
except ModuleNotFoundError:  # pragma: no cover - fallback for package-style import
    from backend.agent.aria_agent import AriaAgentOrchestrator
    from backend.agent.session_manager import SessionStore
    from backend.agent.websocket_handler import WebSocketRegistry

logger = logging.getLogger(__name__)


def _env_flag(name: str, default: bool) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.strip().lower() not in {'0', 'false', 'off', 'no'}


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


class HazardTriggerRequest(BaseModel):
    hazard_type: str = Field(default='manual_hazard', min_length=1, max_length=64)
    position_x: float = Field(default=0.0, ge=-1.0, le=1.0)
    distance: Literal['very_close', 'mid', 'far'] = 'very_close'
    confidence: float = Field(default=0.95, ge=0.0, le=1.0)
    description: str = Field(default='manual trigger from dev endpoint', max_length=400)


USE_FIRESTORE = _env_flag('USE_FIRESTORE', False)
session_store = SessionStore(use_firestore=USE_FIRESTORE)
websocket_registry = WebSocketRegistry()
orchestrator = AriaAgentOrchestrator(session_store=session_store, websocket_registry=websocket_registry)

app = FastAPI(title='VisionGPT ARIA Backend')


@app.get('/healthz')
async def healthz() -> dict[str, object]:
    stats = await orchestrator.stats()
    return {
        'status': 'ok',
        'timestamp': _now(),
        'use_firestore': USE_FIRESTORE,
        **stats,
    }


@app.get('/config')
async def config() -> dict[str, object]:
    return {
        'timestamp': _now(),
        'live_enabled': orchestrator.live_enabled,
        'use_firestore': USE_FIRESTORE,
        'model': os.getenv('GEMINI_MODEL', 'gemini-live-2.5-flash-native-audio'),
        'model_fallbacks': [
            item.strip()
            for item in os.getenv(
                'GEMINI_MODEL_FALLBACKS',
                'gemini-2.5-flash-native-audio-preview-12-2025,gemini-live-2.5-flash-preview',
            ).split(',')
            if item.strip()
        ],
        'use_vertex': _env_flag('GEMINI_USE_VERTEX', False),
    }


@app.post('/dev/hazard/{session_id}')
async def dev_hazard(session_id: str, payload: HazardTriggerRequest) -> dict[str, object]:
    result = await orchestrator.dispatch_tool_call(
        'log_hazard_event',
        {
            'hazard_type': payload.hazard_type,
            'position_x': payload.position_x,
            'distance_category': payload.distance,
            'confidence': payload.confidence,
            'description': payload.description,
            'session_id': session_id,
        },
        session_id=session_id,
    )

    return {
        'ok': bool(result.get('ok', False)),
        'session_id': session_id,
        'result': result,
    }


@app.websocket('/ws/live/{session_id}')
async def websocket_live(websocket: WebSocket, session_id: str) -> None:
    await websocket.accept()
    await websocket_registry.register_live(session_id, websocket)

    await websocket_registry.send_live(
        session_id,
        {
            'type': 'connection_state',
            'state': 'connected',
            'session_id': session_id,
            'timestamp': _now(),
            'detail': 'Live websocket ready',
        },
    )

    try:
        while True:
            payload = await websocket.receive_json()
            if not isinstance(payload, dict):
                continue
            payload.setdefault('session_id', session_id)
            await orchestrator.handle_client_message(session_id, payload)
    except WebSocketDisconnect:
        pass
    except Exception as exc:
        logger.exception('Live websocket failure for %s: %s', session_id, exc)
        await websocket_registry.send_live(
            session_id,
            {
                'type': 'connection_state',
                'state': 'reconnecting',
                'session_id': session_id,
                'timestamp': _now(),
                'detail': f'Live websocket error: {exc}',
            },
        )
    finally:
        await websocket_registry.unregister_live(session_id, websocket)
        await orchestrator.close_session(session_id)


@app.websocket('/ws/emergency/{session_id}')
async def websocket_emergency(websocket: WebSocket, session_id: str) -> None:
    await websocket.accept()
    await websocket_registry.register_emergency(session_id, websocket)
    await websocket.send_json(
        {
            'type': 'emergency_ready',
            'session_id': session_id,
            'timestamp': _now(),
        }
    )

    try:
        while True:
            raw_message = await websocket.receive_text()
            if not raw_message:
                continue

            try:
                payload = json.loads(raw_message)
            except json.JSONDecodeError:
                continue

            if payload.get('type') == 'heartbeat':
                await websocket.send_json(
                    {
                        'type': 'heartbeat_ack',
                        'session_id': session_id,
                        'timestamp': _now(),
                    }
                )
    except WebSocketDisconnect:
        pass
    finally:
        await websocket_registry.unregister_emergency(session_id, websocket)


@app.on_event('shutdown')
async def on_shutdown() -> None:
    await orchestrator.shutdown()
