//! Immutable WAV decoding and deterministic offline sample-rate conversion.
//!
//! Compilation happens while a Project revision is built, never from the audio
//! callback. Playback and rendering can consequently share the same bounded,
//! random-access frame representation. Mono sources stay mono in memory and are
//! read as two identical channels.

use std::{borrow::Cow, collections::BTreeSet, io::Cursor, sync::Arc};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    media::{AssetInventory, LoadedAsset, WavSampleFormat},
    model::{MediaAsset, NodeKind, Project, SettingValue},
};

/// Maximum decoded stereo storage admitted for one compiled source.
///
/// Decoding passes through stereo frames, so this bound applies before mono
/// sources are reduced to one channel.
pub const MAX_COMPILED_SAMPLE_BYTES: u64 = 256 * 1_048_576;
/// Versioned exact-byte sample decode and project-rate conversion identity.
pub const SAMPLE_COMPILE_ALGORITHM: &str = "crump-sample-windowed-sinc-v1";

const RESAMPLER_RADIUS: i64 = 24;

/// Inspectable identity and shape of one immutable decoded sample derivative.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SampleSourceInspection {
    /// Durable Project-local asset ID.
    pub asset_id: String,
    /// Exact authoritative source-byte SHA-256.
    pub source_sha256: String,
    /// Versioned derived-cache identity.
    pub cache_identity: String,
    /// Decode/resampling algorithm identity.
    pub algorithm: &'static str,
    /// Source WAV channel count.
    pub source_channels: u16,
    /// Source WAV sample rate.
    pub source_sample_rate: u32,
    /// Project-rate compiled sample rate.
    pub sample_rate: u32,
    /// Immutable project-rate stereo frame count.
    pub frames: usize,
    /// Conservatively counted immutable frame payload bytes.
    pub compiled_bytes: u64,
}

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

    /// Decodes only assets reachable from Track-assigned sampler or graph programs.
    pub(crate) fn compile_for_project(
        project: &Project,
        inventory: &AssetInventory,
        sample_rate: u32,
    ) -> Result<Self, SampleCompileError> {
        let referenced = runtime_sample_asset_ids(project);
        let sources = inventory
            .iter()
            .filter(|asset| referenced.contains(asset.id()))
            .map(|asset| CompiledSampleSource::compile(asset, sample_rate))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { sources })
    }

    /// Computes exact converted payload storage before decoding or allocating frame buffers.
    pub(crate) fn estimated_allocation_bytes_for_project(
        project: &Project,
        inventory: &AssetInventory,
        sample_rate: u32,
    ) -> Result<u64, SampleCompileError> {
        let referenced = runtime_sample_asset_ids(project);
        let source_capacity = if referenced.is_empty() {
            0
        } else {
            referenced.len().saturating_mul(2).max(4)
        };
        let mut bytes = (source_capacity as u64)
            .saturating_mul(std::mem::size_of::<CompiledSampleSource>() as u64);
        for asset in inventory
            .iter()
            .filter(|asset| referenced.contains(asset.id()))
        {
            let metadata = asset.metadata();
            let source_frames =
                usize::try_from(metadata.frames).map_err(|_| SampleCompileError::TooLarge)?;
            let output_frames =
                converted_frame_count(source_frames, metadata.sample_rate, sample_rate)?;
            admit_frame_storage(output_frames)?;
            bytes = bytes
                .saturating_add((asset.id().len() as u64).saturating_mul(2))
                .saturating_add((asset.sha256().len() as u64).saturating_mul(2))
                .saturating_add((2 * std::mem::size_of::<usize>()) as u64)
                .saturating_add(stored_frame_bytes(output_frames, metadata.channels));
        }
        Ok(bytes)
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

    pub(crate) fn inspection(&self) -> Vec<SampleSourceInspection> {
        self.sources
            .iter()
            .map(CompiledSampleSource::inspection)
            .collect()
    }

    /// Heap storage retained by one publication's immutable decoded media.
    pub(crate) fn allocation_bytes(&self) -> u64 {
        (self.sources.capacity() as u64)
            .saturating_mul(std::mem::size_of::<CompiledSampleSource>() as u64)
            .saturating_add(
                self.sources
                    .iter()
                    .map(CompiledSampleSource::allocation_bytes)
                    .fold(0_u64, u64::saturating_add),
            )
    }
}

