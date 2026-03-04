# Test Matrix (MVP)

## Safety interrupt

- Trigger hazard via `/dev/hazard/{session_id}`.
- Verify frontend receives `HARD_STOP` and starts directional ping before any assistant text.

## AEC stability

- Run mic + speaker 15 minutes with external speaker nearby.
- Confirm no self-interruption loop from local playback.

## Session resilience

- Keep session active > 12 minutes.
- Simulate websocket reconnect by toggling network.
- Verify transcript and mode continuity.

## Adaptive duty cycling

- Stationary: frame cadence near 5000ms.
- Walking: near 1000ms.
- Running: near 500ms.

## Spatial accuracy

- Send `position_x` = -1, 0, 1 through hazard endpoint.
- Confirm left/center/right auditory placement in stereo headphones.

## Permission degradation

- Deny DeviceMotion on iOS and Android.
- Verify app still streams at fixed behavior without crash.

## Automation scripts

- Static + unit + internal latency benchmark: `python3 scripts/hardening_pass.py`
- Reconnect integration (requires running backend + deps): `python3 scripts/test_reconnect.py`
- HARD_STOP E2E benchmark via HTTP/WS (requires running backend + deps): `python3 scripts/benchmark_hard_stop.py --iterations 20 --budget-ms 100`
