# Apollos Extreme Upgrade Research 2026

Date: 2026-03-05  
Scope: competitor intelligence, best practices, case studies, SOTA stack, and pre-mortem risk analysis for real-world deployment.

## 1) Executive Summary

Apollos has a strong core architecture (dual-brain edge + cloud). The biggest upside now is not "one more model", but system-level reliability under chaos: rain, noise, dense crowds, occluded sidewalks, GPS drift, camera angle mismatch, and cognitive overload.

Top opportunities with highest impact:

1. Build a confidence-aware safety policy (not model-only decisions).
2. Add robust localization fusion (GPS + VIO + map matching + uncertainty).
3. Upgrade edge perception from optical-flow-only to multi-cue hazard reflex (depth + motion + semantics).
4. Add structured human fallback and verification loops for ambiguous/high-risk contexts.
5. Institutionalize real-world red-team testing and reliability gates before scale.

Top deployment risks:

1. Over-trust from users in high-risk scenarios despite model uncertainty.
2. Route/hazard mismatch in dense urban VN conditions (motorbikes, vendors, low signs, temporary barriers).
3. Security/operational risks in always-on websocket + camera + location pipelines.

## 2) Market and Product Landscape (What "best in class" is doing)

### 2.1 Be My Eyes (AI + human fallback)
- Be My AI supports image + follow-up chat, and explicitly requires internet.
- Product guidance warns users not to process sensitive personal/financial info with AI.
- Built-in hybrid flow: user can escalate from AI to live volunteer.
- Data controls include auto-retention windows (for AI history) and manual keep options.

Relevance to Apollos:
- Keep "AI first, human override always available".
- Add explicit sensitive-content guardrails in UX and prompt policy.

### 2.2 Seeing AI (task-specialized channels)
- Broad task channels: read, describe, products, people, currency, object finding, photo/video understanding.
- Includes "World" audio AR mode on LiDAR-capable devices.

Relevance:
- Apollos should preserve a small number of explicit task modes optimized for latency/risk profile, instead of one generic loop.

### 2.3 Google Lookout (mode-based practical workflow)
- Distinct modes (Text, Document, Explore, Currency, Food labels, Images beta).
- Includes recents/history style workflow and localized language/currency/country configs.

Relevance:
- Strengthen mode-specific tuning + localized defaults per country and environment.

### 2.4 Aira (professional remote visual interpreters + AI verify)
- On-demand professional interpreters with rich tool dashboard and access-network model.
- Access AI can be verified by human interpreter.

Relevance:
- Apollos can add "verify-with-human" as a reliability tier for mission-critical moments (street crossing, complex station transfers).

### 2.5 GoodMaps and Waymap (indoor precision navigation playbook)
- GoodMaps requires ARCore/ARKit-capable devices for indoor route guidance in mapped venues.
- Waymap emphasizes dead-reckoning / sensor-based navigation with no dependency on live GPS signal after map download.

Relevance:
- Apollos should separate two navigation classes:
  - venue-mapped high-precision mode
  - open-world uncertainty-aware mode.

### 2.6 Envision + Meta glasses (hands-free form factor)
- Envision and Meta show strong momentum on hands-free camera form factors with AI + call-assist flows.

Relevance:
- Apollos should optimize for "head/chest mounted first" interaction and hand-free operation as a primary path, not a fallback.

## 3) Case Study Insights (Evidence that changes roadmap priorities)

### 3.1 Real user behavior: apps supplement cane/dog, not replace
A 2025 survey study of blind/low-vision users highlights that navigation apps are mostly used as supplements, especially on unfamiliar routes; major unmet needs remain indoor navigation and POI precision.

Implication:
- Product messaging and risk policy must avoid "replacement framing".
- Metrics should optimize "assist + reduce friction" over "fully autonomous guidance".

### 3.2 Teleguidance UX evidence
Teleguidance research reports practical setups using chest-mounted phone video and combined audio/haptic communication in indoor/outdoor scenarios.

Implication:
- Keep chest/necklace camera path first-class.
- Continue haptic channel investment (phone + cane + wearable) as parallel signal to voice.

