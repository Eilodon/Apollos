export interface CachedAudioEntry {
  timestamp: string;
  text?: string;
  pcmBase64?: string;
}

export class AudioCache {
  private readonly maxEntries: number;
  private readonly entries: CachedAudioEntry[] = [];

  constructor(maxEntries = 3) {
    this.maxEntries = maxEntries;
  }

  add(entry: CachedAudioEntry): void {
    this.entries.push(entry);
    while (this.entries.length > this.maxEntries) {
      this.entries.shift();
    }
  }

  getLast(): CachedAudioEntry | undefined {
    return this.entries[this.entries.length - 1];
  }

  getAll(): CachedAudioEntry[] {
    return [...this.entries];
  }
}
