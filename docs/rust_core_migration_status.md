# Rust Core Migration Status

## Completed

1. ONNX inference integration in `apollos-core/depth_engine` with runtime model loading (`APOLLOS_DEPTH_ONNX_MODEL`).
2. Gemini Live parity in `apollos-server/gemini_bridge`:
   - Bidi WebSocket session lifecycle
   - realtime frame/audio/user-command forwarding
   - tool-call extraction + tool-response dispatch
   - REST fallback when live path unavailable.
3. Firestore persistence parity in `SessionStore`:
   - session snapshots
   - hazard/emotion subcollections
   - hazard_map seed updates
   - persistence throttling and metadata-token auth support.
4. Twilio token minting:
   - real Video Access Token JWT minting via API key secret
   - automatic fallback to stub tokens when Twilio env is missing.
5. Native shell scaffolding + FFI integration:
   - `apollos-core` exported C ABI (`cdylib`/`staticlib`)
   - Android JNI bridge and Compose shell
   - iOS Swift bridge and shell sources.
6. Phase-4 legacy removal:
   - removed `backend/` (Python runtime)
   - removed `frontend/` (TypeScript/PWA runtime)
   - removed Python regression scripts and local `.venv`.

## Validation

- `cargo fmt --all`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo clippy -p apollos-core --features ml --all-targets -- -D warnings`

All checks pass on the current tree.
