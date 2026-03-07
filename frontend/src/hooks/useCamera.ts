import { MutableRefObject, useEffect, useMemo, useRef } from 'react';
import { KinematicReading, computeRiskScore, computeYawDelta, shouldCaptureFrame } from '../services/kinematicGating';
import type { CarryMode, CarryModeProfile } from '../services/carryMode';
import { HardStopMessage, MotionSnapshot } from '../types/contracts';

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
}

interface UseCameraOptions {
  videoRef: MutableRefObject<HTMLVideoElement | null>;
  enabled: boolean;
  previewEnabled: boolean;
  motionSnapshot: MotionSnapshot;
  onFrame: (payload: CameraFramePayload) => void;
  onHazard?: (message: HardStopMessage) => void;
  onEdgeHazard?: (message: HardStopMessage) => void;
  onDepthStatus?: (state: 'loading' | 'ready' | 'fallback' | 'error', detail: string) => void;
  locationSnapshot?: LocationSnapshot | null;
  carryMode: CarryMode;
  carryProfile: CarryModeProfile;
  minCloudIntervalMs?: number;
}

function intervalForMotionState(state: MotionSnapshot['state']): number {
  if (state === 'stationary') {
    return 5000;
  }
  if (state === 'running') {
    return 150; // Tăng cường: ~6.6 FPS
  }
  return 250; // Tăng cường: ~4 FPS (trước đây là 1000)
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
  previewEnabled,
  motionSnapshot,
  onFrame,
  onHazard,
  onEdgeHazard,
  onDepthStatus,
  locationSnapshot,
  carryMode,
  carryProfile,
  minCloudIntervalMs = 0,
}: UseCameraOptions): void {
  const streamRef = useRef<MediaStream | null>(null);
  const playbackVideoRef = useRef<HTMLVideoElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const edgeCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const reflexWorkerRef = useRef<Worker | null>(null);
  const depthWorkerRef = useRef<Worker | null>(null);
  const kinematicRef = useRef<KinematicReading>({ accel: null, gyro: null });
  const lastCloudPostRef = useRef<number>(Date.now());
  const accumulatedYawRef = useRef<number>(0);
  const lastMotionEventTsRef = useRef<number>(Date.now());
  const lastYawDeltaDegRef = useRef<number>(0);
  const lastEdgeHazardAtRef = useRef<number>(0);
  const lastDepthHazardAtRef = useRef<number>(0);

  const intervalMs = useMemo(() => intervalForMotionState(motionSnapshot.state), [motionSnapshot.state]);

  useEffect(() => {
    if (!playbackVideoRef.current) {
      const playbackVideo = document.createElement('video');
      playbackVideo.autoplay = true;
      playbackVideo.playsInline = true;
      playbackVideo.muted = true;
      playbackVideoRef.current = playbackVideo;
    }

    return () => {
      if (playbackVideoRef.current) {
        playbackVideoRef.current.pause();
        playbackVideoRef.current.srcObject = null;
      }
      playbackVideoRef.current = null;
    };
  }, []);

  useEffect(() => {
    reflexWorkerRef.current = new Worker(new URL('../workers/survivalReflex.worker.ts', import.meta.url), { type: 'module' });
    depthWorkerRef.current = new Worker(new URL('../workers/depthGuard.worker.ts', import.meta.url));
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

    return () => {
      reflexWorkerRef.current?.terminate();
      depthWorkerRef.current?.terminate();
      reflexWorkerRef.current = null;
      depthWorkerRef.current = null;
    };
  }, [onDepthStatus, onEdgeHazard, onHazard]);

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
        if (playbackVideoRef.current) {
          playbackVideoRef.current.srcObject = stream;
          await playbackVideoRef.current.play();
        }
        if (previewEnabled && videoRef.current) {
          videoRef.current.srcObject = stream;
          await videoRef.current.play().catch(() => {
            // Visible preview is optional for safety operation.
          });
        }
      } catch (error) {
        // Permission denied should be handled by UI prompts.
        console.error('Failed to initialize camera stream.', error);
      }
    })();

    return () => {
      mounted = false;
    };
  }, [enabled, previewEnabled, videoRef]);

  useEffect(() => {
    const stream = streamRef.current;
    const previewVideo = videoRef.current;
    if (!previewVideo) {
      return;
    }

    if (!stream || !previewEnabled) {
      previewVideo.pause();
      previewVideo.srcObject = null;
      return;
    }

    previewVideo.srcObject = stream;
    void previewVideo.play().catch(() => {
      // Preview playback can be blocked without affecting background capture.
    });
  }, [previewEnabled, videoRef]);

  useEffect(() => {
    if (!enabled) {
      streamRef.current?.getTracks().forEach((track) => track.stop());
      streamRef.current = null;
      playbackVideoRef.current?.pause();
      if (playbackVideoRef.current) {
        playbackVideoRef.current.srcObject = null;
      }
      if (videoRef.current) {
        videoRef.current.pause();
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

      const video = playbackVideoRef.current;
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

      const ctx = canvas.getContext('2d');
      const edgeCtx = edgeCanvas.getContext('2d', { willReadFrequently: true });
      if (!ctx || !edgeCtx) {
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
      depthWorkerRef.current?.postMessage({ type: 'depth_frame', currentFrame: edgeImageData, riskScore });

      const now = Date.now();
      const timeSinceLastPost = now - lastCloudPostRef.current;

      // Kinematic Gating: toán học vector → chỉ chụp khi thẳng đứng và ổn định
      const isKinematicallyStable = shouldCaptureFrame(kinematicRef.current, carryProfile);
      const effectiveCloudIntervalMs = Math.max(intervalMs, minCloudIntervalMs);
      const isTimeout = timeSinceLastPost > effectiveCloudIntervalMs + 500;
      const isTimeToPost = timeSinceLastPost >= effectiveCloudIntervalMs;

      if (isTimeToPost && carryProfile.cloudEnabled && (isKinematicallyStable || isTimeout)) {
        ctx.drawImage(video, sx, sy, sw, sh, 0, 0, 768, 768);
        const dataUrl = canvas.toDataURL('image/jpeg', 0.5); // Giảm chất lượng đổi lấy mạng
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
    onFrame,
  ]);
}
