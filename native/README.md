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

## iOS wiring

1. Build Rust iOS artifact.
2. Link `libapollos_core.a` in Xcode.
3. Add `ApollosCoreFFI.h` to Objective-C bridging header.
4. Run `ApollosShell` app.
