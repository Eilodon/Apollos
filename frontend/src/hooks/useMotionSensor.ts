import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { MotionSnapshot, MotionState } from '../types/contracts';

type DeviceMotionPermissionEvent = typeof DeviceMotionEvent & {
  requestPermission?: () => Promise<'granted' | 'denied'>;
};

interface UseMotionSensorResult {
  motionSnapshot: MotionSnapshot;
  permissionRequired: boolean;
  requestMotionPermission: () => Promise<boolean>;
  shakeSignal: number;
}

function classifyMotion(avgMagnitude: number): MotionState {
  if (avgMagnitude < 9.9) {
    return 'stationary';
  }
  if (avgMagnitude < 11) {
    return 'walking_slow';
  }
  if (avgMagnitude < 14) {
    return 'walking_fast';
  }
  return 'running';
}

export function useMotionSensor(): UseMotionSensorResult {
  const [motionSnapshot, setMotionSnapshot] = useState<MotionSnapshot>({
    state: 'stationary',
    pitch: 0,
    velocity: 0,
  });
  const [shakeSignal, setShakeSignal] = useState(0);

  const accelHistory = useRef<number[]>([]);
  const latestPitch = useRef(0);
  const latestVelocity = useRef(0);
  const shakeCooldownUntil = useRef(0);

  const permissionRequired = useMemo(() => {
    const withPermission = DeviceMotionEvent as DeviceMotionPermissionEvent;
    return typeof withPermission.requestPermission === 'function';
  }, []);

  useEffect(() => {
    const onMotion = (event: DeviceMotionEvent): void => {
      const acceleration = event.accelerationIncludingGravity;
      if (!acceleration) {
        return;
      }

      const ax = acceleration.x ?? 0;
      const ay = acceleration.y ?? 0;
      const az = acceleration.z ?? 0;
      const magnitude = Math.sqrt(ax ** 2 + ay ** 2 + az ** 2);

      accelHistory.current.push(magnitude);
      if (accelHistory.current.length > 20) {
        accelHistory.current.shift();
      }

      latestVelocity.current = Math.max(0, magnitude - 9.8);
      const horizontalNorm = Math.sqrt(ax ** 2 + az ** 2) || 1;
      latestPitch.current = Math.atan2(ay, horizontalNorm) * (180 / Math.PI);

      const now = Date.now();
      if (magnitude > 22 && now > shakeCooldownUntil.current) {
        shakeCooldownUntil.current = now + 5000;
        setShakeSignal((prev) => prev + 1);
      }
    };

    window.addEventListener('devicemotion', onMotion, { passive: true });
    return () => {
      window.removeEventListener('devicemotion', onMotion);
    };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => {
      const history = accelHistory.current;
      const avgMagnitude =
        history.length > 0 ? history.reduce((acc, value) => acc + value, 0) / history.length : 0;
      const state = classifyMotion(avgMagnitude);
      setMotionSnapshot({
        state,
        pitch: latestPitch.current,
        velocity: latestVelocity.current,
      });
    }, 500);

    return () => {
      window.clearInterval(timer);
    };
  }, []);

  const requestMotionPermission = useCallback(async (): Promise<boolean> => {
    const withPermission = DeviceMotionEvent as DeviceMotionPermissionEvent;
    if (typeof withPermission.requestPermission !== 'function') {
      return true;
    }

    const permission = await withPermission.requestPermission();
    return permission === 'granted';
  }, []);

  return {
    motionSnapshot,
    permissionRequired,
    requestMotionPermission,
    shakeSignal,
  };
}
