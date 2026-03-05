#!/usr/bin/env python3
from __future__ import annotations

import asyncio
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'backend'))

from agent.session_manager import SessionStore  # noqa: E402


async def run_dummy_e2e() -> None:
    store = SessionStore(use_firestore=False)
    session_id = 'wave3-crowd-e2e'
    lat = 10.78032
    lng = 106.69918

    await store.touch_session(session_id, lat=lat, lng=lng, heading_deg=12.0)

    # Simulate repeated confirmations in the same area.
    await store.log_hazard(
        session_id,
        'xe máy',
        position_x=0.25,
        distance_category='mid',
        confidence=0.91,
        description='xe may dau tren via he',
    )
    await store.log_hazard(
        session_id,
        'open_drain',
        position_x=-0.1,
        distance_category='very_close',
        confidence=0.96,
        description='ho ga khong nap',
    )
    await store.log_hazard(
        session_id,
        'xe máy',
        position_x=0.2,
        distance_category='mid',
        confidence=0.9,
        description='xe may tai xuat hien',
    )

    hints = await store.get_crowd_hazard_hints(lat, lng, limit=3)
    print('Crowd hazard hints:')
    for idx, hint in enumerate(hints, start=1):
        print(f'{idx}. {hint}')

    if not hints:
        raise AssertionError('Expected at least one crowd hazard hint.')
    if not any('Xe máy' in hint or 'xe máy' in hint for hint in hints):
        raise AssertionError('Expected parked motorbike taxonomy hint in output.')
    if not any('lần xác nhận' in hint for hint in hints):
        raise AssertionError('Expected confirmation count to be included in hints.')

    print('Dummy crowd hazard map E2E: PASS')


def main() -> None:
    asyncio.run(run_dummy_e2e())


if __name__ == '__main__':
    main()
