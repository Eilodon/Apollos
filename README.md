# Apollos — ARIA Navigation Assistant

Real-time AI navigation assistant for blind and low-vision users. Built on **Dual-Brain Architecture**: Edge reflexes protect lives, Cloud cognition decodes the world.

## Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│  DUAL-BRAIN ARCHITECTURE (The Eidolon Standard)      │
├──────────────────┬──────────────────────────────────┤
│   SPINAL CORD    │          CORTEX                  │
│   (Edge / Local) │          (Cloud / Gemini)        │
├──────────────────┼──────────────────────────────────┤
│ Layer 0          │  Layer 2                         │
│ Survival Reflex  │  Semantic Cognition              │
│ Optical Flow TTC │  Gemini Live 2.5 Flash           │
│ <16ms latency    │  Scene understanding             │
│                  │  Empathic dialogue               │
├──────────────────┼──────────────────────────────────┤
│ Layer 0.5        │  Layer 3                         │
│ Pocket Shield    │  Cloud Fallback                  │
│ Ghost Touch block│  ADK Tool Calls                  │
│ AmbientLight API │  HARD_STOP dispatch              │
├──────────────────┼──────────────────────────────────┤
│ Layer 1          │                                  │
│ Kinematic Gating │                                  │
│ Dot Product tilt │                                  │
│ Anti-blur filter │                                  │
└──────────────────┴──────────────────────────────────┘
```

## What is implemented

### Frontend (React + TypeScript PWA)
- **Layer 0 — Survival Reflex Worker** (`survivalReflex.worker.ts`): Optical Flow TTC detection at 10 FPS. Fires `CRITICAL_EDGE_HAZARD` if Time-To-Collision < 1.5s — completely local, <16ms latency, no network dependency.
- **Layer 0.5 — Pocket Shield** (`usePocketMode.ts`): `AmbientLightSensor` API detects in-pocket state (<5 lux). Blocks all `touchstart` events to prevent Ghost Touch (accidental mode changes from fabric friction).
- **Layer 1 — Kinematic Frame Gating** (`kinematicGating.ts`): Dot Product tilt check (`cos θ > 0.82`) + angular velocity guard (`<45°/s`). Only captures frames when device is vertical and stable — eliminates Motion Blur from lanyard pendulum effect.
- **Semantic Odometry**: Accumulates `yaw_delta_deg` (gyroscope rotation) per frame interval. Injected into every frame payload so Gemini can infer hazard positions between frames without waiting for visual confirmation.
- Live camera capture with adaptive duty cycling by motion state
- Mic streaming with `echoCancellation` + `noiseSuppression`
- Dual WebSocket channels (`/ws/live` + `/ws/emergency`)
- HRTF spatial audio engine + sonar ping matrix (`100/400/800ms`)
- Wake Lock + OLED black mode (also activates in-pocket) + keepalive fallback
- Gesture mapping: tap (mic), double-tap (repeat), long press (human help), swipe up/down (mode/describe), shake (SOS)
- **Deterministic Scan Worker**: Runs barcode scanning on the edge asynchronously when in `READ` or `EXPLORE` modes, speaking results aloud using `SpeechSynthesisUtterance`.
- Hazard compass + transcript panel + mode indicator + helper live view subsystem

### Backend (FastAPI + Gemini Live)
- Gemini Live API bridge (audio realtime + multimodal frame turns)
- **Semantic Odometry injection** in `live_bridge.py`: when `|yaw_delta| > 5°`, injects `[ODOMETRY: rotated X-deg RIGHT. Hazard may now be DIRECTLY AHEAD.]` context hint before frame — Gemini reasons about hazard position between frames.
- Hazard confirmation pipeline (`HAZARD_CONFIRMATION_FRAMES` config)
- Live tool-calling dispatch to local safety tools
- Tool-style functions:
  - `log_hazard_event(hazard_type, position_x, distance_category, confidence, description, session_id)`
  - `set_navigation_mode(mode)`
  - `log_emotion_event(state, confidence)`
  - `get_context_summary()`
  - `request_human_help()`
- HARD_STOP pipeline over emergency WebSocket channel
- HARD_STOP latency benchmarking (`server_emit_ts_ms`)
- Session store + optional Firestore persistence
- RunConfig builder with ADK-compatible path + local fallback

### Infrastructure
- Terraform for Cloud Run + Firestore + IAM
- GitHub Actions deployment/test workflow

## Repository layout

```
frontend/         PWA app
  src/
    hooks/
      useCamera.ts          Camera pipeline (Dual-Brain wiring)
      usePocketMode.ts      Ghost Touch Shield (Layer 0.5)
      useMotionSensor.ts    Accelerometer / Gyroscope
    services/
      kinematicGating.ts    Dot Product Frame Gating (Layer 1)
      spatialAudioEngine.ts HRTF Audio
    workers/
      survivalReflex.worker.ts  Optical Flow TTC (Layer 0)