fn runtime_sample_asset_ids(project: &Project) -> BTreeSet<&str> {
    let runtime_instruments = project
        .song
        .tracks
        .iter()
        .map(|track| track.instrument.as_str())
        .collect::<BTreeSet<_>>();
    let mut assets = BTreeSet::new();
    for instrument in &project.song.instruments {
        if !runtime_instruments.contains(instrument.id.as_str()) {
            continue;
        }
        if let Some(sampler) = &instrument.sampler {
            assets.extend(
                sampler
                    .mappings
                    .iter()
                    .map(|mapping| mapping.asset.as_str()),
            );
        }
        for graph in instrument
            .graph
            .iter()
            .chain(instrument.shared_graph.iter())
        {
            for node in &graph.nodes {
                match node.kind {
                    NodeKind::SampleSource | NodeKind::Granular | NodeKind::Spectral => {
                        if let Some(SettingValue::Text(asset)) = node.setting("asset") {
                            assets.insert(asset.as_str());
                        }
                    }
                    NodeKind::Multisample => {
                        for directive in &node.directives {
                            for pair in directive.tokens.windows(2) {
                                if pair[0].value == "asset" {
                                    assets.insert(pair[1].value.as_str());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    assets
}

/// An immutable, predecoded and project-rate-converted sample source.
#[derive(Debug, Clone)]
pub struct CompiledSampleSource {
    asset_id: String,
    source_sha256: String,
    cache_identity: [u8; 32],
    source_channels: u16,
    source_sample_rate: u32,
    sample_rate: u32,
    frame_count: usize,
    estimated_storage_bytes: u64,
    frames: SampleFrames,
}

/// Compiled frames in their source channel layout.
#[derive(Debug, Clone)]
enum SampleFrames {
    Mono(Arc<[f32]>),
    Stereo(Arc<[[f32; 2]]>),
}

/// Bytes retained for `frames` compiled frames of a source with `channels`.
fn stored_frame_bytes(frames: usize, channels: u16) -> u64 {
    let width = if channels == 1 {
        std::mem::size_of::<f32>()
    } else {
        std::mem::size_of::<[f32; 2]>()
    };
    (frames as u64).saturating_mul(width as u64)
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
        let frame_count = frames.len();
        let estimated_storage_bytes = stored_frame_bytes(frame_count, metadata.channels);
        let frames = if metadata.channels == 1 {
            SampleFrames::Mono(frames.iter().map(|frame| frame[0]).collect())
        } else {
            SampleFrames::Stereo(Arc::from(frames))
        };
        Ok(Self {
            asset_id: asset.id().to_owned(),
            source_sha256: asset.sha256().to_owned(),
            cache_identity: sample_cache_identity(asset.sha256(), sample_rate),
            source_channels: metadata.channels,
            source_sample_rate: metadata.sample_rate,
            sample_rate,
            frame_count,
            estimated_storage_bytes,
            frames,
        })
    }

    pub(crate) fn placeholder(
        declaration: &MediaAsset,
        sample_rate: u32,
        minimum_frames: usize,
    ) -> Result<Self, SampleCompileError> {
        if sample_rate == 0 {
            return Err(SampleCompileError::InvalidTargetSampleRate);
        }
        let frame_count = minimum_frames.max(1);
        admit_frame_storage(frame_count)?;
        Ok(Self {
            asset_id: declaration.id.clone(),
            source_sha256: declaration.sha256.clone(),
            cache_identity: sample_cache_identity(&declaration.sha256, sample_rate),
            source_channels: 1,
            source_sample_rate: sample_rate,
            sample_rate,
            frame_count,
            estimated_storage_bytes: (frame_count as u64)
                .saturating_mul(std::mem::size_of::<[f32; 2]>() as u64),
            frames: SampleFrames::Stereo(Arc::from([])),
        })
    }

    /// Durable media asset identity.
    #[must_use]
    pub fn asset_id(&self) -> &str {
        &self.asset_id
    }

    /// Exact authoritative source-byte SHA-256.
    #[must_use]
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    pub(crate) const fn cache_identity(&self) -> [u8; 32] {
        self.cache_identity
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
    pub const fn len(&self) -> usize {
        self.frame_count
    }

    /// Whether this source contains no compiled frames.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.frame_count == 0
    }

    /// Returns one stereo frame. Mono WAVs are duplicated without gain change.
    #[must_use]
    pub fn frame(&self, index: usize) -> Option<[f32; 2]> {
        match &self.frames {
            SampleFrames::Mono(samples) => samples.get(index).map(|&sample| [sample, sample]),
            SampleFrames::Stereo(frames) => frames.get(index).copied(),
        }
    }

    /// Wraps already-compiled stereo frames for unit tests.
    #[cfg(test)]
    pub(crate) fn from_test_frames(frames: Vec<[f32; 2]>) -> Self {
        Self {
            asset_id: "test".to_owned(),
            source_sha256: String::new(),
            cache_identity: [0; 32],
            source_channels: 2,
            source_sample_rate: 48_000,
            sample_rate: 48_000,
            frame_count: frames.len(),
            estimated_storage_bytes: stored_frame_bytes(frames.len(), 2),
            frames: SampleFrames::Stereo(Arc::from(frames)),
        }
    }

    /// Number of frames actually stored. Validation placeholders store none.
    #[must_use]
    pub(crate) fn stored_len(&self) -> usize {
        match &self.frames {
            SampleFrames::Mono(samples) => samples.len(),
            SampleFrames::Stereo(frames) => frames.len(),
        }
    }

    /// Contiguous stereo frames for offline analysis and transforms. Mono sources
    /// allocate a duplicated copy, so realtime code reads through [`Self::frame`].
    #[must_use]
    pub fn stereo_frames(&self) -> Cow<'_, [[f32; 2]]> {
        match &self.frames {
            SampleFrames::Mono(samples) => samples.iter().map(|&sample| [sample, sample]).collect(),
            SampleFrames::Stereo(frames) => Cow::Borrowed(frames),
        }
    }

    pub(crate) fn allocation_bytes(&self) -> u64 {
        self.estimated_storage_bytes
            .saturating_add(self.asset_id.capacity() as u64)
            .saturating_add(self.source_sha256.capacity() as u64)
            .saturating_add((2 * std::mem::size_of::<usize>()) as u64)
    }

    fn inspection(&self) -> SampleSourceInspection {
        SampleSourceInspection {
            asset_id: self.asset_id.clone(),
            source_sha256: self.source_sha256.clone(),
            cache_identity: hex(&self.cache_identity),
            algorithm: SAMPLE_COMPILE_ALGORITHM,
            source_channels: self.source_channels,
            source_sample_rate: self.source_sample_rate,
            sample_rate: self.sample_rate,
            frames: self.frame_count,
            compiled_bytes: self.estimated_storage_bytes,
        }
    }
}

fn sample_cache_identity(source_sha256: &str, sample_rate: u32) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(SAMPLE_COMPILE_ALGORITHM.as_bytes());
    digest.update(source_sha256.as_bytes());
    digest.update(sample_rate.to_le_bytes());
    digest.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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

pub(crate) fn decode_wav(asset: &LoadedAsset) -> Result<Vec<[f32; 2]>, SampleCompileError> {
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
            .as_chunks::<2>()
            .0
            .iter()
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

pub(crate) fn converted_frame_count(
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
            usages: Vec::new(),
            wavetable: None,
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

    /// Allocation bytes that are not frame storage.
    fn stereo_overhead(source: &CompiledSampleSource) -> u64 {
        source.allocation_bytes() - source.estimated_storage_bytes
    }

    #[test]
    fn decode_preserves_stereo_and_duplicates_mono() {
        let mono =
            CompiledSampleSource::compile(&asset(int_wav(48_000, 1, &[16_384, -16_384])), 48_000)
                .expect("compile mono");
        assert_eq!(mono.source_channels(), 1);
        assert_eq!(&*mono.stereo_frames(), &[[0.5, 0.5], [-0.5, -0.5]]);
        assert_eq!(mono.frame(1), Some([-0.5, -0.5]));
        assert_eq!(mono.allocation_bytes() - stereo_overhead(&mono), 8);

        let stereo = CompiledSampleSource::compile(
            &asset(int_wav(48_000, 2, &[16_384, -16_384, 8_192, -8_192])),
            48_000,
        )
        .expect("compile stereo");
        assert_eq!(&*stereo.stereo_frames(), &[[0.5, -0.5], [0.25, -0.25]]);
        assert_eq!(stereo.allocation_bytes() - stereo_overhead(&stereo), 16);
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
        assert_eq!(first.stereo_frames(), second.stereo_frames());
        assert!(
            first
                .stereo_frames()
                .iter()
                .all(|frame| (frame[0] - 0.25).abs() < 1.0e-6)
        );
    }

    #[test]
    fn runtime_referenced_sample_storage_is_preflighted_conservatively() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/synth-alpha");
        let path = root.join("minimal-multisample.crump");
        let source = std::fs::read_to_string(&path).expect("fixture");
        let parsed = crate::dsl::parse_project(&path, &source);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("Project");
        let (inventory, diagnostics) = crate::media::load_asset_inventory(&root, &project.assets);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let estimated = CompiledSampleLibrary::estimated_allocation_bytes_for_project(
            &project, &inventory, 48_000,
        )
        .expect("estimate");
        let library = CompiledSampleLibrary::compile_for_project(&project, &inventory, 48_000)
            .expect("compile");
        assert!(estimated >= library.allocation_bytes());
        assert_eq!(library.inspection().len(), 2);
        assert!(library.inspection().iter().all(|source| {
            source.algorithm == SAMPLE_COMPILE_ALGORITHM
                && source.cache_identity.len() == 64
                && source.compiled_bytes > 0
        }));
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
        let rms = (compiled.stereo_frames()[100..]
            .iter()
            .map(|frame| f64::from(frame[0]).powi(2))
            .sum::<f64>()
            / (compiled.len() - 100) as f64)
            .sqrt();
        assert!(rms < 0.01, "aliased RMS was {rms}");
    }
}
