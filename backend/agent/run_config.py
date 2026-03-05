from __future__ import annotations

from google.adk.agents.run_config import RunConfig, StreamingMode
from google.genai import types


def get_run_config() -> RunConfig:
    """Builds ADK run config compatible with current google-adk/google-genai SDKs."""
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
            )
        ),
        enable_affective_dialog=True,
        proactivity=types.ProactivityConfig(proactive_audio=True),
        realtime_input_config=types.RealtimeInputConfig(
            automatic_activity_detection=types.AutomaticActivityDetection(disabled=False)
        ),
    )
