from __future__ import annotations

from .decorators import tool
from .runtime import get_current_session, get_runtime

VALID_MODES = {'NAVIGATION', 'EXPLORE', 'READ', 'QUIET'}


@tool
async def set_navigation_mode(mode: str) -> str:
    runtime = get_runtime()
    session_id = get_current_session()

    normalized = mode.strip().upper()
    if normalized not in VALID_MODES:
        return f'Invalid mode: {mode}'

    await runtime.session_store.set_mode(session_id, normalized)
    return f'Mode switched to {normalized}'
