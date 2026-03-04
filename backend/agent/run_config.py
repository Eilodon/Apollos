from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(slots=True)
class FallbackRunConfig:
    """Used when Google ADK packages are unavailable locally."""

    payload: dict[str, Any]


def build_run_config() -> Any:
    payload = {
        'streaming_mode': 'BIDI',
        'response_modalities': ['AUDIO'],
        'session_resumption': {'transparent': True},
        'context_window_compression': {
            'trigger_tokens': 100000,
            'sliding_window': {'target_tokens': 80000},
        },
        'speech_config': {
            'voice_name': 'Kore',
            'enable_affective_dialog': True,
            'enable_proactivity': True,
        },
    }

    try:
        from google.adk.agents.run_config import RunConfig, StreamingMode
        from google.genai import types
    except Exception:
        return FallbackRunConfig(payload=payload)

    return RunConfig(
        streaming_mode=StreamingMode.BIDI,
        response_modalities=['AUDIO'],
        input_audio_transcription=types.AudioTranscriptionConfig(),
        output_audio_transcription=types.AudioTranscriptionConfig(),
        session_resumption=types.SessionResumptionConfig(transparent=True),
        context_window_compression=types.ContextWindowCompressionConfig(
            trigger_tokens=100000,
            sliding_window=types.SlidingWindow(target_tokens=80000),
        ),
        speech_config=types.SpeechConfig(
            voice_config=types.VoiceConfig(
                prebuilt_voice_config=types.PrebuiltVoiceConfig(voice_name='Kore')
            ),
            enable_affective_dialog=True,
            enable_proactivity=True,
        ),
    )
