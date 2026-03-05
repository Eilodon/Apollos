from __future__ import annotations

from dataclasses import dataclass
from typing import Literal


SafetyActionTier = Literal['silent', 'ping', 'voice', 'hard_stop', 'human_escalation']
DistanceCategory = Literal['very_close', 'mid', 'far']

_TIER_ORDER: dict[SafetyActionTier, int] = {
    'silent': 0,
    'ping': 1,
    'voice': 2,
    'hard_stop': 3,
    'human_escalation': 4,
}


@dataclass(frozen=True, slots=True)
class SafetyPolicyInput:
    hazard_confidence: float
    distance_category: DistanceCategory
    motion_state: str
    sensor_health_score: float
    localization_uncertainty_m: float
    edge_reflex_active: bool


@dataclass(frozen=True, slots=True)
class SafetyPolicyDecision:
    tier: SafetyActionTier
    risk_score: float
    reason: str

    @property
    def should_emit_hard_stop(self) -> bool:
        return _TIER_ORDER[self.tier] >= _TIER_ORDER['hard_stop']

    @property
    def should_escalate_human(self) -> bool:
        return self.tier == 'human_escalation'


def _clamp(value: float, low: float, high: float) -> float:
    return max(low, min(high, value))


def _distance_weight(distance: DistanceCategory) -> float:
    if distance == 'very_close':
        return 2.4
    if distance == 'mid':
        return 1.4
    return 0.5


def _motion_weight(motion_state: str) -> float:
    normalized = motion_state.strip().lower()
    if normalized == 'running':
        return 1.2
    if normalized == 'walking_fast':
        return 0.8
    if normalized == 'walking_slow':
        return 0.35
    return 0.0


def max_tier(a: SafetyActionTier, b: SafetyActionTier) -> SafetyActionTier:
    return a if _TIER_ORDER[a] >= _TIER_ORDER[b] else b


def evaluate_safety_policy(payload: SafetyPolicyInput) -> SafetyPolicyDecision:
    confidence = _clamp(payload.hazard_confidence, 0.0, 1.0)
    sensor_health = _clamp(payload.sensor_health_score, 0.0, 1.0)
    loc_uncertainty = _clamp(payload.localization_uncertainty_m, 0.0, 300.0)

    risk_score = confidence * 3.2
    risk_score += _distance_weight(payload.distance_category)
    risk_score += _motion_weight(payload.motion_state)
    risk_score += (1.0 - sensor_health) * 1.8
    risk_score += min(1.0, loc_uncertainty / 100.0) * 0.8
    if payload.edge_reflex_active:
        risk_score += 1.5

    tier: SafetyActionTier
    if risk_score >= 6.0:
        tier = 'human_escalation' if sensor_health < 0.30 else 'hard_stop'
    elif risk_score >= 4.2:
        tier = 'hard_stop'
    elif risk_score >= 3.0:
        tier = 'voice'
    elif risk_score >= 2.0:
        tier = 'ping'
    else:
        tier = 'silent'

    # Safety bias: very-close hazards should never be below voice.
    if payload.distance_category == 'very_close':
        tier = max_tier(tier, 'voice')
        if confidence >= 0.55 or payload.edge_reflex_active:
            tier = max_tier(tier, 'hard_stop')

    # False-positive control for low-confidence far hazards.
    if (
        payload.distance_category == 'far'
        and confidence < 0.40
        and not payload.edge_reflex_active
        and tier in {'hard_stop', 'human_escalation'}
    ):
        tier = 'voice'

    reason_parts = [
        f'conf={confidence:.2f}',
        f'distance={payload.distance_category}',
        f'motion={payload.motion_state or "unknown"}',
        f'sensor_health={sensor_health:.2f}',
        f'loc_uncertainty_m={loc_uncertainty:.1f}',
        f'edge_reflex={"1" if payload.edge_reflex_active else "0"}',
        f'risk={risk_score:.2f}',
    ]
    return SafetyPolicyDecision(tier=tier, risk_score=risk_score, reason='; '.join(reason_parts))
