import { MutableRefObject, useEffect, useMemo, useRef } from 'react';
import { KinematicReading, computeRiskScore, computeYawDelta, shouldCaptureFrame } from '../services/kinematicGating';
import type { CarryMode, CarryModeProfile } from '../services/carryMode';
import { HardStopMessage, MotionSnapshot } from '../types/contracts';

// --- Frame Quality Assessment ---

export type FrameQualityFlag = 'too_dark' | 'too_bright' | 'blurry' | 'occluded';

export interface FrameQualityResult {
  score: number; // 0..1, higher = better
  flags: FrameQualityFlag[];
  avgLuma: number;
  blurVariance: number;
}

const DARK_LUMA_THRESHOLD = 25;
const BRIGHT_LUMA_THRESHOLD = 235;
const BLUR_VARIANCE_THRESHOLD = 80;
const OCCLUSION_BLACK_RATIO_THRESHOLD = 0.70;

function assessFrameQuality(imageData: ImageData): FrameQualityResult {
  const { data, width, height } = imageData;
  const pixelCount = width * height;
  let lumaSum = 0;
  let nearBlackCount = 0;
  const lumaValues = new Float32Array(pixelCount);

  for (let i = 0; i < pixelCount; i++) {
    const offset = i * 4;
    const r = data[offset] ?? 0;
    const g = data[offset + 1] ?? 0;
    const b = data[offset + 2] ?? 0;
    const luma = 0.299 * r + 0.587 * g + 0.114 * b;
    lumaValues[i] = luma;
    lumaSum += luma;
    if (luma < 12) {
      nearBlackCount++;
    }
  }

  const avgLuma = lumaSum / pixelCount;

  // Laplacian variance for blur estimation (3x3 kernel on luminance grid)
  let lapSum = 0;
  let lapCount = 0;
  for (let y = 1; y < height - 1; y++) {
    for (let x = 1; x < width - 1; x++) {
      const idx = y * width + x;
      const lap =
        -4 * lumaValues[idx]
        + lumaValues[idx - 1]
        + lumaValues[idx + 1]
        + lumaValues[idx - width]
        + lumaValues[idx + width];
      lapSum += lap * lap;
      lapCount++;
    }
  }
  const blurVariance = lapCount > 0 ? lapSum / lapCount : 0;
  const blackRatio = nearBlackCount / pixelCount;

  const flags: FrameQualityFlag[] = [];
  let score = 1.0;

  if (avgLuma < DARK_LUMA_THRESHOLD) {
    flags.push('too_dark');
    score -= 0.4;
  }
  if (avgLuma > BRIGHT_LUMA_THRESHOLD) {
    flags.push('too_bright');
    score -= 0.3;
  }
  if (blurVariance < BLUR_VARIANCE_THRESHOLD) {
    flags.push('blurry');
    score -= 0.3;
  }
  if (blackRatio > OCCLUSION_BLACK_RATIO_THRESHOLD) {
    flags.push('occluded');
    score -= 0.4;
  }

  return {
    score: Math.max(0, Math.min(1, score)),
    flags,
    avgLuma: Math.round(avgLuma * 10) / 10,
    blurVariance: Math.round(blurVariance),
  };
}

export interface DeterministicScanResult {
  task: 'barcode';
  value: string;
  format?: string;
  confidence: number;
  timestamp: string;
}

interface CameraFramePayload {
  frameBase64: string;
  timestamp: string;
  /** Góc xoay ngang tích lũy (độ) kể từ frame trước → Semantic Odometry */
  yaw_delta_deg: number;
  carry_mode?: CarryMode;
  lat?: number;
  lng?: number;
  heading_deg?: number;
}

export interface LocationSnapshot {
  lat: number;
  lng: number;
  headingDeg?: number;
  accuracyM: number;
  recordedAtMs: number;
}

interface UseCameraOptions {
  videoRef: MutableRefObject<HTMLVideoElement | null>;
  enabled: boolean;
  motionSnapshot: MotionSnapshot;
  onFrame: (payload: CameraFramePayload) => void;
  onHazard?: (message: HardStopMessage) => void;
  onEdgeHazard?: (message: HardStopMessage) => void;
  onDepthStatus?: (state: 'loading' | 'ready' | 'fallback' | 'error', detail: string) => void;
  onDeterministicScan?: (result: DeterministicScanResult) => void;
  onDeterministicStatus?: (state: 'ready' | 'fallback' | 'disabled', detail: string) => void;
  onFrameQuality?: (result: FrameQualityResult) => void;
  locationSnapshot?: LocationSnapshot | null;
  carryMode: CarryMode;
  carryProfile: CarryModeProfile;
  minCloudIntervalMs?: number;
  deterministicScanEnabled?: boolean;
}

