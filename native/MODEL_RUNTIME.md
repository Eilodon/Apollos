# Native Edge Model Runtime (YOLO + DA3)

Luồng object detection/depth hiện chạy theo kiến trúc model-runtime thật, không còn fallback từ `bbox area` hay objectness heuristic.

## Android (NNAPI/XNNPACK)

Đặt model vào `native/android/app/src/main/assets/models/` với một trong các tên:

- YOLO:
  - `yolov12.tflite`
  - `yolo_v12.tflite`
  - `yolo.tflite`
- Depth Anything:
  - `depth_anything_v3.tflite`
  - `da3.tflite`
  - `depth.tflite`

Pipeline: `YoloDa3NnapiPipeline` (`NNAPI -> XNNPACK fallback`).

## iOS (CoreML)

Bundle phải chứa model compiled `.mlmodelc` với một trong các tên:

- YOLO:
  - `YOLOv12Detector`
  - `YoloV12Detector`
  - `yolov12`
  - `yolo_v12`
  - `yolo`
- Depth Anything:
  - `DepthAnythingV3`
  - `DepthAnything3`
  - `depth_anything_v3`
  - `da3`
  - `depth`

Pipeline: `IOSYoloDa3CoreMLPipeline`.

## Runtime behavior

- Nếu thiếu model: native sẽ set `depth_objects_feed_missing`, không dùng fallback giả.
- Nếu model sẵn sàng: object detections + depth spatials được đẩy trực tiếp vào `apollos_detect_drop_ahead_objects`.

## Fast path (recommended)

1. Cài model đúng canonical name:

```bash
./scripts/install_native_models.sh \
  --android-yolo /abs/path/yolo.tflite \
  --android-depth /abs/path/depth.tflite \
  --ios-yolo /abs/path/YOLOv12Detector.mlmodelc \
  --ios-depth /abs/path/DepthAnythingV3.mlmodelc
```

`install_native_models.sh` tự:
- copy/rename Android models vào `assets/models`
- compile `.mlmodel`/`.mlpackage` sang `.mlmodelc` nếu cần (qua `xcrun`)
- ghi checksum manifest vào `native/model_manifest.sha256`

2. Build native apps:

```bash
./scripts/build_native_apps.sh
```

Script này chạy:
- `scripts/build_native_core.sh` (Rust FFI)
- Android `assembleDebug` (nếu có `native/android/gradlew`)
- iOS `xcodebuild` (nếu có `.xcodeproj/.xcworkspace`)
