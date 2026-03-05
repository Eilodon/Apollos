#!/usr/bin/env python3
"""Regression checks for edge floor-drop heuristics.

Cases mirror Wave 2 false-positive scenarios:
- flickering neon sign
- fast motorcycle pass-by
- rain on camera lens
"""

from __future__ import annotations

from dataclasses import dataclass
import sys

FLOOR_DROP_BOTTOM_THRESHOLD = 60.0
FLOOR_DROP_TOP_MAX = 20.0
FLICKER_SUPPRESS_LUMA_DELTA = 28.0


@dataclass(frozen=True)
class ReflexCase:
    name: str
    top_diff: float
    bottom_diff: float
    luma_delta: float
    expected_drop: bool


def detect_floor_drop(top_diff: float, bottom_diff: float, luma_delta: float) -> bool:
    # Neon-like frame-wide flicker should be suppressed.
    if luma_delta > FLICKER_SUPPRESS_LUMA_DELTA and top_diff > 18.0:
        return False
    return (
        bottom_diff >= FLOOR_DROP_BOTTOM_THRESHOLD
        and top_diff <= FLOOR_DROP_TOP_MAX
        and bottom_diff > top_diff * 2.4
    )


def run() -> int:
    cases = [
        ReflexCase('flickering_neon_sign', top_diff=24, bottom_diff=36, luma_delta=41, expected_drop=False),
        ReflexCase('fast_motorcycle_passby', top_diff=29, bottom_diff=44, luma_delta=8, expected_drop=False),
        ReflexCase('rain_on_lens', top_diff=27, bottom_diff=49, luma_delta=16, expected_drop=False),
        ReflexCase('actual_floor_drop', top_diff=12, bottom_diff=76, luma_delta=9, expected_drop=True),
        ReflexCase('low_curb_noise', top_diff=14, bottom_diff=39, luma_delta=6, expected_drop=False),
    ]

    failed: list[str] = []
    for case in cases:
        actual = detect_floor_drop(case.top_diff, case.bottom_diff, case.luma_delta)
        status = 'PASS' if actual == case.expected_drop else 'FAIL'
        print(
            f'[{status}] {case.name}: expected={case.expected_drop} actual={actual}'
            f' top={case.top_diff:.1f} bottom={case.bottom_diff:.1f} luma={case.luma_delta:.1f}'
        )
        if actual != case.expected_drop:
            failed.append(case.name)

    if failed:
        print(f'\nRegression check failed for {len(failed)} case(s): {", ".join(failed)}', file=sys.stderr)
        return 1

    print(f'\nAll {len(cases)} reflex regression cases passed.')
    return 0


if __name__ == '__main__':
    raise SystemExit(run())
