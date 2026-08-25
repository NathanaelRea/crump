//! Deterministic off-callback sinusoidal-peak analysis for graph spectral sources.
//!
//! Exact Project media bytes remain authoritative. This module consumes the immutable
//! Project-rate sample derivative and builds another disposable, versioned derivative
//! before any realtime runtime exists.

use std::{collections::BTreeSet, sync::Arc};

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    media::AssetInventory,
    model::{NodeKind, Project, SettingValue},
};

use super::sample::{CompiledSampleLibrary, CompiledSampleSource, converted_frame_count};

/// Versioned deterministic spectral analysis/resynthesis contract.
pub const SPECTRAL_ANALYSIS_ALGORITHM: &str = "crump-spectral-peaks-v1";
/// Fixed periodic-Hann analysis width.
pub const SPECTRAL_WINDOW_SIZE: usize = 2_048;
/// Fixed analysis hop in Project-rate frames.
pub const SPECTRAL_HOP_SIZE: usize = 256;
/// Maximum retained sinusoidal peaks in one analysis frame.
pub const MAX_SPECTRAL_PEAKS: usize = 128;

const MAX_COMPILED_SPECTRAL_BYTES: u64 = 192 * 1_048_576;
const MIN_PEAK_MAGNITUDE: f64 = 1.0e-12;

/// Inspectable identity and shape of one immutable spectral derivative.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SpectralAnalysisInspection {
    /// Durable Project-local asset ID.
    pub asset_id: String,
    /// Exact authoritative source-byte SHA-256.
    pub source_sha256: String,
    /// Identity of the Project-rate sample derivative analyzed as input.
    pub sample_cache_identity: String,
    /// Versioned spectral derivative identity.
    pub cache_identity: String,
    /// Analysis algorithm identity.
    pub algorithm: &'static str,
    /// Project sample rate used by analysis and resynthesis.
    pub sample_rate: u32,
    /// Fixed periodic-Hann analysis width.
    pub window_frames: usize,
    /// Fixed analysis hop.
    pub hop_frames: usize,
    /// Number of immutable analysis frames.
    pub analysis_frames: usize,
    /// Fixed retained peak slots per frame.
    pub peaks_per_frame: usize,
    /// Frames classified as transients by the versioned flux rule.
    pub transient_frames: usize,
    /// Conservatively counted immutable analysis payload bytes.
    pub compiled_bytes: u64,
}

/// One Project revision's immutable, referenced-only spectral analyses.
#[derive(Debug, Clone, Default)]
pub(crate) struct CompiledSpectralLibrary {
    analyses: Vec<CompiledSpectralAnalysis>,
}

impl CompiledSpectralLibrary {
    /// Compiles only spectral assets reached by Track-assigned graph Instruments.
    pub(crate) fn compile_for_project(
        project: &Project,
        samples: &CompiledSampleLibrary,
    ) -> Result<Self, SpectralCompileError> {
        let referenced = referenced_spectral_asset_ids(project);
        let analyses = samples
            .iter()
            .filter(|source| referenced.contains(source.asset_id()))
            .map(CompiledSpectralAnalysis::compile)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { analyses })
    }

    /// Estimates complete immutable analysis storage before sample decoding or FFT allocation.
    pub(crate) fn estimated_allocation_bytes_for_project(
        project: &Project,
        inventory: &AssetInventory,
        sample_rate: u32,
    ) -> Result<u64, SpectralCompileError> {
        if sample_rate == 0 {
            return Err(SpectralCompileError::InvalidSampleRate);
        }
        let referenced = referenced_spectral_asset_ids(project);
        let analysis_capacity = if referenced.is_empty() {
            0
        } else {
            referenced.len().saturating_mul(2).max(4)
        };
        let mut bytes = (analysis_capacity as u64)
            .saturating_mul(std::mem::size_of::<CompiledSpectralAnalysis>() as u64);
        for asset_id in referenced {
            let asset = inventory
                .iter()
                .find(|asset| asset.id() == asset_id)
                .ok_or_else(|| SpectralCompileError::MissingAsset(asset_id.to_owned()))?;
            let source_frames = usize::try_from(asset.metadata().frames)
                .map_err(|_| SpectralCompileError::TooLarge)?;
            let converted_frames =
                converted_frame_count(source_frames, asset.metadata().sample_rate, sample_rate)
                    .map_err(|_| SpectralCompileError::TooLarge)?;
            bytes = bytes.saturating_add(estimated_analysis_bytes(
                converted_frames,
                asset.id().len(),
                asset.sha256().len(),
            )?);
        }
        Ok(bytes)
    }

    pub(crate) fn get(&self, asset_id: &str) -> Option<&CompiledSpectralAnalysis> {
        self.analyses
            .iter()
            .find(|analysis| analysis.asset_id == asset_id)
    }

    pub(crate) fn inspection(&self) -> Vec<SpectralAnalysisInspection> {
        self.analyses
            .iter()
            .map(CompiledSpectralAnalysis::inspection)
            .collect()
    }

    pub(crate) fn allocation_bytes(&self) -> u64 {
        (self.analyses.capacity() as u64)
            .saturating_mul(std::mem::size_of::<CompiledSpectralAnalysis>() as u64)
            .saturating_add(
                self.analyses
                    .iter()
                    .map(CompiledSpectralAnalysis::allocation_bytes)
                    .fold(0_u64, u64::saturating_add),
            )
    }
}

