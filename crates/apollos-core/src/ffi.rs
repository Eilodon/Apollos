use apollos_proto::contracts::{CarryMode, DistanceCategory, MotionState};
use once_cell::sync::Lazy;
use std::sync::Mutex;

use crate::{
    depth_engine::{DepthEngine, DepthSource},
    carry_mode::get_carry_mode_profile,
    kinematic_gate::{
        compute_risk_score, compute_yaw_delta, should_capture_frame, Acceleration, GyroRotation,
        KinematicReading,
    },
    optical_flow::LumaFrame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FfiAbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

pub const ABI_VERSION: FfiAbiVersion = FfiAbiVersion {
    major: 0,
    minor: 2,
    patch: 0,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApollosKinematicOutput {
    pub risk_score: f32,
    pub should_capture: u8,
    pub yaw_delta_deg: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApollosCarryModeProfile {
    pub cos_tilt_threshold: f32,
    pub pitch_offset: f32,
    pub gyro_threshold: f32,
    pub cloud_enabled: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApollosDepthHazardOutput {
    pub detected: u8,
    pub position_x: f32,
    pub confidence: f32,
    pub source_code: u8,
    pub distance_code: u8,
}

#[derive(Debug)]
struct SharedDepthEngine {
    engine: DepthEngine,
    onnx_probe_done: bool,
}

static DEPTH_ENGINE: Lazy<Mutex<SharedDepthEngine>> = Lazy::new(|| {
    Mutex::new(SharedDepthEngine {
        engine: DepthEngine::default(),
        onnx_probe_done: false,
    })
});

pub fn abi_version() -> FfiAbiVersion {
    ABI_VERSION
}

#[cfg(feature = "ffi")]
pub fn uniffi_feature_enabled() -> bool {
    true
}

#[cfg(not(feature = "ffi"))]
pub fn uniffi_feature_enabled() -> bool {
    false
}

#[no_mangle]
pub extern "C" fn apollos_abi_version_u32() -> u32 {
    ((ABI_VERSION.major as u32) << 16)
        | ((ABI_VERSION.minor as u32) << 8)
        | ABI_VERSION.patch as u32
}

#[no_mangle]
pub extern "C" fn apollos_analyze_kinematics(
    motion_state_code: u8,
    carry_mode_code: u8,
    pitch: f32,
    velocity: f32,
    yaw_delta_deg: f32,
    accel_x: f32,
    accel_y: f32,
    accel_z: f32,
    gyro_alpha: f32,
    gyro_beta: f32,
    gyro_gamma: f32,
    sensor_unavailable: u8,
) -> ApollosKinematicOutput {
    let motion_state = motion_state_from_code(motion_state_code);
    let carry_mode = carry_mode_from_code(carry_mode_code);
    let profile = get_carry_mode_profile(carry_mode);

    let reading = if sensor_unavailable != 0 {
        KinematicReading::default()
    } else {
        KinematicReading {
            accel: Some(Acceleration {
                x: accel_x,
                y: accel_y,
                z: accel_z,
            }),
            gyro: Some(GyroRotation {
                alpha: gyro_alpha,
                beta: gyro_beta,
                gamma: gyro_gamma,
            }),
        }
    };

    let risk_score = compute_risk_score(motion_state, pitch, velocity, yaw_delta_deg);
    let should_capture = should_capture_frame(reading, profile);

    ApollosKinematicOutput {
        risk_score,
        should_capture: u8::from(should_capture),
        yaw_delta_deg,
    }
}

#[no_mangle]
pub extern "C" fn apollos_compute_yaw_delta(alpha_deg_per_second: f32, dt_ms: f32) -> f32 {
    compute_yaw_delta(
        Some(GyroRotation {
            alpha: alpha_deg_per_second,
            beta: 0.0,
            gamma: 0.0,
        }),
        dt_ms,
    )
}

#[no_mangle]
pub extern "C" fn apollos_get_carry_mode_profile(carry_mode_code: u8) -> ApollosCarryModeProfile {
    let profile = get_carry_mode_profile(carry_mode_from_code(carry_mode_code));
    ApollosCarryModeProfile {
        cos_tilt_threshold: profile.cos_tilt_threshold,
        pitch_offset: profile.pitch_offset,
        gyro_threshold: profile.gyro_threshold,
        cloud_enabled: u8::from(profile.cloud_enabled),
    }
}

#[no_mangle]
pub extern "C" fn apollos_depth_onnx_runtime_enabled() -> u8 {
    let Ok(mut guard) = DEPTH_ENGINE.lock() else {
        return 0;
    };
    if !guard.onnx_probe_done {
        let _ = guard.engine.try_enable_onnx_from_env();
        guard.onnx_probe_done = true;
    }
    u8::from(guard.engine.has_onnx_runtime())
}

#[no_mangle]
pub extern "C" fn apollos_detect_drop_ahead_rgba(
    rgba_ptr: *const u8,
    rgba_len: usize,
    width: u32,
    height: u32,
    risk_score: f32,
    carry_mode_code: u8,
    gyro_magnitude: f32,
    now_ms: u64,
) -> ApollosDepthHazardOutput {
    if rgba_ptr.is_null() {
        return ApollosDepthHazardOutput {
            detected: 0,
            position_x: 0.0,
            confidence: 0.0,
            source_code: 0,
            distance_code: 0,
        };
    }

    let width = width as usize;
    let height = height as usize;
    let Some(expected_len) = width.checked_mul(height).and_then(|value| value.checked_mul(4)) else {
        return ApollosDepthHazardOutput {
            detected: 0,
            position_x: 0.0,
            confidence: 0.0,
            source_code: 0,
            distance_code: 0,
        };
    };
    if rgba_len < expected_len || width == 0 || height == 0 {
        return ApollosDepthHazardOutput {
            detected: 0,
            position_x: 0.0,
            confidence: 0.0,
            source_code: 0,
            distance_code: 0,
        };
    }

    let rgba = unsafe { std::slice::from_raw_parts(rgba_ptr, expected_len) };
    let Some(frame) = LumaFrame::from_rgba(width, height, rgba) else {
        return ApollosDepthHazardOutput {
            detected: 0,
            position_x: 0.0,
            confidence: 0.0,
            source_code: 0,
            distance_code: 0,
        };
    };

    let Ok(mut guard) = DEPTH_ENGINE.lock() else {
        return ApollosDepthHazardOutput {
            detected: 0,
            position_x: 0.0,
            confidence: 0.0,
            source_code: 0,
            distance_code: 0,
        };
    };

    if !guard.onnx_probe_done {
        let _ = guard.engine.try_enable_onnx_from_env();
        guard.onnx_probe_done = true;
    }

    let hazard = guard.engine.process(
        &frame,
        risk_score,
        carry_mode_from_code(carry_mode_code),
        gyro_magnitude,
        now_ms,
    );

    let Some(hazard) = hazard else {
        return ApollosDepthHazardOutput {
            detected: 0,
            position_x: 0.0,
            confidence: 0.0,
            source_code: 0,
            distance_code: 0,
        };
    };

    ApollosDepthHazardOutput {
        detected: 1,
        position_x: hazard.position_x,
        confidence: hazard.confidence,
        source_code: match hazard.source {
            DepthSource::Onnx => 1,
            DepthSource::Heuristic => 0,
        },
        distance_code: match hazard.distance {
            DistanceCategory::VeryClose => 0,
            DistanceCategory::Mid => 1,
            DistanceCategory::Far => 2,
        },
    }
}

fn motion_state_from_code(value: u8) -> MotionState {
    match value {
        1 => MotionState::WalkingSlow,
        2 => MotionState::WalkingFast,
        3 => MotionState::Running,
        _ => MotionState::Stationary,
    }
}

fn carry_mode_from_code(value: u8) -> CarryMode {
    match value {
        0 => CarryMode::HandHeld,
        2 => CarryMode::ChestClip,
        3 => CarryMode::Pocket,
        _ => CarryMode::Necklace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_version_packs_into_u32() {
        let packed = apollos_abi_version_u32();
        assert_eq!(packed, 0x0000_0200);
    }

    #[test]
    fn kinematic_ffi_reports_capture_as_u8_flag() {
        let output =
            apollos_analyze_kinematics(2, 1, 12.0, 2.1, 6.0, 0.0, 9.8, 0.2, 3.0, 2.0, 1.0, 0);

        assert!(output.risk_score >= 1.0);
        assert!(output.should_capture == 0 || output.should_capture == 1);
    }
}
