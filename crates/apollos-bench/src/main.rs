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

    let risk = compute_risk_score(MotionState::WalkingFast, 10.0, 1.8, 8.0);
    let capture = should_capture_frame(reading, profile);

    println!("apollos-bench: risk={risk:.2}, capture={capture}");
}