fn referenced_spectral_asset_ids(project: &Project) -> BTreeSet<&str> {
    let runtime_instruments = project
        .song
        .tracks
        .iter()
        .map(|track| track.instrument.as_str())
        .collect::<BTreeSet<_>>();
    project
        .song
        .instruments
        .iter()
        .filter(|instrument| runtime_instruments.contains(instrument.id.as_str()))
        .flat_map(|instrument| {
            instrument
                .graph
                .iter()
                .chain(instrument.shared_graph.iter())
                .flat_map(|graph| &graph.nodes)
        })
        .filter(|node| node.kind == NodeKind::Spectral)
        .filter_map(|node| match node.setting("asset") {
            Some(SettingValue::Text(asset_id)) => Some(asset_id.as_str()),
            _ => None,
        })
        .collect()
}

/// Immutable flattened spectral peak frames shared by every referencing graph program.
#[derive(Debug, Clone)]
pub(crate) struct CompiledSpectralAnalysis {
    asset_id: String,
    source_sha256: String,
    sample_cache_identity: [u8; 32],
    cache_identity: [u8; 32],
    sample_rate: u32,
    source_frames: usize,
    frame_count: usize,
    transient_count: usize,
    peaks: Arc<[SpectralPeak]>,
    transients: Arc<[u8]>,
}

impl CompiledSpectralAnalysis {
    fn compile(source: &CompiledSampleSource) -> Result<Self, SpectralCompileError> {
        if source.sample_rate() == 0 {
            return Err(SpectralCompileError::InvalidSampleRate);
        }
        let expected = estimated_analysis_bytes(
            source.len(),
            source.asset_id().len(),
            source.source_sha256().len(),
        )?;
        let frame_count = analysis_frame_count(source.len());
        let (peaks, transients) = analyze(source.frames(), source.sample_rate(), frame_count)?;
        let transient_count = transients.iter().filter(|value| **value != 0).count();
        let analysis = Self {
            asset_id: source.asset_id().to_owned(),
            source_sha256: source.source_sha256().to_owned(),
            sample_cache_identity: source.cache_identity(),
            cache_identity: spectral_cache_identity(
                source.source_sha256(),
                source.cache_identity(),
                source.sample_rate(),
            ),
            sample_rate: source.sample_rate(),
            source_frames: source.len(),
            frame_count,
            transient_count,
            peaks: Arc::from(peaks),
            transients: Arc::from(transients),
        };
        debug_assert!(analysis.allocation_bytes() <= expected);
        Ok(analysis)
    }

    pub(crate) fn placeholder(
        asset_id: &str,
        source_sha256: &str,
        sample_cache_identity: [u8; 32],
        sample_rate: u32,
    ) -> Result<Self, SpectralCompileError> {
        if sample_rate == 0 {
            return Err(SpectralCompileError::InvalidSampleRate);
        }
        Ok(Self {
            asset_id: asset_id.to_owned(),
            source_sha256: source_sha256.to_owned(),
            sample_cache_identity,
            cache_identity: spectral_cache_identity(
                source_sha256,
                sample_cache_identity,
                sample_rate,
            ),
            sample_rate,
            source_frames: 1,
            frame_count: 1,
            transient_count: 0,
            peaks: Arc::from(vec![SpectralPeak::default(); MAX_SPECTRAL_PEAKS]),
            transients: Arc::from([0_u8]),
        })
    }

