#!/usr/bin/env bash

# Exit immediately if a command exits with a non-zero status
set -e

# Define directories
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${PROJECT_ROOT}/frontend/public/models"
MODEL_FILE="depth_anything_v2_small_fp16.tflite"
TARGET_PATH="${TARGET_DIR}/${MODEL_FILE}"

# Model URL (Qualcomm AI Hub - Depth Anything V2 TFLite Float)
MODEL_URL="https://qaihub-public-assets.s3.us-west-2.amazonaws.com/qai-hub-models/models/depth_anything_v2/releases/v0.47.0/depth_anything_v2-tflite-float.zip"

echo "=========================================================="
echo " APOLLOS SETUP: Downloading Depth Anything V2 TFLite Model"
echo "=========================================================="
echo ""

# Ensure target directory exists
mkdir -p "${TARGET_DIR}"

if [ -f "${TARGET_PATH}" ]; then
  echo "✅ Model already exists at: ${TARGET_PATH}"
  echo "Skipping download."
  exit 0
fi

echo "⬇️ Downloading from Qualcomm AI Hub (~92 MB)..."

TMP_DIR=$(mktemp -d)
ZIP_PATH="${TMP_DIR}/model.zip"

if command -v wget &> /dev/null; then
  wget -q --show-progress -O "${ZIP_PATH}" "${MODEL_URL}"
elif command -v curl &> /dev/null; then
  curl -L --progress-bar -o "${ZIP_PATH}" "${MODEL_URL}"
else
  echo "❌ Error: Neither wget nor curl is installed."
  rm -rf "${TMP_DIR}"
  exit 1
fi

echo "📦 Extracting and moving model..."
unzip -q "${ZIP_PATH}" -d "${TMP_DIR}"

# The zip contains a folder depth_anything_v2-tflite-float with depth_anything_v2.tflite
EXTRACTED_FILE="${TMP_DIR}/depth_anything_v2-tflite-float/depth_anything_v2.tflite"

if [ -f "${EXTRACTED_FILE}" ]; then
  mv "${EXTRACTED_FILE}" "${TARGET_PATH}"
  echo ""
  echo "✅ Success! Model installed to: ${TARGET_PATH}"
  echo "DepthGuard Worker is now fully operational for offline Edge Hazard detection."
else
  echo "❌ Error: Expected model file not found in the downloaded archive."
  echo "Contents of extracted archive:"
  ls -R "${TMP_DIR}"
  exit 1
fi

rm -rf "${TMP_DIR}"
exit 0
