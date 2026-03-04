from __future__ import annotations

from .decorators import tool
from .runtime import get_current_session, get_runtime


@tool
async def request_human_help() -> str:
    runtime = get_runtime()
    session_id = get_current_session()
    return await runtime.session_store.build_human_help_link(session_id)
