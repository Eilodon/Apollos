from google.adk.run_config import RunConfig, StreamingMode
from google.genai import types

def get_run_config() -> RunConfig:
    """
    RunConfig chính thức cho Gemini Live API BIDI streaming
    (theo ADK Streaming Guide Part 4 - 2026)
    """
    return RunConfig(
        streaming_mode=StreamingMode.BIDI,
        response_modalities=["AUDIO"],                    # Chỉ audio output (voice)
        input_audio_transcription=types.AudioTranscriptionConfig(),
        output_audio_transcription=types.AudioTranscriptionConfig(),
        
        # === SESSION MANAGEMENT (fix 2-min video limit + 10-min connection) ===
        session_resumption=types.SessionResumptionConfig(
            transparent=True                              # ADK tự resume + inject context
        ),
        context_window_compression=types.ContextWindowCompressionConfig(
            trigger_tokens=100000,
            sliding_window=types.SlidingWindow(target_tokens=80000)
        ),
        
        # === SPEECH & AFFECTIVE DIALOG (Emotion-aware + proactive) ===
        speech_config=types.SpeechConfig(
            voice_config=types.PrebuiltVoiceConfig(voice_name="Kore"),
            enable_affective_dialog=True,                 # Native emotion detection
            enable_proactivity=True,                      # Proactive hazard alerts
        ),
        
        # === REALTIME INPUT (VAD + barge-in) ===
        realtime_input_config=types.RealtimeInputConfig(
            automatic_activity_detection=types.AutomaticActivityDetection(
                enabled=True
            )
        ),
        
        thinking_config=types.ThinkingConfig(
            thinking_budget=512
        )
    )
