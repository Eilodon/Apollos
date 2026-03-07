import { useCallback, useEffect, useRef, useState } from 'react';

interface WakeLockSentinelLike {
  released: boolean;
  release: () => Promise<void>;
}

interface WakeLockProvider {
  request: (type: 'screen') => Promise<WakeLockSentinelLike>;
}

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
  const keepAliveContextRef = useRef<AudioContext | null>(null);

  const stopSilentKeepalive = useCallback(() => {
    if (keepAliveIntervalRef.current !== null) {
      window.clearInterval(keepAliveIntervalRef.current);
      keepAliveIntervalRef.current = null;
    }
    if (keepAliveContextRef.current) {
      void keepAliveContextRef.current.close();
      keepAliveContextRef.current = null;
    }
  }, []);

  const startSilentKeepalive = useCallback(() => {
    if (keepAliveIntervalRef.current !== null) {
      return;
    }

    const ensureContext = (): AudioContext | null => {
      try {
        if (!keepAliveContextRef.current) {
          keepAliveContextRef.current = new AudioContext();
        }
        return keepAliveContextRef.current;
      } catch {
        return null;
      }
    };

    keepAliveIntervalRef.current = window.setInterval(() => {
      const ctx = ensureContext();
      if (!ctx) {
        return;
      }

      try {
        if (ctx.state === 'suspended') {
          void ctx.resume();
        }
        const oscillator = ctx.createOscillator();
        const gain = ctx.createGain();
        gain.gain.value = 0;
        oscillator.connect(gain);
        gain.connect(ctx.destination);
        oscillator.start();
        oscillator.stop(ctx.currentTime + 0.02);
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
