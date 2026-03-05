import { MutableRefObject, useEffect, useMemo, useRef } from 'react';
import { KinematicReading, computeYawDelta, shouldCaptureFrame } from '../services/kinematicGating';
import { HardStopMessage, MotionSnapshot } from '../types/contracts';

interface CameraFramePayload {
  frameBase64: string;
  timestamp: string;
  /** Góc xoay ngang tích lũy (độ) kể từ frame trước → Semantic Odometry */
  yaw_delta_deg: number;
}

interface UseCameraOptions {
  videoRef: MutableRefObject<HTMLVideoElement | null>;
  enabled: boolean;
  motionSnapshot: MotionSnapshot;
  onFrame: (payload: CameraFramePayload) => void;
  onHazard?: (message: HardStopMessage) => void;
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

export function useCamera({ videoRef, enabled, motionSnapshot, onFrame, onHazard }: UseCameraOptions): void {
  const streamRef = useRef<MediaStream | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const edgeCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const workerRef = useRef<Worker | null>(null);
  const kinematicRef = useRef<KinematicReading>({ accel: null, gyro: null });
  const lastCloudPostRef = useRef<number>(Date.now());
  const accumulatedYawRef = useRef<number>(0);
  const lastMotionEventTsRef = useRef<number>(Date.now());

  const intervalMs = useMemo(() => intervalForMotionState(motionSnapshot.state), [motionSnapshot.state]);

  useEffect(() => {
    workerRef.current = new Worker(new URL('../workers/survivalReflex.worker.ts', import.meta.url), { type: 'module' });
    workerRef.current.onmessage = (e) => {
      if (e.data.type === 'CRITICAL_EDGE_HAZARD') {
        onHazard?.({
          type: 'HARD_STOP',
          position_x: e.data.positionX,
          distance: e.data.distance,
          hazard_type: e.data.hazard_type,
          confidence: 0.99
        });
      }
    };
    return () => {
      workerRef.current?.terminate();
      workerRef.current = null;
    };
  }, [onHazard]);

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
      accumulatedYawRef.current += computeYawDelta(e.rotationRate, dtMs);
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
    if (streamRef.current) {
      streamRef.current.getVideoTracks().forEach((track) => {
        track.enabled = motionSnapshot.state !== 'stationary';
      });
    }
  }, [motionSnapshot.state]);

  useEffect(() => {
    if (!enabled) {
      streamRef.current?.getTracks().forEach((track) => track.stop());
      streamRef.current = null;
      if (videoRef.current) {
        videoRef.current.srcObject = null;
      }
      return;
    }

    const EDGE_INTERVAL = 100;
    const timer = window.setInterval(() => {
      const video = videoRef.current;
      if (!video || video.readyState < HTMLMediaElement.HAVE_CURRENT_DATA) {
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
      workerRef.current?.postMessage({ currentFrame: edgeImageData, timestamp: Date.now() });

      const now = Date.now();
      const timeSinceLastPost = now - lastCloudPostRef.current;

      // Kinematic Gating: toán học vector → chỉ chụp khi thẳng đứng và ổn định
      const isKinematicallyStable = shouldCaptureFrame(kinematicRef.current);
      const isTimeout = timeSinceLastPost > intervalMs + 500;
      const isTimeToPost = timeSinceLastPost >= intervalMs;

      if (isTimeToPost && (isKinematicallyStable || isTimeout)) {
        ctx.drawImage(video, sx, sy, sw, sh, 0, 0, 768, 768);
        const dataUrl = canvas.toDataURL('image/jpeg', 0.72);
        const frameBase64 = dataUrl.split(',')[1] ?? '';
        const yawSnapshot = accumulatedYawRef.current;
        accumulatedYawRef.current = 0; // Reset sau khi đã capture
        onFrame({ frameBase64, timestamp: new Date().toISOString(), yaw_delta_deg: yawSnapshot });
        lastCloudPostRef.current = now;
      }
    }, EDGE_INTERVAL);

    return () => {
      window.clearInterval(timer);
    };
  }, [enabled, intervalMs, onFrame, videoRef]);
}
