# Apollos — Cognitive Infrastructure (Eidolon-V)

Apollos is a **Safety-Critical, Rust-First Navigation Stack** serving as the Cognitive Infrastructure (Eidolon-V) for the visually impaired. It is built upon a Thermodynamic Causal Architecture designed to guarantee **Zero Hallucinations** and **Safe-by-Design** execution.

The legacy Python/TypeScript runtime has been completely eradicated.

## Core Architecture

Apollos is split into highly optimized Rust crates and Native Shells:

- **`apollos-core` (The Edge Brain):** Computes physics, optical flow, and depth on the edge. Exposes a zero-copy C ABI for mobile FFI.
- **`apollos-server` (The Orchestrator):** Axum-based WebSocket backend handling Gemini Live API duplex streaming, Tool Calls, and Thermodynamic State regulation.
- **`apollos-proto` (The Contract):** Type-safe, Protobuf-based communication removing Schema Drift between Edge and Cloud.
- **`native/android` & `native/ios` (The Sensory Organisms):** Ultra-thin native shells collecting 1000Hz IMU and RGB data, bridging to the Rust Core via JNI/Swift bindings.

## The Thermodynamic Causal Engine

Apollos operates strictly on Causal reasoning (Pearl's do-calculus), rejecting standard AI correlation.

1. **Anti-OOM Backpressure (Gemini Bridge):** Realtime duplex streaming using Bounded Channels (`mpsc::channel(1024)`) with `try_send`. Frames are intelligently dropped during network congestion to preserve memory, but audio is prioritized. Tool calls are strictly decoupled via `tokio::spawn` to prevent blocking the WebSocket loop (Vòng Lặp Tử Thần).
2. **Strict Kinematic Gating:** The Edge Brain refuses to capture data during Free Fall (Magnitude outside 8.0 - 12.0) or when upside-down. The FFI consumes dual Quaternions (not error-prone Gyro) for exact Semantic Odometry (Yaw/Pitch/Roll).
3. **Block Matching Algorithm (BMA):** Frame Differencing has been replaced by a rigorous Data-Oriented BMA for Optical Expansion, calculating pure depth-threats and ignoring ego-motion (walking side-to-side).
4. **Zero-Guess Depth Engine:** ONNX heuristics fallbacks are forbidden. If the Depth ML fails, the system safely degrades. We don't guess with human lives.
5. **Global Trauma Registry:** `apollos-server/session.rs` persists Hazard logs via Firestore (`USE_FIRESTORE=1`), building a collective Crowd Hazard Map for immediate system recoil upon repeated failures.

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

### Local Development / CI Checks

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

### Server Boot
```bash
cargo run -p apollos-server

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
