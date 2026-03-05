use std::collections::VecDeque;

use apollos_proto::contracts::DistanceCategory;

const RING_SIZE: usize = 3;
const BASELINE_THRESHOLD: f32 = 50.0;
const SUSTAINED_THRESHOLD: f32 = 40.0;
const FLOOR_DROP_BOTTOM_THRESHOLD: f32 = 60.0;
const FLOOR_DROP_TOP_MAX: f32 = 20.0;
const FLICKER_SUPPRESS_LUMA_DELTA: f32 = 28.0;
const HAZARD_DEDUP_COOLDOWN_MS: u64 = 3000;

#[derive(Debug, Clone, PartialEq)]
pub struct LumaFrame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<f32>,
}

impl LumaFrame {
    pub fn from_rgba(width: usize, height: usize, rgba: &[u8]) -> Option<Self> {
        if rgba.len() != width * height * 4 {
            return None;
        }

        let mut pixels = vec![0.0_f32; width * height];
        for (idx, chunk) in rgba.chunks_exact(4).enumerate() {
            let r = chunk[0] as f32;
            let g = chunk[1] as f32;
            let b = chunk[2] as f32;
            pixels[idx] = 0.299 * r + 0.587 * g + 0.114 * b;
        }

        Some(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn average_luma(&self) -> f32 {
        if self.pixels.is_empty() {
            return 0.0;
        }

        self.pixels.iter().sum::<f32>() / self.pixels.len() as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpansionPattern {
    Radial,
    Uniform,
    Directional,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpticalExpansion {
    pub center_diff: f32,
    pub avg_diff: f32,
    pub pattern: ExpansionPattern,
    pub lateral_bias: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReflexHazard {
    pub position_x: f32,
    pub hazard_type: String,
    pub subtype: Option<String>,
    pub distance: DistanceCategory,
    pub confidence: f32,
    pub center_diff: f32,
    pub avg_diff: f32,
    pub pattern: ExpansionPattern,
}

#[derive(Debug, Clone)]
struct HazardRecord {
    id: String,
    last_emitted_at_ms: u64,
}

#[derive(Debug, Default)]
pub struct SurvivalReflexEngine {
    diff_history: VecDeque<f32>,
    previous_frame: Option<LumaFrame>,
    recent_hazards: VecDeque<HazardRecord>,
    last_process_time_ms: u64,
}

impl SurvivalReflexEngine {
    pub fn process(
        &mut self,
        current_frame: LumaFrame,
        risk_score: f32,
        now_ms: u64,
    ) -> Option<ReflexHazard> {
        let dynamic_interval_ms = (150.0 - risk_score * 33.0).max(16.0) as u64;
        if now_ms.saturating_sub(self.last_process_time_ms) < dynamic_interval_ms {
            return None;
        }
        self.last_process_time_ms = now_ms;

        let Some(previous) = self.previous_frame.clone() else {
            self.previous_frame = Some(current_frame);
            return None;
        };

        if previous.width != current_frame.width || previous.height != current_frame.height {
            self.previous_frame = Some(current_frame);
            return None;
        }

        let expansion = compute_optical_expansion(&previous, &current_frame);

        if let Some(floor_drop) = detect_floor_drop(&previous, &current_frame) {
            let hazard_id =
                compute_hazard_id(ExpansionPattern::Directional, 0.0, Some("floor_drop"));
            if self.is_duplicate(&hazard_id, now_ms) {
                self.previous_frame = Some(current_frame);
                return None;
            }

            self.record_hazard(hazard_id, now_ms);
            self.previous_frame = Some(current_frame);
            return Some(floor_drop);
        }

        self.push_diff(expansion.avg_diff);
        let immediate_threat = expansion.avg_diff > BASELINE_THRESHOLD
            && expansion.pattern == ExpansionPattern::Radial;
        let sustained_threat = self.is_sustained_threat(expansion.pattern);

        let hazard = if immediate_threat || sustained_threat {
            let hazard_id = compute_hazard_id(expansion.pattern, expansion.lateral_bias, None);
            if self.is_duplicate(&hazard_id, now_ms) {
                None
            } else {
                self.record_hazard(hazard_id, now_ms);
                Some(build_hazard_payload(expansion))
            }
        } else {
            None
        };

        self.previous_frame = Some(current_frame);
        hazard
    }

    fn push_diff(&mut self, diff: f32) {
        self.diff_history.push_back(diff);
        while self.diff_history.len() > RING_SIZE {
            self.diff_history.pop_front();
        }
    }

    fn is_sustained_threat(&self, pattern: ExpansionPattern) -> bool {
        if self.diff_history.len() < RING_SIZE {
            return false;
        }

        self.diff_history
            .iter()
            .all(|value| *value > SUSTAINED_THRESHOLD)
            && pattern == ExpansionPattern::Radial
    }

    fn is_duplicate(&self, hazard_id: &str, now_ms: u64) -> bool {
        self.recent_hazards.iter().any(|entry| {
            entry.id == hazard_id
                && now_ms.saturating_sub(entry.last_emitted_at_ms) < HAZARD_DEDUP_COOLDOWN_MS
        })
    }

    fn record_hazard(&mut self, hazard_id: String, now_ms: u64) {
        if let Some(existing) = self
            .recent_hazards
            .iter_mut()
            .find(|entry| entry.id == hazard_id)
        {
            existing.last_emitted_at_ms = now_ms;
            return;
        }

        self.recent_hazards.push_back(HazardRecord {
            id: hazard_id,
            last_emitted_at_ms: now_ms,
        });

        while self.recent_hazards.len() > 10 {
            self.recent_hazards.pop_front();
        }
    }
}

pub fn compute_optical_expansion(previous: &LumaFrame, current: &LumaFrame) -> OpticalExpansion {
    let width = current.width;
    let height = current.height;

    let half_w = width / 2;
    let half_h = height / 2;

    let q1 = abs_diff_average(previous, current, 0, 0, half_w, half_h);
    let q2 = abs_diff_average(previous, current, half_w, 0, width, half_h);
    let q3 = abs_diff_average(previous, current, 0, half_h, half_w, height);
    let q4 = abs_diff_average(previous, current, half_w, half_h, width, height);
    let quadrants = [q1, q2, q3, q4];

    let center_start_x = ((width as f32) * 0.25) as usize;
    let center_end_x = ((width as f32) * 0.75) as usize;
    let center_start_y = ((height as f32) * 0.25) as usize;
    let center_end_y = ((height as f32) * 0.75) as usize;
    let center_diff = abs_diff_average(
        previous,
        current,
        center_start_x,
        center_start_y,
        center_end_x,
        center_end_y,
    );

    let avg_diff = (q1 + q2 + q3 + q4) / 4.0;
    let pattern = classify_pattern(&quadrants);
    let left_energy = q1 + q3;
    let right_energy = q2 + q4;
    let denominator = (left_energy + right_energy).max(1e-3);
    let lateral_bias = ((right_energy - left_energy) / denominator).clamp(-1.0, 1.0);

    OpticalExpansion {
        center_diff,
        avg_diff,
        pattern,
        lateral_bias,
    }
}

pub fn detect_floor_drop(previous: &LumaFrame, current: &LumaFrame) -> Option<ReflexHazard> {
    let width = current.width;
    let height = current.height;

    let top_diff = abs_diff_average(
        previous,
        current,
        0,
        0,
        width,
        (height as f32 * 0.25) as usize,
    );
    let bottom_diff = abs_diff_average(
        previous,
        current,
        0,
        (height as f32 * 0.75) as usize,
        width,
        height,
    );
    let luma_delta = (current.average_luma() - previous.average_luma()).abs();

    if luma_delta > FLICKER_SUPPRESS_LUMA_DELTA && top_diff > 18.0 {
        return None;
    }

    let is_drop = bottom_diff >= FLOOR_DROP_BOTTOM_THRESHOLD
        && top_diff <= FLOOR_DROP_TOP_MAX
        && bottom_diff > top_diff * 2.4;

    if !is_drop {
        return None;
    }

    Some(ReflexHazard {
        position_x: 0.0,
        hazard_type: "EDGE_DROP_HAZARD".to_string(),
        subtype: Some("floor_drop".to_string()),
        distance: DistanceCategory::VeryClose,
        confidence: 0.95,
        center_diff: bottom_diff,
        avg_diff: (top_diff + bottom_diff) / 2.0,
        pattern: ExpansionPattern::Directional,
    })
}

fn classify_pattern(quadrants: &[f32; 4]) -> ExpansionPattern {
    let min_q = quadrants.iter().copied().fold(f32::INFINITY, f32::min);
    let max_q = quadrants.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let spread = max_q - min_q;
    let avg = quadrants.iter().sum::<f32>() / quadrants.len() as f32;

    if avg < 1.0 {
        return ExpansionPattern::None;
    }

    let active_count = quadrants
        .iter()
        .filter(|value| **value > SUSTAINED_THRESHOLD)
        .count();

    if active_count >= 3 && spread <= 14.0 {
        return ExpansionPattern::Radial;
    }

    if spread <= 8.0 && avg > 20.0 {
        return ExpansionPattern::Uniform;
    }

    if active_count >= 2 && spread > 14.0 {
        return ExpansionPattern::Directional;
    }

    ExpansionPattern::None
}

fn abs_diff_average(
    previous: &LumaFrame,
    current: &LumaFrame,
    x_start: usize,
    y_start: usize,
    x_end: usize,
    y_end: usize,
) -> f32 {
    let width = current.width;
    let mut total = 0.0_f32;
    let mut count = 0_u64;

    for y in y_start..y_end {
        let row = y * width;
        for x in x_start..x_end {
            let idx = row + x;
            let previous_px = previous.pixels[idx];
            let current_px = current.pixels[idx];
            total += (current_px - previous_px).abs();
            count += 1;
        }
    }

    if count == 0 {
        return 0.0;
    }

    total / count as f32
}

fn compute_hazard_id(
    pattern: ExpansionPattern,
    lateral_bias: f32,
    subtype: Option<&str>,
) -> String {
    let lateral_bucket = (lateral_bias * 5.0).round() as i32;
    let head = subtype.unwrap_or(match pattern {
        ExpansionPattern::Radial => "radial",
        ExpansionPattern::Uniform => "uniform",
        ExpansionPattern::Directional => "directional",
        ExpansionPattern::None => "none",
    });
    format!("{head}:{lateral_bucket}")
}

fn build_hazard_payload(expansion: OpticalExpansion) -> ReflexHazard {
    ReflexHazard {
        position_x: expansion.lateral_bias,
        hazard_type: "EDGE_APPROACHING_OBJECT".to_string(),
        subtype: None,
        distance: DistanceCategory::VeryClose,
        confidence: 0.9,
        center_diff: expansion.center_diff,
        avg_diff: expansion.avg_diff,
        pattern: expansion.pattern,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_with_value(width: usize, height: usize, value: f32) -> LumaFrame {
        LumaFrame {
            width,
            height,
            pixels: vec![value; width * height],
        }
    }

    #[test]
    fn computes_no_expansion_for_identical_frames() {
        let previous = frame_with_value(8, 8, 42.0);
        let current = frame_with_value(8, 8, 42.0);

        let expansion = compute_optical_expansion(&previous, &current);
        assert_eq!(expansion.pattern, ExpansionPattern::None);
        assert_eq!(expansion.avg_diff, 0.0);
    }

    #[test]
    fn sustained_high_diff_emits_hazard_once() {
        let mut engine = SurvivalReflexEngine::default();
        let previous = frame_with_value(8, 8, 5.0);
        let current = frame_with_value(8, 8, 95.0);

        let _ = engine.process(previous.clone(), 4.0, 20);
        let first_hazard = engine.process(current.clone(), 4.0, 40);
        assert!(first_hazard.is_some());

        let _ = engine.process(previous, 4.0, 60);
        let duplicated = engine.process(current, 4.0, 80);
        assert!(duplicated.is_none());
    }
}