### 3.3 Human-centered multimodal wearable evidence
Nature Machine Intelligence (2025) reports a human-centered multimodal system (visual + audio + haptic + training) with improved usability and real-world navigation task performance.

Implication:
- Apollos should treat onboarding/training protocol as core model performance multiplier, not UX garnish.

## 4) SOTA Tech Stack Opportunities for Apollos

### 4.1 Edge perception SOTA candidates
- Depth Anything V2: strong monocular depth gains and efficient scaling options.
- YOLOv10: NMS-free real-time detection path with latency/efficiency gains.
- SAM 2: promptable image/video segmentation with streaming memory for temporal consistency.
- RT-DETR family: strong real-time end-to-end detector alternative.

Recommended architecture:
- Keep optical-flow reflex as safety baseline.
- Add lightweight depth head + hazard detector head on edge.
- Use segmentation/tracking to reduce repeated false alarms and improve hazard persistence.

### 4.2 Localization and state estimation
- OpenVINS/MSCKF ecosystem remains strong for robust visual-inertial estimation under mobile constraints.
- ARCore geospatial + scene semantics can enrich map grounding, but semantics docs emphasize outdoor-only reliability and device support constraints.

Recommended:
- Build uncertainty-aware fusion:
  - GPS
  - IMU dead-reckoning
  - VIO
  - map constraints
  - per-source confidence.
- Never issue directional "hard guidance" without minimum confidence threshold.

### 4.3 Cloud cognition / live agent layer
- Gemini Live native audio docs indicate strong capabilities for affective dialog, proactive audio, multilingual switching, and tool use.

Recommended:
- Continue strict split:
  - edge = survival reflex
  - cloud = semantic enrichment + social/emotional support.
- Explicitly cap cloud authority in high-risk windows.

## 5) Pre-mortem: Likely Blind Spots and Failure Breakpoints in Real Deployment

## 5.1 Safety and human factors
- Risk: user over-trust in low-confidence model output.
- Risk: cognitive overload from verbose speech in fast-moving streets.
- Risk: delayed/noisy warnings in crowded intersections.

Mitigation:
- Confidence-tiered output policy (silence/ping/short command/human escalation).
- Strict utterance budget under motion.
- User-calibrated verbosity profiles.

## 5.2 Perception failure modes
- Night/rain/glare/lens occlusion degrade all vision models.
- Phone carry-angle drift breaks directional assumptions.
- Dynamic occlusions (motorbike swarms, moving vendor carts) break static-scene heuristics.

Mitigation:
- Sensor health score per frame.
- "cannot-see-clearly" protocol with explicit behavior downgrade.
- Temporal consensus across edge cues before declaring path-safe.

## 5.3 Localization failure modes
- Indoor/outdoor transitions and urban canyon GPS multipath.
- Inertial drift over longer sessions.
- Map mismatch from temporary urban changes.

Mitigation:
- Maintain uncertainty ellipse and expose it to decision engine.
- Re-localization checkpoints.
- crowd-map decay and freshness scoring (already aligned with Wave 3 direction).

## 5.4 Security and privacy breakpoints
- Long-lived websocket channels and token misuse risks.
- Prompt/tool injection style payloads through multimodal channels.
- Sensitive image/audio/location retention liabilities.

Mitigation:
- OIDC short-lived tokens + strict issuer/audience/alg validation.
- Message schema validation and command allowlists.
- Data minimization, retention TTLs, and user-visible controls.
- Continuous security regression tests for websocket/channel abuse.

## 5.5 Operational breakpoints
- Cloud API quota/latency spikes.
- Device thermal throttling and battery collapse in long walking sessions.
- Backend region outages.

Mitigation:
- Adaptive duty cycle tied to battery and thermal envelope.
- Multi-region backend failover strategy.
- "degraded-safe mode" that is explicit and audible.

## 6) Extreme Upgrade Blueprint (12-16 weeks)

### Phase A (Weeks 1-4): Reliability hardening
- Add confidence model over edge + cloud cues.
- Add sensor health and observability scoring.
- Implement policy engine for safety-tiered output.

