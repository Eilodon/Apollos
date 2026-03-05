# Apollos — Upgrade Plan v1.0
**Kế hoạch nâng cấp chuyên sâu — từ 24h MVP đến production-grade**

---

## Tổng quan triết lý nâng cấp

Apollos hiện tại đã có kiến trúc đúng về mặt triết học: Edge protects lives, Cloud decodes the world. Mọi upgrade dưới đây đều follow cùng một principle — **không thay đổi triết lý, chỉ nâng chất lượng thực thi từng layer.**

Ràng buộc xuyên suốt:
- Không phá vỡ dual-brain separation
- Không tăng latency của Layer 0 (phải giữ <16ms)
- Không thêm dependency bắt buộc network vào Edge path
- Mọi upgrade phải degrade gracefully khi feature không available (iOS, older Android)

---

## UPGRADE 1 — Temporal Smoothing cho Optical Flow

### Vấn đề
`avgDiff > 50` trên 64×64 grayscale là threshold duy nhất để fire `CRITICAL_EDGE_HAZARD`. Không có temporal context. Một frame ánh sáng thay đổi đột ngột (bước ra nắng, đèn xe pha, bóng cây) đủ để trigger false positive.

### Điều kiện kỹ thuật
- Chạy hoàn toàn trong `survivalReflex.worker.ts` — không ảnh hưởng main thread
- Không tăng memory footprint đáng kể (chỉ cần lưu thêm 1 frame trước)
- Latency vẫn phải <16ms

### Đề xuất cụ thể

**Bước 1 — Ring buffer 3 frames:**
```typescript
// survivalReflex.worker.ts
const RING_SIZE = 3;
const diffHistory: number[] = [];

// Thay vì:
if (avgDiff > 50) postMessage(CRITICAL_EDGE_HAZARD)

// Dùng:
diffHistory.push(avgDiff);
if (diffHistory.length > RING_SIZE) diffHistory.shift();

const sustainedThreat = diffHistory.length === RING_SIZE
  && diffHistory.every(d => d > 40);  // threshold thấp hơn nhưng sustained

if (sustainedThreat) postMessage(CRITICAL_EDGE_HAZARD)
```

**Bước 2 — Expansion vector thay vì scalar:**
Thay vì chỉ đo avgDiff toàn frame, chia 64×64 thành 4 quadrant (32×32 mỗi ô). Object approaching thật sự sẽ expand từ tâm ra — tất cả 4 quadrant tăng. Ánh sáng thay đổi sẽ tăng uniform hoặc một phía.

```typescript
function computeOpticalExpansion(prev: ImageData, curr: ImageData): {
  centerDiff: number,
  expansionPattern: 'radial' | 'uniform' | 'directional' | 'none'
}
```

Chỉ fire HARD_STOP khi `expansionPattern === 'radial'` — đây là signature thật sự của vật thể tiến đến.

### Ràng buộc
- Ring buffer 3 frames @ 100ms = 300ms delay tối đa để confirm hazard — acceptable vì TTC threshold là 1.5s
- Expansion pattern check tăng compute ~2ms — vẫn trong budget <16ms
- Fallback: nếu không đủ frame history → dùng single-frame logic cũ (safer than nothing)

### Kết quả kỳ vọng
- False positive giảm ~80% từ light changes
- True positive rate giữ nguyên với real approaching objects (expansion pattern rõ ràng)

---

## UPGRADE 2 — Silence-as-Signal Audio Hierarchy

### Vấn đề
ARIA hiện tại với `proactive_audio: true` có thể liên tục mô tả môi trường khi di chuyển — "sidewalk clear ahead, parked motorcycle to the right, shop entrance on left..." Sau 5–10 phút, user bị audio fatigue và trust giảm. Research confirm: cognitive overload từ continuous voice narration là killer cho real-world adoption.

### Triết lý thiết kế mới
**Sonar nói chuyện thay voice.** Voice chỉ dùng khi ngôn ngữ thực sự cần thiết.

