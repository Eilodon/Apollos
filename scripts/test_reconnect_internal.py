#!/usr/bin/env python3
from __future__ import annotations

import asyncio
import sys
import uuid
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'backend'))

from agent.aria_agent import AriaAgentOrchestrator  # noqa: E402
from agent.session_manager import SessionStore  # noqa: E402
from agent.websocket_handler import WebSocketRegistry  # noqa: E402


class FakeLiveSocket:
    def __init__(self) -> None:
        self.messages: list[dict[str, Any]] = []

    async def send_json(self, payload: dict[str, Any]) -> None:
        self.messages.append(payload)


def has_text(messages: list[dict[str, Any]], needle: str) -> bool:
    return any(msg.get('type') == 'assistant_text' and needle in str(msg.get('text', '')) for msg in messages)


async def run() -> int:
    session_id = f'internal-reconnect-{uuid.uuid4()}'
    store = SessionStore(use_firestore=False)
    registry = WebSocketRegistry()
    orchestrator = AriaAgentOrchestrator(session_store=store, websocket_registry=registry)

    first_socket = FakeLiveSocket()
    await registry.register_live(session_id, first_socket)
    await orchestrator.handle_client_message(
        session_id,
        {
            'type': 'user_command',
            'session_id': session_id,
            'timestamp': '2026-03-05T00:00:00Z',
            'command': 'request_human_help',
        },
    )
    if not has_text(first_socket.messages, 'Human help requested'):
        print(f'FAIL: expected human-help assistant_text, got: {first_socket.messages}')
        return 1

    await registry.unregister_live(session_id, first_socket)
    await orchestrator.close_session(session_id)

    second_socket = FakeLiveSocket()
    await registry.register_live(session_id, second_socket)
    await orchestrator.handle_client_message(
        session_id,
        {
            'type': 'user_command',
            'session_id': session_id,
            'timestamp': '2026-03-05T00:00:00Z',
            'command': 'describe_detailed',
        },
    )
    if not has_text(second_socket.messages, 'Context snapshot:'):
        print(f'FAIL: expected context snapshot after reconnect, got: {second_socket.messages}')
        return 2

    await registry.unregister_live(session_id, second_socket)
    await orchestrator.close_session(session_id)
    await orchestrator.shutdown()

    print('PASS: internal reconnect flow succeeded.')
    return 0


def main() -> None:
    raise SystemExit(asyncio.run(run()))


if __name__ == '__main__':
    main()
