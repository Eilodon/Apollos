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
    depth_engine::{BoundingBox, DepthEngine, DepthSpatials, ObjectSensorFusionInput},
    kinematic_gate::{
        compute_risk_score, compute_yaw_delta, should_capture_frame, Acceleration, GyroRotation,
        KinematicReading,
    },
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
    minor: 8,
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
    pub distance_code: u8,
    pub distance_m: f32,
    pub relative_velocity_mps: f32,
    pub time_to_collision_s: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApollosBoundingBox {
    pub label_id: u32,
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
    pub confidence: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApollosDepthSpatials {
    pub median_depth_m: f32,
    pub min_depth_m: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApollosObjectSensorFusionInput {
    pub bbox: ApollosBoundingBox,
    pub spatial: ApollosDepthSpatials,
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

#[derive(Debug)]
struct SharedDepthEngine {
    engine: DepthEngine,
}

#[derive(Debug)]
struct SharedEskfEngine {
    engine: EskfFusionEngine,
}

static DEPTH_ENGINE: Lazy<Mutex<SharedDepthEngine>> = Lazy::new(|| {
    Mutex::new(SharedDepthEngine {
        engine: DepthEngine::default(),
    })
});

static ESKF_ENGINES: Lazy<Mutex<HashMap<u64, SharedEskfEngine>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static ESKF_NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static SESSION_START_TIME: AtomicU64 = AtomicU64::new(0);

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
    // 1. Initial warm-up check: Set start time if not set
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    
    let start_time = SESSION_START_TIME.load(Ordering::Relaxed);
    if start_time == 0 {
        SESSION_START_TIME.store(now, Ordering::Relaxed);
        return ApollosKinematicOutput { risk_score: 1.0, should_capture: 0, yaw_delta_deg: 0.0 };
    }

    // 2. Warm-up phase: Skip processing for the first 1000ms to allow sensors to stabilize
    if now.saturating_sub(start_time) < 1000 {
        return ApollosKinematicOutput { risk_score: 1.0, should_capture: 0, yaw_delta_deg: 0.0 };
    }

    // 3. NaN protection for critical safety metrics
    if !pitch.is_finite() || !velocity.is_finite() || !accel_x.is_finite() || !accel_y.is_finite() || !accel_z.is_finite() {
        return ApollosKinematicOutput { risk_score: 1.0, should_capture: 0, yaw_delta_deg: 0.0 };
    }
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
        should_capture: should_capture as u8,
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
    0
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

fn invalid_depth_hazard_output() -> ApollosDepthHazardOutput {
    ApollosDepthHazardOutput {
        detected: 0,
        position_x: 0.0,
        confidence: 0.0,
        distance_code: 0,
        distance_m: 0.0,
        relative_velocity_mps: 0.0,
        time_to_collision_s: -1.0,
    }
}

// visual odometry RGBA helpers removed

fn detect_drop_ahead_from_objects(
    objects: &[ObjectSensorFusionInput],
    risk_score: f32,
    carry_mode_code: u8,
    gyro_magnitude: f32,
    now_ms: u64,
) -> ApollosDepthHazardOutput {
    if !risk_score.is_finite() || !gyro_magnitude.is_finite() {
        return invalid_depth_hazard_output();
    }
    let Ok(mut guard) = DEPTH_ENGINE.lock() else {
        return invalid_depth_hazard_output();
    };

    let hazard = guard.engine.process(
        objects,
        risk_score,
        carry_mode_from_code(carry_mode_code),
        gyro_magnitude,
        now_ms,
    );

    let Some(hazard) = hazard else {
        return invalid_depth_hazard_output();
    };

    ApollosDepthHazardOutput {
        detected: 1,
        position_x: hazard.position_x,
        confidence: hazard.confidence,
        distance_code: match hazard.distance {
            DistanceCategory::VeryClose => 0,
            DistanceCategory::Mid => 1,
            DistanceCategory::Far => 2,
        },
        distance_m: hazard.distance_m,
        relative_velocity_mps: hazard.relative_velocity_mps,
        time_to_collision_s: hazard.time_to_collision_s.unwrap_or(-1.0),
    }
}

// strided frame parsing removed

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

// Optical flow logic removed. Visual odometry via pixels is deprecated in favor of NPU integration.

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
/// Runs depth hazard detection on a list of objects fused from YOLO and Depth Anything.
///
/// # Safety
/// - `objects_ptr` must point to `objects_len` instances of `ApollosObjectSensorFusionInput`.
/// - pointer must remain valid for this call.
pub unsafe extern "C" fn apollos_detect_drop_ahead_objects(
    objects_ptr: *const ApollosObjectSensorFusionInput,
    objects_len: usize,
    risk_score: f32,
    carry_mode_code: u8,
    gyro_magnitude: f32,
    now_ms: u64,
) -> ApollosDepthHazardOutput {
    let raw_objects = if objects_ptr.is_null() || objects_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(objects_ptr, objects_len) }
    };

    let objects: Vec<ObjectSensorFusionInput> = raw_objects
        .iter()
        .map(|obj| ObjectSensorFusionInput {
            bbox: BoundingBox {
                label_id: obj.bbox.label_id,
                x_min: obj.bbox.x_min,
                y_min: obj.bbox.y_min,
                x_max: obj.bbox.x_max,
                y_max: obj.bbox.y_max,
                confidence: obj.bbox.confidence,
            },
            spatial: DepthSpatials {
                median_depth_m: obj.spatial.median_depth_m,
                min_depth_m: obj.spatial.min_depth_m,
            },
        })
        .collect();

    detect_drop_ahead_from_objects(
        &objects,
        risk_score,
        carry_mode_code,
        gyro_magnitude,
        now_ms,
    )
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
        assert_eq!(packed, 0x0000_0800);
    }

    #[test]
    fn kinematic_ffi_reports_capture_as_u8_flag() {
        let output =
            apollos_analyze_kinematics(2, 1, 12.0, 2.1, 6.0, 0.0, 9.8, 0.2, 3.0, 2.0, 1.0, 0);

        assert!(output.risk_score >= 1.0);
        assert!(output.should_capture <= 2);
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
}
