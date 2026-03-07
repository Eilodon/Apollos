use apollos_core::{
    compute_risk_score, get_carry_mode_profile, should_capture_frame, Acceleration, GyroRotation,
    KinematicReading,
};
use apollos_proto::contracts::{CarryMode, MotionState};

fn main() {
    let profile = get_carry_mode_profile(CarryMode::Necklace);
    let reading = KinematicReading {
        accel: Some(Acceleration {
            x: 0.1,
            y: 9.7,
            z: 0.5,
        }),
        gyro: Some(GyroRotation {
            alpha: 2.0,
            beta: 1.0,
            gamma: 1.0,
        }),
    };

    // Original benchmark
    let risk_normal = compute_risk_score(MotionState::WalkingFast, 10.0, 1.8, 8.0);
    let capture_normal = should_capture_frame(reading.clone(), profile);

    println!("apollos-bench (normal): risk={risk_normal:.2}, capture={capture_normal:?}");

    // Extreme benchmark: Văng ngang (lateral acceleration spike) & Vận tốc cao
    let extreme_reading = KinematicReading {
        accel: Some(Acceleration {
            x: 10.0, // Văng ngang cực lớn
            y: 0.0,
            z: 0.5,
        }),
        gyro: Some(GyroRotation {
            alpha: 5.0,
            beta: 1.0,
            gamma: 1.0,
        }),
    };
    
    // velocity 5.0 (sprint/ngã)
    let risk_extreme = compute_risk_score(MotionState::Running, 15.0, 5.0, 15.0);
    let capture_extreme = should_capture_frame(extreme_reading, profile);

    println!("apollos-bench (extreme): risk={risk_extreme:.2}, capture={capture_extreme:?}");
}
