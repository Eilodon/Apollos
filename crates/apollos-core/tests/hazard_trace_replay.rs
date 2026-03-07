use apollos_core::depth_engine::{BoundingBox, DepthSpatials, ObjectSensorFusionInput};
use apollos_core::DepthEngine;
use apollos_proto::contracts::CarryMode;

fn object(
    label_id: u32,
    x_min: f32,
    x_max: f32,
    confidence: f32,
    min_depth_m: f32,
) -> ObjectSensorFusionInput {
    ObjectSensorFusionInput {
        bbox: BoundingBox {
            label_id,
            x_min,
            y_min: 0.2,
            x_max,
            y_max: 0.8,
            confidence,
        },
        spatial: DepthSpatials {
            median_depth_m: min_depth_m + 0.3,
            min_depth_m,
        },
    }
}

#[test]
fn replay_object_fusion_stream_detects_nearest_hazard() {
    let mut engine = DepthEngine::default();

    let warmup = [object(1, 0.4, 0.6, 0.8, 3.2)];
    let none = engine.process(&warmup, 1.2, CarryMode::Necklace, 0.0, 10);
    assert!(none.is_none());

    let mixed = [
        object(7, 0.1, 0.2, 0.55, 1.8),
        object(42, 0.48, 0.58, 0.92, 0.35),
        object(9, 0.75, 0.88, 0.88, 0.9),
    ];
    let hazard = engine
        .process(&mixed, 1.8, CarryMode::Necklace, 0.2, 80)
        .expect("expected nearest valid hazard");

    assert_eq!(hazard.hazard_type, "HAZARD_42");
    assert!(hazard.position_x.abs() < 0.2);
    assert!(hazard.confidence > 0.9);
}

#[test]
fn replay_object_fusion_respects_rate_limit_window() {
    let mut engine = DepthEngine::default();
    let hazardous = [object(3, 0.2, 0.4, 0.9, 0.6)];

    let first = engine.process(&hazardous, 1.6, CarryMode::Necklace, 0.0, 100);
    assert!(first.is_some());

    let suppressed = engine.process(&hazardous, 1.6, CarryMode::Necklace, 0.0, 120);
    assert!(suppressed.is_none());

    let resumed = engine.process(&hazardous, 1.6, CarryMode::Necklace, 0.0, 170);
    assert!(resumed.is_some());
}
