from __future__ import annotations

import secrets
import threading
import time
from dataclasses import dataclass
from typing import Any

import jwt


class BrokerAuthError(Exception):
    pass


@dataclass(frozen=True, slots=True)
class BrokerConfig:
    signing_key: str
    issuer: str
    audience: tuple[str, ...]
    ws_ttl_seconds: int
    session_ttl_seconds: int
    cookie_name: str
    cookie_secure: bool
    cookie_samesite: str
    cookie_domain: str
    cookie_path: str


@dataclass(slots=True)
class _BrokerSession:
    session_id: str
    claims: dict[str, Any]
    expires_epoch: float
    created_epoch: float


class OIDCBrokerManager:
    def __init__(self, config: BrokerConfig) -> None:
        if not config.signing_key.strip():
            raise BrokerAuthError('OIDC broker signing key is required.')
        self._config = config
        self._sessions: dict[str, _BrokerSession] = {}
        self._lock = threading.Lock()

    @property
    def cookie_name(self) -> str:
        return self._config.cookie_name

    @property
    def session_ttl_seconds(self) -> int:
        return self._config.session_ttl_seconds

    @property
    def ws_ttl_seconds(self) -> int:
        return self._config.ws_ttl_seconds

    @property
    def cookie_secure(self) -> bool:
        return self._config.cookie_secure

    @property
    def cookie_samesite(self) -> str:
        return self._config.cookie_samesite

    @property
    def cookie_domain(self) -> str:
        return self._config.cookie_domain

    @property
    def cookie_path(self) -> str:
        return self._config.cookie_path

    def create_session(self, oidc_claims: dict[str, Any]) -> str:
        now_epoch = time.time()
        session_id = secrets.token_urlsafe(32)
        session = _BrokerSession(
            session_id=session_id,
            claims=dict(oidc_claims),
            expires_epoch=now_epoch + self._config.session_ttl_seconds,
            created_epoch=now_epoch,
        )
        with self._lock:
            self._prune_locked(now_epoch)
            self._sessions[session_id] = session
        return session_id

    def revoke_session(self, session_id: str) -> None:
        with self._lock:
            self._sessions.pop(session_id, None)

    def issue_ws_ticket(self, session_id: str) -> tuple[str, int]:
        now_epoch = time.time()
        with self._lock:
            self._prune_locked(now_epoch)
            session = self._sessions.get(session_id)
            if session is None:
                raise BrokerAuthError('Invalid broker session.')
            if session.expires_epoch <= now_epoch:
                self._sessions.pop(session_id, None)
                raise BrokerAuthError('Broker session expired.')

        expires_in = min(self._config.ws_ttl_seconds, int(max(1, session.expires_epoch - now_epoch)))
        subject = str(session.claims.get('sub') or session.claims.get('email') or 'anonymous')
        token_payload: dict[str, Any] = {
            'iss': self._config.issuer,
            'sub': subject,
            'aud': list(self._config.audience) if len(self._config.audience) > 1 else self._config.audience[0],
            'iat': int(now_epoch),
            'exp': int(now_epoch + expires_in),
            'sid': session_id,
            'auth_time': session.claims.get('auth_time'),
            'email': session.claims.get('email'),
            'name': session.claims.get('name'),
            'scope': 'apollos.ws',
        }
        token = jwt.encode(token_payload, self._config.signing_key, algorithm='HS256')
        return token, expires_in

    def verify_ws_ticket(self, token: str) -> dict[str, Any]:
        if not token or not token.strip():
            raise BrokerAuthError('Missing broker token.')

        try:
            claims = jwt.decode(
                token,
                self._config.signing_key,
                algorithms=['HS256'],
                audience=list(self._config.audience),
                issuer=self._config.issuer,
                options={
                    'require': ['exp', 'sub', 'sid'],
                    'verify_signature': True,
                    'verify_exp': True,
                    'verify_aud': True,
                    'verify_iss': True,
                },
            )
        except Exception as exc:
            raise BrokerAuthError(f'Invalid broker token: {exc}') from exc

        sid = str(claims.get('sid', '')).strip()
        if not sid:
            raise BrokerAuthError('Broker token missing sid.')

        now_epoch = time.time()
        with self._lock:
            self._prune_locked(now_epoch)
            session = self._sessions.get(sid)
            if session is None:
                raise BrokerAuthError('Broker session not found.')
            if session.expires_epoch <= now_epoch:
                self._sessions.pop(sid, None)
                raise BrokerAuthError('Broker session expired.')

        return claims

    def _prune_locked(self, now_epoch: float) -> None:
        expired = [sid for sid, session in self._sessions.items() if session.expires_epoch <= now_epoch]
        for sid in expired:
            self._sessions.pop(sid, None)