```
TIER 1 — EXISTENTIAL (luôn fire):
  HARD_STOP: Sonar ping + haptic + voice ngắn <8 words

TIER 2 — ACTIONABLE (fire khi cần hành động):
  Soft hazard: Sonar ping rhythm thay đổi (không cần voice)
  Direction change: Voice ngắn "rẽ phải sau 10 bước"
  Path blocked: Voice "vỉa hè bị chặn, xuống lòng đường"

TIER 3 — CONTEXTUAL (chỉ khi user hỏi hoặc context thay đổi lớn):
  Scene description: Chỉ khi EXPLORE mode hoặc user hỏi
  POI identification: Chỉ khi dừng lại >5 giây
  Weather/lighting: Chỉ khi cực đoan (tối đột ngột, mưa)
```

### Đề xuất cụ thể

**System prompt thay đổi — ARIA speaking rules:**
```
=== AUDIO ECONOMY RULES ===
1. Distance information → ALWAYS use sonar ping, NEVER voice
2. "Path is clear" → NEVER say this. Silence = safe.
3. Voice only when: hazard requires direction, user asks, scene changes radically
4. Max voice cadence: 1 utterance per 8 seconds unless TIER 1
5. Prefer: "Stop. Hole. Left." over "There appears to be a hole on your left side"
```

**Sonar ping semantic encoding:**
```typescript
// spatialAudioEngine.ts — extend ping patterns
const PING_SEMANTIC = {
  approaching_object:  { interval: 100, pitch: 880 },   // high urgency
  soft_obstacle:       { interval: 400, pitch: 440 },   // medium
  path_clear:          null,                             // silence
  turning_recommended: { interval: 600, pitch: 330 },   // low, directional
  destination_near:    { interval: 200, pitch: 660 },   // rising pattern
}
```

### Ràng buộc
- Thay đổi chủ yếu ở system prompt + spatialAudioEngine — không ảnh hưởng safety layers
- Cần tune kỹ để không under-warn (false negative nguy hiểm hơn false positive)
- User study cần validate sau khi implement (ngay cả 5 người test là đủ signal)

---

## UPGRADE 3 — Vocal Stress Detection → Mode Auto-Escalation

### Vấn đề
QUIET mode hiện tại chỉ speak cho hazards <2m. Nhưng nếu user đang hoảng sợ (bị lạc, xe đang đến gần, ngã) và đang ở QUIET mode → ARIA im lặng khi user cần nhất.

### Cơ sở kỹ thuật
Gemini Live 2.5 với `enable_affective_dialog: true` đã detect vocal stress. Vấn đề là hiện tại không có logic escalate mode dựa trên emotion state.

### Đề xuất cụ thể

**ADK tool mới:**
```python
# tools/emotion_escalator.py
def escalate_mode_if_stressed(state: str, confidence: float, current_mode: str) -> dict:
    """
    Called by Gemini when it detects vocal stress/fear/panic.
    Overrides quiet mode to ensure user gets guidance when distressed.
    """
    if state in ['stressed', 'fearful', 'panicked'] and confidence > 0.7:
        if current_mode in ['QUIET', 'EXPLORE']:
            return {
                'action': 'set_mode',
                'new_mode': 'NAVIGATION',
                'reason': 'vocal_distress_detected',
                'revert_after_seconds': 120  # auto-revert sau 2 phút
            }
    return {'action': 'none'}
```

**System prompt addition:**
```
=== AFFECTIVE OVERRIDE ===
If you detect vocal stress, fear, or panic in user's voice:
1. IMMEDIATELY call escalate_mode_if_stressed()
2. Speak calmly: "I'm here. [Most important safety info first]."
3. Do NOT ask "Are you okay?" — give actionable info first, check after.
```

### Ràng buộc
- Confidence threshold 0.7 quan trọng — tránh escalate từ normal excited speech
- Auto-revert sau 120s tránh stuck ở NAVIGATION mode sau sự kiện
- `log_emotion_event()` đã có sẵn — chỉ cần thêm escalation logic

---

## UPGRADE 4 — Session Spatial Memory

### Vấn đề
Apollos hiện tại stateless — mỗi frame là independent. Không nhớ "góc đường này có bậc thang" từ 5 phút trước. User phải re-discover hazards mỗi lần đi qua.

### Architecture

