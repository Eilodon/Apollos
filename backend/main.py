from __future__ import annotations

import hmac
import json
import logging
import os
from contextlib import asynccontextmanager
from datetime import datetime, timezone
from typing import Literal

from fastapi import FastAPI, HTTPException, Request, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel, Field

try:
    from agent.aria_agent import AriaAgentOrchestrator
    from agent.auth import OIDCAuthError, OIDCVerifier, load_auth_config_from_env
    from agent.session_manager import SessionStore
    from agent.websocket_handler import WebSocketRegistry
except ModuleNotFoundError:  # pragma: no cover - fallback for package-style import
    from backend.agent.aria_agent import AriaAgentOrchestrator
    from backend.agent.auth import OIDCAuthError, OIDCVerifier, load_auth_config_from_env
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


def _cors_origins_from_env() -> list[str]:
    configured = os.getenv('CORS_ALLOW_ORIGINS', '').strip()
    if not configured:
        return [
            'http://localhost:5173',
            'http://127.0.0.1:5173',
        ]
    origins = [item.strip() for item in configured.split(',') if item.strip()]
    return origins or ['http://localhost:5173', 'http://127.0.0.1:5173']


class HazardTriggerRequest(BaseModel):
    hazard_type: str = Field(default='manual_hazard', min_length=1, max_length=64)
    position_x: float = Field(default=0.0, ge=-1.0, le=1.0)
    distance: Literal['very_close', 'mid', 'far'] = 'very_close'
    confidence: float = Field(default=0.95, ge=0.0, le=1.0)
    description: str = Field(default='manual trigger from dev endpoint', max_length=400)


USE_FIRESTORE = _env_flag('USE_FIRESTORE', False)
ENABLE_DEV_ENDPOINTS = _env_flag('ENABLE_DEV_ENDPOINTS', False)
DEV_ENDPOINT_TOKEN = os.getenv('DEV_ENDPOINT_TOKEN', '').strip()
APP_ENV = os.getenv('APP_ENV', 'development').strip().lower()
CORS_ALLOW_ORIGINS = _cors_origins_from_env()
AUTH_CONFIG = load_auth_config_from_env()
session_store = SessionStore(use_firestore=USE_FIRESTORE)
websocket_registry = WebSocketRegistry()
orchestrator = AriaAgentOrchestrator(session_store=session_store, websocket_registry=websocket_registry)
_oidc_verifier: OIDCVerifier | None = None


@asynccontextmanager
async def lifespan(application: FastAPI):
    global _oidc_verifier
    if APP_ENV in {'prod', 'production'} and AUTH_CONFIG.mode == 'shared_token' and not AUTH_CONFIG.shared_token:
        raise RuntimeError('WS_AUTH_TOKEN is required in production when WS_AUTH_MODE=shared_token.')
    if AUTH_CONFIG.mode == 'oidc':
        _oidc_verifier = OIDCVerifier(AUTH_CONFIG)
    if APP_ENV in {'prod', 'production'} and ENABLE_DEV_ENDPOINTS and not DEV_ENDPOINT_TOKEN:
        raise RuntimeError('DEV_ENDPOINT_TOKEN is required when ENABLE_DEV_ENDPOINTS=1 in production.')
    if APP_ENV in {'prod', 'production'} and '*' in CORS_ALLOW_ORIGINS:
        raise RuntimeError('CORS_ALLOW_ORIGINS must not contain "*" in production.')
    yield
    await orchestrator.shutdown()


app = FastAPI(title='VisionGPT ARIA Backend', lifespan=lifespan)
app.add_middleware(
    CORSMiddleware,
    allow_origins=CORS_ALLOW_ORIGINS,
    allow_credentials=True,
    allow_methods=['*'],
    allow_headers=['*'],
)


def _extract_client_id(websocket: WebSocket) -> str | None:
    client_id = websocket.query_params.get('client_id') or websocket.headers.get('x-client-id')
    if not client_id:
        return None
    normalized = client_id.strip()
    return normalized or None


def _extract_ws_token(websocket: WebSocket) -> str:
    token = websocket.query_params.get('token') or websocket.headers.get('x-ws-token')
    if token:
        return token.strip()
    auth_header = websocket.headers.get('authorization', '')
    if auth_header.lower().startswith('bearer '):
        return auth_header[7:].strip()
    return ''


