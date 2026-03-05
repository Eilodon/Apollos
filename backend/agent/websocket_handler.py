from __future__ import annotations

import asyncio
import time
from dataclasses import dataclass, field
from typing import Any, Protocol

try:
    from fastapi import WebSocket
except Exception:
    class WebSocket(Protocol):
        async def send_json(self, payload: dict[str, Any]) -> None:
            ...


class WebSocketRegistry:
    @dataclass(slots=True)
    class _ManagedSocket:
        ws: WebSocket
        client_id: str | None = None
        connected_epoch: float = field(default_factory=time.time)

    def __init__(self) -> None:
        self._live: dict[str, WebSocketRegistry._ManagedSocket] = {}
        self._emergency: dict[str, WebSocketRegistry._ManagedSocket] = {}
        self._lock = asyncio.Lock()

    async def register_live(self, session_id: str, ws: WebSocket, client_id: str | None = None) -> tuple[bool, str]:
        async with self._lock:
            active = self._live.get(session_id)
            if (
                active is not None
                and active.ws is not ws
                and active.client_id
                and active.client_id != client_id
            ):
                return False, 'live session already owned by another client'
            self._live[session_id] = self._ManagedSocket(ws=ws, client_id=client_id)
        return True, ''

    async def register_emergency(self, session_id: str, ws: WebSocket, client_id: str | None = None) -> tuple[bool, str]:
        async with self._lock:
            live = self._live.get(session_id)
            if live is not None and live.client_id and client_id != live.client_id:
                return False, 'emergency channel client mismatch'

            active = self._emergency.get(session_id)
            if (
                active is not None
                and active.ws is not ws
                and active.client_id
                and active.client_id != client_id
            ):
                return False, 'emergency channel already owned by another client'

            self._emergency[session_id] = self._ManagedSocket(ws=ws, client_id=client_id)
        return True, ''

    async def unregister_live(self, session_id: str, ws: WebSocket | None = None) -> None:
        async with self._lock:
            active = self._live.get(session_id)
            if active and (ws is None or active.ws is ws):
                self._live.pop(session_id, None)

    async def unregister_emergency(self, session_id: str, ws: WebSocket | None = None) -> None:
        async with self._lock:
            active = self._emergency.get(session_id)
            if active and (ws is None or active.ws is ws):
                self._emergency.pop(session_id, None)

    async def send_live(self, session_id: str, payload: dict[str, Any]) -> bool:
        managed = self._live.get(session_id)
        if not managed:
            return False

        try:
            await managed.ws.send_json(payload)
            return True
        except Exception:
            await self.unregister_live(session_id, managed.ws)
            return False

    async def send_emergency(self, session_id: str, payload: dict[str, Any]) -> bool:
        managed = self._emergency.get(session_id)
        if not managed:
            return False

        try:
            await managed.ws.send_json(payload)
            return True
        except Exception:
            await self.unregister_emergency(session_id, managed.ws)
            return False

    async def emit_hard_stop(self, session_id: str, payload: dict[str, Any]) -> None:
        delivered = await self.send_emergency(session_id, payload)
        if delivered:
            return

        # Fallback to live channel if emergency channel is unavailable.
        await self.send_live(session_id, payload)
