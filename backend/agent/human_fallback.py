from __future__ import annotations

import asyncio
import base64
import json
import os
import secrets
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any

import jwt


class HumanFallbackError(Exception):
    pass


@dataclass(frozen=True, slots=True)
class HumanFallbackConfig:
    enabled: bool
    signing_key: str
    issuer: str
    public_help_base: str
    help_ticket_ttl_seconds: int
    viewer_token_ttl_seconds: int
    emergency_contacts: tuple[str, ...]
    twilio_account_sid: str
    twilio_auth_token: str
    twilio_from_number: str
    rtc_provider: str
    twilio_video_api_key_sid: str
    twilio_video_api_key_secret: str
    twilio_video_room_prefix: str
    twilio_video_token_ttl_seconds: int


def _env_flag(name: str, default: bool) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.strip().lower() not in {'0', 'false', 'off', 'no'}


def build_human_fallback_config(app_env: str) -> HumanFallbackConfig:
    enabled = _env_flag('HUMAN_FALLBACK_ENABLED', True)
    signing_key = os.getenv('HUMAN_HELP_SIGNING_KEY', '').strip()
    if enabled and not signing_key and app_env in {'prod', 'production'}:
        raise RuntimeError('HUMAN_HELP_SIGNING_KEY is required in production when HUMAN_FALLBACK_ENABLED=1.')
    if enabled and not signing_key:
        signing_key = secrets.token_urlsafe(48)

    contacts = tuple(
        item.strip()
        for item in os.getenv('EMERGENCY_CONTACTS', '').split(',')
        if item.strip()
    )
    help_base = os.getenv('PUBLIC_HELP_BASE', 'https://example.com/help').rstrip('/')
    return HumanFallbackConfig(
        enabled=enabled,
        signing_key=signing_key,
        issuer=os.getenv('HUMAN_HELP_ISSUER', 'apollos-human-help').strip() or 'apollos-human-help',
        public_help_base=help_base,
        help_ticket_ttl_seconds=max(60, int(os.getenv('HUMAN_HELP_TICKET_TTL_SECONDS', '300') or 300)),
        viewer_token_ttl_seconds=max(60, int(os.getenv('HUMAN_HELP_VIEWER_TOKEN_TTL_SECONDS', '900') or 900)),
        emergency_contacts=contacts,
        twilio_account_sid=os.getenv('TWILIO_ACCOUNT_SID', '').strip(),
        twilio_auth_token=os.getenv('TWILIO_AUTH_TOKEN', '').strip(),
        twilio_from_number=os.getenv('TWILIO_FROM_NUMBER', '').strip(),
        rtc_provider=(os.getenv('HELP_RTC_PROVIDER', 'twilio').strip().lower() or 'twilio'),
        twilio_video_api_key_sid=os.getenv('TWILIO_VIDEO_API_KEY_SID', '').strip(),
        twilio_video_api_key_secret=os.getenv('TWILIO_VIDEO_API_KEY_SECRET', '').strip(),
        twilio_video_room_prefix=(os.getenv('TWILIO_VIDEO_ROOM_PREFIX', 'apollos-help').strip() or 'apollos-help'),
        twilio_video_token_ttl_seconds=max(60, int(os.getenv('TWILIO_VIDEO_TOKEN_TTL_SECONDS', '900') or 900)),
    )