backend/          FastAPI + agent orchestration
  agent/
    live_bridge.py          Gemini Live + Semantic Odometry
    aria_agent.py           Multi-agent orchestrator
infra/            Terraform IaC
docs/             Architecture, system prompt, submission checklist
scripts/          Hardening, benchmark, integration tests
```

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

Copy `.env.example` values into `frontend/.env` and `backend/.env`.

For Gemini Live production path, set at least:

- `ENABLE_GEMINI_LIVE=1`
- `GOOGLE_API_KEY=...` (or `GEMINI_API_KEY`)
- `GEMINI_MODEL=gemini-live-2.5-flash-native-audio`
- `APP_ENV=production`
- `WS_AUTH_MODE=oidc`
- `OIDC_ISSUER=<your-oidc-issuer>`
- `OIDC_AUDIENCE=<your-api-audience>`
- `CORS_ALLOW_ORIGINS=https://<your-frontend-domain>`
- `ENABLE_DEV_ENDPOINTS=0`

Security hardening notes:

- Never commit `backend/.env`, `frontend/.env`, or service-account JSON keys.
- Keep `/dev/*` endpoints disabled outside local development.
- Avoid wildcard CORS in production.
- WebSocket auth token is transported via `Sec-WebSocket-Protocol` (`authb64.<token>`), not URL query by default.
- Keep `WS_ALLOW_QUERY_TOKEN=0` in production (empty value auto-disables query token in production).
- Keep realtime payload limits (`MAX_WS_MESSAGE_BYTES`, `MAX_FRAME_B64_CHARS`, `MAX_AUDIO_CHUNK_B64_CHARS`) enabled.
- Use OIDC broker flow (`/auth/oidc/exchange` -> secure cookie -> `/auth/ws-ticket`) so browser does not persist WS token in localStorage.
- Keep `OIDC_BROKER_SIGNING_KEY` configured in production.
- Keep `HUMAN_HELP_SIGNING_KEY` configured in production; helper links are one-time exchange tickets.
- Do not enable helper viewer token in query string; keep subprotocol auth (`authb64.<token>`).

### Key Rotation (Gemini + Firebase)

Automated script:

```bash
scripts/rotate_keys_gcp.sh
```

Optional flags:

```bash
PROJECT_ID=apollos-c7028 \
SERVICE_ACCOUNT_KEY_FILE=$HOME/.config/apollos/firebase-adminsdk.json \
BACKEND_ENV_FILE=/abs/path/backend/.env \
scripts/rotate_keys_gcp.sh
```

Defaults:
- `BACKEND_ENV_FILE=<repo>/backend/.env`
- `SERVICE_ACCOUNT_KEY_FILE` is inferred from `GOOGLE_APPLICATION_CREDENTIALS` in `BACKEND_ENV_FILE` when present, otherwise falls back to `$HOME/.config/apollos/firebase-adminsdk.json`.

The script rotates:

- Firebase service-account key (replaces local JSON file)
- `GEMINI_API_KEY` in `backend/.env`
- Deletes old keys unless `SKIP_DELETE_OLD_KEYS=1`

Optional Vertex path:

- `GEMINI_USE_VERTEX=1`
- `GOOGLE_CLOUD_PROJECT=...`
- `GOOGLE_CLOUD_LOCATION=us-central1`

### OIDC Broker (Production Hardening)

When `WS_AUTH_MODE=oidc`, backend supports a broker flow with short-lived websocket tickets:

1. Client sends external OIDC token to `POST /auth/oidc/exchange` (Bearer token).
2. Backend verifies token and sets `HttpOnly` broker session cookie.
3. Client calls `POST /auth/ws-ticket` to receive short-lived `access_token` for websocket auth.
4. Frontend refreshes ticket in-memory before expiry (no localStorage token persistence).

### Human Fallback Live Link (Production Hardening)

When `HUMAN_FALLBACK_ENABLED=1`, `request_human_help()` and safety-escalation can generate secure helper links:

