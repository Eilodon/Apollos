import { useMemo } from 'react';
import type { SensorHealthSnapshot } from '../types/contracts';
import type { BatteryGovernorSnapshot } from './useBatteryGovernor';
import type { LocationContextSnapshot } from './useLocationContext';

export type DepthPipelineState = 'unknown' | 'loading' | 'ready' | 'fallback' | 'error';

interface UseSensorHealthOptions {
  sessionActive: boolean;
  pocketSensorAvailable: boolean;
  motionPermissionDenied: boolean;
  locationSnapshot: LocationContextSnapshot | null;
  battery: BatteryGovernorSnapshot;
  depthState: DepthPipelineState;
}

function clamp(value: number, low: number, high: number): number {
  return Math.max(low, Math.min(high, value));
}

export function useSensorHealth({
  sessionActive,
  pocketSensorAvailable,
  motionPermissionDenied,
  locationSnapshot,
  battery,
  depthState,
}: UseSensorHealthOptions): SensorHealthSnapshot {
  return useMemo(() => {
    if (!sessionActive) {
      return {
        score: 1,
        flags: [],
        degraded: false,
        source: 'edge-fused-v1',
      };
    }

    let score = 1.0;
    const flags: string[] = [];

    if (!pocketSensorAvailable) {
      score -= 0.18;
      flags.push('pocket_sensor_unavailable');
    }

    if (motionPermissionDenied) {
      score -= 0.25;
      flags.push('motion_permission_denied');
    }

    if (!locationSnapshot) {
      score -= 0.22;
      flags.push('location_missing');
    } else if (locationSnapshot.accuracyM > 40) {
      score -= 0.12;
      flags.push('location_low_accuracy');
    }

    if (depthState === 'fallback') {
      score -= 0.18;
      flags.push('depth_fallback');
    } else if (depthState === 'error') {
      score -= 0.35;
      flags.push('depth_error');
    } else if (depthState === 'loading') {
      score -= 0.08;
      flags.push('depth_loading');
    }

    if (battery.lowBattery) {
      score -= 0.08;
      flags.push('battery_low');
    }
    if (battery.highDischarge) {
      score -= 0.06;
      flags.push('battery_high_discharge');
    }

    const normalizedScore = clamp(score, 0, 1);
    const degraded = (
      normalizedScore < 0.55
      || flags.includes('depth_error')
      || flags.includes('motion_permission_denied')
      || flags.includes('location_missing')
    );

    return {
      score: Math.round(normalizedScore * 100) / 100,
      flags,
      degraded,
      source: 'edge-fused-v1',
    };
  }, [
    battery.highDischarge,
    battery.lowBattery,
    depthState,
    locationSnapshot,
    motionPermissionDenied,
    pocketSensorAvailable,
    sessionActive,
  ]);
}
