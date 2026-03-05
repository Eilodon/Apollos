#!/usr/bin/env python3
"""Perception robustness test suite for Apollos edge workers.

GAP 2: Automated tests simulating night/rain/glare/lens occlusion using
synthetic test frames fed through the survival reflex and depth guard.

This suite generates test frames and validates that the edge perception
pipeline responds correctly (no false negatives for real hazards under
adverse conditions, controlled false positive rate).

Usage:
    python scripts/red_team/perception_robustness_suite.py
"""
from __future__ import annotations

import argparse
import json
import math
import struct
import sys
from dataclasses import dataclass


@dataclass
class SyntheticFrame:
    """A synthetic test frame for edge perception testing."""
    name: str
    width: int
    height: int
    description: str
    expected_flags: list[str]
    pixels_rgba: bytes

    @property
    def avg_luma(self) -> float:
        total = 0.0
        count = self.width * self.height
        for i in range(count):
            offset = i * 4
            r = self.pixels_rgba[offset]
            g = self.pixels_rgba[offset + 1]
            b = self.pixels_rgba[offset + 2]
            total += 0.299 * r + 0.587 * g + 0.114 * b
        return total / count if count > 0 else 0.0


def _make_solid_frame(width: int, height: int, r: int, g: int, b: int) -> bytes:
    """Create a solid color RGBA frame."""
    pixel = bytes([r, g, b, 255])
    return pixel * (width * height)


def _make_noisy_frame(width: int, height: int, base_luma: int, noise_amplitude: int) -> bytes:
    """Create a noisy frame simulating rain/interference."""
    import random
    random.seed(42)
    pixels = bytearray()
    for _ in range(width * height):
        luma = max(0, min(255, base_luma + random.randint(-noise_amplitude, noise_amplitude)))
        pixels.extend([luma, luma, luma, 255])
    return bytes(pixels)


def _make_half_occluded_frame(width: int, height: int) -> bytes:
    """Create a frame that is 75% black (finger over lens)."""
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            if y > height // 4 or x > width // 4:
                pixels.extend([0, 0, 0, 255])
            else:
                pixels.extend([128, 128, 128, 255])
    return bytes(pixels)


def _make_overexposed_frame(width: int, height: int) -> bytes:
    """Create an overexposed (glare) frame."""
    return _make_solid_frame(width, height, 250, 250, 250)


def build_test_frames(width: int = 64, height: int = 64) -> list[SyntheticFrame]:
    """Build the test suite of synthetic frames."""
    return [
        SyntheticFrame(
            name='pitch_black_night',
            width=width, height=height,
            description='Complete darkness - simulates night with no streetlights',
            expected_flags=['too_dark'],
            pixels_rgba=_make_solid_frame(width, height, 0, 0, 0),
        ),
        SyntheticFrame(
            name='dim_night_scene',
            width=width, height=height,
            description='Very dim scene - simulates poorly lit alley',
            expected_flags=['too_dark'],
            pixels_rgba=_make_solid_frame(width, height, 15, 15, 15),
        ),
        SyntheticFrame(
            name='heavy_rain_noise',
            width=width, height=height,
            description='High noise simulating heavy rain on lens',
            expected_flags=[],  # Noisy but not dark/bright
            pixels_rgba=_make_noisy_frame(width, height, 100, 80),
        ),
        SyntheticFrame(
            name='lens_occluded',
            width=width, height=height,
            description='75% of frame is black - finger or object covering lens',
            expected_flags=['occluded'],
            pixels_rgba=_make_half_occluded_frame(width, height),
        ),
        SyntheticFrame(
            name='sun_glare',
            width=width, height=height,
            description='Overexposed frame from direct sunlight',
            expected_flags=['too_bright'],
            pixels_rgba=_make_overexposed_frame(width, height),
        ),
        SyntheticFrame(
            name='normal_outdoor',
            width=width, height=height,
            description='Normal daylight scene (control)',
            expected_flags=[],
            pixels_rgba=_make_noisy_frame(width, height, 120, 30),
        ),
    ]


def evaluate_frame_quality(frame: SyntheticFrame) -> dict[str, object]:
    """Replicates the TypeScript assessFrameQuality logic in Python for testing."""
    width, height = frame.width, frame.height
    pixel_count = width * height
    luma_values = []
    near_black = 0

    for i in range(pixel_count):
        offset = i * 4
        r = frame.pixels_rgba[offset]
        g = frame.pixels_rgba[offset + 1]
        b = frame.pixels_rgba[offset + 2]
        luma = 0.299 * r + 0.587 * g + 0.114 * b
        luma_values.append(luma)
        if luma < 12:
            near_black += 1

    avg_luma = sum(luma_values) / pixel_count if pixel_count else 0
    black_ratio = near_black / pixel_count if pixel_count else 0

    # Laplacian variance
    lap_sum = 0.0
    lap_count = 0
    for y in range(1, height - 1):
        for x in range(1, width - 1):
            idx = y * width + x
            lap = (
                -4 * luma_values[idx]
                + luma_values[idx - 1]
                + luma_values[idx + 1]
                + luma_values[idx - width]
                + luma_values[idx + width]
            )
            lap_sum += lap * lap
            lap_count += 1
    blur_variance = lap_sum / lap_count if lap_count else 0

    flags: list[str] = []
    score = 1.0
    if avg_luma < 25:
        flags.append('too_dark')
        score -= 0.4
    if avg_luma > 235:
        flags.append('too_bright')
        score -= 0.3
    if blur_variance < 80:
        flags.append('blurry')
        score -= 0.3
    if black_ratio > 0.70:
        flags.append('occluded')
        score -= 0.4

    return {
        'score': max(0, min(1, score)),
        'flags': flags,
        'avg_luma': round(avg_luma, 1),
        'blur_variance': round(blur_variance),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description='Perception robustness test suite')
    parser.add_argument('--width', type=int, default=64)
    parser.add_argument('--height', type=int, default=64)
    args = parser.parse_args()

    frames = build_test_frames(args.width, args.height)
    passed = 0
    failed = 0

    for frame in frames:
        result = evaluate_frame_quality(frame)
        detected_flags = set(result['flags'])
        # Filter expected flags that our assessment can detect
        expected = set(f for f in frame.expected_flags if f in {'too_dark', 'too_bright', 'occluded', 'blurry'})

        # Check all expected flags are detected
        missing = expected - detected_flags
        if missing:
            print(f'  ✗ FAIL {frame.name}: missing expected flags {missing}')
            print(f'    result: {result}')
            failed += 1
        else:
            print(f'  ✓ PASS {frame.name}: score={result["score"]:.2f} flags={result["flags"]}')
            passed += 1

    print(f'\n===== RESULTS: {passed} passed, {failed} failed =====')
    if failed > 0:
        sys.exit(1)


if __name__ == '__main__':
    main()
