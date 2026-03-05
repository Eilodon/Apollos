import { useEffect, useState } from 'react';

export interface LocationContextSnapshot {
  lat: number;
  lng: number;
  headingDeg?: number;
  accuracyM: number;
  recordedAtMs: number;
}

export function useLocationContext(enabled: boolean): LocationContextSnapshot | null {
  const [snapshot, setSnapshot] = useState<LocationContextSnapshot | null>(null);

  useEffect(() => {
    if (!enabled || !('geolocation' in navigator)) {
      return;
    }

    const watchId = navigator.geolocation.watchPosition(
      (position) => {
        const now = Date.now();
        setSnapshot((prev) => ({
          lat: position.coords.latitude,
          lng: position.coords.longitude,
          accuracyM:
            typeof position.coords.accuracy === 'number' && !Number.isNaN(position.coords.accuracy)
              ? Math.max(0, position.coords.accuracy)
              : prev?.accuracyM ?? 120,
          recordedAtMs: now,
          headingDeg:
            typeof position.coords.heading === 'number' && !Number.isNaN(position.coords.heading)
              ? position.coords.heading
              : prev?.headingDeg,
        }));
      },
      () => {
        // Graceful degradation: continue without location context.
      },
      {
        enableHighAccuracy: true,
        maximumAge: 10_000,
        timeout: 12_000,
      },
    );

    return () => {
      navigator.geolocation.clearWatch(watchId);
    };
  }, [enabled]);

  useEffect(() => {
    if (!enabled) {
      return;
    }

    const handleOrientation = (event: DeviceOrientationEvent): void => {
      const alpha = event.alpha;
      if (typeof alpha !== 'number' || Number.isNaN(alpha)) {
        return;
      }
      setSnapshot((prev) => {
        if (!prev) {
          return prev;
        }
        return { ...prev, headingDeg: alpha };
      });
    };

    window.addEventListener('deviceorientation', handleOrientation, { passive: true });
    return () => {
      window.removeEventListener('deviceorientation', handleOrientation);
    };
  }, [enabled]);

  return snapshot;
}
