use apollos_proto::contracts::SensorHealthSnapshot;

#[derive(Debug, Clone, Copy, Default)]
pub struct SensorSample {
    pub imu_available: bool,
    pub location_available: bool,
    pub camera_available: bool,
    pub depth_available: bool,
    pub location_accuracy_m: Option<f32>,
}

pub fn compute_sensor_health(sample: SensorSample) -> SensorHealthSnapshot {
    let mut score = 1.0_f32;
    let mut flags = Vec::new();

    if !sample.imu_available {
        score -= 0.35;
        flags.push("imu_unavailable".to_string());
    }
    if !sample.camera_available {
        score -= 0.30;
        flags.push("camera_unavailable".to_string());
    }
    if !sample.depth_available {
        score -= 0.15;
        flags.push("depth_fallback".to_string());
    }
    if !sample.location_available {
        score -= 0.10;
        flags.push("location_unavailable".to_string());
    }

    if let Some(accuracy) = sample.location_accuracy_m {
        if accuracy > 30.0 {
            score -= 0.10;
            flags.push("gps_low_accuracy".to_string());
        }
    }

    let clamped = score.clamp(0.0, 1.0);

    SensorHealthSnapshot {
        score: clamped,
        flags,
        degraded: clamped < 0.6,
        source: "edge-fused-rust-v1".to_string(),
    }
}
