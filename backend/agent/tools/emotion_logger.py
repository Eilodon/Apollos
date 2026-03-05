from __future__ import annotations

from .decorators import tool
from .runtime import get_current_session, get_runtime


@tool
async def log_emotion_event(state: str, confidence: float) -> str:
    runtime = get_runtime()
    session_id = get_current_session()

    await runtime.session_store.log_emotion(session_id, state=state, confidence=confidence)
    normalized_state = state.strip().lower()
    if normalized_state in {'stressed', 'fearful', 'panicked'} and confidence > 0.7:
        mode = await runtime.session_store.get_effective_mode(session_id)
        if mode in {'QUIET', 'EXPLORE'}:
            await runtime.session_store.apply_stress_mode_override(
                session_id=session_id,
                reason='emotion_logger_auto_override',
                revert_after_seconds=120,
            )
    return f'Emotion logged: {state} ({confidence:.2f})'
