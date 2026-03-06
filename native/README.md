# Native Shells + FFI

This folder contains thin Android/iOS shells that call into `apollos-core` via C ABI.

## Build Rust Core Artifacts

Run:

```bash
./scripts/build_native_core.sh
```

The script builds:
- Android `libapollos_core.so` for `arm64-v8a`
- iOS static libraries for `aarch64-apple-ios` and simulator (`aarch64`/`x86_64`)

## Android wiring

1. Build Rust Android artifact.
2. Copy `target/aarch64-linux-android/release/libapollos_core.so` to:
   `native/android/app/src/main/jniLibs/arm64-v8a/libapollos_core.so`
3. Open `native/android` with Android Studio and run.
4. In app UI, provide:
   - backend base URL (`http://10.0.2.2:8000` for emulator)
   - OIDC `id_token`
5. Grant runtime permissions (camera/mic/location), then start live session.

## iOS wiring

1. Build Rust iOS artifact.
2. Link `libapollos_core.a` in Xcode.
3. Add `ApollosCoreFFI.h` to Objective-C bridging header.
4. Ensure `Info.plist` includes camera/mic/location usage descriptions.
5. Run `ApollosShell` app and provide backend URL + OIDC `id_token`.

## WS Payload Smoke (Android/iOS)

Run end-to-end WebSocket smoke verification for native payload contracts:

```bash
./scripts/native_ws_smoke.sh
```

This test opens a local server, obtains a dev WS ticket, sends Android/iOS-style
`multimodal_frame` payloads (including `sensor_health` + `sensor_uncertainty`),
and verifies observability updates end-to-end.
