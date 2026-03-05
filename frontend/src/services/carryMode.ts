export type CarryMode = 'hand_held' | 'necklace' | 'chest_clip' | 'pocket';

export interface CarryModeProfile {
  cosTiltThreshold: number;
  pitchOffset: number;
  gyroThreshold: number;
  cloudEnabled: boolean;
}

export const CARRY_MODE_STORAGE_KEY = 'apollos_carry_mode_v1';
export const DEFAULT_CARRY_MODE: CarryMode = 'necklace';

export const CARRY_MODE_PROFILES: Record<CarryMode, CarryModeProfile> = {
  hand_held: { cosTiltThreshold: 0.82, pitchOffset: 0, gyroThreshold: 45, cloudEnabled: true },
  necklace: { cosTiltThreshold: 0.65, pitchOffset: 15, gyroThreshold: 55, cloudEnabled: true },
  chest_clip: { cosTiltThreshold: 0.72, pitchOffset: 8, gyroThreshold: 50, cloudEnabled: true },
  pocket: { cosTiltThreshold: 0, pitchOffset: 0, gyroThreshold: 999, cloudEnabled: false },
};

export function isCarryMode(value: unknown): value is CarryMode {
  return value === 'hand_held' || value === 'necklace' || value === 'chest_clip' || value === 'pocket';
}

export function loadStoredCarryMode(): CarryMode | null {
  try {
    const stored = localStorage.getItem(CARRY_MODE_STORAGE_KEY);
    if (!stored) {
      return null;
    }
    return isCarryMode(stored) ? stored : null;
  } catch {
    return null;
  }
}

export function persistCarryMode(mode: CarryMode): void {
  try {
    localStorage.setItem(CARRY_MODE_STORAGE_KEY, mode);
  } catch {
    // Ignore write errors and keep in-memory state only.
  }
}

export function getCarryModeProfile(mode: CarryMode): CarryModeProfile {
  return CARRY_MODE_PROFILES[mode];
}
