use apollos_proto::contracts::{CarryMode, DistanceCategory, MotionState};
use once_cell::sync::Lazy;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use crate::{
    carry_mode::get_carry_mode_profile,
    depth_engine::{DepthEngine, DepthSource},
    kinematic_gate::{
        compute_risk_score, compute_yaw_delta, should_capture_frame, Acceleration, GyroRotation,
        KinematicReading,
    },
    optical_flow::{compute_optical_expansion, ExpansionPattern, LumaFrame},
    sensor_fusion::EskfFusionEngine,
};
use nalgebra::Vector3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FfiAbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

pub const ABI_VERSION: FfiAbiVersion = FfiAbiVersion {
    major: 0,
    minor: 5,
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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApollosEskfSnapshot {
    pub sensor_health_score: f32,
    pub degraded: u8,
    pub localization_uncertainty_m: f32,
    pub innovation_norm: f32,
    pub covariance_xx: f32,
    pub covariance_yy: f32,
    pub covariance_zz: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApollosVisionOdometryOutput {
    pub applied: u8,
    pub delta_x_m: f32,
    pub delta_y_m: f32,
    pub pose_x_m: f32,
    pub pose_y_m: f32,
    pub variance_m2: f32,
    pub optical_flow_score: f32,
    pub lateral_bias: f32,
}

#[derive(Debug)]
struct SharedDepthEngine {
    engine: DepthEngine,
    onnx_probe_done: bool,
}

#[derive(Debug, Default)]
struct VisionOdometryState {
    previous_frame: Option<LumaFrame>,
    pose_x_m: f32,
    pose_y_m: f32,
}

#[derive(Debug, Clone, Copy)]
struct VisionOdometryMeasurement {
    delta_x_m: f32,
    delta_y_m: f32,
    pose_x_m: f32,
    pose_y_m: f32,
    variance_m2: f32,
    optical_flow_score: f32,
    lateral_bias: f32,
}

impl VisionOdometryState {
    fn reset(&mut self) {
        self.previous_frame = None;
        self.pose_x_m = 0.0;
        self.pose_y_m = 0.0;
    }

    fn ingest_frame(
        &mut self,
        current_frame: LumaFrame,
        dt_s: f32,
    ) -> Option<VisionOdometryMeasurement> {
        let dt = dt_s.clamp(1e-3, 0.2);
        let previous = self.previous_frame.take();
        let mut measurement = None;

        if let Some(previous_frame) = previous.as_ref() {
            if previous_frame.width == current_frame.width
                && previous_frame.height == current_frame.height
            {
                let expansion = compute_optical_expansion(previous_frame, &current_frame);
                let flow_score = (expansion.avg_diff / 35.0).clamp(0.0, 1.0);
                let pattern_scale = match expansion.pattern {
                    ExpansionPattern::Radial => 1.0,
                    ExpansionPattern::Uniform => 0.85,
                    ExpansionPattern::Directional => 0.65,
                    ExpansionPattern::None => 0.4,
                };
                let forward_speed_mps =
                    ((expansion.avg_diff / 255.0) * 3.2 * pattern_scale).clamp(0.0, 2.2);

                if flow_score >= 0.05 && forward_speed_mps > 1e-3 {
                    let lateral_speed_mps = expansion.lateral_bias * forward_speed_mps * 0.7;
                    let delta_x_m = lateral_speed_mps * dt;
                    let delta_y_m = forward_speed_mps * dt;

                    self.pose_x_m += delta_x_m;
                    self.pose_y_m += delta_y_m;

                    let mut variance_m2 = (1.8 - flow_score * 1.3).clamp(0.25, 2.5);
                    if matches!(expansion.pattern, ExpansionPattern::Directional) {
                        variance_m2 = (variance_m2 * 1.2).clamp(0.25, 3.0);
                    } else if matches!(expansion.pattern, ExpansionPattern::None) {
                        variance_m2 = 3.0;
                    }

                    measurement = Some(VisionOdometryMeasurement {
                        delta_x_m,
                        delta_y_m,
                        pose_x_m: self.pose_x_m,
                        pose_y_m: self.pose_y_m,
                        variance_m2,
                        optical_flow_score: flow_score,
                        lateral_bias: expansion.lateral_bias,
                    });
                }
            }
        }

        self.previous_frame = Some(current_frame);
        measurement
    }
}

#[derive(Debug)]
struct SharedEskfEngine {
    engine: EskfFusionEngine,
    vision_odometry: VisionOdometryState,
}

static DEPTH_ENGINE: Lazy<Mutex<SharedDepthEngine>> = Lazy::new(|| {
    Mutex::new(SharedDepthEngine {
        engine: DepthEngine::default(),
        onnx_probe_done: false,
    })
});

static ESKF_ENGINES: Lazy<Mutex<HashMap<u64, SharedEskfEngine>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static ESKF_NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

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
pub extern "C" fn apollos_eskf_create() -> u64 {
    let Ok(mut guard) = ESKF_ENGINES.lock() else {
        return 0;
    };

    for _ in 0..1024 {
        let handle = ESKF_NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        if handle == 0 {
            continue;
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = guard.entry(handle) {
            entry.insert(SharedEskfEngine {
                engine: EskfFusionEngine::new(),
                vision_odometry: VisionOdometryState::default(),
            });
            return handle;
        }
    }

    0
}

#[no_mangle]
pub extern "C" fn apollos_eskf_destroy(handle: u64) -> u8 {
    if handle == 0 {
        return 0;
    }

    let Ok(mut guard) = ESKF_ENGINES.lock() else {
        return 0;
    };
    u8::from(guard.remove(&handle).is_some())
}

#[no_mangle]
pub extern "C" fn apollos_eskf_reset(handle: u64) -> u8 {
    if handle == 0 {
        return 0;
    }

    let Ok(mut guard) = ESKF_ENGINES.lock() else {
        return 0;
    };

    let Some(shared) = guard.get_mut(&handle) else {
        return 0;
    };
    shared.engine = EskfFusionEngine::new();
    shared.vision_odometry.reset();
    1
}

fn invalid_eskf_snapshot() -> ApollosEskfSnapshot {
    ApollosEskfSnapshot {
        sensor_health_score: 0.0,
        degraded: 1,
        localization_uncertainty_m: 999.0,
        innovation_norm: 10.0,
        covariance_xx: 999.0,
        covariance_yy: 999.0,
        covariance_zz: 999.0,
    }
}

fn invalid_vision_odometry_output() -> ApollosVisionOdometryOutput {
    ApollosVisionOdometryOutput {
        applied: 0,
        delta_x_m: 0.0,
        delta_y_m: 0.0,
        pose_x_m: 0.0,
        pose_y_m: 0.0,
        variance_m2: 999.0,
        optical_flow_score: 0.0,
        lateral_bias: 0.0,
    }
}

#[no_mangle]
pub extern "C" fn apollos_eskf_predict_imu(
    handle: u64,
    accel_x: f32,
    accel_y: f32,
    accel_z: f32,
    dt_s: f32,
) -> u8 {
    if handle == 0 {
        return 0;
    }
    if !accel_x.is_finite() || !accel_y.is_finite() || !accel_z.is_finite() || !dt_s.is_finite() {
        return 0;
    }

    let Ok(mut guard) = ESKF_ENGINES.lock() else {
        return 0;
    };

    let Some(shared) = guard.get_mut(&handle) else {
        return 0;
    };

    shared
        .engine
        .predict_imu(Vector3::new(accel_x, accel_y, accel_z), dt_s);
    1
}

#[no_mangle]
pub extern "C" fn apollos_eskf_update_vision(
    handle: u64,
    position_x: f32,
    position_y: f32,
    position_z: f32,
    variance_m2: f32,
) -> u8 {
    if handle == 0 {
        return 0;
    }
    if !position_x.is_finite()
        || !position_y.is_finite()
        || !position_z.is_finite()
        || !variance_m2.is_finite()
    {
        return 0;
    }

    let Ok(mut guard) = ESKF_ENGINES.lock() else {
        return 0;
    };

    let Some(shared) = guard.get_mut(&handle) else {
        return 0;
    };

    u8::from(shared.engine.update_vision_with_variance(
        Vector3::new(position_x, position_y, position_z),
        variance_m2.max(1e-4),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn apollos_eskf_update_visual_odometry_rgba(
    handle: u64,
    rgba_ptr: *const u8,
    rgba_len: usize,
    width: u32,
    height: u32,
    dt_s: f32,
) -> ApollosVisionOdometryOutput {
    if handle == 0 || rgba_ptr.is_null() || !dt_s.is_finite() {
        return invalid_vision_odometry_output();
    }

    let width = width as usize;
    let height = height as usize;
    let Some(expected_len) = width
        .checked_mul(height)
        .and_then(|value| value.checked_mul(4))
    else {
        return invalid_vision_odometry_output();
    };

    if width == 0 || height == 0 || rgba_len < expected_len {
        return invalid_vision_odometry_output();
    }

    // SAFETY:
    // - caller guarantees rgba_ptr points to at least `expected_len` readable bytes.
    // - pointer is checked for null and bounds are verified above.
    let rgba = unsafe { std::slice::from_raw_parts(rgba_ptr, expected_len) };
    let Some(frame) = LumaFrame::from_rgba(width, height, rgba) else {
        return invalid_vision_odometry_output();
    };

    let Ok(mut guard) = ESKF_ENGINES.lock() else {
        return invalid_vision_odometry_output();
    };
    let Some(shared) = guard.get_mut(&handle) else {
        return invalid_vision_odometry_output();
    };

    let Some(measurement) = shared.vision_odometry.ingest_frame(frame, dt_s) else {
        return invalid_vision_odometry_output();
    };

    let applied = shared.engine.update_vision_with_variance(
        Vector3::new(measurement.pose_x_m, measurement.pose_y_m, 0.0),
        measurement.variance_m2,
    );

    ApollosVisionOdometryOutput {
        applied: u8::from(applied),
        delta_x_m: measurement.delta_x_m,
        delta_y_m: measurement.delta_y_m,
        pose_x_m: measurement.pose_x_m,
        pose_y_m: measurement.pose_y_m,
        variance_m2: measurement.variance_m2,
        optical_flow_score: measurement.optical_flow_score,
        lateral_bias: measurement.lateral_bias,
    }
}

#[no_mangle]
pub extern "C" fn apollos_eskf_snapshot(handle: u64) -> ApollosEskfSnapshot {
    if handle == 0 {
        return invalid_eskf_snapshot();
    }

    let Ok(guard) = ESKF_ENGINES.lock() else {
        return invalid_eskf_snapshot();
    };
    let Some(shared) = guard.get(&handle) else {
        return invalid_eskf_snapshot();
    };

    let health = shared.engine.compute_health();
    let uncertainty = shared.engine.compute_uncertainty();
    let cov = uncertainty.covariance_3x3;

    ApollosEskfSnapshot {
        sensor_health_score: health.score,
        degraded: u8::from(health.degraded),
        localization_uncertainty_m: shared.engine.localization_uncertainty_m(),
        innovation_norm: uncertainty.innovation_norm,
        covariance_xx: cov.first().copied().unwrap_or(0.0),
        covariance_yy: cov.get(4).copied().unwrap_or(0.0),
        covariance_zz: cov.get(8).copied().unwrap_or(0.0),
    }
}

#[no_mangle]
/// Runs depth hazard detection on an RGBA frame and returns a compact ABI-safe output.
///
/// # Safety
/// - `rgba_ptr` must point to at least `width * height * 4` readable bytes.
/// - `rgba_ptr` must remain valid for the duration of this call.
/// - Caller is responsible for ensuring the buffer memory is properly aligned and initialized.
pub unsafe extern "C" fn apollos_detect_drop_ahead_rgba(
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
    let Some(expected_len) = width
        .checked_mul(height)
        .and_then(|value| value.checked_mul(4))
    else {
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

    // SAFETY:
    // - caller guarantees rgba_ptr points to readable memory.
    // - expected_len is validated against width/height with checked arithmetic.
    // - function returns early when pointer is null or buffer length is insufficient.
    let rgba = std::slice::from_raw_parts(rgba_ptr, expected_len);
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
        assert_eq!(packed, 0x0000_0500);
    }

    #[test]
    fn kinematic_ffi_reports_capture_as_u8_flag() {
        let output =
            apollos_analyze_kinematics(2, 1, 12.0, 2.1, 6.0, 0.0, 9.8, 0.2, 3.0, 2.0, 1.0, 0);

        assert!(output.risk_score >= 1.0);
        assert!(output.should_capture == 0 || output.should_capture == 1);
    }

    #[test]
    fn eskf_ffi_predict_and_snapshot_flow() {
        let handle = apollos_eskf_create();
        assert_ne!(handle, 0);

        let ok_predict = apollos_eskf_predict_imu(handle, 0.1, -0.05, 0.0, 0.02);
        let ok_update = apollos_eskf_update_vision(handle, 0.0, 0.0, 0.0, 0.5);
        let snapshot = apollos_eskf_snapshot(handle);

        assert_eq!(ok_predict, 1);
        assert!(ok_update == 0 || ok_update == 1);
        assert!(snapshot.sensor_health_score.is_finite());
        assert!(snapshot.localization_uncertainty_m.is_finite());

        let destroyed = apollos_eskf_destroy(handle);
        assert_eq!(destroyed, 1);
        assert_eq!(apollos_eskf_predict_imu(handle, 0.0, 0.0, 0.0, 0.02), 0);
    }

    #[test]
    fn eskf_ffi_handles_are_isolated() {
        let left = apollos_eskf_create();
        let right = apollos_eskf_create();
        assert_ne!(left, 0);
        assert_ne!(right, 0);
        assert_ne!(left, right);

        assert_eq!(apollos_eskf_predict_imu(left, 0.2, 0.0, 0.0, 0.05), 1);

        let left_snapshot = apollos_eskf_snapshot(left);
        let right_snapshot = apollos_eskf_snapshot(right);
        assert!(left_snapshot.covariance_xx > right_snapshot.covariance_xx);

        assert_eq!(apollos_eskf_destroy(left), 1);
        assert_eq!(apollos_eskf_destroy(right), 1);
    }

    #[test]
    fn eskf_visual_odometry_updates_pose_after_second_frame() {
        let handle = apollos_eskf_create();
        assert_ne!(handle, 0);

        let width = 32_u32;
        let height = 24_u32;
        let mut frame_a = vec![0_u8; (width as usize) * (height as usize) * 4];
        let mut frame_b = vec![0_u8; (width as usize) * (height as usize) * 4];

        for pixel in frame_a.chunks_exact_mut(4) {
            pixel[0] = 32;
            pixel[1] = 32;
            pixel[2] = 32;
            pixel[3] = 255;
        }
        for pixel in frame_b.chunks_exact_mut(4) {
            pixel[0] = 96;
            pixel[1] = 96;
            pixel[2] = 96;
            pixel[3] = 255;
        }

        // SAFETY: test buffers are valid RGBA and pointers remain alive for the call.
        let first = unsafe {
            apollos_eskf_update_visual_odometry_rgba(
                handle,
                frame_a.as_ptr(),
                frame_a.len(),
                width,
                height,
                0.033,
            )
        };
        assert_eq!(first.applied, 0);

        // SAFETY: test buffers are valid RGBA and pointers remain alive for the call.
        let second = unsafe {
            apollos_eskf_update_visual_odometry_rgba(
                handle,
                frame_b.as_ptr(),
                frame_b.len(),
                width,
                height,
                0.033,
            )
        };
        assert!(second.optical_flow_score > 0.0);
        assert!(second.pose_y_m >= 0.0);
        assert!(second.variance_m2.is_finite());

        assert_eq!(apollos_eskf_destroy(handle), 1);
    }
}
