#!/usr/bin/env bash

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${PROJECT_ROOT}/assets/models"
MODEL_FILE="${APOLLOS_DEPTH_ONNX_MODEL_FILE:-depth_model.onnx}"
TARGET_PATH="${TARGET_DIR}/${MODEL_FILE}"
MODEL_URL="${APOLLOS_DEPTH_ONNX_MODEL_URL:-}"

echo "Apollos ONNX model setup"

if [ -z "$MODEL_URL" ]; then
  echo "Set APOLLOS_DEPTH_ONNX_MODEL_URL to download a model."
  echo "Example: APOLLOS_DEPTH_ONNX_MODEL_URL=https://.../model.onnx ./scripts/download_depth_model.sh"
  exit 1
fi

mkdir -p "$TARGET_DIR"

if [ -f "$TARGET_PATH" ]; then
  echo "Model already exists at: $TARGET_PATH"
  exit 0
fi

if command -v curl >/dev/null 2>&1; then
  curl -L --fail --progress-bar "$MODEL_URL" -o "$TARGET_PATH"
elif command -v wget >/dev/null 2>&1; then
  wget -O "$TARGET_PATH" "$MODEL_URL"
else
  echo "Neither curl nor wget is available"
  exit 1
fi

echo "Model downloaded: $TARGET_PATH"
echo "Set APOLLOS_DEPTH_ONNX_MODEL=$TARGET_PATH when running apollos-core with --features ml."
