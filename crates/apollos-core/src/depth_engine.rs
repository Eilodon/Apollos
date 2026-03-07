use apollos_proto::contracts::{CarryMode, DistanceCategory};

const MAX_TRACKED_DEPTH_M: f32 = 5.0;
const MIN_HAZARD_CONFIDENCE: f32 = 0.5;
const MIN_TRACK_DT_S: f32 = 0.04;
const MAX_TRACK_DT_S: f32 = 1.5;
const MAX_TRACK_CENTER_DRIFT: f32 = 0.22;
const MAX_RELATIVE_SPEED_MPS: f32 = 12.0;
const MIN_TTC_CLOSING_SPEED_MPS: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq)]
struct TrackedHazardSample {
    label_id: u32,
    center_x: f32,
    depth_m: f32,
    relative_velocity_mps: f32,
    timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub label_id: u32,
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthSpatials {
    pub median_depth_m: f32,
    pub min_depth_m: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectSensorFusionInput {
    pub bbox: BoundingBox,
    pub spatial: DepthSpatials,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DropAheadHazard {
    pub hazard_type: String,
    pub distance: DistanceCategory,
    pub position_x: f32,
    pub confidence: f32,
    pub distance_m: f32,
    pub relative_velocity_mps: f32,
    pub time_to_collision_s: Option<f32>,
}

#[derive(Debug, Default)]
pub struct DepthEngine {
    last_process_time_ms: u64,
    last_hazard_sample: Option<TrackedHazardSample>,
}

impl DepthEngine {
    pub fn process(
        &mut self,
        objects: &[ObjectSensorFusionInput],
        _risk_score: f32,
        _carry_mode: CarryMode,
        _gyro_magnitude: f32,
        now_ms: u64,
    ) -> Option<DropAheadHazard> {
        let interval_ms = 50;
        if now_ms.saturating_sub(self.last_process_time_ms) < interval_ms {
            return None;
        }
        self.last_process_time_ms = now_ms;

        let mut most_critical_hazard = None;
        let mut min_depth = f32::MAX;
        let mut tracked_sample = None;

        for obj in objects {
            let cx = (obj.bbox.x_min + obj.bbox.x_max) / 2.0;
            let depth = obj.spatial.min_depth_m;
            let conf = obj.bbox.confidence;

            // Keep a wider pre-safety envelope here and let the policy layer decide urgency.
            if depth < MAX_TRACKED_DEPTH_M && depth < min_depth && conf > MIN_HAZARD_CONFIDENCE {
                min_depth = depth;
                let relative_velocity_mps =
                    self.estimate_relative_velocity(obj.bbox.label_id, cx, depth, now_ms);
                let time_to_collision_s = if relative_velocity_mps < -MIN_TTC_CLOSING_SPEED_MPS {
                    Some((depth / (-relative_velocity_mps)).max(0.0))
                } else {
                    None
                };
                most_critical_hazard = Some(DropAheadHazard {
                    hazard_type: format!("HAZARD_{}", obj.bbox.label_id),
                    distance: if depth < 0.5 {
                        DistanceCategory::VeryClose
                    } else if depth < 1.0 {
                        DistanceCategory::Mid
                    } else {
                        DistanceCategory::Far
                    },
                    position_x: cx * 2.0 - 1.0, // map [0..1] to [-1..1]
                    confidence: conf,
                    distance_m: depth,
                    relative_velocity_mps,
                    time_to_collision_s,
                });
                tracked_sample = Some(TrackedHazardSample {
                    label_id: obj.bbox.label_id,
                    center_x: cx,
                    depth_m: depth,
                    relative_velocity_mps,
                    timestamp_ms: now_ms,
                });
            }
        }

        self.last_hazard_sample = tracked_sample;
        most_critical_hazard
    }

    fn estimate_relative_velocity(
        &self,
        label_id: u32,
        center_x: f32,
        depth_m: f32,
        now_ms: u64,
    ) -> f32 {
        let Some(previous) = self.last_hazard_sample else {
            return 0.0;
        };
        let dt_s = (now_ms.saturating_sub(previous.timestamp_ms) as f32) / 1000.0;
        if !(MIN_TRACK_DT_S..=MAX_TRACK_DT_S).contains(&dt_s) {
            return 0.0;
        }

        let same_track = previous.label_id == label_id
            && (previous.center_x - center_x).abs() <= MAX_TRACK_CENTER_DRIFT;
        if !same_track {
            return 0.0;
        }

        let instantaneous = ((depth_m - previous.depth_m) / dt_s)
            .clamp(-MAX_RELATIVE_SPEED_MPS, MAX_RELATIVE_SPEED_MPS);
        (previous.relative_velocity_mps * 0.45 + instantaneous * 0.55)
            .clamp(-MAX_RELATIVE_SPEED_MPS, MAX_RELATIVE_SPEED_MPS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(depth_m: f32, confidence: f32, x_min: f32, x_max: f32) -> ObjectSensorFusionInput {
        ObjectSensorFusionInput {
            bbox: BoundingBox {
                label_id: 7,
                x_min,
                y_min: 0.1,
                x_max,
                y_max: 0.8,
                confidence,
            },
            spatial: DepthSpatials {
                median_depth_m: depth_m,
                min_depth_m: depth_m,
            },
        }
    }

    #[test]
    fn tracks_high_confidence_hazard_out_to_five_meters() {
        let mut engine = DepthEngine::default();
        let hazard = engine.process(
            &[object(4.2, 0.9, 0.2, 0.4)],
            0.0,
            CarryMode::Necklace,
            0.0,
            100,
        );

        let hazard = hazard.expect("expected hazard within wider pre-safety envelope");
        assert_eq!(hazard.distance, DistanceCategory::Far);
        assert!(hazard.confidence > 0.5);
        assert_eq!(hazard.distance_m, 4.2);
    }

    #[test]
    fn ignores_low_confidence_or_out_of_range_objects() {
        let mut engine = DepthEngine::default();
        let hazard = engine.process(
            &[
                object(4.8, 0.3, 0.2, 0.4),
                object(5.4, 0.9, 0.5, 0.7),
            ],
            0.0,
            CarryMode::Necklace,
            0.0,
            100,
        );

        assert!(hazard.is_none());
    }

    #[test]
    fn estimates_closing_speed_and_ttc_from_edge_observation_history() {
        let mut engine = DepthEngine::default();
        let first = engine.process(
            &[object(3.6, 0.92, 0.2, 0.38)],
            0.0,
            CarryMode::Necklace,
            0.0,
            100,
        );
        let second = engine.process(
            &[object(2.7, 0.92, 0.21, 0.39)],
            0.0,
            CarryMode::Necklace,
            0.0,
            200,
        );

        assert_eq!(
            first
                .expect("first observation should still produce a hazard")
                .relative_velocity_mps,
            0.0
        );

        let second = second.expect("second observation should produce tracked hazard");
        assert!(second.relative_velocity_mps < -1.0);
        assert!(second.time_to_collision_s.expect("ttc should exist") < 1.0);
    }
}
