import { useEffect, useMemo, useRef, useState } from 'react';
import { pcm16Base64ToFloat32 } from '../utils/pcm';

interface HelperLiveViewProps {
  helpTicket: string;
}

interface HelpExchangeResponse {
  session_id?: string;
  viewer_token?: string;
  expires_in?: number;
}

function encodeBase64Url(value: string): string {
  const bytes = new TextEncoder().encode(value);
  let binary = '';
  bytes.forEach((byte) => {
    binary += String.fromCharCode(byte);
  });
  return window.btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
}

function deriveAuthBase(): string {
  const configuredAuthBase = (import.meta.env.VITE_AUTH_BROKER_BASE as string | undefined)?.trim();
  if (configuredAuthBase) {
    return configuredAuthBase;
  }
  return `${window.location.origin}/auth`;
}

function deriveHelpWsBase(): string {
  const configuredWsBase = (import.meta.env.VITE_BACKEND_WS_BASE as string | undefined)?.trim();
  if (configuredWsBase) {
    try {
      const wsUrl = new URL(configuredWsBase, window.location.href);
      const protocol = wsUrl.protocol === 'https:' ? 'wss:' : wsUrl.protocol === 'http:' ? 'ws:' : wsUrl.protocol;
      return `${protocol}//${wsUrl.host}/ws`;
    } catch {
      // Fall through to runtime origin path.
    }
  }
  return `${window.location.protocol === 'https:' ? 'wss' : 'ws'}://${window.location.host}/ws`;
}

