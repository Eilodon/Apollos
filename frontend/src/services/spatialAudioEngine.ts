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
      const response = await fetch('/assets/alert_ping.mp3');
      if (!response.ok) {
        return;
      }
      const buffer = await response.arrayBuffer();
      this.sirenBuffer = await this.ctx.decodeAudioData(buffer);
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

  private setPosition(positionX: number): void {
    const clamped = Math.max(-1, Math.min(1, positionX));
    this.panner.setPosition(clamped * 3, 0, -1);
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
    this.setPosition(positionX);

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

    this.setPosition(hazardPositionX);
    const audioBuffer = this.ctx.createBuffer(1, pcmData.length, 24000);
    audioBuffer.getChannelData(0).set(pcmData);

    const source = this.ctx.createBufferSource();
    source.buffer = audioBuffer;
    source.connect(this.panner);
    this.registerSource(source);
    source.start();
  }

  playChunkFromBase64(pcmBase64: string, hazardPositionX = 0): void {
    const pcmData = pcm16Base64ToFloat32(pcmBase64);
    this.playChunk(pcmData, hazardPositionX);
  }
}
