import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  AudioChunkMessage,
  BackendToClientMessage,
  HardStopMessage,
  MultimodalFrameMessage,
  UserCommandMessage,
} from '../types/contracts';

interface UseARIAOptions {
  sessionId: string;
  onBackendMessage?: (message: BackendToClientMessage) => void;
  onHardStop?: (message: HardStopMessage) => void;
}

interface UseARIAResult {
  status: 'disconnected' | 'connecting' | 'connected' | 'reconnecting';
  connect: () => void;
  disconnect: () => void;
  sendFrame: (message: Omit<MultimodalFrameMessage, 'type' | 'session_id'>) => void;
  sendAudioChunk: (chunkBase64: string) => void;
  sendUserCommand: (command: string) => void;
}

const MAX_RETRIES = 6;

function buildWsUrl(path: string): string {
  const configuredBase = import.meta.env.VITE_BACKEND_WS_BASE as string | undefined;
  const base = configuredBase || `${window.location.protocol === 'https:' ? 'wss' : 'ws'}://${window.location.hostname}:8000/ws`;
  return `${base}${path}`;
}

export function useARIA({ sessionId, onBackendMessage, onHardStop }: UseARIAOptions): UseARIAResult {
  const [status, setStatus] = useState<UseARIAResult['status']>('disconnected');

  const liveSocketRef = useRef<WebSocket | null>(null);
  const emergencySocketRef = useRef<WebSocket | null>(null);
  const reconnectAttempts = useRef(0);
  const reconnectTimer = useRef<number | null>(null);
  const shouldReconnectRef = useRef(false);

  const liveUrl = useMemo(() => buildWsUrl(`/live/${sessionId}`), [sessionId]);
  const emergencyUrl = useMemo(() => buildWsUrl(`/emergency/${sessionId}`), [sessionId]);

  const clearReconnectTimer = useCallback(() => {
    if (reconnectTimer.current !== null) {
      window.clearTimeout(reconnectTimer.current);
      reconnectTimer.current = null;
    }
  }, []);

  const disconnect = useCallback(() => {
    shouldReconnectRef.current = false;
    clearReconnectTimer();

    if (liveSocketRef.current) {
      liveSocketRef.current.close(1000, 'client_disconnect');
      liveSocketRef.current = null;
    }

    if (emergencySocketRef.current) {
      emergencySocketRef.current.close(1000, 'client_disconnect');
      emergencySocketRef.current = null;
    }

    reconnectAttempts.current = 0;
    setStatus('disconnected');
  }, [clearReconnectTimer]);

  const connect = useCallback(() => {
    if (liveSocketRef.current?.readyState === WebSocket.OPEN || liveSocketRef.current?.readyState === WebSocket.CONNECTING) {
      liveSocketRef.current.close(1000, 'reconnecting');
    }
    if (emergencySocketRef.current?.readyState === WebSocket.OPEN || emergencySocketRef.current?.readyState === WebSocket.CONNECTING) {
      emergencySocketRef.current.close(1000, 'reconnecting');
    }

    shouldReconnectRef.current = true;
    clearReconnectTimer();

    setStatus(reconnectAttempts.current > 0 ? 'reconnecting' : 'connecting');

    const liveSocket = new WebSocket(liveUrl);
    const emergencySocket = new WebSocket(emergencyUrl);
    liveSocketRef.current = liveSocket;
    emergencySocketRef.current = emergencySocket;

    liveSocket.onopen = () => {
      reconnectAttempts.current = 0;
      setStatus('connected');
    };

    liveSocket.onmessage = (event) => {
      let data: BackendToClientMessage;
      try {
        data = JSON.parse(event.data) as BackendToClientMessage;
      } catch {
        return;
      }
      onBackendMessage?.(data);
      if (data.type === 'HARD_STOP') {
        onHardStop?.(data);
      }
    };

    emergencySocket.onmessage = (event) => {
      let data: BackendToClientMessage;
      try {
        data = JSON.parse(event.data) as BackendToClientMessage;
      } catch {
        return;
      }
      onBackendMessage?.(data);
      if (data.type === 'HARD_STOP') {
        onHardStop?.(data);
      }
    };

    const onClose = () => {
      // Force close the other socket to maintain consistency
      if (liveSocket.readyState === WebSocket.OPEN) {
        liveSocket.close(1000, 'sync_close');
      }
      if (emergencySocket.readyState === WebSocket.OPEN) {
        emergencySocket.close(1000, 'sync_close');
      }

      if (!shouldReconnectRef.current) {
        setStatus('disconnected');
        return;
      }
      if (reconnectAttempts.current >= MAX_RETRIES) {
        shouldReconnectRef.current = false;
        setStatus('disconnected');
        return;
      }
      if (reconnectTimer.current !== null) {
        return;
      }

      reconnectAttempts.current += 1;
      setStatus('reconnecting');
      const delay = Math.min(1000 * 2 ** reconnectAttempts.current, 8000);
      reconnectTimer.current = window.setTimeout(() => {
        reconnectTimer.current = null;
        connect();
      }, delay);
    };

    liveSocket.onclose = onClose;
    emergencySocket.onclose = onClose;

    liveSocket.onerror = () => {
      liveSocket.close();
    };

    emergencySocket.onerror = () => {
      emergencySocket.close();
    };
  }, [clearReconnectTimer, emergencyUrl, liveUrl, onBackendMessage, onHardStop]);

  const sendFrame = useCallback(
    (message: Omit<MultimodalFrameMessage, 'type' | 'session_id'>) => {
      if (liveSocketRef.current?.readyState !== WebSocket.OPEN) {
        return;
      }

      const payload: MultimodalFrameMessage = {
        type: 'multimodal_frame',
        session_id: sessionId,
        ...message,
      };

      liveSocketRef.current.send(JSON.stringify(payload));
    },
    [sessionId],
  );

  const sendAudioChunk = useCallback(
    (chunkBase64: string) => {
      if (liveSocketRef.current?.readyState !== WebSocket.OPEN) {
        return;
      }

      const payload: AudioChunkMessage = {
        type: 'audio_chunk',
        session_id: sessionId,
        timestamp: new Date().toISOString(),
        audio_chunk_pcm16: chunkBase64,
      };

      liveSocketRef.current.send(JSON.stringify(payload));
    },
    [sessionId],
  );

  const sendUserCommand = useCallback(
    (command: string) => {
      if (liveSocketRef.current?.readyState !== WebSocket.OPEN) {
        return;
      }

      const payload: UserCommandMessage = {
        type: 'user_command',
        session_id: sessionId,
        timestamp: new Date().toISOString(),
        command,
      };

      liveSocketRef.current.send(JSON.stringify(payload));
    },
    [sessionId],
  );

  useEffect(() => {
    return () => {
      disconnect();
    };
  }, [disconnect]);

  return {
    status,
    connect,
    disconnect,
    sendFrame,
    sendAudioChunk,
    sendUserCommand,
  };
}
