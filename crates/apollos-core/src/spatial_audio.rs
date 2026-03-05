#[derive(Debug, Clone, Copy)]
pub struct SpatialCue {
    pub azimuth_deg: f32,
    pub distance_m: f32,
}

#[derive(Debug, Default)]
pub struct SpatialAudioEngine {
    initialized: bool,
}

impl SpatialAudioEngine {
    pub fn initialize(&mut self) {
        self.initialized = true;
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn queue_ping(&self, _cue: SpatialCue) -> bool {
        self.initialized
    }
}