def _authorize_ws(websocket: WebSocket) -> tuple[bool, str, dict[str, object] | None]:
    mode = AUTH_CONFIG.mode
    token = _extract_ws_token(websocket)

    if mode in {'disabled', 'none'}:
        return True, '', None

    if mode == 'shared_token':
        if not AUTH_CONFIG.shared_token:
            return True, '', None
        if token and hmac.compare_digest(token, AUTH_CONFIG.shared_token):
            return True, '', None
        return False, 'Unauthorized websocket token.', None

    if mode == 'oidc':
        if _oidc_verifier is None:
            return False, 'OIDC verifier not initialized.', None
        try:
            claims = _oidc_verifier.verify_token(token)
        except OIDCAuthError as exc:
            return False, str(exc), None
        return True, '', claims

    return False, f'Unsupported WS_AUTH_MODE={mode}', None


async def _reject_ws(websocket: WebSocket, detail: str, code: int) -> None:
    await websocket.accept()
    await websocket.send_json(
        {
            'type': 'connection_state',
            'state': 'disconnected',
            'timestamp': _now(),
            'detail': detail,
        }
    )
    await websocket.close(code=code)


def _is_dev_request_authorized(request: Request) -> bool:
    if not DEV_ENDPOINT_TOKEN:
        return True
    supplied = request.headers.get('x-dev-token') or request.query_params.get('token')
    return bool(supplied and supplied == DEV_ENDPOINT_TOKEN)


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
                '',
            ).split(',')
            if item.strip()
        ],
        'use_vertex': _env_flag('GEMINI_USE_VERTEX', False),
        'maps_grounding_enabled': _env_flag('ENABLE_MAPS_GROUNDING', False),
        'hazard_confirmation_frames': int(os.getenv('HAZARD_CONFIRMATION_FRAMES', '1')),
        'edge_hazard_suppress_seconds': float(os.getenv('EDGE_HAZARD_SUPPRESS_SECONDS', '2.5')),
        'ws_auth_mode': AUTH_CONFIG.mode,
        'ws_auth_enabled': AUTH_CONFIG.mode not in {'disabled', 'none'},
        'dev_endpoints_enabled': ENABLE_DEV_ENDPOINTS,
    }


@app.post('/dev/hazard/{session_id}')
async def dev_hazard(session_id: str, payload: HazardTriggerRequest, request: Request) -> dict[str, object]:
    if not ENABLE_DEV_ENDPOINTS:
        raise HTTPException(status_code=404, detail='Not Found')
    if not _is_dev_request_authorized(request):
        raise HTTPException(status_code=401, detail='Unauthorized dev endpoint access')

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
    authorized, reason, claims = _authorize_ws(websocket)
    if not authorized:
        await _reject_ws(websocket, detail=reason or 'Unauthorized websocket token.', code=4401)
        return

    client_id = _extract_client_id(websocket)
    if not client_id:
        await _reject_ws(websocket, detail='Missing client_id.', code=4400)
        return
    await websocket.accept()
    if claims is not None:
        websocket.scope['auth_claims'] = claims
    registered, reason = await websocket_registry.register_live(session_id, websocket, client_id=client_id)
    if not registered:
        await websocket.send_json(
            {
                'type': 'connection_state',
                'state': 'disconnected',
                'session_id': session_id,
                'timestamp': _now(),
                'detail': reason,
            }
        )
        await websocket.close(code=4409)
        return

    await websocket_registry.send_live(
        session_id,
        {
            'type': 'connection_state',
            'state': 'connected',
            'session_id': session_id,
            'timestamp': _now(),
            'detail': 'Live websocket ready',
            'client_id': client_id,
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
    authorized, reason, claims = _authorize_ws(websocket)
    if not authorized:
        await _reject_ws(websocket, detail=reason or 'Unauthorized websocket token.', code=4401)
        return

    client_id = _extract_client_id(websocket)
    if not client_id:
        await _reject_ws(websocket, detail='Missing client_id.', code=4400)
        return
    await websocket.accept()
    if claims is not None:
        websocket.scope['auth_claims'] = claims
    registered, reason = await websocket_registry.register_emergency(session_id, websocket, client_id=client_id)
    if not registered:
        await websocket.send_json(
            {
                'type': 'connection_state',
                'state': 'disconnected',
                'session_id': session_id,
                'timestamp': _now(),
                'detail': reason,
            }
        )
        await websocket.close(code=4409)
        return

    await websocket.send_json(
        {
            'type': 'emergency_ready',
            'session_id': session_id,
            'timestamp': _now(),
            'client_id': client_id,
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
                continue

            if payload.get('type') == 'EDGE_HAZARD':
                await orchestrator.handle_emergency_message(session_id, payload)
                await websocket.send_json(
                    {
                        'type': 'edge_hazard_ack',
                        'session_id': session_id,
                        'timestamp': _now(),
                    }
                )
    except WebSocketDisconnect:
        pass
    finally:
        await websocket_registry.unregister_emergency(session_id, websocket)