    pub(crate) fn peak(&self, frame: usize, slot: usize) -> SpectralPeak {
        self.peaks
            .get(
                frame
                    .saturating_mul(MAX_SPECTRAL_PEAKS)
                    .saturating_add(slot),
            )
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn transient(&self, frame: usize) -> bool {
        self.transients.get(frame).copied().unwrap_or(0) != 0
    }

    pub(crate) fn asset_id(&self) -> &str {
        &self.asset_id
    }

    pub(crate) fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    pub(crate) const fn cache_identity(&self) -> [u8; 32] {
        self.cache_identity
    }

    pub(crate) const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) const fn source_frames(&self) -> usize {
        self.source_frames
    }

    pub(crate) const fn frame_count(&self) -> usize {
        self.frame_count
    }

    pub(crate) fn allocation_bytes(&self) -> u64 {
        (self.asset_id.capacity() as u64)
            .saturating_add(self.source_sha256.capacity() as u64)
            .saturating_add((4 * std::mem::size_of::<usize>()) as u64)
            .saturating_add(
                (self.peaks.len() as u64)
                    .saturating_mul(std::mem::size_of::<SpectralPeak>() as u64),
            )
            .saturating_add(self.transients.len() as u64)
    }

    fn inspection(&self) -> SpectralAnalysisInspection {
        SpectralAnalysisInspection {
            asset_id: self.asset_id.clone(),
            source_sha256: self.source_sha256.clone(),
            sample_cache_identity: hex(&self.sample_cache_identity),
            cache_identity: hex(&self.cache_identity),
            algorithm: SPECTRAL_ANALYSIS_ALGORITHM,
            sample_rate: self.sample_rate,
            window_frames: SPECTRAL_WINDOW_SIZE,
            hop_frames: SPECTRAL_HOP_SIZE,
            analysis_frames: self.frame_count,
            peaks_per_frame: MAX_SPECTRAL_PEAKS,
            transient_frames: self.transient_count,
            compiled_bytes: (self.peaks.len() as u64)
                .saturating_mul(std::mem::size_of::<SpectralPeak>() as u64)
                .saturating_add(self.transients.len() as u64),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SpectralPeak {
    pub(crate) frequency_hz: f32,
    pub(crate) left_amplitude: f32,
    pub(crate) right_amplitude: f32,
    pub(crate) left_phase: f32,
    pub(crate) right_phase: f32,
}

fn analysis_frame_count(source_frames: usize) -> usize {
    source_frames.max(1).div_ceil(SPECTRAL_HOP_SIZE)
}

fn estimated_analysis_bytes(
    source_frames: usize,
    asset_id_bytes: usize,
    source_hash_bytes: usize,
) -> Result<u64, SpectralCompileError> {
    let frames = analysis_frame_count(source_frames);
    let peaks = frames
        .checked_mul(MAX_SPECTRAL_PEAKS)
        .ok_or(SpectralCompileError::TooLarge)?;
    let payload = (peaks as u64)
        .saturating_mul(std::mem::size_of::<SpectralPeak>() as u64)
        .saturating_add(frames as u64);
    if payload > MAX_COMPILED_SPECTRAL_BYTES {
        return Err(SpectralCompileError::TooLarge);
    }
    Ok(payload
        .saturating_add(asset_id_bytes as u64)
        .saturating_add(source_hash_bytes as u64)
        .saturating_add((4 * std::mem::size_of::<usize>()) as u64))
}

fn analyze(
    source: &[[f32; 2]],
    sample_rate: u32,
    frame_count: usize,
) -> Result<(Vec<SpectralPeak>, Vec<u8>), SpectralCompileError> {
    let peak_count = frame_count
        .checked_mul(MAX_SPECTRAL_PEAKS)
        .ok_or(SpectralCompileError::TooLarge)?;
    let mut peaks = Vec::with_capacity(peak_count);
    let mut transients = Vec::with_capacity(frame_count);
    let mut left = vec![Complex::default(); SPECTRAL_WINDOW_SIZE];
    let mut right = vec![Complex::default(); SPECTRAL_WINDOW_SIZE];
    let mut previous_magnitudes = vec![0.0_f64; SPECTRAL_WINDOW_SIZE / 2];
    let mut candidates = Vec::<(usize, f64)>::with_capacity(SPECTRAL_WINDOW_SIZE / 2);
    let window_sum = SPECTRAL_WINDOW_SIZE as f64 * 0.5;

    for analysis_frame in 0..frame_count {
        let source_start = analysis_frame.saturating_mul(SPECTRAL_HOP_SIZE);
        for index in 0..SPECTRAL_WINDOW_SIZE {
            let window = 0.5
                - 0.5 * (std::f64::consts::TAU * index as f64 / SPECTRAL_WINDOW_SIZE as f64).cos();
            let frame = source
                .get(source_start.saturating_add(index))
                .copied()
                .unwrap_or([0.0; 2]);
            left[index] = Complex {
                re: f64::from(frame[0]) * window,
                im: 0.0,
            };
            right[index] = Complex {
                re: f64::from(frame[1]) * window,
                im: 0.0,
            };
        }
        fft(&mut left);
        fft(&mut right);

        candidates.clear();
        let mut positive_flux = 0.0_f64;
        let mut previous_energy = 0.0_f64;
        for bin in 1..SPECTRAL_WINDOW_SIZE / 2 {
            let magnitude = stereo_magnitude(left[bin], right[bin]);
            positive_flux += (magnitude - previous_magnitudes[bin]).max(0.0);
            previous_energy += previous_magnitudes[bin];
            previous_magnitudes[bin] = magnitude;
            if bin + 1 < SPECTRAL_WINDOW_SIZE / 2 {
                let before = stereo_magnitude(left[bin - 1], right[bin - 1]);
                let after = stereo_magnitude(left[bin + 1], right[bin + 1]);
                if magnitude > MIN_PEAK_MAGNITUDE && magnitude >= before && magnitude > after {
                    candidates.push((bin, magnitude));
                }
            }
        }
        let transient = analysis_frame == 0
            || (previous_energy > MIN_PEAK_MAGNITUDE && positive_flux > previous_energy * 0.35);
        transients.push(u8::from(transient));
        candidates.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        for (bin, _) in candidates.iter().take(MAX_SPECTRAL_PEAKS) {
            let left_bin = left[*bin];
            let right_bin = right[*bin];
            peaks.push(SpectralPeak {
                frequency_hz: (*bin as f64 * f64::from(sample_rate) / SPECTRAL_WINDOW_SIZE as f64)
                    as f32,
                left_amplitude: bounded_f32(2.0 * left_bin.magnitude() / window_sum),
                right_amplitude: bounded_f32(2.0 * right_bin.magnitude() / window_sum),
                left_phase: left_bin.im.atan2(left_bin.re) as f32,
                right_phase: right_bin.im.atan2(right_bin.re) as f32,
            });
        }
        peaks.resize(
            (analysis_frame + 1).saturating_mul(MAX_SPECTRAL_PEAKS),
            SpectralPeak::default(),
        );
    }
    Ok((peaks, transients))
}

fn bounded_f32(value: f64) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, f32::MAX as f64) as f32
    } else {
        0.0
    }
}

