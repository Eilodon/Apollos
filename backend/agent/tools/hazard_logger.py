from __future__ import annotations

from datetime import datetime, timezone
from time import time

from ..hazard_taxonomy import normalize_hazard_type
from ..safety_policy import SafetyPolicyInput, evaluate_safety_policy
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
) -> dict[str, object]:
    runtime = get_runtime()
    sid = session_id or get_current_session()
    normalized_hazard = normalize_hazard_type(hazard_type)
    distance = str(distance_category or 'mid').strip().lower()
    if distance not in {'very_close', 'mid', 'far'}:
        distance = 'mid'
    normalized_confidence = max(0.0, min(1.0, float(confidence)))

    observability = await runtime.session_store.get_observability(sid)
    session_state = await runtime.session_store.ensure_session(sid)
    edge_reflex_active = await runtime.session_store.is_edge_hazard_active(sid)
    decision = evaluate_safety_policy(
        SafetyPolicyInput(
            hazard_confidence=normalized_confidence,
            distance_category=distance,  # type: ignore[arg-type]
            motion_state=session_state.motion_state,
            sensor_health_score=float(observability.get('sensor_health_score', 1.0) or 1.0),
            localization_uncertainty_m=float(observability.get('localization_uncertainty_m', 120.0) or 120.0),
            edge_reflex_active=edge_reflex_active,
        )
    )
    await runtime.session_store.update_observability(sid, safety_tier=decision.tier)

    emitted_at_ms = int(time() * 1000)
    emitted_at = datetime.now(timezone.utc).isoformat()

    payload = {
        'type': 'HARD_STOP',
        'position_x': max(-1.0, min(1.0, position_x)),
        'distance': distance,
        'hazard_type': normalized_hazard,
        'confidence': normalized_confidence,
        'server_emit_ts': emitted_at,
        'server_emit_ts_ms': emitted_at_ms,
        'safety_tier': decision.tier,
        'safety_reason': decision.reason,
    }

    emitted_hard_stop = False
    if decision.should_emit_hard_stop:
        await runtime.websocket_registry.emit_hard_stop(sid, payload)
        emitted_hard_stop = True
    elif decision.tier == 'voice':
        await runtime.websocket_registry.send_live(
            sid,
            {
                'type': 'assistant_text',
                'session_id': sid,
                'timestamp': emitted_at,
                'text': f'Caution: {normalized_hazard} {distance}.',
            },
        )
    elif decision.tier == 'ping':
        await runtime.websocket_registry.send_live(
            sid,
            {
                'type': 'semantic_cue',
                'cue': 'soft_obstacle',
                'position_x': payload['position_x'],
            },
        )

    human_help_link = ''
    if decision.should_escalate_human:
        manager = runtime.human_fallback_manager
        if manager is not None:
            human_help_link = manager.build_help_link(sid, reason='safety_degraded')
            await manager.notify_contacts(human_help_link, reason='safety_degraded')
        else:
            human_help_link = await runtime.session_store.build_human_help_link(sid)
        await runtime.websocket_registry.send_live(
            sid,
            {
                'type': 'assistant_text',
                'session_id': sid,
                'timestamp': emitted_at,
                'text': f'Safety degraded. Human help: {human_help_link}',
            },
        )

    await runtime.session_store.log_hazard(
        sid,
        normalized_hazard,
        payload['position_x'],
        distance,
        normalized_confidence,
        description,
    )

    return {
        'ok': True,
        'hazard_type': normalized_hazard,
        'distance': distance,
        'position_x': payload['position_x'],
        'confidence': normalized_confidence,
        'safety_tier': decision.tier,
        'safety_reason': decision.reason,
        'hard_stop_emitted': emitted_hard_stop,
        'human_help_link': human_help_link,
    }
