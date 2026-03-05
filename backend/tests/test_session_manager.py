import unittest

from agent.session_manager import SessionStore, clock_face_from_delta, encode_geohash


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
        await store.touch_session('session-3', heading_deg=12.0)
        await store.add_spatial_hazard_memory(
            session_id='session-3',
            hazard_type='stairs',
            yaw_at_detection=15.0,
            position_description='Near entrance',
        )
        context = await store.get_spatial_context('session-3', current_yaw=20.0)
        self.assertIn('stairs', context)

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


if __name__ == '__main__':
    unittest.main()