fn spectral_cache_identity(
    source_sha256: &str,
    sample_cache_identity: [u8; 32],
    sample_rate: u32,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(SPECTRAL_ANALYSIS_ALGORITHM.as_bytes());
    digest.update(source_sha256.as_bytes());
    digest.update(sample_cache_identity);
    digest.update(sample_rate.to_le_bytes());
    digest.update((SPECTRAL_WINDOW_SIZE as u64).to_le_bytes());
    digest.update((SPECTRAL_HOP_SIZE as u64).to_le_bytes());
    digest.update((MAX_SPECTRAL_PEAKS as u64).to_le_bytes());
    digest.update(b"periodic-hann:stereo-ranked-peaks:flux-0.35");
    digest.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Clone, Copy, Default)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    fn magnitude(self) -> f64 {
        self.re.hypot(self.im)
    }
}

fn stereo_magnitude(left: Complex, right: Complex) -> f64 {
    ((left.re * left.re + left.im * left.im + right.re * right.re + right.im * right.im) * 0.5)
        .sqrt()
}

fn fft(values: &mut [Complex]) {
    debug_assert!(values.len().is_power_of_two());
    let length = values.len();
    let mut target = 0_usize;
    for source in 1..length {
        let mut bit = length >> 1;
        while target & bit != 0 {
            target ^= bit;
            bit >>= 1;
        }
        target ^= bit;
        if source < target {
            values.swap(source, target);
        }
    }
    let mut width = 2;
    while width <= length {
        let angle = -std::f64::consts::TAU / width as f64;
        let root = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        for start in (0..length).step_by(width) {
            let mut rotation = Complex { re: 1.0, im: 0.0 };
            for offset in 0..width / 2 {
                let even = values[start + offset];
                let odd = multiply(values[start + offset + width / 2], rotation);
                values[start + offset] = Complex {
                    re: even.re + odd.re,
                    im: even.im + odd.im,
                };
                values[start + offset + width / 2] = Complex {
                    re: even.re - odd.re,
                    im: even.im - odd.im,
                };
                rotation = multiply(rotation, root);
            }
        }
        width *= 2;
    }
}

