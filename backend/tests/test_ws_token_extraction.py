import base64
import unittest

try:
    from agent.ws_auth import extract_ws_token, resolve_allow_query_token
except ModuleNotFoundError:  # pragma: no cover - package-style fallback
    from backend.agent.ws_auth import extract_ws_token, resolve_allow_query_token


class WsTokenExtractionTests(unittest.TestCase):
    def test_extracts_authb64_token_from_subprotocol(self) -> None:
        raw = 'eyJhbGciOiJSUzI1NiJ9.payload.signature'
        encoded = base64.urlsafe_b64encode(raw.encode('utf-8')).decode('ascii').rstrip('=')
        token = extract_ws_token(
            headers={'sec-websocket-protocol': f'apollos.v1, authb64.{encoded}'},
            query_params={},
            allow_query_token=False,
        )

        self.assertEqual(token, raw)

    def test_query_token_is_disabled_when_policy_blocks_it(self) -> None:
        token = extract_ws_token(
            headers={},
            query_params={'token': 'legacy-query-token'},
            allow_query_token=False,
        )

        self.assertEqual(token, '')

    def test_query_token_is_allowed_for_legacy_fallback(self) -> None:
        token = extract_ws_token(
            headers={},
            query_params={'token': 'legacy-query-token'},
            allow_query_token=True,
        )

        self.assertEqual(token, 'legacy-query-token')

    def test_query_token_auto_disabled_for_production(self) -> None:
        self.assertFalse(resolve_allow_query_token('production', None))

    def test_query_token_auto_enabled_for_development(self) -> None:
        self.assertTrue(resolve_allow_query_token('development', None))


if __name__ == '__main__':
    unittest.main()
