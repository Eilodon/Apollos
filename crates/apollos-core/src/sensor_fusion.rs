use apollos_proto::contracts::{SensorHealthSnapshot, SensorUncertaintySnapshot};
use nalgebra::{Matrix3, Matrix3x6, Matrix6, Vector3};

#[derive(Debug, Clone, Copy, Default)]
pub struct SensorSample {
    pub imu_available: bool,
    pub location_available: bool,
    pub camera_available: bool,
    pub depth_available: bool,
    pub location_accuracy_m: Option<f32>,
}

/// Compact continuous-state fusion (position + velocity) for edge observability.
///
/// This is a lightweight ESKF-style approximation for T1 use:
/// - `predict_imu` propagates state and covariance with IMU acceleration.
/// - `update_vision` corrects position from camera/depth observation.
#[derive(Debug, Clone)]
pub struct EskfFusionEngine {
    pub covariance: Matrix6<f32>,
    pub position: Vector3<f32>,
    pub velocity: Vector3<f32>,
    imu_noise_variance: f32,
    vision_noise_variance: f32,
    last_innovation_norm: f32,
}

impl Default for EskfFusionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl EskfFusionEngine {
    pub fn new() -> Self {
        Self {
            covariance: Matrix6::identity() * 0.5,
            position: Vector3::zeros(),
            velocity: Vector3::zeros(),
            imu_noise_variance: 0.1,
            vision_noise_variance: 0.02,
            last_innovation_norm: 0.0,
        }
    }

    pub fn with_noise(mut self, imu_noise_variance: f32, vision_noise_variance: f32) -> Self {
        self.imu_noise_variance = imu_noise_variance.max(1e-6);
        self.vision_noise_variance = vision_noise_variance.max(1e-6);
        self
    }

    pub fn predict_imu(&mut self, accel_m_s2: Vector3<f32>, dt_s: f32) {
        let dt = dt_s.clamp(1e-4, 0.2);
        let dt2 = dt * dt;
        let dt3 = dt2 * dt;
        let dt4 = dt2 * dt2;

        self.position += self.velocity * dt + accel_m_s2 * (0.5 * dt2);
        self.velocity += accel_m_s2 * dt;

        let mut f = Matrix6::identity();
        f.fixed_view_mut::<3, 3>(0, 3)
            .copy_from(&(Matrix3::identity() * dt));

        // White-acceleration model for process covariance propagation.
        let q_acc = self.imu_noise_variance.max(1e-6);
        let q_pos = 0.25 * dt4 * q_acc;
        let q_cross = 0.5 * dt3 * q_acc;
        let q_vel = dt2 * q_acc;

        let mut q = Matrix6::zeros();
        q.fixed_view_mut::<3, 3>(0, 0)
            .copy_from(&(Matrix3::identity() * q_pos));
        q.fixed_view_mut::<3, 3>(0, 3)
            .copy_from(&(Matrix3::identity() * q_cross));
        q.fixed_view_mut::<3, 3>(3, 0)
            .copy_from(&(Matrix3::identity() * q_cross));
        q.fixed_view_mut::<3, 3>(3, 3)
            .copy_from(&(Matrix3::identity() * q_vel));

        self.covariance = f * self.covariance * f.transpose() + q;
        self.repair_covariance();
    }

    pub fn update_vision(&mut self, vision_pos: Vector3<f32>) -> bool {
        self.update_vision_with_variance(vision_pos, self.vision_noise_variance)
    }

    pub fn update_vision_with_variance(&mut self, vision_pos: Vector3<f32>, variance: f32) -> bool {
        if !vision_pos.iter().all(|value| value.is_finite()) {
            return false;
        }

        let mut h = Matrix3x6::zeros();
        h.fixed_view_mut::<3, 3>(0, 0)
            .copy_from(&Matrix3::identity());

        let r = Matrix3::identity() * variance.max(1e-6);
        let y = vision_pos - self.position;
        let s: Matrix3<f32> = h * self.covariance * h.transpose() + r;
        let Some(s_inv) = s.try_inverse() else {
            return false;
        };

        let k = self.covariance * h.transpose() * s_inv;
        let delta_x = k * y;

        self.position += delta_x.fixed_view::<3, 1>(0, 0).into_owned();
        self.velocity += delta_x.fixed_view::<3, 1>(3, 0).into_owned();

        // Joseph update for numeric stability / PSD preservation.
        let i = Matrix6::identity();
        let i_kh = i - k * h;
        self.covariance = i_kh * self.covariance * i_kh.transpose() + k * r * k.transpose();
        self.repair_covariance();

        let nis = (y.transpose() * s_inv * y)[(0, 0)];
        self.last_innovation_norm = nis.max(0.0).sqrt();
        true
    }

