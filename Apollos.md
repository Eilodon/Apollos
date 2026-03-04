# Apollos — Technical Blueprint v1.3
**Real-Time AI Navigation Assistant for the Blind**

> **Changelog v1.3:** Added Adaptive Duty Cycling (battery survival), fixed AEC Echo Death Loop (`echoCancellation: true` + bone-conduction hardware recommendation), upgraded HRTF from X-axis-only to full 3D Sonar Ping Matrix (distance-based ping rhythm), updated demo script with adaptive framerate overlay + bone-conduction headphone shot.
>
> **Changelog v1.2:** Fixed Layer 2 (Text Interceptor → Tool-Triggered Hardware Interrupt), fixed PWA screen lock (Wake Lock + OLED Black), added HRTF Spatial Audio engine, added Kinematic Sensor Fusion.
>
> **Changelog v1.1:** Fixed session limits (2-min video), corrected ADK RunConfig API, added Emotion-Aware Mode.

| | |
|---|---|
| **Category** | Live Agents |
| **Contest** | Gemini Live Agent Challenge 2026 |
| **Target Prize** | Grand Prize — $25,000 |
| **Submission Deadline** | March 16, 2026 |

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Competitive Landscape & Gap Analysis](#2-competitive-landscape--gap-analysis)
3. [System Architecture](#3-system-architecture)
4. [Technology Stack & Decision Records](#4-technology-stack--decision-records)
5. [Agent Design & Persona](#5-agent-design--persona)
6. [Safety Architecture](#6-safety-architecture)
7. [UX Design Specification](#7-ux-design-specification)
8. [Implementation Plan](#8-implementation-plan)
9. [Key Technical Specifications](#9-key-technical-specifications)
10. [Contest Scoring Strategy](#10-contest-scoring-strategy)
11. [Risk Register](#11-risk-register)
12. [Post-Hackathon Product Roadmap](#12-post-hackathon-product-roadmap)

---

## 1. Executive Summary

VisionGPT is a real-time, voice-first AI navigation agent built specifically for blind and visually impaired users. Powered by the Gemini Live API and deployed on Google Cloud, it transforms a smartphone camera into an always-on pair of intelligent eyes — describing environments, detecting hazards, reading text, recognizing faces, and guiding navigation through natural conversational speech.

Unlike existing tools (BeMyEyes, Seeing AI, Envision) that operate in a tap-and-wait paradigm, VisionGPT delivers:
- **Continuous live session** with full barge-in support
- **Proactive hazard detection** via Tool-Triggered Hardware Interrupts — faster than speech
- **HRTF Spatial Audio** — warning sounds emerge physically from the direction of danger
- **Emotion-aware tone adjustment** via Gemini's native affective dialog
- **Kinematic sensor fusion** — AI knows if user is walking fast toward a hazard
- **Persistent context** across session boundaries via ADK session resumption

> **Core Value Proposition:** 1 smartphone → replaces white cane intelligence + sighted guide + text reader + face recognizer + navigation assistant. Available 24/7, works hands-free, zero additional hardware.

---

## 2. Competitive Landscape & Gap Analysis

### 2.1 Existing Solutions

| Product | Interaction Model | Critical Gap |
|---|---|---|
| BeMyEyes + GPT-4o | Tap photo → wait → read description | Turn-based, no live session, high latency |
| Microsoft Seeing AI | Camera modes, static analysis | No conversational memory, no proactive alerts |
| Envision Glasses/App | Wearable-focused, scene description | $2,500+ hardware barrier, no barge-in |
| NaviGPT (research) | Mobile navigation, brief descriptions | Prototype only, no voice interaction |
| Aira (human agents) | Live video to human interpreter | $30+/month, human-dependent |
| OKO AI | Traffic light detection only | Single-purpose, no general assistance |

### 2.2 The Opportunity Gap

No current solution combines:
- Continuous live video streaming (not tap-per-photo)
- Barge-in voice interaction (interrupt agent mid-sentence)
- **Hardware-interrupt hazard alerts** (faster than speech — local siren fires before LLM speaks)
- **HRTF Spatial Audio** — warning voice literally comes from the direction of the obstacle
- **Emotion-aware proactive care** — detects stress, adapts tone
- **Kinematic awareness** — AI knows user's movement speed from accelerometer
- Conversational memory across session boundaries
- Zero additional hardware required

> **Strategic Insight:** VisionGPT is the first live navigation agent where the *physics of sound itself* guides the user. When a warning voice comes from the right, the user turns left instinctively — before their brain even processes the words. No competitor is doing this.

---

## 3. System Architecture

### 3.1 High-Level Architecture

```
+----------------------+   WebSocket (BIDI)   +----------------------+   Live API   +---------------------------+
|   Mobile PWA         | <==================> |   ADK Backend        | <==========> |   Gemini Live API         |
|   (Client)           |                      |   (Cloud Run)        |              |   (AI Engine)             |
+----------------------+                      +----------------------+              +---------------------------+
| - Camera 1 FPS       |                      | - RunConfig mgr      |              | - gemini-live-2.5-flash   |
| - Mic 16kHz PCM      |                      | - Tool event router  |              |   -native-audio           |
| - SpatialAudioEngine |                      | - Session resumption |              | - Vision + Audio native   |
| - Wake Lock (OLED)   |                      | - Context compress   |              | - VAD + barge-in          |
| - Gesture system     |                      | - Firestore bridge   |              | - Affective dialog        |
| - DeviceMotion data  |                      | - Function calling   |              | - Proactivity enabled     |
| - Local siren cache  |                      |                      |              |                           |
+----------------------+                      +----------------------+              +---------------------------+
        ^                                              |
        | {"type":"HARD_STOP", "position_x": 0.8}     v
        +----------------------------------------------+
                   Emergency WebSocket channel          |
                                                        v
                                               +------------------+
                                               |   Firestore      |
                                               |  (State & Logs)  |
                                               +------------------+
```

### 3.2 Data Flow *(v1.2: sensor fusion + spatial audio pipeline added)*

| Step | Description |
|---|---|
| 1. Camera Capture | PWA captures frame at 1 FPS via `getUserMedia` → resize to 768×768 JPEG → base64 encode |
| 2. Sensor Fusion | `DeviceMotionEvent` sampled at 10Hz → compute `velocity_vector` + `pitch` → attach as JSON metadata to each frame |
| 3. Audio Capture | Continuous mic stream at 16kHz mono PCM, 50–100ms chunks via AudioWorklet |
| 4. WebSocket Upload | Frame + motion metadata + audio chunks multiplexed over single BIDI WebSocket |
| 5. Gemini Processing | Live API processes frame + audio + injected motion context in unified multimodal model |
| 6. Function Call (hazard) | Gemini calls `log_hazard_event(position_x, type, confidence)` — backend immediately emits `{"type":"HARD_STOP","position_x":0.8}` to PWA |
| 7. Hardware Interrupt | PWA receives HARD_STOP → **instantly** cuts AudioContext + plays pre-cached local siren positioned at `position_x` via HRTF PannerNode |
| 8. Audio Stream | Gemini continues streaming PCM audio chunks (24kHz) → SpatialAudioEngine pans based on last known hazard position |
| 9. Context Persist | Environmental context + emotion state written to Firestore |

### 3.3 Session Management

> **CRITICAL CONSTRAINTS (Vertex AI Live API, confirmed):**
> - Audio + video sessions: **2-minute limit** without compression
> - Connection lifetime: **~10 minutes**
> - Audio-only: **15-minute limit**
>
> All three solved by ADK native `SessionResumptionConfig` + `ContextWindowCompressionConfig`.
> ADK manages the `live_session_resumption_handle` internally — zero custom code.

```python
run_config = RunConfig(
    streaming_mode=StreamingMode.BIDI,
    session_resumption=types.SessionResumptionConfig(transparent=True),
    context_window_compression=types.ContextWindowCompressionConfig(
        trigger_tokens=100000,
        sliding_window=types.SlidingWindow(target_tokens=80000)
    ),
    # ... (see Section 9.2 for full config)
)
```

---

## 4. Technology Stack & Decision Records

### 4.1 Core Technology Decisions

| Component | Decision | Rationale |
|---|---|---|
| AI Engine | `gemini-live-2.5-flash-native-audio` | Native audio = no STT/TTS pipeline, lowest latency. Confirmed Live API model — Gemini 3.x not yet on Live API |
| Frontend | Progressive Web App (React + TypeScript) | No app store review; instant URL for judges; Wake Lock API supported on all modern browsers |
| Backend | ADK + FastAPI on Cloud Run | Contest requires GCP; ADK handles session lifecycle; function calling routes hardware interrupts |
| Safety System | Tool-Triggered Hardware Interrupt | **Replaces text interceptor** — see Section 6 for rationale |
| Audio Engine | Web Audio API + HRTF PannerNode | Spatial 3D audio positioning; HRTF model simulates human hearing physics |
| Sensor Input | HTML5 DeviceMotionEvent | Zero-cost kinematic context; no extra hardware |
| Session Management | ADK `SessionResumptionConfig(transparent=True)` | Solves 2-min video limit transparently |
| Database | Firestore | Real-time; ADK native integration |
| Auth | Ephemeral Tokens (Vertex AI) | API keys never on client |
| IaC | Terraform + GitHub Actions | Contest bonus +0.2 |

### 4.2 Why PWA — and How We Solve the Screen Lock Problem

| Factor | Detail |
|---|---|
| Speed to demo | No app store review. Judge opens URL immediately |
| Cross-platform | Android Chrome 116+ + Safari iOS 16+ |
| Screen lock mitigation | `navigator.wakeLock.request('screen')` + OLED Black UI — see Section 7 |
| Known limitation | Background processing stops on some browsers if Wake Lock fails — documented in onboarding |
| Production path | Phase 2: Firebase AI Logic SDK native Android/iOS for background support + sub-800ms latency |

> **Why not native app for hackathon?** App store review takes 1–3 weeks. PWA + Wake Lock solves the screen-lock problem sufficiently for demo and judging.

### 4.3 Model Selection

| Model | Role |
|---|---|
| `gemini-live-2.5-flash-native-audio` | Primary: vision + voice + barge-in + affective dialog + proactivity + function calling |
| `gemini-2.5-flash` (standard, non-live) | Offline summarizer only — called once at session boundary if needed |
| Not used: Gemini 3.x | Does not support Live API native audio as of March 2026 |
| Not used: YOLO / TFLite | Gemini vision sufficient for walking pace; on-device ML planned for Phase 2 |

---

## 5. Agent Design & Persona

### 5.1 Agent Identity

| Attribute | Value |
|---|---|
| **Name** | ARIA (Adaptive Real-time Intelligence Assistant) |
| **Voice** | Gemini `"Kore"` — warm, clear, gender-neutral |
| **Tone** | Calm and direct. Panic only when genuine danger. Never verbose |
| **Emotional Intelligence** | `enable_affective_dialog=True` — detects vocal stress + facial cues, adapts automatically |
| **Spatial Awareness** | Voice physically comes from the direction of hazard via HRTF |
| **Kinematic Awareness** | Knows if user is walking fast toward an obstacle |

### 5.2 Interaction Modes

| Mode | Behavior | Activation |
|---|---|---|
| **NAVIGATION** (default) | Proactive hazard alerts. Hardware interrupt for critical danger. Confirms before movement instructions | "Let's walk" / "Guide me" |
| **EXPLORE** | Rich description on request. Less proactive speech | "Describe my surroundings" |
| **READ** | OCR-focused: reads menus, signs, labels verbatim | "Read this" / "What does it say?" |
| **QUIET** | Only speaks for imminent hazards (within ~2m) | "Be quiet" / "Quiet mode" |
| **EMOTION-AWARE** | Automatic — detects stress signals (voice tremor, facial tension) → slower speech + rest offer | Auto-triggered by Gemini affective dialog |

**Emotion-Aware — Technical Detail:**

`enable_affective_dialog=True` gives Gemini native access to:
- Acoustic cues: pitch elevation, speech rate, tremor
- Visual micro-expressions in 1FPS frames: brow tension, jaw clenching

On detection → ARIA slows pace, shortens sentences, offers pause: *"You seem tense. Let's slow down. You're safe."*  
`log_emotion_event()` captures state to Firestore for analytics. Zero extra API cost.

### 5.3 System Prompt — ARIA v1.2

```
You are ARIA, a real-time navigation assistant for a blind user.
Your primary mission: SAFE → NAVIGATE → DESCRIBE.

=== SAFETY RULES (ABSOLUTE — NEVER VIOLATE) ===
1. NEVER say "go", "walk", "step", "move" unless HIGH confidence path is clear
   in the CURRENT frame.
2. Before ANY movement instruction: "Path looks clear. Say yes to proceed."
3. If you detect: steps, stairs, drops, vehicles, water, construction,
   fast-moving objects → CALL log_hazard_event() IMMEDIATELY, BEFORE speaking.
   The tool fires a hardware interrupt on the device — faster than your voice.
4. NEVER guess distances. Use: "very close (within 1m)", "nearby (1-3m)",
   "ahead (3-5m)", "in the distance (5m+)".
5. If frame quality is poor (dark, blurry) → "I can't see clearly. Please stop."
   NEVER fabricate scene description.
6. Ambiguous hazard = assume danger. False positive = safe. False negative = dangerous.

=== KINEMATIC CONTEXT ===
7. Each frame includes motion_state metadata. If motion_state = "walking_fast"
   AND hazard visible → URGENCY level maximum. Call log_hazard_event() without delay.
8. If motion_state = "stationary" → can give richer descriptive responses.

=== EMOTION-AWARE RULES ===
9. Detect stress signals (fast speech, tremor, facial tension) →
   slow pace, shorter sentences, offer pause.
   "I notice you may be stressed. Let's slow down. You're safe."
10. Adjust warmth based on environment: calmer in high-traffic, warmer indoors.

=== RESPONSE FORMAT ===
- Hazard alerts: < 8 words. "Stop. Step down, very close." (tool fires first)
- Descriptions: 1-3 sentences. Most important info first.
- Reading text: Verbatim. "Sign reads: EXIT — Floor 2"
- Directions: clock position + distance. "3 o'clock, nearby (1-2m)"

=== CURRENT MODE ===
{MODE}

=== SESSION CONTEXT ===
{CONTEXT_SUMMARY}

=== PROACTIVE RULES ===
- NAVIGATION: Speak when scene changes or hazard detected.
- EXPLORE: Speak only when asked, unless critical hazard.
- READ: Speak when new text visible.
- QUIET: Critical danger only.
- Silence is correct behavior when nothing important changed.
```

### 5.4 Function Calling Tools

| Tool | Signature | Purpose | Trigger |
|---|---|---|---|
| `log_hazard_event` | `(type, position_x, confidence, description)` | Fires `HARD_STOP` interrupt to PWA **before** ARIA speaks. Logs to Firestore | Any hazard detection |
| `set_navigation_mode` | `(mode)` | Switch NAVIGATION / EXPLORE / READ / QUIET | Voice command |
| `log_emotion_event` | `(state, confidence)` | Log stress/calm state to Firestore | Stress detected |
| `get_context_summary` | `()` | Retrieve env context from Firestore | Session start / reconnect |
| `request_human_help` | `()` | Generate shareable live camera link | "Call for help" |

> **Critical design note:** `log_hazard_event` is always called **FIRST** before ARIA speaks. The function call emits a hardware interrupt on the PWA in milliseconds — far faster than the LLM can vocalize a warning. ARIA's spoken alert is then a human-readable confirmation of what the device already signaled.

---

## 6. Safety Architecture

### 6.1 Why the Old Text Interceptor Was Wrong *(v1.2 fix)*

**v1.1 described:** A backend interceptor that buffers ARIA's text transcript, scans for dangerous phrases, and blocks the audio if found.

**Why this fails:** `gemini-live-2.5-flash-native-audio` streams 24kHz PCM audio chunks **in parallel** with the transcript. To block audio based on text content, the backend must buffer the entire spoken sentence — destroying real-time latency entirely. The system becomes walkie-talkie-grade (press-and-release), not conversational.

**v1.2 solution:** Remove text buffering entirely. Replace with **Tool-Triggered Hardware Interrupt**.

### 6.2 Four-Layer Safety Stack *(v1.2: Layer 2 redesigned)*

| Layer | Mechanism |
|---|---|
| **Layer 0 — Client Fallback Cache** | PWA stores last 3 valid audio responses. On network drop → plays last safe cached instruction + haptic 3-pulse (stop signal). No silence, no crash |
| **Layer 1 — Prompt Grounding** | System prompt enforces: never give directional instruction without frame confirmation, uncertainty always verbalized, `log_hazard_event()` called before speech |
| **Layer 2 — Tool-Triggered Hardware Interrupt** *(redesigned)* | Gemini calls `log_hazard_event(position_x, ...)` → ADK backend emits `{"type":"HARD_STOP","position_x":0.8}` over emergency WebSocket channel → PWA instantly cuts AudioContext + fires pre-cached local siren via HRTF PannerNode at `position_x`. This fires **in milliseconds** — faster than any spoken word |
| **Layer 3 — Temporal Consistency** | Hazard must appear in 2+ consecutive frames before `log_hazard_event()` fires (prevents false positives from JPEG artifacts). Scene change >40% → pause guidance + announce "Scene changed, recalibrating" |

```
Hazard in frame
      |
      v
Gemini calls log_hazard_event(position_x=0.8, distance_category="very_close", type="drop", confidence=0.92)
      |
      +---> ADK emits {"type":"HARD_STOP","position_x":0.8,"distance":"very_close"}  ← milliseconds
      |           |
      |           v
      |     PWA AudioContext.close()  ← kills current audio instantly
      |     PWA fires Sonar Ping at position_x=0.8:
      |       distance=very_close → ping interval 100ms  ← rapid-fire, panic signal
      |       distance=mid       → ping interval 400ms  ← urgent but navigable
      |       distance=far       → ping interval 800ms  ← awareness only
      |
      +---> ARIA speaks: "Stop. Drop ahead, very close, right side. Do not move."
```

### 6.3 Confidence Signaling Protocol

| Level | Pattern | Example |
|---|---|---|
| HIGH (clear frame, stable 2+ frames) | Direct + confirmation | "Path clear for 3 meters. Ready to move?" |
| MEDIUM (partial visibility) | Qualified | "Path appears clear, but I can only see about 2 meters. Proceed slowly?" |
| LOW (poor frame quality) | Stop + hand off | "I can't see clearly. Please stop and tell me what you feel ahead." |
| CRITICAL (hazard) | Hardware interrupt fires first, ARIA confirms | [siren from right ear] + "Stop. Drop ahead, right side. Do not move." |

### 6.4 Hallucination Mitigation

- **2-frame confirmation:** Hazard must appear in 2 consecutive frames before interrupt fires
- **40% scene change threshold:** Large scene delta → pause + recalibrate, not immediate reinterpret
- **Distance qualifier policy:** Always qualitative — no metric distances without depth sensor
- **Identity humility:** Never names a specific person; "a person who appears to be..."
- **OCR 2-frame:** Text confirmed across 2 frames before reading aloud
- **Kinematic safety boost:** `motion_state = "walking_fast"` + visible hazard → skip 2-frame check, interrupt immediately

---

## 7. UX Design Specification

### 7.1 UX Philosophy

**The voice IS the UI.** Every pixel is inaccessible. Every interaction must work screen-off, VoiceOver/TalkBack active, phone in pocket or hanging from neck lanyard.

HRTF Spatial Audio elevates this further: the *location* of sound is itself the message. The user's auditory instincts respond before conscious thought.

### 7.2 Wake Lock + OLED Black Mode *(v1.2 fix)*

**The problem:** Screen lock kills `getUserMedia` camera stream and WebSocket on most mobile browsers. For a blind user, this happens constantly — they have no reason to keep the screen on.

**The solution:**

```typescript
// frontend/src/hooks/useWakeLock.ts
export async function activateNavigationMode(): Promise<void> {
  // 1. Request screen wake lock — prevents auto screen-off
  let wakeLock: WakeLockSentinel | null = null;
  try {
    wakeLock = await navigator.wakeLock.request('screen');
  } catch (err) {
    console.warn('Wake Lock not supported, falling back to keepalive ping');
    // Fallback: play silent audio every 25s to prevent browser suspension
    startSilentAudioKeepalive();
  }

  // 2. Switch UI to OLED Black mode
  // Screen stays ON (wake lock) but displays pure #000000
  // On OLED screens: zero power from display pixels
  // On LCD screens: same backlight as any other color (documented caveat)
  document.body.style.backgroundColor = '#000000';
  document.body.style.filter = 'brightness(0)'; // Belt + suspenders
  setOLEDBlackMode(true);

  // 3. Reactivate wake lock on visibility change (tab switch, brief lock)
  document.addEventListener('visibilitychange', async () => {
    if (document.visibilityState === 'visible' && wakeLock?.released) {
      wakeLock = await navigator.wakeLock.request('screen');
    }
  });
}
```

**Demo talking point:** *"We use Wake Lock API to keep the camera alive, combined with an OLED Black display — the screen stays on for our sensors but emits zero light from its pixels on OLED hardware, extending battery life."*

> **Documented caveat:** OLED power saving applies to OLED panels (iPhone X+, most flagship Android). On LCD devices, backlight power is unchanged. This is clearly documented in onboarding and README.

### 7.3 HRTF Spatial Audio Engine + Sonar Ping Matrix *(v1.2 new)*

**Concept:** When ARIA warns about something to the right, her voice (and the siren) physically come from the right. The user's spatial hearing instinct kicks in before conscious processing. This is neurological shortcutting — faster and more reliable than any verbal direction.

**Implementation:**

```typescript
// frontend/src/services/spatialAudioEngine.ts

type DistanceCategory = 'very_close' | 'mid' | 'far';

const PING_INTERVALS: Record<DistanceCategory, number> = {
  very_close: 100,   // Rapid-fire — panic signal, like car reversing sensor at 10cm
  mid:        400,   // Urgent but navigable
  far:        800,   // Awareness only
};

export class SpatialAudioEngine {
  private ctx: AudioContext;
  private panner: PannerNode;
  private sirenBuffer: AudioBuffer | null = null;
  private pingIntervalId: ReturnType<typeof setInterval> | null = null;

  constructor() {
    this.ctx = new (window.AudioContext || (window as any).webkitAudioContext)();

    this.panner = this.ctx.createPanner();
    this.panner.panningModel = 'HRTF';
    this.panner.distanceModel = 'inverse';
    this.panner.refDistance = 1;
    this.panner.maxDistance = 10000;
    this.panner.rolloffFactor = 1;
    this.panner.coneInnerAngle = 360;
    this.panner.coneOuterAngle = 0;
    this.panner.coneOuterGain = 0;
    this.panner.connect(this.ctx.destination);
    this.ctx.listener.setPosition(0, 0, 0);

    this.preloadSiren();
  }

  private async preloadSiren(): Promise<void> {
    const res = await fetch('/assets/alert_ping.mp3'); // Short ping, not long siren
    const buf = await res.arrayBuffer();
    this.sirenBuffer = await this.ctx.decodeAudioData(buf);
  }

  // Called by WebSocket handler on HARD_STOP event
  // position_x: -1.0 (left) → 1.0 (right)
  // distance: very_close | mid | far  ← Z-axis conveyed via rhythm, not volume
  public fireHardStop(positionX: number, distance: DistanceCategory): void {
    // Clear any existing ping rhythm
    if (this.pingIntervalId) clearInterval(this.pingIntervalId);

    // Set 3D position: X = left/right, Z = -1 for depth perception
    this.panner.setPosition(positionX * 3, 0, -1);

    // Start Sonar Ping rhythm — interval encodes distance
    const interval = PING_INTERVALS[distance];
    const firePing = () => {
      if (!this.sirenBuffer) return;
      const source = this.ctx.createBufferSource();
      source.buffer = this.sirenBuffer;
      source.connect(this.panner);
      source.start();
    };

    firePing(); // Immediate first ping
    this.pingIntervalId = setInterval(firePing, interval);

    // Auto-stop after 3 seconds (ARIA speech takes over)
    setTimeout(() => {
      if (this.pingIntervalId) clearInterval(this.pingIntervalId);
    }, 3000);
  }

  public stopPing(): void {
    if (this.pingIntervalId) clearInterval(this.pingIntervalId);
  }

  // Streaming PCM from Gemini — voice spatially positioned at last hazard
  public playChunk(pcmData: Float32Array, hazardPositionX: number = 0): void {
    this.panner.setPosition(hazardPositionX * 2, 0, -1);
    const buffer = this.ctx.createBuffer(1, pcmData.length, 24000);
    buffer.getChannelData(0).set(pcmData);
    const source = this.ctx.createBufferSource();
    source.buffer = buffer;
    source.connect(this.panner);
    source.start();
  }
}
```

**Why Sonar Ping rhythm beats volume-based distance:**
- `inverse rolloff` (volume falloff) requires conscious interpretation — slow
- Ping rhythm is processed instinctively by the brain — like a car reversing sensor
- 100ms interval = panic response without thinking. 800ms = calm awareness
- Works equally well at any volume level, any ambient noise

### 7.4 Audio Capture — AEC (Echo Cancellation) *(v1.3: critical fix)*

**The problem:** When ARIA speaks through the phone speaker, the microphone picks up that audio. VAD mistakes the siren/ARIA voice for user speech → self-interruption loop. Gemini hears its own output → hallucination risk.

**Fix — mandatory `getUserMedia` flags:**

```typescript
// frontend/src/hooks/useAudioStream.ts

const micStream = await navigator.mediaDevices.getUserMedia({
  video: false,
  audio: {
    echoCancellation: true,    // CRITICAL — prevents AEC death loop
    noiseSuppression: true,    // Reduces ambient noise for cleaner VAD
    autoGainControl: true,     // Normalizes mic level for consistent VAD
    sampleRate: 16000,         // Match Gemini Live input requirement
    channelCount: 1,           // Mono
  }
});
```

> **Note:** `echoCancellation: true` is a standard getUserMedia constraint supported across all target browsers. It leverages the device's hardware AEC chip where available (all modern smartphones), falling back to software AEC otherwise.

### 7.5 Hardware Recommendation — Bone-Conduction / Open-Ear *(v1.3 new)*

> **Hardware Paradigm:** VisionGPT is optimized for **open-ear or bone-conduction audio** (e.g., Shokz OpenRun, AfterShokz). This is not just an accessory suggestion — it is a safety requirement.

**Why bone-conduction or open-ear is mandatory for real-world use:**

| Concern | Standard Earbuds | Open-ear / Bone-conduction |
|---|---|---|
| Traffic awareness | **Dangerous** — blocks ambient sound | Full situational awareness maintained |
| AEC feedback | Prone to echo if mic too close to speaker | Physically separated — minimal feedback |
| HRTF accuracy | Earbud insertion changes ear canal acoustics | Open ear = HRTF works as designed |
| Comfort (all-day) | Ear fatigue after 1-2 hours | Designed for all-day wear |

**Demo note:** The demo actor wears visible bone-conduction headphones. This communicates the full design intent to judges in a single shot — no explanation needed.

**Mention in onboarding:** *"For the safest experience, use open-ear headphones or bone-conduction headphones so you can always hear traffic around you."*

**The problem:** Running Wake Lock + BIDI WebSocket + 1FPS JPEG encoding + DeviceMotion at 10Hz + HRTF processing simultaneously on a mobile browser creates serious thermal and battery stress. For a blind user, a dead phone mid-street is a survival threat, not a UX inconvenience.

**Solution: Adaptive Duty Cycling** — use `motionState` to govern camera framerate dynamically.

```typescript
// frontend/src/hooks/useCamera.ts — Adaptive Duty Cycling

export function useCamera(motionState: 'stationary' | 'walking_slow' | 'walking_fast' | 'running') {
  useEffect(() => {
    // Dynamic framerate based on motion — saves ~80% camera energy when stationary
    const intervalMs =
      motionState === 'stationary'    ? 5000 :  // 0.2 FPS — minimal awareness
      motionState === 'walking_slow'  ? 1000 :  // 1.0 FPS — full navigation
      motionState === 'walking_fast'  ? 1000 :  // 1.0 FPS — full + urgent
      /* running */                     500;    // 2.0 FPS — emergency mode

    const timer = setInterval(() => {
      captureAndSendFrame();
    }, intervalMs);

    return () => clearInterval(timer);
  }, [motionState]);
}
```

**Battery impact table:**

| Motion State | FPS | Relative Camera Energy | Scenario |
|---|---|---|---|
| stationary | 0.2 | ~10% of max | Waiting at traffic light, standing in store |
| walking_slow | 1.0 | 100% | Normal navigation |
| walking_fast | 1.0 | 100% | Active navigation, urgent context |
| running | 2.0 | 200% | Emergency mode only |

```typescript
// frontend/src/hooks/useMotionSensor.ts

interface MotionState {
  state: 'stationary' | 'walking_slow' | 'walking_fast' | 'running';
  pitch: number;
  velocity: number;
}

export function useMotionSensor(): { getMotionSnapshot: () => MotionState } {
  const accelHistory = useRef<number[]>([]);

  useEffect(() => {
    // iOS 13+: requires user gesture permission — requested in onboarding step 2
    const handler = (e: DeviceMotionEvent) => {
      const acc = e.accelerationIncludingGravity;
      if (!acc?.x) return;
      const magnitude = Math.sqrt(acc.x**2 + acc.y**2 + acc.z**2);
      accelHistory.current.push(magnitude);
      if (accelHistory.current.length > 10) accelHistory.current.shift();
    };
    window.addEventListener('devicemotion', handler);
    return () => window.removeEventListener('devicemotion', handler);
  }, []);

  const getMotionSnapshot = (): MotionState => {
    const avg = accelHistory.current.reduce((a, b) => a + b, 0) / (accelHistory.current.length || 1);
    const state =
      avg < 9.9  ? 'stationary' :
      avg < 11   ? 'walking_slow' :
      avg < 14   ? 'walking_fast' : 'running';
    return { state, pitch: 0, velocity: avg - 9.8 };
  };

  return { getMotionSnapshot };
}
```

**System prompt injection per frame:**
```
[KINEMATIC: User is walking_fast. Pitch: 15deg. Treat all hazards with maximum urgency.]
```

> **iOS caveat:** `DeviceMotionEvent` requires explicit user permission on iOS 13+. Requested in onboarding step 2 with audio explanation. Graceful degradation: system runs at fixed 1FPS without motion context if permission denied.

### 7.7 Gesture System

| Gesture | Action |
|---|---|
| One tap anywhere | Toggle mic on/off |
| Double tap | Repeat last ARIA response |
| Long press | Emergency — `request_human_help()` |
| Swipe up | Switch to next mode |
| Swipe down | "Describe where I am in detail" |
| Shake device | SOS + announce location |
| Voice (any) | ARIA always listening when session active |

### 7.8 Non-Speech Audio Cues *(v1.2: spatial audio cues added)*

| Sound | Spatial behavior | Meaning |
|---|---|---|
| 2-tone ascending chime | Center | Session connected |
| 1 short beep (high) | Center | Mic on |
| 1 short beep (low) | Center | Mic off |
| **Siren (local cached)** | **From hazard direction** | **HARD_STOP — critical danger. Fires before ARIA speaks** |
| Rapid 3 beeps | From hazard direction | Approaching obstacle, lower confidence |
| Sustained low tone | Center | ARIA speaking urgently |
| Soft descending tone | Center | Mode switched |
| 2 short vibrations | Center | Reconnecting — stay still |
| Soft warm chime | Center | Emotion-aware mode activated |

### 7.9 Visual UI (Judges / Sighted Companions)

- Full-screen camera viewfinder
- **OLED Black overlay** when Navigation mode active (screen stays on, emits no light on OLED)
- Mode indicator: large, high-contrast, top-center
- Audio waveform: ARIA vs user speech visualization
- Real-time transcript panel (collapsible)
- Connection quality: ms latency + signal strength
- **Hazard direction indicator:** semicircular compass at bottom — glowing dot shows hazard position
- Emotion indicator: badge color shifts blue → amber when emotion-aware fires

---

## 8. Implementation Plan

### 8.1 Repository Structure

```
visiongpt/
+-- frontend/                         # React PWA
|   +-- src/
|   |   +-- hooks/
|   |   |   +-- useCamera.ts          # 1FPS capture at 768x768
|   |   |   +-- useAudioStream.ts     # 16kHz PCM mic
|   |   |   +-- useARIA.ts            # WebSocket BIDI manager
|   |   |   +-- useWakeLock.ts        # Wake Lock + OLED Black mode
|   |   |   +-- useMotionSensor.ts    # DeviceMotionEvent sensor fusion
|   |   +-- components/
|   |   |   +-- CameraView.tsx
|   |   |   +-- TranscriptPanel.tsx
|   |   |   +-- ModeIndicator.tsx
|   |   |   +-- HazardCompass.tsx     # Semicircular hazard direction UI
|   |   |   +-- OLEDBlackOverlay.tsx
|   |   +-- services/
|   |   |   +-- spatialAudioEngine.ts # HRTF PannerNode engine
|   |   |   +-- haptics.ts
|   |   |   +-- audioCache.ts         # Layer 0 fallback cache
|   |   +-- App.tsx
+-- backend/
|   +-- agent/
|   |   +-- aria_agent.py             # ADK Agent definition
|   |   +-- run_config.py             # RunConfig with SessionResumption
|   |   +-- session_manager.py        # Firestore context bridge
|   |   +-- websocket_handler.py      # Emits HARD_STOP on tool call
|   |   +-- tools/
|   |       +-- hazard_logger.py      # Fires HARD_STOP + logs to Firestore
|   |       +-- context_manager.py
|   |       +-- emotion_logger.py
|   |       +-- human_help.py
|   |       +-- mode_switcher.py
|   +-- main.py
|   +-- Dockerfile
|   +-- requirements.txt
+-- infra/
|   +-- main.tf
|   +-- cloud_run.tf
|   +-- firestore.tf
|   +-- iam.tf
|   +-- variables.tf
+-- .github/
|   +-- workflows/
|       +-- deploy.yml                # GitHub Actions auto-deploy
+-- docs/
|   +-- BLUEPRINT.md
|   +-- ARCHITECTURE.png
|   +-- SYSTEM_PROMPT.md
+-- assets/
|   +-- alert_siren.mp3               # Pre-cached local siren (Layer 2)
+-- README.md
```

### 8.2 12-Day Build Sprint *(v1.2 updated)*

| Day | Focus | Deliverable |
|---|---|---|
| Day 1 | Repo init + ADK backend + RunConfig | Gemini Live voice echo works end-to-end |
| Day 2 | Camera 1FPS + WebSocket BIDI | Frames flowing to backend, Gemini sees camera |
| Day 3 | SessionResumption + ContextCompression | Session survives 2-min video boundary transparently |
| Day 4 | System prompt v1.2 + 4 modes + emotion | ARIA persona active, affective dialog on |
| Day 5 | **Hardware Interrupt pipeline** — `log_hazard_event` → `HARD_STOP` → local siren | Tool call fires siren in <100ms |
| Day 6 | **SpatialAudioEngine** + HRTF PannerNode + position pipeline | Siren and ARIA voice come from correct direction |
| Day 7 | **Wake Lock + OLED Black mode** + DeviceMotion sensor fusion | Screen stays on in pocket; motion injected per frame |
| Day 8 | PWA frontend — gesture system, audio cues, HazardCompass, barge-in | Full UX working |
| Day 9 | Terraform IaC + GitHub Actions CI/CD + Cloud Run deploy | One-command deploy, proof ready |
| Day 10 | End-to-end testing: indoor + **outdoor 4G** + earphone HRTF | 15+ scenarios; verify spatial audio with headphones |
| Day 11 | Demo recording (3 takes, with earphones) + architecture diagram | 4-min video final |
| Day 12 | Devpost submit + blog post publish | Done |

---

## 9. Key Technical Specifications

### 9.1 Performance Targets

| Parameter | Value / Target |
|---|---|
| Primary model | `gemini-live-2.5-flash-native-audio` |
| Video input | **Adaptive: 1 FPS when moving, 0.2 FPS (5s interval) when stationary** — saves ~80% camera energy during waits. 768×768 JPEG. Deliberate safety boundary: unsuitable for fast movement. |
| Audio input | 16kHz mono PCM, 50–100ms chunks |
| Audio output | 24kHz PCM → SpatialAudioEngine |
| HARD_STOP latency (tool call → siren) | Target < 100ms |
| Target TTFT | < 600ms on 4G |
| End-to-end latency (frame → audio) | < 1.5s |
| Session video limit | 2 min → solved by ContextWindowCompression |
| Connection limit | ~10 min → solved by SessionResumption |
| Firestore writes | <= 1 per 30s |
| Bandwidth upstream | ~20 KB/s (video + audio + motion) |
| Bandwidth downstream | ~30 KB/s (24kHz audio) |
| Battery target | < 12%/hr moving, < 5%/hr stationary (Adaptive Duty Cycling + OLED Black) |
| Wake Lock support | Chrome 84+, Safari 16.4+, Firefox (partial) |

### 9.2 Full ADK RunConfig

```python
# backend/agent/run_config.py
from google.adk.agents.run_config import RunConfig, StreamingMode
from google.genai import types

def build_run_config() -> RunConfig:
    return RunConfig(
        streaming_mode=StreamingMode.BIDI,
        response_modalities=["AUDIO"],
        input_audio_transcription=types.AudioTranscriptionConfig(),
        output_audio_transcription=types.AudioTranscriptionConfig(),
        session_resumption=types.SessionResumptionConfig(
            transparent=True
        ),
        context_window_compression=types.ContextWindowCompressionConfig(
            trigger_tokens=100000,
            sliding_window=types.SlidingWindow(target_tokens=80000)
        ),
        speech_config=types.SpeechConfig(
            voice_config=types.VoiceConfig(
                prebuilt_voice_config=types.PrebuiltVoiceConfig(voice_name="Kore")
            ),
            enable_affective_dialog=True,
            enable_proactivity=True,
        ),
    )
```

### 9.3 Hazard Logger Tool (Layer 2 Hardware Interrupt)

```python
# backend/agent/tools/hazard_logger.py
from google.adk.tools import tool

active_connections: dict[str, object] = {}

@tool
async def log_hazard_event(
    hazard_type: str,
    position_x: float,          # -1.0 (left) → 0 (center) → 1.0 (right)
    distance_category: str,     # "very_close" | "mid" | "far"
    confidence: float,
    description: str,
    session_id: str,
) -> str:
    """
    Called by ARIA immediately upon hazard detection — BEFORE speaking.
    Fires hardware interrupt to PWA with position + distance for Sonar Ping.
    """
    ws = active_connections.get(session_id)
    if ws:
        await ws.send_json({
            "type": "HARD_STOP",
            "position_x": position_x,
            "distance": distance_category,   # PWA uses this for ping rhythm
            "hazard_type": hazard_type,
            "confidence": confidence,
        })

    await firestore_log_hazard(session_id, hazard_type, position_x, distance_category, confidence, description)
    return f"Interrupt fired. {hazard_type} at x={position_x}, distance={distance_category}"
```

### 9.4 Terraform IaC

```hcl
# infra/cloud_run.tf
resource "google_cloud_run_v2_service" "aria_backend" {
  name     = "visiongpt-aria-backend"
  location = var.region

  template {
    containers {
      image = "gcr.io/${var.project_id}/aria-backend:latest"
      resources { limits = { cpu = "2", memory = "2Gi" } }
      env { name = "GOOGLE_CLOUD_PROJECT", value = var.project_id }
    }
  }
}

resource "google_firestore_database" "aria_sessions" {
  name        = "(default)"
  location_id = var.region
  type        = "FIRESTORE_NATIVE"
}
```

---

## 10. Contest Scoring Strategy

### 10.1 Judging Criteria Alignment

| Criterion (Weight) | VisionGPT v1.2 | Expected Score |
|---|---|---|
| Innovation & Multimodal UX (40%) | HRTF Sonar Ping Matrix (distance conveyed via rhythm, not volume). Bone-conduction hardware recommendation. Adaptive Duty Cycling showing physical-world constraints solved. Emotion-aware. No competitor is within 3 years of this combination on commodity hardware. | **5.0 / 5** |
| Technical Implementation (30%) | ADK native session resumption + context compression. Tool-triggered hardware interrupt. AEC echoCancellation mandatory flag. Wake Lock + OLED Black. Adaptive framerate via motionState. Architecturally sound for native audio streaming. | **4.8 / 5** |
| Demo & Presentation (30%) | Real outdoor footage. Earphone demo shows spatial audio physically working. Architecture diagram includes hardware interrupt pipeline. | **4.7 / 5** |
| Bonus: Blog post | "How HRTF Spatial Audio and ADK Tool Interrupts Make AI Navigation Actually Safe for Blind Users" | **+0.6** |
| Bonus: Terraform + GitHub Actions | Full IaC in /infra + auto-deploy workflow | **+0.2** |

**Estimated Final Score: ~5.6 / 6.0**

### 10.2 Demo Video Script — v1.3

| Timestamp | Content |
|---|---|
| 0:00 – 0:20 | **Hook:** "253 million blind people. Today's AI makes them tap and wait. We built something different." Show BeMyEyes tap-wait as contrast |
| 0:20 – 0:50 | **Spatial audio + bone-conduction opening** *(actor wearing Shokz visibly)*: Obstacle to the right. Rapid-fire ping (100ms) from right ear. ARIA voice from right. Actor turns left instinctively. Subtitle: *"Warning ping FROM the direction of danger. Rhythm = distance."* |
| 0:50 – 1:00 | **Adaptive framerate overlay** *(stationary pause before walking)*: On-screen:  then actor starts walking: . Subtitle: *"Adaptive Duty Cycling: 80% battery saved when still"* |
| 1:00 – 1:40 | **Outdoor on 4G** *(subtitle: "Real 1FPS stream — no mockup")*: Step down → HARD_STOP + rapid ping from right → ARIA confirms. Traffic light read. Session reconnects invisibly |
| 1:40 – 2:10 | **Indoor navigation:** Chair detected proactively. Sign read unprompted. Barge-in mid-sentence. All real |
| 2:10 – 2:40 | **Emotion-aware:** Busy street, user tense. ARIA: *"You seem tense. Let's slow down."* Speech audibly slows. Subtitle: *"Zero extra API calls — Gemini affective dialog native"* |
| 2:40 – 3:05 | **READ mode:** Restaurant menu + product label. Fast and verbatim |
| 3:05 – 3:35 | **Technical proof:** Architecture diagram with Hardware Interrupt + Sonar Ping Matrix. ADK RunConfig. Cloud Run. Firestore HARD_STOP logs |
| 3:35 – 4:00 | **Close:** Split: tap-wait vs VisionGPT live+spatial+adaptive. *"VisionGPT — Eyes for everyone."* |

> **Production notes:**
> - Record with stereo — verify L/R ping separation in final export
> - Spatial audio moment (0:20-0:50): judges with earphones must feel the ping shift
> - Adaptive framerate overlay (0:50-1:00): engineers will love this detail
> - Actor wears Shokz bone-conduction throughout — design intent in one shot

### 10.3 Full Submission Checklist

- [ ] Devpost account registered
- [ ] Public GitHub repo — created after Feb 16, 2026
- [ ] README.md with local + GCP spin-up instructions
- [ ] Architecture diagram PNG in `/docs/` — includes Hardware Interrupt pipeline
- [ ] Cloud Run deployment proof: screen recording of console
- [ ] YouTube/Vimeo demo — max 4 min, English subtitles, stereo audio
- [ ] Devpost text description: problem, solution, tech, learnings
- [ ] Terraform IaC in `/infra/` — `terraform apply` works
- [ ] GitHub Actions in `.github/workflows/deploy.yml`
- [ ] Blog post: public, #GeminiLiveAgentChallenge, before March 12
- [ ] GDG membership link (if applicable)
- [ ] Category selected: **Live Agents**

---

## 11. Risk Register *(v1.2 updated)*

| Risk | Likelihood / Impact | Mitigation |
|---|---|---|
| Wake Lock not supported (older iOS) | Medium / High | Fallback: silent audio keepalive every 25s. Document in README. Test on iPhone before Day 10 |
| HRTF panning imperceptible without headphones | Medium / Medium | Demo recording requires earphones. Add subtitle explaining spatial audio. Verify stereo in final export |
| AEC feedback loop (speaker → mic) | High / High | `echoCancellation: true` in getUserMedia — mandatory, not optional. Enforce in code review. Also mitigated by bone-conduction recommendation (physically separates speaker from mic path) |
| Thermal death / battery drain | Medium / High | Adaptive Duty Cycling: 0.2FPS when stationary. Wake Lock only in Navigation mode. OLED Black reduces display power. Test 1-hour continuous session before demo day |
| Sonar ping too aggressive (constant 100ms) | Medium / Medium | Auto-stop ping after 3s (ARIA speech takes over). Only fires on HARD_STOP, not for mid/far hazards at same rate. Tune in Day 10 |
| `position_x` from Gemini inaccurate | Medium / Low | position_x from bounding box is approximate. Acceptable — left/center/right granularity is sufficient |
| DeviceMotion permission denied (iOS) | Low / Medium | Request in onboarding with clear audio explanation. Graceful degradation: run without motion context |
| 2-min session boundary during outdoor demo | Medium / High | `SessionResumptionConfig(transparent=True)` handles invisibly. Test 20+ reconnects |
| OLED Black caveat on LCD devices | Low / Low | Document clearly in onboarding and README. Not a functional issue |
| GCP costs exceed $100 | Low / Medium | Billing alerts at $50 and $80. 60-min/day session cap during dev |

---

## 12. Post-Hackathon Product Roadmap

### Phase 2 — Production (Months 1–3)
- **Native apps:** iOS (Swift + AVFoundation) + Android (Kotlin + CameraX) — background camera support, sub-800ms latency via Firebase AI Logic SDK
- **On-device TFLite:** Parallel obstacle detection for network-drop fallback (true Layer 0)
- **HRTF calibration:** Personalized HRTF profiles — different head shapes affect HRTF perception
- **User profiles:** Verbosity level, language, emergency contacts, familiar route memory

### Phase 3 — Platform (Months 4–12)
- Wearable: Apple Watch haptic compass + Android Wear spatial feedback
- Smart glasses: Envision Glasses API, future Google Glass
- Google Maps integration: street-level navigation + real-time rerouting
- Community: crowdsourced hazard reports per location
- B2B SDK: ARIA as embeddable accessibility module

> **Market Opportunity:** 253 million visually impaired globally (WHO). Assistive tech market $26B, 7.4% CAGR. VisionGPT removes the $300–$2,500 hardware barrier of all current solutions.

---

## Appendix: Key References

| Resource | Source |
|---|---|
| Gemini Live API session limits & video specs | cloud.google.com/vertex-ai/generative-ai/docs/live-api |
| ADK Streaming Dev Guide Part 4 (RunConfig) | google.github.io/adk-docs/streaming/dev-guide/part4/ |
| ADK SessionResumptionConfig + ContextWindowCompression | `google.adk.agents.run_config.RunConfig` |
| enable_affective_dialog + enable_proactivity | `google.genai.types.SpeechConfig` |
| Wake Lock API — browser support | MDN: developer.mozilla.org/en-US/docs/Web/API/Screen_Wake_Lock_API |
| Web Audio API PannerNode (HRTF) | MDN: developer.mozilla.org/en-US/docs/Web/API/PannerNode |
| DeviceMotionEvent — iOS permission | MDN: developer.mozilla.org/en-US/docs/Web/API/DeviceMotionEvent |
| ADK GitHub Repo | github.com/google/adk-python |
| WHO Visual Impairment Stats | who.int/news-room/fact-sheets/detail/blindness-and-visual-impairment |
| AIDEN blind user study (2024) | arxiv.org/html/2511.06080v1 |
| ObjectFinder blind UX patterns | arxiv.org/abs/2412.03118 |

---

*VisionGPT Blueprint v1.3 — March 2026*

*Changes from v1.2:*
- *Section 7.3 (HRTF): Upgraded to Sonar Ping Matrix — distance conveyed via ping rhythm (100/400/800ms), not volume rolloff. `alert_siren.mp3` → `alert_ping.mp3`*
- *Section 7.4 (AEC): Added mandatory `echoCancellation: true` to getUserMedia. Prevents VAD self-interruption and Gemini hallucination from speaker feedback*
- *Section 7.5 (Hardware): Added bone-conduction / open-ear recommendation — safety requirement, not accessory*
- *Section 7.6 (Kinematic): Added Adaptive Duty Cycling — 0.2FPS stationary, 1FPS walking, 2FPS running. ~80% battery saving when stationary*
- *Section 9.1: Performance targets updated with adaptive framerate and battery estimates*
- *Section 9.3: `log_hazard_event` updated with `distance_category` parameter*
- *Section 10.2: Demo script updated — adaptive framerate overlay shot, bone-conduction actor, Sonar Ping rhythm highlight*
- *Section 11: Risk register updated with thermal/battery risk and AEC risk*
