import { MutableRefObject, useEffect, useMemo, useRef } from 'react';
import { MotionSnapshot } from '../types/contracts';

interface CameraFramePayload {
  frameBase64: string;
  timestamp: string;
}

interface UseCameraOptions {
  videoRef: MutableRefObject<HTMLVideoElement | null>;
  enabled: boolean;
  motionSnapshot: MotionSnapshot;
  onFrame: (payload: CameraFramePayload) => void;
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

export function useCamera({ videoRef, enabled, motionSnapshot, onFrame }: UseCameraOptions): void {
  const streamRef = useRef<MediaStream | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  const intervalMs = useMemo(() => intervalForMotionState(motionSnapshot.state), [motionSnapshot.state]);

  useEffect(() => {
    if (!enabled || streamRef.current) {
      return;
    }

    let mounted = true;
    void (async () => {
      try {
        const stream = await navigator.mediaDevices.getUserMedia({
          video: {
            facingMode: 'environment',
            width: { ideal: 1280 },
            height: { ideal: 720 },
          },
          audio: false,
        });

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

    const timer = window.setInterval(() => {
      const video = videoRef.current;
      if (!video || video.readyState < HTMLMediaElement.HAVE_CURRENT_DATA) {
        return;
      }

      const canvas = canvasRef.current ?? document.createElement('canvas');
      canvasRef.current = canvas;
      canvas.width = 768;
      canvas.height = 768;

      const ctx = canvas.getContext('2d');
      if (!ctx) {
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

      ctx.drawImage(video, sx, sy, sw, sh, 0, 0, 768, 768);
      const dataUrl = canvas.toDataURL('image/jpeg', 0.72);
      const frameBase64 = dataUrl.split(',')[1] ?? '';
      onFrame({ frameBase64, timestamp: new Date().toISOString() });
    }, intervalMs);

    return () => {
      window.clearInterval(timer);
    };
  }, [enabled, intervalMs, onFrame, videoRef]);
}
