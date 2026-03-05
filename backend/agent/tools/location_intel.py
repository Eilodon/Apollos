from __future__ import annotations

import math
import os
import time

from .decorators import tool
from .runtime import get_current_session, get_runtime


@tool
async def identify_location(lat: float, lng: float, heading_deg: float) -> dict[str, object]:
    runtime = get_runtime()
    session_id = get_current_session()

    now_epoch = time.time()
    can_lookup = await runtime.session_store.should_lookup_location(session_id, now_epoch=now_epoch, min_interval_s=30)
    if not can_lookup:
        return {'action': 'skip', 'reason': 'rate_limited_or_not_stationary'}

    await runtime.session_store.mark_location_lookup(session_id, now_epoch=now_epoch)

    # Graceful fallback when maps grounding is unavailable.
    maps_enabled = os.getenv('ENABLE_MAPS_GROUNDING', '0').strip() in {'1', 'true', 'TRUE'}
    if not maps_enabled:
        return {
            'action': 'fallback',
            'name': 'Nearby area',
            'type': 'unknown',
            'distance_m': 0,
            'relevant_info': 'Maps grounding unavailable; using visual context only.',
            'heading_deg': heading_deg,
        }

    # Placeholder grounding logic with deterministic result for demo reproducibility.
    rounded_lat = round(lat, 4)
    rounded_lng = round(lng, 4)
    synthetic_distance = int(abs(math.sin(math.radians(heading_deg))) * 40) + 10
    return {
        'action': 'identified',
        'name': f'POI {rounded_lat},{rounded_lng}',
        'type': 'transit' if synthetic_distance < 20 else 'shop',
        'distance_m': synthetic_distance,
        'relevant_info': 'Use caution around curb cuts and parked motorbikes.',
        'heading_deg': heading_deg,
    }
