export type MotionState = 'stationary' | 'walking_slow' | 'walking_fast' | 'running';
export type DistanceCategory = 'very_close' | 'mid' | 'far';
export type NavigationMode = 'NAVIGATION' | 'EXPLORE' | 'READ' | 'QUIET';
export type CarryMode = 'hand_held' | 'necklace' | 'chest_clip' | 'pocket';
export type CognitionLayer = 'l1_survival' | 'l2_edge' | 'l3_cloud';
export type HumanHelpProvider = 'twilio' | 'livekit';

export const HAZARD_TYPE = {
  UNSPECIFIED: 0,
  DROP_AHEAD: 1,
  STATIC_OBSTACLE: 2,
  DYNAMIC_OBSTACLE: 3,
  VEHICLE: 4,
} as const;

export type HazardType = typeof HAZARD_TYPE[keyof typeof HAZARD_TYPE];

export interface MotionSnapshot {
  state: MotionState;
  pitch: number;
  velocity: number;
}

export interface MultimodalFrameMessage {
  type: 'multimodal_frame';
  session_id: string;
  timestamp_ms: number;
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
  edge_semantic_cues: EdgeSemanticCueMessage[];
}

export interface AudioChunkMessage {
  type: 'audio_chunk';
  session_id: string;
  timestamp_ms: number;
  audio_chunk_pcm16: string;
}

export interface UserCommandMessage {
  type: 'user_command';
  session_id: string;
  timestamp_ms: number;
  command: string;
}

export interface EdgeSemanticCueMessage {
  cue_type: string;
  text?: string;
  confidence: number;
  position_x?: number;
  distance_m?: number;
  position_clock?: string;
  ttl_ms?: number;
  source: string;
}

export interface HazardObservationMessage {
  type: 'hazard_observation';
  session_id: string;
  timestamp_ms: number;
  hazard_type: HazardType;
  bearing_x?: number;
  distance_m: number;
  relative_velocity_mps: number;
  confidence?: number;
  source?: string;
  suppress_ms?: number;
}

export interface AssistantTextMessage {
  type: 'assistant_text';
  session_id: string;
  timestamp_ms: number;
  text: string;
}

export interface AssistantAudioMessage {
  type: 'audio_chunk';
  session_id: string;
  timestamp_ms: number;
  pcm24?: string;
  pcm16?: string;
  hazard_position_x?: number;
}

export interface SemanticCueMessage {
  type: 'semantic_cue';
  cue: 'approaching_object' | 'soft_obstacle' | 'turning_recommended' | 'destination_near' | 'pocket_mode_active';
  position_x?: number;
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

export interface SafetyDirectiveMessage {
  type: 'safety_directive';
  session_id: string;
  timestamp_ms: number;
  hazard_type?: HazardType;
  hazard_score: number;
  hard_stop: boolean;
  haptic_intensity: number;
  spatial_audio_pitch_hz: number;
  spatial_audio_pan: number;
  needs_human_assistance: boolean;
  reason?: string;
  flush_audio: boolean;
}

export interface HumanHelpSessionMessage {
  type: 'human_help_session';
  session_id: string;
  timestamp_ms: number;
  help_link?: string;
  rtc: {
    provider: HumanHelpProvider;
    room_name: string;
    identity?: string;
    token: string;
    expires_in: number;
  };
}

export interface CognitionStateMessage {
  type: 'cognition_state';
  session_id: string;
  timestamp_ms: number;
  active_layer: CognitionLayer;
  cloud_link_healthy: boolean;
  edge_cognition_available: boolean;
  cloud_rtt_ms?: number;
  reason?: string;
}

export type BackendToClientMessage =
  | AssistantTextMessage
  | AssistantAudioMessage
  | SafetyDirectiveMessage
  | ConnectionStateMessage
  | SemanticCueMessage
  | HumanHelpSessionMessage
  | CognitionStateMessage;

export function hazardTypeToLabel(hazardType?: HazardType | null): string {
  switch (hazardType) {
    case HAZARD_TYPE.DROP_AHEAD:
      return 'drop_ahead';
    case HAZARD_TYPE.STATIC_OBSTACLE:
      return 'static_obstacle';
    case HAZARD_TYPE.DYNAMIC_OBSTACLE:
      return 'dynamic_obstacle';
    case HAZARD_TYPE.VEHICLE:
      return 'vehicle';
    default:
      return 'unspecified';
  }
}

export function parseHazardType(value: string | HazardType | null | undefined): HazardType {
  if (typeof value === 'number') {
    if (Object.values(HAZARD_TYPE).includes(value as HazardType)) {
      return value as HazardType;
    }
    return HAZARD_TYPE.UNSPECIFIED;
  }

  const normalized = value?.trim().toLowerCase();
  switch (normalized) {
    case 'drop_ahead':
    case 'dropahead':
    case 'edge_drop_hazard':
      return HAZARD_TYPE.DROP_AHEAD;
    case 'static_obstacle':
    case 'staticobstacle':
    case 'pole':
      return HAZARD_TYPE.STATIC_OBSTACLE;
    case 'dynamic_obstacle':
    case 'dynamicobstacle':
    case 'moving_obstacle':
      return HAZARD_TYPE.DYNAMIC_OBSTACLE;
    case 'vehicle':
    case 'bike':
    case 'motorbike':
    case 'car':
      return HAZARD_TYPE.VEHICLE;
    default:
      return HAZARD_TYPE.UNSPECIFIED;
  }
}

export function distanceCategoryToMeters(distance: DistanceCategory): number {
  switch (distance) {
    case 'very_close':
      return 0.45;
    case 'mid':
      return 1.5;
    case 'far':
      return 3.5;
  }
}

export function metersToDistanceCategory(distanceM: number): DistanceCategory {
  if (distanceM <= 0.8) {
    return 'very_close';
  }
  if (distanceM <= 2.5) {
    return 'mid';
  }
  return 'far';
}
