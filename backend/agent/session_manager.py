from __future__ import annotations

import asyncio
import os
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any

from .types import NavigationMode


@dataclass(slots=True)
class SessionState:
    session_id: str
    mode: NavigationMode = 'NAVIGATION'
    context_summary: str = ''
    motion_state: str = 'stationary'
    last_seen: str = field(default_factory=lambda: datetime.now(timezone.utc).isoformat())


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
        if len(self._sessions) > 5000:
            # Sort by last_seen to evict oldest
            sorted_keys = sorted(
                self._sessions.keys(),
                key=lambda k: self._sessions[k].last_seen
            )
            # Remove oldest 1000 to keep memory bound
            for k in sorted_keys[:1000]:
                self._sessions.pop(k, None)
                self._hazards.pop(k, None)
                self._emotions.pop(k, None)

    @staticmethod
    def _now() -> str:
        return datetime.now(timezone.utc).isoformat()

    def now(self) -> str:
        return self._now()

    async def touch_session(self, session_id: str, motion_state: str = 'stationary') -> SessionState:
        async with self._lock:
            state = self._sessions.get(session_id)
            if not state:
                self._trim_sessions_if_needed()
                state = SessionState(session_id=session_id)
                self._sessions[session_id] = state

            state.motion_state = motion_state
            state.last_seen = self._now()
            await self._persist_session(state)
            return state

    async def set_mode(self, session_id: str, mode: NavigationMode) -> SessionState:
        state = await self.touch_session(session_id)
        async with self._lock:
            state.mode = mode
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
        return f"Mode={state.mode}; motion={state.motion_state}; last_seen={state.last_seen}"

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

        async with self._lock:
            hazards_list = self._hazards.setdefault(session_id, [])
            hazards_list.append(event)
            if len(hazards_list) > 50:
                hazards_list.pop(0)

        await self._persist_subcollection(session_id, 'hazards', event)

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
