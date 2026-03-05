from __future__ import annotations

import hmac
import json
import logging
import os
import secrets
from contextlib import asynccontextmanager
from datetime import datetime, timezone
from typing import Literal

from fastapi import FastAPI, HTTPException, Request, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel, Field
from starlette.responses import JSONResponse

try:
    from agent.aria_agent import AriaAgentOrchestrator
    from agent.auth import OIDCAuthError, OIDCVerifier, load_auth_config_from_env
    from agent.broker_auth import BrokerAuthError, BrokerConfig, OIDCBrokerManager
    from agent.human_fallback import (
        HumanFallbackError,
        HumanFallbackManager,
        build_human_fallback_config,
    )
    from agent.session_manager import SessionStore
    from agent.websocket_handler import WebSocketRegistry
    from agent.ws_auth import extract_ws_token, resolve_allow_query_token, select_ws_subprotocol
except ModuleNotFoundError:  # pragma: no cover - fallback for package-style import
    from backend.agent.aria_agent import AriaAgentOrchestrator
    from backend.agent.auth import OIDCAuthError, OIDCVerifier, load_auth_config_from_env
    from backend.agent.broker_auth import BrokerAuthError, BrokerConfig, OIDCBrokerManager
    from backend.agent.human_fallback import (
        HumanFallbackError,
        HumanFallbackManager,
        build_human_fallback_config,
    )
    from backend.agent.session_manager import SessionStore
    from backend.agent.websocket_handler import WebSocketRegistry
    from backend.agent.ws_auth import extract_ws_token, resolve_allow_query_token, select_ws_subprotocol

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


class OIDCExchangeRequest(BaseModel):
    id_token: str | None = None


class HelpTicketExchangeRequest(BaseModel):
    ticket: str = Field(min_length=32, max_length=4096)


USE_FIRESTORE = _env_flag('USE_FIRESTORE', False)
ENABLE_DEV_ENDPOINTS = _env_flag('ENABLE_DEV_ENDPOINTS', False)
DEV_ENDPOINT_TOKEN = os.getenv('DEV_ENDPOINT_TOKEN', '').strip()
APP_ENV = os.getenv('APP_ENV', 'development').strip().lower()
CORS_ALLOW_ORIGINS = _cors_origins_from_env()
AUTH_CONFIG = load_auth_config_from_env()
ALLOW_WS_QUERY_TOKEN = resolve_allow_query_token(APP_ENV, os.getenv('WS_ALLOW_QUERY_TOKEN'))
MAX_WS_MESSAGE_BYTES = max(2048, int(os.getenv('MAX_WS_MESSAGE_BYTES', '1250000') or 1250000))
OIDC_BROKER_ENABLED = _env_flag('OIDC_BROKER_ENABLED', True)
HUMAN_FALLBACK_CONFIG = build_human_fallback_config(APP_ENV)

session_store = SessionStore(use_firestore=USE_FIRESTORE)
websocket_registry = WebSocketRegistry()
orchestrator = AriaAgentOrchestrator(session_store=session_store, websocket_registry=websocket_registry)
_oidc_verifier: OIDCVerifier | None = None
_broker_manager: OIDCBrokerManager | None = None
_human_fallback_manager: HumanFallbackManager | None = None


def _build_broker_config() -> BrokerConfig:
    signing_key = os.getenv('OIDC_BROKER_SIGNING_KEY', '').strip()
    if not signing_key:
        if APP_ENV in {'prod', 'production'}:
            raise RuntimeError('OIDC_BROKER_SIGNING_KEY is required in production.')
        signing_key = secrets.token_urlsafe(48)

    cookie_secure = _env_flag('OIDC_BROKER_COOKIE_SECURE', APP_ENV in {'prod', 'production'})
    cookie_samesite = os.getenv('OIDC_BROKER_COOKIE_SAMESITE', 'lax').strip().lower() or 'lax'
    if cookie_samesite not in {'lax', 'strict', 'none'}:
        cookie_samesite = 'lax'

    audience = AUTH_CONFIG.audience or ('apollos-ws',)
    return BrokerConfig(
        signing_key=signing_key,
        issuer=os.getenv('OIDC_BROKER_ISSUER', 'apollos-oidc-broker').strip() or 'apollos-oidc-broker',
        audience=audience,
        ws_ttl_seconds=max(30, int(os.getenv('OIDC_BROKER_WS_TTL_SECONDS', '90') or 90)),
        session_ttl_seconds=max(300, int(os.getenv('OIDC_BROKER_SESSION_TTL_SECONDS', '3600') or 3600)),
        cookie_name=os.getenv('OIDC_BROKER_COOKIE_NAME', 'apollos_broker_session').strip() or 'apollos_broker_session',
        cookie_secure=cookie_secure,
        cookie_samesite=cookie_samesite,
        cookie_domain=os.getenv('OIDC_BROKER_COOKIE_DOMAIN', '').strip(),
        cookie_path=os.getenv('OIDC_BROKER_COOKIE_PATH', '/').strip() or '/',
    )


