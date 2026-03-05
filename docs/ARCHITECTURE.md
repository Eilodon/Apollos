# Apollos Architecture (Rust + Native)

## Runtime split

- `apollos-core` (Rust): edge reflex, kinematic gating, depth engine, FFI surface.
- `apollos-server` (Rust/Axum): websocket transport, Gemini Live bridge, auth, persistence, human fallback.
- `apollos-proto` (Rust): shared contracts and protobuf envelopes.
- `native/android` + `native/ios`: thin UI/sensor/audio shells invoking Rust over FFI.

## Data path

1. Native shell captures motion/frame/audio.
2. Shell calls `apollos-core` FFI for reflex/risk computations.
3. Shell forwards multimodal events to `apollos-server` over WS.
4. Server relays realtime input to Gemini Live and handles tool calls.
5. Server emits assistant/hard-stop/help responses back to clients.
6. Session/hazard/emotion data persists to Firestore when enabled.
