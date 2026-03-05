#!/usr/bin/env python3
"""Human verification flow stub for Apollos.

GAP 6: Provides a minimal /human-assist/{session_id} web page
that shows the last camera frame + transcript, with text input
for a human responder. This is an architecture proof, wired to
the safety_policy human_escalation tier.

NOTE: This is a development stub and intentionally minimal.
Production usage requires auth, rate limiting, and a real-time
bridge to the user's session.
"""
from __future__ import annotations

HUMAN_ASSIST_HTML_TEMPLATE = """<!DOCTYPE html>
<html lang="vi">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Apollos Human Assist – {session_id}</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    font-family: system-ui, sans-serif;
    background: #0a0a0a;
    color: #e0e0e0;
    padding: 20px;
    max-width: 600px;
    margin: 0 auto;
  }}
  h1 {{ font-size: 1.3rem; color: #00ffff; margin-bottom: 12px; }}
  .status {{ color: #ffd700; font-size: 0.9rem; margin-bottom: 16px; }}
  .frame-container {{
    background: #1a1a1a;
    border: 1px solid #333;
    border-radius: 8px;
    padding: 8px;
    margin-bottom: 16px;
    text-align: center;
  }}
  .frame-container img {{
    max-width: 100%;
    border-radius: 4px;
  }}
  .transcript {{
    background: #1a1a1a;
    border: 1px solid #333;
    border-radius: 8px;
    padding: 12px;
    margin-bottom: 16px;
    max-height: 200px;
    overflow-y: auto;
    font-size: 0.85rem;
  }}
  .transcript p {{ margin-bottom: 6px; }}
  .input-group {{
    display: flex;
    gap: 8px;
  }}
  .input-group input {{
    flex: 1;
    padding: 12px;
    border: 1px solid #444;
    border-radius: 8px;
    background: #222;
    color: #fff;
    font-size: 1rem;
  }}
  .input-group button {{
    padding: 12px 24px;
    background: #00ffff;
    color: #000;
    border: none;
    border-radius: 8px;
    font-weight: bold;
    cursor: pointer;
    font-size: 1rem;
  }}
  .input-group button:hover {{ background: #00e0e0; }}
</style>
</head>
<body>
  <h1>🧭 Apollos Human Assist</h1>
  <div class="status">Session: {session_id} | Status: {status}</div>

  <div class="frame-container">
    <img src="{frame_url}" alt="Last camera frame"
         onerror="this.src='data:image/svg+xml,<svg xmlns=&quot;http://www.w3.org/2000/svg&quot; width=&quot;300&quot; height=&quot;200&quot;><text x=&quot;50%&quot; y=&quot;50%&quot; text-anchor=&quot;middle&quot; fill=&quot;%23666&quot;>No frame available</text></svg>'">
  </div>

  <div class="transcript">
    <p><em>Recent transcript:</em></p>
    {transcript_html}
  </div>

  <form method="POST" action="/human-assist/{session_id}/respond">
    <div class="input-group">
      <input type="text" name="guidance" placeholder="Type guidance for the user..."
             autocomplete="off" autofocus required>
      <button type="submit">Send</button>
    </div>
  </form>
</body>
</html>"""


def render_human_assist_page(
    session_id: str,
    transcript_entries: list[str] | None = None,
    frame_url: str = '',
    status: str = 'awaiting human response',
) -> str:
    """Render the human-assist HTML page."""
    entries = transcript_entries or ['No transcript data available.']
    transcript_html = '\n'.join(f'<p>{entry}</p>' for entry in entries[-10:])
    return HUMAN_ASSIST_HTML_TEMPLATE.format(
        session_id=session_id,
        frame_url=frame_url or f'/api/frame/{session_id}/latest',
        transcript_html=transcript_html,
        status=status,
    )
