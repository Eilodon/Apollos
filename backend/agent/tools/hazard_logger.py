from __future__ import annotations

from datetime import datetime, timezone
from time import time

from .decorators import tool
from .runtime import get_current_session, get_runtime


@tool
async def log_hazard_event(
    hazard_type: str,
    position_x: float,
    distance_category: str,
    confidence: float,
    description: str,
    session_id: str,
) -> str:
    runtime = get_runtime()
    sid = session_id or get_current_session()
    emitted_at_ms = int(time() * 1000)
    emitted_at = datetime.now(timezone.utc).isoformat()

    payload = {
        'type': 'HARD_STOP',
        'position_x': max(-1.0, min(1.0, position_x)),
        'distance': distance_category,
        'hazard_type': hazard_type,
        'confidence': confidence,
        'server_emit_ts': emitted_at,
        'server_emit_ts_ms': emitted_at_ms,
    }

    await runtime.websocket_registry.emit_hard_stop(sid, payload)
    await runtime.session_store.log_hazard(
        sid,
        hazard_type,
        payload['position_x'],
        distance_category,
        confidence,
        description,
    )

    return f"Interrupt fired. {hazard_type} at x={payload['position_x']}"