function intervalForMotionState(state: MotionSnapshot['state']): number {
  if (state === 'stationary') {
    return 5000;
  }
  if (state === 'running') {
    return 500;
  }
  return 1000;
}

async function getOptimalCameraStream() {
  const devices = await navigator.mediaDevices.enumerateDevices();
  const videoDevices = devices.filter(d => d.kind === 'videoinput');

  const ultrawide = videoDevices.find(d =>
    d.label.toLowerCase().includes('ultra') ||
    d.label.toLowerCase().includes('0.5x') ||
    d.label.toLowerCase().includes('wide') // Fallback a bit broader if ultra not explicitly named
  );

  const constraints: MediaStreamConstraints = {
    video: {
      deviceId: ultrawide ? { exact: ultrawide.deviceId } : undefined,
      facingMode: ultrawide ? undefined : { ideal: 'environment' },
      width: { ideal: 768 },
      height: { ideal: 768 },
      frameRate: { ideal: 10 }
    },
    audio: false,
  };

  return await navigator.mediaDevices.getUserMedia(constraints);
}

export function useCamera({
  videoRef,
  enabled,
  motionSnapshot,
  onFrame,
  onHazard,
  onEdgeHazard,
  onDepthStatus,
  onDeterministicScan,
  onDeterministicStatus,
  onFrameQuality,
  locationSnapshot,
  carryMode,
  carryProfile,
  minCloudIntervalMs = 0,
  deterministicScanEnabled = false,
}: UseCameraOptions): void {
  const streamRef = useRef<MediaStream | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const edgeCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const reflexWorkerRef = useRef<Worker | null>(null);
  const depthWorkerRef = useRef<Worker | null>(null);
  const deterministicWorkerRef = useRef<Worker | null>(null);
  const kinematicRef = useRef<KinematicReading>({ accel: null, gyro: null });
  const lastCloudPostRef = useRef<number>(Date.now());
  const accumulatedYawRef = useRef<number>(0);
  const lastMotionEventTsRef = useRef<number>(Date.now());
  const lastYawDeltaDegRef = useRef<number>(0);
  const lastEdgeHazardAtRef = useRef<number>(0);
  const lastDepthHazardAtRef = useRef<number>(0);
  const deterministicCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const consecutiveBadFramesRef = useRef<number>(0);
  const lastQualityReportRef = useRef<string>('');

  const intervalMs = useMemo(() => intervalForMotionState(motionSnapshot.state), [motionSnapshot.state]);

  useEffect(() => {
    reflexWorkerRef.current = new Worker(new URL('../workers/survivalReflex.worker.ts', import.meta.url), { type: 'module' });
    depthWorkerRef.current = new Worker(new URL('../workers/depthGuard.worker.ts', import.meta.url));
    deterministicWorkerRef.current = new Worker(new URL('../workers/deterministicScan.worker.ts', import.meta.url), { type: 'module' });
    const configuredModelUrl = import.meta.env.VITE_DEPTH_MODEL_URL;
    const depthModelUrl = typeof configuredModelUrl === 'string'
      ? configuredModelUrl
      : '/models/depth_anything_v2_small_fp16.tflite';
    const configuredWasmBase = import.meta.env.VITE_TFLITE_WASM_BASE as string | undefined;
    const defaultWasmBase = `${import.meta.env.BASE_URL}tflite-wasm/`;
    const wasmBaseUrl = (configuredWasmBase && configuredWasmBase.trim()) || defaultWasmBase;

    depthWorkerRef.current.postMessage({
      type: 'init_depth_model',
      modelUrl: depthModelUrl,
      wasmBaseUrl,
    });

    reflexWorkerRef.current.onmessage = (e) => {
      if (e.data.type === 'CRITICAL_EDGE_HAZARD') {
        const now = Date.now();
        if (now - lastEdgeHazardAtRef.current < 1200) {
          return;
        }
        lastEdgeHazardAtRef.current = now;
        const message: HardStopMessage = {
          type: 'HARD_STOP',
          position_x: e.data.positionX,
          distance: e.data.distance,
          hazard_type: e.data.hazard_type,
          confidence: 0.99
        };
        onHazard?.(message);
        onEdgeHazard?.(message);
      }
    };

    depthWorkerRef.current.onmessage = (e) => {
      if (e.data.type === 'DEPTH_STATUS') {
        onDepthStatus?.(e.data.state, String(e.data.detail ?? ''));
        return;
      }
      if (e.data.type === 'DROP_AHEAD_HAZARD') {
        const now = Date.now();
        if (now - lastDepthHazardAtRef.current < 1800) {
          return;
        }
        lastDepthHazardAtRef.current = now;
        const message: HardStopMessage = {
          type: 'HARD_STOP',
          position_x: e.data.positionX ?? 0,
          distance: e.data.distance ?? 'very_close',
          hazard_type: e.data.hazard_type ?? 'DROP_AHEAD',
          confidence: Number(e.data.confidence ?? 0.8),
        };
        onHazard?.(message);
        onEdgeHazard?.(message);
      }
    };
    deterministicWorkerRef.current.onmessage = (e) => {
      if (e.data.type === 'DETERMINISTIC_SCAN_STATUS') {
        onDeterministicStatus?.(e.data.state, String(e.data.detail ?? ''));
        return;
      }
      if (e.data.type === 'DETERMINISTIC_SCAN_RESULT') {
        onDeterministicScan?.({
          task: 'barcode',
          value: String(e.data.value ?? ''),
          format: String(e.data.format ?? ''),
          confidence: Number(e.data.confidence ?? 0.7),
          timestamp: String(e.data.timestamp ?? new Date().toISOString()),
        });
      }
    };

    return () => {
      reflexWorkerRef.current?.terminate();
      depthWorkerRef.current?.terminate();
      deterministicWorkerRef.current?.terminate();
      reflexWorkerRef.current = null;
      depthWorkerRef.current = null;
      deterministicWorkerRef.current = null;
    };
  }, [onDepthStatus, onDeterministicScan, onDeterministicStatus, onEdgeHazard, onHazard]);

  useEffect(() => {
    const handler = (e: DeviceMotionEvent) => {
      const now = Date.now();
      const dtMs = now - lastMotionEventTsRef.current;
      lastMotionEventTsRef.current = now;

      kinematicRef.current = {
        accel: e.accelerationIncludingGravity ?? e.acceleration,
        gyro: e.rotationRate,
      };

      // Tích lũy yaw delta để bơm vào từng frame khi chụp
      const yawDelta = computeYawDelta(e.rotationRate, dtMs);
      lastYawDeltaDegRef.current = yawDelta;
      accumulatedYawRef.current += yawDelta;
    };
    window.addEventListener('devicemotion', handler);
    return () => window.removeEventListener('devicemotion', handler);
  }, []);

  useEffect(() => {
    if (!enabled || streamRef.current) {
      return;
    }

    let mounted = true;
    void (async () => {
      try {
        const stream = await getOptimalCameraStream();

        if (!mounted) {
          stream.getTracks().forEach((track) => track.stop());
          return;
        }

        streamRef.current = stream;
        if (videoRef.current) {
          videoRef.current.srcObject = stream;
          await videoRef.current.play();
        }
      } catch (error) {
        // Permission denied should be handled by UI prompts.
        console.error('Failed to initialize camera stream.', error);
      }
    })();

    return () => {
      mounted = false;
    };
  }, [enabled, videoRef]);

  useEffect(() => {
    if (!enabled) {
      streamRef.current?.getTracks().forEach((track) => track.stop());
      streamRef.current = null;
      if (videoRef.current) {
        videoRef.current.srcObject = null;
      }
      return;
    }

    let timer = 0;
    let cancelled = false;

    const tick = () => {
      if (cancelled) {
        return;
      }

      const video = videoRef.current;
      if (!video || video.readyState < HTMLMediaElement.HAVE_CURRENT_DATA) {
        timer = window.setTimeout(tick, 120);
        return;
      }

      const canvas = canvasRef.current ?? document.createElement('canvas');
      canvasRef.current = canvas;
      canvas.width = 768;
      canvas.height = 768;

      const edgeCanvas = edgeCanvasRef.current ?? document.createElement('canvas');
      edgeCanvasRef.current = edgeCanvas;
      edgeCanvas.width = 64;
      edgeCanvas.height = 64;
      const deterministicCanvas = deterministicCanvasRef.current ?? document.createElement('canvas');
      deterministicCanvasRef.current = deterministicCanvas;
      deterministicCanvas.width = 256;
      deterministicCanvas.height = 256;

      const ctx = canvas.getContext('2d');
      const edgeCtx = edgeCanvas.getContext('2d', { willReadFrequently: true });
      const deterministicCtx = deterministicCanvas.getContext('2d', { willReadFrequently: true });
      if (!ctx || !edgeCtx || !deterministicCtx) {
        return;
      }

      const srcAspect = video.videoWidth / Math.max(video.videoHeight, 1);
      const dstAspect = 1;
      let sx = 0;
      let sy = 0;
      let sw = video.videoWidth;
      let sh = video.videoHeight;

      if (srcAspect > dstAspect) {
        sw = video.videoHeight;
        sx = (video.videoWidth - sw) / 2;
      } else {
        sh = video.videoWidth;
        sy = (video.videoHeight - sh) / 2;
      }

      edgeCtx.drawImage(video, sx, sy, sw, sh, 0, 0, 64, 64);
      const edgeImageData = edgeCtx.getImageData(0, 0, 64, 64);
      const riskScore = computeRiskScore(
        motionSnapshot.state,
        motionSnapshot.pitch,
        motionSnapshot.velocity,
        lastYawDeltaDegRef.current,
      );
      const edgeInterval = Math.max(16, 150 - riskScore * 33);

      reflexWorkerRef.current?.postMessage({ currentFrame: edgeImageData, riskScore });
      const gyro = kinematicRef.current.gyro;
      const gyroMagnitude = Math.sqrt(
        (gyro?.alpha ?? 0) ** 2
        + (gyro?.beta ?? 0) ** 2
        + (gyro?.gamma ?? 0) ** 2,
      );
      depthWorkerRef.current?.postMessage({
        type: 'depth_frame',
        currentFrame: edgeImageData,
        riskScore,
        carryMode,
        gyroMagnitude,
      });

      if (deterministicScanEnabled) {
        deterministicCtx.drawImage(video, sx, sy, sw, sh, 0, 0, 256, 256);
        const deterministicImageData = deterministicCtx.getImageData(0, 0, 256, 256);
        deterministicWorkerRef.current?.postMessage({
          type: 'scan_frame',
          currentFrame: deterministicImageData,
        });
      }

      // Frame quality assessment
      const quality = assessFrameQuality(edgeImageData);
      const qualitySig = `${quality.score.toFixed(2)}-${quality.flags.join(',')}`;
      if (qualitySig !== lastQualityReportRef.current) {
        lastQualityReportRef.current = qualitySig;
        onFrameQuality?.(quality);
      }
      if (quality.score < 0.5) {
        consecutiveBadFramesRef.current++;
      } else {
        consecutiveBadFramesRef.current = 0;
      }

      const now = Date.now();
      const timeSinceLastPost = now - lastCloudPostRef.current;

      // Kinematic Gating: toán học vector → chỉ chụp khi thẳng đứng và ổn định
      const isKinematicallyStable = shouldCaptureFrame(kinematicRef.current, carryProfile);
      const effectiveCloudIntervalMs = Math.max(intervalMs, minCloudIntervalMs);
      const isTimeout = timeSinceLastPost > effectiveCloudIntervalMs + 500;
      const isTimeToPost = timeSinceLastPost >= effectiveCloudIntervalMs;

      if (isTimeToPost && carryProfile.cloudEnabled && (isKinematicallyStable || isTimeout)) {
        ctx.drawImage(video, sx, sy, sw, sh, 0, 0, 768, 768);
        const dataUrl = canvas.toDataURL('image/jpeg', 0.72);
        const frameBase64 = dataUrl.split(',')[1] ?? '';
        const yawSnapshot = accumulatedYawRef.current;
        accumulatedYawRef.current = 0; // Reset sau khi đã capture
        onFrame({
          frameBase64,
          timestamp: new Date().toISOString(),
          yaw_delta_deg: yawSnapshot,
          carry_mode: carryMode,
          lat: locationSnapshot?.lat,
          lng: locationSnapshot?.lng,
          heading_deg: locationSnapshot?.headingDeg,
        });
        lastCloudPostRef.current = now;
      }
      timer = window.setTimeout(tick, edgeInterval);
    };
    tick();

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [
    carryMode,
    carryProfile,
    enabled,
    intervalMs,
    minCloudIntervalMs,
    locationSnapshot,
    motionSnapshot.pitch,
    motionSnapshot.state,
    motionSnapshot.velocity,
    deterministicScanEnabled,
    onFrame,
    videoRef,
  ]);
}
