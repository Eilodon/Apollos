from __future__ import annotations

from dataclasses import dataclass
from typing import Literal, TypedDict

MotionState = Literal['stationary', 'walking_slow', 'walking_fast', 'running']
DistanceCategory = Literal['very_close', 'mid', 'far']
NavigationMode = Literal['NAVIGATION', 'EXPLORE', 'READ', 'QUIET']


class MultimodalFramePayload(TypedDict, total=False):
    type: Literal['multimodal_frame']
    session_id: str
    timestamp: str
    frame_jpeg_base64: str
    motion_state: MotionState
    pitch: float
    velocity: float
    user_text: str
    yaw_delta_deg: float
    lat: float
    lng: float
    heading_deg: float


class AudioChunkPayload(TypedDict):
    type: Literal['audio_chunk']
    session_id: str
    timestamp: str
    audio_chunk_pcm16: str


class UserCommandPayload(TypedDict):
    type: Literal['user_command']
    session_id: str
    timestamp: str
    command: str


@dataclass(slots=True)
class HardStopEvent:
    position_x: float
    distance: DistanceCategory
    hazard_type: str
    confidence: float