**Session memory store trong `session_manager.py`:**
```python
@dataclass
class HazardMemory:
    hazard_type: str
    position_description: str  # "3 steps before the door with red sign"
    yaw_at_detection: float
    frame_sequence: int
    confirmed_count: int  # tăng mỗi lần re-detected
    last_seen_frame: int

class SpatialMemoryStore:
    def __init__(self, session_id: str):
        self.memories: list[HazardMemory] = []
        self.max_memories = 20  # sliding window

    def add_hazard(self, hazard: HazardMemory):
        # Dedup: nếu cùng hazard_type và yaw tương tự (+/- 15°) → update confirmed_count
        # Không duplicate memories cho cùng một hazard

    def get_relevant_context(self, current_yaw: float) -> str:
        # Trả về memories với yaw gần current ±30° → inject vào Gemini context
        relevant = [m for m in self.memories
                    if abs(m.yaw_at_detection - current_yaw) < 30]
        if relevant:
            return "[SPATIAL MEMORY: " + "; ".join(
                f"{m.hazard_type} ahead ({m.confirmed_count}x confirmed)" 
                for m in relevant
            ) + "]"
        return ""
```

**Inject vào live_bridge.py:**
```python
# Thêm vào motion_text construction:
spatial_context = session.spatial_memory.get_relevant_context(current_yaw)
motion_text = f"[KINEMATIC: ...]{odometry_hint}{spatial_context}"
```

**System prompt addition:**
```
=== SPATIAL MEMORY ===
[SPATIAL MEMORY: ...] in context = hazards confirmed in this area before.
Use this to pre-warn: "This area had a pothole earlier — proceed carefully."
Do not over-warn: only mention if user is approaching the remembered hazard zone.
```

### Ràng buộc
- Max 20 memories per session — tránh context window bloat
- Yaw-based proximity (±30°) là approximation — không chính xác tuyệt đối nhưng đủ để useful
- Memory không persist across sessions (privacy-safe, không cần consent flow phức tạp)
- Post-contest: persistent crowdsourced map sẽ thay thế in-session memory

---

## UPGRADE 5 — Maps Grounding + Location Intelligence

### Vấn đề
ARIA hiện tại mô tả "có một cửa kính lớn phía trước" nhưng không biết đó là cái gì. Maps grounding biến vision description thành world knowledge.

### Điều kiện kỹ thuật
- Cần GPS permission (đã có hoặc dễ thêm)
- Gemini Grounding with Google Maps available qua Vertex AI hoặc AI Studio
- Chỉ trigger khi user dừng lại >5s (không cần maps query khi đang đi)

### Đề xuất cụ thể

**ADK tool mới:**
```python
# tools/location_intel.py
async def identify_location(lat: float, lng: float, heading_deg: float) -> dict:
    """
    Trigger Google Maps grounding để identify POI trong tầm nhìn.
    Chỉ call khi: motion_state == 'stationary' AND last_call > 30s ago
    """
    # Rate limit: max 1 call/30s khi stationary
    # Returns: {name, type, distance_m, relevant_info}
    # Example: {name: "Bệnh viện Bạch Mai", type: "hospital",
    #           entrance: "main entrance 20m north", hours: "24/7"}
```

**System prompt addition:**
```
=== LOCATION AWARENESS ===
When identify_location() returns data:
- Announce only if user seems to be approaching or looking for this place
- Format: "[Place name]. [One relevant fact]." — max 2 sentences
- For hospitals/pharmacies/transit: always announce when nearby
- For general shops: only announce if user asks
```

### Ràng buộc
- Rate limit critical — maps query mỗi frame sẽ đốt quota và tăng latency
- Stationary-only trigger giải quyết rate limit tự nhiên
- Fallback: nếu Maps grounding không available → ARIA mô tả visual sẽ vẫn hoạt động

---

## UPGRADE 6 — Kinematic Safety Escalation Matrix

### Vấn đề
Hiện tại chỉ có một binary: `walking_fast` → skip confirmation. Không có gradation dựa trên kết hợp nhiều tín hiệu.

### Đề xuất: Risk Score thay vì Binary

