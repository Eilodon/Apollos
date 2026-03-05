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

const ZCR_MIN = 25;
const ZCR_MAX = 150;
const ENERGY_FLOOR = 0.015;
const SPECTRAL_MIN = 0.2;
const SPECTRAL_MAX = 6.5;
const FALLBACK_HOLD_FRAMES = 8;

function isSpeechLike(buffer: Float32Array): boolean {
  if (buffer.length < 2) {
    return false;
  }

  let zeroCrossings = 0;
  let energy = 0;
  let spectralEnergy = 0;
  for (let i = 1; i < buffer.length; i += 1) {
    const sample = buffer[i] ?? 0;
    const prev = buffer[i - 1] ?? 0;
    energy += sample * sample;
    const diff = sample - prev;
    spectralEnergy += diff * diff;
    if ((sample >= 0 && prev < 0) || (sample < 0 && prev >= 0)) {
      zeroCrossings += 1;
    }
  }

  const effectiveZcrMax = Math.min(ZCR_MAX, buffer.length - 1);
  const zcrInRange = zeroCrossings >= ZCR_MIN && zeroCrossings <= effectiveZcrMax;
  const enoughEnergy = energy >= ENERGY_FLOOR;
  const spectralRatio = spectralEnergy / Math.max(energy, 1e-6);
  const spectralInRange = spectralRatio >= SPECTRAL_MIN && spectralRatio <= SPECTRAL_MAX;
  return zcrInRange && enoughEnergy && spectralInRange;
}

type WorkletGateMessage = {
  type?: string;
  chunk?: Float32Array;
};

export function useAudioStream({ onAudioChunk }: UseAudioStreamOptions): UseAudioStreamResult {
  const [micActive, setMicActive] = useState(false);

  const mediaStreamRef = useRef<MediaStream | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  const sourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const fallbackProcessorRef = useRef<ScriptProcessorNode | null>(null);
  const workletNodeRef = useRef<AudioWorkletNode | null>(null);
  const sinkGainRef = useRef<GainNode | null>(null);
  const fallbackHoldFramesRef = useRef(0);

  const stopMic = useCallback(() => {
    workletNodeRef.current?.disconnect();
    if (workletNodeRef.current) {
      workletNodeRef.current.port.onmessage = null;
    }
    fallbackProcessorRef.current?.disconnect();
    sourceRef.current?.disconnect();
    sinkGainRef.current?.disconnect();

    workletNodeRef.current = null;
    fallbackProcessorRef.current = null;
    sourceRef.current = null;
    sinkGainRef.current = null;

    if (audioCtxRef.current) {
      void audioCtxRef.current.close();
      audioCtxRef.current = null;
    }

    mediaStreamRef.current?.getTracks().forEach((track) => track.stop());
    mediaStreamRef.current = null;
    fallbackHoldFramesRef.current = 0;

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

      const context = new AudioContext({ sampleRate: 16000, latencyHint: 'interactive' });
      const source = context.createMediaStreamSource(stream);
      const sinkGain = context.createGain();
      sinkGain.gain.value = 0;

      let workletStarted = false;
      if ('audioWorklet' in context) {
        try {
          await context.audioWorklet.addModule(new URL('../worklets/spectralVoiceGate.worklet.ts', import.meta.url));
          const worklet = new AudioWorkletNode(context, 'spectral-voice-gate', {
            numberOfInputs: 1,
            numberOfOutputs: 1,
            outputChannelCount: [1],
          });
          worklet.port.onmessage = (event: MessageEvent<WorkletGateMessage>) => {
            const data = event.data;
            if (!data || data.type !== 'voice_gate_chunk' || !data.chunk) {
              return;
            }
            const chunk = data.chunk instanceof Float32Array ? data.chunk : new Float32Array(data.chunk);
            onAudioChunk(floatToPcm16Base64(chunk));
          };

          source.connect(worklet);
          worklet.connect(sinkGain);
          workletNodeRef.current = worklet;
          workletStarted = true;
        } catch (error) {
          // Worklet is optional; ScriptProcessor fallback keeps compatibility.
          console.warn('AudioWorklet unavailable, falling back to ScriptProcessor gate.', error);
        }
      }

      if (!workletStarted) {
        const processor = context.createScriptProcessor(2048, 1, 1);
        processor.onaudioprocess = (event: AudioProcessingEvent) => {
          const input = event.inputBuffer.getChannelData(0);
          const chunk = new Float32Array(input.length);
          chunk.set(input);

          if (isSpeechLike(chunk)) {
            fallbackHoldFramesRef.current = FALLBACK_HOLD_FRAMES;
          } else if (fallbackHoldFramesRef.current > 0) {
            fallbackHoldFramesRef.current -= 1;
          }

          if (fallbackHoldFramesRef.current === 0) {
            return;
          }

          onAudioChunk(floatToPcm16Base64(chunk));
        };
        source.connect(processor);
        processor.connect(sinkGain);
        fallbackProcessorRef.current = processor;
      }

      sinkGain.connect(context.destination);

      mediaStreamRef.current = stream;
      audioCtxRef.current = context;
      sourceRef.current = source;
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
