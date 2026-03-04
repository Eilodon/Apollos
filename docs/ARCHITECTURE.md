# Architecture (Text Diagram)

```text
Mobile PWA (React)
  - Camera (adaptive FPS)
  - Mic 16k PCM
  - SpatialAudioEngine (HRTF)
  - Wake Lock + OLED black
  - Motion sensor fusion
      |
      | WebSocket BIDI (/ws/live)
      v
FastAPI + Agent Orchestrator
  - Session manager
  - Tool runtime
  - hazard logger => HARD_STOP
  - Firestore bridge
      |
      | Emergency WS (/ws/emergency)
      v
Client hard interrupt
  - stop current audio
  - sonar ping by direction + distance rhythm
```

For submission, export this into PNG and place at `docs/ARCHITECTURE.png`.
