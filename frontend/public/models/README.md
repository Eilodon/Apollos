Place the quantized TFLite depth model here for on-device drop detection.

Expected filename:
- `depth_anything_v2_small_fp16.tflite`

The depth worker will:
1. Load this file on first run.
2. Cache bytes in IndexedDB for offline reuse.
3. Fall back to heuristic depth if file is missing/unavailable.
