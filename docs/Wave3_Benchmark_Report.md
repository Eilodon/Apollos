# Wave 3 Benchmark Report

Date: 2026-03-05  
Workspace: `/home/ybao/B.1/Apollos`

## 1) Edge Latency (HARD_STOP)

Command:

```bash
PYTHONPATH=backend python3 scripts/benchmark_hard_stop_internal.py --iterations 100 --budget-ms 100
```

Result summary:

- Iterations: 100
- Avg: **0.027ms**
- P50: **0.024ms**
- P95: **0.041ms**
- Trigger -> Server emit P95: **0.000ms**
- Server emit -> Receive P95: **0.956ms**
- Budget check: **PASS** (`p95 <= 100ms`)

Cross-check via hardening pass:

```bash
python3 scripts/hardening_pass.py
```

- Backend unit tests: PASS
- AEC static check: PASS
- Internal HARD_STOP benchmark: PASS (p95 0.051ms in 20-iteration run)
- Internal reconnect test: PASS

## 2) Crowdsourced Hazard Map E2E (Dummy Data)

Command:

```bash
python3 scripts/crowd_hazard_map_dummy_e2e.py
```

Result:

- Script prints VN crowd hints with taxonomy mapping and confirmation count.
- Status: **PASS**

## 3) ASGI In-Process Benchmark

Command attempted:

```bash
python3 scripts/benchmark_hard_stop_asgi.py --iterations 30 --budget-ms 100
```

Status in this environment:

- Timed out (`exit 124`) without output under sandbox runtime.
- Internal benchmark path remains valid and passing.

## 4) Cloud TTFT

Status in this environment:

- Not measured yet (requires live Gemini session + stable external network/API key).

Recommended measurement protocol:

1. Start backend with valid Gemini credentials.
2. Stream camera/audio from frontend for at least 30 turns.
3. For each turn, record:
   - client frame/audio send timestamp
   - first assistant audio chunk receive timestamp
4. Compute P50/P95 TTFT and segment by network type (Wi-Fi/4G).

## 5) Battery Measurement

Status in this environment:

- Real-device battery drain not measured in sandbox.
- Power governor logic is implemented and active (`useBatteryGovernor` + adaptive cloud FPS + auto QUIET at <=20%).

Recommended field protocol:

1. Android device, 100% battery, screen brightness fixed, LTE on.
2. Run 30-minute walking session in each mode:
   - NAVIGATION normal
   - NAVIGATION + high drain scenario
   - QUIET low-battery scenario
3. Record start/end battery and compute `%/hour`.
4. Capture logs for FPS caps and low-battery auto-switch events.
