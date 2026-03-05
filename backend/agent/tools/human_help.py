from __future__ import annotations

from .decorators import tool
from .runtime import get_current_session, get_runtime


@tool
async def request_human_help() -> dict[str, object]:
    runtime = get_runtime()
    session_id = get_current_session()
    manager = runtime.human_fallback_manager
    if manager is None:
        help_link = await runtime.session_store.build_human_help_link(session_id)
        return {'help_link': help_link}

    session = manager.create_help_session(session_id, reason='manual_sos')
    help_link = str(session.get('help_link', '')).strip()
    sms_result = await manager.notify_contacts(help_link, reason='manual_sos') if help_link else {'enabled': False, 'sent': 0, 'errors': []}
    return {
        'help_link': help_link,
        'rtc': session.get('rtc'),
        'sms': sms_result,
    }