Exit criteria:
- HARD_STOP recall improves on curated hazard set.
- False-stop rate below agreed threshold in non-hazard paths.

### Phase B (Weeks 5-8): Perception upgrade
- Integrate lightweight edge detector + depth head.
- Add temporal hazard tracking memory (short horizon).
- Add weather/night robustness test suite.

Exit criteria:
- Improved detection in low-light/rain-like scenarios.
- Stable latency under thermal throttling.

### Phase C (Weeks 9-12): Localization upgrade
- Fuse GPS + IMU + VIO + map matching with uncertainty.
- Add indoor venue mode and uncertainty-dependent guidance strictness.

Exit criteria:
- Route completion and correction-rate targets in mixed environments.

### Phase D (Weeks 13-16): Human-in-loop and safety governance
- One-tap "verify with human" for high-risk steps.
- Incident taxonomy + postmortem pipeline + safety dashboard.
- Red-team drills before every release.

Exit criteria:
- Incident detection and rollback process proven in staging.

## 7) Recommended Metrics and Gates

Safety metrics:
- Time-to-first-alert for imminent hazards
- HARD_STOP recall / precision
- False calm events (danger missed)
- High-risk scenario success rate (street crossing approach, curb drop, overhead obstacle)

Mobility metrics:
- Route completion rate
- Number of corrective prompts per 100m
- POI/entrance acquisition time

Human factors:
- NASA-TLX style workload
- Trust calibration score (over-trust vs under-trust)
- Long-session fatigue and abandonment rate

Reliability and ops:
- p95/p99 edge-to-alert latency
- cloud fallback rate
- reconnect success rate
- battery drain per hour by mode

Security:
- token misuse detection rate
- websocket abuse test pass rate
- prompt/tool injection red-team pass rate

## 8) Direct Actions for Current Apollos Codebase

1. Add a formal `SafetyPolicyEngine` module:
   - input: hazard confidence, localization uncertainty, motion state, sensor health
   - output: action tier (silent/ping/voice/stop/human).
2. Add `SensorHealthScore` contract to every frame.
3. Add degraded-mode UX contract with explicit user notifications.
4. Add red-team scripts for:
   - night/rain/occlusion
   - noisy audio and false wake
   - websocket reconnect + auth edge cases.
5. Add deployment gate that blocks release if safety KPIs regress.

## 9) Project-Specific Security Audit Snapshot (2026-03-05)

High-priority findings (local audit):
- A Firebase service-account JSON key file exists in workspace root (contains `private_key` material).
- Legacy websocket token transport via query string existed in client path (token leakage risk via logs/proxies/referrers).
- Rotation helper script used a hardcoded absolute credential path tied to a specific key filename.

Hardening already applied in this pass:
- Websocket auth token transport switched to `Sec-WebSocket-Protocol` (`authb64.<token>`) by default.
- Query-string token fallback is now policy-gated:
  - auto enabled in local development
  - auto disabled in production
  - explicit override via `WS_ALLOW_QUERY_TOKEN`.
- Dev endpoint token compare now uses constant-time comparison.
- Rotation script defaults to safer credential locations:
  - `SERVICE_ACCOUNT_KEY_FILE=$HOME/.config/apollos/firebase-adminsdk.json`
  - `BACKEND_ENV_FILE=<repo>/backend/.env`.

Residual risks to close next:
- External OIDC login bootstrap still needs full IdP integration in production (PKCE + secure redirect + token exchange); current broker supports ticketing + short refresh after exchange.
- Add payload size/schema limits for websocket `multimodal_frame` and `audio_chunk` to reduce abuse/DoS surface.
- Move runtime secrets (`GEMINI_API_KEY`, Firebase key) to Secret Manager + workload identity in production.

## 10) Execution Snapshot (Implemented)

