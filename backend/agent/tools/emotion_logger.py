from __future__ import annotations

from .decorators import tool
from .runtime import get_current_session, get_runtime


@tool
async def log_emotion_event(state: str, confidence: float) -> str:
    runtime = get_runtime()
    session_id = get_current_session()

    await runtime.session_store.log_emotion(session_id, state=state, confidence=confidence)
    return f'Emotion logged: {state} ({confidence:.2f})'
