#!/usr/bin/env bash

# Exit immediately if a command exits with a non-zero status
set -e

# Define directories
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${PROJECT_ROOT}/frontend/public/models"
MODEL_FILE="depth_anything_v2_small_fp16.tflite"
TARGET_PATH="${TARGET_DIR}/${MODEL_FILE}"

echo "=========================================================="
echo " APOLLOS SETUP: Depth Anything V2 TFLite Model"
echo "=========================================================="
echo ""

if [ -f "${TARGET_PATH}" ]; then
  echo "✅ Model already exists at: ${TARGET_PATH}"
  exit 0
fi

echo "❌ Model not found at: ${TARGET_PATH}"
echo ""
echo "To enable local Neural Depth Estimation (DepthGuard Worker), you must place"
echo "the TFLite model file at the path above."
echo ""
echo "INSTRUCTIONS:"
echo "1. Go to Hugging Face or another model repository."
echo "2. Find: 'Depth Anything V2 Small' in TFLite FP16 format."
echo "3. Download the .tflite file (~47 MB)."
echo "4. Rename it to 'depth_anything_v2_small_fp16.tflite'."
echo "5. Move it to: frontend/public/models/"
echo ""
echo "Note: If the file is missing, the worker will safely fall back to heuristic"
echo "Optical Flow depth processing."
exit 1
