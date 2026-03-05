export type MotionState = 'stationary' | 'walking_slow' | 'walking_fast' | 'running';
export type DistanceCategory = 'very_close' | 'mid' | 'far';
export type NavigationMode = 'NAVIGATION' | 'EXPLORE' | 'READ' | 'QUIET';
export type CarryMode = 'hand_held' | 'necklace' | 'chest_clip' | 'pocket';
export type SafetyTier = 'silent' | 'ping' | 'voice' | 'hard_stop' | 'human_escalation';

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
  carry_mode?: CarryMode;
  sensor_unavailable?: boolean;
  lat?: number;
  lng?: number;
  heading_deg?: number;
  location_accuracy_m?: number;
  location_age_ms?: number;
  sensor_health?: SensorHealthSnapshot;
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

export interface EdgeHazardMessage {
  type: 'EDGE_HAZARD';
  session_id: string;
  timestamp: string;
  hazard_type: string;
  position_x?: number;
  distance?: DistanceCategory;
  confidence?: number;
  suppress_seconds?: number;
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

export interface SemanticCueMessage {
  type: 'semantic_cue';
  cue: 'approaching_object' | 'soft_obstacle' | 'turning_recommended' | 'destination_near' | 'pocket_mode_active';
  position_x?: number;
}

export interface SensorHealthSnapshot {
  score: number;
  flags: string[];
  degraded: boolean;
  source: string;
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
  state: 'connected' | 'reconnecting' | 'disconnected' | 'degraded';
  detail?: string;
}

export interface SafetyStateMessage {
  type: 'safety_state';
  session_id: string;
  timestamp: string;
  degraded: boolean;
  reason?: string;
  sensor_health_score: number;
  sensor_health_flags?: string[];
  localization_uncertainty_m: number;
  tier: SafetyTier;
}

export interface HumanHelpRTCSession {
  provider: 'twilio' | 'livekit';
  room_name: string;
  identity?: string;
  token: string;
  expires_in: number;
}

export interface HumanHelpSessionMessage {
  type: 'human_help_session';
  session_id: string;
  timestamp: string;
  help_link?: string;
  rtc: HumanHelpRTCSession;
}

export type BackendToClientMessage =
  | AssistantTextMessage
  | AssistantAudioMessage
  | HardStopMessage
  | ConnectionStateMessage
  | SemanticCueMessage
  | SafetyStateMessage
  | HumanHelpSessionMessage;
