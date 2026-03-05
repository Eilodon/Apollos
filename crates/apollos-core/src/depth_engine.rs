use apollos_proto::contracts::{CarryMode, DistanceCategory};
use thiserror::Error;

use crate::optical_flow::LumaFrame;

const BASE_INTERVAL_MS: u64 = 100;
#[cfg(feature = "ml")]
const DEFAULT_ONNX_INPUT_SIZE: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthSource {
    Onnx,
    Heuristic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DropAheadHazard {
    pub hazard_type: String,
    pub distance: DistanceCategory,
    pub position_x: f32,
    pub confidence: f32,
    pub source: DepthSource,
}

#[derive(Debug, Error)]
pub enum DepthEngineError {
    #[error("onnx model path does not exist: {0}")]
    OnnxModelMissing(String),
    #[error("onnx model read failed: {0}")]
    OnnxModelRead(String),
    #[error("onnx depth output is empty")]
    EmptyOnnxOutput,
    #[error("depth engine conversion failed")]
    InvalidDepthMap,
    #[cfg(feature = "ml")]
    #[error("onnx runtime error: {0}")]
    Ort(#[from] ort::Error),
}

#[derive(Debug)]
pub struct DepthEngine {
    model_available: bool,
    last_process_time_ms: u64,
    base_interval_ms: u64,
    #[cfg(feature = "ml")]
    onnx_runtime: Option<OnnxRuntime>,
}

impl Default for DepthEngine {
    fn default() -> Self {
        Self {
            model_available: false,
            last_process_time_ms: 0,
            base_interval_ms: BASE_INTERVAL_MS,
            #[cfg(feature = "ml")]
            onnx_runtime: None,
        }
    }
}

impl DepthEngine {
    pub fn set_model_available(&mut self, available: bool) {
        self.model_available = available;
    }

    pub fn with_base_interval(mut self, base_interval_ms: u64) -> Self {
        self.base_interval_ms = base_interval_ms.max(30);
        self
    }

    pub fn process(
        &mut self,
        frame: &LumaFrame,
        risk_score: f32,
        carry_mode: CarryMode,
        gyro_magnitude: f32,
        now_ms: u64,
    ) -> Option<DropAheadHazard> {
        let dynamic_interval_ms = ((self.base_interval_ms as f32) - (risk_score - 1.0) * 15.0)
            .max(50.0)
            .round() as u64;

        if now_ms.saturating_sub(self.last_process_time_ms) < dynamic_interval_ms {
            return None;
        }

        self.last_process_time_ms = now_ms;

        #[cfg(feature = "ml")]
        let mut source = DepthSource::Heuristic;
        #[cfg(not(feature = "ml"))]
        let source = DepthSource::Heuristic;
        let depth_map = {
            #[cfg(feature = "ml")]
            {
                if let Some(depth_map) = self
                    .infer_depth_map_onnx(frame)
                    .inspect(|_| source = DepthSource::Onnx)
                {
                    depth_map
                } else {
                    infer_heuristic_depth(frame)
                }
            }
            #[cfg(not(feature = "ml"))]
            {
                infer_heuristic_depth(frame)
            }
        };

        let mut hazard = detect_drop_ahead(
            &depth_map,
            frame.width,
            frame.height,
            carry_mode,
            gyro_magnitude,
            self.model_available,
        )?;

        hazard.source = source;
        Some(hazard)
    }

    #[cfg(feature = "ml")]
    pub fn has_onnx_runtime(&self) -> bool {
        self.onnx_runtime.is_some()
    }

    #[cfg(not(feature = "ml"))]
    pub fn has_onnx_runtime(&self) -> bool {
        false
    }

    #[cfg(feature = "ml")]
    pub fn load_onnx_model<P: AsRef<std::path::Path>>(
        &mut self,
        path: P,
    ) -> Result<(), DepthEngineError> {
        use std::sync::{Arc, Mutex};

        let path = path.as_ref();
        if !path.exists() {
            return Err(DepthEngineError::OnnxModelMissing(
                path.display().to_string(),
            ));
        }

        let model_bytes =
            std::fs::read(path).map_err(|err| DepthEngineError::OnnxModelRead(err.to_string()))?;
        let mut session = ort::session::Session::builder()?.commit_from_memory(&model_bytes)?;
        // Warm-up run to fail fast if model input contract is incompatible.
        let warmup = vec![0_f32; 3 * DEFAULT_ONNX_INPUT_SIZE * DEFAULT_ONNX_INPUT_SIZE];
        let input = ort::value::Tensor::<f32>::from_array((
            [1_usize, 3, DEFAULT_ONNX_INPUT_SIZE, DEFAULT_ONNX_INPUT_SIZE],
            warmup.into_boxed_slice(),
        ))?;
        let _ = session.run(ort::inputs![input])?;

        self.onnx_runtime = Some(OnnxRuntime {
            session: Arc::new(Mutex::new(session)),
            input_size: DEFAULT_ONNX_INPUT_SIZE,
        });
        self.model_available = true;
        Ok(())
    }

    #[cfg(not(feature = "ml"))]
    pub fn load_onnx_model<P: AsRef<std::path::Path>>(
        &mut self,
        _path: P,
    ) -> Result<(), DepthEngineError> {
        Err(DepthEngineError::InvalidDepthMap)
    }

    pub fn try_enable_onnx_from_env(&mut self) -> Result<bool, DepthEngineError> {
        let Ok(model_path) = std::env::var("APOLLOS_DEPTH_ONNX_MODEL") else {
            return Ok(false);
        };

        if model_path.trim().is_empty() {
            return Ok(false);
        }

        #[cfg(feature = "ml")]
        {
            self.load_onnx_model(model_path)?;
            Ok(true)
        }

        #[cfg(not(feature = "ml"))]
        {
            let _ = model_path;
            Ok(false)
        }
    }

    #[cfg(feature = "ml")]
    fn infer_depth_map_onnx(&self, frame: &LumaFrame) -> Option<Vec<f32>> {
        let runtime = self.onnx_runtime.as_ref()?;
        let mut session = runtime.session.lock().ok()?;

        let input_data = build_onnx_input(frame, runtime.input_size);
        let input = ort::value::Tensor::<f32>::from_array((
            [1_usize, 3, runtime.input_size, runtime.input_size],
            input_data.into_boxed_slice(),
        ))
        .ok()?;

        let outputs = session.run(ort::inputs![input]).ok()?;
        if outputs.len() == 0 {
            return None;
        }

        let output_value = &outputs[0];
        let (shape, tensor_values) = output_value.try_extract_tensor::<f32>().ok()?;
        if tensor_values.is_empty() {
            return None;
        }

        let (out_width, out_height) = onnx_output_dims(shape, tensor_values.len())?;
        let output_len = out_width.checked_mul(out_height)?;
        if tensor_values.len() < output_len {
            return None;
        }

        let normalized = normalize_depth_values(&tensor_values[..output_len]);
        let resized = resize_depth_map(
            &normalized,
            out_width,
            out_height,
            frame.width,
            frame.height,
        )
        .ok()?;

        Some(resized)
    }
}

#[cfg(feature = "ml")]
#[derive(Debug, Clone)]
struct OnnxRuntime {
    session: std::sync::Arc<std::sync::Mutex<ort::session::Session>>,
    input_size: usize,
}

pub fn infer_heuristic_depth(frame: &LumaFrame) -> Vec<f32> {
    frame
        .pixels
        .iter()
        .map(|value| 1.0 - (value / 255.0))
        .collect()
}

#[cfg(feature = "ml")]
fn build_onnx_input(frame: &LumaFrame, target_size: usize) -> Vec<f32> {
    let mut out = vec![0.0_f32; 3 * target_size * target_size];

    for y in 0..target_size {
        let src_y = y.saturating_mul(frame.height) / target_size;
        for x in 0..target_size {
            let src_x = x.saturating_mul(frame.width) / target_size;
            let src_idx = src_y.saturating_mul(frame.width) + src_x;
            let luma = frame.pixels.get(src_idx).copied().unwrap_or(0.0) / 255.0;

            let base = y * target_size + x;
            out[base] = luma;
            out[target_size * target_size + base] = luma;
            out[2 * target_size * target_size + base] = luma;
        }
    }

    out
}

#[cfg(feature = "ml")]
fn normalize_depth_values(values: &[f32]) -> Vec<f32> {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;

    for value in values {
        min = min.min(*value);
        max = max.max(*value);
    }

    let span = (max - min).max(1e-6);
    values.iter().map(|value| (*value - min) / span).collect()
}

#[cfg(feature = "ml")]
fn onnx_output_dims(shape: &ort::value::Shape, values_len: usize) -> Option<(usize, usize)> {
    let positive_dims: Vec<usize> = shape
        .iter()
        .copied()
        .filter(|dim| *dim > 0)
        .map(|dim| dim as usize)
        .collect();

    if positive_dims.len() >= 2 {
        let out_height = positive_dims[positive_dims.len() - 2];
        let out_width = positive_dims[positive_dims.len() - 1];
        if out_width > 0 && out_height > 0 && out_width.saturating_mul(out_height) <= values_len {
            return Some((out_width, out_height));
        }
    }

    let side = (values_len as f64).sqrt() as usize;
    if side > 0 && side.saturating_mul(side) == values_len {
        Some((side, side))
    } else {
        None
    }
}

#[cfg(feature = "ml")]
fn resize_depth_map(
    input: &[f32],
    in_width: usize,
    in_height: usize,
    out_width: usize,
    out_height: usize,
) -> Result<Vec<f32>, DepthEngineError> {
    if input.len() != in_width.saturating_mul(in_height) || in_width == 0 || in_height == 0 {
        return Err(DepthEngineError::InvalidDepthMap);
    }

    let mut output = vec![0.0_f32; out_width.saturating_mul(out_height)];
    for y in 0..out_height {
        let src_y = y.saturating_mul(in_height) / out_height.max(1);
        for x in 0..out_width {
            let src_x = x.saturating_mul(in_width) / out_width.max(1);
            output[y * out_width + x] = input[src_y * in_width + src_x];
        }
    }

    Ok(output)
}

pub fn detect_drop_ahead(
    depth: &[f32],
    width: usize,
    height: usize,
    carry_mode: CarryMode,
    gyro_magnitude: f32,
    model_available: bool,
) -> Option<DropAheadHazard> {
    if depth.len() != width * height || width == 0 || height < 2 {
        return None;
    }

    let roi = match carry_mode {
        CarryMode::HandHeld => RoiProfile {
            y_start_ratio: 0.60,
            min_confidence: 0.55,
            min_ratio: 0.10,
        },
        CarryMode::Necklace => RoiProfile {
            y_start_ratio: 0.50,
            min_confidence: 0.62,
            min_ratio: 0.13,
        },
        CarryMode::ChestClip => RoiProfile {
            y_start_ratio: 0.48,
            min_confidence: 0.60,
            min_ratio: 0.12,
        },
        CarryMode::Pocket => RoiProfile {
            y_start_ratio: 0.62,
            min_confidence: 0.74,
            min_ratio: 0.16,
        },
    };

    let x_start = (width as f32 * 0.15) as usize;
    let x_end = (width as f32 * 0.85) as usize;
    let y_start = (height as f32 * roi.y_start_ratio) as usize;
    let y_end = height.saturating_sub(1);

    let mut discontinuity_count: usize = 0;
    let mut confidence_sum = 0.0_f32;
    let mut weighted_x = 0.0_f32;
    let mut weighted_weight = 0.0_f32;

    for y in (y_start + 1)..y_end {
        let row = y * width;
        let previous_row = (y - 1) * width;
        for x in x_start..x_end {
            let current = depth[row + x];
            let previous = depth[previous_row + x];
            let delta = current - previous;
            if delta > 0.22 {
                discontinuity_count += 1;
                confidence_sum += delta;
                weighted_x += x as f32 * delta;
                weighted_weight += delta;
            }
        }
    }

    let sample_count = ((x_end.saturating_sub(x_start)) * (y_end.saturating_sub(y_start))).max(1);
    let discontinuity_ratio = discontinuity_count as f32 / sample_count as f32;
    let confidence = (discontinuity_ratio * 2.2 + confidence_sum / sample_count as f32).min(1.0);

    let pendulum_penalty = if matches!(carry_mode, CarryMode::Necklace | CarryMode::ChestClip) {
        ((gyro_magnitude - 110.0).max(0.0) / 1200.0).min(0.10)
    } else {
        0.0
    };

    let stabilized_confidence = (confidence - pendulum_penalty).max(0.0);
    if stabilized_confidence < roi.min_confidence || discontinuity_ratio < roi.min_ratio {
        return None;
    }

    let avg_x = if weighted_weight > 0.0 {
        weighted_x / weighted_weight
    } else {
        width as f32 / 2.0
    };

    let position_x = ((avg_x / width as f32) * 2.0 - 1.0).clamp(-1.0, 1.0);

    Some(DropAheadHazard {
        hazard_type: "DROP_AHEAD".to_string(),
        distance: DistanceCategory::VeryClose,
        position_x,
        confidence: stabilized_confidence,
        source: if model_available {
            DepthSource::Onnx
        } else {
            DepthSource::Heuristic
        },
    })
}

#[derive(Debug, Clone, Copy)]
struct RoiProfile {
    y_start_ratio: f32,
    min_confidence: f32,
    min_ratio: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_drop_map(width: usize, height: usize) -> Vec<f32> {
        let mut depth = vec![0.1_f32; width * height];
        for y in (height / 2)..height {
            for x in 0..width {
                depth[y * width + x] = if y % 2 == 0 { 0.1 } else { 0.95 };
            }
        }
        depth
    }

    #[test]
    fn detects_drop_with_heuristic_map() {
        let map = synthetic_drop_map(64, 64);
        let hazard = detect_drop_ahead(&map, 64, 64, CarryMode::Necklace, 0.0, false);
        assert!(hazard.is_some());
        let hazard = hazard.expect("hazard should exist");
        assert_eq!(hazard.hazard_type, "DROP_AHEAD");
        assert_eq!(hazard.source, DepthSource::Heuristic);
    }

    #[test]
    fn process_respects_dynamic_interval() {
        let frame = LumaFrame {
            width: 8,
            height: 8,
            pixels: vec![32.0; 64],
        };

        let mut engine = DepthEngine::default();
        let first = engine.process(&frame, 1.0, CarryMode::Necklace, 0.0, 10);
        let second = engine.process(&frame, 1.0, CarryMode::Necklace, 0.0, 20);
        assert!(first.is_none());
        assert!(second.is_none());
    }

    #[test]
    fn env_without_model_keeps_runtime_disabled() {
        let mut engine = DepthEngine::default();
        std::env::remove_var("APOLLOS_DEPTH_ONNX_MODEL");
        let enabled = engine
            .try_enable_onnx_from_env()
            .expect("env probing should not fail");
        assert!(!enabled);
        assert!(!engine.has_onnx_runtime());
    }
}