@asynccontextmanager
async def lifespan(application: FastAPI):
    global _oidc_verifier, _broker_manager, _human_fallback_manager
    if APP_ENV in {'prod', 'production'} and AUTH_CONFIG.mode == 'shared_token' and not AUTH_CONFIG.shared_token:
        raise RuntimeError('WS_AUTH_TOKEN is required in production when WS_AUTH_MODE=shared_token.')
    if AUTH_CONFIG.mode == 'oidc':
        _oidc_verifier = OIDCVerifier(AUTH_CONFIG)
        if OIDC_BROKER_ENABLED:
            _broker_manager = OIDCBrokerManager(_build_broker_config())
    else:
        _broker_manager = None
    if HUMAN_FALLBACK_CONFIG.enabled:
        _human_fallback_manager = HumanFallbackManager(HUMAN_FALLBACK_CONFIG)
    else:
        _human_fallback_manager = None
    orchestrator.set_human_fallback_manager(_human_fallback_manager)
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
    return extract_ws_token(
        headers=websocket.headers,
        query_params=websocket.query_params,
        allow_query_token=ALLOW_WS_QUERY_TOKEN,
        logger=logger,
    )


def _select_ws_subprotocol(websocket: WebSocket) -> str | None:
    return select_ws_subprotocol(websocket.headers, preferred='apollos.v1')


def _select_help_ws_subprotocol(websocket: WebSocket) -> str | None:
    return select_ws_subprotocol(websocket.headers, preferred='apollos.help.v1')


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
        if not token:
            return False, 'Missing bearer token.', None
        try:
            claims = _oidc_verifier.verify_token(token)
            claims['auth_source'] = 'oidc'
            return True, '', claims
        except OIDCAuthError as exc:
            oidc_error = str(exc)
            if _broker_manager is None:
                return False, oidc_error, None
            try:
                claims = _broker_manager.verify_ws_ticket(token)
                claims['auth_source'] = 'oidc_broker'
                return True, '', claims
            except BrokerAuthError:
                return False, oidc_error, None

    return False, f'Unsupported WS_AUTH_MODE={mode}', None


def _authorize_help_viewer(ws_token: str, session_id: str) -> tuple[bool, str, dict[str, object] | None]:
    if _human_fallback_manager is None:
        return False, 'Human fallback not enabled.', None
    if not ws_token:
        return False, 'Missing helper viewer token.', None
    try:
        claims = _human_fallback_manager.verify_viewer_token(ws_token, session_id=session_id)
        return True, '', claims
    except HumanFallbackError as exc:
        return False, str(exc), None


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
    return bool(supplied and hmac.compare_digest(supplied, DEV_ENDPOINT_TOKEN))


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
        'oidc_broker_enabled': bool(OIDC_BROKER_ENABLED and _broker_manager is not None),
        'oidc_broker_ws_ttl_seconds': _broker_manager.ws_ttl_seconds if _broker_manager else 0,
        'human_fallback_enabled': bool(_human_fallback_manager is not None and _human_fallback_manager.enabled),
        'dev_endpoints_enabled': ENABLE_DEV_ENDPOINTS,
        'max_ws_message_bytes': MAX_WS_MESSAGE_BYTES,
    }


def _extract_bearer_from_request(request: Request) -> str:
    auth_header = request.headers.get('authorization', '')
    if auth_header.lower().startswith('bearer '):
        return auth_header[7:].strip()
    return ''


@app.post('/auth/oidc/exchange')
async def auth_oidc_exchange(request: Request, payload: OIDCExchangeRequest | None = None) -> JSONResponse:
    if AUTH_CONFIG.mode != 'oidc' or _oidc_verifier is None or _broker_manager is None:
        raise HTTPException(status_code=404, detail='OIDC broker not enabled.')

    token = _extract_bearer_from_request(request) or str((payload.id_token if payload else '') or '').strip()
    if not token:
        raise HTTPException(status_code=401, detail='Missing OIDC bearer token.')
    try:
        claims = _oidc_verifier.verify_token(token)
    except OIDCAuthError as exc:
        raise HTTPException(status_code=401, detail=str(exc)) from exc

    session_id = _broker_manager.create_session(claims)
    response = JSONResponse(
        {
            'ok': True,
            'subject': claims.get('sub', ''),
            'expires_in': _broker_manager.session_ttl_seconds,
        }
    )
    response.set_cookie(
        key=_broker_manager.cookie_name,
        value=session_id,
        max_age=_broker_manager.session_ttl_seconds,
        httponly=True,
        secure=_broker_manager.cookie_secure,
        samesite=_broker_manager.cookie_samesite,
        domain=_broker_manager.cookie_domain or None,
        path=_broker_manager.cookie_path,
    )
    return response


