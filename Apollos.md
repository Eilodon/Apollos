# Apollos — Technical Blueprint v2.0
**Real-Time AI Navigation Assistant for Blind & Low-Vision Users**

| | |
|---|---|
| **Contest** | Gemini Live Agent Challenge 2026 |
| **Category** | Live Agents |
| **Model** | `gemini-live-2.5-flash-native-audio` |
| **Stack** | React PWA + FastAPI + Gemini Live API + Cloud Run |

---

## Table of Contents

1. [Dual-Brain Architecture](#1-dual-brain-architecture)
2. [Data Flow](#2-data-flow)
3. [Safety Layer Stack](#3-safety-layer-stack)
4. [Frontend Tech Spec](#4-frontend-tech-spec)
5. [Backend Tech Spec](#5-backend-tech-spec)
6. [Agent Design](#6-agent-design)
7. [Performance Targets](#7-performance-targets)
8. [Infrastructure](#8-infrastructure)

---

## 1. Dual-Brain Architecture

The core design principle: **Edge protects lives. Cloud decodes the world.** No single latency budget serves both survival reflexes and semantic understanding — so we use two separate processing paths running in parallel.

```
┌────────────────────────────────────────────────────────────────────┐
│            DUAL-BRAIN ARCHITECTURE (The Eidolon Standard)          │
├───────────────────────────┬────────────────────────────────────────┤
│   SPINAL CORD (Edge)      │   CORTEX (Cloud - Gemini Live)        │
├───────────────────────────┼────────────────────────────────────────┤
│ Layer 0 — Survival Reflex │   Layer 2 — Semantic Cognition         │
│ survivalReflex.worker.ts  │   live_bridge.py → Gemini              │
│ Optical Flow TTC @ 10FPS  │   Scene understanding, OCR             │
│ <16ms local latency       │   Empathic dialogue                    │
│ No network dependency     │   Function calling (HARD_STOP)         │
├───────────────────────────┼────────────────────────────────────────┤
│ Layer 0.5 — Pocket Shield │   Layer 3 — Cloud Fallback             │
│ usePocketMode.ts          │   Emergency WebSocket channel          │
│ AmbientLightSensor API    │   ADK Tool dispatch                    │
│ Ghost Touch prevention    │   HARD_STOP server-side emit           │
├───────────────────────────┼────────────────────────────────────────┤
│ Layer 1 — Kinematic Gate  │                                        │
│ kinematicGating.ts        │                                        │
│ Dot Product tilt check    │                                        │
│ Anti-Motion-Blur filter   │                                        │
└───────────────────────────┴────────────────────────────────────────┘
```

### Layer Responsibilities

| Layer | Where | Latency | Mechanism | Triggers |
|---|---|---|---|---|
| **0 — Survival Reflex** | Browser Worker | **<16ms** | Optical Flow TTC | TTC < 1.5s → `CRITICAL_EDGE_HAZARD` |
| **0.5 — Pocket Shield** | Browser main | Realtime | `AmbientLightSensor` | Light < 5 lux → block all `touchstart` |
| **1 — Kinematic Gate** | Browser main | Per-frame | Dot Product `cos(θ)` | Only pass frame if `cos(θ) > 0.82` AND `|ω| < 45°/s` |
| **2 — Cognitive** | Cloud Run | ~600ms+ | Gemini Live 2.5 | Every N-th stable frame |
| **3 — Cloud Fallback** | Cloud Run | ~100ms | ADK tool call | `log_hazard_event()` → WebSocket HARD_STOP |

---

## 2. Data Flow

### Frame Pipeline (Cloud Path — Layer 1 → 2)

```
DeviceMotion event (60Hz)
        │
        ▼
kinematicGating.ts
  cos(tilt) = |accel.y| / |accel|
  isVertical  = cos(tilt) > 0.82   ← phone upright within 35°
  isStable    = |gyro| < 45°/s     ← no spin/rotation
  yaw_delta   += gyro.alpha * dt   ← accumulate rotation since last frame
        │
        │ [gate passes OR timeout]
        ▼
useCamera.ts — capture 768×768 JPEG @ EDGE_INTERVAL=100ms
  CLOUD post only when: (isVertical && isStable) || timeout
        │
        ▼
App.tsx → aria.sendFrame({
  frame_jpeg_base64,
  motion_state,
  pitch,
  velocity,
  yaw_delta_deg   ← Semantic Odometry
})
        │
        ▼                        ┌─────────────────────────────────┐
live_bridge.py                   │ KINEMATIC context injected:     │
  Builds motion_text:            │ "[KINEMATIC: walking_fast.      │
  + ODOMETRY hint if             │  Pitch: 8.2deg. Velocity: 2.1]  │
    |yaw_delta| > 5°             │ [ODOMETRY: rotated 45-deg RIGHT.│
        │                        │  Hazard RIGHT may be now AHEAD]"│
        ▼                        └─────────────────────────────────┘
Gemini Live API
  (image/jpeg inline_data + context text)
        │
   [hazard detected]
        ▼
log_hazard_event(position_x, distance_category, hazard_type, confidence)
        │
        ▼
Emergency WebSocket → {type: HARD_STOP, position_x, distance, hazard_type}
        │
        ▼
App.tsx → onHardStop() → SpatialAudioEngine.fireHardStop(position_x, distance)
```

### Survival Reflex Pipeline (Edge Path — Layer 0)

```
useCamera.ts — capture 64×64 ImageData @ 100ms (10 FPS)
        │
        ▼              ┌────────────────────────────────────┐
survivalReflex          │ Optical Flow: compare central     │
.worker.ts              │ pixel brightness vs prev frame.   │
  computeOptical        │ avgDiff > 50 → TTC ≈ 1.2s        │
  Expansion()           │ (object expanding rapidly → close)│
        │               └────────────────────────────────────┘
        │ [TTC < 1.5s]
        ▼
postMessage({type: CRITICAL_EDGE_HAZARD, positionX: 0, distance: 'very_close'})
        │
        ▼
useCamera.ts → onHazard?.({type: 'HARD_STOP', ...})
        │
        ▼
App.tsx → onHardStop() → [Sonar Ping + Haptics + Hazard Compass]
                          INSTANT — no network required
```

### Pocket Shield Flow (Layer 0.5)

```
AmbientLightSensor → reading event
  illuminance < 5 lux → inPocket = true
        │
        ├─→ OLEDBlackOverlay activates (screen black = no Ghost Touch feedback)
        └─→ document.addEventListener('touchstart', e => e.preventDefault())
                     ← blocks ALL touch: accidental mode change impossible
```

---

## 3. Safety Layer Stack

### Four-Layer Safety Architecture

| Layer | Mechanism | Response Time | Failure Mode if Absent |
|---|---|---|---|
| **Layer 0 — Optical Flow Edge** | `survivalReflex.worker.ts` Optical Flow TTC < 1.5s | **<16ms** | No pre-network hazard detection |
| **Layer 1 — Prompt Grounding** | System prompt: `log_hazard_event()` BEFORE speech. 2-frame confirmation. Distance qualifiers only | — | Hallucinated distances, premature movement commands |
| **Layer 2 — Tool-Triggered HARD_STOP** | `log_hazard_event()` ADK tool call → emergency WebSocket → local HRTF siren | **<100ms target** | Relying on slow spoken warning only |
| **Layer 3 — Fallback Cache** | `AudioCache` stores last 3 valid responses. Network drop → plays last safe instruction + 3-pulse haptic | instant local | Silence on disconnect = no guidance |

### `HARD_STOP` Dual-Trigger Architecture

`HARD_STOP` can now be fired by TWO independent sources:

```
Source A (Cloud — Layer 2/3):          Source B (Edge — Layer 0):
Gemini detects semantic hazard          Optical Flow detects TTC < 1.5s
        │                                       │
        ▼                                       ▼
log_hazard_event() ADK call             CRITICAL_EDGE_HAZARD worker msg
        │                                       │
        └──────────────┬────────────────────────┘
                       ▼
              App.tsx → onHardStop()
                       │
           ┌───────────┼───────────────┐
           ▼           ▼               ▼
    HRTF Sonar Ping  Haptic pulse  HazardCompass UI
    (position_x)     (vibrateHardStop)  (glowing dot)
```

### Kinematic Safety Boost

If `motion_state = "walking_fast"` AND hazard visible → skip 2-frame confirmation, call `log_hazard_event()` immediately. Injected into system prompt context text per frame.

### Semantic Odometry Safety Rule

When `|yaw_delta_deg| > 5°` between frames, `live_bridge.py` injects:
```
[ODOMETRY: User rotated 45-deg RIGHT. If hazard was RIGHT, warn it may be DIRECTLY AHEAD. Do not wait for next frame.]
```

---

## 4. Frontend Tech Spec

### File Structure

```
frontend/src/
├── hooks/
│   ├── useCamera.ts          Camera pipeline — Dual-Brain wiring hub
│   ├── usePocketMode.ts      Ghost Touch Shield (Layer 0.5)
│   ├── useMotionSensor.ts    Accelerometer/Gyroscope state machine
│   ├── useARIA.ts            WebSocket BIDI manager + reconnect
│   ├── useAudioStream.ts     16kHz PCM mic capture
│   └── useWakeLock.ts        Wake Lock + OLED Black mode
├── services/
│   ├── kinematicGating.ts    Dot Product frame gating (Layer 1)
│   ├── spatialAudioEngine.ts HRTF PannerNode engine
│   ├── audioCache.ts         Layer 3 fallback cache (last 3 responses)
│   └── haptics.ts            Vibration patterns
├── workers/
│   └── survivalReflex.worker.ts  Optical Flow TTC (Layer 0)
├── components/
│   ├── CameraView.tsx
│   ├── HazardCompass.tsx     Semicircular hazard direction UI
│   ├── ModeIndicator.tsx
│   ├── TranscriptPanel.tsx
│   └── OLEDBlackOverlay.tsx
└── types/
    └── contracts.ts          WebSocket message interfaces
```

### Key Hooks & Services

#### `useCamera.ts` — Camera Pipeline Hub

Orchestrates all three Edge layers in a single hook:

```typescript
// Runs at 100ms interval (10 FPS for Edge, throttled for Cloud)
// Layer 0: sends 64×64 ImageData to Worker every 100ms
// Layer 1: captures 768×768 JPEG only when kinematically stable
// Odometry: accumulates yaw_delta_deg, resets after each Cloud post

const EDGE_INTERVAL = 100;  // 10 FPS for Worker
// Cloud post rate: intervalForMotionState(state)
//   stationary → 5000ms (0.2 FPS)
//   walking    → 1000ms (1 FPS)
//   running    →  500ms (2 FPS)
```

#### `kinematicGating.ts` — Frame Quality Gate

```typescript
// Dot Product tilt check:
const cosTilt = Math.abs(accel.y) / magnitude;
const isVertical = cosTilt > 0.82;  // < 35° from vertical

// Angular velocity check:
const isStable = |alpha| < 45 && |beta| < 45 && |gamma| < 45;  // deg/s

// Graceful degradation: no sensor → returns true (captures normally)
export function shouldCaptureFrame(reading: KinematicReading): boolean

// Yaw accumulator for Semantic Odometry:
export function computeYawDelta(gyro, dtMs): number  // degrees
```

#### `usePocketMode.ts` — Ghost Touch Shield

```typescript
// AmbientLightSensor at 5Hz
// < 5 lux → inPocket = true → OLEDBlackOverlay + touchstart block
// Fallback: graceful if sensor unavailable (Chrome permission policy)
export function usePocketMode(): boolean
```

#### `survivalReflex.worker.ts` — Optical Flow TTC

```typescript
// Runs in isolated Worker thread — never blocks UI
// Compares 64×64 frames: pixel brightness diff sampling
// avgDiff > 50 → TTC ≈ 1.2s (object rapidly expanding = approaching)
// postMessage({type: 'CRITICAL_EDGE_HAZARD', positionX: 0, distance: 'very_close'})
```

#### `SpatialAudioEngine` — HRTF Sonar

```typescript
// Web Audio API PannerNode (panningModel: 'HRTF')
// position_x: -1.0 (left) → 0 (center) → 1.0 (right) mapped to X-axis × 3
// Distance conveyed via Sonar Ping rhythm, not volume:
const PING_INTERVALS = { very_close: 100, mid: 400, far: 800 };  // ms
// Auto-stop after 3s (ARIA speech takes over)
```

### WebSocket Contracts

#### Client → Backend: `multimodal_frame`

```json
{
  "type": "multimodal_frame",
  "session_id": "...",
  "timestamp": "2026-03-05T00:00:00Z",
  "frame_jpeg_base64": "...",
  "motion_state": "walking_fast",
  "pitch": 8.2,
  "velocity": 2.1,
  "yaw_delta_deg": 12.5
}
```

> `yaw_delta_deg`: accumulated gyroscope yaw rotation (degrees) since last Cloud frame. Triggers Semantic Odometry hint in backend.

#### Client → Backend: `audio_chunk`

```json
{
  "type": "audio_chunk",
  "session_id": "...",
  "timestamp": "...",
  "audio_chunk_pcm16": "...base64 PCM 16kHz..."
}
```

#### Backend → Client: `HARD_STOP`

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

> Can be emitted by **Edge Worker** (<16ms, no network) **or** Cloud ADK tool call (<100ms target).

#### Backend → Client: `audio_chunk` (with position)

```json
{
  "type": "audio_chunk",
  "pcm16": "...base64 PCM 24kHz...",
  "hazard_position_x": 0.8
}
```

### Gesture System

| Gesture | Action |
|---|---|
| One tap | Toggle mic |
| Double tap | Repeat last response (from `AudioCache`) |
| Long press (650ms) | `request_human_help()` |
| Swipe up | Cycle navigation mode |
| Swipe down | `describe_detailed` command |
| Shake | SOS → `request_human_help()` |

### Audio Constraints (AEC)

```typescript
// useAudioStream.ts
audio: {
  echoCancellation: true,   // CRITICAL — prevents VAD death loop
  noiseSuppression: true,   // Wind noise reduction
  autoGainControl: true,
  sampleRate: 16000,
  channelCount: 1,
}
```

---

## 5. Backend Tech Spec

### File Structure

```
backend/
├── agent/
│   ├── aria_agent.py         Multi-agent orchestrator
│   ├── live_bridge.py        Gemini Live session + Semantic Odometry
│   ├── prompts.py            System prompt (ARIA persona)
│   ├── run_config.py         ADK RunConfig builder
│   ├── session_manager.py    Session store + Firestore bridge
│   ├── websocket_handler.py  WebSocket registry
│   └── tools/
│       ├── hazard_logger.py  log_hazard_event → HARD_STOP emit
│       ├── context_manager.py
│       ├── emotion_logger.py
│       ├── human_help.py
│       └── mode_switcher.py
└── main.py                   FastAPI app + WebSocket endpoints
```

### `live_bridge.py` — Semantic Odometry Injection

```python
# send_multimodal_frame():
yaw_delta = float(payload.get('yaw_delta_deg', 0.0) or 0.0)

odometry_hint = ''
if abs(yaw_delta) > 5.0:
    direction = 'RIGHT' if yaw_delta > 0 else 'LEFT'
    odometry_hint = (
        f' [ODOMETRY: User rotated {abs(yaw_delta):.0f}-deg {direction}.'
        f' If hazard was {direction.lower()}, warn it may now be DIRECTLY AHEAD.]'
    )

motion_text = (
    f"[KINEMATIC: User is {motion_state}. Pitch: {pitch:.1f}deg. "
    f"Velocity: {velocity:.2f}. Treat visible hazards with safety-first urgency.]{odometry_hint}"
)
```

### Hazard Confirmation Pipeline

```python
# Configurable via env vars:
HAZARD_CONFIRMATION_FRAMES = int(os.getenv('HAZARD_CONFIRMATION_FRAMES', '1'))
HAZARD_CONFIRMATION_TIMEOUT_S = float(os.getenv('HAZARD_CONFIRMATION_TIMEOUT_S', '3.0'))

# Bypass confirmation if: motion_state in {'walking_fast', 'running'}
# Default: 1 frame (immediate) — tunable for false positive reduction
```

### ADK Tool Catalog

| Tool | Signature | Effect |
|---|---|---|
| `log_hazard_event` | `(hazard_type, position_x, distance_category, confidence, description, session_id)` | Emit `HARD_STOP` over emergency WebSocket + Firestore log |
| `set_navigation_mode` | `(mode: NAVIGATION\|EXPLORE\|READ\|QUIET)` | Update session mode |
| `log_emotion_event` | `(state, confidence)` | Firestore analytics |
| `get_context_summary` | `()` | Return session context for reconnect |
| `request_human_help` | `()` | Generate shareable live link |

### Session Management (Gemini Live)

```python
# live_bridge.py — config hierarchy tried in order:
# 1. "full": session_resumption + context_window_compression
# 2. "reduced": base config only
# 3. "minimal": response_modalities + tools + system_instruction only

config = {
    'response_modalities': ['AUDIO'],
    'input_audio_transcription': {},
    'output_audio_transcription': {},
    'speech_config': {
        'voice_config': {'prebuilt_voice_config': {'voice_name': 'Kore'}},
    },
    'enable_affective_dialog': True,
    'proactivity': {'proactive_audio': True},
    'realtime_input_config': {
        'automatic_activity_detection': {'disabled': False},
    },
    'tools': [{'function_declarations': [...]}],
    'system_instruction': SYSTEM_PROMPT,
    # Full config adds:
    'session_resumption': {'transparent': True},
    'context_window_compression': {
        'trigger_tokens': 100000,
        'sliding_window': {'target_tokens': 80000},
    },
}
```

---

## 6. Agent Design

### Navigation Modes

| Mode | Behavior |
|---|---|
| **NAVIGATION** (default) | Proactive hazard alerts. HARD_STOP for critical danger. |
| **EXPLORE** | Rich descriptions on request. Less proactive speech. |
| **READ** | OCR-focused: reads signs, menus verbatim. |
| **QUIET** | Speaks only for imminent hazards (<2m). |

### System Prompt Core Rules (ARIA)

```
=== SAFETY RULES (ABSOLUTE) ===
1. NEVER say "go", "walk", "move" unless HIGH confidence path is clear in CURRENT frame.
2. If hazard detected → CALL log_hazard_event() IMMEDIATELY, BEFORE speaking.
3. Poor frame quality (dark, blurry) → "I can't see clearly. Please stop."
4. Ambiguous hazard = assume danger. False positive = safe. False negative = dangerous.

=== KINEMATIC CONTEXT (injected per frame) ===
[KINEMATIC: User is {motion_state}. Pitch: {pitch}deg. Velocity: {velocity}.]
[ODOMETRY: rotated {N}-deg {DIR}. If hazard was {DIR}, warn it may be DIRECTLY AHEAD.]

=== RESPONSE FORMAT ===
Hazard alerts: <8 words. Tool fires first.
Descriptions: 1-3 sentences. Most important info first.
Directions: clock position + distance. "3 o'clock, nearby (1-2m)"
```

### Voice & Persona

| Attribute | Value |
|---|---|
| **Voice** | Gemini `"Kore"` — warm, gender-neutral |
| **Affective Dialog** | `enable_affective_dialog=True` — detects vocal stress, adapts tone |
| **Proactivity** | `proactive_audio=True` — speaks without prompt when scene changes |

---

## 7. Performance Targets

| Parameter | Target | Mechanism |
|---|---|---|
| Edge hazard detection | **<16ms** | `survivalReflex.worker.ts` — no network |
| Cloud HARD_STOP (tool → siren) | **<100ms** | Emergency WebSocket channel |
| TTFT (frame → first audio token) | <600ms on 4G | Gemini Live 2.5 Flash native audio |
| End-to-end (frame → audio) | <1.5s | — |
| Camera @ walking | 1 FPS | `intervalMs = 1000` |
| Camera @ stationary | 0.2 FPS | `intervalMs = 5000` (80% energy saved) |
| Camera @ running | 2 FPS | `intervalMs = 500` |
| Kinematic gate resolution | 100ms | `EDGE_INTERVAL` constant |
| Audio input | 16kHz mono PCM | 50–100ms chunks via AudioWorklet |
| Audio output | 24kHz PCM | Streamed → `SpatialAudioEngine` |
| Bandwidth upstream | ~20 KB/s | Video (JPEG 768×768 @72%) + audio + motion |
| Bandwidth downstream | ~30 KB/s | 24kHz PCM audio stream |
| Session video limit | 2 min → transparent | `session_resumption: {transparent: true}` |
| Context window | 100k tokens trigger | Sliding window compress → 80k target |
| Firestore writes | ≤1/30s | Rate-limited in session manager |
| HARD_STOP latency benchmark | `<100ms` gate | `server_emit_ts_ms` field + benchmark scripts |

---

## 8. Infrastructure

### Cloud Architecture

```
GitHub Actions (CI/CD) → Cloud Run (FastAPI backend)
                                    ↕ WebSocket BIDI
                         Client PWA (React)
                                    ↕
                         Gemini Live API
                                    ↕
                         Firestore (sessions + logs)
```

### Environment Variables

| Variable | Default | Purpose |
|---|---|---|
| `ENABLE_GEMINI_LIVE` | `1` | Toggle Live API path |
| `GOOGLE_API_KEY` / `GEMINI_API_KEY` | — | Auth (non-Vertex) |
| `GEMINI_MODEL` | `gemini-live-2.5-flash-native-audio` | Primary model |
| `GEMINI_MODEL_FALLBACKS` | `gemini-2.5-flash-native-audio-preview-12-2025,...` | Fallback chain |
| `GEMINI_USE_VERTEX` | `0` | Use Vertex AI path |
| `GOOGLE_CLOUD_PROJECT` | — | Required if Vertex |
| `GOOGLE_CLOUD_LOCATION` | `us-central1` | — |
| `HAZARD_CONFIRMATION_FRAMES` | `1` | Frames before HARD_STOP fires |
| `HAZARD_CONFIRMATION_TIMEOUT_S` | `3.0` | Reset window (s) |

### Build & Test

```bash
# Frontend production build
cd frontend && npm run build
# Builds 47 modules, Worker bundle included: survivalReflex.worker-*.js

# Backend unit tests
PYTHONPATH=backend python -m unittest discover backend/tests

# Hardening pass (unit + AEC audit + latency + reconnect)
python3 scripts/hardening_pass.py

# ASGI in-process benchmark (no live server needed)
python3 scripts/benchmark_hard_stop_asgi.py --iterations 20 --budget-ms 100

# Internal latency benchmark
PYTHONPATH=backend python3 scripts/benchmark_hard_stop_internal.py --iterations 100 --budget-ms 100
```

### Terraform (Cloud Run + Firestore)

```hcl
# infra/cloud_run.tf
resource "google_cloud_run_v2_service" "aria_backend" {
  name     = "apollos-aria-backend"
  location = var.region
  template {
    containers {
      image = "gcr.io/${var.project_id}/aria-backend:latest"
      resources { limits = { cpu = "2", memory = "2Gi" } }
    }
  }
}
```

---

*Blueprint v2.0 — Updated March 2026*
*Reflects codebase: Dual-Brain Architecture (Eidolon Standard), Layer 0 Optical Flow, Layer 0.5 Pocket Shield, Layer 1 Kinematic Gating, Semantic Odometry, multi-agent backend.*
