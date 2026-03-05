use apollos_core::{DepthEngine, LumaFrame, SurvivalReflexEngine};
use apollos_proto::contracts::CarryMode;

fn uniform_frame(width: usize, height: usize, luma: f32) -> LumaFrame {
    LumaFrame {
        width,
        height,
        pixels: vec![luma; width * height],
    }
}

fn drop_like_frame(width: usize, height: usize) -> LumaFrame {
    let mut pixels = vec![220.0_f32; width * height];

    for y in (height / 2)..height {
        for x in 0..width {
            let luma = if y % 2 == 0 { 220.0 } else { 20.0 };
            pixels[y * width + x] = luma;
        }
    }

    LumaFrame {
        width,
        height,
        pixels,
    }
}

#[test]
fn replay_survival_reflex_stream_emits_and_dedups_hazard() {
    let mut engine = SurvivalReflexEngine::default();

    // warm-up frame
    let _ = engine.process(uniform_frame(16, 16, 10.0), 4.0, 20);

    let first = engine.process(uniform_frame(16, 16, 200.0), 4.0, 40);
    assert!(first.is_some());

    // within dedup cooldown window
    let _ = engine.process(uniform_frame(16, 16, 10.0), 4.0, 60);
    let duplicated = engine.process(uniform_frame(16, 16, 200.0), 4.0, 80);
    assert!(duplicated.is_none());

    // after cooldown should emit again
    let second = engine.process(uniform_frame(16, 16, 10.0), 4.0, 3100);
    if second.is_none() {
        let third = engine.process(uniform_frame(16, 16, 200.0), 4.0, 3200);
        assert!(third.is_some());
    }
}

#[test]
fn replay_depth_guard_stream_detects_drop_ahead() {
    let mut engine = DepthEngine::default();

    let _ = engine.process(
        &uniform_frame(64, 64, 220.0),
        4.0,
        CarryMode::Necklace,
        0.0,
        100,
    );

    let hazard = engine.process(&drop_like_frame(64, 64), 4.0, CarryMode::Necklace, 0.0, 220);
    assert!(hazard.is_some());

    let hazard = hazard.expect("hazard should be detected");
    assert_eq!(hazard.hazard_type, "DROP_AHEAD");
    assert!(hazard.confidence >= 0.62);
}
