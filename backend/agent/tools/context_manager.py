from __future__ import annotations

from .decorators import tool
from .runtime import get_current_session, get_runtime


@tool
async def get_context_summary() -> str:
    runtime = get_runtime()
    session_id = get_current_session()
    return await runtime.session_store.get_context_summary(session_id)
