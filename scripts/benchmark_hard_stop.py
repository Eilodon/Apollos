#!/usr/bin/env python3
from __future__ import annotations

import argparse
import asyncio
import json
import statistics
import sys
import time
import uuid
from dataclasses import dataclass
from typing import Any

import httpx
import websockets


@dataclass(slots=True)
class IterationResult:
    idx: int
    trigger_client_ts_ms: float
    server_emit_ts_ms: float | None
    receive_client_ts_ms: float
    e2e_ms: float
    trigger_to_emit_ms: float | None
    emit_to_receive_ms: float | None
    request_status: int


def percentile(values: list[float], p: float) -> float:
    if not values:
        return 0.0
    if len(values) == 1:
        return values[0]
    k = (len(values) - 1) * p
    f = int(k)
    c = min(f + 1, len(values) - 1)
    if f == c:
        return values[f]
    return values[f] + (values[c] - values[f]) * (k - f)


async def receive_hard_stop(ws: websockets.WebSocketClientProtocol, timeout_s: float) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_s
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError('Timed out waiting for HARD_STOP event.')

        raw = await asyncio.wait_for(ws.recv(), timeout=remaining)
        payload = json.loads(raw)
        if payload.get('type') == 'HARD_STOP':
            return payload


async def run(args: argparse.Namespace) -> int:
    session_id = args.session_id or f'bench-{uuid.uuid4()}'
    ws_url = f"{args.ws_base.rstrip('/')}/ws/emergency/{session_id}"
    hazard_url = f"{args.http_base.rstrip('/')}/dev/hazard/{session_id}"

    results: list[IterationResult] = []

    async with websockets.connect(ws_url, max_size=2**20) as ws, httpx.AsyncClient(timeout=args.http_timeout) as client:
        await ws.send(json.dumps({'type': 'heartbeat'}))

        for idx in range(1, args.iterations + 1):
            t0_perf = time.perf_counter()
            t0_epoch_ms = time.time() * 1000

            response = await client.post(
                hazard_url,
                json={
                    'hazard_type': args.hazard_type,
                    'position_x': args.position_x,
                    'distance': args.distance,
                    'confidence': args.confidence,
                    'description': f'benchmark iteration {idx}',
                },
            )
            event = await receive_hard_stop(ws, timeout_s=args.event_timeout)

            t1_perf = time.perf_counter()
            t1_epoch_ms = time.time() * 1000

            e2e_ms = (t1_perf - t0_perf) * 1000
            emit_ts_ms = event.get('server_emit_ts_ms')
            server_emit_ts_ms = float(emit_ts_ms) if emit_ts_ms is not None else None
            trigger_to_emit_ms = (server_emit_ts_ms - t0_epoch_ms) if server_emit_ts_ms is not None else None
            if trigger_to_emit_ms is not None and trigger_to_emit_ms < 0:
                trigger_to_emit_ms = 0.0
            emit_to_receive_ms = (t1_epoch_ms - server_emit_ts_ms) if server_emit_ts_ms is not None else None

            results.append(
                IterationResult(
                    idx=idx,
                    trigger_client_ts_ms=t0_epoch_ms,
                    server_emit_ts_ms=server_emit_ts_ms,
                    receive_client_ts_ms=t1_epoch_ms,
                    e2e_ms=e2e_ms,
                    trigger_to_emit_ms=trigger_to_emit_ms,
                    emit_to_receive_ms=emit_to_receive_ms,
                    request_status=response.status_code,
                )
            )

            print(
                f"[{idx:02d}] status={response.status_code} "
                f"trigger_ms={t0_epoch_ms:.0f} "
                f"server_emit_ms={(f'{server_emit_ts_ms:.0f}' if server_emit_ts_ms is not None else 'NA')} "
                f"recv_ms={t1_epoch_ms:.0f} "
                f"e2e={e2e_ms:.2f}ms"
                + (f" trigger->emit={trigger_to_emit_ms:.2f}ms" if trigger_to_emit_ms is not None else '')
                + (f" emit->recv={emit_to_receive_ms:.2f}ms" if emit_to_receive_ms is not None else '')
            )

            await asyncio.sleep(args.sleep_ms / 1000.0)

    e2e_values = [item.e2e_ms for item in results]
    t2e_values = [item.trigger_to_emit_ms for item in results if item.trigger_to_emit_ms is not None]
    e2r_values = [item.emit_to_receive_ms for item in results if item.emit_to_receive_ms is not None]

    p50 = percentile(sorted(e2e_values), 0.50)
    p95 = percentile(sorted(e2e_values), 0.95)
    avg = statistics.mean(e2e_values)

    print('\n=== HARD_STOP Benchmark Summary ===')
    print(f'session_id: {session_id}')
    print(f'iterations: {len(results)}')
    print(f'e2e avg: {avg:.2f}ms')
    print(f'e2e p50: {p50:.2f}ms')
    print(f'e2e p95: {p95:.2f}ms')

    if t2e_values:
        print(f'trigger->server_emit avg: {statistics.mean(t2e_values):.2f}ms')
        print(f'trigger->server_emit p95: {percentile(sorted(t2e_values), 0.95):.2f}ms')

    if e2r_values:
        print(f'server_emit->receive avg: {statistics.mean(e2r_values):.2f}ms')
        print(f'server_emit->receive p95: {percentile(sorted(e2r_values), 0.95):.2f}ms')

    failures = [item for item in results if item.request_status >= 400]
    if failures:
        print(f'FAIL: {len(failures)} HTTP request(s) failed.')
        return 1

    if p95 > args.budget_ms:
        print(f'FAIL: e2e p95 {p95:.2f}ms > budget {args.budget_ms:.2f}ms')
        return 2

    print(f'PASS: e2e p95 {p95:.2f}ms <= budget {args.budget_ms:.2f}ms')
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description='Benchmark HARD_STOP latency over emergency websocket.')
    parser.add_argument('--http-base', default='http://127.0.0.1:8000', help='Backend HTTP base URL')
    parser.add_argument('--ws-base', default='ws://127.0.0.1:8000', help='Backend WS base URL')
    parser.add_argument('--session-id', default='', help='Session ID (autogenerated when empty)')
    parser.add_argument('--iterations', type=int, default=20, help='Number of benchmark iterations')
    parser.add_argument('--budget-ms', type=float, default=100.0, help='Latency budget for p95')
    parser.add_argument('--sleep-ms', type=int, default=120, help='Pause between iterations')
    parser.add_argument('--event-timeout', type=float, default=5.0, help='Timeout waiting for HARD_STOP event')
    parser.add_argument('--http-timeout', type=float, default=5.0, help='HTTP timeout for hazard trigger')
    parser.add_argument('--hazard-type', default='benchmark_drop')
    parser.add_argument('--position-x', type=float, default=0.8)
    parser.add_argument('--distance', choices=['very_close', 'mid', 'far'], default='very_close')
    parser.add_argument('--confidence', type=float, default=0.95)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    try:
        exit_code = asyncio.run(run(args))
    except KeyboardInterrupt:
        exit_code = 130
    except Exception as exc:
        print(f'ERROR: {exc}')
        exit_code = 1
    raise SystemExit(exit_code)


if __name__ == '__main__':
    main()
