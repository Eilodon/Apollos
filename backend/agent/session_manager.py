from __future__ import annotations

import asyncio
import os
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any

from .hazard_taxonomy import description_vi_for_hazard, normalize_hazard_type
from .types import NavigationMode

GEOHASH_BASE32 = '0123456789bcdefghjkmnpqrstuvwxyz'
SECONDS_PER_WEEK = 7 * 24 * 60 * 60


def encode_geohash(lat: float, lng: float, precision: int = 7) -> str:
    lat_interval = [-90.0, 90.0]
    lng_interval = [-180.0, 180.0]
    geohash: list[str] = []
    bits = [16, 8, 4, 2, 1]
    bit = 0
    ch = 0
    even = True

    while len(geohash) < precision:
        if even:
            mid = (lng_interval[0] + lng_interval[1]) / 2
            if lng > mid:
                ch |= bits[bit]
                lng_interval[0] = mid
            else:
                lng_interval[1] = mid
        else:
            mid = (lat_interval[0] + lat_interval[1]) / 2
            if lat > mid:
                ch |= bits[bit]
                lat_interval[0] = mid
            else:
                lat_interval[1] = mid
        even = not even
        if bit < 4:
            bit += 1
        else:
            geohash.append(GEOHASH_BASE32[ch])
            bit = 0
            ch = 0

    return ''.join(geohash)


def normalize_delta_deg(value: float) -> float:
    return ((value + 180.0) % 360.0) - 180.0


def clock_face_from_delta(delta_deg: float) -> int:
    step = int(round(delta_deg / 30.0))
    hour = step % 12
    return 12 if hour == 0 else hour


@dataclass(slots=True)
class HazardMemory:
    hazard_type: str
    position_description: str
    yaw_at_detection: float
    geohash: str
    detected_epoch: float
    frame_sequence: int
    confirmed_count: int
    last_seen_frame: int


@dataclass(slots=True)
class SessionState:
    session_id: str
    mode: NavigationMode = 'NAVIGATION'
    context_summary: str = ''
    motion_state: str = 'stationary'
    last_seen: str = field(default_factory=lambda: datetime.now(timezone.utc).isoformat())
    mode_override_until_epoch: float = 0.0
    mode_override_reason: str = ''
    lat: float | None = None
    lng: float | None = None
    heading_deg: float | None = None
    frame_sequence: int = 0
    spatial_memories: list[HazardMemory] = field(default_factory=list)
    last_location_lookup_epoch: float = 0.0
    edge_hazard_until_epoch: float = 0.0
    edge_hazard_type: str = ''
    sensor_health_score: float = 1.0
    sensor_health_flags: list[str] = field(default_factory=list)
    localization_uncertainty_m: float = 120.0
    degraded_mode: bool = False
    degraded_reason: str = ''
    degraded_since_epoch: float = 0.0
    last_safety_tier: str = 'silent'
    last_persist_epoch: float = 0.0
    utterance_timestamps: list[float] = field(default_factory=list)


