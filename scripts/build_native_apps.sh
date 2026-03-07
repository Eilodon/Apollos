#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

ANDROID_YOLO="${ROOT_DIR}/native/android/app/src/main/assets/models/yolov12.tflite"
ANDROID_DEPTH="${ROOT_DIR}/native/android/app/src/main/assets/models/depth_anything_v3.tflite"
IOS_YOLO="${ROOT_DIR}/native/ios/ApollosShell/Models/YOLOv12Detector.mlmodelc"
IOS_DEPTH="${ROOT_DIR}/native/ios/ApollosShell/Models/DepthAnythingV3.mlmodelc"

BUILD_ANDROID=1
BUILD_IOS=1
BUILD_RUST=1

usage() {
  cat <<USAGE
Build native Android/iOS apps after model installation.

Usage:
  $0 [--android-only] [--ios-only] [--skip-rust]

Options:
  --android-only   Build Android only
  --ios-only       Build iOS only
  --skip-rust      Skip scripts/build_native_core.sh
USAGE
}

build_android() {
  local android_dir="${ROOT_DIR}/native/android"

  if [[ -x "${android_dir}/gradlew" ]]; then
    (
      cd "$android_dir"
      ./gradlew :app:assembleDebug --no-daemon
    )
    echo "Android build done: native/android/app/build/outputs/apk/debug"
    return 0
  fi

  echo "Android wrapper (native/android/gradlew) not found." >&2
  echo "Use Android Studio (recommended) or create wrapper with Gradle 8+:" >&2
  echo "  cd native/android && gradle wrapper --gradle-version 8.7" >&2
  return 1
}

build_ios() {
  if ! command -v xcodebuild >/dev/null 2>&1; then
    echo "xcodebuild not found. iOS build must run on macOS with Xcode." >&2
    return 1
  fi

  local workspace
  workspace="$(find "${ROOT_DIR}/native/ios" -maxdepth 2 -name '*.xcworkspace' | head -n 1 || true)"
  local project
  project="$(find "${ROOT_DIR}/native/ios" -maxdepth 2 -name '*.xcodeproj' | head -n 1 || true)"
  local scheme="${APOLLOS_IOS_SCHEME:-ApollosShell}"

  if [[ -n "$workspace" ]]; then
    xcodebuild \
      -workspace "$workspace" \
      -scheme "$scheme" \
      -configuration Debug \
      -destination 'generic/platform=iOS Simulator' \
      build
  elif [[ -n "$project" ]]; then
    xcodebuild \
      -project "$project" \
      -scheme "$scheme" \
      -configuration Debug \
      -destination 'generic/platform=iOS Simulator' \
      build
  else
    echo "No .xcodeproj/.xcworkspace found under native/ios." >&2
    echo "Open native/ios in Xcode and ensure project files are present." >&2
    return 1
  fi

  echo "iOS build done (scheme=${scheme})."
}

ensure_models() {
  if [[ "$BUILD_ANDROID" -eq 1 ]]; then
    if [[ ! -f "$ANDROID_YOLO" || ! -f "$ANDROID_DEPTH" ]]; then
      echo "Android models missing. Run scripts/install_native_models.sh first." >&2
      return 1
    fi
  fi

  if [[ "$BUILD_IOS" -eq 1 ]]; then
    if [[ ! -d "$IOS_YOLO" || ! -d "$IOS_DEPTH" ]]; then
      echo "iOS models missing. Run scripts/install_native_models.sh first." >&2
      return 1
    fi
  fi

  return 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --android-only)
      BUILD_ANDROID=1
      BUILD_IOS=0
      shift
      ;;
    --ios-only)
      BUILD_ANDROID=0
      BUILD_IOS=1
      shift
      ;;
    --skip-rust)
      BUILD_RUST=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

ensure_models

if [[ "$BUILD_RUST" -eq 1 ]]; then
  echo "Building Rust FFI core..."
  "${ROOT_DIR}/scripts/build_native_core.sh"
fi

if [[ "$BUILD_ANDROID" -eq 1 ]]; then
  echo "Building Android app..."
  build_android
fi

if [[ "$BUILD_IOS" -eq 1 ]]; then
  echo "Building iOS app..."
  build_ios
fi

echo "Native build flow completed."
