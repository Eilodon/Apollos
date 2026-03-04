from __future__ import annotations

import json
import os
from datetime import datetime, timezone
from time import time
from typing import Any

from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel, Field

from agent.aria_agent import AriaAgent
from agent.session_manager import SessionStore
from agent.tools.hazard_logger import log_hazard_event
from agent.tools.runtime import reset_current_session, set_current_session
from agent.websocket_handler import WebSocketRegistry


class HazardRequest(BaseModel):
    hazard_type: str = 'drop'
    position_x: float = Field(default=0.0, ge=-1.0, le=1.0)
    distance: str = 'very_close'
    confidence: float = Field(default=0.95, ge=0.0, le=1.0)
    description: str = 'Manual hazard trigger for testing.'


app = FastAPI(title='VisionGPT ARIA Backend', version='0.1.0')

app.add_middleware(
    CORSMiddleware,
    allow_origins=['*'],
    allow_methods=['*'],
    allow_headers=['*'],
)

session_store = SessionStore(
    project_id=os.getenv('GOOGLE_CLOUD_PROJECT'),
    use_firestore=os.getenv('USE_FIRESTORE', '0') == '1',
)
ws_registry = WebSocketRegistry()
aria_agent = AriaAgent(session_store=session_store, websocket_registry=ws_registry)


@app.get('/healthz')
async def healthz() -> dict[str, str]:
    return {'status': 'ok'}


@app.get('/config')
async def read_config() -> dict[str, object]:
    run_config = aria_agent.run_config
    if hasattr(run_config, 'payload'):
        serialized = run_config.payload
    else:
        serialized = {'type': type(run_config).__name__}

    return {
        'model': os.getenv('GEMINI_MODEL', 'gemini-live-2.5-flash-native-audio'),
        'run_config': serialized,
    }


@app.on_event('shutdown')
async def on_shutdown() -> None:
    await aria_agent.close_all_sessions()


@app.websocket('/ws/live/{session_id}')
async def ws_live(websocket: WebSocket, session_id: str) -> None:
    await websocket.accept()
    await ws_registry.register_live(session_id, websocket)

    await ws_registry.send_live(
        session_id,
        {
            'type': 'connection_state',
            'state': 'connected',
            'detail': 'Live channel ready',
        },
    )

    try:
        while True:
            raw_message = await websocket.receive_text()
            try:
                payload = json.loads(raw_message)
            except json.JSONDecodeError:
                await ws_registry.send_live(
                    session_id,
                    {
                        'type': 'assistant_text',
                        'session_id': session_id,
                        'timestamp': session_store.now(),
                        'text': 'Malformed JSON payload.',
                    },
                )
                continue
            await aria_agent.handle_client_message(session_id, payload)
    except WebSocketDisconnect:
        pass
    finally:
        await ws_registry.unregister_live(session_id, websocket)
        await aria_agent.close_session(session_id)


@app.websocket('/ws/emergency/{session_id}')
async def ws_emergency(websocket: WebSocket, session_id: str) -> None:
    await websocket.accept()
    await ws_registry.register_emergency(session_id, websocket)

    try:
        while True:
            # Keep socket alive. Client may optionally send heartbeat messages.
            await websocket.receive_text()
    except WebSocketDisconnect:
        pass
    finally:
        await ws_registry.unregister_emergency(session_id, websocket)


@app.post('/dev/hazard/{session_id}')
async def trigger_hazard(session_id: str, body: HazardRequest) -> dict[str, Any]:
    received_at_ms = int(time() * 1000)
    token = set_current_session(session_id)
    try:
        message = await log_hazard_event(
            hazard_type=body.hazard_type,
            position_x=body.position_x,
            distance_category=body.distance,
            confidence=body.confidence,
            description=body.description,
            session_id=session_id,
        )
    finally:
        reset_current_session(token)

    return {
        'status': 'ok',
        'message': message,
        'request_received_ts': datetime.now(timezone.utc).isoformat(),
        'request_received_ts_ms': received_at_ms,
    }
