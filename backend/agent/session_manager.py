from __future__ import annotations

import asyncio
import os
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any

from .types import NavigationMode

GEOHASH_BASE32 = '0123456789bcdefghjkmnpqrstuvwxyz'


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


class SessionStore:
    def __init__(self, project_id: str | None = None, use_firestore: bool = False) -> None:
        self._sessions: dict[str, SessionState] = {}
        self._hazards: dict[str, list[dict[str, Any]]] = {}
        self._emotions: dict[str, list[dict[str, Any]]] = {}
        self._lock = asyncio.Lock()

        self._project_id = project_id or os.getenv('GOOGLE_CLOUD_PROJECT')
        self._use_firestore = use_firestore
        self._firestore_client = None

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

    async def touch_session(
        self,
        session_id: str,
        motion_state: str = 'stationary',
        lat: float | None = None,
        lng: float | None = None,
        heading_deg: float | None = None,
    ) -> SessionState:
        async with self._lock:
            state = self._sessions.get(session_id)
            if state is None:
                self._trim_sessions_if_needed()
                state = SessionState(session_id=session_id)
                self._sessions[session_id] = state

            state.motion_state = motion_state
            state.last_seen = self._now()
            state.frame_sequence += 1
            if lat is not None:
                state.lat = lat
            if lng is not None:
                state.lng = lng
            if heading_deg is not None:
                state.heading_deg = heading_deg
            await self._persist_session(state)
            return state

    async def set_mode(self, session_id: str, mode: NavigationMode) -> SessionState:
        state = await self.touch_session(session_id)
        async with self._lock:
            state.mode = mode
            state.mode_override_until_epoch = 0.0
            state.mode_override_reason = ''
            state.last_seen = self._now()
            await self._persist_session(state)
            return state

    async def get_effective_mode(self, session_id: str) -> NavigationMode:
        state = await self.touch_session(session_id)
        async with self._lock:
            if state.mode_override_until_epoch > time.time():
                return 'NAVIGATION'
            if state.mode_override_until_epoch > 0:
                state.mode_override_until_epoch = 0.0
                state.mode_override_reason = ''
                await self._persist_session(state)
            return state.mode

    async def apply_stress_mode_override(
        self,
        session_id: str,
        reason: str,
        revert_after_seconds: int = 120,
    ) -> SessionState:
        state = await self.touch_session(session_id)
        async with self._lock:
            state.mode_override_until_epoch = time.time() + max(30, revert_after_seconds)
            state.mode_override_reason = reason
            state.last_seen = self._now()
            await self._persist_session(state)
            return state

    async def update_context_summary(self, session_id: str, summary: str) -> SessionState:
        state = await self.touch_session(session_id)
        async with self._lock:
            state.context_summary = summary
            state.last_seen = self._now()
            await self._persist_session(state)
            return state

    async def get_context_summary(self, session_id: str) -> str:
        state = await self.touch_session(session_id)
        summary = state.context_summary.strip()
        if summary:
            return summary
        edge_state = (
            f"; edge_reflex={state.edge_hazard_type or 'none'}"
            if state.edge_hazard_until_epoch > time.time()
            else ''
        )
        return f"Mode={state.mode}; motion={state.motion_state}; last_seen={state.last_seen}{edge_state}"

    async def get_spatial_context(self, session_id: str, current_yaw: float | None) -> str:
        state = await self.touch_session(session_id)
        if current_yaw is None:
            return ''
        async with self._lock:
            now_epoch = time.time()
            recent_memories = [
                memory
                for memory in state.spatial_memories
                if (now_epoch - memory.detected_epoch) <= 300
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
        state = await self.touch_session(session_id)
        if yaw_at_detection is None:
            yaw_at_detection = state.heading_deg if state.heading_deg is not None else 0.0

        async with self._lock:
            existing = next(
                (
                    memory
                    for memory in state.spatial_memories
                    if memory.hazard_type == hazard_type and abs(memory.yaw_at_detection - yaw_at_detection) <= 15
                ),
                None,
            )
            if existing is not None:
                existing.confirmed_count += 1
                existing.last_seen_frame = state.frame_sequence
                existing.detected_epoch = time.time()
            else:
                state.spatial_memories.append(
                    HazardMemory(
                        hazard_type=hazard_type,
                        position_description=position_description,
                        yaw_at_detection=yaw_at_detection,
                        detected_epoch=time.time(),
                        frame_sequence=state.frame_sequence,
                        confirmed_count=1,
                        last_seen_frame=state.frame_sequence,
                    )
                )
                if len(state.spatial_memories) > 20:
                    state.spatial_memories = state.spatial_memories[-20:]
            await self._persist_session(state)

    async def should_lookup_location(self, session_id: str, now_epoch: float, min_interval_s: int = 30) -> bool:
        state = await self.touch_session(session_id)
        async with self._lock:
            if state.motion_state != 'stationary':
                return False
            if state.lat is None or state.lng is None:
                return False
            return (now_epoch - state.last_location_lookup_epoch) >= min_interval_s

    async def mark_location_lookup(self, session_id: str, now_epoch: float) -> None:
        state = await self.touch_session(session_id)
        async with self._lock:
            state.last_location_lookup_epoch = now_epoch
            await self._persist_session(state)

    async def get_location_snapshot(self, session_id: str) -> tuple[float | None, float | None, float | None]:
        state = await self.touch_session(session_id)
        return state.lat, state.lng, state.heading_deg

    async def mark_edge_hazard(
        self,
        session_id: str,
        hazard_type: str,
        suppress_seconds: float = 2.5,
    ) -> None:
        state = await self.touch_session(session_id)
        async with self._lock:
            state.edge_hazard_type = hazard_type
            state.edge_hazard_until_epoch = time.time() + max(0.5, suppress_seconds)
            state.last_seen = self._now()
            await self._persist_session(state)

    async def is_edge_hazard_active(self, session_id: str, now_epoch: float | None = None) -> bool:
        state = await self.touch_session(session_id)
        reference = now_epoch if now_epoch is not None else time.time()
        return state.edge_hazard_until_epoch > reference

    async def get_crowd_hazard_hints(self, lat: float, lng: float, limit: int = 3) -> list[str]:
        if not self._use_firestore or not self._firestore_client:
            return []

        prefix5 = encode_geohash(lat, lng, precision=5)

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
            return []

        hints: list[str] = []
        for doc in docs:
            payload = doc.to_dict() or {}
            hazard = str(payload.get('hazard_type', 'unknown'))
            count = int(payload.get('confirmed_count', 1) or 1)
            hints.append(f'{hazard} ({count} confirmations)')
        return hints

    async def log_hazard(
        self,
        session_id: str,
        hazard_type: str,
        position_x: float,
        distance_category: str,
        confidence: float,
        description: str,
    ) -> None:
        event = {
            'hazard_type': hazard_type,
            'position_x': position_x,
            'distance': distance_category,
            'confidence': confidence,
            'description': description,
            'ts': self._now(),
        }

        await self.add_spatial_hazard_memory(
            session_id=session_id,
            hazard_type=hazard_type,
            yaw_at_detection=None,
            position_description=description,
        )

        async with self._lock:
            hazards_list = self._hazards.setdefault(session_id, [])
            hazards_list.append(event)
            if len(hazards_list) > 50:
                hazards_list.pop(0)

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
                    'detected_epoch': memory.detected_epoch,
                    'frame_sequence': memory.frame_sequence,
                    'confirmed_count': memory.confirmed_count,
                    'last_seen_frame': memory.last_seen_frame,
                }
                for memory in state.spatial_memories
            ],
            'edge_hazard_until_epoch': state.edge_hazard_until_epoch,
            'edge_hazard_type': state.edge_hazard_type,
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

        geohash = encode_geohash(state.lat, state.lng, precision=7)
        geohash_prefix5 = geohash[:5]
        hazard_type = str(hazard_payload.get('hazard_type', 'unknown'))
        seed_payload = {
            'geohash': geohash,
            'geohash_prefix5': geohash_prefix5,
            'lat': state.lat,
            'lng': state.lng,
            'hazard_type': hazard_type,
            'confirmed_count': 1,
            'last_confirmed': self._now(),
            'description_vi': str(hazard_payload.get('description', '')),
            'heading_deg': state.heading_deg,
        }
        doc_id = f'{geohash}-{hazard_type}'.replace('/', '_')
        doc_ref = self._firestore_client.collection('hazard_map').document(doc_id)

        def upsert_payload() -> None:
            snapshot = doc_ref.get()
            if snapshot.exists:
                current = snapshot.to_dict() or {}
                current_count = int(current.get('confirmed_count', 1) or 1)
                seed_payload['confirmed_count'] = current_count + 1
            doc_ref.set(seed_payload, merge=True)

        await asyncio.to_thread(upsert_payload)