@app.post('/auth/ws-ticket')
async def auth_ws_ticket(request: Request) -> dict[str, object]:
    if AUTH_CONFIG.mode != 'oidc' or _broker_manager is None:
        raise HTTPException(status_code=404, detail='OIDC broker not enabled.')

    session_id = request.cookies.get(_broker_manager.cookie_name, '').strip()
    if not session_id:
        raise HTTPException(status_code=401, detail='Missing broker session.')
    try:
        access_token, expires_in = _broker_manager.issue_ws_ticket(session_id)
    except BrokerAuthError as exc:
        raise HTTPException(status_code=401, detail=str(exc)) from exc

    return {
        'access_token': access_token,
        'token_type': 'Bearer',
        'expires_in': expires_in,
    }


@app.post('/auth/logout')
async def auth_logout(request: Request) -> JSONResponse:
    if _broker_manager is None:
        return JSONResponse({'ok': True})
    session_id = request.cookies.get(_broker_manager.cookie_name, '').strip()
    if session_id:
        _broker_manager.revoke_session(session_id)
    response = JSONResponse({'ok': True})
    response.delete_cookie(
        key=_broker_manager.cookie_name,
        domain=_broker_manager.cookie_domain or None,
        path=_broker_manager.cookie_path,
    )
    return response


@app.post('/auth/help-ticket/exchange')
async def auth_help_ticket_exchange(payload: HelpTicketExchangeRequest) -> dict[str, object]:
    if _human_fallback_manager is None:
        raise HTTPException(status_code=404, detail='Human fallback not enabled.')
    try:
        exchanged = _human_fallback_manager.exchange_help_ticket(payload.ticket.strip())
    except HumanFallbackError as exc:
        raise HTTPException(status_code=401, detail=str(exc)) from exc
    return {
        'session_id': str(exchanged.get('session_id', '')).strip(),
        'viewer_token': str(exchanged.get('viewer_token', '')).strip(),
        'expires_in': int(exchanged.get('expires_in', 0) or 0),
        'rtc': exchanged.get('rtc') if isinstance(exchanged.get('rtc'), dict) else None,
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
    subprotocol = _select_ws_subprotocol(websocket)
    if subprotocol:
        await websocket.accept(subprotocol=subprotocol)
    else:
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
            raw_message = await websocket.receive_text()
            if len(raw_message.encode('utf-8')) > MAX_WS_MESSAGE_BYTES:
                await websocket.send_json(
                    {
                        'type': 'connection_state',
                        'state': 'disconnected',
                        'session_id': session_id,
                        'timestamp': _now(),
                        'detail': 'Inbound websocket payload exceeds MAX_WS_MESSAGE_BYTES.',
                    }
                )
                await websocket.close(code=4400)
                break
            try:
                payload = json.loads(raw_message)
            except json.JSONDecodeError:
                continue
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
    subprotocol = _select_ws_subprotocol(websocket)
    if subprotocol:
        await websocket.accept(subprotocol=subprotocol)
    else:
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
            if len(raw_message.encode('utf-8')) > MAX_WS_MESSAGE_BYTES:
                await websocket.send_json(
                    {
                        'type': 'connection_state',
                        'state': 'disconnected',
                        'session_id': session_id,
                        'timestamp': _now(),
                        'detail': 'Inbound websocket payload exceeds MAX_WS_MESSAGE_BYTES.',
                    }
                )
                await websocket.close(code=4400)
                break

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


@app.websocket('/ws/help/{session_id}')
async def websocket_help_view(websocket: WebSocket, session_id: str) -> None:
    token = extract_ws_token(
        headers=websocket.headers,
        query_params=websocket.query_params,
        allow_query_token=False,
        logger=logger,
    )
    authorized, reason, claims = _authorize_help_viewer(token, session_id=session_id)
    if not authorized:
        await _reject_ws(websocket, detail=reason, code=4401)
        return

    viewer_id = str((claims or {}).get('jti') or '').strip() or f'viewer-{int(datetime.now(timezone.utc).timestamp())}'
    subprotocol = _select_help_ws_subprotocol(websocket)
    if subprotocol:
        await websocket.accept(subprotocol=subprotocol)
    else:
        await websocket.accept()
    await websocket_registry.register_help_viewer(session_id, websocket, viewer_id=viewer_id)
    await websocket.send_json(
        {
            'type': 'help_ready',
            'session_id': session_id,
            'timestamp': _now(),
            'viewer_id': viewer_id,
        }
    )
    try:
        while True:
            raw_message = await websocket.receive_text()
            if not raw_message:
                continue
            if raw_message == 'ping':
                await websocket.send_text('pong')
                continue
            try:
                payload = json.loads(raw_message)
            except json.JSONDecodeError:
                continue
            if isinstance(payload, dict) and payload.get('type') == 'heartbeat':
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
        await websocket_registry.unregister_help_viewer(session_id, websocket, viewer_id=viewer_id)
