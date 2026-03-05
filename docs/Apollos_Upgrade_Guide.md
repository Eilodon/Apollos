# Apollos — Hướng Dẫn Nâng Cấp Toàn Diện
**Khai thác triệt để lợi thế · Vá mọi lỗ hổng · Đẩy đến cực hạn**

> Dựa trên: Apollos Technical Blueprint v3.0 · Gemini Live 2.5 Flash Native Audio GA · Research học thuật 2024–2026

---

## Cách đọc tài liệu này

Mỗi phần nâng cấp được gắn nhãn độ ưu tiên:
- 🔴 **CRITICAL** — chặn deploy nếu thiếu
- 🟡 **HIGH** — ảnh hưởng lớn đến UX & safety
- 🟢 **ENHANCE** — khai thác lợi thế cạnh tranh

Đọc theo thứ tự nếu đang build; nhảy đến section cụ thể nếu đang review.

---

## Tổng quan nhanh

| Hạng mục | Vấn đề cốt lõi | Hướng nâng cấp |
|---|---|---|
| 🔴 iOS / PWA | Camera fail, AudioWorklet, AmbientLightSensor không có | Android-first + fallback layer |
| 🔴 Phone carry | Người mù cầm gậy → không có tay cho phone | Necklace/clip mount + góc camera mới |
| 🔴 Spatial Memory | Yaw drift gây false match sau 5+ phút | GPS anchor (geohash + yaw) |
| 🟡 AEC tiếng Việt | 400ms hold clip thanh điệu, gây mất nghĩa | Adaptive hold per phoneme density |
| 🟡 Battery | 14–16%/hr quá optimistic | Adaptive FPS + thermal throttle detection |
| 🟡 Cane integration | Chưa có luồng cho người dùng gậy trắng | Smart cane Bluetooth haptic bridge |
| 🟢 Gemini upgrade | Model cũ deprecated 19/3/2026 | Migrate + khai thác Thinking Mode |
| 🟢 Contest edge | Các tính năng unique chưa được demo rõ | Demo script + judging criteria alignment |

---

# PHẦN 1 — CRITICAL FIXES
> Phải vá trước khi demo hoặc ship

---

## C1 🔴 iOS & PWA — Mảnh Đất Nổ Chưa Được Gỡ

