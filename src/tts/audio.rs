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