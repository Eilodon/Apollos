#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run_step(name: str, cmd: list[str], env: dict[str, str]) -> int:
    print(f'\n== {name} ==')
    print(' '.join(cmd))
    completed = subprocess.run(cmd, cwd=ROOT, env=env)
    if completed.returncode != 0:
        print(f'FAIL: {name} exited with {completed.returncode}')
    else:
        print(f'PASS: {name}')
    return completed.returncode


def resolve_python() -> str:
    venv_python = ROOT / 'backend' / '.venv' / 'bin' / 'python'
    if venv_python.exists():
        return str(venv_python)
    return sys.executable


def main() -> None:
    parser = argparse.ArgumentParser(description='Run hardening checks for VisionGPT backend/frontend contracts.')
    parser.add_argument('--asgi', action='store_true', help='Run in-process ASGI reconnect + HARD_STOP benchmark')
    parser.add_argument('--integration', action='store_true', help='Run external reconnect + HARD_STOP benchmark against running backend URL')
    parser.add_argument('--budget-ms', type=float, default=100.0, help='Latency budget for HARD_STOP benchmark p95')
    parser.add_argument('--iterations', type=int, default=20, help='Benchmark iterations')
    args = parser.parse_args()

    env = os.environ.copy()
    env.setdefault('PYTHONPATH', 'backend')
    py = resolve_python()

    failures = 0
    failures += run_step('Backend unit tests', [py, '-m', 'unittest', 'discover', 'backend/tests'], env)
    failures += run_step('AEC static check', [py, 'scripts/check_aec_config.py'], env)
    failures += run_step(
        'Internal HARD_STOP benchmark',
        [
            py,
            'scripts/benchmark_hard_stop_internal.py',
            '--iterations',
            str(args.iterations),
            '--budget-ms',
            str(args.budget_ms),
        ],
        env,
    )
    failures += run_step('Internal reconnect test', [py, 'scripts/test_reconnect_internal.py'], env)

    if args.asgi:
        failures += run_step('Reconnect ASGI test', [py, 'scripts/test_reconnect_asgi.py'], env)
        failures += run_step(
            'ASGI HARD_STOP benchmark',
            [
                py,
                'scripts/benchmark_hard_stop_asgi.py',
                '--iterations',
                str(args.iterations),
                '--budget-ms',
                str(args.budget_ms),
            ],
            env,
        )
    else:
        print('\nSkipping ASGI in-process checks (pass --asgi to enable).')

    if args.integration:
        failures += run_step('Websocket reconnect test', [py, 'scripts/test_reconnect.py'], env)
        failures += run_step(
            'HARD_STOP latency benchmark',
            [
                py,
                'scripts/benchmark_hard_stop.py',
                '--iterations',
                str(args.iterations),
                '--budget-ms',
                str(args.budget_ms),
            ],
            env,
        )
    else:
        print('\nSkipping external socket integration checks (pass --integration to enable).')

    if failures:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
