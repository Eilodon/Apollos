import { DistanceCategory } from '../types/contracts';
import { pcm16Base64ToFloat32 } from '../utils/pcm';

const PING_INTERVALS: Record<DistanceCategory, number> = {
  very_close: 100,
  mid: 400,
  far: 800,
};

export class SpatialAudioEngine {
  private readonly ctx: AudioContext;
  private readonly panner: PannerNode;
  private sirenBuffer: AudioBuffer | null = null;
  private pingInterval: number | null = null;
  private activeSources = new Set<AudioBufferSourceNode>();
  private nextPlayTime: number = 0;
  private readonly JITTER_DELAY = 0.1;

  constructor() {
    const Ctx = window.AudioContext || (window as typeof window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Ctx) {
      throw new Error('Web Audio API is not supported on this browser.');
    }
    this.ctx = new Ctx();
    this.panner = this.ctx.createPanner();
    this.panner.panningModel = 'HRTF';
    this.panner.distanceModel = 'inverse';
    this.panner.refDistance = 1;
    this.panner.maxDistance = 10000;
    this.panner.rolloffFactor = 1;
    this.panner.coneInnerAngle = 360;
    this.panner.coneOuterAngle = 0;
    this.panner.coneOuterGain = 0;
    this.panner.connect(this.ctx.destination);
    this.ctx.listener.setPosition(0, 0, 0);
  }

  async warmup(): Promise<void> {
    if (this.ctx.state === 'suspended') {
      await this.ctx.resume();
    }
    if (this.sirenBuffer) {
      return;
    }

    try {
      const base64Data = "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";
      const binaryString = window.atob(base64Data);
      const len = binaryString.length;
      const bytes = new Uint8Array(len);
      for (let i = 0; i < len; i++) {
        bytes[i] = binaryString.charCodeAt(i);
      }
      this.sirenBuffer = await this.ctx.decodeAudioData(bytes.buffer);
    } catch {
      // Falls back to oscillator beep if asset decode fails.
    }
  }

  stopAll(): void {
    this.stopPing();
    this.activeSources.forEach((source) => {
      try {
        source.stop();
      } catch {
        // Ignore node stop errors.
      }
    });
    this.activeSources.clear();
  }

  private setPosition(positionX: number, distance: DistanceCategory | 'default' = 'default'): void {
    const clamped = Math.max(-1, Math.min(1, positionX));
    let z = -3;
    if (distance === 'very_close') z = -1;
    if (distance === 'far') z = -6;
    this.panner.setPosition(clamped * 3, 0, z);
  }

  private registerSource(source: AudioBufferSourceNode): void {
    this.activeSources.add(source);
    source.addEventListener('ended', () => {
      this.activeSources.delete(source);
    });
  }

  private playOscillatorPing(): void {
    const oscillator = this.ctx.createOscillator();
    const gain = this.ctx.createGain();
    oscillator.frequency.value = 980;
    gain.gain.value = 0.1;
    oscillator.connect(gain);
    gain.connect(this.panner);
    oscillator.start();
    oscillator.stop(this.ctx.currentTime + 0.08);
  }

  private playPingOnce(): void {
    if (!this.sirenBuffer) {
      this.playOscillatorPing();
      return;
    }

    const source = this.ctx.createBufferSource();
    source.buffer = this.sirenBuffer;
    source.connect(this.panner);
    this.registerSource(source);
    source.start();
  }

  fireHardStop(positionX: number, distance: DistanceCategory): void {
    this.stopAll();
    this.setPosition(positionX, distance);

    const interval = PING_INTERVALS[distance];
    this.playPingOnce();
    this.pingInterval = window.setInterval(() => {
      this.playPingOnce();
    }, interval);

    window.setTimeout(() => {
      this.stopPing();
    }, 3000);
  }

  stopPing(): void {
    if (this.pingInterval !== null) {
      window.clearInterval(this.pingInterval);
      this.pingInterval = null;
    }
  }

  playChunk(pcmData: Float32Array, hazardPositionX = 0): void {
    if (!pcmData.length) {
      return;
    }

    this.setPosition(hazardPositionX, 'default');
    const audioBuffer = this.ctx.createBuffer(1, pcmData.length, 24000);
    audioBuffer.getChannelData(0).set(pcmData);

    const source = this.ctx.createBufferSource();
    source.buffer = audioBuffer;
    source.connect(this.panner);
    this.registerSource(source);

    const currentTime = this.ctx.currentTime;
    if (this.nextPlayTime < currentTime) {
      this.nextPlayTime = currentTime + this.JITTER_DELAY;
    }

    source.start(this.nextPlayTime);
    this.nextPlayTime += audioBuffer.duration;
  }

  playChunkFromBase64(pcmBase64: string, hazardPositionX = 0): void {
    const pcmData = pcm16Base64ToFloat32(pcmBase64);
    this.playChunk(pcmData, hazardPositionX);
  }
}