```python
# live_bridge.py
def compute_risk_multiplier(motion_state: str, pitch: float,
                             velocity: float, yaw_delta: float) -> float:
    score = 1.0

    # Motion urgency
    if motion_state == 'running': score *= 2.0
    elif motion_state == 'walking_fast': score *= 1.5

    # Unstable carry (phone tilted = attention divided)
    if abs(pitch) > 20: score *= 1.3

    # Recent rotation = changed environment
    if abs(yaw_delta) > 30: score *= 1.4

    # High velocity + pitch = potential fall risk
    if velocity > 2.5 and abs(pitch) > 15: score *= 1.5

    return min(score, 4.0)  # cap at 4x

# Inject vào system prompt:
# risk_score > 2.0 → "ELEVATED RISK — skip confirmation, act immediately"
# risk_score > 3.0 → "HIGH RISK — treat any ambiguous hazard as confirmed"
```

### Ràng buộc
- Cap tại 4.0 để tránh over-trigger trong mọi điều kiện
- Score chỉ affects confirmation threshold, không affects Layer 0 edge detection
- Cần tune multipliers dựa trên real-world testing

---

## UPGRADE 7 — AEC Hardening cho Vietnam Urban Noise

### Vấn đề
Vietnam street noise là extreme case: còi xe liên tục, nhạc chợ, tiếng động công trình. Standard `echoCancellation: true` chưa đủ — VAD death loop (ARIA nói → mic capture ARIA → VAD trigger → ARIA ngắt giữa chừng) là real risk trong môi trường ồn.

### Đề xuất cụ thể

**AudioWorklet noise gate:**
```typescript
// Thêm vào useAudioStream.ts
// Noise gate: chỉ send audio chunk khi có voice activity thật sự
// Không gửi background noise liên tục → giảm VAD false triggers

class VoiceGateProcessor extends AudioWorkletProcessor {
    private rmsHistory: number[] = [];
    private readonly VOICE_THRESHOLD = 0.02;
    private readonly GATE_HOLD_FRAMES = 8;  // 400ms hold sau voice

    process(inputs: Float32Array[][]): boolean {
        const rms = computeRMS(inputs[0][0]);
        this.rmsHistory.push(rms);
        if (this.rmsHistory.length > 10) this.rmsHistory.shift();

        const isVoice = rms > this.VOICE_THRESHOLD
            || this.rmsHistory.slice(-this.GATE_HOLD_FRAMES)
                              .some(r => r > this.VOICE_THRESHOLD);

        if (isVoice) {
            this.port.postMessage({ type: 'audio_chunk', data: inputs[0][0] });
        }
        return true;
    }
}
```

**Earphone detection + output routing:**
```typescript
// Detect earphone via AudioContext.destination.channelCount
// Nếu earphone: output HRTF stereo (hiện tại)
// Nếu speaker: output mono + tăng volume + đổi sang directional cues
//   bằng pan + delay thay vì HRTF (HRTF qua speaker không effective)
const outputMode = await detectAudioOutput();
if (outputMode === 'speaker') {
    panner.panningModel = 'equalpower';  // thay HRTF
    panner.positionX.value = position_x * 1.5;  // exaggerate pan
}
```

### Ràng buộc
- Voice gate có thể clip đầu từ của user — hold time 400ms giảm thiểu nhưng không loại bỏ hoàn toàn
- Speaker mode HRTF degradation là real — directional accuracy giảm ~50% nhưng vẫn tốt hơn không có gì
- Test mandatory với earphone không có mic (common với Android users)

---

## UPGRADE 8 — Onboarding Flow cho Real-World Trust Building

### Vấn đề không ai nói đến
Người mù trust app bằng cách nào? Không phải đọc feature list — mà qua một vài giây đầu tiên app prove được nó hiểu môi trường. Onboarding kém → user quit trước khi discover giá trị thật.

### Đề xuất: 90-Second Trust Protocol

