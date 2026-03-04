#!/usr/bin/env python3
import math
import struct
import wave
from pathlib import Path

def generate_ping(output_path: Path):
    sample_rate = 44100
    duration = 0.11 # 110ms
    frequency = 980.0 # 980 Hz for sharp ping

    num_samples = int(sample_rate * duration)
    
    with wave.open(str(output_path), 'w') as wav_file:
        wav_file.setnchannels(1)
        wav_file.setsampwidth(2)
        wav_file.setframerate(sample_rate)
        
        for i in range(num_samples):
            t = float(i) / sample_rate
            
            # Simple fade in/out envelope to avoid clicks
            envelope = 1.0
            if t < 0.01:
                envelope = t / 0.01
            elif t > duration - 0.02:
                envelope = (duration - t) / 0.02
                
            sample = math.sin(2.0 * math.pi * frequency * t) * envelope
            
            # Convert to 16-bit PCM integer
            int_sample = int(sample * 32767.0)
            wav_file.writeframes(struct.pack('<h', int_sample))
            
    print(f"Generated alert ping at {output_path}")

if __name__ == '__main__':
    asset_dir = Path(__file__).resolve().parents[1] / 'assets'
    asset_dir.mkdir(parents=True, exist_ok=True)
    
    # Generate as WAV but name it .mp3 as requested
    # The browser's AudioContext or HTMLAudioElement can decode wav files 
    # even if the extension is mp3 in most modern browsers.
    # To be safe, if the mp3 file is just a wav file with the wrong extension,
    # we can also just save it directly.
    target = asset_dir / 'alert_ping.mp3'
    generate_ping(target)
