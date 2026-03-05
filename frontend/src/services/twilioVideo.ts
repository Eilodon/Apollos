type TwilioVideoGlobal = {
  connect: (token: string, options?: Record<string, unknown>) => Promise<any>;
};

declare global {
  interface Window {
    Twilio?: {
      Video?: TwilioVideoGlobal;
    };
  }
}

let twilioSdkPromise: Promise<TwilioVideoGlobal> | null = null;

function defaultSdkUrl(): string {
  const configured = (import.meta.env.VITE_TWILIO_VIDEO_SDK_URL as string | undefined)?.trim();
  if (configured) {
    return configured;
  }
  return 'https://sdk.twilio.com/js/video/releases/2.31.0/twilio-video.min.js';
}

export async function loadTwilioVideoSdk(): Promise<TwilioVideoGlobal> {
  if (window.Twilio?.Video) {
    return window.Twilio.Video;
  }
  if (twilioSdkPromise) {
    return twilioSdkPromise;
  }

  twilioSdkPromise = new Promise<TwilioVideoGlobal>((resolve, reject) => {
    const sdkUrl = defaultSdkUrl();
    const existing = document.querySelector<HTMLScriptElement>(`script[data-twilio-video-sdk="${sdkUrl}"]`);
    if (existing) {
      existing.addEventListener('load', () => {
        if (window.Twilio?.Video) {
          resolve(window.Twilio.Video);
        } else {
          reject(new Error('Twilio Video SDK loaded but global not found.'));
        }
      });
      existing.addEventListener('error', () => reject(new Error('Failed to load Twilio Video SDK script.')));
      return;
    }

    const script = document.createElement('script');
    script.src = sdkUrl;
    script.async = true;
    script.defer = true;
    script.dataset.twilioVideoSdk = sdkUrl;
    script.onload = () => {
      if (window.Twilio?.Video) {
        resolve(window.Twilio.Video);
      } else {
        reject(new Error('Twilio Video SDK loaded but global not found.'));
      }
    };
    script.onerror = () => reject(new Error('Failed to load Twilio Video SDK script.'));
    document.head.appendChild(script);
  });

  return twilioSdkPromise;
}

export async function connectTwilioRoom(
  token: string,
  roomName: string,
  options?: {
    publishAudio?: boolean;
    publishVideo?: boolean;
  },
): Promise<any> {
  const video = await loadTwilioVideoSdk();
  return video.connect(token, {
    name: roomName,
    audio: options?.publishAudio ?? true,
    video: options?.publishVideo ?? true,
    dominantSpeaker: true,
    networkQuality: { local: 1, remote: 1 },
  });
}

export function disconnectTwilioRoom(room: any | null): void {
  if (!room || typeof room.disconnect !== 'function') {
    return;
  }
  try {
    room.disconnect();
  } catch {
    // ignore
  }
}
