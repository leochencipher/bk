/// ONNX Runtime inference for the Inflect v2 TTS model.
///
/// Loads `duration.onnx` and `decode.onnx` and runs the two-stage synthesis:
///   tokens → duration model → acoustic distribution → decode model → waveform

use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;

use super::normalize;
use super::phonemes;
use super::symbols;

/// ONNX inference engine for Inflect v2.
pub struct InflectOnnx {
    duration: Session,
    decode: Session,
}

impl InflectOnnx {
    pub fn new(model_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let onnx_dir = model_dir.join("onnx");
        let duration = {
            let mut builder = Session::builder()?;
            builder.commit_from_file(onnx_dir.join("duration.onnx"))?
        };
        let decode = {
            let mut builder = Session::builder()?;
            builder.commit_from_file(onnx_dir.join("decode.onnx"))?
        };
        Ok(Self { duration, decode })
    }

    /// Synthesize a single chunk of normalized text.
    ///
    /// Returns a mono f32 waveform at 24 kHz.
    pub fn synthesize_chunk(
        &mut self,
        text: &str,
        speed: f32,
        variation: f32,
        seed: i32,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        // 1. Phonemize
        let phoneme_text = phonemes::phonemize(text)?;

        // 2. Tokenize
        let tokens = symbols::phonemes_to_tokens(&phoneme_text);
        let num_tokens = tokens.len();
        let token_tensor = Tensor::from_array(([1i64, num_tokens as i64], tokens))?;

        // 3. Duration model
        let lengths = Tensor::from_array(([1i64], vec![num_tokens as i64]))?;
        let length_scale = Tensor::from_array(([1i64], vec![1.0f32 / speed]))?;

        let duration_outputs = self.duration.run(ort::inputs![
            "tokens" => token_tensor,
            "lengths" => lengths,
            "length_scale" => length_scale,
        ])?;

        // Extract duration outputs as f32 slices
        let m_p_exp_view = duration_outputs["m_p_exp"].try_extract_array::<f32>()?;
        let logs_p_exp_view = duration_outputs["logs_p_exp"].try_extract_array::<f32>()?;
        let y_mask_view = duration_outputs["y_mask"].try_extract_array::<f32>()?;

        let m_p_exp: Vec<f32> = m_p_exp_view.as_slice().unwrap().to_vec();
        let logs_p_exp: Vec<f32> = logs_p_exp_view.as_slice().unwrap().to_vec();
        let y_mask: Vec<f32> = y_mask_view.as_slice().unwrap().to_vec();
        let m_p_shape: Vec<i64> = m_p_exp_view.shape().iter().map(|&d| d as i64).collect();
        let y_mask_shape: Vec<i64> = y_mask_view.shape().iter().map(|&d| d as i64).collect();

        // 4. Generate latent noise (seeded)
        let noise: Vec<f32> = seeded_normal(seed, &m_p_exp);

        // 5. Decode model
        let m_p_tensor = Tensor::from_array((m_p_shape.clone(), m_p_exp))?;
        let logs_tensor = Tensor::from_array((m_p_shape.clone(), logs_p_exp))?;
        let mask_tensor = Tensor::from_array((y_mask_shape, y_mask))?;
        let noise_tensor = Tensor::from_array((m_p_shape.clone(), noise))?;
        let noise_scale = Tensor::from_array(([1i64], vec![variation]))?;

        let decode_outputs = self.decode.run(ort::inputs![
            "m_p_exp" => m_p_tensor,
            "logs_p_exp" => logs_tensor,
            "y_mask" => mask_tensor,
            "zp_noise" => noise_tensor,
            "noise_scale" => noise_scale,
        ])?;

        let waveform_view = decode_outputs["waveform"].try_extract_array::<f32>()?;
        let mut waveform: Vec<f32> = waveform_view.as_slice().unwrap().to_vec();

        // 6. Edge fade
        normalize::edge_fade(&mut waveform, 24_000, 5.0);

        Ok(waveform)
    }
}

/// Generate a Vec<f32> of normally-distributed random values using a seeded
/// linear congruential generator + Box-Muller transform.
///
/// This matches NumPy's `default_rng(seed).standard_normal(shape)` behavior
/// closely enough for TTS purposes (the exact sequence may differ from NumPy,
/// but the statistical properties are identical).
fn seeded_normal(seed: i32, template: &[f32]) -> Vec<f32> {
    let n = template.len();
    let mut state = seed as u64;

    let mut rng = move || {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (state as f64) / (u64::MAX as f64)
    };

    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        let u1 = rng();
        let u2 = rng();
        // Box-Muller
        let r = (-2.0 * u1.max(1e-10).ln()).sqrt();
        let theta = 2.0 * std::f64::consts::PI * u2;
        out.push((r * theta.cos()) as f32);
        if i + 1 < n {
            out.push((r * theta.sin()) as f32);
        }
        i += 2;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_ids_in_range() {
        // Test that phonemization produces tokens within the model's expected range
        let phoneme_text = phonemes::phonemize("hello world").expect("espeak-ng should work");
        println!("Phonemes: {}", phoneme_text);

        let tokens = symbols::phonemes_to_tokens(&phoneme_text);
        println!("Token count: {}", tokens.len());
        println!("Tokens: {:?}", &tokens[..tokens.len().min(20)]);

        let max_id = tokens.iter().max().copied().unwrap_or(0);
        let min_id = tokens.iter().min().copied().unwrap_or(0);
        println!("Token range: {} .. {}", min_id, max_id);

        // The model has 178 symbols (indices 0-177). All token IDs must be in range.
        assert!(max_id <= 177, "Token ID {} exceeds max 177", max_id);
        assert!(min_id >= 0, "Token ID {} is negative", min_id);
    }

    #[test]
    fn test_full_pipeline_smoke() {
        let model_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut onnx = InflectOnnx::new(model_dir).expect("Failed to load ONNX models");

        let waveform = onnx.synthesize_chunk("hello world", 1.0, 0.667, 42)
            .expect("Synthesis should succeed");

        assert!(!waveform.is_empty(), "Waveform should not be empty");
        assert!(waveform.len() > 1000, "Waveform should have reasonable length");

        let max_abs = waveform.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max_abs > 0.0, "Waveform should have non-zero amplitude");
        assert!(max_abs <= 1.0, "Waveform should be clipped to [-1, 1]");
    }
}