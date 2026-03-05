from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Any

try:
    import jwt
    from jwt import PyJWKClient
except ModuleNotFoundError:  # pragma: no cover - optional dependency path
    jwt = None  # type: ignore[assignment]
    PyJWKClient = None  # type: ignore[assignment]


class OIDCAuthError(Exception):
    pass


@dataclass(frozen=True, slots=True)
class AuthConfig:
    mode: str
    shared_token: str
    issuer: str
    audience: tuple[str, ...]
    jwks_url: str
    algorithms: tuple[str, ...]
    leeway_seconds: int


def load_auth_config_from_env() -> AuthConfig:
    mode = os.getenv('WS_AUTH_MODE', 'shared_token').strip().lower() or 'shared_token'
    shared_token = os.getenv('WS_AUTH_TOKEN', '').strip()
    issuer = os.getenv('OIDC_ISSUER', '').strip().rstrip('/')
    audience = tuple(item.strip() for item in os.getenv('OIDC_AUDIENCE', '').split(',') if item.strip())
    jwks_url = os.getenv('OIDC_JWKS_URL', '').strip()
    if not jwks_url and issuer:
        jwks_url = f'{issuer}/.well-known/jwks.json'
    algorithms = tuple(
        item.strip().upper()
        for item in os.getenv('OIDC_ALGORITHMS', 'RS256,ES256').split(',')
        if item.strip()
    )
    leeway_seconds = int(os.getenv('OIDC_LEEWAY_SECONDS', '30') or 30)
    return AuthConfig(
        mode=mode,
        shared_token=shared_token,
        issuer=issuer,
        audience=audience,
        jwks_url=jwks_url,
        algorithms=algorithms,
        leeway_seconds=max(0, leeway_seconds),
    )


class OIDCVerifier:
    def __init__(self, config: AuthConfig) -> None:
        if jwt is None or PyJWKClient is None:
            raise OIDCAuthError('OIDC mode requires PyJWT[crypto].')
        if not config.issuer:
            raise OIDCAuthError('OIDC_ISSUER is required when WS_AUTH_MODE=oidc.')
        if not config.audience:
            raise OIDCAuthError('OIDC_AUDIENCE is required when WS_AUTH_MODE=oidc.')
        if not config.jwks_url:
            raise OIDCAuthError('OIDC_JWKS_URL (or OIDC_ISSUER) is required when WS_AUTH_MODE=oidc.')

        self._config = config
        self._jwk_client = PyJWKClient(config.jwks_url, cache_keys=True)

    def verify_token(self, token: str) -> dict[str, Any]:
        if not token or not token.strip():
            raise OIDCAuthError('Missing bearer token.')

        try:
            signing_key = self._jwk_client.get_signing_key_from_jwt(token).key
            claims = jwt.decode(
                token,
                signing_key,
                algorithms=list(self._config.algorithms),
                audience=list(self._config.audience),
                issuer=self._config.issuer,
                leeway=self._config.leeway_seconds,
                options={
                    'require': ['exp'],
                    'verify_signature': True,
                    'verify_exp': True,
                    'verify_aud': True,
                    'verify_iss': True,
                },
            )
        except Exception as exc:
            raise OIDCAuthError(f'Invalid OIDC token: {exc}') from exc

        return claims
