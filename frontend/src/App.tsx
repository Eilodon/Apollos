import { TouchEvent, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { CameraView } from './components/CameraView';
import { HazardCompass } from './components/HazardCompass';
import { ModeIndicator } from './components/ModeIndicator';
import { OLEDBlackOverlay } from './components/OLEDBlackOverlay';
import { TranscriptPanel } from './components/TranscriptPanel';
import { useARIA } from './hooks/useARIA';
import { useAudioStream } from './hooks/useAudioStream';
import { useCamera } from './hooks/useCamera';
import { useMotionSensor } from './hooks/useMotionSensor';
import { useWakeLock } from './hooks/useWakeLock';
import { AudioCache } from './services/audioCache';
import { SpatialAudioEngine } from './services/spatialAudioEngine';
import { vibrateHardStop, vibrateReconnect, vibrateSoftConfirm } from './services/haptics';
import { BackendToClientMessage, HardStopMessage, NavigationMode } from './types/contracts';

type TranscriptEntry = {
  id: string;
  role: 'assistant' | 'user' | 'system';
  text: string;
  ts: string;
};

const MODE_ORDER: NavigationMode[] = ['NAVIGATION', 'EXPLORE', 'READ', 'QUIET'];

function nextMode(current: NavigationMode): NavigationMode {
  const index = MODE_ORDER.indexOf(current);
  return MODE_ORDER[(index + 1) % MODE_ORDER.length];
}

function createEntry(role: TranscriptEntry['role'], text: string): TranscriptEntry {
  return {
    id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
    role,
    text,
    ts: new Date().toISOString(),
  };
}

export default function App(): JSX.Element {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const sessionId = useMemo(
    () =>
      globalThis.crypto?.randomUUID?.() ??
      `session-${Date.now()}-${Math.random().toString(16).slice(2)}`,
    [],
  );

  const [navigationMode, setNavigationMode] = useState<NavigationMode>('NAVIGATION');
  const [transcriptEntries, setTranscriptEntries] = useState<TranscriptEntry[]>([]);
  const [sessionActive, setSessionActive] = useState(false);
  const [hazardPosition, setHazardPosition] = useState(0);
  const [hazardVisible, setHazardVisible] = useState(false);
  const [hazardDistance, setHazardDistance] = useState<'very_close' | 'mid' | 'far'>('mid');

  const audioCacheRef = useRef(new AudioCache(3));
  const spatialRef = useRef<SpatialAudioEngine | null>(null);
  const touchRef = useRef({
    x: 0,
    y: 0,
    at: 0,
    longPressTimer: 0,
    longPressTriggered: false,
    lastTapAt: 0,
  });

  const {
    motionSnapshot,
    permissionRequired,
    requestMotionPermission,
    shakeSignal,
  } = useMotionSensor();

  const { oledBlackMode, wakeLockActive, activateNavigationMode, deactivateNavigationMode } = useWakeLock();

  const onBackendMessage = useCallback((message: BackendToClientMessage) => {
    if (message.type === 'assistant_text') {
      setTranscriptEntries((prev) => [...prev, createEntry('assistant', message.text)]);
      audioCacheRef.current.add({ timestamp: message.timestamp, text: message.text });
      return;
    }

    if (message.type === 'audio_chunk') {
      const pcmBase64 = message.pcm24 || message.pcm16;
      if (!pcmBase64 || !spatialRef.current) {
        return;
      }

      spatialRef.current.playChunkFromBase64(pcmBase64, message.hazard_position_x ?? hazardPosition);
      audioCacheRef.current.add({ timestamp: message.timestamp, pcmBase64 });
      return;
    }

    if (message.type === 'connection_state' && message.state === 'reconnecting') {
      vibrateReconnect();
    }
  }, [hazardPosition]);

  const onHardStop = useCallback((message: HardStopMessage) => {
    setHazardVisible(true);
    setHazardPosition(message.position_x);
    setHazardDistance(message.distance);

    spatialRef.current?.fireHardStop(message.position_x, message.distance);
    vibrateHardStop();

    setTranscriptEntries((prev) => [
      ...prev,
      createEntry('system', `STOP: ${message.hazard_type} (${message.distance})`),
    ]);
  }, []);

  const aria = useARIA({
    sessionId,
    onBackendMessage,
    onHardStop,
  });

  const { micActive, startMic, stopMic, toggleMic } = useAudioStream({
    onAudioChunk: aria.sendAudioChunk,
  });

  useCamera({
    videoRef,
    enabled: sessionActive && aria.status === 'connected',
    motionSnapshot,
    onFrame: ({ frameBase64, timestamp }) => {
      aria.sendFrame({
        timestamp,
        frame_jpeg_base64: frameBase64,
        motion_state: motionSnapshot.state,
        pitch: motionSnapshot.pitch,
        velocity: motionSnapshot.velocity,
      });
    },
  });

  const appendSystemEntry = useCallback((text: string) => {
    setTranscriptEntries((prev) => [...prev, createEntry('system', text)]);
  }, []);

  const startSession = useCallback(async () => {
    if (sessionActive) {
      return;
    }

    if (permissionRequired) {
      const granted = await requestMotionPermission();
      if (!granted) {
        appendSystemEntry('Motion permission denied. Falling back to fixed behavior.');
      }
    }

    if (!spatialRef.current) {
      try {
        spatialRef.current = new SpatialAudioEngine();
      } catch {
        appendSystemEntry('Spatial audio unavailable in this browser.');
      }
    }

    await spatialRef.current?.warmup();
    await activateNavigationMode();

    aria.connect();
    await startMic();

    setSessionActive(true);
    appendSystemEntry('ARIA session started.');
  }, [activateNavigationMode, appendSystemEntry, aria, permissionRequired, requestMotionPermission, sessionActive, startMic]);

  const stopSession = useCallback(async () => {
    if (!sessionActive) {
      return;
    }

    stopMic();
    aria.disconnect();
    spatialRef.current?.stopAll();
    await deactivateNavigationMode();

    setSessionActive(false);
    setHazardVisible(false);
    appendSystemEntry('ARIA session stopped.');
  }, [aria, deactivateNavigationMode, appendSystemEntry, sessionActive, stopMic]);

  const cycleMode = useCallback(() => {
    const updated = nextMode(navigationMode);
    setNavigationMode(updated);
    aria.sendUserCommand(`set_navigation_mode:${updated}`);
    appendSystemEntry(`Mode switched to ${updated}.`);
    vibrateSoftConfirm();
  }, [appendSystemEntry, aria, navigationMode]);

  const repeatLastResponse = useCallback(() => {
    const last = audioCacheRef.current.getLast();
    if (!last) {
      appendSystemEntry('No cached response yet.');
      return;
    }

    if (last.pcmBase64 && spatialRef.current) {
      spatialRef.current.playChunkFromBase64(last.pcmBase64, hazardPosition);
    }

    if (last.text) {
      appendSystemEntry(`Repeat: ${last.text}`);
    }
  }, [appendSystemEntry, hazardPosition]);

  const requestHumanHelp = useCallback(() => {
    aria.sendUserCommand('request_human_help');
    appendSystemEntry('Human help requested.');
    vibrateHardStop();
  }, [appendSystemEntry, aria]);

  const describeInDetail = useCallback(() => {
    aria.sendUserCommand('describe_detailed');
    appendSystemEntry('Requested detailed description.');
  }, [appendSystemEntry, aria]);

  useEffect(() => {
    if (!sessionActive || shakeSignal === 0) {
      return;
    }

    aria.sendUserCommand('sos');
    appendSystemEntry('Shake detected. SOS sent.');
    vibrateHardStop();
  }, [appendSystemEntry, aria, sessionActive, shakeSignal]);

  useEffect(() => {
    if (aria.status === 'connected') {
      appendSystemEntry('Connected to backend.');
    }
    if (aria.status === 'reconnecting') {
      appendSystemEntry('Reconnecting. Please stay still.');
    }
  }, [appendSystemEntry, aria.status]);

  const onTouchStart = useCallback((event: TouchEvent<HTMLDivElement>) => {
    const touch = event.touches[0];
    if (!touch) {
      return;
    }

    const state = touchRef.current;
    state.x = touch.clientX;
    state.y = touch.clientY;
    state.at = Date.now();
    state.longPressTriggered = false;

    state.longPressTimer = window.setTimeout(() => {
      state.longPressTriggered = true;
      requestHumanHelp();
    }, 650);
  }, [requestHumanHelp]);

  const onTouchEnd = useCallback((event: TouchEvent<HTMLDivElement>) => {
    const changed = event.changedTouches[0];
    const state = touchRef.current;
    window.clearTimeout(state.longPressTimer);

    if (!changed || state.longPressTriggered) {
      return;
    }

    const dx = changed.clientX - state.x;
    const dy = changed.clientY - state.y;
    const duration = Date.now() - state.at;

    if (Math.abs(dy) > 60 && Math.abs(dy) > Math.abs(dx)) {
      if (dy < 0) {
        cycleMode();
      } else {
        describeInDetail();
      }
      return;
    }

    if (duration < 260 && Math.abs(dx) < 30 && Math.abs(dy) < 30) {
      const now = Date.now();
      if (now - state.lastTapAt < 320) {
        repeatLastResponse();
      } else {
        void toggleMic();
      }
      state.lastTapAt = now;
    }
  }, [cycleMode, describeInDetail, repeatLastResponse, toggleMic]);

  return (
    <div className="app-shell" onTouchStart={onTouchStart} onTouchEnd={onTouchEnd}>
      <header className="top-bar">
        <h1>ARIA Navigation Console</h1>
        <div className="status-cluster">
          <span className={`dot ${aria.status}`} />
          <span>{aria.status}</span>
          <span>{micActive ? 'Mic on' : 'Mic off'}</span>
          <span>{wakeLockActive ? 'Wake lock' : 'Fallback keepalive'}</span>
          <span>{hazardDistance}</span>
        </div>
      </header>

      <main className="dashboard">
        <CameraView videoRef={videoRef} connectionStatus={aria.status} motionState={motionSnapshot.state} />
        <aside className="side-panel">
          <ModeIndicator mode={navigationMode} />
          <HazardCompass positionX={hazardPosition} visible={hazardVisible} />
          <TranscriptPanel entries={transcriptEntries} />
        </aside>
      </main>

      <footer className="controls">
        <button type="button" onClick={() => void startSession()} disabled={sessionActive}>
          Start Session
        </button>
        <button type="button" onClick={() => void stopSession()} disabled={!sessionActive}>
          Stop Session
        </button>
        <button type="button" onClick={() => void toggleMic()} disabled={!sessionActive}>
          {micActive ? 'Mic Off' : 'Mic On'}
        </button>
        <button type="button" onClick={cycleMode} disabled={!sessionActive}>
          Next Mode
        </button>
        <button type="button" onClick={repeatLastResponse} disabled={!sessionActive}>
          Repeat
        </button>
      </footer>

      <p className="gesture-hint">
        Tap: mic toggle. Double tap: repeat. Long press: human help. Swipe up: next mode. Swipe down: detailed describe.
      </p>

      <OLEDBlackOverlay enabled={oledBlackMode && sessionActive} />
    </div>
  );
}
