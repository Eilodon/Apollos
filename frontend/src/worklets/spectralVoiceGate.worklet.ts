declare const sampleRate: number;

declare abstract class AudioWorkletProcessor {
  readonly port: MessagePort;
  constructor(options?: unknown);
  abstract process(
    inputs: Float32Array[][],
    outputs: Float32Array[][],
    parameters: Record<string, Float32Array>,
  ): boolean;
}

declare function registerProcessor(
  name: string,
  processorCtor: new () => AudioWorkletProcessor,
): void;

class SpectralVoiceGateProcessor extends AudioWorkletProcessor {
  private readonly ZCR_MIN = 25;
  private readonly ZCR_MAX = 150;
  private readonly ENERGY_FLOOR = 0.015;
  private readonly SPECTRAL_MIN = 0.2;
  private readonly SPECTRAL_MAX = 6.5;
  private readonly holdFramesTarget: number;
  private holdFrames = 0;

  constructor() {
    super();
    // Keep gate open for ~500ms to avoid clipping words mid-syllable.
    this.holdFramesTarget = Math.max(8, Math.round((sampleRate * 0.5) / 128));
  }

  process(
    inputs: Float32Array[][],
    _outputs: Float32Array[][],
    _parameters: Record<string, Float32Array>,
  ): boolean {
    const channel = inputs[0]?.[0];
    if (!channel || channel.length < 2) {
      return true;
    }

    let zeroCrossings = 0;
    let energy = 0;
    let spectralEnergy = 0;

    for (let i = 1; i < channel.length; i += 1) {
      const sample = channel[i] ?? 0;
      const prev = channel[i - 1] ?? 0;
      energy += sample * sample;
      const diff = sample - prev;
      spectralEnergy += diff * diff;
      if ((sample >= 0 && prev < 0) || (sample < 0 && prev >= 0)) {
        zeroCrossings += 1;
      }
    }

    const effectiveZcrMax = Math.min(this.ZCR_MAX, channel.length - 1);
    const zcrInRange = zeroCrossings >= this.ZCR_MIN && zeroCrossings <= effectiveZcrMax;
    const enoughEnergy = energy >= this.ENERGY_FLOOR;
    const spectralRatio = spectralEnergy / Math.max(energy, 1e-6);
    const spectralInRange = spectralRatio >= this.SPECTRAL_MIN && spectralRatio <= this.SPECTRAL_MAX;

    if (zcrInRange && enoughEnergy && spectralInRange) {
      this.holdFrames = Math.max(this.holdFrames, this.holdFramesTarget);
    }

    if (this.holdFrames > 0) {
      const out = new Float32Array(channel.length);
      out.set(channel);
      this.port.postMessage({ type: 'voice_gate_chunk', chunk: out }, [out.buffer]);
      this.holdFrames -= 1;
    }

    return true;
  }
}

registerProcessor('spectral-voice-gate', SpectralVoiceGateProcessor);