export function HelperLiveView({ helpTicket }: HelperLiveViewProps): JSX.Element {
  const [status, setStatus] = useState<'booting' | 'connecting' | 'connected' | 'error'>('booting');
  const [error, setError] = useState('');
  const [sessionId, setSessionId] = useState('');
  const [lastFrame, setLastFrame] = useState('');
  const [audioEnabled, setAudioEnabled] = useState(false);
  const [lastHeartbeat, setLastHeartbeat] = useState('');
  const socketRef = useRef<WebSocket | null>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const nextPlayTimeRef = useRef(0);

  const authBase = useMemo(() => deriveAuthBase(), []);
  const wsBase = useMemo(() => deriveHelpWsBase(), []);

  const ensureAudioContext = async (): Promise<AudioContext | null> => {
    try {
      if (!audioContextRef.current) {
        audioContextRef.current = new AudioContext({ sampleRate: 16000 });
      }
      if (audioContextRef.current.state === 'suspended') {
        await audioContextRef.current.resume();
      }
      return audioContextRef.current;
    } catch {
      return null;
    }
  };

  const playPcm16 = async (base64: string, sampleRateHz?: number): Promise<void> => {
    if (!audioEnabled || !base64) {
      return;
    }
    const ctx = await ensureAudioContext();
    if (!ctx) {
      return;
    }
    const samples = pcm16Base64ToFloat32(base64);
    if (samples.length === 0) {
      return;
    }
    const buffer = ctx.createBuffer(1, samples.length, sampleRateHz && sampleRateHz > 0 ? sampleRateHz : 16000);
    buffer.copyToChannel(Float32Array.from(samples), 0);
    const source = ctx.createBufferSource();
    source.buffer = buffer;
    source.connect(ctx.destination);
    const startAt = Math.max(ctx.currentTime + 0.01, nextPlayTimeRef.current);
    source.start(startAt);
    nextPlayTimeRef.current = startAt + buffer.duration;
  };

  useEffect(() => {
    let cancelled = false;
    const connect = async () => {
      setStatus('connecting');
      setError('');
      try {
        const exchangeResp = await fetch(`${authBase}/help-ticket/exchange`, {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ ticket: helpTicket }),
        });
        if (!exchangeResp.ok) {
          const message = await exchangeResp.text();
          throw new Error(`Help ticket exchange failed: ${exchangeResp.status} ${message}`);
        }
        const payload = (await exchangeResp.json()) as HelpExchangeResponse;
        const viewerToken = String(payload.viewer_token || '').trim();
        const sid = String(payload.session_id || '').trim();
        if (!viewerToken || !sid) {
          throw new Error('Invalid help exchange response.');
        }
        if (cancelled) {
          return;
        }
        setSessionId(sid);

        const wsUrl = `${wsBase}/help/${encodeURIComponent(sid)}`;
        const protocols = ['apollos.help.v1', `authb64.${encodeBase64Url(viewerToken)}`];
        const ws = new WebSocket(wsUrl, protocols);
        socketRef.current = ws;

        ws.onopen = () => {
          if (cancelled) {
            return;
          }
          setStatus('connected');
          ws.send(JSON.stringify({ type: 'heartbeat', timestamp: new Date().toISOString() }));
        };
        ws.onmessage = (event) => {
          let data: unknown = event.data;
          if (typeof data === 'string') {
            try {
              data = JSON.parse(data);
            } catch {
              if (event.data === 'pong') {
                setLastHeartbeat(new Date().toISOString());
              }
              return;
            }
          }
          if (!data || typeof data !== 'object') {
            return;
          }
          const typed = data as Record<string, unknown>;
          if (typed.type === 'help_frame') {
            const frame = String(typed.frame_jpeg_base64 || '').trim();
            if (frame) {
              setLastFrame(`data:image/jpeg;base64,${frame}`);
            }
            return;
          }
          if (typed.type === 'help_audio') {
            const audio = String(typed.audio_chunk_pcm16 || '').trim();
            const sampleRateHz = Number(typed.sample_rate_hz || 16000);
            void playPcm16(audio, sampleRateHz);
            return;
          }
          if (typed.type === 'heartbeat_ack') {
            setLastHeartbeat(String(typed.timestamp || new Date().toISOString()));
          }
        };
        ws.onerror = () => {
          if (!cancelled) {
            setStatus('error');
            setError('Realtime helper stream failed.');
          }
        };
        ws.onclose = () => {
          if (!cancelled) {
            setStatus('error');
            setError('Realtime helper stream closed.');
          }
        };
      } catch (err) {
        if (cancelled) {
          return;
        }
        setStatus('error');
        setError(String(err));
      }
    };
    void connect();

    const heartbeatTimer = window.setInterval(() => {
      if (socketRef.current?.readyState === WebSocket.OPEN) {
        socketRef.current.send(JSON.stringify({ type: 'heartbeat', timestamp: new Date().toISOString() }));
      }
    }, 15000);

    return () => {
      cancelled = true;
      window.clearInterval(heartbeatTimer);
      if (socketRef.current) {
        socketRef.current.close(1000, 'helper_view_closed');
        socketRef.current = null;
      }
      if (audioContextRef.current) {
        void audioContextRef.current.close();
        audioContextRef.current = null;
      }
    };
  }, [authBase, helpTicket, wsBase]);

  return (
    <main className="app-shell" style={{ padding: 16, gap: 12 }}>
      <header className="top-bar" style={{ marginBottom: 8 }}>
        <strong>Apollos Helper Live</strong>
        <span>{sessionId ? `Session: ${sessionId}` : 'Waiting for session...'}</span>
      </header>
      <section className="status-grid">
        <p>Status: {status}</p>
        {lastHeartbeat ? <p>Last heartbeat: {lastHeartbeat}</p> : null}
        {error ? <p role="alert">Error: {error}</p> : null}
      </section>
      <section style={{ display: 'grid', gap: 8 }}>
        <button
          type="button"
          onClick={() => {
            setAudioEnabled(true);
            void ensureAudioContext();
          }}
        >
          {audioEnabled ? 'Audio enabled' : 'Enable helper audio'}
        </button>
      </section>
      <section style={{ border: '1px solid rgba(255,255,255,0.2)', borderRadius: 12, overflow: 'hidden', minHeight: 240 }}>
        {lastFrame ? (
          <img
            src={lastFrame}
            alt="Live helper frame"
            style={{ width: '100%', height: 'auto', display: 'block' }}
          />
        ) : (
          <div style={{ padding: 16 }}>Waiting for live camera frames...</div>
        )}
      </section>
    </main>
  );
}
