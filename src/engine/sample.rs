//! Immutable WAV decoding and deterministic offline sample-rate conversion.
//!
//! Compilation happens while a Project revision is built, never from the audio
//! callback. Playback and rendering can consequently share the same bounded,
//! random-access stereo frame representation.

use std::{io::Cursor, sync::Arc};

use crate::media::{AssetInventory, LoadedAsset, WavSampleFormat};

/// Maximum decoded stereo storage admitted for one compiled source.
pub const MAX_COMPILED_SAMPLE_BYTES: u64 = 256 * 1_048_576;

const RESAMPLER_RADIUS: i64 = 24;

/// Immutable compiled sources for one Project revision.
#[derive(Debug, Clone, Default)]
pub struct CompiledSampleLibrary {
    sources: Vec<CompiledSampleSource>,
}

impl CompiledSampleLibrary {
    /// Decodes and converts every admitted asset in authored order.
    pub fn compile(
        inventory: &AssetInventory,
        sample_rate: u32,
    ) -> Result<Self, SampleCompileError> {
        let sources = inventory
            .iter()
            .map(|asset| CompiledSampleSource::compile(asset, sample_rate))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { sources })
    }

    /// Looks up one immutable source by durable asset ID.
    #[must_use]
    pub fn get(&self, asset_id: &str) -> Option<&CompiledSampleSource> {
        self.sources
            .iter()
            .find(|source| source.asset_id == asset_id)
    }

    /// Iterates in authored asset order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &CompiledSampleSource> {
        self.sources.iter()
    }

    /// Returns whether the Project has no compiled sources.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Heap storage retained by one publication's immutable decoded media.
    pub(crate) fn allocation_bytes(&self) -> u64 {
        (self.sources.capacity() as u64)
            .saturating_mul(std::mem::size_of::<CompiledSampleSource>() as u64)
            .saturating_add(
                self.sources
                    .iter()
                    .map(|source| {
                        (source.asset_id.capacity() as u64).saturating_add(
                            (source.frames.len() as u64)
                                .saturating_mul(std::mem::size_of::<[f32; 2]>() as u64),
                        )
                    })
                    .fold(0_u64, u64::saturating_add),
            )
    }
}

/// An immutable, predecoded and project-rate-converted sample source.
#[derive(Debug, Clone)]
pub struct CompiledSampleSource {
    asset_id: String,
    source_channels: u16,
    source_sample_rate: u32,
    sample_rate: u32,
    frames: Arc<[[f32; 2]]>,
}

impl CompiledSampleSource {
    /// Decodes one loaded WAV and converts it to `sample_rate` away from the callback.
    pub fn compile(asset: &LoadedAsset, sample_rate: u32) -> Result<Self, SampleCompileError> {
        if sample_rate == 0 {
            return Err(SampleCompileError::InvalidTargetSampleRate);
        }
        let metadata = asset.metadata();
        let source_frames =
            usize::try_from(metadata.frames).map_err(|_| SampleCompileError::TooLarge)?;
        admit_frame_storage(source_frames)?;
        let output_frames =
            converted_frame_count(source_frames, metadata.sample_rate, sample_rate)?;
        admit_frame_storage(output_frames)?;
        let decoded = decode_wav(asset)?;
        let frames = if metadata.sample_rate == sample_rate {
            decoded
        } else {
            resample(&decoded, metadata.sample_rate, sample_rate, output_frames)
        };
        Ok(Self {
            asset_id: asset.id().to_owned(),
            source_channels: metadata.channels,
            source_sample_rate: metadata.sample_rate,
            sample_rate,
            frames: Arc::from(frames),
        })
    }

    /// Durable media asset identity.
    #[must_use]
    pub fn asset_id(&self) -> &str {
        &self.asset_id
    }

    /// Channel count in the source WAV before mono duplication.
    #[must_use]
    pub const fn source_channels(&self) -> u16 {
        self.source_channels
    }

    /// Sample rate in the source WAV.
    #[must_use]
    pub const fn source_sample_rate(&self) -> u32 {
        self.source_sample_rate
    }

    /// Sample rate of the compiled frames.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Number of random-access compiled frames.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether this source contains no compiled frames.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Returns one stereo frame. Mono WAVs are duplicated without gain change.
    #[must_use]
    pub fn frame(&self, index: usize) -> Option<[f32; 2]> {
        self.frames.get(index).copied()
    }

    /// Immutable contiguous frames shared by realtime and offline adapters.
    #[must_use]
    pub fn frames(&self) -> &[[f32; 2]] {
        &self.frames
    }
}

