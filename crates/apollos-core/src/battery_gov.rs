use apollos_proto::contracts::MotionState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DutyCycle {
    pub camera_interval_ms: u64,
    pub depth_interval_ms: u64,
}

pub fn compute_duty_cycle(battery_percent: u8, motion_state: MotionState) -> DutyCycle {
    let low_power = battery_percent <= 20;

    if low_power {
        return DutyCycle {
            camera_interval_ms: 100,
            depth_interval_ms: 200,
        };
    }

    match motion_state {
        MotionState::Running => DutyCycle {
            camera_interval_ms: 33,
            depth_interval_ms: 66,
        },
        MotionState::WalkingFast => DutyCycle {
            camera_interval_ms: 50,
            depth_interval_ms: 80,
        },
        MotionState::WalkingSlow => DutyCycle {
            camera_interval_ms: 90,
            depth_interval_ms: 110,
        },
        MotionState::Stationary => DutyCycle {
            camera_interval_ms: 150,
            depth_interval_ms: 120,
        },
    }
}
