mod audio;
mod normalize;
mod numbers;
mod onnx;
mod phonemes;
mod symbols;

use std::path::Path;


pub use onnx::InflectOnnx;
pub use normalize::normalize_text;

/// Synthesis parameters.
#[derive(Clone, Debug)]
pub struct SynthesizeParams {
    /// Speaking rate: 0.5 (slow) .. 2.0 (fast). Default 1.0.
    pub speed: f32,
    /// Stochastic variation: 0.0 (monotone) .. 1.0 (expressive). Default 0.667.
    pub variation: f32,
    /// Random seed for reproducible output.
    pub seed: i32,
}

impl Default for SynthesizeParams {
    fn default() -> Self {
        Self {
            speed: 1.0,
            variation: 0.667,
            seed: 0,
        }
    }
}

/// Top-level Inflect TTS engine: text normalization → phonemization → ONNX synthesis.
pub struct InflectTts {
    onnx: InflectOnnx,
}

impl InflectTts {
    /// Load ONNX models from `model_dir` (directory containing `onnx/duration.onnx`
    /// and `onnx/decode.onnx`).
    pub fn new(model_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let onnx = InflectOnnx::new(model_dir)?;
        Ok(Self { onnx })
    }

    /// Synthesize text into a 24 kHz mono f32 waveform.
    /// Returns `(sample_rate, waveform)`.
    pub fn synthesize(
        &mut self,
        text: &str,
        params: &SynthesizeParams,
    ) -> Result<(u32, Vec<f32>), Box<dyn std::error::Error>> {
        let normalized = normalize_text(text);
        if normalized.is_empty() {
            return Err("text must not be empty".into());
        }
        if !(0.5..=2.0).contains(&params.speed) {
            return Err("speed must be between 0.5 and 2.0".into());
        }
        if !(0.0..=1.0).contains(&params.variation) {
            return Err("variation must be between 0.0 and 1.0".into());
        }

        let chunks = normalize::split_text(&normalized);
        let mut pieces: Vec<Vec<f32>> = Vec::with_capacity(chunks.len() * 2);

        for (index, chunk) in chunks.iter().enumerate() {
            if index > 0 {
                let pause = normalize::boundary_pause_seconds(&chunks[index - 1]);
                let silence_len = (24000.0 * pause).round() as usize;
                pieces.push(vec![0.0f32; silence_len]);
            }
            let seed = params.seed.wrapping_add(index as i32);
            let waveform = self.onnx.synthesize_chunk(
                chunk,
                params.speed,
                params.variation,
                seed,
            )?;
            pieces.push(waveform);
        }

        let total_len: usize = pieces.iter().map(|p| p.len()).sum();
        let mut waveform = Vec::with_capacity(total_len);
        for piece in pieces {
            waveform.extend_from_slice(&piece);
        }

        // Clip to [-1, 1]
        for sample in &mut waveform {
            *sample = sample.clamp(-1.0, 1.0);
        }

        Ok((24_000, waveform))
    }

    /// Synthesize and save to a WAV file.
    pub fn save(
        &mut self,
        text: &str,
        output: &Path,
        params: &SynthesizeParams,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let (sample_rate, waveform) = self.synthesize(text, params)?;
        audio::save_wav(output, sample_rate, &waveform)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synthesize_params_default() {
        let params = SynthesizeParams::default();
        assert!((params.speed - 1.0).abs() < f32::EPSILON);
        assert!((params.variation - 0.667).abs() < 0.001);
        assert_eq!(params.seed, 0);
    }

    #[test]
    fn test_synthesize_params_clone() {
        let params = SynthesizeParams {
            speed: 1.5,
            variation: 0.5,
            seed: 42,
        };
        let cloned = params.clone();
        assert!((cloned.speed - 1.5).abs() < f32::EPSILON);
        assert!((cloned.variation - 0.5).abs() < f32::EPSILON);
        assert_eq!(cloned.seed, 42);
    }

    #[test]
    fn test_synthesize_params_debug() {
        let params = SynthesizeParams::default();
        let debug = format!("{:?}", params);
        assert!(debug.contains("speed"));
        assert!(debug.contains("variation"));
        assert!(debug.contains("seed"));
    }
}