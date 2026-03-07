#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# Android NDK Detection & Env Setup
if [[ -z "${ANDROID_HOME:-}" ]]; then
    export ANDROID_HOME="$HOME/Android/Sdk"
fi

NDK_DIR=$(ls -d $ANDROID_HOME/ndk/* 2>/dev/null | sort -V | tail -n 1)
if [[ -z "$NDK_DIR" ]]; then
    echo "Error: Android NDK not found in $ANDROID_HOME/ndk. Please set ANDROID_HOME."
    exit 1
fi

echo "Using NDK: $NDK_DIR"
HOST_TAG="linux-x86_64"
if [[ "$(uname)" == "Darwin" ]]; then HOST_TAG="darwin-x86_64"; fi

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_DIR/toolchains/llvm/prebuilt/$HOST_TAG/bin/aarch64-linux-android29-clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$NDK_DIR/toolchains/llvm/prebuilt/$HOST_TAG/bin/llvm-ar"

# Android arm64
RUSTFLAGS="-C link-arg=-Wl,-soname,libapollos_core.so" \
cargo build -p apollos-core --release --features "ffi" --target aarch64-linux-android
mkdir -p native/android/app/src/main/jniLibs/arm64-v8a
cp target/aarch64-linux-android/release/libapollos_core.so native/android/app/src/main/jniLibs/arm64-v8a/libapollos_core.so

# iOS device + simulator (Mac only)
if [[ "$(uname)" == "Darwin" ]]; then
    cargo build -p apollos-core --release --features "ffi" --target aarch64-apple-ios
    cargo build -p apollos-core --release --features "ffi" --target aarch64-apple-ios-sim
    cargo build -p apollos-core --release --features "ffi" --target x86_64-apple-ios
else
    echo "Skipping iOS build on non-macOS platform."
fi

echo "Native core artifacts built under target/<triple>/release/."
