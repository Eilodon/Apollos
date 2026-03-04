from __future__ import annotations

from contextvars import ContextVar, Token
from dataclasses import dataclass

from ..session_manager import SessionStore
from ..websocket_handler import WebSocketRegistry


@dataclass(slots=True)
class ToolRuntime:
    session_store: SessionStore
    websocket_registry: WebSocketRegistry


_runtime: ToolRuntime | None = None
_current_session: ContextVar[str | None] = ContextVar('current_session', default=None)


def configure_runtime(session_store: SessionStore, websocket_registry: WebSocketRegistry) -> None:
    global _runtime
    _runtime = ToolRuntime(session_store=session_store, websocket_registry=websocket_registry)


def get_runtime() -> ToolRuntime:
    if _runtime is None:
        raise RuntimeError('Tool runtime has not been configured yet.')
    return _runtime


def set_current_session(session_id: str) -> Token[str | None]:
    return _current_session.set(session_id)


def reset_current_session(token: Token[str | None]) -> None:
    _current_session.reset(token)


def get_current_session() -> str:
    session_id = _current_session.get()
    if not session_id:
        raise RuntimeError('No active session in tool runtime context.')
    return session_id
