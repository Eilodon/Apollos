from __future__ import annotations

from .decorators import tool
from .runtime import get_current_session, get_runtime

DISTRESS_STATES = {'stressed', 'fearful', 'panicked'}


@tool
async def escalate_mode_if_stressed(
    state: str,
    confidence: float,
    current_mode: str,
) -> dict[str, object]:
    runtime = get_runtime()
    session_id = get_current_session()

    normalized_state = state.strip().lower()
    normalized_mode = current_mode.strip().upper()
    if normalized_state not in DISTRESS_STATES or confidence <= 0.7:
        return {'action': 'none'}

    if normalized_mode not in {'QUIET', 'EXPLORE'}:
        return {'action': 'none'}

    await runtime.session_store.apply_stress_mode_override(
        session_id=session_id,
        reason='vocal_distress_detected',
        revert_after_seconds=120,
    )
    await runtime.session_store.log_emotion(session_id, state=normalized_state, confidence=confidence)

    return {
        'action': 'set_mode',
        'new_mode': 'NAVIGATION',
        'reason': 'vocal_distress_detected',
        'revert_after_seconds': 120,
    }
