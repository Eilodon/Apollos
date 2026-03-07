# Apollos — Cognitive Infrastructure (Eidolon-V)

Apollos là **Lõi Điều Hướng Sinh Tồn (Safety-Critical Navigation Stack)** viết bằng Rust, đóng vai trò là Hạ tầng Nhận thức (Cognitive Infrastructure - Eidolon-V) cho người khiếm thị. Dự án khước từ hoàn toàn các thiết kế UI/UX truyền thống để vươn tới một Lõi Sinh Tồn ngầm (Subconscious Core), kết hợp Thị giác máy tính Attention-centric, Không gian 3D, và khả năng suy luận trên thiết bị biên.

Mục tiêu Tối thượng (Prime Directive): **Zero Hallucinations** & **Safe-by-Design**. Sinh mạng luôn luôn đè bẹp Kiến thức.

---

## 🏗️ Kiến trúc Hệ thống 3 Tầng (Tri-Layer Architecture)

Sự phân lập tuyệt đối để đảm bảo hệ thống không bao giờ sụp đổ (Crash-proof).

### 1. Tầng 1: Lõi Sinh Tồn (apollos-core) - Tốc độ Ánh sáng
Computes physics, optical flow, and depth on the edge. Exposes a zero-copy C ABI for mobile FFI.
- **Sensor Fusion bằng Toán Học Nguyên Thủy:** Cơ chế **Error-State Kalman Filter (ESKF)** (`sensor_fusion.rs`) code thủ công bằng `nalgebra` với độ trễ siêu thấp (<2ms). Dung hợp liên tục liên tục tín hiệu IMU (1000Hz) và Vision/Depth mà không cần Garbage Collection hay Deep Learning nặng nề.
- **Kinematic Gating (`kinematic_gate.rs`):** Đánh giá động học (Pitch, Velocity, Accelerometer) trước khi khởi chạy các phép toán đắt đỏ, tự động phân tích Rủi ro rớt, va chạm, tạo nên hệ thống phản xạ không điều kiện.
- **Phương trình Nguy hiểm ($H$):** Tích hợp cứng trong `safety_policy.rs`. Biến đổi trực tiếp các tham số Động học, Khoảng cách (depth), và Độ tin cậy (confidence) thành xung lực Xúc giác (Haptic Heartbeat) và Âm thanh không gian tuyến tính (Spatial Audio Pitch Hz).
- **Depth Engine:** Dung hợp dữ liệu Bounding Box (YOLO) và Depth Spatials (Depth Anything) để định mức và cảnh báo khoảng cách tuyến tính.

### 2. Tầng 2: Lõi Nhận Thức (Edge VLM - L2Edge) - Phân lập An toàn
- **Sự Đứt gãy Logic (Cognition Layer):** Tầng 2 hoạt động độc lập để đảm bảo an toàn tuyệt đối khi Offline (`agent.rs`).
- **Semantic Grounding Threshold:** Khi tín hiệu từ môi trường ở Tầng 1 (`edge_semantic_cues`) vượt qua ngưỡng cho phép, hệ thống Lõi Nhận Thức cắt hẳn luồng chuyển dữ liệu lên Cloud AI (Gemini Live) và ưu tiên hiển thị Cảnh báo Biên (Edge Cues - ví dụ "ApproachingObject").

### 3. Tầng 3: Lõi Ý Thức (apollos-server) - Trùm Phối hợp
Axum-based WebSocket backend handling Gemini Live API duplex streaming, Tool Calls, and Thermodynamic State regulation.
- **Luồng Cắt Ngang Kép (Dual-Interrupt Engine):** Quản lý kết nối WebRTC/Websocket với Gemini Live thông qua `gemini_bridge.rs`. Bất cứ khi nào Phương trình Nguy hiểm tại thiết bị biên ($H$) vượt ngưỡng **HARD_STOP_THRESHOLD**, Apollos lập tức nhồi lện `LiveControl::Interrupt` thẳng xuống luồng Cloud, Đập nát bất kỳ câu văn thuyết minh/tả cảnh dài dòng nào của LLM.
- **Human Fallback (Sự Cứu Rỗi Cuối Cùng):** Khi độ không chắc chắn (Uncertainty) quá cao hoặc cảm biến bị mù (Sensor Health < 35%), Session sẽ được tự động chuyển cho lưới tình nguyện viên/người thân qua Twilio WebRTC.

---

## 📂 Trật tự Workspace 

```text
crates/
  apollos-core     # FFI (C ABI), Physics, ESKF Fusion, Kinematic Gating, Depth Engine
  apollos-server   # Axum, Gemini Bridge (Live WS), Agent Orchestrator, Session, Fallback
  apollos-proto    # Type-safe Protobuf definitions (hủy hoại Schema Drift)
  apollos-bench    # Benchmarking
native/
  android/         # Kotlin Shell + C++ JNI Config (Haptic/Audio Engine)
  ios/             # Swift UI + C Header Bridge
scripts/
  build_native_core.sh
```

---

## 🚀 Hướng dẫn Kích hoạt (Running the Engine)

### Khởi động Lõi Phối Hợp (Server)
```bash
# Production server boot
cargo run -p apollos-server --release

# Health check
curl http://127.0.0.1:8000/healthz
```

### Rèn Đúc Lõi Sinh Tồn (Native Edge Core)
```bash
./scripts/build_native_core.sh
```

---

## 🔐 Môi trường & Chân lý (Crucial Environment Variables)

**Gemini Live Orchestrator:**
- `ENABLE_GEMINI_LIVE=1` (Bật/Tắt Luồng Đàm Thoại 2 Chiều)
- `GEMINI_API_KEY` (Sự Cấp Phép Bắt Buộc)
- `GEMINI_MODEL` (Mặc định: `gemini-2.5-flash` hoặc các biến thể Pro)

**Trauma Registry (Firestore) - Bộ Nhớ Ký Ức:**
- `USE_FIRESTORE=1`
- `GOOGLE_CLOUD_PROJECT`
- `FIRESTORE_BEARER_TOKEN` (hoặc Cloud Run Default Service Account)

**Human Support Escapement (Twilio) - Kết nối Sinh Tử:**
- `TWILIO_ACCOUNT_SID`
- `TWILIO_VIDEO_API_KEY_SID`
- `TWILIO_VIDEO_API_KEY_SECRET`

---
*Mọi dòng code là một lời thề bảo vệ sinh mạng người dùng.*
