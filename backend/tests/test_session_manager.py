import unittest

from agent.session_manager import SessionStore


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


if __name__ == '__main__':
    unittest.main()
