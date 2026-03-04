import { useCallback, useEffect, useRef, useState } from 'react';

type WakeLockSentinelLike = {
  released: boolean;
  release: () => Promise<void>;
};

type WakeLockProvider = {
  request: (type: 'screen') => Promise<WakeLockSentinelLike>;
};

interface UseWakeLockResult {
  oledBlackMode: boolean;
  wakeLockActive: boolean;
  activateNavigationMode: () => Promise<void>;
  deactivateNavigationMode: () => Promise<void>;
}

export function useWakeLock(): UseWakeLockResult {
  const [oledBlackMode, setOledBlackMode] = useState(false);
  const [wakeLockActive, setWakeLockActive] = useState(false);

  const wakeLockRef = useRef<WakeLockSentinelLike | null>(null);
  const keepAliveIntervalRef = useRef<number | null>(null);

  const stopSilentKeepalive = useCallback(() => {
    if (keepAliveIntervalRef.current !== null) {
      window.clearInterval(keepAliveIntervalRef.current);
      keepAliveIntervalRef.current = null;
    }
  }, []);

  const startSilentKeepalive = useCallback(() => {
    if (keepAliveIntervalRef.current !== null) {
      return;
    }

    keepAliveIntervalRef.current = window.setInterval(() => {
      try {
        const ctx = new AudioContext();
        const oscillator = ctx.createOscillator();
        const gain = ctx.createGain();
        gain.gain.value = 0;
        oscillator.connect(gain);
        gain.connect(ctx.destination);
        oscillator.start();
        oscillator.stop(ctx.currentTime + 0.02);
        window.setTimeout(() => {
          void ctx.close();
        }, 150);
      } catch {
        // Ignore keepalive errors.
      }
    }, 25000);
  }, []);

  const requestWakeLock = useCallback(async (): Promise<boolean> => {
    const provider = (navigator as Navigator & { wakeLock?: WakeLockProvider }).wakeLock;
    if (!provider) {
      return false;
    }

    try {
      wakeLockRef.current = await provider.request('screen');
      setWakeLockActive(true);
      return true;
    } catch {
      setWakeLockActive(false);
      wakeLockRef.current = null;
      return false;
    }
  }, []);

  const activateNavigationMode = useCallback(async () => {
    const lockGranted = await requestWakeLock();
    if (!lockGranted) {
      startSilentKeepalive();
    }

    document.body.style.backgroundColor = '#000000';
    document.body.style.filter = 'brightness(0)';
    setOledBlackMode(true);
  }, [requestWakeLock, startSilentKeepalive]);

  const deactivateNavigationMode = useCallback(async () => {
    if (wakeLockRef.current && !wakeLockRef.current.released) {
      await wakeLockRef.current.release();
    }

    wakeLockRef.current = null;
    setWakeLockActive(false);
    stopSilentKeepalive();

    document.body.style.backgroundColor = '';
    document.body.style.filter = '';
    setOledBlackMode(false);
  }, [stopSilentKeepalive]);

  useEffect(() => {
    const onVisibilityChange = async (): Promise<void> => {
      if (document.visibilityState !== 'visible') {
        return;
      }

      if (wakeLockRef.current?.released) {
        await requestWakeLock();
      }
    };

    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => {
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [requestWakeLock]);

  useEffect(() => {
    return () => {
      void deactivateNavigationMode();
    };
  }, [deactivateNavigationMode]);

  return {
    oledBlackMode,
    wakeLockActive,
    activateNavigationMode,
    deactivateNavigationMode,
  };
}
