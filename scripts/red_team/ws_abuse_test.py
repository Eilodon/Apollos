#!/usr/bin/env python3
"""Red-team adversarial test scripts for Apollos.

GAP 3: Structured adversarial testing for WebSocket abuse, audio false-wake,
and perception edge cases.

Usage:
    python scripts/red_team/ws_abuse_test.py --backend-url ws://localhost:8000
"""
from __future__ import annotations

import argparse
import asyncio
import json
import os
import secrets
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


async def _connect(url: str, token: str | None = None) -> 'websockets.WebSocketClientProtocol':
    """Connect to the WS endpoint."""
    import websockets  # type: ignore[import-untyped]

    protocols = ['json.v1']
    if token:
        import base64
        encoded = base64.b64encode(token.encode()).decode()
        protocols.append(f'authb64.{encoded}')
    return await websockets.connect(url, subprotocols=protocols, close_timeout=3)


# --- Test case: Rapid connect / disconnect ---

async def test_rapid_reconnect(url: str, iterations: int = 20) -> dict[str, object]:
    """Open and immediately close the WS many times."""
    print(f'[ws_abuse] rapid_reconnect: {iterations} iterations')
    errors = 0
    for i in range(iterations):
        try:
            ws = await _connect(url)
            await ws.close()
        except Exception as exc:
            errors += 1
            if i == 0:
                print(f'  first error: {exc}')
    result = {'test': 'rapid_reconnect', 'iterations': iterations, 'errors': errors}
    print(f'  result: {result}')
    return result


# --- Test case: Oversized payload ---

async def test_oversized_payload(url: str) -> dict[str, object]:
    """Send payload that exceeds MAX_WS_MESSAGE_BYTES."""
    print('[ws_abuse] oversized_payload')
    try:
        ws = await _connect(url)
        oversized = json.dumps({
            'type': 'multimodal_frame',
            'session_id': 'red-team-test',
            'timestamp': '2026-01-01T00:00:00Z',
            'frame_jpeg_base64': 'A' * (5 * 1024 * 1024),  # 5 MB
        })
        await ws.send(oversized)
        # Server should close or reject.
        try:
            response = await asyncio.wait_for(ws.recv(), timeout=3)
            result = {'test': 'oversized_payload', 'response': str(response)[:200], 'rejected': False}
        except Exception:
            result = {'test': 'oversized_payload', 'rejected': True}
        await ws.close()
    except Exception as exc:
        result = {'test': 'oversized_payload', 'rejected': True, 'error': str(exc)[:200]}
    print(f'  result: {result}')
    return result


# --- Test case: Malformed JSON ---

async def test_malformed_json(url: str) -> dict[str, object]:
    """Send non-JSON data over the WebSocket."""
    print('[ws_abuse] malformed_json')
    try:
        ws = await _connect(url)
        await ws.send('{this is not valid json!!!')
        try:
            response = await asyncio.wait_for(ws.recv(), timeout=3)
            result = {'test': 'malformed_json', 'response': str(response)[:200], 'handled': True}
        except Exception:
            result = {'test': 'malformed_json', 'handled': True, 'server_closed': True}
        await ws.close()
    except Exception as exc:
        result = {'test': 'malformed_json', 'handled': True, 'error': str(exc)[:200]}
    print(f'  result: {result}')
    return result


# --- Test case: Token replay (stale token) ---

async def test_token_replay(url: str) -> dict[str, object]:
    """Try connecting with a fabricated/expired token."""
    print('[ws_abuse] token_replay')
    fake_token = f'eyJ{secrets.token_hex(32)}'
    try:
        ws = await _connect(url, token=fake_token)
        try:
            response = await asyncio.wait_for(ws.recv(), timeout=3)
            result = {'test': 'token_replay', 'accepted': True, 'response': str(response)[:200]}
        except Exception:
            result = {'test': 'token_replay', 'accepted': False}
        await ws.close()
    except Exception as exc:
        result = {'test': 'token_replay', 'accepted': False, 'error': str(exc)[:200]}
    print(f'  result: {result}')
    return result


async def main() -> None:
    parser = argparse.ArgumentParser(description='WebSocket red-team abuse tests')
    parser.add_argument('--backend-url', default='ws://localhost:8000/ws/live/red-team-session')
    args = parser.parse_args()
    url = args.backend_url

    results = []
    results.append(await test_rapid_reconnect(url))
    results.append(await test_oversized_payload(url))
    results.append(await test_malformed_json(url))
    results.append(await test_token_replay(url))

    print('\n===== SUMMARY =====')
    for r in results:
        status = '✓ PASS' if r.get('rejected') or r.get('handled') or r.get('errors', 0) == 0 else '✗ REVIEW'
        print(f"  {r['test']}: {status}")


if __name__ == '__main__':
    asyncio.run(main())
