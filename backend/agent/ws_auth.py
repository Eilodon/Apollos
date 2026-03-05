from __future__ import annotations

import base64
import binascii
import logging
from typing import Mapping


_PROD_ENVS = {'prod', 'production'}
_FALSEY = {'0', 'false', 'off', 'no'}


def resolve_allow_query_token(app_env: str, configured_value: str | None) -> bool:
    normalized_env = (app_env or '').strip().lower()
    if configured_value is None or not configured_value.strip():
        return normalized_env not in _PROD_ENVS
    return configured_value.strip().lower() not in _FALSEY


def extract_ws_token(
    *,
    headers: Mapping[str, str],
    query_params: Mapping[str, str],
    allow_query_token: bool,
    logger: logging.Logger | None = None,
) -> str:
    protocol_header = (headers.get('sec-websocket-protocol') or '').strip()
    for item in protocol_header.split(','):
        candidate = item.strip()
        if not candidate:
            continue
        if candidate.startswith('authb64.'):
            encoded = candidate[len('authb64.'):]
            if not encoded:
                continue
            padding = '=' * (-len(encoded) % 4)
            try:
                return base64.urlsafe_b64decode(encoded + padding).decode('utf-8').strip()
            except (ValueError, UnicodeDecodeError, binascii.Error):
                if logger:
                    logger.warning('Invalid authb64 websocket subprotocol token')
                continue
        if candidate.startswith('bearer.'):
            return candidate[7:].strip()

    ws_header_token = (headers.get('x-ws-token') or '').strip()
    if ws_header_token:
        return ws_header_token

    auth_header = (headers.get('authorization') or '').strip()
    if auth_header.lower().startswith('bearer '):
        return auth_header[7:].strip()

    query_token = (query_params.get('token') or '').strip()
    if query_token and allow_query_token:
        return query_token
    if query_token and not allow_query_token and logger:
        logger.warning('Rejected query-string websocket token while WS_ALLOW_QUERY_TOKEN=0')

    return ''


def select_ws_subprotocol(headers: Mapping[str, str], preferred: str = 'apollos.v1') -> str | None:
    protocol_header = (headers.get('sec-websocket-protocol') or '').strip()
    for item in protocol_header.split(','):
        candidate = item.strip()
        if candidate == preferred:
            return candidate
    return None
