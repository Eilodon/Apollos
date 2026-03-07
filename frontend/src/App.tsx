import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { CameraView } from './components/CameraView';
import { HazardCompass } from './components/HazardCompass';
import { ModeIndicator } from './components/ModeIndicator';
import { OLEDBlackOverlay } from './components/OLEDBlackOverlay';
import { TranscriptPanel } from './components/TranscriptPanel';
import { useARIA } from './hooks/useARIA';
import { useAudioStream } from './hooks/useAudioStream';
import { useBatteryGovernor } from './hooks/useBatteryGovernor';
import { useCamera } from './hooks/useCamera';
import { useCarryMode } from './hooks/useCarryMode';
import { useLocationContext } from './hooks/useLocationContext';
import { useMotionSensor } from './hooks/useMotionSensor';
import { usePocketMode } from './hooks/usePocketMode';
import { useSmartCane } from './hooks/useSmartCane';
import { useWakeLock } from './hooks/useWakeLock';
import { AudioCache } from './services/audioCache';
import type { CarryMode } from './services/carryMode';
import { SpatialAudioEngine } from './services/spatialAudioEngine';
import { vibrateHardStop, vibrateReconnect, vibrateSoftConfirm } from './services/haptics';
import { getPlatformCapabilities } from './services/platformDetect';
import type {
  BackendToClientMessage,
  HardStopMessage,
  NavigationMode,
  SemanticCueMessage
} from './types/contracts';
import {
  distanceCategoryToMeters,
  parseHazardType,
} from './types/contracts';

interface TranscriptEntry {
  id: string;
  role: 'assistant' | 'user' | 'system';
  text: string;
  ts: string;
}

const MODE_ORDER: NavigationMode[] = ['NAVIGATION', 'EXPLORE', 'READ', 'QUIET'];
const MAX_TRANSCRIPT_ENTRIES = 200;
const ONBOARDING_KEY = 'apollos_onboarding_completed_v1';
const SESSION_ID_KEY = 'apollos_session_id_v1';
const CLIENT_ID_KEY = 'apollos_client_id_v1';
type PocketOverride = 'auto' | 'force_in' | 'force_out';

const CARRY_MODE_LABELS: Record<CarryMode, string> = {
  hand_held: 'Hand-Held',
  necklace: 'Necklace Mount',
  chest_clip: 'Chest Clip',
  pocket: 'Pocket',
};