**Vấn đề thực tế:** Blueprint chọn React PWA vì distribution dễ. Nhưng trên iOS Safari, camera trong PWA mode có bug dai dẳng — camera brief xuất hiện rồi tắt ngay (documented trong WebKit bug #215884). `AmbientLightSensor` API không tồn tại trên iOS. `AudioWorklet` trên iOS Safari có behavior khác Chrome. Background processing bị kill khi app mất focus.

> **Bằng chứng từ field:** Nghiên cứu teleguidance 2021 (Springer) cho người mù ngoài đường dùng phone đeo cổ — họ test trên Android với kết nối 4G router di động, KHÔNG dùng iOS PWA. Đây không phải ngẫu nhiên.

### Chiến lược xử lý: Android-First với iOS Graceful Degradation

| Tầng | Hành động cụ thể |
|---|---|
| **Platform detect** | Khi start: detect iOS Safari → hiện banner "Apollos hoạt động tốt nhất trên Android Chrome. Trên iOS, một số tính năng an toàn bị giới hạn." + nút Install on Android. KHÔNG im lặng degrade. |
| **Camera iOS fix** | Xóa `apple-mobile-web-app-capable` meta tag → force chạy trong Safari tab thay vì standalone PWA mode. Đổi lại: mất một số UI nhưng camera hoạt động ổn định hơn. |
| **Pocket Shield iOS** | `AmbientLightSensor` unavailable → fallback sang `proximity event` (`deviceproximity` API) hoặc manual "Pocket Mode" button trong UI. Log `sensor_unavailable` rõ ràng trong session metadata (đã có trong v3.0 — tốt). |
| **AudioWorklet iOS** | Test `VoiceGateProcessor` trên iOS Safari riêng. Nếu fail gracefully → fallback sang `ScriptProcessor` (deprecated nhưng vẫn hoạt động trên iOS). Add unit test cho cả hai path. |
| **Audio routing bug** | iOS 17+ bug: sau khi `getUserMedia()` camera, audio route sang earpiece thay vì speaker. Fix: gọi `audioContext.destination.channelCount = 2` và set AudioSession category sau khi camera stream bắt đầu. |

```typescript
// platformDetect.ts — thêm vào App.tsx init
export function getPlatformCapabilities() {
  const isIOS = /iPad|iPhone|iPod/.test(navigator.userAgent);
  const isSafari = /^((?!chrome|android).)*safari/i.test(navigator.userAgent);
  const hasAmbientLight = 'AmbientLightSensor' in window;
  const hasAudioWorklet = 'AudioWorklet' in window;
  return {
    isIOS,
    isSafari,
    pocketShieldAvailable: hasAmbientLight,
    voiceGateAvailable: hasAudioWorklet,
    recommendNative: isIOS,    // prompt Android install
    safetyGrade: isIOS ? 'REDUCED' : 'FULL'
  };
}
```

---

## C2 🔴 Vấn Đề Cầm Điện Thoại — Blind Spot Lớn Nhất

**Vấn đề cốt lõi:** Người mù hoàn toàn sử dụng gậy trắng tay trái để quét mặt đất. Nếu cầm điện thoại tay phải → không còn tay tự do. Nếu để túi → Pocket Shield block, góc camera sai hoàn toàn. Kinematic gate yêu cầu `cos(θ) > 0.82` (trong vòng 35° thẳng đứng) — không khớp với cách người ta thực sự mang phone ngoài đường.

> **Từ nghiên cứu học thuật (NavWear, 2025; Teleguidance, 2021):** Giải pháp tốt nhất đã được validate là **phone đeo cổ (necklace mount)**, camera hướng ra trước. Kết hợp với white cane → ít va chạm hơn, người dùng cảm thấy an toàn hơn và tự tin hơn. Apollos cần hỗ trợ mount mode này explicitly.

### Nâng cấp: Hỗ trợ 3 Carry Modes

| Carry Mode | Đặc điểm | Kinematic thay đổi | Ưu tiên |
|---|---|---|---|
| Hand-Held | Cầm tay, giơ lên, 1 tay bận | `cos(θ) > 0.82` như hiện tại | Low (không khuyến nghị) |
| **Necklace Mount ⭐** | Đeo cổ, camera ngang tầm ngực-cổ, 2 tay tự do | `cos(θ) > 0.65`, pitch offset +15° | **HIGH — mặc định** |
| Chest Clip | Kẹp áo, ổn định hơn necklace | `cos(θ) > 0.72`, gyro threshold rộng hơn | MEDIUM |
| Pocket (passive) | Chỉ edge detection, cloud suspend | Pocket Shield = block all cloud | Fallback only |

```typescript
// useCarryMode.ts — NEW
export type CarryMode = 'hand_held' | 'necklace' | 'chest_clip' | 'pocket';

interface CarryModeProfile {
  cosTiltThreshold: number;    // Kinematic gate threshold
  pitchOffset: number;          // Compensation for mount angle (degrees)
  gyroThreshold: number;        // Acceptable angular velocity
  cloudEnabled: boolean;
}

const CARRY_PROFILES: Record<CarryMode, CarryModeProfile> = {
  hand_held:  { cosTiltThreshold: 0.82, pitchOffset: 0,   gyroThreshold: 45, cloudEnabled: true  },
  necklace:   { cosTiltThreshold: 0.65, pitchOffset: 15,  gyroThreshold: 55, cloudEnabled: true  },
  chest_clip: { cosTiltThreshold: 0.72, pitchOffset: 8,   gyroThreshold: 50, cloudEnabled: true  },
  pocket:     { cosTiltThreshold: 0.0,  pitchOffset: 0,   gyroThreshold: 999, cloudEnabled: false },
};

// Thêm vào Onboarding: ARIA hỏi 'Bạn đang đeo điện thoại ở đâu?'
// → Lưu vào session profile, inject vào kinematicGating.ts
```

> **Tích hợp Onboarding (Section 9 update):** Thêm bước 0 vào Trust Protocol: trước cả PRESENCE PROOF, hỏi carry mode. "Bạn đang cầm điện thoại, hay đeo vào cổ/áo?" → set profile ngay. Đây là thông tin ảnh hưởng đến toàn bộ kinematic pipeline.

---

## C3 🔴 Spatial Memory — Yaw Drift & False Match

**Bug cụ thể:** `SpatialMemoryStore` match hazard bằng điều kiện `|yaw_stored - yaw_current| < 30°`. Sau 5–10 phút đi bộ, gyro drift tích lũy 10–20°. Nếu user đi một vòng và quay đầu về hướng cũ, hazard cũ ở vị trí khác có thể match sai. Tệ hơn: hazard ở góc 85° và hazard ở góc 115° đều match với `current_yaw = 100°`, dù là hai vật thể hoàn toàn khác nhau.

```python
# spatial_memory.py — PATCHED VERSION

from dataclasses import dataclass, field
import math

@dataclass
class HazardMemory:
    hazard_type: str
    yaw_at_detection: float
    geohash: str            # NEW: GPS anchor (precision 7, ~150m²)
    frame_sequence: int
    confirmed_count: int = 1
    last_seen_frame: int = 0

class SpatialMemoryStore:
    MAX_MEMORIES = 20
    YAW_MATCH_THRESHOLD = 30.0
    GEOHASH_MATCH_PREFIX = 6   # NEW: precision 6 = ~1.2km² — same block

    def add_hazard(self, hazard_type, yaw, frame_seq, geohash: str):
        for m in self.memories:
            # NEW: must match BOTH geohash prefix AND yaw
            geo_match = m.geohash[:self.GEOHASH_MATCH_PREFIX] == geohash[:self.GEOHASH_MATCH_PREFIX]
            yaw_match = abs(m.yaw_at_detection - yaw) < self.YAW_MATCH_THRESHOLD
            if m.hazard_type == hazard_type and geo_match and yaw_match:
                m.confirmed_count += 1
                m.last_seen_frame = frame_seq
                return
        # New memory
        if len(self.memories) >= self.MAX_MEMORIES:
            self.memories.pop(0)
        self.memories.append(HazardMemory(hazard_type, yaw, geohash, frame_seq))

    def get_relevant_context(self, current_yaw: float, current_geohash: str) -> str:
        relevant = [
            m for m in self.memories
            if m.geohash[:self.GEOHASH_MATCH_PREFIX] == current_geohash[:self.GEOHASH_MATCH_PREFIX]
            and abs(m.yaw_at_detection - current_yaw) < self.YAW_MATCH_THRESHOLD
        ]
        # ... rest unchanged
```

**Cũng cần:** Truyền `current_lat` và `current_lng` từ payload → tính geohash server-side trong `live_bridge.py`. Add `python-geohash` vào `requirements.txt`. Add unit test: user walk 200m rồi quay lại → memory KHÔNG match khi geohash khác.

---

# PHẦN 2 — HIGH IMPACT IMPROVEMENTS
> Ảnh hưởng trực tiếp đến UX & safety ngoài đường

---

## H1 🟡 AEC Tiếng Việt — Thanh Điệu Bị Cắt

Hold time 400ms sau khi voice kết thúc là trade-off đúng đắn để chặn background noise. Nhưng tiếng Việt có 6 thanh điệu — nhiều từ đơn tiết ngắn như "dừng" (stop), "nguy" (danger), "trái" (left), "phải" (right). Hold 400ms quá dài = clip syllable đầu câu tiếp theo.

```typescript
// VoiceGateProcessor — PATCHED
// Thay vì fixed 400ms, dùng adaptive hold dựa trên energy pattern

class VoiceGateProcessor extends AudioWorkletProcessor {
  private readonly GATE_HOLD_FRAMES_NOISY = 8;  // 400ms — môi trường ồn
  private readonly GATE_HOLD_FRAMES_QUIET = 3;  // 150ms — yên tĩnh
  private ambientRMS = 0;

  process(inputs): boolean {
    const rms = computeRMS(inputs[0][0]);
    // Track ambient noise level (slow-moving average)
    this.ambientRMS = this.ambientRMS * 0.99 + rms * 0.01;

    // Adaptive hold: noise > threshold → long hold, quiet → short hold
    const isNoisyEnv = this.ambientRMS > 0.05;
    const holdFrames = isNoisyEnv
      ? this.GATE_HOLD_FRAMES_NOISY   // 400ms for street noise
      : this.GATE_HOLD_FRAMES_QUIET;  // 150ms for indoor

    const isVoice = rms > this.VOICE_THRESHOLD
      || this.rmsHistory.slice(-holdFrames).some(r => r > this.VOICE_THRESHOLD);

    if (isVoice) this.port.postMessage({ type: 'audio_chunk', data: inputs[0][0] });
    return true;
  }
}

// Truyền ambient noise level lên App.tsx để ARIA biết môi trường
// → Inject [ENV: noisy_street] vào system prompt → ARIA nói chậm hơn, rõ hơn
```

---

## H2 🟡 Battery — 14–16%/hr Cần Được Validate & Hardened

Con số 14–16%/hr từ blueprint có thể optimistic. Camera liên tục + Gemini Live WebSocket BIDI + GPS + AudioWorklet trên Android thực tế thường **25–35%/hr** với các app tương tự. Người dùng đi bộ 2–3 tiếng cần >40% battery budget.

| Cơ chế | Mô tả |
|---|---|
| **Thermal Throttle Detection** | Đọc `navigator.getBattery()` để theo dõi `dischargeRate`. Nếu `dischargeRate > 0.25%/phút` (cao bất thường) → giảm cloud FPS từ 1FPS xuống 0.5FPS và notify user. |
| **Adaptive FPS bổ sung** | Thêm: `in_building` (GPS không thay đổi, ánh sáng ổn định) → reduce to 0.3 FPS. |
| **Audio Output Power** | Speaker mode tốn 2–3x power so với earphone. Khi detect speaker → tăng ping volume +6dB (đã có) nhưng GIẢM ping frequency. |
| **Battery Warning** | Khi battery < 20% → ARIA thông báo voice 1 lần: "Pin còn 20%, tôi sẽ chuyển sang chế độ tiết kiệm năng lượng." → Auto switch sang QUIET mode + giảm cloud FPS. |

---

## H3 🟡 Tích Hợp Gậy Trắng — Lỗ Hổng UX Lớn Nhất

Research 2025 (NavWear, Springer) chứng minh: kết hợp ETA + white cane → ít va chạm hơn, cảm giác an toàn cao hơn. Apollos hiện tại không có luồng nào cho người dùng gậy trắng — đây là phần lớn target users.

> **Insight từ research:** Smart cane haptic có 2 actuator (left/right) cho directional guidance được validate tốt hơn 1 actuator với variable pattern. Apollos đã có HRTF sonar ping — đây là complement hoàn hảo, không phải replacement.

### Giải pháp: Bluetooth Haptic Bridge Protocol

Thay vì build smart cane riêng, Apollos communicate với smart cane third-party (WeWalk, Augmented Cane) qua Bluetooth LE.

```typescript
// useSmartCane.ts — NEW optional module

interface SmartCaneAdapter {
  connect(): Promise<void>;
  sendDirectional(direction: 'left' | 'right' | 'stop', intensity: number): void;
  sendHazardPattern(urgency: 'soft' | 'hard'): void;
}

// Khi HARD_STOP fires — onHardStop() update:
export function onHardStop(event: HardStopEvent) {
  // Existing: HRTF siren + haptic phone vibration
  SpatialAudioEngine.fireHardStop(event.position_x, event.distance);
  haptics.vibrateHardStop();

  // NEW: Smart cane bridge (if connected)
  if (smartCaneAdapter.isConnected()) {
    smartCaneAdapter.sendHazardPattern('hard');  // 3 rapid pulses
  }

  // NEW: Directional cane hint
  if (event.position_x > 0.3) smartCaneAdapter.sendDirectional('right', 0.8);
  if (event.position_x < -0.3) smartCaneAdapter.sendDirectional('left', 0.8);
}

// Onboarding bổ sung: 'Bạn có dùng gậy thông minh không?'
// → Nếu có: scan BLE devices, pair, test haptic ping
```

> **Nếu không có smart cane:** Phone vibration đã là haptic channel quan trọng. Với necklace mount, vibration từ phone truyền qua dây đeo xuống ngực — user cảm nhận được kể cả trong môi trường ồn.

---

# PHẦN 3 — COMPETITIVE ENHANCEMENTS
> Khai thác lợi thế cạnh tranh đến cực hạn

---

## E1 🟢 Gemini Model Upgrade — Deadline 19/3/2026

> ⚠️ **URGENT:** `gemini-live-2.5-flash-preview-native-audio-09-2025` bị deprecated và xóa vào 19/3/2026. Blueprint v3.0 đã dùng `gemini-live-2.5-flash-native-audio` (correct) nhưng fallback chain vẫn có model cũ.

| Thay đổi | Chi tiết |
|---|---|
| **Xóa model cũ khỏi fallback** | `GEMINI_MODEL_FALLBACKS`: xóa `gemini-2.5-flash-native-audio-preview-12-2025` và `preview-09-2025`. Chỉ giữ `gemini-live-2.5-flash-native-audio`. |
| **Khai thác Thinking Mode** | Native audio 2.5 có "thinking" mode cho complex queries. Dùng cho: `identify_location()` khi ambiguous, affective dialog khi user stressed. KHÔNG dùng cho HARD_STOP path — latency không chấp nhận được. |
| **30 HD Voices** | Model mới có 30 HD voices trong 24 ngôn ngữ. Tiếng Việt native — test voice `Kore` với các cảnh báo ngắn có dấu. |
| **Proactive Audio validate** | Validate ARIA không respond với câu chuyện trên đường, tiếng xe máy, âm nhạc từ cửa hàng. Test với audio samples đường phố Việt Nam. |
| **Language auto-detect** | Native audio models tự detect ngôn ngữ — verify ARIA luôn respond tiếng Việt khi nhận tiếng Việt từ user. |

---

## E2 🟢 Khai Thác Triệt Để Dual-Brain — Chưa Ai Làm Được

Đây là differentiator lớn nhất của Apollos. Be My Eyes, Seeing AI, Google Lookout đều là single-path (cloud only). Apollos là app đầu tiên có survival reflex chạy offline <16ms.

| Khả năng hiện tại | Nâng cấp đề xuất |
|---|---|
| Layer 0: chỉ detect radial expansion | **Thêm floor drop detection** — khi bottom quadrant suddenly drops, fire `EDGE_DROP_HAZARD` ngay. Bậc thang, hố ga, lề đường xuống — cực kỳ nguy hiểm với người mù. |
| Layer 0: 64×64 resolution | Giữ nguyên. **Thêm dark frame detection** (avgPixel < 20) → "Tôi không thể nhìn thấy, vui lòng hướng camera ra trước." |
| Layer 1: Risk Score Matrix | **Thêm HeadShake detection** — gyro pattern lắc đầu liên tục → user có thể confused/lost → tự động switch sang NAVIGATION mode + query location. |
| Survival Reflex false positive | **Thêm test cases VN:** flickering neon sign, xe máy chạy qua nhanh, mưa trên camera lens. |

```typescript
// survivalReflex.worker.ts — Thêm floor drop detection

function detectFloorDrop(prev: ImageData, curr: ImageData): boolean {
  // So sánh bottom 25% của frame (rows 48–64)
  const bottomDiff = computeBottomQuarterDiff(prev, curr);
  const topDiff = computeTopQuarterDiff(prev, curr);

  // Top stable, bottom changes drastically = floor drop
  return bottomDiff > 60 && topDiff < 20;
}

onmessage = (e) => {
  const { prev, curr } = e.data;

  // NEW: floor drop check (fires before radial check)
  if (detectFloorDrop(prev, curr)) {
    postMessage({ type: 'CRITICAL_EDGE_HAZARD', positionX: 0,
                  distance: 'very_close', subtype: 'floor_drop' });
    return;
  }

  // ... existing radial detection unchanged
};
```

---

## E3 🟢 Silence-as-Signal — Khai Thác Triệt Để Hơn Nữa

Blueprint đã có Rule #8: "Path is clear → NEVER say this. Silence = safe." Đây là design decision xuất sắc. Có thể khai thác sâu hơn:

| Hiện tại | Đề xuất nâng cấp |
|---|---|
| Silence = safe (passive) | **Heartbeat ping:** mỗi 15 giây khi không có gì xảy ra → emit ping rất nhẹ (220Hz, 50ms, volume thấp). User biết app vẫn chạy, không chết. Không phải voice — chỉ tín hiệu "tôi vẫn đây". |
| `destination_near` ping khi đến nơi | **Fire sớm hơn** — khi còn 30m thay vì chỉ khi đến nơi. User cần prepare, giảm tốc, tìm entrance. |
| Sonar 5 semantic types | **Thêm `surface_change` ping** — khi camera detect texture thay đổi (asphalt → gạch → đất) → soft ping báo hiệu ground type thay đổi. |
| HRTF với pan cố định | **Volume envelope theo khoảng cách:** `very_close = 100%`, `nearby = 75%`, `moderate = 50%`. Trực quan như bat echolocation. |

---

## E4 🟢 Crowdsourced Hazard Map — Long-term Moat

Schema v3.0 đã tốt. Đây là tính năng duy nhất trong mảng navigation assistive tech mà không ai đang làm — và nó có network effect thực sự.

| Cải tiến | Chi tiết kỹ thuật |
|---|---|
| **Time decay** | Hazard tự giảm `confirmed_count` nếu không được confirm lại. Mỗi 7 ngày không có confirm → count -1. count = 0 → xóa. Xe đậu tạm thời không nên persist mãi mãi. |
| **Time pattern intelligence** | Khi ARIA announce crowd hazard, kèm time context: "Vỉa hè đoạn này hay bị xe đậu vào 7–9h sáng — hiện là 8h, cẩn thận hơn." (data đã có, chỉ cần inject vào prompt). |
| **Hazard taxonomy VN** | `parked_motorbike`, `street_vendor`, `broken_pavement`, `open_drain`, `construction_barrier`, `overhead_obstacle`. Train ARIA recognise và log đúng types. |
| **Anonymous contribution** | Khi session end: "Bạn đã đóng góp X hazard vào bản đồ cộng đồng hôm nay." Không reveal location hay user ID. Tạo sense of contribution mà không compromise privacy. |

---

## E5 🟢 Tối Ưu Hóa Cho Thị Trường Việt Nam

Đây là moat thứ hai — và là lý do dự án có khả năng win ở Việt Nam trong khi tất cả đối thủ quốc tế đang nhắm châu Mỹ và châu Âu.

| Hạng mục | Nâng cấp |
|---|---|
| **System prompt VN context** | Inject explicitly: "Bạn đang ở Việt Nam. Vỉa hè thường bị xe đậu. Xe máy có thể ra từ hẻm bất ngờ. Biển hiệu thấp phổ biến. Mưa làm mặt đường trơn." → ARIA bias warning theo rủi ro thực tế VN. |
| **Địa điểm ưu tiên VN** | Mở rộng `PRIORITY_TYPES`: thêm `bus_stop_xe_buyt`, `xe_om_stand`, `atm`, `cho_truyen_thong`. Tên tiếng Việt trong `description_vi`. |
| **Phong cách giọng nói VN** | Dùng từ địa phương — "hẻm" không phải "ngõ nhỏ", "vỉa hè" không phải "lề đường", "xe hai bánh" thay vì "motorcycle" khi phát hiện xe máy. |
| **Offline map VN** | Cache OSM tiles cho khu vực user thường đi. Khi GPS available nhưng internet slow → fallback sang offline map. Quan trọng cho khu vực underground (tầng hầm, bệnh viện). |

---

# PHẦN 4 — GEMINI LIVE AGENT CHALLENGE: CHIẾN LƯỢC WIN

Contest category là "Live Agents" — judges đang tìm kiếm agent thực sự LIVE, không phải chatbot có camera. Apollos đã có architecture đúng. Cần present đúng cách.

### Những gì judges sẽ đánh giá cao nhất

| Tiêu chí | Apollos hiện có |
|---|---|
| Genuinely live interaction | ✅ WebSocket BIDI, streaming audio, real-time camera — không phải request-response |
| Khai thác Gemini-specific features | ✅ Proactive audio, affective dialog, session resumption, context compression, ADK tools, Maps grounding |
| Real-world impact | ✅ Assistive technology cho người mù — impact rõ ràng, measurable |
| Technical depth | ✅ Dual-Brain, Temporal Optical Flow, Kinematic Risk Score, Spatial Memory |
| Novel approach | ✅ Silence-as-Signal, Semantic Ping Encoding, Crowdsourced Hazard Map |
| Code quality | 🟡 Cần: `hardening_pass.py` pass 100%, benchmark results documented |

### Demo Script — 90 giây show everything

> Yêu cầu: Demo phải show Dual-Brain hoạt động **song song**, không phải sequential.

1. **[0–15s] Trust Protocol:** ARIA mô tả ngay 1 object cụ thể trong frame → prove camera is live. Giơ tay → HARD_STOP fires với sonar ping. Judges nghe thấy ping, thấy HazardCompass UI.

2. **[15–35s] Dual-Brain song song:** Di chuyển camera nhanh về phía obstacle → Layer 0 fire (<16ms, NO network). Đồng thời Gemini mô tả scene. Hiện trên screen: `Edge: 8ms | Cloud: 612ms` side by side. Show rõ hai path.

3. **[35–55s] Spatial Memory:** Detect hazard, move away, rotate, quay lại hướng cũ → ARIA pre-warns BEFORE camera sees it again. "Cẩn thận — vật cản phía trước, đã xác nhận từ trước." Judges thấy AI có memory.

4. **[55–75s] Location Intelligence:** Đứng yên 5 giây → ARIA announce: "Bạn đang đứng gần bệnh viện Bạch Mai, cửa chính 20m về phía bắc, mở 24/7." Maps grounding in action.

5. **[75–90s] Crowdsourced Map:** Sau khi hazard log → show Firestore `hazard_map` entry live update. "Bản đồ cộng đồng đã ghi nhận vật cản này." Network effect moat được demonstrate.

### Câu hỏi Judges hay hỏi — Chuẩn bị sẵn

| Câu hỏi | Câu trả lời |
|---|---|
| "Tại sao không dùng native app?" | PWA cho phép zero-install, cross-device. Người mù không thể dễ dàng navigate App Store. Instant access từ URL là accessibility decision. Android-native là bước tiếp theo sau validation. |
| "Latency 600ms có đủ safe không?" | Layer 0 Edge (<16ms) handle survival reflexes không cần network. 600ms cloud chỉ cho semantic understanding — không phải life-or-death path. Chúng tôi không chờ cloud để stop user. |
| "Battery thực tế như thế nào?" | Chúng tôi dự kiến 20–25%/hr trong field tests (honest). Adaptive power management target giảm xuống 15–18%. Đủ cho 2h đi bộ với phone đầy pin. |
| "Người mù thực sự dùng chưa?" | User testing là bước tiếp theo ngay sau contest. Onboarding Trust Protocol và Carry Mode selection được thiết kế dựa trên nghiên cứu học thuật (NavWear 2025, Teleguidance 2021). |

---

# PHẦN 5 — IMPLEMENTATION ROADMAP

### Wave 1 — Pre-Contest (Tuần 1–2)
*Tất cả items 🔴 CRITICAL + Gemini model migration*

- [ ] Migrate khỏi deprecated Gemini model. Kiểm tra fallback chain.
- [ ] Add `platformDetect.ts`, hiện iOS warning banner, fix camera iOS PWA.
- [ ] Implement `useCarryMode.ts`, update kinematic thresholds, thêm carry mode vào Onboarding.
- [ ] Patch `SpatialMemoryStore` với geohash anchor. Add `python-geohash`. Viết unit tests.
- [ ] Chạy `hardening_pass.py` đến khi pass 100%.
- [ ] Chạy `benchmark_hard_stop` với HARD_STOP < 100ms target confirmed.

### Wave 2 — Polish (Tuần 3)

- [ ] H1: Adaptive AEC hold time cho tiếng Việt.
- [ ] H2: Battery monitoring + ARIA battery warning voice.
- [ ] E1: Validate `proactive_audio` với Vietnamese street audio samples.
- [ ] E2: Thêm floor drop detection vào `survivalReflex.worker.ts`.
- [ ] E2: Thêm regression tests: flickering neon, fast motorcycle pass-by.
- [ ] E3: Heartbeat ping 15s, `destination_near` sớm hơn.

### Wave 3 — Competitive Edge (Trước submission)

- [x] E4: Test Crowdsourced Hazard Map end-to-end với dummy data.
- [x] E5: System prompt Vietnamese context injection.
- [x] E5: Hazard taxonomy VN (xe máy, hẻm, vỉa hè).
- [x] H3: Bluetooth smart cane stub (optional — nếu có hardware).
- [ ] Rehearse demo script 10 lần, time mỗi segment.
- [x] Document benchmark results: edge latency, cloud TTFT, battery measurements thực tế.

---

## Tổng kết

Apollos v3.0 đã có kiến trúc tốt hơn mọi app hiện có trong mảng này. Dual-Brain, Silence-as-Signal, Crowdsourced Hazard Map, 90s Trust Protocol — đây là product thinking level cao.

3 thứ cần fix trước khi demo: iOS camera fail, người mù không có tay cầm phone, và spatial memory drift sau 10 phút. Tất cả đều có solution cụ thể trong tài liệu này.

**Điểm mạnh lớn nhất** là thứ không ai khác có: Edge <16ms chạy offline + Cloud 600ms semantic, song song, không trade-off. Đây là moat kỹ thuật thực sự. Demo nó thật rõ ràng.

> *"Edge protects lives. Cloud decodes the world. Memory makes it home."*
> Câu tagline này tóm tắt chính xác lý do tại sao architecture này win.

---

*Apollos Upgrade Guide · March 2026 · Based on Blueprint v3.0*