1. Backend issues short-lived signed `help_ticket` embedded in link (`PUBLIC_HELP_BASE?help_ticket=...`).
2. Helper page exchanges it once via `POST /auth/help-ticket/exchange`.
3. Backend returns short-lived `viewer_token`.
4. If Twilio Video is configured, exchange response also includes short-lived WebRTC room token for helper viewer.
5. Patient app receives `human_help_session` message and publishes A/V to Twilio room (WebRTC).
6. Helper page joins Twilio room for low-latency live audio/video.
7. Fallback path remains available: helper websocket `GET /ws/help/{session_id}` using `Sec-WebSocket-Protocol: apollos.help.v1,authb64.<viewer_token>`.

Optional SMS dispatch:
- Configure `EMERGENCY_CONTACTS` and Twilio credentials to send helper links automatically.

Twilio Video environment:
- `HELP_RTC_PROVIDER=twilio`
- `TWILIO_ACCOUNT_SID=...`
- `TWILIO_VIDEO_API_KEY_SID=...`
- `TWILIO_VIDEO_API_KEY_SECRET=...`
- `TWILIO_VIDEO_ROOM_PREFIX=apollos-help`

## WebSocket contracts

### Client → backend: `multimodal_frame`

```json
{
  "type": "multimodal_frame",
  "session_id": "...",
  "timestamp": "2026-03-05T00:00:00Z",
  "frame_jpeg_base64": "...",
  "motion_state": "walking_fast",
  "pitch": 8.2,
  "velocity": 2.1,
  "yaw_delta_deg": 12.5,
  "location_accuracy_m": 18.0,
  "location_age_ms": 500,
  "sensor_health": {
    "score": 0.81,
    "flags": ["depth_fallback"],
    "degraded": false,
    "source": "edge-fused-v1"
  }
}
```

> `yaw_delta_deg`: accumulated gyroscope yaw rotation (degrees) since last frame capture. Used to inject Semantic Odometry context into Gemini prompts.

### Client → backend: `audio_chunk`

```json
{
  "type": "audio_chunk",
  "session_id": "...",
  "timestamp": "2026-03-05T00:00:00Z",
  "audio_chunk_pcm16": "...base64..."
}
```

### Backend → client: `HARD_STOP`

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

> `HARD_STOP` can be triggered by **either** the Edge Survival Reflex Worker (Layer 0, <16ms) or the Cloud Gemini agent (Layer 2, ~600ms+). Edge fires first.

### Backend → client: `safety_state`

```json
{
  "type": "safety_state",
  "session_id": "...",
  "timestamp": "2026-03-05T00:00:00Z",
  "degraded": true,
  "reason": "low_sensor_health,high_localization_uncertainty",
  "sensor_health_score": 0.42,
  "sensor_health_flags": ["depth_error", "location_missing"],
  "localization_uncertainty_m": 120,
  "tier": "voice"
}
```

### Backend → client: `human_help_session`

```json
{
  "type": "human_help_session",
  "session_id": "...",
  "timestamp": "2026-03-05T00:00:00Z",
  "help_link": "https://.../help?help_ticket=...",
  "rtc": {
    "provider": "twilio",
    "room_name": "apollos-help-...",
    "identity": "patient-...",
    "token": "eyJ...",
    "expires_in": 900
  }
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

Default hardening includes:
- Backend unit tests
- AEC mic constraint audit
- Internal tool latency benchmark
- Internal reconnect/resume simulation

Optional in-process ASGI checks (no live backend required):

```bash
python3 scripts/hardening_pass.py --asgi --budget-ms 100 --iterations 20
```

Run benchmark only:

```bash
python3 scripts/benchmark_hard_stop_asgi.py --iterations 20 --budget-ms 100
```

Internal latency benchmark (no network):

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

## Known limitations

- **iOS Web Audio Quirks:** Background execution on iOS Safari is restricted. The app may suspend audio/mic when screen is locked without specific PWA capabilities.
- `AmbientLightSensor` (Pocket Shield) requires `generic-sensor` permission policy in browser. Falls back gracefully if unavailable.
- `DeviceMotionEvent` requires explicit permission grant on iOS 13+.
- Exact Live API config compatibility can vary by SDK/model version.

## Hardware Requirements & Safety Note

**Mandatory:** For real-world usage, **open-ear or bone-conduction headphones (e.g., Shokz)** are strongly required.
Using standard noise-canceling or in-ear headphones blocks environmental sounds (traffic, pedestrians) — a critical safety hazard for blind and low-vision users. Always preserve natural acoustic awareness.
