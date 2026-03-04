from __future__ import annotations

import asyncio
from typing import Any, Protocol

try:
    from fastapi import WebSocket
except Exception:
    class WebSocket(Protocol):
        async def send_json(self, payload: dict[str, Any]) -> None:
            ...


class WebSocketRegistry:
    def __init__(self) -> None:
        self._live: dict[str, WebSocket] = {}
        self._emergency: dict[str, WebSocket] = {}
        self._lock = asyncio.Lock()

    async def register_live(self, session_id: str, ws: WebSocket) -> None:
        async with self._lock:
            self._live[session_id] = ws

    async def register_emergency(self, session_id: str, ws: WebSocket) -> None:
        async with self._lock:
            self._emergency[session_id] = ws

    async def unregister_live(self, session_id: str, ws: WebSocket | None = None) -> None:
        async with self._lock:
            active = self._live.get(session_id)
            if active and (ws is None or active is ws):
                self._live.pop(session_id, None)

    async def unregister_emergency(self, session_id: str, ws: WebSocket | None = None) -> None:
        async with self._lock:
            active = self._emergency.get(session_id)
            if active and (ws is None or active is ws):
                self._emergency.pop(session_id, None)

    async def send_live(self, session_id: str, payload: dict[str, Any]) -> bool:
        ws = self._live.get(session_id)
        if not ws:
            return False

        try:
            await ws.send_json(payload)
            return True
        except Exception:
            await self.unregister_live(session_id, ws)
            return False

    async def send_emergency(self, session_id: str, payload: dict[str, Any]) -> bool:
        ws = self._emergency.get(session_id)
        if not ws:
            return False

        try:
            await ws.send_json(payload)
            return True
        except Exception:
            await self.unregister_emergency(session_id, ws)
            return False

    async def emit_hard_stop(self, session_id: str, payload: dict[str, Any]) -> None:
        delivered = await self.send_emergency(session_id, payload)
        if delivered:
            return

        # Fallback to live channel if emergency channel is unavailable.
        await self.send_live(session_id, payload)