Implemented in current codebase upgrade pass:
- Added `SafetyPolicyEngine` (`silent/ping/voice/hard_stop/human_escalation`) and wired it into `log_hazard_event`.
- Added `SensorHealthScore` contract on frontend + backend and persisted observability state per session.
- Added explicit degraded-safe mode signaling (`safety_state`) from backend to frontend with UX surfacing.
- Added websocket payload guards (raw WS message limit + frame/audio/command size checks).
- Added OIDC broker flow with short-lived websocket tickets (`/auth/oidc/exchange` + `HttpOnly` cookie + `/auth/ws-ticket`) to avoid localStorage token persistence.
- Added carry-mode-aware depth hazard ROI and pendulum-aware confidence damping (`hand_held` vs `necklace/chest_clip`).
- Added deterministic edge barcode worker path (browser `BarcodeDetector` with stable multi-frame confirmation, no LLM dependency).
- Added production-hardened human fallback flow:
  - signed `help_ticket` link generation
  - one-time ticket exchange endpoint (`/auth/help-ticket/exchange`)
  - short-lived helper viewer token + dedicated helper websocket (`/ws/help/{session_id}`)
  - Twilio Video WebRTC upgrade (patient publisher + helper subscriber) while preserving ticket/security model
  - optional SMS dispatch to emergency contacts when configured.
- Added release-blocking safety gate:
  - policy recall/false-stop KPI checks
  - HARD_STOP latency benchmark check.
- Extended test suite for policy, payload guards, observability transitions, and hazard-tier behavior.

---

## 11) Deep-Dive Assessment for 3 New Upgrade Ideas (2026-03-05)

This section evaluates whether each idea should be applied to Apollos now, and how to integrate with the current codebase (`request_human_help`, `emotion_escalator`, `carryMode`, depth worker, OIDC broker flow).

### 11.1 Idea #1: "Human Fallback Insurance" (Be My Eyes / Aira style)

Decision: **GO (phased rollout, safety-gated)**  
Reason: High value in true edge-case failures (panic, camera occlusion, dense traffic chaos), aligned with `human_escalation` safety tier already present in policy engine.

Current readiness in Apollos:
- `request_human_help()` exists but currently only returns a static help link with `session_id`.
- Emotion distress signal exists (`emotion_escalator.py`), plus `sos` and long-press command path in frontend/backend.
- Emergency websocket channel exists, but not a true browser-based live A/V bridge for family/caregiver.

Recommended implementation path:
1. **Trusted-contact model + explicit consent**
   - Add emergency contacts (verified phone numbers).
   - Require explicit onboarding consent for emergency A/V sharing.
2. **One-time emergency ticket (never expose raw session id)**
   - Create short-lived, one-time help ticket (e.g., 3-5 min TTL).
   - Bind ticket to `session_id`, allowed roles, and max viewers.
3. **Browser-based live link**
   - Open caregiver link with web-only join (no app install).
   - Use WebRTC provider integration (LiveKit or Twilio Video) with server-minted room tokens.
4. **Automatic trigger policy**
   - Trigger only on strict conditions: distress confidence + explicit SOS phrase + high-risk context.
   - Keep manual trigger always available.
5. **SMS dispatch + audit**
   - Send link via SMS gateway API.
   - Log trigger reason, ticket issuance, delivery status, and join events.

Hard guardrails (must-have):
- Per-event consent copy + audible confirmation to user.
- Link TTL + one-time join + revoke-on-demand.
- No direct camera/mic stream without fresh user action in low-confidence triggers.
- Incident log + tamper-evident audit trail.

Suggested KPI gates:
- `time_to_human_join_p95` < 20s (staging target).
- False auto-trigger rate < agreed threshold.
- 100% expired tickets rejected.

### 11.2 Idea #2: "Deterministic Certainty" for high-stakes tasks (Seeing AI style)

Decision: **GO (immediate for barcode/QR, pilot for VN currency)**  
Reason: Correctly separates "deterministic edge tasks" from "LLM semantic reasoning". This reduces latency, cloud spend, and catastrophic misread risk.

Current readiness in Apollos:
- Edge worker architecture already in place (`survivalReflex.worker`, `depthGuard.worker`).
- Cloud path currently handles generic multimodal reasoning.
- No dedicated deterministic scanner pipeline yet.

Recommended architecture:
1. **Barcode/QR fast path (apply now)**
   - Priority A: native `BarcodeDetector` in secure contexts when supported.
   - Priority B: fallback to ZXing JS browser layer in worker.
   - Emit deterministic result directly from edge layer (no LLM dependency).