class HumanFallbackManager:
    def __init__(self, config: HumanFallbackConfig) -> None:
        self._config = config
        self._used_ticket_jti: dict[str, float] = {}
        self._lock = threading.Lock()

    @property
    def enabled(self) -> bool:
        return self._config.enabled

    def build_help_link(self, session_id: str, reason: str = 'manual') -> str:
        return self.create_help_session(session_id, reason=reason).get('help_link', '')

    def create_help_session(self, session_id: str, reason: str = 'manual') -> dict[str, Any]:
        if not self._config.enabled:
            return {
                'help_link': f"{self._config.public_help_base}?session={urllib.parse.quote(session_id)}",
                'rtc': None,
            }
        now_epoch = int(time.time())
        rtc_room = ''
        rtc_identity = ''
        rtc_payload: dict[str, Any] | None = None
        if self._is_twilio_video_enabled():
            rtc_room = f'{self._config.twilio_video_room_prefix}-{secrets.token_hex(8)}'
            rtc_identity = f'patient-{secrets.token_hex(6)}'
            rtc_token, rtc_expires_in = self._mint_twilio_video_token(
                identity=rtc_identity,
                room_name=rtc_room,
                ttl_seconds=self._config.twilio_video_token_ttl_seconds,
            )
            rtc_payload = {
                'provider': 'twilio',
                'room_name': rtc_room,
                'identity': rtc_identity,
                'token': rtc_token,
                'expires_in': rtc_expires_in,
            }
        ticket = jwt.encode(
            {
                'iss': self._config.issuer,
                'aud': 'apollos.help.ticket',
                'sub': session_id,
                'scope': 'help.ticket',
                'reason': reason,
                'rtc_provider': rtc_payload.get('provider', '') if rtc_payload else '',
                'rtc_room': rtc_room,
                'iat': now_epoch,
                'exp': now_epoch + self._config.help_ticket_ttl_seconds,
                'jti': secrets.token_urlsafe(24),
            },
            self._config.signing_key,
            algorithm='HS256',
        )
        encoded_ticket = urllib.parse.quote(ticket, safe='')
        return {
            'help_link': f"{self._config.public_help_base}?help_ticket={encoded_ticket}",
            'rtc': rtc_payload,
        }

    def exchange_help_ticket(self, ticket: str) -> dict[str, Any]:
        if not self._config.enabled:
            raise HumanFallbackError('Human fallback is disabled.')
        normalized_ticket = urllib.parse.unquote(ticket.strip())
        claims = self._decode_token(normalized_ticket, expected_audience='apollos.help.ticket')
        if str(claims.get('scope', '')) != 'help.ticket':
            raise HumanFallbackError('Invalid help ticket scope.')

        session_id = str(claims.get('sub', '')).strip()
        jti = str(claims.get('jti', '')).strip()
        if not session_id or not jti:
            raise HumanFallbackError('Invalid help ticket payload.')

        now_epoch = time.time()
        with self._lock:
            self._prune_used_tickets_locked(now_epoch)
            if jti in self._used_ticket_jti:
                raise HumanFallbackError('Help ticket has already been used.')
            self._used_ticket_jti[jti] = now_epoch

        viewer_token, expires_in = self._mint_viewer_token(session_id=session_id)
        rtc = self._build_viewer_rtc_from_ticket(claims)
        result = {
            'session_id': session_id,
            'viewer_token': viewer_token,
            'expires_in': expires_in,
        }
        if rtc is not None:
            result['rtc'] = rtc
        return result

    def verify_viewer_token(self, token: str, session_id: str) -> dict[str, Any]:
        if not self._config.enabled:
            raise HumanFallbackError('Human fallback is disabled.')
        claims = self._decode_token(token, expected_audience='apollos.help.viewer')
        if str(claims.get('scope', '')) != 'help.viewer':
            raise HumanFallbackError('Invalid viewer token scope.')
        token_session = str(claims.get('sub', '')).strip()
        if not token_session or token_session != session_id:
            raise HumanFallbackError('Viewer token session mismatch.')
        return claims

    def _build_viewer_rtc_from_ticket(self, claims: dict[str, Any]) -> dict[str, Any] | None:
        provider = str(claims.get('rtc_provider', '')).strip().lower()
        if provider != 'twilio' or not self._is_twilio_video_enabled():
            return None
        room_name = str(claims.get('rtc_room', '')).strip()
        if not room_name:
            return None
        viewer_identity = f'helper-{secrets.token_hex(6)}'
        token, expires_in = self._mint_twilio_video_token(
            identity=viewer_identity,
            room_name=room_name,
            ttl_seconds=self._config.twilio_video_token_ttl_seconds,
        )
        return {
            'provider': 'twilio',
            'room_name': room_name,
            'identity': viewer_identity,
            'token': token,
            'expires_in': expires_in,
        }

    async def notify_contacts(self, help_link: str, reason: str) -> dict[str, Any]:
        contacts = self._config.emergency_contacts
        if not contacts:
            return {'enabled': False, 'sent': 0, 'errors': ['EMERGENCY_CONTACTS is empty']}

        if not (
            self._config.twilio_account_sid
            and self._config.twilio_auth_token
            and self._config.twilio_from_number
        ):
            return {
                'enabled': False,
                'sent': 0,
                'errors': ['Twilio credentials are not configured'],
            }

        sent = 0
        errors: list[str] = []
        for to_number in contacts:
            ok, detail = await asyncio.to_thread(self._send_sms_via_twilio, to_number, help_link, reason)
            if ok:
                sent += 1
            else:
                errors.append(f'{to_number}: {detail}')

        return {'enabled': True, 'sent': sent, 'errors': errors}

    def _mint_viewer_token(self, session_id: str) -> tuple[str, int]:
        now_epoch = int(time.time())
        expires_in = self._config.viewer_token_ttl_seconds
        viewer_token = jwt.encode(
            {
                'iss': self._config.issuer,
                'aud': 'apollos.help.viewer',
                'sub': session_id,
                'scope': 'help.viewer',
                'iat': now_epoch,
                'exp': now_epoch + expires_in,
                'jti': secrets.token_urlsafe(24),
            },
            self._config.signing_key,
            algorithm='HS256',
        )
        return viewer_token, expires_in

    def _is_twilio_video_enabled(self) -> bool:
        return (
            self._config.rtc_provider == 'twilio'
            and bool(self._config.twilio_account_sid)
            and bool(self._config.twilio_video_api_key_sid)
            and bool(self._config.twilio_video_api_key_secret)
        )

    def _mint_twilio_video_token(self, *, identity: str, room_name: str, ttl_seconds: int) -> tuple[str, int]:
        if not self._is_twilio_video_enabled():
            raise HumanFallbackError('Twilio Video is not configured.')

        now_epoch = int(time.time())
        expires_in = max(60, ttl_seconds)
        payload = {
            'jti': f"{self._config.twilio_video_api_key_sid}-{secrets.token_hex(12)}",
            'iss': self._config.twilio_video_api_key_sid,
            'sub': self._config.twilio_account_sid,
            'iat': now_epoch,
            'exp': now_epoch + expires_in,
            'grants': {
                'identity': identity,
                'video': {'room': room_name},
            },
        }
        token = jwt.encode(
            payload,
            self._config.twilio_video_api_key_secret,
            algorithm='HS256',
            headers={
                'typ': 'JWT',
                'cty': 'twilio-fpa;v=1',
            },
        )
        return token, expires_in

    def _decode_token(self, token: str, *, expected_audience: str) -> dict[str, Any]:
        try:
            return jwt.decode(
                token,
                self._config.signing_key,
                algorithms=['HS256'],
                audience=expected_audience,
                issuer=self._config.issuer,
                options={
                    'require': ['exp', 'iat', 'jti', 'sub', 'scope'],
                    'verify_exp': True,
                    'verify_signature': True,
                    'verify_aud': True,
                    'verify_iss': True,
                },
            )
        except Exception as exc:
            raise HumanFallbackError(f'Invalid token: {exc}') from exc

    def _prune_used_tickets_locked(self, now_epoch: float) -> None:
        retention = float(self._config.help_ticket_ttl_seconds + 600)
        expired = [jti for jti, used_at in self._used_ticket_jti.items() if now_epoch - used_at > retention]
        for jti in expired:
            self._used_ticket_jti.pop(jti, None)

    def _send_sms_via_twilio(self, to_number: str, help_link: str, reason: str) -> tuple[bool, str]:
        account_sid = self._config.twilio_account_sid
        auth_token = self._config.twilio_auth_token
        from_number = self._config.twilio_from_number
        if not account_sid or not auth_token or not from_number:
            return False, 'missing Twilio configuration'

        api_url = f'https://api.twilio.com/2010-04-01/Accounts/{account_sid}/Messages.json'
        body_text = (
            'Apollos emergency assist requested.\n'
            f'Reason: {reason}\n'
            f'Join live help: {help_link}'
        )
        payload = urllib.parse.urlencode(
            {
                'To': to_number,
                'From': from_number,
                'Body': body_text,
            }
        ).encode('utf-8')
        basic = base64.b64encode(f'{account_sid}:{auth_token}'.encode('utf-8')).decode('ascii')
        request = urllib.request.Request(
            api_url,
            data=payload,
            method='POST',
            headers={
                'Authorization': f'Basic {basic}',
                'Content-Type': 'application/x-www-form-urlencoded',
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=8) as response:
                raw = response.read().decode('utf-8', errors='ignore')
            parsed = json.loads(raw or '{}')
            sid = str(parsed.get('sid', '')).strip()
            if sid:
                return True, sid
            return False, f'No sid in Twilio response ({raw[:200]})'
        except urllib.error.HTTPError as exc:
            body = exc.read().decode('utf-8', errors='ignore') if hasattr(exc, 'read') else str(exc)
            return False, f'HTTP {exc.code}: {body[:200]}'
        except Exception as exc:
            return False, str(exc)
