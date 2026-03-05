#!/usr/bin/env python3
from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys
from dataclasses import dataclass

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'backend'))

from agent.safety_policy import SafetyPolicyInput, evaluate_safety_policy


@dataclass(frozen=True, slots=True)
class PolicyCase:
    name: str
    payload: SafetyPolicyInput
    expect_hard_stop: bool


def build_policy_cases() -> list[PolicyCase]:
    return [
        PolicyCase(
            name='very_close_vehicle_fast_motion',
            payload=SafetyPolicyInput(
                hazard_confidence=0.94,
                distance_category='very_close',
                motion_state='walking_fast',
                sensor_health_score=0.82,
                localization_uncertainty_m=18,
                edge_reflex_active=False,
            ),
            expect_hard_stop=True,
        ),
        PolicyCase(
            name='mid_distance_construction_running',
            payload=SafetyPolicyInput(
                hazard_confidence=0.78,
                distance_category='mid',
                motion_state='running',
                sensor_health_score=0.76,
                localization_uncertainty_m=28,
                edge_reflex_active=False,
            ),
            expect_hard_stop=True,
        ),
        PolicyCase(
            name='low_confidence_far_static',
            payload=SafetyPolicyInput(
                hazard_confidence=0.24,
                distance_category='far',
                motion_state='stationary',
                sensor_health_score=0.92,
                localization_uncertainty_m=12,
                edge_reflex_active=False,
            ),
            expect_hard_stop=False,
        ),
        PolicyCase(
            name='uncertain_far_with_edge_reflex',
            payload=SafetyPolicyInput(
                hazard_confidence=0.31,
                distance_category='far',
                motion_state='walking_slow',
                sensor_health_score=0.44,
                localization_uncertainty_m=85,
                edge_reflex_active=True,
            ),
            expect_hard_stop=True,
        ),
        PolicyCase(
            name='very_close_low_confidence_safety_bias',
            payload=SafetyPolicyInput(
                hazard_confidence=0.48,
                distance_category='very_close',
                motion_state='walking_slow',
                sensor_health_score=0.90,
                localization_uncertainty_m=20,
                edge_reflex_active=False,
            ),
            expect_hard_stop=True,
        ),
        PolicyCase(
            name='very_close_moderate_confidence',
            payload=SafetyPolicyInput(
                hazard_confidence=0.62,
                distance_category='very_close',
                motion_state='walking_slow',
                sensor_health_score=0.90,
                localization_uncertainty_m=20,
                edge_reflex_active=False,
            ),
            expect_hard_stop=True,
        ),
    ]


def evaluate_policy_gate(
    *,
    min_recall: float,
    max_false_stop: float,
) -> int:
    cases = build_policy_cases()
    expected_positive = [case for case in cases if case.expect_hard_stop]
    expected_negative = [case for case in cases if not case.expect_hard_stop]

    tp = 0
    fp = 0
    for case in cases:
        decision = evaluate_safety_policy(case.payload)
        predicted = decision.should_emit_hard_stop
        if predicted and case.expect_hard_stop:
            tp += 1
        elif predicted and not case.expect_hard_stop:
            fp += 1
        print(
            f"[policy] {case.name}: tier={decision.tier} risk={decision.risk_score:.2f} "
            f"expected_hard_stop={int(case.expect_hard_stop)} predicted={int(predicted)}",
        )

    recall = tp / max(1, len(expected_positive))
    false_stop_rate = fp / max(1, len(expected_negative))
    print(f'[policy] recall={recall:.3f} (min {min_recall:.3f})')
    print(f'[policy] false_stop_rate={false_stop_rate:.3f} (max {max_false_stop:.3f})')

    if recall < min_recall:
        print('[policy] FAIL: recall regression detected')
        return 1
    if false_stop_rate > max_false_stop:
        print('[policy] FAIL: false-stop regression detected')
        return 2
    print('[policy] PASS')
    return 0


def run_latency_gate(*, iterations: int, budget_ms: float, python_bin: str) -> int:
    command = [
        python_bin,
        str(ROOT / 'scripts' / 'benchmark_hard_stop_internal.py'),
        '--iterations',
        str(iterations),
        '--budget-ms',
        str(budget_ms),
    ]
    print('[latency] running', ' '.join(command))
    completed = subprocess.run(command, cwd=ROOT)
    if completed.returncode != 0:
        print('[latency] FAIL')
    else:
        print('[latency] PASS')
    return completed.returncode


def main() -> None:
    parser = argparse.ArgumentParser(description='Safety regression gate for release blocking.')
    parser.add_argument('--budget-ms', type=float, default=100.0, help='HARD_STOP p95 latency budget')
    parser.add_argument('--iterations', type=int, default=20, help='Iterations for internal HARD_STOP benchmark')
    parser.add_argument('--min-recall', type=float, default=0.80, help='Minimum policy recall target')
    parser.add_argument('--max-false-stop', type=float, default=0.34, help='Maximum policy false-stop rate')
    parser.add_argument('--python-bin', default=sys.executable, help='Python executable for child benchmark process')
    args = parser.parse_args()

    policy_exit = evaluate_policy_gate(
        min_recall=args.min_recall,
        max_false_stop=args.max_false_stop,
    )
    if policy_exit != 0:
        raise SystemExit(policy_exit)

    latency_exit = run_latency_gate(
        iterations=args.iterations,
        budget_ms=args.budget_ms,
        python_bin=args.python_bin,
    )
    if latency_exit != 0:
        raise SystemExit(latency_exit)


if __name__ == '__main__':
    main()
