import time
import unittest

try:
    from agent.broker_auth import BrokerAuthError, BrokerConfig, OIDCBrokerManager
except ModuleNotFoundError:  # pragma: no cover - package-style fallback
    from backend.agent.broker_auth import BrokerAuthError, BrokerConfig, OIDCBrokerManager


class BrokerAuthTests(unittest.TestCase):
    def _config(self, *, ws_ttl: int = 90, session_ttl: int = 3600) -> BrokerConfig:
        return BrokerConfig(
            signing_key='unit-test-signing-key-0123456789abcdef',
            issuer='apollos-oidc-broker',
            audience=('apollos-api',),
            ws_ttl_seconds=ws_ttl,
            session_ttl_seconds=session_ttl,
            cookie_name='apollos_broker_session',
            cookie_secure=False,
            cookie_samesite='lax',
            cookie_domain='',
            cookie_path='/',
        )

    def test_issue_and_verify_ws_ticket(self) -> None:
        manager = OIDCBrokerManager(self._config())
        session_id = manager.create_session({'sub': 'user-123', 'email': 'user@example.com'})
        token, expires_in = manager.issue_ws_ticket(session_id)

        self.assertGreater(expires_in, 0)
        claims = manager.verify_ws_ticket(token)
        self.assertEqual(claims.get('sub'), 'user-123')
        self.assertEqual(claims.get('sid'), session_id)

    def test_revoked_session_invalidates_ticket(self) -> None:
        manager = OIDCBrokerManager(self._config())
        session_id = manager.create_session({'sub': 'user-abc'})
        token, _ = manager.issue_ws_ticket(session_id)
        manager.revoke_session(session_id)

        with self.assertRaises(BrokerAuthError):
            manager.verify_ws_ticket(token)

    def test_expired_session_rejected(self) -> None:
        manager = OIDCBrokerManager(self._config(ws_ttl=60, session_ttl=1))
        session_id = manager.create_session({'sub': 'user-exp'})
        # Force expiry without sleeping.
        manager._sessions[session_id].expires_epoch = time.time() - 1  # type: ignore[attr-defined]

        with self.assertRaises(BrokerAuthError):
            manager.issue_ws_ticket(session_id)


if __name__ == '__main__':
    unittest.main()
