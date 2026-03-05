from __future__ import annotations

import math
import os
import time

from .decorators import tool
from .runtime import get_current_session, get_runtime

PRIORITY_TYPES = (
    ('bus_stop_xe_buyt', 'Tram xe buyt'),
    ('xe_om_stand', 'Diem xe om'),
    ('atm', 'May ATM'),
    ('cho_truyen_thong', 'Cho truyen thong'),
)


def _pick_priority_type(lat: float, lng: float, heading_deg: float) -> tuple[str, str]:
    seed = int(abs(lat * 10_000) + abs(lng * 10_000) + abs(heading_deg))
    return PRIORITY_TYPES[seed % len(PRIORITY_TYPES)]


@tool
async def identify_location(lat: float, lng: float, heading_deg: float) -> dict[str, object]:
    runtime = get_runtime()
    session_id = get_current_session()

    now_epoch = time.time()
    can_lookup = await runtime.session_store.should_lookup_location(session_id, now_epoch=now_epoch, min_interval_s=30)
    if not can_lookup:
        return {'action': 'skip', 'reason': 'rate_limited_or_not_stationary'}

    await runtime.session_store.mark_location_lookup(session_id, now_epoch=now_epoch)

    crowd_hints = await runtime.session_store.get_crowd_hazard_hints(lat, lng, limit=3)
    priority_type, priority_label = _pick_priority_type(lat, lng, heading_deg)

    # Graceful fallback when maps grounding is unavailable.
    maps_enabled = os.getenv('ENABLE_MAPS_GROUNDING', '0').strip() in {'1', 'true', 'TRUE'}
    if not maps_enabled:
        hint_text = ' '.join(crowd_hints[:2]) if crowd_hints else 'No crowd hazards in this geohash yet.'
        return {
            'action': 'fallback',
            'name': 'Nearby area',
            'type': priority_type,
            'priority_label_vi': priority_label,
            'distance_m': None,
            'relevant_info': f'Maps grounding unavailable; using visual context only. VN priority context: {priority_label}. Crowd memory: {hint_text}',
            'heading_deg': heading_deg,
            'crowd_hazards': crowd_hints,
        }

    # Placeholder grounding logic with deterministic result for demo reproducibility.
    rounded_lat = round(lat, 4)
    rounded_lng = round(lng, 4)
    synthetic_distance = int(abs(math.sin(math.radians(heading_deg))) * 40) + 10
    return {
        'action': 'identified',
        'name': f'POI {rounded_lat},{rounded_lng}',
        'type': priority_type,
        'priority_label_vi': priority_label,
        'distance_m': synthetic_distance,
        'destination_near': synthetic_distance <= 30,
        'relevant_info': (
            f'{priority_label} nearby. Use caution around hem exits, parked motorbikes, and wet pavement.'
            + (f" Crowd memory: {'; '.join(crowd_hints[:2])}." if crowd_hints else '')
        ),
        'heading_deg': heading_deg,
        'crowd_hazards': crowd_hints,
    }