fn multiply(left: Complex, right: Complex) -> Complex {
    Complex {
        re: left.re * right.re - left.im * right.im,
        im: left.re * right.im + left.im * right.re,
    }
}

/// Spectral derivative construction failure.
#[derive(Debug, Error)]
pub(crate) enum SpectralCompileError {
    /// The Project sample rate is zero.
    #[error("spectral analysis requires a positive Project sample rate")]
    InvalidSampleRate,
    /// A referenced exact asset was unavailable during preflight.
    #[error("spectral asset `{0}` was not available in the verified inventory")]
    MissingAsset(String),
    /// Analysis dimensions or retained payload exceed the fixed bound.
    #[error(
        "spectral analysis exceeds the fixed {MAX_COMPILED_SPECTRAL_BYTES}-byte per-source bound"
    )]
    TooLarge,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_identity_covers_exact_source_sample_derivative_and_rate() {
        let baseline = spectral_cache_identity("a", [1; 32], 48_000);
        assert_ne!(baseline, spectral_cache_identity("b", [1; 32], 48_000));
        assert_ne!(baseline, spectral_cache_identity("a", [2; 32], 48_000));
        assert_ne!(baseline, spectral_cache_identity("a", [1; 32], 96_000));
        assert_eq!(baseline, spectral_cache_identity("a", [1; 32], 48_000));
    }

    #[test]
    fn peak_analysis_is_deterministic_bounded_and_frequency_ranked() {
        let sample_rate = 48_000;
        let source = (0..4_096)
            .map(|frame| {
                let phase = std::f64::consts::TAU * 1_125.0 * frame as f64 / sample_rate as f64;
                let value = (phase.sin() * 0.5) as f32;
                [value, value]
            })
            .collect::<Vec<_>>();
        let frame_count = analysis_frame_count(source.len());
        let first = analyze(&source, sample_rate, frame_count).expect("analysis");
        let second = analyze(&source, sample_rate, frame_count).expect("analysis");
        assert_eq!(first.0.len(), frame_count * MAX_SPECTRAL_PEAKS);
        assert_eq!(first.1.len(), frame_count);
        assert_eq!(first.0[0].frequency_hz.to_bits(), 1_125.0_f32.to_bits());
        assert!(first.0[0].left_amplitude > 0.45);
        assert_eq!(
            first
                .0
                .iter()
                .map(|peak| (
                    peak.frequency_hz.to_bits(),
                    peak.left_amplitude.to_bits(),
                    peak.right_amplitude.to_bits(),
                    peak.left_phase.to_bits(),
                    peak.right_phase.to_bits(),
                ))
                .collect::<Vec<_>>(),
            second
                .0
                .iter()
                .map(|peak| (
                    peak.frequency_hz.to_bits(),
                    peak.left_amplitude.to_bits(),
                    peak.right_amplitude.to_bits(),
                    peak.left_phase.to_bits(),
                    peak.right_phase.to_bits(),
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(first.1, second.1);
    }
}
