import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  AudioChunkMessage,
  BackendToClientMessage,
  EdgeHazardMessage,
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
  sendEdgeHazard: (payload: Omit<EdgeHazardMessage, 'type' | 'session_id' | 'timestamp'>) => void;
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
  const emergencyRetryTimer = useRef<number | null>(null);
  const shouldReconnectRef = useRef(false);

  const liveUrl = useMemo(() => buildWsUrl(`/live/${sessionId}`), [sessionId]);
  const emergencyUrl = useMemo(() => buildWsUrl(`/emergency/${sessionId}`), [sessionId]);

  const handleInboundMessage = useCallback((payload: unknown) => {
    if (!payload || typeof payload !== 'object') {
      return;
    }

    const type = (payload as { type?: string }).type;
    if (!type) {
      return;
    }

    if (type === 'HARD_STOP') {
      const hardStop = payload as HardStopMessage;
      onBackendMessage?.(hardStop);
      onHardStop?.(hardStop);
      return;
    }

    if (type === 'assistant_text' || type === 'audio_chunk' || type === 'connection_state' || type === 'semantic_cue') {
      onBackendMessage?.(payload as BackendToClientMessage);
    }
  }, [onBackendMessage, onHardStop]);

  const clearReconnectTimer = useCallback(() => {
    if (reconnectTimer.current !== null) {
      window.clearTimeout(reconnectTimer.current);
      reconnectTimer.current = null;
    }
    if (emergencyRetryTimer.current !== null) {
      window.clearTimeout(emergencyRetryTimer.current);
      emergencyRetryTimer.current = null;
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

    shouldReconnectRef.current = true;
    clearReconnectTimer();

    setStatus(reconnectAttempts.current > 0 ? 'reconnecting' : 'connecting');

    const openEmergencyChannel = () => {
      if (!shouldReconnectRef.current) {
        return;
      }
      if (
        emergencySocketRef.current?.readyState === WebSocket.OPEN ||
        emergencySocketRef.current?.readyState === WebSocket.CONNECTING
      ) {
        return;
      }

      const emergencySocket = new WebSocket(emergencyUrl);
      emergencySocketRef.current = emergencySocket;

      emergencySocket.onopen = () => {
        emergencySocket.send(JSON.stringify({ type: 'heartbeat', session_id: sessionId, timestamp: new Date().toISOString() }));
      };

      emergencySocket.onmessage = (event) => {
        let data: unknown;
        try {
          data = JSON.parse(event.data);
        } catch {
          return;
        }
        handleInboundMessage(data);
      };

      emergencySocket.onclose = () => {
        emergencySocketRef.current = null;
        if (!shouldReconnectRef.current) {
          return;
        }
        if (liveSocketRef.current?.readyState !== WebSocket.OPEN) {
          return;
        }
        if (emergencyRetryTimer.current !== null) {
          return;
        }
        emergencyRetryTimer.current = window.setTimeout(() => {
          emergencyRetryTimer.current = null;
          openEmergencyChannel();
        }, 750);
      };

      emergencySocket.onerror = () => {
        emergencySocket.close();
      };
    };

    if (emergencySocketRef.current?.readyState === WebSocket.OPEN || emergencySocketRef.current?.readyState === WebSocket.CONNECTING) {
      emergencySocketRef.current.close(1000, 'reconnecting');
      emergencySocketRef.current = null;
    }

    const liveSocket = new WebSocket(liveUrl);
    liveSocketRef.current = liveSocket;

    liveSocket.onopen = () => {
      reconnectAttempts.current = 0;
      setStatus('connected');
      openEmergencyChannel();
    };

    liveSocket.onmessage = (event) => {
      let data: unknown;
      try {
        data = JSON.parse(event.data);
      } catch {
        return;
      }
      handleInboundMessage(data);
    };

    const onClose = () => {
      if (emergencySocketRef.current) {
        emergencySocketRef.current.close(1000, 'live_disconnected');
        emergencySocketRef.current = null;
      }

      // Manage reconnect state
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

    liveSocket.onerror = () => {
      liveSocket.close();
    };
  }, [clearReconnectTimer, emergencyUrl, handleInboundMessage, liveUrl, sessionId]);

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

  const sendEdgeHazard = useCallback(
    (payload: Omit<EdgeHazardMessage, 'type' | 'session_id' | 'timestamp'>) => {
      const message: EdgeHazardMessage = {
        type: 'EDGE_HAZARD',
        session_id: sessionId,
        timestamp: new Date().toISOString(),
        ...payload,
      };

      if (emergencySocketRef.current?.readyState === WebSocket.OPEN) {
        emergencySocketRef.current.send(JSON.stringify(message));
      }
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
    sendEdgeHazard,
  };
}
