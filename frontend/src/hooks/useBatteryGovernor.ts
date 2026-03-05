import { useEffect, useRef, useState } from 'react';

interface BatteryManagerLike extends EventTarget {
  level: number;
  charging: boolean;
}

interface BatterySample {
  level: number;
  atMs: number;
}

export interface BatteryGovernorSnapshot {
  supported: boolean;
  levelPercent: number | null;
  dischargeRatePerMin: number | null;
  highDischarge: boolean;
  lowBattery: boolean;
}

const HIGH_DISCHARGE_PER_MIN = 0.25;

export function useBatteryGovernor(enabled: boolean): BatteryGovernorSnapshot {
  const [supported, setSupported] = useState(false);
  const [levelPercent, setLevelPercent] = useState<number | null>(null);
  const [dischargeRatePerMin, setDischargeRatePerMin] = useState<number | null>(null);
  const lastSampleRef = useRef<BatterySample | null>(null);
  const pollTimerRef = useRef<number | null>(null);

  useEffect(() => {
    if (!enabled) {
      return;
    }

    const nav = navigator as Navigator & { getBattery?: () => Promise<BatteryManagerLike> };
    if (typeof nav.getBattery !== 'function') {
      setSupported(false);
      return;
    }

    let active = true;
    let battery: BatteryManagerLike | null = null;

    const clearTimer = () => {
      if (pollTimerRef.current !== null) {
        window.clearInterval(pollTimerRef.current);
        pollTimerRef.current = null;
      }
    };

    const onBatteryUpdate = () => {
      if (!active || !battery) {
        return;
      }
      const now = Date.now();
      const level = Math.max(0, Math.min(1, Number(battery.level || 0)));
      setLevelPercent(Math.round(level * 1000) / 10);

      const prev = lastSampleRef.current;
      lastSampleRef.current = { level, atMs: now };

      if (!prev || battery.charging) {
        if (battery.charging) {
          setDischargeRatePerMin(0);
        }
        return;
      }

      const elapsedMin = (now - prev.atMs) / 60000;
      if (elapsedMin <= 0.05) {
        return;
      }

      const deltaPercent = (prev.level - level) * 100;
      if (deltaPercent < 0) {
        // Battery may jump up due to recalibration.
        return;
      }
      const instantaneousRate = deltaPercent / elapsedMin;
      setDischargeRatePerMin((current) => {
        if (current === null) {
          return Math.max(0, instantaneousRate);
        }
        return Math.max(0, current * 0.75 + instantaneousRate * 0.25);
      });
    };

    const attach = async () => {
      try {
        battery = await nav.getBattery?.() ?? null;
        if (!active || !battery) {
          return;
        }
        setSupported(true);
        onBatteryUpdate();
        battery.addEventListener('levelchange', onBatteryUpdate);
        battery.addEventListener('chargingchange', onBatteryUpdate);
        pollTimerRef.current = window.setInterval(onBatteryUpdate, 60_000);
      } catch {
        setSupported(false);
      }
    };

    void attach();

    return () => {
      active = false;
      clearTimer();
      if (battery) {
        battery.removeEventListener('levelchange', onBatteryUpdate);
        battery.removeEventListener('chargingchange', onBatteryUpdate);
      }
    };
  }, [enabled]);

  return {
    supported,
    levelPercent,
    dischargeRatePerMin,
    highDischarge: (dischargeRatePerMin ?? 0) > HIGH_DISCHARGE_PER_MIN,
    lowBattery: (levelPercent ?? 100) <= 20,
  };
}