/// A failure while preparing an immutable sample source.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SampleCompileError {
    /// The requested engine sample rate was zero.
    #[error("target sample rate must be positive")]
    InvalidTargetSampleRate,
    /// The decoded or converted source exceeds the fixed per-source admission bound.
    #[error("compiled sample exceeds the {MAX_COMPILED_SAMPLE_BYTES}-byte per-source limit")]
    TooLarge,
    /// The WAV became unreadable despite inventory validation.
    #[error("asset `{asset_id}` could not be decoded: {message}")]
    Decode {
        /// Durable asset identity.
        asset_id: String,
        /// Decoder detail.
        message: String,
    },
    /// Float WAV input contained NaN or infinity.
    #[error("asset `{asset_id}` contains a non-finite sample at interleaved index {sample_index}")]
    NonFinite {
        /// Durable asset identity.
        asset_id: String,
        /// Zero-based interleaved sample index.
        sample_index: usize,
    },
}

fn decode_wav(asset: &LoadedAsset) -> Result<Vec<[f32; 2]>, SampleCompileError> {
    let metadata = asset.metadata();
    let mut reader = hound::WavReader::new(Cursor::new(asset.bytes()))
        .map_err(|error| decode_error(asset, error))?;
    let sample_count = usize::try_from(metadata.frames)
        .ok()
        .and_then(|frames| frames.checked_mul(usize::from(metadata.channels)))
        .ok_or(SampleCompileError::TooLarge)?;
    let mut samples = Vec::with_capacity(sample_count);
    match metadata.sample_format {
        WavSampleFormat::Integer => {
            let scale = 2.0_f64.powi(i32::from(metadata.bits_per_sample) - 1);
            for sample in reader.samples::<i32>() {
                let sample = sample.map_err(|error| decode_error(asset, error))?;
                samples.push((f64::from(sample) / scale).clamp(-1.0, 1.0) as f32);
            }
        }
        WavSampleFormat::Float => {
            for (sample_index, sample) in reader.samples::<f32>().enumerate() {
                let sample = sample.map_err(|error| decode_error(asset, error))?;
                if !sample.is_finite() {
                    return Err(SampleCompileError::NonFinite {
                        asset_id: asset.id().to_owned(),
                        sample_index,
                    });
                }
                samples.push(sample);
            }
        }
    }
    if samples.len() != sample_count {
        return Err(SampleCompileError::Decode {
            asset_id: asset.id().to_owned(),
            message: format!("expected {sample_count} samples, decoded {}", samples.len()),
        });
    }
    let frames = if metadata.channels == 1 {
        samples.into_iter().map(|sample| [sample, sample]).collect()
    } else {
        samples
            .chunks_exact(2)
            .map(|sample| [sample[0], sample[1]])
            .collect()
    };
    Ok(frames)
}

fn decode_error(asset: &LoadedAsset, error: hound::Error) -> SampleCompileError {
    SampleCompileError::Decode {
        asset_id: asset.id().to_owned(),
        message: error.to_string(),
    }
}

fn admit_frame_storage(frames: usize) -> Result<(), SampleCompileError> {
    let bytes = (frames as u64)
        .checked_mul(std::mem::size_of::<[f32; 2]>() as u64)
        .ok_or(SampleCompileError::TooLarge)?;
    if bytes > MAX_COMPILED_SAMPLE_BYTES {
        return Err(SampleCompileError::TooLarge);
    }
    Ok(())
}

fn converted_frame_count(
    source_frames: usize,
    source_rate: u32,
    target_rate: u32,
) -> Result<usize, SampleCompileError> {
    let numerator = (source_frames as u128) * u128::from(target_rate);
    let frames = numerator.div_ceil(u128::from(source_rate));
    usize::try_from(frames).map_err(|_| SampleCompileError::TooLarge)
}

fn resample(
    source: &[[f32; 2]],
    source_rate: u32,
    target_rate: u32,
    output_frames: usize,
) -> Vec<[f32; 2]> {
    let ratio = f64::from(source_rate) / f64::from(target_rate);
    let cutoff = (f64::from(target_rate) / f64::from(source_rate)).min(1.0);
    let mut output = Vec::with_capacity(output_frames);
    for output_index in 0..output_frames {
        let source_position = output_index as f64 * ratio;
        let center = source_position.floor() as i64;
        let mut sum = [0.0_f64; 2];
        let mut weight_sum = 0.0_f64;
        for source_index in (center - RESAMPLER_RADIUS + 1)..=(center + RESAMPLER_RADIUS) {
            let Some(frame) = usize::try_from(source_index)
                .ok()
                .and_then(|index| source.get(index))
            else {
                continue;
            };
            let distance = source_position - source_index as f64;
            let normalized = distance / RESAMPLER_RADIUS as f64;
            if normalized.abs() >= 1.0 {
                continue;
            }
            let window = 0.5 + 0.5 * (std::f64::consts::PI * normalized).cos();
            let weight = cutoff * sinc(cutoff * distance) * window;
            sum[0] += f64::from(frame[0]) * weight;
            sum[1] += f64::from(frame[1]) * weight;
            weight_sum += weight;
        }
        if weight_sum.abs() > f64::EPSILON {
            output.push([(sum[0] / weight_sum) as f32, (sum[1] / weight_sum) as f32]);
        } else {
            output.push([0.0, 0.0]);
        }
    }
    output
}

