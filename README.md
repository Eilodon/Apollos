# Apollos — Cognitive Infrastructure (Eidolon-V)

Apollos is a **Safety-Critical, Rust-First Navigation Stack** serving as the Cognitive Infrastructure (Eidolon-V) for the visually impaired. It is built upon a Thermodynamic Causal Architecture designed to guarantee **Zero Hallucinations** and **Safe-by-Design** execution.

## Core Architecture

Apollos is split into highly optimized Rust crates and Native Shells:

- **`apollos-core` (The Edge Brain):** Computes physics, optical flow, and depth on the edge. Exposes a zero-copy C ABI for mobile FFI.
- **`apollos-server` (The Orchestrator):** Axum-based WebSocket backend handling Gemini Live API duplex streaming, Tool Calls, and Thermodynamic State regulation.
- **`apollos-proto` (The Contract):** Type-safe, Protobuf-based communication removing Schema Drift between Edge and Cloud.
- **`native/android` & `native/ios` (The Sensory Organisms):** Ultra-thin native shells collecting 1000Hz IMU and RGB data, bridging to the Rust Core via JNI/Swift bindings.

## The Thermodynamic Causal Engine

Apollos operates strictly on Causal reasoning (Pearl's do-calculus), rejecting standard AI correlation.

1. **Bounded Realtime Backpressure:** Gemini live duplex uses bounded channels (`mpsc::channel`) with drop-on-congestion behavior for non-critical payloads, keeping the WS loop stable under burst load.
2. **Kinematic + Sensor Gating:** `apollos-core` evaluates motion/tilt/risk before expensive operations and tags degraded sensor conditions for downstream policy decisions.
3. **Depth Engine with Source Tagging:** ONNX inference is used when runtime/model is available; deterministic heuristic fallback is used otherwise, with `source` explicitly attached to outputs.
4. **Policy-Driven Safety Tiers:** `apollos-server/safety_policy.rs` maps confidence, distance, motion, sensor health, and edge reflex into `Silent/Ping/Voice/HardStop/HumanEscalation`.
5. **Session Persistence + Human Escalation:** Firestore persistence and Twilio help escalation are first-class runtime paths with production strictness controls.

## Workspace Layout

```text
crates/
  apollos-core     # FFI, Physics, Block Matcher, Depth Engine
  apollos-server   # Axum, Gemini Bridge, Session, Tool Registry
  apollos-proto    # Protobuf definitions
  apollos-bench    # Benchmarking
native/
  android/         # Kotlin Shell + C++ JNI Config
  ios/             # Swift UI + C Header Bridge
scripts/
  build_native_core.sh
```

## Running the Engine

```bash
# Production server boot
cargo run -p apollos-server --release

# Health check
curl http://127.0.0.1:8000/healthz
```

### Native Core Building
```bash
./scripts/build_native_core.sh
```

## Crucial Environment Variables

**Gemini Live Orchestrator:**
- `ENABLE_GEMINI_LIVE=1`
- `GEMINI_API_KEY` (Required)
- `GEMINI_MODEL` (Default: `gemini-2.5-flash`)

**Trauma Registry (Firestore):**
- `USE_FIRESTORE=1`
- `GOOGLE_CLOUD_PROJECT`
- `FIRESTORE_BEARER_TOKEN` (or Cloud Run Default Service Account)

**Human Support Escapement (Twilio):**
- `TWILIO_ACCOUNT_SID`
- `TWILIO_VIDEO_API_KEY_SID`
- `TWILIO_VIDEO_API_KEY_SECRET`