function getOrCreateStableId(storageKey: string, prefix: string): string {
  const fallback = `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  try {
    const existing = localStorage.getItem(storageKey);
    if (existing && existing.trim()) {
      return existing;
    }
    const created = globalThis.crypto?.randomUUID?.() ?? fallback;
    localStorage.setItem(storageKey, created);
    return created;
  } catch {
    return globalThis.crypto?.randomUUID?.() ?? fallback;
  }
}

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

function toRadians(value: number): number {
  return (value * Math.PI) / 180;
}

function distanceMeters(lat1: number, lng1: number, lat2: number, lng2: number): number {
  const earthRadiusM = 6_371_000;
  const dLat = toRadians(lat2 - lat1);
  const dLng = toRadians(lng2 - lng1);
  const a = (
    Math.sin(dLat / 2) ** 2
    + Math.cos(toRadians(lat1)) * Math.cos(toRadians(lat2)) * Math.sin(dLng / 2) ** 2
  );
  return earthRadiusM * 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
}

export default function App(): JSX.Element {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const interactiveShellRef = useRef<HTMLDivElement | null>(null);
  const sessionId = useMemo(() => getOrCreateStableId(SESSION_ID_KEY, 'session'), []);
  const clientId = useMemo(() => getOrCreateStableId(CLIENT_ID_KEY, 'client'), []);

  const [navigationMode, setNavigationMode] = useState<NavigationMode>('NAVIGATION');
  const [transcriptEntries, setTranscriptEntries] = useState<TranscriptEntry[]>([]);
  const [sessionActive, setSessionActive] = useState(false);
  const [hazardPosition, setHazardPosition] = useState(0);
  const [hazardVisible, setHazardVisible] = useState(false);
  const [hazardDistance, setHazardDistance] = useState<'very_close' | 'mid' | 'far'>('mid');
  const [onboardingState, setOnboardingState] = useState<'await_carry' | 'pending' | 'running' | 'await_mode' | 'done'>('pending');
  const [pocketOverride, setPocketOverride] = useState<PocketOverride>('auto');
  const [pocketSensorUnavailable, setPocketSensorUnavailable] = useState(false);
  const [indoorLikely, setIndoorLikely] = useState(false);

  const audioCacheRef = useRef(new AudioCache(3));
  const spatialRef = useRef<SpatialAudioEngine | null>(null);
  const platformCapabilities = useMemo(() => getPlatformCapabilities(), []);
  const { carryMode, activeCarryMode, profile: carryProfile, setCarryMode } = useCarryMode();
  const lowBatteryHandledRef = useRef(false);
  const highDischargeHandledRef = useRef(false);
  const indoorThrottleHandledRef = useRef(false);
  // eslint-disable-next-line react-hooks/purity
  const lastActivityAtRef = useRef(Date.now());
  const lastHeartbeatAtRef = useRef(0);
  const indoorAnchorRef = useRef<{ lat: number; lng: number; sinceMs: number } | null>(null);
  const lastDepthStatusRef = useRef('');
  const lastSmartCaneErrorRef = useRef('');
  const lastHardStopAtRef = useRef(0);

  const {
    motionSnapshot,
    permissionRequired,
    requestMotionPermission,
    shakeSignal,
  } = useMotionSensor();

  const { oledBlackMode, wakeLockActive, activateNavigationMode, deactivateNavigationMode } = useWakeLock();
  const onPocketModeActive = useCallback(() => {
    spatialRef.current?.fireSemanticCue('pocket_mode_active', 0);
  }, []);
  const onPocketSensorUnavailable = useCallback((reason: string) => {
    setPocketSensorUnavailable(true);
    setTranscriptEntries((prev) => {
      const message = `Pocket Shield fallback active (${reason}).`;
      if (prev.some((entry) => entry.role === 'system' && entry.text === message)) {
        return prev;
      }
      const updated = [...prev, createEntry('system', message)];
      return updated.length > MAX_TRANSCRIPT_ENTRIES ? updated.slice(-MAX_TRANSCRIPT_ENTRIES) : updated;
    });
  }, []);
  const { inPocket, sensorAvailable: pocketSensorAvailable } = usePocketMode({
    onPocketModeActive,
    onSensorUnavailable: onPocketSensorUnavailable,
    manualOverride: pocketOverride,
  });
  const overlayActive = (oledBlackMode || inPocket) && sessionActive;
  const locationSnapshot = useLocationContext(sessionActive);
  const battery = useBatteryGovernor(sessionActive);
  const {
    supported: smartCaneSupported,
    connected: smartCaneConnected,
    connecting: smartCaneConnecting,
    deviceName: smartCaneName,
    lastError: smartCaneLastError,
    connect: connectSmartCane,
    disconnect: disconnectSmartCane,
    sendDirectional,
    sendHazardPattern,
  } = useSmartCane();
  const minCloudIntervalMs = useMemo(() => {
    let minInterval = 0;
    if (battery.highDischarge) {
      minInterval = Math.max(minInterval, 2000); // 0.5 FPS cap
    }
    if (indoorLikely) {
      minInterval = Math.max(minInterval, 3333); // ~0.3 FPS in stable indoor conditions
    }
    if (battery.lowBattery) {
      minInterval = Math.max(minInterval, 3333);
    }
    return minInterval;
  }, [battery.highDischarge, battery.lowBattery, indoorLikely]);

  const onBackendMessage = useCallback((message: BackendToClientMessage) => {
    lastActivityAtRef.current = Date.now();
    if (message.type === 'assistant_text') {
      setTranscriptEntries((prev) => {
        const updated = [...prev, createEntry('assistant', message.text)];
        return updated.length > MAX_TRANSCRIPT_ENTRIES ? updated.slice(-MAX_TRANSCRIPT_ENTRIES) : updated;
      });
      audioCacheRef.current.add({ timestampMs: message.timestamp_ms, text: message.text });
      return;
    }

    if (message.type === 'audio_chunk') {
      const pcmBase64 = message.pcm24 || message.pcm16;
      if (!pcmBase64 || !spatialRef.current) {
        return;
      }

      spatialRef.current.playChunkFromBase64(pcmBase64, message.hazard_position_x ?? hazardPosition);
      audioCacheRef.current.add({ timestampMs: message.timestamp_ms, pcmBase64 });
      return;
    }

    if (message.type === 'semantic_cue') {
      if (!spatialRef.current) {
        return;
      }
      const cueMessage = message as SemanticCueMessage;
      spatialRef.current.fireSemanticCue(cueMessage.cue, cueMessage.position_x ?? hazardPosition);
      return;
    }

    if (message.type === 'connection_state') {
      if (message.state === 'reconnecting') {
        vibrateReconnect();
      }
      if (message.state === 'degraded' && message.detail) {
        setTranscriptEntries((prev) => {
          const updated = [...prev, createEntry('system', `Backend degraded: ${message.detail}`)];
          return updated.length > MAX_TRANSCRIPT_ENTRIES ? updated.slice(-MAX_TRANSCRIPT_ENTRIES) : updated;
        });
      }
      return;
    }

    if (message.type === 'safety_directive') {
      if (message.flush_audio) {
        spatialRef.current?.stopAll();
      }
      if (message.needs_human_assistance) {
        setTranscriptEntries((prev) => {
          const updated = [...prev, createEntry('system', 'Human assistance escalation recommended.')];
          return updated.length > MAX_TRANSCRIPT_ENTRIES ? updated.slice(-MAX_TRANSCRIPT_ENTRIES) : updated;
        });
      }
      return;
    }

    if (message.type === 'human_help_session') {
      setTranscriptEntries((prev) => {
        const provider = message.rtc.provider.toUpperCase();
        const updated = [...prev, createEntry('system', `Human help session ready via ${provider}.`)];
        return updated.length > MAX_TRANSCRIPT_ENTRIES ? updated.slice(-MAX_TRANSCRIPT_ENTRIES) : updated;
      });
      return;
    }

    if (message.type === 'cognition_state' && message.reason) {
      setTranscriptEntries((prev) => {
        const updated = [...prev, createEntry('system', `Cognition layer ${message.active_layer}: ${message.reason}`)];
        return updated.length > MAX_TRANSCRIPT_ENTRIES ? updated.slice(-MAX_TRANSCRIPT_ENTRIES) : updated;
      });
    }
  }, [hazardPosition]);

  const onHardStop = useCallback((message: HardStopMessage) => {
    const now = Date.now();
    if (now - lastHardStopAtRef.current < 600) return; // Debounce: chống double-fire Edge+Cloud
    lastHardStopAtRef.current = now;

    lastActivityAtRef.current = now;
    setHazardVisible(true);
    setHazardPosition(message.position_x);
    setHazardDistance(message.distance);

    spatialRef.current?.fireHardStop(message.position_x, message.distance);
    vibrateHardStop();
    if (smartCaneConnected) {
      sendHazardPattern('hard');
      if (message.position_x > 0.3) {
        sendDirectional('right', 0.8);
      } else if (message.position_x < -0.3) {
        sendDirectional('left', 0.8);
      } else {
        sendDirectional('stop', 0.6);
      }
    }

    setTranscriptEntries((prev) => {
      const updated = [...prev, createEntry('system', `STOP: ${message.hazard_type} (${message.distance})`)];
      return updated.length > MAX_TRANSCRIPT_ENTRIES ? updated.slice(-MAX_TRANSCRIPT_ENTRIES) : updated;
    });
  }, [sendDirectional, sendHazardPattern, smartCaneConnected]);

  const {
    status: ariaStatus,
    connect: ariaConnect,
    disconnect: ariaDisconnect,
    sendFrame,
    sendAudioChunk,
    sendUserCommand,
    sendEdgeHazard,
  } = useARIA({
    sessionId,
    clientId,
    onBackendMessage,
    onHardStop,
  });

  const onEdgeHazard = useCallback((message: HardStopMessage) => {
    const distanceM = distanceCategoryToMeters(message.distance);
    const relativeVelocityMps = (() => {
      if (message.hazard_type.toLowerCase().includes('vehicle')) {
        return -2.4;
      }
      if (message.distance === 'very_close') {
        return -1.6;
      }
      if (message.distance === 'mid') {
        return -1.0;
      }
      return -0.55;
    })();

    sendEdgeHazard({
      hazard_type: parseHazardType(message.hazard_type),
      bearing_x: message.position_x,
      distance_m: distanceM,
      relative_velocity_mps: relativeVelocityMps,
      confidence: message.confidence,
      source: 'web_edge_reflex',
      suppress_ms: 2500,
    });
  }, [sendEdgeHazard]);

  const { micActive, startMic, stopMic, toggleMic } = useAudioStream({
    onAudioChunk: sendAudioChunk,
  });

  const onDepthStatus = useCallback((state: 'loading' | 'ready' | 'fallback' | 'error', detail: string) => {
    const signature = `${state}:${detail}`;
    if (signature === lastDepthStatusRef.current) {
      return;
    }
    lastDepthStatusRef.current = signature;
    if (state === 'ready') {
      setTranscriptEntries((prev) => {
        const updated = [...prev, createEntry('system', 'Depth model ready for drop detection.')];
        return updated.length > MAX_TRANSCRIPT_ENTRIES ? updated.slice(-MAX_TRANSCRIPT_ENTRIES) : updated;
      });
    } else if (state === 'fallback') {
      setTranscriptEntries((prev) => {
        const updated = [...prev, createEntry('system', 'Depth model unavailable, using heuristic fallback.')];
        return updated.length > MAX_TRANSCRIPT_ENTRIES ? updated.slice(-MAX_TRANSCRIPT_ENTRIES) : updated;
      });
    } else {
      setTranscriptEntries((prev) => {
        const updated = [...prev, createEntry('system', `Depth pipeline error: ${detail}`)];
        return updated.length > MAX_TRANSCRIPT_ENTRIES ? updated.slice(-MAX_TRANSCRIPT_ENTRIES) : updated;
      });
    }
  }, []);

  // TODO: KRONOS-CRITICAL: Giới hạn Sinh tồn cho TFJS
  // WebAssembly chạy tfjs-tflite (YOLO/Depth) hoàn toàn bằng CPU, sẽ dẫn đến Thermal Throttling và sập pin nhanh.
  // Nếu phải dùng Web làm Fallback, bắt buộc viết mạch Throttling giới hạn max 2 FPS. Không được dùng cho Survival Mode.
  useCamera({
    videoRef,
    enabled: sessionActive && ariaStatus === 'connected',
    previewEnabled: !overlayActive,
    motionSnapshot,
    onHazard: onHardStop,
    onEdgeHazard,
    onDepthStatus,
    locationSnapshot,
    carryMode: activeCarryMode,
    carryProfile,
    minCloudIntervalMs,
    onFrame: ({ frameBase64, timestamp_ms, yaw_delta_deg, carry_mode }) => {
      sendFrame({
        timestamp_ms,
        frame_jpeg_base64: frameBase64,
        motion_state: motionSnapshot.state,
        pitch: motionSnapshot.pitch,
        velocity: motionSnapshot.velocity,
        yaw_delta_deg,
        carry_mode,
        sensor_unavailable: !pocketSensorAvailable,
        lat: locationSnapshot?.lat,
        lng: locationSnapshot?.lng,
        heading_deg: locationSnapshot?.headingDeg,
        edge_semantic_cues: [],
      });
    },
  });

  const appendSystemEntry = useCallback((text: string) => {
    setTranscriptEntries((prev) => {
      const updated = [...prev, createEntry('system', text)];
      return updated.length > MAX_TRANSCRIPT_ENTRIES ? updated.slice(-MAX_TRANSCRIPT_ENTRIES) : updated;
    });
  }, []);

  const sleep = useCallback((ms: number) => new Promise<void>((resolve) => {
    window.setTimeout(resolve, ms);
  }), []);

  const completeOnboarding = useCallback((preferredMode: NavigationMode) => {
    setNavigationMode(preferredMode);
    sendUserCommand(`set_navigation_mode:${preferredMode}`);
    localStorage.setItem(ONBOARDING_KEY, '1');
    setOnboardingState('done');
    appendSystemEntry(`Onboarding completed. Mode set to ${preferredMode}.`);
  }, [appendSystemEntry, sendUserCommand]);

  const selectCarryMode = useCallback((mode: CarryMode) => {
    setCarryMode(mode);
    setPocketOverride(mode === 'pocket' && !pocketSensorAvailable ? 'force_in' : 'auto');
    appendSystemEntry(`Carry mode set: ${CARRY_MODE_LABELS[mode]}.`);
    if (mode === 'pocket') {
      appendSystemEntry('Pocket mode active: cloud frame streaming paused until carry mode changes.');
    }
    if (localStorage.getItem(ONBOARDING_KEY) === '1') {
      setOnboardingState('done');
    } else {
      setOnboardingState('pending');
    }
  }, [appendSystemEntry, pocketSensorAvailable, setCarryMode]);

  const runTrustProtocol = useCallback(async () => {
    if (onboardingState !== 'pending' || !sessionActive) {
      return;
    }
    setOnboardingState('running');
    appendSystemEntry('Onboarding 90s: ARIA trust protocol started.');

    appendSystemEntry('Xin chao, toi la ARIA. Toi dang nhin qua camera cua ban.');
    sendUserCommand('describe_detailed');
    await sleep(9000);

    appendSystemEntry('Hay gio tay truoc camera de toi xac nhan khoang cach.');
    sendUserCommand('describe_hand_distance');
    await sleep(18000);

    appendSystemEntry('Dang calibration am thanh dinh huong: trai -> phai -> giua.');
    spatialRef.current?.fireSemanticCue('turning_recommended', -1);
    await sleep(1000);
    spatialRef.current?.fireSemanticCue('turning_recommended', 1);
    await sleep(1000);
    spatialRef.current?.fireSemanticCue('destination_near', 0);
    await sleep(1500);

    appendSystemEntry('Chon cach ARIA se noi: NAVIGATION (nhieu huong dan) hoac QUIET (chi khi critical).');
    setOnboardingState('await_mode');
  }, [appendSystemEntry, sendUserCommand, onboardingState, sessionActive, sleep]);

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

    ariaConnect();
    await startMic();

    setSessionActive(true);
    appendSystemEntry('ARIA session started.');

    if (!carryMode) {
      setOnboardingState('await_carry');
      appendSystemEntry('Step 0: Choose how you are carrying the phone before trust protocol.');
      return;
    }
    if (localStorage.getItem(ONBOARDING_KEY) === '1') {
      setOnboardingState('done');
    } else {
      setOnboardingState('pending');
    }
  }, [
    activateNavigationMode,
    appendSystemEntry,
    ariaConnect,
    carryMode,
    permissionRequired,
    requestMotionPermission,
    sessionActive,
    startMic,
  ]);

  const stopSession = useCallback(async () => {
    if (!sessionActive) {
      return;
    }

    stopMic();
    ariaDisconnect();
    spatialRef.current?.stopAll();
    await deactivateNavigationMode();

    setSessionActive(false);
    setHazardVisible(false);
    appendSystemEntry('ARIA session stopped.');
  }, [ariaDisconnect, deactivateNavigationMode, appendSystemEntry, sessionActive, stopMic]);

  const cycleMode = useCallback(() => {
    const updated = nextMode(navigationMode);
    setNavigationMode(updated);
    sendUserCommand(`set_navigation_mode:${updated}`);
    appendSystemEntry(`Mode switched to ${updated}.`);
    vibrateSoftConfirm();
  }, [appendSystemEntry, sendUserCommand, navigationMode]);

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
    sendUserCommand('request_human_help');
    appendSystemEntry('Human help requested.');
    vibrateHardStop();
  }, [appendSystemEntry, sendUserCommand]);

  const describeInDetail = useCallback(() => {
    sendUserCommand('describe_detailed');
    appendSystemEntry('Requested detailed description.');
  }, [appendSystemEntry, sendUserCommand]);

  const handleSmartCaneConnect = useCallback(async () => {
    const connected = await connectSmartCane();
    if (connected) {
      appendSystemEntry('Smart cane connected. Haptic bridge enabled.');
    }
  }, [appendSystemEntry, connectSmartCane]);

  const handleSmartCaneDisconnect = useCallback(() => {
    disconnectSmartCane();
    appendSystemEntry('Smart cane disconnected.');
  }, [appendSystemEntry, disconnectSmartCane]);

  useEffect(() => {
    if (!sessionActive || shakeSignal === 0) {
      return;
    }

    sendUserCommand('sos');
    appendSystemEntry('Shake detected. SOS sent.');
    vibrateHardStop();
  }, [appendSystemEntry, sendUserCommand, sessionActive, shakeSignal]);

  useEffect(() => {
    if (!sessionActive || !locationSnapshot) {
      setIndoorLikely(false);
      indoorThrottleHandledRef.current = false;
      indoorAnchorRef.current = null;
      return;
    }

    const current = {
      lat: locationSnapshot.lat,
      lng: locationSnapshot.lng,
      sinceMs: Date.now(),
    };
    const anchor = indoorAnchorRef.current;
    if (!anchor) {
      indoorAnchorRef.current = current;
      setIndoorLikely(false);
      return;
    }

    const movedMeters = distanceMeters(anchor.lat, anchor.lng, current.lat, current.lng);
    if (movedMeters > 12) {
      indoorAnchorRef.current = current;
      setIndoorLikely(false);
      return;
    }

    const elapsedMs = Date.now() - anchor.sinceMs;
    const likelyIndoor = elapsedMs >= 90_000 && movedMeters <= 4;
    setIndoorLikely(likelyIndoor);
  }, [locationSnapshot, sessionActive]);

  useEffect(() => {
    if (!sessionActive) {
      return;
    }
    if (indoorLikely && !indoorThrottleHandledRef.current) {
      indoorThrottleHandledRef.current = true;
      appendSystemEntry('Indoor/stable location detected. Reducing cloud frame rate to save battery.');
      return;
    }
    if (!indoorLikely && indoorThrottleHandledRef.current) {
      indoorThrottleHandledRef.current = false;
      appendSystemEntry('Mobility increased. Restoring normal cloud frame budget.');
    }
  }, [appendSystemEntry, indoorLikely, sessionActive]);

  useEffect(() => {
    if (!sessionActive) {
      return;
    }

    if (battery.highDischarge && !highDischargeHandledRef.current) {
      highDischargeHandledRef.current = true;
      appendSystemEntry('Power drain high detected. Reducing cloud frame rate for thermal safety.');
    }
    if (!battery.highDischarge && highDischargeHandledRef.current) {
      highDischargeHandledRef.current = false;
      appendSystemEntry('Power drain normalized. Restoring normal cloud frame budget.');
    }
  }, [appendSystemEntry, battery.highDischarge, sessionActive]);

  useEffect(() => {
    if (!sessionActive || !battery.lowBattery || lowBatteryHandledRef.current) {
      return;
    }

    lowBatteryHandledRef.current = true;
    appendSystemEntry('Pin còn 20%, tôi chuyển sang chế độ tiết kiệm năng lượng.');
    if (navigationMode !== 'QUIET') {
      setNavigationMode('QUIET');
      sendUserCommand('set_navigation_mode:QUIET');
    }
  }, [appendSystemEntry, battery.lowBattery, navigationMode, sendUserCommand, sessionActive]);

  useEffect(() => {
    if (!battery.lowBattery) {
      lowBatteryHandledRef.current = false;
    }
  }, [battery.lowBattery]);

  useEffect(() => {
    const completed = localStorage.getItem(ONBOARDING_KEY) === '1';
    if (completed && carryMode) {
      setOnboardingState('done');
    }
  }, [carryMode]);

  useEffect(() => {
    if (pocketSensorAvailable) {
      setPocketSensorUnavailable(false);
    }
  }, [pocketSensorAvailable]);

  useEffect(() => {
    if (!sessionActive || onboardingState !== 'pending') {
      return;
    }
    void runTrustProtocol();
  }, [onboardingState, runTrustProtocol, sessionActive]);

  useEffect(() => {
    if (ariaStatus === 'connected') {
      lastActivityAtRef.current = Date.now();
      appendSystemEntry('Connected to backend.');
    }
    if (ariaStatus === 'reconnecting') {
      appendSystemEntry('Reconnecting. Please stay still.');
    }
  }, [appendSystemEntry, ariaStatus]);

  useEffect(() => {
    if (!smartCaneLastError || smartCaneLastError === lastSmartCaneErrorRef.current) {
      return;
    }
    lastSmartCaneErrorRef.current = smartCaneLastError;
    appendSystemEntry(`Smart cane bridge error: ${smartCaneLastError}`);
  }, [appendSystemEntry, smartCaneLastError]);

  useEffect(() => {
    if (!sessionActive || ariaStatus !== 'connected') {
      return;
    }

    const timer = window.setInterval(() => {
      const now = Date.now();
      const idleMs = now - lastActivityAtRef.current;
      const sinceHeartbeatMs = now - lastHeartbeatAtRef.current;
      if (idleMs < 15_000 || sinceHeartbeatMs < 14_000) {
        return;
      }
      spatialRef.current?.fireHeartbeatPing();
      lastHeartbeatAtRef.current = now;
      lastActivityAtRef.current = now;
    }, 1000);

    return () => {
      window.clearInterval(timer);
    };
  }, [ariaStatus, sessionActive]);

  useEffect(() => {
    if (!interactiveShellRef.current) {
      return;
    }
    interactiveShellRef.current.inert = overlayActive;
    return () => {
      if (interactiveShellRef.current) {
        interactiveShellRef.current.inert = false;
      }
    };
  }, [overlayActive]);

  return (
    <div className="app-shell">
      <div className="app-live-region sr-only" aria-live="polite" aria-atomic="true">
        {overlayActive ? 'Screen guard active. Camera preview hidden and touch input locked.' : ''}
      </div>

      <div className="app-interactive-shell" ref={interactiveShellRef}>
        <header className="top-bar">
          <h1>ARIA Navigation Console</h1>
          <div className="status-cluster" role="status" aria-live="polite" aria-label="System status">
            <span className={`dot ${ariaStatus}`} aria-hidden="true" />
            <span aria-label={`Connection: ${ariaStatus}`}>{ariaStatus}</span>
            <span aria-label={micActive ? 'Microphone is on' : 'Microphone is off'}>{micActive ? 'Mic on' : 'Mic off'}</span>
            <span>{wakeLockActive ? 'Wake lock' : 'Fallback keepalive'}</span>
            <span>Carry: {CARRY_MODE_LABELS[activeCarryMode]}</span>
            <span>Safety: {platformCapabilities.safetyGrade}</span>
            {battery.levelPercent !== null && <span aria-label={`Battery ${battery.levelPercent.toFixed(0)} percent`}>Battery: {battery.levelPercent.toFixed(1)}%</span>}
            {battery.dischargeRatePerMin !== null && <span>Drain: {battery.dischargeRatePerMin.toFixed(2)}%/min</span>}
            {minCloudIntervalMs > 0 && <span>Power save FPS cap</span>}
            {pocketSensorUnavailable && <span>Pocket sensor fallback</span>}
            {smartCaneSupported && <span>Cane: {smartCaneConnected ? (smartCaneName || 'Connected') : 'Disconnected'}</span>}
            <span aria-label={`Hazard distance: ${hazardDistance}`}>{hazardDistance}</span>
          </div>
        </header>

        {platformCapabilities.recommendNative && (
          <section className="platform-banner" role="status" aria-live="polite">
            iOS Safari detected. Apollos safety features are reduced on this platform. Android Chrome is recommended for full protection.
          </section>
        )}

        <main className="dashboard">
          <CameraView
            videoRef={videoRef}
            connectionStatus={ariaStatus}
            motionState={motionSnapshot.state}
            previewVisible={!overlayActive}
          />
          <aside className="side-panel">
            <ModeIndicator mode={navigationMode} />
            <HazardCompass positionX={hazardPosition} visible={hazardVisible} distance={hazardDistance} />
            <TranscriptPanel entries={transcriptEntries} />
          </aside>
        </main>

        <footer className="controls" role="toolbar" aria-label="Session controls">
          <button type="button" onClick={() => void startSession()} disabled={sessionActive} aria-label="Start ARIA navigation session">
            Start Session
          </button>
          <button type="button" onClick={() => void stopSession()} disabled={!sessionActive} aria-label="Stop ARIA navigation session">
            Stop Session
          </button>
          <button type="button" onClick={() => void toggleMic()} disabled={!sessionActive} aria-label={micActive ? 'Turn microphone off' : 'Turn microphone on'}>
            {micActive ? 'Mic Off' : 'Mic On'}
          </button>
          <button type="button" onClick={cycleMode} disabled={!sessionActive} aria-label={`Switch navigation mode. Current: ${navigationMode}`}>
            Next Mode
          </button>
          <button type="button" onClick={repeatLastResponse} disabled={!sessionActive} aria-label="Repeat last ARIA response">
            Repeat
          </button>
          <button type="button" onClick={describeInDetail} disabled={!sessionActive} aria-label="Request a detailed scene description">
            Describe
          </button>
          <button type="button" onClick={requestHumanHelp} disabled={!sessionActive} aria-label="Request human help immediately">
            Human Help
          </button>
          <button
            type="button"
            onClick={() => {
              setPocketOverride((prev) => (prev === 'auto' ? 'force_in' : 'auto'));
              appendSystemEntry(pocketOverride === 'auto' ? 'Pocket shield forced on.' : 'Pocket shield returned to automatic control.');
            }}
            disabled={!sessionActive}
            aria-label={pocketOverride === 'auto' ? 'Force pocket shield on' : 'Return pocket shield to automatic mode'}
          >
            {pocketOverride === 'auto' ? 'Pocket Shield On' : 'Pocket Shield Auto'}
          </button>
          <button
            type="button"
            onClick={() => {
              setPocketOverride('force_out');
              appendSystemEntry('Pocket shield manually released.');
            }}
            disabled={!sessionActive}
            aria-label="Force pocket shield off and resume controls"
          >
            Resume Controls
          </button>
          {smartCaneSupported && (
            <button
              type="button"
              onClick={smartCaneConnected ? handleSmartCaneDisconnect : () => void handleSmartCaneConnect()}
              disabled={!sessionActive || smartCaneConnecting}
              aria-label={smartCaneConnected ? 'Disconnect smart cane' : 'Connect smart cane via Bluetooth'}
            >
              {smartCaneConnected ? 'Disconnect Cane' : smartCaneConnecting ? 'Connecting Cane...' : 'Connect Cane'}
            </button>
          )}
        </footer>

        {sessionActive && onboardingState === 'await_carry' && (
          <div className="controls carry-controls" role="group" aria-label="Phone carry mode selection">
            <span className="carry-title">Step 0: Phone Carry Mode</span>
            <button type="button" onClick={() => selectCarryMode('necklace')} aria-label="Set carry mode to necklace, recommended">
              Necklace (Recommended)
            </button>
            <button type="button" onClick={() => selectCarryMode('chest_clip')} aria-label="Set carry mode to chest clip">
              Chest Clip
            </button>
            <button type="button" onClick={() => selectCarryMode('hand_held')} aria-label="Set carry mode to hand held">
              Hand-Held
            </button>
            <button type="button" onClick={() => selectCarryMode('pocket')} aria-label="Set carry mode to pocket">
              Pocket
            </button>
          </div>
        )}

        {sessionActive && onboardingState === 'await_mode' && (
          <div className="controls" role="group" aria-label="Onboarding mode selection">
            <button type="button" onClick={() => completeOnboarding('NAVIGATION')} aria-label="Complete onboarding with navigation mode, full guidance">
              Onboarding: NAVIGATION
            </button>
            <button type="button" onClick={() => completeOnboarding('QUIET')} aria-label="Complete onboarding with quiet mode, critical alerts only">
              Onboarding: QUIET
            </button>
            <button type="button" onClick={() => completeOnboarding('NAVIGATION')} aria-label="Skip onboarding and use navigation mode">
              Skip
            </button>
          </div>
        )}

        <p className="gesture-hint">
          Use the controls below. With TalkBack or VoiceOver, focus a control and double tap to activate it.
        </p>
      </div>

      {sessionActive && inPocket && (
        <button
          type="button"
          className="overlay-escape-button"
          onClick={() => {
            setPocketOverride('force_out');
            appendSystemEntry('Pocket shield manually released from overlay.');
          }}
          aria-label="Resume controls and unlock screen guard"
        >
          Resume Controls
        </button>
      )}

      <OLEDBlackOverlay enabled={overlayActive} />
    </div>
  );
}
