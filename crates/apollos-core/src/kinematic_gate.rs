use apollos_proto::contracts::MotionState;

use crate::carry_mode::CarryModeProfile;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Acceleration {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GyroRotation {
    pub alpha: f32,
    pub beta: f32,
    pub gamma: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KinematicReading {
    pub accel: Option<Acceleration>,
    pub gyro: Option<GyroRotation>,
}

pub fn compute_risk_score(
    motion_state: MotionState,
    pitch: f32,
    velocity: f32,
    yaw_delta_deg: f32,
) -> f32 {
    let mut score = 1.0_f32;

    if motion_state == MotionState::WalkingFast {
        score *= 1.5;
    } else if motion_state == MotionState::Running {
        score *= 2.0;
    }

    if pitch.abs() > 20.0 {
        score *= 1.3;
    }

    if yaw_delta_deg.abs() > 30.0 {
        score *= 1.4;
    }

    if velocity > 2.5 && pitch.abs() > 15.0 {
        score *= 1.5;
    }

    score.clamp(1.0, 4.0)
}

pub fn should_capture_frame(reading: KinematicReading, profile: CarryModeProfile) -> bool {
    if !profile.cloud_enabled {
        return false;
    }

    let (accel, gyro) = match (reading.accel, reading.gyro) {
        (Some(accel), Some(gyro)) => (accel, gyro),
        _ => return true,
    };

    let magnitude = (accel.x * accel.x + accel.y * accel.y + accel.z * accel.z).sqrt();
    if magnitude < 1.0 {
        return true;
    }

    let cos_tilt = accel.y.abs() / magnitude;
    let pitch_compensation = (profile.pitch_offset / 100.0).clamp(-0.2, 0.2);
    let corrected_cos_tilt = (cos_tilt + pitch_compensation).min(1.0);
    let is_vertical = corrected_cos_tilt > profile.cos_tilt_threshold;

    let is_stable = gyro.alpha.abs() < profile.gyro_threshold
        && gyro.beta.abs() < profile.gyro_threshold
        && gyro.gamma.abs() < profile.gyro_threshold;

    is_vertical && is_stable
}

pub fn compute_yaw_delta(gyro: Option<GyroRotation>, dt_ms: f32) -> f32 {
    let Some(gyro) = gyro else {
        return 0.0;
    };

    (gyro.alpha * dt_ms) / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carry_mode::get_carry_mode_profile;
    use apollos_proto::contracts::CarryMode;

    #[test]
    fn risk_score_matches_ts_clamping_behavior() {
        let score = compute_risk_score(MotionState::Running, 30.0, 3.0, 40.0);
        assert_eq!(score, 4.0);
    }

    #[test]
    fn pocket_mode_blocks_capture() {
        let profile = get_carry_mode_profile(CarryMode::Pocket);
        let reading = KinematicReading::default();
        assert!(!should_capture_frame(reading, profile));
    }

    #[test]
    fn stable_vertical_frame_is_capture_eligible() {
        let profile = get_carry_mode_profile(CarryMode::Necklace);
        let reading = KinematicReading {
            accel: Some(Acceleration {
                x: 0.0,
                y: 9.8,
                z: 0.3,
            }),
            gyro: Some(GyroRotation {
                alpha: 3.0,
                beta: 2.0,
                gamma: 1.0,
            }),
        };

        assert!(should_capture_frame(reading, profile));
    }

    #[test]
    fn yaw_delta_uses_alpha_deg_per_second() {
        let delta = compute_yaw_delta(
            Some(GyroRotation {
                alpha: 90.0,
                beta: 0.0,
                gamma: 0.0,
            }),
            100.0,
        );
        assert!((delta - 9.0).abs() < 1e-6);
    }
}
