#!/usr/bin/env python3
from __future__ import annotations

import argparse
import asyncio
import pathlib
import statistics
import sys
import time
from dataclasses import dataclass

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'backend'))

from agent.session_manager import SessionStore
from agent.tools.hazard_logger import log_hazard_event
from agent.tools.runtime import configure_runtime, reset_current_session, set_current_session
from agent.websocket_handler import WebSocketRegistry


@dataclass(slots=True)
class Sample:
    idx: int
    latency_ms: float
    trigger_client_ts_ms: float
    server_emit_ts_ms: float | None
    receive_client_ts_ms: float
    trigger_to_emit_ms: float | None
    emit_to_receive_ms: float | None


class FakeEmergencySocket:
    def __init__(self) -> None:
        self.messages: list[dict] = []

    async def send_json(self, payload: dict) -> None:
        self.messages.append(payload)


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


async def run(iterations: int, budget_ms: float) -> int:
    store = SessionStore(use_firestore=False)
    registry = WebSocketRegistry()
    configure_runtime(session_store=store, websocket_registry=registry)

    session_id = 'internal-bench-session'
    fake_ws = FakeEmergencySocket()
    await registry.register_emergency(session_id, fake_ws)

    samples: list[Sample] = []

    for idx in range(1, iterations + 1):
        token = set_current_session(session_id)
        t0_epoch_ms = time.time() * 1000
        t0 = time.perf_counter()
        try:
            await log_hazard_event(
                hazard_type='internal_bench_hazard',
                position_x=0.7,
                distance_category='very_close',
                confidence=0.99,
                description=f'internal bench iteration {idx}',
                session_id=session_id,
            )
        finally:
            reset_current_session(token)
        t1 = time.perf_counter()
        t1_epoch_ms = time.time() * 1000

        latency = (t1 - t0) * 1000
        payload = fake_ws.messages[-1] if fake_ws.messages else {}
        server_emit_raw = payload.get('server_emit_ts_ms')
        server_emit_ts_ms = float(server_emit_raw) if server_emit_raw is not None else None
        trigger_to_emit_ms = (server_emit_ts_ms - t0_epoch_ms) if server_emit_ts_ms is not None else None
        if trigger_to_emit_ms is not None and trigger_to_emit_ms < 0:
            trigger_to_emit_ms = 0.0
        emit_to_receive_ms = (t1_epoch_ms - server_emit_ts_ms) if server_emit_ts_ms is not None else None

        samples.append(
            Sample(
                idx=idx,
                latency_ms=latency,
                trigger_client_ts_ms=t0_epoch_ms,
                server_emit_ts_ms=server_emit_ts_ms,
                receive_client_ts_ms=t1_epoch_ms,
                trigger_to_emit_ms=trigger_to_emit_ms,
                emit_to_receive_ms=emit_to_receive_ms,
            )
        )
        print(
            f"[{idx:02d}] e2e={latency:.3f}ms "
            f"trigger_ms={t0_epoch_ms:.0f} "
            f"server_emit_ms={(f'{server_emit_ts_ms:.0f}' if server_emit_ts_ms is not None else 'NA')} "
            f"recv_ms={t1_epoch_ms:.0f}"
            + (f" trigger->emit={trigger_to_emit_ms:.3f}ms" if trigger_to_emit_ms is not None else '')
            + (f" emit->recv={emit_to_receive_ms:.3f}ms" if emit_to_receive_ms is not None else '')
        )

    values = [item.latency_ms for item in samples]
    t2e_values = [item.trigger_to_emit_ms for item in samples if item.trigger_to_emit_ms is not None]
    e2r_values = [item.emit_to_receive_ms for item in samples if item.emit_to_receive_ms is not None]
    p50 = percentile(sorted(values), 0.5)
    p95 = percentile(sorted(values), 0.95)
    avg = statistics.mean(values)

    print('\n=== Internal HARD_STOP Benchmark Summary ===')
    print(f'iterations: {iterations}')
    print(f'avg: {avg:.3f}ms')
    print(f'p50: {p50:.3f}ms')
    print(f'p95: {p95:.3f}ms')
    if t2e_values:
        print(f'trigger->server_emit avg: {statistics.mean(t2e_values):.3f}ms')
        print(f'trigger->server_emit p95: {percentile(sorted(t2e_values), 0.95):.3f}ms')
    if e2r_values:
        print(f'server_emit->receive avg: {statistics.mean(e2r_values):.3f}ms')
        print(f'server_emit->receive p95: {percentile(sorted(e2r_values), 0.95):.3f}ms')

    if p95 > budget_ms:
        print(f'FAIL: p95 {p95:.3f}ms > budget {budget_ms:.3f}ms')
        return 1

    print(f'PASS: p95 {p95:.3f}ms <= budget {budget_ms:.3f}ms')
    return 0


def main() -> None:
    parser = argparse.ArgumentParser(description='Internal benchmark for HARD_STOP emit latency.')
    parser.add_argument('--iterations', type=int, default=100)
    parser.add_argument('--budget-ms', type=float, default=100.0)
    args = parser.parse_args()

    exit_code = asyncio.run(run(args.iterations, args.budget_ms))
    raise SystemExit(exit_code)


if __name__ == '__main__':
    main()
