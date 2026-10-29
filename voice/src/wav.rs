use std::path::Path;

use crate::store::StoreError;

/// Writes 16-bit mono WAV next to `path` as `.part` and renames it into place, so a crash never leaves half a WAV
/// under a real name.
pub(crate) fn write_wav(path: &Path, samples: &[i16], sample_rate: u32) -> Result<(), StoreError> {
    let part = path.with_extension("wav.part");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&part, spec)?;
    samples
        .iter()
        .try_for_each(|&sample| return writer.write_sample(sample))?;
    writer.finalize()?;
    std::fs::rename(&part, path)?;
    return Ok(());
}

/// Samples and sample rate of a WAV written by the store.
pub fn read_wav(path: &Path) -> Result<(Vec<i16>, u32), StoreError> {
    let mut reader = hound::WavReader::open(path)?;
    let sample_rate = reader.spec().sample_rate;
    let samples = reader.samples::<i16>().collect::<Result<Vec<i16>, hound::Error>>()?;
    return Ok((samples, sample_rate));
}
