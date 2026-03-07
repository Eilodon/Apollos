#!/usr/bin/env bash

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${PROJECT_ROOT}/assets/edge_models"

mkdir -p "$TARGET_DIR"

# URLs can be overridden via environment variables if the models are hosted elsewhere.
# These URLs point to the standard release assets for the edge-optimized variants.
YOLO_TFLITE_URL="${YOLO_TFLITE_URL:-https://huggingface.co/datasets/apollos-project/edge-models/resolve/main/yolov12n_float16.tflite}"
YOLO_COREML_URL="${YOLO_COREML_URL:-https://huggingface.co/datasets/apollos-project/edge-models/resolve/main/yolov12n.mlpackage.zip}"

DA3_TFLITE_URL="${DA3_TFLITE_URL:-https://huggingface.co/datasets/apollos-project/edge-models/resolve/main/depth_anything_v3_vits.tflite}"
DA3_COREML_URL="${DA3_COREML_URL:-https://huggingface.co/datasets/apollos-project/edge-models/resolve/main/depth_anything_v3_vits.mlpackage.zip}"


download_file() {
    local url=$1
    local dest=$2
    echo "Downloading ${dest}..."
    if command -v curl >/dev/null 2>&1; then
        curl -L --fail --progress-bar "$url" -o "$dest" || echo "Warning: Could not download $dest (Check URL or network)"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$dest" "$url" || echo "Warning: Could not download $dest (Check URL or network)"
    else
        echo "Error: curl or wget is required to download models."
        exit 1
    fi
}

echo "=== Downloading YOLOv12 Models ==="
download_file "$YOLO_TFLITE_URL" "$TARGET_DIR/yolov12_edge.tflite"
download_file "$YOLO_COREML_URL" "$TARGET_DIR/yolov12_edge.mlpackage.zip"

echo "=== Downloading Depth Anything V3 Models ==="
download_file "$DA3_TFLITE_URL" "$TARGET_DIR/da3_edge.tflite"
download_file "$DA3_COREML_URL" "$TARGET_DIR/da3_edge.mlpackage.zip"

echo ""
echo "Models downloaded to $TARGET_DIR"
echo "Next, run: ./scripts/install_native_models.sh $TARGET_DIR/yolov12_edge.tflite $TARGET_DIR/yolov12_edge.mlpackage.zip $TARGET_DIR/da3_edge.tflite $TARGET_DIR/da3_edge.mlpackage.zip"
