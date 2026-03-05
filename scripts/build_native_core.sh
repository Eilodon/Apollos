#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# Android arm64
cargo build -p apollos-core --release --features "ffi" --target aarch64-linux-android
mkdir -p native/android/app/src/main/jniLibs/arm64-v8a
cp target/aarch64-linux-android/release/libapollos_core.so native/android/app/src/main/jniLibs/arm64-v8a/libapollos_core.so

# iOS device + simulator
cargo build -p apollos-core --release --features "ffi" --target aarch64-apple-ios
cargo build -p apollos-core --release --features "ffi" --target aarch64-apple-ios-sim
cargo build -p apollos-core --release --features "ffi" --target x86_64-apple-ios

echo "Native core artifacts built under target/<triple>/release/."
