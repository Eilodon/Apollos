export interface PlatformCapabilities {
  isIOS: boolean;
  isSafari: boolean;
  hasAmbientLight: boolean;
  hasDeviceProximity: boolean;
  hasAudioWorklet: boolean;
  pocketShieldAvailable: boolean;
  voiceGateAvailable: boolean;
  recommendNative: boolean;
  safetyGrade: 'FULL' | 'REDUCED';
}

function hasAudioWorkletSupport(): boolean {
  const audioContextCtor = window.AudioContext
    // Safari fallback constructor name.
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    || (window as any).webkitAudioContext;
  if (!audioContextCtor?.prototype) {
    return false;
  }
  return 'audioWorklet' in audioContextCtor.prototype;
}

export function getPlatformCapabilities(): PlatformCapabilities {
  const userAgent = navigator.userAgent;
  const isIOS = /iPad|iPhone|iPod/.test(userAgent);
  const isSafari = /^((?!chrome|android).)*safari/i.test(userAgent);
  const hasAmbientLight = 'AmbientLightSensor' in window;
  const hasDeviceProximity = 'ondeviceproximity' in window;
  const hasAudioWorklet = hasAudioWorkletSupport();
  const pocketShieldAvailable = hasAmbientLight || hasDeviceProximity;
  const recommendNative = isIOS && isSafari;

  return {
    isIOS,
    isSafari,
    hasAmbientLight,
    hasDeviceProximity,
    hasAudioWorklet,
    pocketShieldAvailable,
    voiceGateAvailable: hasAudioWorklet,
    recommendNative,
    safetyGrade: recommendNative ? 'REDUCED' : 'FULL',
  };
}
