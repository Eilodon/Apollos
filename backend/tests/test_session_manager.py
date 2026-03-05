import unittest

try:
    from agent.session_manager import SessionStore, clock_face_from_delta, encode_geohash
except ModuleNotFoundError:  # pragma: no cover - package-style fallback
    from backend.agent.session_manager import SessionStore, clock_face_from_delta, encode_geohash


class SessionStoreTests(unittest.IsolatedAsyncioTestCase):
    async def test_set_mode_and_context_roundtrip(self) -> None:
        store = SessionStore(use_firestore=False)
        await store.set_mode('session-1', 'QUIET')
        await store.update_context_summary('session-1', 'User is indoors near reception desk')

        summary = await store.get_context_summary('session-1')
        self.assertIn('indoors', summary)

    async def test_log_hazard_and_emotion(self) -> None:
        store = SessionStore(use_firestore=False)
        await store.log_hazard('session-2', 'drop', 0.2, 'very_close', 0.92, 'Step down at right side')
        await store.log_emotion('session-2', 'stressed', 0.81)

        # No exceptions implies in-memory append + optional persistence path works.
        summary = await store.get_context_summary('session-2')
        self.assertIn('Mode=', summary)

    async def test_spatial_memory_context(self) -> None:
        store = SessionStore(use_firestore=False)
        await store.touch_session('session-3', heading_deg=12.0, lat=10.7801, lng=106.6999)
        await store.add_spatial_hazard_memory(
            session_id='session-3',
            hazard_type='stairs',
            yaw_at_detection=15.0,
            position_description='Near entrance',
        )
        context = await store.get_spatial_context(
            'session-3',
            current_yaw=20.0,
            current_lat=10.7802,
            current_lng=106.7001,
        )
        self.assertIn('stairs', context)

    async def test_spatial_memory_requires_geohash_prefix_match(self) -> None:
        store = SessionStore(use_firestore=False)
        await store.touch_session('session-geohash', heading_deg=5.0, lat=10.7801, lng=106.6999)
        await store.add_spatial_hazard_memory(
            session_id='session-geohash',
            hazard_type='open_drain',
            yaw_at_detection=8.0,
            position_description='Near curb edge',
        )

        nearby = await store.get_spatial_context(
            'session-geohash',
            current_yaw=6.0,
            current_lat=10.7803,
            current_lng=106.7000,
        )
        far_away = await store.get_spatial_context(
            'session-geohash',
            current_yaw=6.0,
            current_lat=10.9200,
            current_lng=106.9800,
        )

        self.assertIn('open_drain', nearby)
        self.assertEqual(far_away, '')

    async def test_stress_mode_override_auto_expiry(self) -> None:
        store = SessionStore(use_firestore=False)
        await store.set_mode('session-4', 'QUIET')
        await store.apply_stress_mode_override('session-4', reason='test', revert_after_seconds=120)
        mode = await store.get_effective_mode('session-4')
        self.assertEqual(mode, 'NAVIGATION')

    def test_geohash_precision_and_clock_mapping(self) -> None:
        geohash = encode_geohash(10.3602, 106.3598, precision=7)
        self.assertEqual(len(geohash), 7)
        self.assertEqual(clock_face_from_delta(-90), 9)

    async def test_mark_edge_hazard_sets_active_window(self) -> None:
        store = SessionStore(use_firestore=False)
        await store.mark_edge_hazard('session-5', hazard_type='DROP_AHEAD', suppress_seconds=2.0)
        active = await store.is_edge_hazard_active('session-5')
        self.assertTrue(active)

    async def test_read_operations_do_not_reset_motion_state_or_advance_frame(self) -> None:
        store = SessionStore(use_firestore=False)
        state = await store.touch_session('session-6', motion_state='running', advance_frame=True)
        self.assertEqual(state.motion_state, 'running')
        self.assertEqual(state.frame_sequence, 1)

        await store.get_effective_mode('session-6')
        await store.get_spatial_context('session-6', current_yaw=0)
        await store.is_edge_hazard_active('session-6')

        state_after = await store.ensure_session('session-6')
        self.assertEqual(state_after.motion_state, 'running')
        self.assertEqual(state_after.frame_sequence, 1)

    async def test_crowd_hints_use_vn_taxonomy_description(self) -> None:
        store = SessionStore(use_firestore=False)
        lat = 10.7801
        lng = 106.6999
        await store.touch_session('session-crowd-1', lat=lat, lng=lng, heading_deg=12.0)
        await store.log_hazard(
            'session-crowd-1',
            'xe máy',
            position_x=0.1,
            distance_category='mid',
            confidence=0.9,
            description='xe may dung tren via he',
        )

        hints = await store.get_crowd_hazard_hints(lat, lng, limit=3)
        self.assertTrue(hints)
        self.assertIn('Xe máy đậu chắn lối đi', hints[0])

    async def test_crowd_hints_apply_time_decay_and_remove_expired_entries(self) -> None:
        store = SessionStore(use_firestore=False)
        lat = 10.7802
        lng = 106.7002
        await store.touch_session('session-crowd-2', lat=lat, lng=lng, heading_deg=0.0)
        await store.log_hazard(
            'session-crowd-2',
            'open_drain',
            position_x=-0.2,
            distance_category='very_close',
            confidence=0.95,
            description='cong ho',
        )

        geohash = encode_geohash(lat, lng, precision=7)
        doc_id = f'{geohash}-open_drain'
        entry = store._crowd_hazard_map[doc_id]
        entry['confirmed_count'] = 1
        entry['last_confirmed'] = '2025-01-01T00:00:00+00:00'

        hints = await store.get_crowd_hazard_hints(lat, lng, limit=3)
        self.assertEqual(hints, [])
        self.assertNotIn(doc_id, store._crowd_hazard_map)

    async def test_crowd_hints_include_time_pattern_context_when_peak_hour_exists(self) -> None:
        store = SessionStore(use_firestore=False)
        lat = 10.781
        lng = 106.701
        await store.touch_session('session-crowd-3', lat=lat, lng=lng, heading_deg=90.0)
        await store.log_hazard(
            'session-crowd-3',
            'open_drain',
            position_x=0.0,
            distance_category='mid',
            confidence=0.88,
            description='open drain first',
        )
        await store.log_hazard(
            'session-crowd-3',
            'open_drain',
            position_x=0.1,
            distance_category='mid',
            confidence=0.9,
            description='open drain second',
        )

        hints = await store.get_crowd_hazard_hints(lat, lng, limit=3)
        self.assertTrue(hints)
        self.assertIn('hay xuất hiện vào', hints[0])


if __name__ == '__main__':
    unittest.main()
