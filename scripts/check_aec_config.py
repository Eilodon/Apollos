#!/usr/bin/env python3
from __future__ import annotations

import pathlib
import re
import sys


TARGET = pathlib.Path('frontend/src/hooks/useAudioStream.ts')
REQUIRED_PATTERNS = {
    'echoCancellation': r'echoCancellation\s*:\s*true',
    'noiseSuppression': r'noiseSuppression\s*:\s*true',
    'autoGainControl': r'autoGainControl\s*:\s*true',
    'sampleRate': r'sampleRate\s*:\s*16000',
    'channelCount': r'channelCount\s*:\s*1',
    'audioWorkletGate': r'AudioWorkletNode\s*\(',
    'zcrMin': r'ZCR_MIN\s*=\s*25',
    'zcrMax': r'ZCR_MAX\s*=\s*150',
    'energyFloor': r'ENERGY_FLOOR\s*=\s*0\.015',
}


def main() -> None:
    if not TARGET.exists():
        print(f'FAIL: file not found: {TARGET}')
        raise SystemExit(1)

    content = TARGET.read_text(encoding='utf-8')
    missing = [name for name, pattern in REQUIRED_PATTERNS.items() if not re.search(pattern, content)]

    if missing:
        print('FAIL: missing required AEC microphone constraints:')
        for name in missing:
            print(f' - {name}')
        raise SystemExit(1)

    print('PASS: required AEC microphone constraints are present.')


if __name__ == '__main__':
    main()
