# VisionGPT (ARIA) Contest MVP

Real-time AI navigation assistant blueprint implementation for blind and low-vision users.

## What is implemented

- React + TypeScript PWA frontend with:
  - Live camera capture (adaptive duty cycling by motion state)
  - Mic streaming with `echoCancellation: true`
  - Dual websocket channels (`/ws/live` + `/ws/emergency`)
  - HRTF spatial audio engine + sonar ping matrix (`100/400/800ms`)
  - Wake Lock + OLED black mode + keepalive fallback
  - Gesture mapping (tap, double tap, long press, swipe up/down, shake SOS)
  - Hazard compass + transcript + mode indicator
- FastAPI backend with:
  - Gemini Live API bridge in stream path (audio realtime + multimodal frame turns)
  - Live tool-calling dispatch to local safety tools (`log_hazard_event` etc.)
  - Session store + optional Firestore persistence
  - Tool-style functions:
    - `log_hazard_event(hazard_type, position_x, distance_category, confidence, description, session_id)`
    - `set_navigation_mode(mode)`
    - `log_emotion_event(state, confidence)`
    - `get_context_summary()`
    - `request_human_help()`
  - HARD_STOP pipeline over emergency websocket channel
  - HARD_STOP server timestamp fields for latency benchmarking (`server_emit_ts_ms`)
  - RunConfig builder with ADK-compatible path + local fallback
- Infra and delivery:
  - Terraform for Cloud Run + Firestore + IAM
  - GitHub Actions deployment/test workflow
  - Submission docs and script checklist

## Repository layout

- `frontend/`: PWA app
- `backend/`: FastAPI + agent orchestration
- `infra/`: Terraform IaC
- `docs/`: architecture, system prompt, submission checklist
- `assets/`: local alert audio asset placeholder

## Local development

### 1) Backend

```bash
cd backend
python -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
uvicorn main:app --reload --host 0.0.0.0 --port 8000
```

Health check:

```bash
curl http://localhost:8000/healthz
```

Manual HARD_STOP trigger:

```bash
curl -X POST http://localhost:8000/dev/hazard/demo-session \
  -H "content-type: application/json" \
  -d '{"hazard_type":"drop","position_x":0.8,"distance":"very_close","confidence":0.95}'
```

### 2) Frontend

```bash
cd frontend
npm install
npm run dev
```

Open `http://localhost:5173`.

### 3) Environment

Copy `.env.example` values into:

- root `.env` (for reference)
- `frontend/.env`
- `backend/.env`

For Gemini Live production path, set at least:

- `ENABLE_GEMINI_LIVE=1`
- `GOOGLE_API_KEY=...` (or `GEMINI_API_KEY`)
- `GEMINI_MODEL=gemini-live-2.5-flash-native-audio`

Optional Vertex path:

- `GEMINI_USE_VERTEX=1`
- `GOOGLE_CLOUD_PROJECT=...`
- `GOOGLE_CLOUD_LOCATION=us-central1`

## WebSocket contracts

### Client -> backend: `multimodal_frame`

```json
{
  "type": "multimodal_frame",
  "session_id": "...",
  "timestamp": "2026-03-04T00:00:00Z",
  "frame_jpeg_base64": "...",
  "motion_state": "walking_fast",
  "pitch": 8.2,
  "velocity": 2.1
}
```

### Client -> backend: `audio_chunk`

```json
{
  "type": "audio_chunk",
  "session_id": "...",
  "timestamp": "2026-03-04T00:00:00Z",
  "audio_chunk_pcm16": "...base64..."
}
```

### Backend -> client: `HARD_STOP`

```json
{
  "type": "HARD_STOP",
  "position_x": 0.8,
  "distance": "very_close",
  "hazard_type": "drop",
  "confidence": 0.92,
  "server_emit_ts_ms": 1772580000123
}
```

## Testing

Run backend unit tests:

```bash
PYTHONPATH=backend python -m unittest discover backend/tests
```

Run hardening static checks:

```bash
python3 scripts/hardening_pass.py
```

Run hardening with integration checks against a running backend:

```bash
python3 scripts/hardening_pass.py --integration --budget-ms 100 --iterations 20
```

Run benchmark only:

```bash
python3 scripts/benchmark_hard_stop.py --iterations 20 --budget-ms 100
```

If integration dependencies are unavailable, run internal latency benchmark:

```bash
PYTHONPATH=backend python3 scripts/benchmark_hard_stop_internal.py --iterations 100 --budget-ms 100
```

## Deployment (Terraform)

```bash
cd infra
terraform init
terraform apply \
  -var="project_id=YOUR_PROJECT" \
  -var="container_image=gcr.io/YOUR_PROJECT/aria-backend:latest"
```

## Architecture: Layer 0 Fallback (Upcoming)

Whilst Gemini Live API BIDI streaming (Layer 2/3) provides the core multimodal intelligence, we are planning a **Layer 0 TFLite Fallback** stub. This will utilize on-device Firebase ML Kit / TFLite object detection to ensure basic obstacle and drop-off alerts remain functional even during complete network drops.

## Known limitations in this MVP

- **iOS Web Audio Quirks:** Background execution on iOS Safari is heavily restricted. The app may suspend spatial audio or microphone streaming when the screen is locked unless specific PWA capabilities are granted.
- Exact Live API config compatibility can vary by SDK/model version.
- `assets/alert_ping.mp3` spatial depths are mapped but may vary slightly based on the HRTF implementation of the user's browser.

## Hardware Requirements & Safety Note

**Mandatory:** For real-world usage, **open-ear or bone-conduction headphones (e.g., Shokz)** are strongly required. 
Using standard noise-canceling or in-ear headphones will block environmental sounds (traffic, pedestrians), which is a critical safety hazard for blind and low-vision users. Always preserve your natural acoustic awareness.
