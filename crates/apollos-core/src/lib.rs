//! Edge-brain primitives for Apollos native shells.

pub mod barcode_scan;
pub mod battery_gov;
pub mod carry_mode;
pub mod depth_engine;
pub mod ffi;
pub mod kinematic_gate;
pub mod optical_flow;
pub mod pocket_shield;
pub mod safety_policy;
pub mod sensor_fusion;
pub mod session;
pub mod spatial_audio;
pub mod ws_client;

pub use carry_mode::{get_carry_mode_profile, CarryModeProfile, DEFAULT_CARRY_MODE};
pub use depth_engine::{DepthEngine, DepthSource, DropAheadHazard};
pub use kinematic_gate::{
    compute_risk_score, compute_yaw_delta, should_capture_frame, Acceleration, GyroRotation,
    KinematicReading,
};
pub use optical_flow::{LumaFrame, SurvivalReflexEngine};