```
Giây 0-10:   ARIA giới thiệu bằng tiếng Việt tự nhiên
             "Xin chào, tôi là ARIA. Tôi đang nhìn qua camera của bạn."
             → Mô tả ngay 1 vật thể rõ ràng nhất trong frame
             → Prove camera đang hoạt động với mô tả cụ thể, không generic

Giây 10-30:  Hazard awareness demo
             → Hướng dẫn user giơ tay trước camera
             → ARIA: "Tôi thấy bàn tay của bạn, khoảng 30cm, chính giữa"
             → Demonstrate detection capability một cách tangible

Giây 30-60:  Sonar calibration
             → ARIA: "Tôi sẽ test âm thanh định hướng — nghe hướng nào?"
             → Fire sonar ping LEFT, RIGHT, CENTER
             → Confirm user nghe đúng hướng trước khi ra đường

Giây 60-90:  Mode selection
             → "Bạn muốn tôi nói nhiều hay chỉ cảnh báo khi cần?"
             → Set NAVIGATION vs QUIET dựa trên preference
```

### Ràng buộc
- Skip button mandatory — experienced users không muốn onboarding
- Lưu onboarding_completed flag → không show lại
- Sonar calibration đặc biệt quan trọng: nếu user nghe sai hướng → adjust HRTF pan offset

---

## UPGRADE 9 — TFLite Depth Model (Post-Contest V2)

*Ghi lại đầy đủ để implement ngay sau contest.*

### Target model: RTS-Mono hoặc Depth Anything V2 Small

**Deployment path:**
```
1. Export model sang TFLite FP16 (giảm size ~50%)
2. Convert sang WASM với TFLite WASM runtime
3. Replace survivalReflex.worker.ts optical flow bằng depth inference
4. Output: depth map 64×64 → identify closest object + position_x chính xác
```

**Upgrade gì so với optical flow:**
- `position_x` trong HARD_STOP payload thật sự chính xác (hiện tại luôn = 0)
- Phân biệt được "vật thể gần" vs "ánh sáng thay đổi"
- Detect bậc thang xuống (depth discontinuity) — optical flow không thể làm được
- Detect hố, vỉa hè lún — critical cho Vietnam streets

### Ràng buộc chính
- Model size: target <15MB sau quantization để PWA load nhanh
- Inference budget: <10ms trên mid-range Android (Snapdragon 700 series)
- Cold start: model load lần đầu ~2-3s — cần loading state trong UI

---

## UPGRADE 10 — Crowdsourced Hazard Map Schema (Long-Term)

*Thiết kế vào Firestore schema ngay bây giờ, populate data sau.*

```typescript
// Firestore collection: hazard_map
interface HazardMapEntry {
    geohash: string;           // geohash precision 7 (~150m²)
    lat: number;
    lng: number;
    hazard_type: HazardType;
    confirmed_count: number;   // số lần detect bởi users khác nhau
    last_confirmed: Timestamp;
    time_pattern?: {           // hazards có time pattern
        peak_hours: number[];  // [7, 8, 17, 18] = rush hour
        days: number[];        // [1,2,3,4,5] = weekdays only
    };
    description_vi: string;    // Vietnamese description
}
```

**ARIA integration khi có đủ data:**
Khi `identify_location()` query → check geohash của current position → nếu có hazard entries với `confirmed_count > 5` → inject vào context: `[CROWD MAP: Vỉa hè đoạn này thường bị xe đậu vào 7-9h sáng]`

---

## Summary — Upgrade Priority Matrix

| # | Upgrade | Impact | Dependencies | Notes |
|---|---|---|---|---|
| 1 | Temporal smoothing optical flow | Safety | None | Fix ngay |
| 2 | Silence-as-signal audio hierarchy | UX | System prompt | Prompt engineering |
| 3 | Vocal stress → mode escalation | Safety + UX | Gemini affective dialog | New ADK tool |
| 4 | Session spatial memory | Intelligence | session_manager.py | New data structure |
| 5 | Maps grounding + location intel | Wow factor | GPS + Vertex AI | New ADK tool + rate limit |
| 6 | Kinematic risk score matrix | Safety | live_bridge.py | Replace binary flag |
| 7 | AEC hardening + speaker mode | Real-world robustness | AudioWorklet | Vietnam noise profile |
| 8 | Onboarding trust protocol | Adoption | ARIA persona | 90s sequence |
| 9 | TFLite depth model | Accuracy leap | WASM runtime | V2 |
| 10 | Crowdsourced hazard map | Data moat | Firestore schema | Long-term |

---

*Apollos Upgrade Plan v1.0 — March 2026*
*"Edge protects lives. Cloud decodes the world. Memory makes it home."*