fn sinc(value: f64) -> f64 {
    if value.abs() < 1.0e-12 {
        1.0
    } else {
        let radians = std::f64::consts::PI * value;
        radians.sin() / radians
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, io::Cursor, path::PathBuf};

    use crate::{
        diagnostic::SourceLocation,
        media::load_asset_inventory_from_bytes,
        model::{MediaAsset, ValueSite},
    };

    use super::*;

    fn asset(bytes: Vec<u8>) -> LoadedAsset {
        use sha2::Digest as _;
        let digest = sha2::Sha256::digest(&bytes);
        let digest = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        let declaration = MediaAsset {
            id: "sample".to_owned(),
            name: "Sample".to_owned(),
            path: PathBuf::from("media/sample.wav"),
            sha256: digest,
            generated: None,
            path_site: ValueSite::new("project.crump", 0, 0, ""),
            sha256_site: ValueSite::new("project.crump", 0, 0, ""),
            location: SourceLocation::file("project.crump"),
        };
        let mut provided = BTreeMap::new();
        provided.insert(declaration.path.clone(), bytes);
        let (inventory, diagnostics) = load_asset_inventory_from_bytes(&[declaration], &provided);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        inventory.get("sample").expect("asset").clone()
    }

    fn int_wav(sample_rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut cursor,
            hound::WavSpec {
                channels,
                sample_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .expect("writer");
        for sample in samples {
            writer.write_sample(*sample).expect("sample");
        }
        writer.finalize().expect("finalize");
        cursor.into_inner()
    }

    fn float_wav(samples: &[f32]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut cursor,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .expect("writer");
        for sample in samples {
            writer.write_sample(*sample).expect("sample");
        }
        writer.finalize().expect("finalize");
        cursor.into_inner()
    }

    #[test]
    fn decode_preserves_stereo_and_duplicates_mono() {
        let mono =
            CompiledSampleSource::compile(&asset(int_wav(48_000, 1, &[16_384, -16_384])), 48_000)
                .expect("compile mono");
        assert_eq!(mono.source_channels(), 1);
        assert_eq!(mono.frames(), &[[0.5, 0.5], [-0.5, -0.5]]);

        let stereo = CompiledSampleSource::compile(
            &asset(int_wav(48_000, 2, &[16_384, -16_384, 8_192, -8_192])),
            48_000,
        )
        .expect("compile stereo");
        assert_eq!(stereo.frames(), &[[0.5, -0.5], [0.25, -0.25]]);
    }

    #[test]
    fn float_decode_rejects_non_finite_input_before_publication() {
        let source = asset(float_wav(&[0.0, f32::NAN]));
        assert!(matches!(
            CompiledSampleSource::compile(&source, 48_000),
            Err(SampleCompileError::NonFinite {
                sample_index: 1,
                ..
            })
        ));
    }

    #[test]
    fn conversion_is_deterministic_has_exact_duration_and_preserves_dc() {
        let source = asset(int_wav(44_100, 1, &vec![8_192; 441]));
        let first = CompiledSampleSource::compile(&source, 48_000).expect("first");
        let second = CompiledSampleSource::compile(&source, 48_000).expect("second");
        assert_eq!(first.len(), 480);
        assert_eq!(first.frames(), second.frames());
        assert!(
            first
                .frames()
                .iter()
                .all(|frame| (frame[0] - 0.25).abs() < 1.0e-6)
        );
    }

    #[test]
    fn downsampling_rejects_a_frequency_above_the_new_nyquist_band() {
        let samples = (0..4_800)
            .map(|index| {
                let phase = 2.0 * std::f64::consts::PI * 18_000.0 * index as f64 / 48_000.0;
                (phase.sin() * 20_000.0) as i16
            })
            .collect::<Vec<_>>();
        let compiled = CompiledSampleSource::compile(&asset(int_wav(48_000, 1, &samples)), 24_000)
            .expect("compile");
        let rms = (compiled.frames()[100..]
            .iter()
            .map(|frame| f64::from(frame[0]).powi(2))
            .sum::<f64>()
            / (compiled.len() - 100) as f64)
            .sqrt();
        assert!(rms < 0.01, "aliased RMS was {rms}");
    }
}
