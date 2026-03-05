#[derive(Debug, Clone, PartialEq)]
pub struct DeterministicScanResult {
    pub value: String,
    pub format: String,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct DeterministicScanTracker {
    min_emit_interval_ms: u64,
    last_emit_at_ms: u64,
    candidate_value: String,
    candidate_stable_count: u32,
}

impl Default for DeterministicScanTracker {
    fn default() -> Self {
        Self {
            min_emit_interval_ms: 2500,
            last_emit_at_ms: 0,
            candidate_value: String::new(),
            candidate_stable_count: 0,
        }
    }
}

impl DeterministicScanTracker {
    pub fn observe(
        &mut self,
        raw_value: Option<&str>,
        format: Option<&str>,
        now_ms: u64,
    ) -> Option<DeterministicScanResult> {
        let Some(raw_value) = raw_value else {
            self.candidate_value.clear();
            self.candidate_stable_count = 0;
            return None;
        };

        let value = raw_value.trim();
        if value.is_empty() {
            self.candidate_value.clear();
            self.candidate_stable_count = 0;
            return None;
        }

        if self.candidate_value == value {
            self.candidate_stable_count += 1;
        } else {
            self.candidate_value = value.to_string();
            self.candidate_stable_count = 1;
        }

        if self.candidate_stable_count < 2
            || now_ms.saturating_sub(self.last_emit_at_ms) < self.min_emit_interval_ms
        {
            return None;
        }

        self.last_emit_at_ms = now_ms;

        let confidence = (0.72 + self.candidate_stable_count as f32 * 0.09).min(0.99);
        Some(DeterministicScanResult {
            value: self.candidate_value.clone(),
            format: format.unwrap_or("").trim().to_ascii_uppercase(),
            confidence,
        })
    }
}
