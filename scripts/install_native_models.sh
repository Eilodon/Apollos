#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_MODELS_DIR="${ROOT_DIR}/native/android/app/src/main/assets/models"
IOS_MODELS_DIR="${ROOT_DIR}/native/ios/ApollosShell/Models"
MANIFEST_FILE="${ROOT_DIR}/native/model_manifest.sha256"

ANDROID_YOLO_SRC=""
ANDROID_DEPTH_SRC=""
IOS_YOLO_SRC=""
IOS_DEPTH_SRC=""

usage() {
  cat <<USAGE
Install native YOLO + Depth models with canonical names.

Usage:
  $0 \
    --android-yolo /path/to/yolo.tflite \
    --android-depth /path/to/depth.tflite \
    --ios-yolo /path/to/YOLOv12Detector.mlmodelc|.mlmodel|.mlpackage \
    --ios-depth /path/to/DepthAnythingV3.mlmodelc|.mlmodel|.mlpackage

Output paths:
  Android: ${ANDROID_MODELS_DIR}/yolov12.tflite
  Android: ${ANDROID_MODELS_DIR}/depth_anything_v3.tflite
  iOS:     ${IOS_MODELS_DIR}/YOLOv12Detector.mlmodelc
  iOS:     ${IOS_MODELS_DIR}/DepthAnythingV3.mlmodelc
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --android-yolo)
      ANDROID_YOLO_SRC="${2:-}"
      shift 2
      ;;
    --android-depth)
      ANDROID_DEPTH_SRC="${2:-}"
      shift 2
      ;;
    --ios-yolo)
      IOS_YOLO_SRC="${2:-}"
      shift 2
      ;;
    --ios-depth)
      IOS_DEPTH_SRC="${2:-}"
      shift 2
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

for required in "$ANDROID_YOLO_SRC" "$ANDROID_DEPTH_SRC" "$IOS_YOLO_SRC" "$IOS_DEPTH_SRC"; do
  if [[ -z "$required" ]]; then
    echo "Missing required arguments." >&2
    usage
    exit 1
  fi
  if [[ ! -e "$required" ]]; then
    echo "Model path does not exist: $required" >&2
    exit 1
  fi
done

mkdir -p "$ANDROID_MODELS_DIR" "$IOS_MODELS_DIR"

cp "$ANDROID_YOLO_SRC" "${ANDROID_MODELS_DIR}/yolov12.tflite"
cp "$ANDROID_DEPTH_SRC" "${ANDROID_MODELS_DIR}/depth_anything_v3.tflite"

compile_or_copy_ios_model() {
  local src="$1"
  local canonical_name="$2"
  local target_dir="${IOS_MODELS_DIR}/${canonical_name}.mlmodelc"

  rm -rf "$target_dir"

  if [[ "$src" == *.mlmodelc ]]; then
    cp -R "$src" "$target_dir"
    return
  fi

  if [[ "$src" != *.mlmodel && "$src" != *.mlpackage ]]; then
    echo "Unsupported iOS model format for ${canonical_name}: $src" >&2
    exit 1
  fi

  if ! command -v xcrun >/dev/null 2>&1; then
    echo "xcrun not found. Provide precompiled .mlmodelc for ${canonical_name} or run on macOS with Xcode." >&2
    exit 1
  fi

  local tmp_dir
  tmp_dir="$(mktemp -d)"

  xcrun coremlcompiler compile "$src" "$tmp_dir" >/dev/null

  local compiled
  compiled="$(find "$tmp_dir" -maxdepth 1 -type d -name '*.mlmodelc' | head -n 1)"
  if [[ -z "$compiled" ]]; then
    rm -rf "$tmp_dir"
    echo "CoreML compile did not produce .mlmodelc for ${canonical_name}" >&2
    exit 1
  fi

  cp -R "$compiled" "$target_dir"
  rm -rf "$tmp_dir"
}

compile_or_copy_ios_model "$IOS_YOLO_SRC" "YOLOv12Detector"
compile_or_copy_ios_model "$IOS_DEPTH_SRC" "DepthAnythingV3"

sha256_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
  else
    shasum -a 256 "$path" | awk '{print $1}'
  fi
}

sha256_dir() {
  local dir="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    tar -cf - -C "$dir" . | sha256sum | awk '{print $1}'
  else
    tar -cf - -C "$dir" . | shasum -a 256 | awk '{print $1}'
  fi
}

cat > "$MANIFEST_FILE" <<MANIFEST
# generated_at=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
$(sha256_file "${ANDROID_MODELS_DIR}/yolov12.tflite")  native/android/app/src/main/assets/models/yolov12.tflite
$(sha256_file "${ANDROID_MODELS_DIR}/depth_anything_v3.tflite")  native/android/app/src/main/assets/models/depth_anything_v3.tflite
$(sha256_dir "${IOS_MODELS_DIR}/YOLOv12Detector.mlmodelc")  native/ios/ApollosShell/Models/YOLOv12Detector.mlmodelc
$(sha256_dir "${IOS_MODELS_DIR}/DepthAnythingV3.mlmodelc")  native/ios/ApollosShell/Models/DepthAnythingV3.mlmodelc
MANIFEST

echo "Installed models successfully."
echo "- Android: ${ANDROID_MODELS_DIR}/yolov12.tflite"
echo "- Android: ${ANDROID_MODELS_DIR}/depth_anything_v3.tflite"
echo "- iOS: ${IOS_MODELS_DIR}/YOLOv12Detector.mlmodelc"
echo "- iOS: ${IOS_MODELS_DIR}/DepthAnythingV3.mlmodelc"
echo "- Manifest: ${MANIFEST_FILE}"
