export type MotionState = 'stationary' | 'walking_slow' | 'walking_fast' | 'running';
export type DistanceCategory = 'very_close' | 'mid' | 'far';
export type NavigationMode = 'NAVIGATION' | 'EXPLORE' | 'READ' | 'QUIET';

export interface MotionSnapshot {
  state: MotionState;
  pitch: number;
  velocity: number;
}

export interface MultimodalFrameMessage {
  type: 'multimodal_frame';
  session_id: string;
  timestamp: string;
  frame_jpeg_base64?: string;
  motion_state: MotionState;
  pitch: number;
  velocity: number;
  user_text?: string;
  /** Góc xoay ngang tích lũy (độ) kể từ frame trước → Semantic Odometry */
  yaw_delta_deg?: number;
}

export interface AudioChunkMessage {
  type: 'audio_chunk';
  session_id: string;
  timestamp: string;
  audio_chunk_pcm16: string;
}

export interface UserCommandMessage {
  type: 'user_command';
  session_id: string;
  timestamp: string;
  command: string;
}

export interface AssistantTextMessage {
  type: 'assistant_text';
  session_id: string;
  timestamp: string;
  text: string;
}

export interface AssistantAudioMessage {
  type: 'audio_chunk';
  session_id: string;
  timestamp: string;
  pcm24?: string;
  pcm16?: string;
  hazard_position_x?: number;
}

export interface HardStopMessage {
  type: 'HARD_STOP';
  position_x: number;
  distance: DistanceCategory;
  hazard_type: string;
  confidence: number;
  ts?: string;
}

export interface ConnectionStateMessage {
  type: 'connection_state';
  state: 'connected' | 'reconnecting' | 'disconnected';
  detail?: string;
}

export type BackendToClientMessage =
  | AssistantTextMessage
  | AssistantAudioMessage
  | HardStopMessage
  | ConnectionStateMessage;
