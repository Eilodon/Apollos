import unittest

try:
    from agent.human_fallback import HumanFallbackConfig, HumanFallbackError, HumanFallbackManager
except ModuleNotFoundError:  # pragma: no cover - package-style fallback
    from backend.agent.human_fallback import HumanFallbackConfig, HumanFallbackError, HumanFallbackManager


class HumanFallbackTests(unittest.TestCase):
    def _config(self) -> HumanFallbackConfig:
        return HumanFallbackConfig(
            enabled=True,
            signing_key='unit-test-human-fallback-signing-key-1234567890',
            issuer='apollos-human-help',
            public_help_base='https://example.com/help',
            help_ticket_ttl_seconds=180,
            viewer_token_ttl_seconds=300,
            emergency_contacts=(),
            twilio_account_sid='',
            twilio_auth_token='',
            twilio_from_number='',
            rtc_provider='twilio',
            twilio_video_api_key_sid='',
            twilio_video_api_key_secret='',
            twilio_video_room_prefix='apollos-help',
            twilio_video_token_ttl_seconds=900,
        )

    def test_help_ticket_exchange_only_once(self) -> None:
        manager = HumanFallbackManager(self._config())
        link = manager.build_help_link('session-123', reason='manual_sos')
        ticket = link.split('help_ticket=', 1)[1]
        first = manager.exchange_help_ticket(ticket)
        self.assertEqual(first.get('session_id'), 'session-123')
        self.assertTrue(bool(first.get('viewer_token')))
        with self.assertRaises(HumanFallbackError):
            manager.exchange_help_ticket(ticket)

    def test_viewer_token_session_binding(self) -> None:
        manager = HumanFallbackManager(self._config())
        ticket = manager.build_help_link('session-abc').split('help_ticket=', 1)[1]
        exchanged = manager.exchange_help_ticket(ticket)
        viewer_token = str(exchanged.get('viewer_token'))
        claims = manager.verify_viewer_token(viewer_token, session_id='session-abc')
        self.assertEqual(claims.get('sub'), 'session-abc')
        with self.assertRaises(HumanFallbackError):
            manager.verify_viewer_token(viewer_token, session_id='session-other')

    def test_twilio_rtc_payload_when_configured(self) -> None:
        cfg = HumanFallbackConfig(
            enabled=True,
            signing_key='unit-test-human-fallback-signing-key-1234567890',
            issuer='apollos-human-help',
            public_help_base='https://example.com/help',
            help_ticket_ttl_seconds=180,
            viewer_token_ttl_seconds=300,
            emergency_contacts=(),
            twilio_account_sid='AC1234567890abcdef1234567890abcd',
            twilio_auth_token='',
            twilio_from_number='',
            rtc_provider='twilio',
            twilio_video_api_key_sid='SK1234567890abcdef1234567890abcd',
            twilio_video_api_key_secret='twilio-video-api-secret-test-0123456789',
            twilio_video_room_prefix='apollos-help',
            twilio_video_token_ttl_seconds=900,
        )
        manager = HumanFallbackManager(cfg)
        session = manager.create_help_session('session-rtc')
        self.assertIsInstance(session.get('rtc'), dict)
        link = str(session.get('help_link'))
        exchange = manager.exchange_help_ticket(link.split('help_ticket=', 1)[1])
        self.assertIsInstance(exchange.get('rtc'), dict)


if __name__ == '__main__':
    unittest.main()
