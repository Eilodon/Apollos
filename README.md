# Apollos — Rust Core + Native Shells

Apollos is now a Rust-first safety navigation stack:

- `apollos-core`: edge/reflex logic + C ABI for mobile FFI
- `apollos-server`: Axum backend with Gemini Live WS + tool-calls
- `apollos-proto`: shared contracts and protobuf transport
- `apollos-bench`: benchmark harness
- `native/android` + `native/ios`: thin native shells calling Rust core

Legacy TypeScript/Python runtime has been removed.

## Workspace

```text
crates/
  apollos-core
  apollos-server
  apollos-proto
  apollos-bench
native/
  android/
  ios/
scripts/
  build_native_core.sh
```

## Core Commands

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p apollos-core --features ml --all-targets -- -D warnings
```

Run backend:

```bash
cargo run -p apollos-server
```

Health check:

```bash
curl http://127.0.0.1:8000/healthz
```

## Gemini Live

`apollos-server` supports:

- Live WebSocket bridge to Gemini BidiGenerateContent
- Realtime frame/audio forwarding
- Tool-call dispatch + tool-response loop
- REST `generateContent` fallback path

Important envs:

- `ENABLE_GEMINI_LIVE=1`
- `GEMINI_API_KEY` or `GOOGLE_API_KEY`
- `GEMINI_MODEL` (default `gemini-2.5-flash`)
- `GEMINI_LIVE_ENDPOINT_BASE` (optional override)

## Firestore Persistence

Session and event persistence is available via Firestore REST when enabled:

- `USE_FIRESTORE=1`
- `GOOGLE_CLOUD_PROJECT=<project-id>`
- Auth: either
  - `FIRESTORE_BEARER_TOKEN=<oauth-token>`
  - or Cloud metadata token (Cloud Run / GCE default service account)

Persisted data:

- `sessions/{session_id}`
- `sessions/{session_id}/hazards`
- `sessions/{session_id}/emotions`
- `hazard_map/{geohash-hazard}`

## Twilio Human Help Tokens

Human fallback now mints real Twilio Video access tokens when configured:

- `TWILIO_ACCOUNT_SID`
- `TWILIO_VIDEO_API_KEY_SID`
- `TWILIO_VIDEO_API_KEY_SECRET`
- `TWILIO_VIDEO_ROOM_PREFIX` (optional, default `apollos-help`)
- `TWILIO_VIDEO_TOKEN_TTL_SECONDS` (optional)

If credentials are absent, service falls back to explicit stub token strings.

## Native FFI Integration

`apollos-core` exports C ABI symbols from `crates/apollos-core/src/ffi.rs`:

- `apollos_abi_version_u32`
- `apollos_analyze_kinematics`
- `apollos_compute_yaw_delta`
- `apollos_get_carry_mode_profile`

Build native artifacts:

```bash
./scripts/build_native_core.sh
```

Android shell uses JNI bridge (`native/android/app/src/main/cpp/rust_bridge.cpp`) and Kotlin wrapper (`RustCoreBridge.kt`).

iOS shell uses Swift bridge (`native/ios/ApollosShell/RustCoreBridge.swift`) + C header (`ApollosCoreFFI.h`).
