/// WAV file output for synthesized audio.

use std::path::Path;

/// Save a mono f32 waveform as a 16-bit PCM WAV file.
pub fn save_wav(
    path: &Path,
    sample_rate: u32,
    waveform: &[f32],
) -> Result<(), Box<dyn std::error::Error>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(path, spec)?;

    // Convert f32 [-1, 1] to i16
    for &sample in waveform {
        let clamped = sample.clamp(-1.0, 1.0);
        let int_sample = (clamped * 32767.0) as i16;
        writer.write_sample(int_sample)?;
    }

    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_save_wav_creates_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_output.wav");
        let _ = std::fs::remove_file(&path);

        let waveform = vec![0.0f32, 0.5, -0.5, 1.0, -1.0, 0.3];
        save_wav(&path, 24000, &waveform).expect("failed to save WAV");

        assert!(path.exists(), "WAV file should exist");

        // Verify WAV header and content
        let reader = hound::WavReader::open(&path).expect("failed to open WAV");
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 24000);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(spec.sample_format, hound::SampleFormat::Int);
        assert_eq!(reader.len() as usize, waveform.len());

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_save_wav_empty() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_empty.wav");
        let _ = std::fs::remove_file(&path);

        let waveform: Vec<f32> = vec![];
        save_wav(&path, 44100, &waveform).expect("failed to save empty WAV");

        let reader = hound::WavReader::open(&path).expect("failed to open WAV");
        assert_eq!(reader.len(), 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_save_wav_clamps_values() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_clamp.wav");
        let _ = std::fs::remove_file(&path);

        // Values outside [-1, 1] should be clamped
        let waveform = vec![-1.5f32, 0.0, 1.5];
        save_wav(&path, 8000, &waveform).expect("failed to save WAV");

        let mut reader = hound::WavReader::open(&path).expect("failed to open WAV");
        let samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(samples.len(), 3);
        // -1.5 clamped to -1.0 -> -32767
        assert_eq!(samples[0], -32767);
        assert_eq!(samples[1], 0);
        // 1.5 clamped to 1.0 -> 32767
        assert_eq!(samples[2], 32767);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_save_wav_invalid_path() {
        let result = save_wav(Path::new("/nonexistent_dir/output.wav"), 24000, &[0.0f32]);
        assert!(result.is_err());
    }
}