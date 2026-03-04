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


def main() -> None:
    parser = argparse.ArgumentParser(description='Run hardening checks for VisionGPT backend/frontend contracts.')
    parser.add_argument('--integration', action='store_true', help='Run reconnect + HARD_STOP benchmark against running backend')
    parser.add_argument('--budget-ms', type=float, default=100.0, help='Latency budget for HARD_STOP benchmark p95')
    parser.add_argument('--iterations', type=int, default=20, help='Benchmark iterations')
    args = parser.parse_args()

    env = os.environ.copy()
    env.setdefault('PYTHONPATH', 'backend')

    failures = 0
    failures += run_step('Backend unit tests', ['python3', '-m', 'unittest', 'discover', 'backend/tests'], env)
    failures += run_step('AEC static check', ['python3', 'scripts/check_aec_config.py'], env)
    failures += run_step(
        'Internal HARD_STOP benchmark',
        [
            'python3',
            'scripts/benchmark_hard_stop_internal.py',
            '--iterations',
            str(args.iterations),
            '--budget-ms',
            str(args.budget_ms),
        ],
        env,
    )

    if args.integration:
        failures += run_step('Websocket reconnect test', ['python3', 'scripts/test_reconnect.py'], env)
        failures += run_step(
            'HARD_STOP latency benchmark',
            [
                'python3',
                'scripts/benchmark_hard_stop.py',
                '--iterations',
                str(args.iterations),
                '--budget-ms',
                str(args.budget_ms),
            ],
            env,
        )
    else:
        print('\nSkipping integration checks (pass --integration to enable).')

    if failures:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
