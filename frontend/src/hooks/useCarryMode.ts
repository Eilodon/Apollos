import { useCallback, useMemo, useState } from 'react';
import type {
  CarryMode,
  CarryModeProfile} from '../services/carryMode';
import {
  DEFAULT_CARRY_MODE,
  getCarryModeProfile,
  loadStoredCarryMode,
  persistCarryMode,
} from '../services/carryMode';

interface UseCarryModeResult {
  carryMode: CarryMode | null;
  activeCarryMode: CarryMode;
  profile: CarryModeProfile;
  setCarryMode: (mode: CarryMode) => void;
}

export function useCarryMode(): UseCarryModeResult {
  const [carryMode, setCarryModeState] = useState<CarryMode | null>(() => loadStoredCarryMode());

  const setCarryMode = useCallback((mode: CarryMode) => {
    setCarryModeState(mode);
    persistCarryMode(mode);
  }, []);

  const activeCarryMode = carryMode ?? DEFAULT_CARRY_MODE;
  const profile = useMemo(() => getCarryModeProfile(activeCarryMode), [activeCarryMode]);

  return {
    carryMode,
    activeCarryMode,
    profile,
    setCarryMode,
  };
}