class SessionStore:
    SPATIAL_MEMORY_MAX = 20
    SPATIAL_MEMORY_YAW_THRESHOLD_DEG = 30.0
    SPATIAL_MEMORY_GEOHASH_PREFIX = 6

    def __init__(self, project_id: str | None = None, use_firestore: bool = False) -> None:
        self._sessions: dict[str, SessionState] = {}
        self._hazards: dict[str, list[dict[str, Any]]] = {}
        self._emotions: dict[str, list[dict[str, Any]]] = {}
        self._crowd_hazard_map: dict[str, dict[str, Any]] = {}
        self._lock = asyncio.Lock()

        self._project_id = project_id or os.getenv('GOOGLE_CLOUD_PROJECT')
        self._use_firestore = use_firestore
        self._firestore_client = None
        self._persist_min_interval_s = max(0.2, float(os.getenv('SESSION_PERSIST_MIN_INTERVAL_S', '1.5')))
        self._sensor_health_degraded_threshold = max(
            0.2,
            min(0.9, float(os.getenv('SENSOR_HEALTH_DEGRADED_THRESHOLD', '0.55'))),
        )
        self._localization_uncertainty_degraded_m = max(
            10.0,
            float(os.getenv('LOCALIZATION_UNCERTAINTY_DEGRADED_M', '45')),
        )

        if self._use_firestore:
            try:
                from google.cloud import firestore

                self._firestore_client = firestore.Client(project=self._project_id)
            except Exception:
                self._use_firestore = False
                self._firestore_client = None

    def _trim_sessions_if_needed(self) -> None:
        if len(self._sessions) <= 5000:
            return
        sorted_keys = sorted(self._sessions.keys(), key=lambda key: self._sessions[key].last_seen)
        for key in sorted_keys[:1000]:
            self._sessions.pop(key, None)
            self._hazards.pop(key, None)
            self._emotions.pop(key, None)

    @staticmethod
    def _now() -> str:
        return datetime.now(timezone.utc).isoformat()

    def now(self) -> str:
        return self._now()

    def _get_or_create_state_locked(self, session_id: str) -> SessionState:
        state = self._sessions.get(session_id)
        if state is None:
            self._trim_sessions_if_needed()
            state = SessionState(session_id=session_id)
            self._sessions[session_id] = state
        return state

    def _mark_persist_if_due_locked(self, state: SessionState, force: bool = False) -> bool:
        if not self._use_firestore or not self._firestore_client:
            return False
        now_epoch = time.time()
        if not force and (now_epoch - state.last_persist_epoch) < self._persist_min_interval_s:
            return False
        state.last_persist_epoch = now_epoch
        return True

    async def ensure_session(self, session_id: str) -> SessionState:
        async with self._lock:
            return self._get_or_create_state_locked(session_id)

    async def touch_session(
        self,
        session_id: str,
        motion_state: str | None = None,
        lat: float | None = None,
        lng: float | None = None,
        heading_deg: float | None = None,
        *,
        advance_frame: bool = True,
        persist: bool = True,
        force_persist: bool = False,
    ) -> SessionState:
        should_persist = False
        async with self._lock:
            state = self._get_or_create_state_locked(session_id)
            if motion_state is not None:
                state.motion_state = motion_state
            state.last_seen = self._now()
            if advance_frame:
                state.frame_sequence += 1
            if lat is not None:
                state.lat = lat
            if lng is not None:
                state.lng = lng
            if heading_deg is not None:
                state.heading_deg = heading_deg
            if persist:
                should_persist = self._mark_persist_if_due_locked(state, force=force_persist)

        if should_persist:
            await self._persist_session(state)
        return state

    async def set_mode(self, session_id: str, mode: NavigationMode) -> SessionState:
        state = await self.ensure_session(session_id)
        should_persist = False
        async with self._lock:
            state.mode = mode
            state.mode_override_until_epoch = 0.0
            state.mode_override_reason = ''
            state.last_seen = self._now()
            should_persist = self._mark_persist_if_due_locked(state, force=True)

        if should_persist:
            await self._persist_session(state)
        return state

    async def get_effective_mode(self, session_id: str) -> NavigationMode:
        state = await self.ensure_session(session_id)
        should_persist = False
        effective_mode: NavigationMode
        async with self._lock:
            if state.mode_override_until_epoch > time.time():
                return 'NAVIGATION'
            if state.mode_override_until_epoch > 0:
                state.mode_override_until_epoch = 0.0
                state.mode_override_reason = ''
                should_persist = self._mark_persist_if_due_locked(state, force=True)
            effective_mode = state.mode
        if should_persist:
            await self._persist_session(state)
        return effective_mode

    async def apply_stress_mode_override(
        self,
        session_id: str,
        reason: str,
        revert_after_seconds: int = 120,
    ) -> SessionState:
        state = await self.ensure_session(session_id)
        should_persist = False
        async with self._lock:
            state.mode_override_until_epoch = time.time() + max(30, revert_after_seconds)
            state.mode_override_reason = reason
            state.last_seen = self._now()
            should_persist = self._mark_persist_if_due_locked(state, force=True)

        if should_persist:
            await self._persist_session(state)
        return state

    async def update_context_summary(self, session_id: str, summary: str) -> SessionState:
        state = await self.ensure_session(session_id)
        should_persist = False
        async with self._lock:
            state.context_summary = summary
            state.last_seen = self._now()
            should_persist = self._mark_persist_if_due_locked(state, force=True)

        if should_persist:
            await self._persist_session(state)
        return state

    async def get_context_summary(self, session_id: str) -> str:
        state = await self.ensure_session(session_id)
        summary = state.context_summary.strip()
        if summary:
            return summary
        edge_state = (
            f"; edge_reflex={state.edge_hazard_type or 'none'}"
            if state.edge_hazard_until_epoch > time.time()
            else ''
        )
        degraded_state = (
            f"; degraded={state.degraded_reason or 'active'}"
            if state.degraded_mode
            else ''
        )
        return (
            f"Mode={state.mode}; motion={state.motion_state}; "
            f"sensor_health={state.sensor_health_score:.2f}; "
            f"loc_uncertainty_m={state.localization_uncertainty_m:.1f}; "
            f"last_seen={state.last_seen}{edge_state}{degraded_state}"
        )

    async def get_spatial_context(
        self,
        session_id: str,
        current_yaw: float | None,
        current_lat: float | None = None,
        current_lng: float | None = None,
    ) -> str:
        state = await self.ensure_session(session_id)
        if current_yaw is None or current_lat is None or current_lng is None:
            return ''
        current_geohash = encode_geohash(current_lat, current_lng, precision=7)
        async with self._lock:
            now_epoch = time.time()
            recent_memories = [
                memory
                for memory in state.spatial_memories
                if (now_epoch - memory.detected_epoch) <= 300
                and memory.geohash
                and memory.geohash[: self.SPATIAL_MEMORY_GEOHASH_PREFIX]
                == current_geohash[: self.SPATIAL_MEMORY_GEOHASH_PREFIX]
                and abs(normalize_delta_deg(memory.yaw_at_detection - current_yaw))
                <= self.SPATIAL_MEMORY_YAW_THRESHOLD_DEG
            ]
            if not recent_memories:
                return ''

            contextualized: list[str] = []
            for memory in recent_memories[-4:]:
                delta = normalize_delta_deg(memory.yaw_at_detection - current_yaw)
                clock_face = clock_face_from_delta(delta)
                if abs(delta) < 15:
                    bearing = 'ahead'
                elif delta > 0:
                    bearing = f'{abs(delta):.0f}deg right'
                else:
                    bearing = f'{abs(delta):.0f}deg left'
                age_s = int(max(0, now_epoch - memory.detected_epoch))
                contextualized.append(
                    f"{memory.hazard_type} around {clock_face} o'clock ({bearing}, {age_s}s ago)"
                )

            if not contextualized:
                return ''
            return '[SPATIAL MEMORY: ' + '; '.join(contextualized) + ']'

    async def add_spatial_hazard_memory(
        self,
        session_id: str,
        hazard_type: str,
        yaw_at_detection: float | None,
        position_description: str,
    ) -> None:
        state = await self.ensure_session(session_id)
        if yaw_at_detection is None:
            yaw_at_detection = state.heading_deg if state.heading_deg is not None else 0.0
        detection_geohash = ''
        if state.lat is not None and state.lng is not None:
            detection_geohash = encode_geohash(state.lat, state.lng, precision=7)

        should_persist = False
        async with self._lock:
            existing = next(
                (
                    memory
                    for memory in state.spatial_memories
                    if memory.hazard_type == hazard_type
                    and memory.geohash
                    and detection_geohash
                    and memory.geohash[: self.SPATIAL_MEMORY_GEOHASH_PREFIX]
                    == detection_geohash[: self.SPATIAL_MEMORY_GEOHASH_PREFIX]
                    and abs(normalize_delta_deg(memory.yaw_at_detection - yaw_at_detection))
                    <= self.SPATIAL_MEMORY_YAW_THRESHOLD_DEG
                ),
                None,
            )
            if existing is not None:
                existing.confirmed_count += 1
                existing.last_seen_frame = state.frame_sequence
                existing.detected_epoch = time.time()
                if detection_geohash:
                    existing.geohash = detection_geohash
            else:
                state.spatial_memories.append(
                    HazardMemory(
                        hazard_type=hazard_type,
                        position_description=position_description,
                        yaw_at_detection=yaw_at_detection,
                        geohash=detection_geohash,
                        detected_epoch=time.time(),
                        frame_sequence=state.frame_sequence,
                        confirmed_count=1,
                        last_seen_frame=state.frame_sequence,
                    )
                )
                if len(state.spatial_memories) > self.SPATIAL_MEMORY_MAX:
                    state.spatial_memories = state.spatial_memories[-self.SPATIAL_MEMORY_MAX :]
            should_persist = self._mark_persist_if_due_locked(state, force=True)

        if should_persist:
            await self._persist_session(state)

    async def should_lookup_location(self, session_id: str, now_epoch: float, min_interval_s: int = 30) -> bool:
        state = await self.ensure_session(session_id)
        async with self._lock:
            if state.motion_state != 'stationary':
                return False
            if state.lat is None or state.lng is None:
                return False
            return (now_epoch - state.last_location_lookup_epoch) >= min_interval_s

    async def mark_location_lookup(self, session_id: str, now_epoch: float) -> None:
        state = await self.ensure_session(session_id)
        should_persist = False
        async with self._lock:
            state.last_location_lookup_epoch = now_epoch
            should_persist = self._mark_persist_if_due_locked(state, force=True)

        if should_persist:
            await self._persist_session(state)

    async def get_location_snapshot(self, session_id: str) -> tuple[float | None, float | None, float | None]:
        state = await self.ensure_session(session_id)
        return state.lat, state.lng, state.heading_deg

    async def get_observability(self, session_id: str) -> dict[str, Any]:
        state = await self.ensure_session(session_id)
        return {
            'sensor_health_score': state.sensor_health_score,
            'sensor_health_flags': list(state.sensor_health_flags),
            'localization_uncertainty_m': state.localization_uncertainty_m,
            'degraded_mode': state.degraded_mode,
            'degraded_reason': state.degraded_reason,
            'last_safety_tier': state.last_safety_tier,
        }

    async def update_observability(
        self,
        session_id: str,
        *,
        sensor_health_score: float | None = None,
        sensor_health_flags: list[str] | None = None,
        localization_uncertainty_m: float | None = None,
        safety_tier: str | None = None,
    ) -> dict[str, Any]:
        state = await self.ensure_session(session_id)
        should_persist = False

        async with self._lock:
            score = (
                float(sensor_health_score)
                if sensor_health_score is not None
                else float(state.sensor_health_score)
            )
            score = max(0.0, min(1.0, score))

            uncertainty_m = (
                float(localization_uncertainty_m)
                if localization_uncertainty_m is not None
                else float(state.localization_uncertainty_m)
            )
            uncertainty_m = max(0.0, min(500.0, uncertainty_m))

            cleaned_flags: list[str]
            if sensor_health_flags is None:
                cleaned_flags = list(state.sensor_health_flags)
            else:
                cleaned_flags = []
                for raw in sensor_health_flags[:8]:
                    normalized = str(raw).strip().lower().replace(' ', '_')
                    if normalized and normalized not in cleaned_flags:
                        cleaned_flags.append(normalized)

            degraded_reasons: list[str] = []
            if score < self._sensor_health_degraded_threshold:
                degraded_reasons.append('low_sensor_health')
            if uncertainty_m > self._localization_uncertainty_degraded_m:
                degraded_reasons.append('high_localization_uncertainty')
            if any(
                flag in cleaned_flags
                for flag in (
                    'depth_error',
                    'motion_permission_denied',
                    'camera_unavailable',
                    'location_missing',
                )
            ):
                degraded_reasons.append('critical_sensor_gap')

            degraded_mode = bool(degraded_reasons)
            degraded_reason = ','.join(degraded_reasons)
            previous_degraded = state.degraded_mode
            previous_reason = state.degraded_reason
            previous_score = state.sensor_health_score
            previous_uncertainty = state.localization_uncertainty_m
            previous_tier = state.last_safety_tier

            state.sensor_health_score = score
            state.sensor_health_flags = cleaned_flags
            state.localization_uncertainty_m = uncertainty_m
            state.degraded_mode = degraded_mode
            state.degraded_reason = degraded_reason
            if safety_tier:
                state.last_safety_tier = str(safety_tier).strip().lower() or state.last_safety_tier
            if degraded_mode and not previous_degraded:
                state.degraded_since_epoch = time.time()
            if not degraded_mode:
                state.degraded_since_epoch = 0.0
            state.last_seen = self._now()

            changed = (
                (abs(score - previous_score) >= 0.05)
                or (abs(uncertainty_m - previous_uncertainty) >= 5.0)
                or (degraded_mode != previous_degraded)
                or (degraded_reason != previous_reason)
                or (state.last_safety_tier != previous_tier)
            )

            if changed:
                should_persist = self._mark_persist_if_due_locked(state, force=True)

        if should_persist:
            await self._persist_session(state)

        return {
            'sensor_health_score': state.sensor_health_score,
            'sensor_health_flags': list(state.sensor_health_flags),
            'localization_uncertainty_m': state.localization_uncertainty_m,
            'degraded_mode': state.degraded_mode,
            'degraded_reason': state.degraded_reason,
            'degraded_changed': state.degraded_mode != previous_degraded,
            'last_safety_tier': state.last_safety_tier,
        }

    async def mark_edge_hazard(
        self,
        session_id: str,
        hazard_type: str,
        suppress_seconds: float = 2.5,
    ) -> None:
        state = await self.ensure_session(session_id)
        should_persist = False
        async with self._lock:
            state.edge_hazard_type = hazard_type
            state.edge_hazard_until_epoch = time.time() + max(0.5, suppress_seconds)
            state.last_seen = self._now()
            should_persist = self._mark_persist_if_due_locked(state, force=True)

        if should_persist:
            await self._persist_session(state)

    async def is_edge_hazard_active(self, session_id: str, now_epoch: float | None = None) -> bool:
        state = await self.ensure_session(session_id)
        reference = now_epoch if now_epoch is not None else time.time()
        return state.edge_hazard_until_epoch > reference

    # --- Utterance Budget ---
    UTTERANCE_WINDOW_S = 10.0
    UTTERANCE_BUDGET_FAST_MOTION = 2
    UTTERANCE_BUDGET_NORMAL = 5

    async def should_allow_utterance(
        self,
        session_id: str,
        *,
        is_hard_stop: bool = False,
    ) -> bool:
        """Return True if an assistant_text message should be emitted.

        Hard-stop messages always pass. For normal voice, a rolling window
        based on motion state limits the number of utterances.
        """
        if is_hard_stop:
            return True
        state = await self.ensure_session(session_id)
        now = time.time()
        async with self._lock:
            # Prune stale entries.
            cutoff = now - self.UTTERANCE_WINDOW_S
            state.utterance_timestamps = [
                ts for ts in state.utterance_timestamps if ts > cutoff
            ]
            budget = (
                self.UTTERANCE_BUDGET_FAST_MOTION
                if state.motion_state in ('walking_fast', 'running')
                else self.UTTERANCE_BUDGET_NORMAL
            )
            if len(state.utterance_timestamps) >= budget:
                return False
            state.utterance_timestamps.append(now)
            return True

    @staticmethod
    def _parse_iso_epoch(value: Any) -> float:
        if isinstance(value, (int, float)):
            return float(value)
        if not isinstance(value, str) or not value.strip():
            return 0.0
        candidate = value.strip()
        if candidate.endswith('Z'):
            candidate = candidate[:-1] + '+00:00'
        try:
            return datetime.fromisoformat(candidate).timestamp()
        except ValueError:
            return 0.0

    @staticmethod
    def _current_local_hour(now_epoch: float) -> int:
        return datetime.fromtimestamp(now_epoch).hour

    @staticmethod
    def _decayed_confirmed_count(confirmed_count: int, last_confirmed_epoch: float, now_epoch: float) -> int:
        if confirmed_count <= 0:
            return 0
        if last_confirmed_epoch <= 0:
            return confirmed_count
        stale_seconds = max(0.0, now_epoch - last_confirmed_epoch)
        decay_steps = int(stale_seconds // SECONDS_PER_WEEK)
        return max(0, confirmed_count - decay_steps)

    @staticmethod
    def _normalize_hourly_histogram(raw: Any) -> dict[int, int]:
        if not isinstance(raw, dict):
            return {}
        normalized: dict[int, int] = {}
        for key, value in raw.items():
            try:
                hour = int(key)
                count = int(value)
            except (TypeError, ValueError):
                continue
            if 0 <= hour <= 23 and count > 0:
                normalized[hour] = count
        return normalized

    @staticmethod
    def _serialize_hourly_histogram(histogram: dict[int, int]) -> dict[str, int]:
        return {str(hour): count for hour, count in sorted(histogram.items()) if 0 <= hour <= 23 and count > 0}

    @staticmethod
    def _extract_peak_hours(hourly_histogram: dict[int, int]) -> list[int]:
        if not hourly_histogram:
            return []
        top_count = max(hourly_histogram.values())
        # Require at least 2 confirmations in a bucket to claim a stable pattern.
        if top_count < 2:
            return []
        peak_hours = [hour for hour, count in hourly_histogram.items() if count == top_count]
        return sorted(peak_hours)[:3]

    @staticmethod
    def _format_peak_hours(peak_hours: list[int]) -> str:
        if not peak_hours:
            return ''
        if len(peak_hours) == 1:
            return f'{peak_hours[0]}h'
        if len(peak_hours) == 2 and peak_hours[1] - peak_hours[0] == 1:
            return f'{peak_hours[0]}-{peak_hours[1]}h'
        return ', '.join(f'{hour}h' for hour in peak_hours)

    @staticmethod
    def _is_hour_near(now_hour: int, target_hour: int, tolerance: int = 1) -> bool:
        delta = abs(now_hour - target_hour)
        return min(delta, 24 - delta) <= tolerance

    def _build_crowd_hint(self, payload: dict[str, Any], now_epoch: float) -> str:
        hazard_type = normalize_hazard_type(str(payload.get('hazard_type', 'unknown')))
        description_vi = str(payload.get('description_vi', '')).strip() or description_vi_for_hazard(hazard_type)
        count = int(payload.get('confirmed_count', 1) or 1)
        hourly_histogram = self._normalize_hourly_histogram(payload.get('hourly_histogram', {}))
        peak_hours = self._extract_peak_hours(hourly_histogram)
        now_hour = self._current_local_hour(now_epoch)

        if peak_hours:
            peak_text = self._format_peak_hours(peak_hours)
            if any(self._is_hour_near(now_hour, hour) for hour in peak_hours):
                return (
                    f'{description_vi} hay xuất hiện vào {peak_text}; '
                    f'hiện là {now_hour}h, cẩn thận hơn ({count} lần xác nhận).'
                )
            return f'{description_vi} thường xuất hiện vào {peak_text} ({count} lần xác nhận).'

        return f'{description_vi} ({count} lần xác nhận).'

    def _upsert_crowd_hazard_local_locked(
        self,
        state: SessionState,
        hazard_payload: dict[str, Any],
        now_epoch: float,
        now_iso: str,
    ) -> None:
        if state.lat is None or state.lng is None:
            return

        geohash = encode_geohash(state.lat, state.lng, precision=7)
        hazard_type = normalize_hazard_type(str(hazard_payload.get('hazard_type', 'unknown')))
        doc_id = f'{geohash}-{hazard_type}'.replace('/', '_')
        current_hour = self._current_local_hour(now_epoch)

        existing = self._crowd_hazard_map.get(doc_id)
        if existing:
            base_count = self._decayed_confirmed_count(
                int(existing.get('confirmed_count', 1) or 1),
                self._parse_iso_epoch(existing.get('last_confirmed')),
                now_epoch,
            )
            confirmed_count = base_count + 1
            hourly_histogram = self._normalize_hourly_histogram(existing.get('hourly_histogram', {}))
        else:
            confirmed_count = 1
            hourly_histogram = {}

        hourly_histogram[current_hour] = hourly_histogram.get(current_hour, 0) + 1
        self._crowd_hazard_map[doc_id] = {
            'geohash': geohash,
            'geohash_prefix5': geohash[:5],
            'lat': state.lat,
            'lng': state.lng,
            'hazard_type': hazard_type,
            'confirmed_count': confirmed_count,
            'last_confirmed': now_iso,
            'description_vi': description_vi_for_hazard(hazard_type),
            'heading_deg': state.heading_deg,
            'hourly_histogram': self._serialize_hourly_histogram(hourly_histogram),
            'peak_hours': self._extract_peak_hours(hourly_histogram),
        }

    async def get_crowd_hazard_hints(self, lat: float, lng: float, limit: int = 3) -> list[str]:
        now_epoch = time.time()
        prefix5 = encode_geohash(lat, lng, precision=5)
        candidates: dict[str, dict[str, Any]] = {}

        async with self._lock:
            stale_doc_ids: list[str] = []
            for doc_id, payload in self._crowd_hazard_map.items():
                if str(payload.get('geohash_prefix5', '')) != prefix5:
                    continue

                decayed_count = self._decayed_confirmed_count(
                    int(payload.get('confirmed_count', 1) or 1),
                    self._parse_iso_epoch(payload.get('last_confirmed')),
                    now_epoch,
                )
                if decayed_count <= 0:
                    stale_doc_ids.append(doc_id)
                    continue

                if decayed_count != int(payload.get('confirmed_count', 1) or 1):
                    payload['confirmed_count'] = decayed_count
                candidates[doc_id] = dict(payload)

            for doc_id in stale_doc_ids:
                self._crowd_hazard_map.pop(doc_id, None)

        if not self._use_firestore or not self._firestore_client:
            ordered = sorted(
                candidates.values(),
                key=lambda item: int(item.get('confirmed_count', 0) or 0),
                reverse=True,
            )
            return [self._build_crowd_hint(item, now_epoch) for item in ordered[:limit]]

        def query_docs() -> list[Any]:
            query = (
                self._firestore_client.collection('hazard_map')
                .where('geohash_prefix5', '==', prefix5)
                .limit(limit)
            )
            return list(query.stream())

        try:
            docs = await asyncio.to_thread(query_docs)
        except Exception:
            ordered = sorted(
                candidates.values(),
                key=lambda item: int(item.get('confirmed_count', 0) or 0),
                reverse=True,
            )
            return [self._build_crowd_hint(item, now_epoch) for item in ordered[:limit]]

        for doc in docs:
            payload = doc.to_dict() or {}
            doc_id = str(getattr(doc, 'id', '') or '')
            hazard_type = normalize_hazard_type(str(payload.get('hazard_type', 'unknown')))
            geohash = str(payload.get('geohash', ''))
            if not doc_id and geohash:
                doc_id = f'{geohash}-{hazard_type}'.replace('/', '_')
            if not doc_id:
                continue

            raw_count = int(payload.get('confirmed_count', 1) or 1)
            last_confirmed_epoch = self._parse_iso_epoch(payload.get('last_confirmed'))
            decayed_count = self._decayed_confirmed_count(raw_count, last_confirmed_epoch, now_epoch)
            if decayed_count <= 0:
                try:
                    await asyncio.to_thread(doc.reference.delete)
                except Exception:
                    pass
                continue

            if decayed_count != raw_count:
                try:
                    await asyncio.to_thread(doc.reference.set, {'confirmed_count': decayed_count}, merge=True)
                except Exception:
                    pass

            normalized_payload = {
                **payload,
                'hazard_type': hazard_type,
                'confirmed_count': decayed_count,
                'description_vi': str(payload.get('description_vi', '')).strip() or description_vi_for_hazard(hazard_type),
            }
            existing = candidates.get(doc_id)
            if existing is None or int(existing.get('confirmed_count', 0) or 0) < decayed_count:
                candidates[doc_id] = normalized_payload

        ordered = sorted(
            candidates.values(),
            key=lambda item: int(item.get('confirmed_count', 0) or 0),
            reverse=True,
        )
        return [self._build_crowd_hint(item, now_epoch) for item in ordered[:limit]]

    async def log_hazard(
        self,
        session_id: str,
        hazard_type: str,
        position_x: float,
        distance_category: str,
        confidence: float,
        description: str,
    ) -> None:
        normalized_hazard = normalize_hazard_type(hazard_type)
        now_iso = self._now()
        now_epoch = time.time()
        event = {
            'hazard_type': normalized_hazard,
            'position_x': position_x,
            'distance': distance_category,
            'confidence': confidence,
            'description': description,
            'ts': now_iso,
        }

        await self.add_spatial_hazard_memory(
            session_id=session_id,
            hazard_type=normalized_hazard,
            yaw_at_detection=None,
            position_description=description,
        )

        async with self._lock:
            state = self._get_or_create_state_locked(session_id)
            hazards_list = self._hazards.setdefault(session_id, [])
            hazards_list.append(event)
            if len(hazards_list) > 50:
                hazards_list.pop(0)
            self._upsert_crowd_hazard_local_locked(state, event, now_epoch=now_epoch, now_iso=now_iso)

        await self._persist_subcollection(session_id, 'hazards', event)
        await self._persist_hazard_map_seed(session_id, event)

    async def log_emotion(self, session_id: str, state: str, confidence: float) -> None:
        event = {'state': state, 'confidence': confidence, 'ts': self._now()}
        async with self._lock:
            emotions_list = self._emotions.setdefault(session_id, [])
            emotions_list.append(event)
            if len(emotions_list) > 50:
                emotions_list.pop(0)

        await self._persist_subcollection(session_id, 'emotions', event)

    async def build_human_help_link(self, session_id: str) -> str:
        public_help_base = os.getenv('PUBLIC_HELP_BASE', 'https://example.com/help').rstrip('/')
        return f"{public_help_base}?session={session_id}"

    async def _persist_session(self, state: SessionState) -> None:
        if not self._use_firestore or not self._firestore_client:
            return
        payload = {
            'mode': state.mode,
            'context_summary': state.context_summary,
            'motion_state': state.motion_state,
            'last_seen': state.last_seen,
            'mode_override_until_epoch': state.mode_override_until_epoch,
            'mode_override_reason': state.mode_override_reason,
            'lat': state.lat,
            'lng': state.lng,
            'heading_deg': state.heading_deg,
            'frame_sequence': state.frame_sequence,
            'spatial_memories': [
                {
                    'hazard_type': memory.hazard_type,
                    'position_description': memory.position_description,
                    'yaw_at_detection': memory.yaw_at_detection,
                    'geohash': memory.geohash,
                    'detected_epoch': memory.detected_epoch,
                    'frame_sequence': memory.frame_sequence,
                    'confirmed_count': memory.confirmed_count,
                    'last_seen_frame': memory.last_seen_frame,
                }
                for memory in state.spatial_memories
            ],
            'edge_hazard_until_epoch': state.edge_hazard_until_epoch,
            'edge_hazard_type': state.edge_hazard_type,
            'sensor_health_score': state.sensor_health_score,
            'sensor_health_flags': list(state.sensor_health_flags),
            'localization_uncertainty_m': state.localization_uncertainty_m,
            'degraded_mode': state.degraded_mode,
            'degraded_reason': state.degraded_reason,
            'degraded_since_epoch': state.degraded_since_epoch,
            'last_safety_tier': state.last_safety_tier,
        }
        await asyncio.to_thread(
            self._firestore_client.collection('sessions').document(state.session_id).set,
            payload,
            merge=True,
        )

    async def _persist_subcollection(self, session_id: str, collection: str, payload: dict[str, Any]) -> None:
        if not self._use_firestore or not self._firestore_client:
            return
        doc_ref = self._firestore_client.collection('sessions').document(session_id)
        await asyncio.to_thread(doc_ref.collection(collection).add, payload)

    async def _persist_hazard_map_seed(self, session_id: str, hazard_payload: dict[str, Any]) -> None:
        if not self._use_firestore or not self._firestore_client:
            return
        state = self._sessions.get(session_id)
        if not state or state.lat is None or state.lng is None:
            return

        now_epoch = time.time()
        now_iso = self._now()
        current_hour = self._current_local_hour(now_epoch)
        geohash = encode_geohash(state.lat, state.lng, precision=7)
        geohash_prefix5 = geohash[:5]
        hazard_type = normalize_hazard_type(str(hazard_payload.get('hazard_type', 'unknown')))
        initial_histogram = {str(current_hour): 1}
        seed_payload = {
            'geohash': geohash,
            'geohash_prefix5': geohash_prefix5,
            'lat': state.lat,
            'lng': state.lng,
            'hazard_type': hazard_type,
            'confirmed_count': 1,
            'last_confirmed': now_iso,
            'description_vi': description_vi_for_hazard(hazard_type),
            'heading_deg': state.heading_deg,
            'hourly_histogram': initial_histogram,
            'peak_hours': [],
        }
        doc_id = f'{geohash}-{hazard_type}'.replace('/', '_')
        doc_ref = self._firestore_client.collection('hazard_map').document(doc_id)

        def upsert_payload() -> None:
            snapshot = doc_ref.get()
            if snapshot.exists:
                current = snapshot.to_dict() or {}
                current_count = int(current.get('confirmed_count', 1) or 1)
                last_confirmed_epoch = self._parse_iso_epoch(current.get('last_confirmed'))
                decayed_count = self._decayed_confirmed_count(current_count, last_confirmed_epoch, now_epoch)
                seed_payload['confirmed_count'] = decayed_count + 1

                merged_histogram = self._normalize_hourly_histogram(current.get('hourly_histogram', {}))
                merged_histogram[current_hour] = merged_histogram.get(current_hour, 0) + 1
                seed_payload['hourly_histogram'] = self._serialize_hourly_histogram(merged_histogram)
                seed_payload['peak_hours'] = self._extract_peak_hours(merged_histogram)
            else:
                seed_payload['peak_hours'] = self._extract_peak_hours({current_hour: 1})
            doc_ref.set(seed_payload, merge=True)

        await asyncio.to_thread(upsert_payload)
