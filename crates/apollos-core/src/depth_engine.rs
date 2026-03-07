use apollos_proto::contracts::{CarryMode, DistanceCategory};

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
}

#[derive(Debug)]
pub struct DepthEngine {
    last_process_time_ms: u64,
}

impl Default for DepthEngine {
    fn default() -> Self {
        Self {
            last_process_time_ms: 0,
        }
    }
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

        for obj in objects {
            let cx = (obj.bbox.x_min + obj.bbox.x_max) / 2.0;
            let depth = obj.spatial.min_depth_m;
            let conf = obj.bbox.confidence;

            // Simplified: treat anything closer than 2.0m as a potential hazard.
            // Using a threshold to detect imminent hazards
            if depth < 2.0 && depth < min_depth && conf > 0.5 {
                min_depth = depth;
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
                });
            }
        }

        most_critical_hazard
    }
}