    pub fn localization_uncertainty_m(&self) -> f32 {
        let pos_trace = self.covariance[(0, 0)] + self.covariance[(1, 1)] + self.covariance[(2, 2)];
        pos_trace.max(0.0).sqrt()
    }

    pub fn compute_health(&self) -> SensorHealthSnapshot {
        let uncertainty = self.localization_uncertainty_m();
        let score = (-0.5 * uncertainty).exp().clamp(0.0, 1.0);
        let degraded = score < 0.6 || uncertainty > 2.0;

        let mut flags = Vec::new();
        if uncertainty > 2.0 {
            flags.push("critical_uncertainty".to_string());
        }
        if uncertainty > 4.0 {
            flags.push("tracking_lost".to_string());
        }

        SensorHealthSnapshot {
            score,
            flags,
            degraded,
            source: "eskf-continuous-math-v3".to_string(),
        }
    }

    pub fn compute_uncertainty(&self) -> SensorUncertaintySnapshot {
        let covariance_3x3 = vec![
            self.covariance[(0, 0)],
            self.covariance[(0, 1)],
            self.covariance[(0, 2)],
            self.covariance[(1, 0)],
            self.covariance[(1, 1)],
            self.covariance[(1, 2)],
            self.covariance[(2, 0)],
            self.covariance[(2, 1)],
            self.covariance[(2, 2)],
        ];

        SensorUncertaintySnapshot {
            covariance_3x3,
            innovation_norm: self.last_innovation_norm,
            source: "eskf-continuous-math-v3".to_string(),
        }
    }

    fn repair_covariance(&mut self) {
        self.covariance = (self.covariance + self.covariance.transpose()) * 0.5;
        for idx in 0..6 {
            if self.covariance[(idx, idx)] < 1e-6 {
                self.covariance[(idx, idx)] = 1e-6;
            }
        }
    }
}

pub fn compute_sensor_health(sample: SensorSample) -> SensorHealthSnapshot {
    fusion_engine_from_sample(sample).compute_health()
}

pub fn compute_sensor_uncertainty(sample: SensorSample) -> SensorUncertaintySnapshot {
    fusion_engine_from_sample(sample).compute_uncertainty()
}

fn fusion_engine_from_sample(sample: SensorSample) -> EskfFusionEngine {
    let mut engine = EskfFusionEngine::new();

    let mut pos_var = 0.8_f32;
    let mut vel_var = 0.4_f32;
    let mut drift_var = 0.5_f32;

    if !sample.imu_available {
        vel_var += 2.6;
        drift_var += 2.0;
    }
    if !sample.camera_available {
        pos_var += 2.2;
    }
    if !sample.depth_available {
        pos_var += 1.4;
    }
    if !sample.location_available {
        pos_var += 2.8;
    }

    if let Some(accuracy_m) = sample.location_accuracy_m {
        pos_var += (accuracy_m / 6.0).powi(2).min(100.0);
    } else {
        pos_var += 6.0;
    }

    let mut covariance = Matrix6::identity() * 0.1;
    covariance[(0, 0)] = pos_var;
    covariance[(1, 1)] = pos_var;
    covariance[(2, 2)] = pos_var + 0.3;
    covariance[(3, 3)] = vel_var;
    covariance[(4, 4)] = vel_var;
    covariance[(5, 5)] = drift_var;
    engine.covariance = covariance;
    engine.last_innovation_norm = ((pos_var + vel_var + drift_var).sqrt() / 5.0).clamp(0.0, 10.0);
    engine
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predict_step_increases_uncertainty_without_measurement() {
        let mut engine = EskfFusionEngine::new();
        let before = engine.localization_uncertainty_m();

        engine.predict_imu(Vector3::new(0.0, 0.0, 0.0), 0.05);
        let after = engine.localization_uncertainty_m();

        assert!(after >= before);
    }

    #[test]
    fn vision_update_reduces_position_uncertainty() {
        let mut engine = EskfFusionEngine::new();
        for _ in 0..10 {
            engine.predict_imu(Vector3::new(0.2, -0.1, 0.05), 0.02);
        }
        let before = engine.localization_uncertainty_m();

        let updated = engine.update_vision(Vector3::new(0.0, 0.0, 0.0));
        let after = engine.localization_uncertainty_m();

        assert!(updated);
        assert!(after <= before);
    }

    #[test]
    fn high_uncertainty_marks_degraded() {
        let snapshot = compute_sensor_health(SensorSample {
            imu_available: false,
            location_available: false,
            camera_available: false,
            depth_available: false,
            location_accuracy_m: Some(60.0),
        });

        assert!(snapshot.degraded);
    }
}
