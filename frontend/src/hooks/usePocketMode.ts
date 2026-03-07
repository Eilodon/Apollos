import { useEffect, useRef, useState } from 'react';

interface DeviceProximityEventLike extends Event {
  value?: number;
  min?: number;
  max?: number;
}

interface UsePocketModeOptions {
  onPocketModeActive?: () => void;
  onSensorUnavailable?: (reason: string) => void;
  manualOverride?: 'auto' | 'force_in' | 'force_out';
}

interface UsePocketModeResult {
  inPocket: boolean;
  sensorAvailable: boolean;
}

/**
 * Layer 0.5 - Pocket-Safe UI (Ghost Touch Prevention)
 *
 * TODO: KRONOS-CRITICAL: Lớp Phòng Thủ Bọc Lót iOS
 * API deviceproximity và AmbientLightSensor không được hỗ trợ trên iOS/Safari.
 * Cần sử dụng thuật toán gia tốc/con quay hồi chuyển (từ useMotionSensor.ts) làm Fallback.
 * Nếu môi trường tối thui (camera đen) VÀ gia tốc hướng xuống đất -> 90% đang ở trong túi.
 *
 * Priority:
 * 1. AmbientLightSensor (< 5 lux => in pocket)
 * 2. deviceproximity (if available)
 * 3. Manual override from UI
 */
export function usePocketMode({
  onPocketModeActive,
  onSensorUnavailable,
  manualOverride = 'auto',
}: UsePocketModeOptions = {}): UsePocketModeResult {
  const [inPocket, setInPocket] = useState(manualOverride === 'force_in');
  const [sensorAvailable, setSensorAvailable] = useState(false);
  const sensorPocketRef = useRef(false);
  const manualOverrideRef = useRef(manualOverride);
  const announcedRef = useRef(false);
  const unavailableReasonRef = useRef('');

  const applyResolvedPocketState = (sensorPocket: boolean, override: 'auto' | 'force_in' | 'force_out') => {
    if (override === 'force_in') {
      return true;
    }
    if (override === 'force_out') {
      return false;
    }
    return sensorPocket;
  };

  useEffect(() => {
    manualOverrideRef.current = manualOverride;
    const next = applyResolvedPocketState(sensorPocketRef.current, manualOverride);
    setInPocket(next);
    if (next && !announcedRef.current) {
      announcedRef.current = true;
      onPocketModeActive?.();
    }
    if (!next) {
      announcedRef.current = false;
    }
  }, [manualOverride, onPocketModeActive]);

  useEffect(() => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const globalWindow = window as any;
    let ambientCleanup: (() => void) | null = null;
    let proximityCleanup: (() => void) | null = null;
    let hasSensorPath = false;

    const applyPocketState = () => {
      const next = applyResolvedPocketState(sensorPocketRef.current, manualOverrideRef.current);
      setInPocket(next);
      if (next && !announcedRef.current) {
        announcedRef.current = true;
        onPocketModeActive?.();
      }
      if (!next) {
        announcedRef.current = false;
      }
    };

    if ('AmbientLightSensor' in window) {
      hasSensorPath = true;
      try {
        const sensor = new globalWindow.AmbientLightSensor({ frequency: 5 });
        const onReading = () => {
          sensorPocketRef.current = Number(sensor.illuminance ?? 999) < 5;
          applyPocketState();
        };
        const onError = () => {
          // Keep running with remaining fallbacks.
        };
        sensor.addEventListener('reading', onReading);
        sensor.addEventListener('error', onError);
        sensor.start();
        ambientCleanup = () => {
          sensor.removeEventListener('reading', onReading);
          sensor.removeEventListener('error', onError);
          try {
            sensor.stop();
          } catch {
            // Ignore
          }
        };
      } catch {
        // Permission denied or unsupported implementation.
      }
    }

    if ('ondeviceproximity' in window) {
      hasSensorPath = true;
      const onProximity = (event: Event) => {
        const data = event as DeviceProximityEventLike;
        const value = Number(data.value ?? Number.POSITIVE_INFINITY);
        const max = Number(data.max ?? Number.POSITIVE_INFINITY);
        const threshold = Number.isFinite(max) ? max * 0.12 : 3;
        sensorPocketRef.current = value <= threshold;
        applyPocketState();
      };
      window.addEventListener('deviceproximity', onProximity as EventListener);
      proximityCleanup = () => {
        window.removeEventListener('deviceproximity', onProximity as EventListener);
      };
    }

    // eslint-disable-next-line react-hooks/set-state-in-effect
    setSensorAvailable(hasSensorPath);
    if (!hasSensorPath) {
      const reason = 'sensor_unavailable:ambient_light_and_proximity';
      if (unavailableReasonRef.current !== reason) {
        unavailableReasonRef.current = reason;
        onSensorUnavailable?.(reason);
      }
    } else if (unavailableReasonRef.current) {
      unavailableReasonRef.current = '';
    }

    return () => {
      ambientCleanup?.();
      proximityCleanup?.();
    };
  }, [onPocketModeActive, onSensorUnavailable]);

  return {
    inPocket,
    sensorAvailable,
  };
}
