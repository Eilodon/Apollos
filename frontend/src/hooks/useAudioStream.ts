import { useCallback, useEffect, useRef, useState } from 'react';
import { floatToPcm16Base64 } from '../utils/pcm';

interface UseAudioStreamOptions {
  onAudioChunk: (chunkBase64: string) => void;
}

interface UseAudioStreamResult {
  micActive: boolean;
  startMic: () => Promise<void>;
  stopMic: () => void;
  toggleMic: () => Promise<void>;
}

export function useAudioStream({ onAudioChunk }: UseAudioStreamOptions): UseAudioStreamResult {
  const [micActive, setMicActive] = useState(false);

  const mediaStreamRef = useRef<MediaStream | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  const sourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const processorRef = useRef<ScriptProcessorNode | null>(null);
  const sinkGainRef = useRef<GainNode | null>(null);

  const stopMic = useCallback(() => {
    processorRef.current?.disconnect();
    sourceRef.current?.disconnect();
    sinkGainRef.current?.disconnect();

    processorRef.current = null;
    sourceRef.current = null;
    sinkGainRef.current = null;

    if (audioCtxRef.current) {
      void audioCtxRef.current.close();
      audioCtxRef.current = null;
    }

    mediaStreamRef.current?.getTracks().forEach((track) => track.stop());
    mediaStreamRef.current = null;

    setMicActive(false);
  }, []);

  const startMic = useCallback(async () => {
    if (micActive) {
      return;
    }

    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        video: false,
        audio: {
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: true,
          sampleRate: 16000,
          channelCount: 1,
        },
      });

      const context = new AudioContext({ sampleRate: 16000 });
      const source = context.createMediaStreamSource(stream);
      const processor = context.createScriptProcessor(2048, 1, 1);
      const sinkGain = context.createGain();
      sinkGain.gain.value = 0;

      processor.onaudioprocess = (event: AudioProcessingEvent) => {
        const input = event.inputBuffer.getChannelData(0);
        const chunk = new Float32Array(input.length);
        chunk.set(input);
        const base64 = floatToPcm16Base64(chunk);
        onAudioChunk(base64);
      };

      source.connect(processor);
      processor.connect(sinkGain);
      sinkGain.connect(context.destination);

      mediaStreamRef.current = stream;
      audioCtxRef.current = context;
      sourceRef.current = source;
      processorRef.current = processor;
      sinkGainRef.current = sinkGain;
      setMicActive(true);
    } catch (error) {
      console.error('Failed to start microphone stream.', error);
      stopMic();
    }
  }, [micActive, onAudioChunk, stopMic]);

  const toggleMic = useCallback(async () => {
    if (micActive) {
      stopMic();
      return;
    }
    await startMic();
  }, [micActive, startMic, stopMic]);

  useEffect(() => {
    return () => {
      stopMic();
    };
  }, [stopMic]);

  return {
    micActive,
    startMic,
    stopMic,
    toggleMic,
  };
}
