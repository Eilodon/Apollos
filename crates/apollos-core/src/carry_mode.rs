use apollos_proto::contracts::CarryMode;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarryModeProfile {
    pub cos_tilt_threshold: f32,
    pub pitch_offset: f32,
    pub gyro_threshold: f32,
    pub cloud_enabled: bool,
}

pub const DEFAULT_CARRY_MODE: CarryMode = CarryMode::Necklace;

pub fn get_carry_mode_profile(mode: CarryMode) -> CarryModeProfile {
    match mode {
        CarryMode::HandHeld => CarryModeProfile {
            cos_tilt_threshold: 0.82,
            pitch_offset: 0.0,
            gyro_threshold: 45.0,
            cloud_enabled: true,
        },
        CarryMode::Necklace => CarryModeProfile {
            cos_tilt_threshold: 0.65,
            pitch_offset: 15.0,
            gyro_threshold: 55.0,
            cloud_enabled: true,
        },
        CarryMode::ChestClip => CarryModeProfile {
            cos_tilt_threshold: 0.72,
            pitch_offset: 8.0,
            gyro_threshold: 50.0,
            cloud_enabled: true,
        },
        CarryMode::Pocket => CarryModeProfile {
            cos_tilt_threshold: 0.0,
            pitch_offset: 0.0,
            gyro_threshold: 999.0,
            cloud_enabled: false,
        },
    }
}

pub fn parse_carry_mode(value: &str) -> Option<CarryMode> {
    match value {
        "hand_held" => Some(CarryMode::HandHeld),
        "necklace" => Some(CarryMode::Necklace),
        "chest_clip" => Some(CarryMode::ChestClip),
        "pocket" => Some(CarryMode::Pocket),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pocket_mode_disables_cloud_frames() {
        let profile = get_carry_mode_profile(CarryMode::Pocket);
        assert!(!profile.cloud_enabled);
    }

    #[test]
    fn parser_matches_contract_values() {
        assert_eq!(parse_carry_mode("necklace"), Some(CarryMode::Necklace));
        assert_eq!(parse_carry_mode("invalid"), None);
    }
}