2. **Currency mode (pilot)**
   - Add dedicated VN banknote classifier (TFLite int8) + confidence threshold.
   - Require temporal consensus (e.g., 2 of 3 recent frames) before speaking denomination.
   - If below threshold: explicit "uncertain" and optional cloud verify.
3. **Policy split**
   - Add `task_mode=deterministic` contract for barcode/money.
   - Block LLM from overriding deterministic output for these task types.

Hard guardrails (must-have):
- Deterministic tasks must return confidence + fallback state.
- Never output exact denomination/medicine code when confidence below threshold.
- Add adversarial test set: blur, glare, partial occlusion, folded banknotes.

Suggested KPI gates:
- `barcode_precision` and `barcode_recall` by format.
- `currency_top1_accuracy` and `uncertain_rate`.
- `edge_latency_p95` for deterministic mode.

### 11.3 Idea #3: "Biomechanical Anchoring" (carry-mode-aware perception)

Decision: **GO (high priority, low integration risk)**  
Reason: Apollos already has `carryMode` profiles and kinematic gating; this is a direct multiplier for edge reliability in real walking scenarios.

Current readiness in Apollos:
- `carryMode.ts` and `useCarryMode.ts` already active.
- `useCamera.ts` already passes `carry_mode` and uses gyro-based gating.
- Depth worker currently uses a fixed ROI (`yStart=0.52`) and does not adapt by carry mode.

Recommended implementation path:
1. **Mode-aware depth ROI**
   - `hand_held`: shift drop-detection ROI lower to emphasize near-ground hazards.
   - `necklace/chest_clip`: center ROI and widen temporal smoothing window.
2. **Anti-pendulum stabilization**
   - Add IMU-based temporal smoothing/compensation before depth hazard trigger.
   - Use gait-aware damping in necklace/chest modes to suppress oscillation false positives.
3. **Mode-specific thresholds**
   - Tune `gyroThreshold`, `cosTiltThreshold`, and hazard confidence floor per mode.
   - Keep conservative fallback when mode uncertain.

Hard guardrails (must-have):
- Preserve HARD_STOP recall while reducing false alarms by carry mode.
- Never silence alerts solely due to mode inference uncertainty.
- Add drift monitoring (yaw drift and carry mode flip rate).

Suggested KPI gates:
- False HARD_STOP per 100m by carry mode.
- Time-to-alert under walking oscillation scenarios.
- Carry-mode classification stability.

### 11.4 Integrated priority order for Apollos

1. **Immediate (1-2 sprints)**: deterministic barcode/QR path + carry-mode-aware depth ROI.
2. **Next (2-4 sprints)**: anti-pendulum stabilization + VN currency pilot dataset/model.
3. **After auth/governance hardening (3-5 sprints)**: full human fallback live link + SMS dispatch.

### 11.5 Risks if applied incorrectly

- Human fallback without one-time ticketing can leak live streams.
- Currency "confidence theater" (always speaking a value) is dangerous; uncertainty must be explicit.
- Aggressive stabilization can over-smooth and delay true hazard alerts.

---

## Sources

- Apollos internal blueprint: `docs/Apollos.md` and `README.md` in this repository.
- Gemini Live / native audio docs (Vertex AI):
  - https://docs.cloud.google.com/vertex-ai/generative-ai/docs/models/gemini/2-5-flash-live-api
  - https://cloud.google.com/vertex-ai/generative-ai/docs/live-api
  - https://cloud.google.com/vertex-ai/generative-ai/docs/live-api/tools
- Be My Eyes help/news:
  - https://support.bemyeyes.com/hc/en-us/articles/18133134809105-How-do-I-use-Be-My-AI
  - https://support.bemyeyes.com/hc/en-us/articles/17493302011921-Be-My-AI-image-to-text-assistance
  - https://support.bemyeyes.com/hc/en-us/articles/360006070777-Do-I-have-to-pay-for-Be-My-Eyes-or-is-it-free
  - https://support.bemyeyes.com/hc/en-us/articles/360005522738-Calling-a-Sighted-Volunteer
