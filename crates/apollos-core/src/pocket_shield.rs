#[derive(Debug, Clone)]
pub struct PocketShield {
    lux_threshold: f32,
    blocked: bool,
}

impl Default for PocketShield {
    fn default() -> Self {
        Self {
            lux_threshold: 5.0,
            blocked: false,
        }
    }
}

impl PocketShield {
    pub fn update_lux(&mut self, lux: f32) {
        self.blocked = lux <= self.lux_threshold;
    }

    pub fn is_touch_blocked(&self) -> bool {
        self.blocked
    }
}
