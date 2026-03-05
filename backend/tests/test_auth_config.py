import os
import unittest

try:
    from agent.auth import load_auth_config_from_env
except ModuleNotFoundError:  # pragma: no cover - package-style fallback
    from backend.agent.auth import load_auth_config_from_env


class AuthConfigTests(unittest.TestCase):
    KEYS = (
        'WS_AUTH_MODE',
        'WS_AUTH_TOKEN',
        'OIDC_ISSUER',
        'OIDC_AUDIENCE',
        'OIDC_JWKS_URL',
        'OIDC_ALGORITHMS',
        'OIDC_LEEWAY_SECONDS',
    )

    def setUp(self) -> None:
        self._original = {key: os.environ.get(key) for key in self.KEYS}
        for key in self.KEYS:
            os.environ.pop(key, None)

    def tearDown(self) -> None:
        for key in self.KEYS:
            value = self._original.get(key)
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value

    def test_default_config_uses_shared_token_mode(self) -> None:
        config = load_auth_config_from_env()
        self.assertEqual(config.mode, 'shared_token')
        self.assertEqual(config.shared_token, '')
        self.assertEqual(config.audience, ())

    def test_oidc_config_derives_jwks_url_from_issuer(self) -> None:
        os.environ['WS_AUTH_MODE'] = 'oidc'
        os.environ['OIDC_ISSUER'] = 'https://issuer.example.com/'
        os.environ['OIDC_AUDIENCE'] = 'apollos-api'
        config = load_auth_config_from_env()

        self.assertEqual(config.mode, 'oidc')
        self.assertEqual(config.issuer, 'https://issuer.example.com')
        self.assertEqual(config.jwks_url, 'https://issuer.example.com/.well-known/jwks.json')
        self.assertEqual(config.audience, ('apollos-api',))


if __name__ == '__main__':
    unittest.main()