- Seeing AI:
  - https://apps.apple.com/us/app/seeing-ai/id999062298
- Google Lookout / accessibility:
  - https://support.google.com/accessibility/android/answer/9031274
  - https://www.google.com/intl/en-GB/accessibility/products-features/
- Aira:
  - https://apps.apple.com/us/app/aira-explorer/id1590186766
  - https://go.aira.io/
- GoodMaps:
  - https://goodmaps.com/the-app/
  - https://goodmaps.com/release-notes/
  - https://support.goodmaps.com/check-in-1
- Waymap:
  - https://www.waymapnav.com/
  - https://www.waymapnav.com/newsroom/waymap-goes-live-in-washington-dc
  - https://www.waymapnav.com/faq
- Envision:
  - https://www.letsenvision.com/glasses/home
  - https://support.letsenvision.com/hc/en-us/articles/37264338754193-Set-Up-and-Personalize-ally-on-Envision-Glasses
- Meta accessibility updates:
  - https://about.fb.com/news/2025/05/advancing-accessibility-meta/
- Apple assistive detection warnings/capabilities:
  - https://support.apple.com/guide/iphone/get-live-descriptions-of-your-surroundings-iph37e6b3844/ios
  - https://support.apple.com/guide/iphone/detect-doors-around-you-iph35c335575/ios
  - https://support.apple.com/en-am/guide/iphone/iph35c335575/ios
- ARCore semantics/geospatial:
  - https://developers.google.com/ar/develop/scene-semantics
  - https://developers.google.com/ar/reference/java/com/google/ar/core/Config.SemanticMode
  - https://developers.google.com/ar/reference/c/group/ar-config
- Research papers / studies:
  - Depth Anything V2: https://arxiv.org/abs/2406.09414
  - SAM 2: https://arxiv.org/abs/2408.00714
  - YOLOv10: https://arxiv.org/abs/2405.14458
  - RT-DETR: https://arxiv.org/abs/2304.08069
  - Smartphone app usage priorities (2025/2026 publication): https://pubmed.ncbi.nlm.nih.gov/40854009/
  - Navigation systems review (2024): https://pubmed.ncbi.nlm.nih.gov/38841448/
  - Teleguidance UX study: https://pubmed.ncbi.nlm.nih.gov/34054327/
  - Human-centred multimodal wearable system (Nature Machine Intelligence, 2025): https://www.nature.com/articles/s42256-025-01018-6
- Security/risk frameworks:
  - OWASP WebSocket Security Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html
  - RFC 8725 (JWT BCP): https://www.rfc-editor.org/rfc/rfc8725
  - RFC 7519 (JWT): https://www.rfc-editor.org/rfc/rfc7519
  - RFC 9700 (OAuth 2.0 Security BCP): https://www.rfc-editor.org/rfc/rfc9700
  - OpenID Connect Core: https://openid.net/specs/openid-connect-core-1_0-18.html
  - RFC 7636 (PKCE): https://www.rfc-editor.org/rfc/rfc7636
  - RFC 9449 (DPoP): https://www.rfc-editor.org/rfc/rfc9449
  - NIST AI RMF 1.0: https://www.nist.gov/publications/artificial-intelligence-risk-management-framework-ai-rmf-10
  - NIST GenAI Profile: https://www.nist.gov/publications/artificial-intelligence-risk-management-framework-generative-artificial-intelligence
- Realtime communications / web platform / deterministic scanning:
  - LiveKit tokens & grants: https://docs.livekit.io/frontends/authentication/tokens/
  - LiveKit encryption overview: https://docs.livekit.io/transport/encryption/
  - LiveKit connect flow: https://docs.livekit.io/intro/basics/connect/
  - Twilio Video access tokens: https://www.twilio.com/docs/video/tutorials/user-identity-access-tokens
  - Twilio Messages API: https://www.twilio.com/docs/messaging/api/message-resource
  - MDN BarcodeDetector: https://developer.mozilla.org/en-US/docs/Web/API/BarcodeDetector
  - ZXing browser layer: https://github.com/zxing-js/browser
  - MDN getUserMedia security/permissions: https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia
