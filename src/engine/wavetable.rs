//! Immutable exact-asset wavetable compilation and deterministic band-limited lookup.

use std::{collections::BTreeSet, sync::Arc};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    media::{AssetInventory, LoadedAsset},
    model::{
        MediaAsset, NodeKind, Project, SettingValue, WavetableAssetDefinition, WavetableMorph,
        WavetableNormalization,
    },
};

use super::sample::{SampleCompileError, decode_wav};

pub(crate) const WAVETABLE_ALGORITHM: &str = "crump-wavetable-radix2-v1";

/// Inspectable identity and shape of one disposable compiled table cache.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WavetableInspection {
    /// Durable Project-local exact asset ID.
    pub asset_id: String,
    /// Exact authoritative source-byte SHA-256.
    pub source_sha256: String,
    /// Derived-cache SHA-256 binding source, algorithm, sample rate, and structure.
    pub cache_identity: String,
    /// Versioned deterministic table compiler.
    pub algorithm: &'static str,
    /// Project sample rate used for safe mip selection.
    pub sample_rate: u32,
    /// Samples in each periodic authored frame.
    pub frame_length: usize,
    /// Authored frame count.
    pub frames: usize,
    /// Source-amplitude policy applied before Fourier filtering.
    pub normalization: WavetableNormalization,
    /// Inter-frame interpolation law.
    pub morph: WavetableMorph,
    /// Maximum retained harmonic for every compiled level, highest first.
    pub max_harmonics: Vec<u32>,
    /// Immutable derived sample and level-index storage.
    pub compiled_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CompiledWavetableLibrary {
    tables: Vec<Arc<CompiledWavetable>>,
}

impl CompiledWavetableLibrary {
    pub(crate) fn compile(
        project: &Project,
        inventory: &AssetInventory,
        sample_rate: u32,
    ) -> Result<Self, WavetableCompileError> {
        let referenced = referenced_wavetable_asset_ids(project);
        let mut tables = Vec::with_capacity(referenced.len());
        for declaration in &project.assets {
            if !referenced.contains(declaration.id.as_str()) {
                continue;
            }
            let Some(definition) = declaration.wavetable.as_ref() else {
                continue;
            };
            let asset = inventory
                .get(&declaration.id)
                .ok_or_else(|| WavetableCompileError::MissingAsset(declaration.id.clone()))?;
            tables.push(Arc::new(CompiledWavetable::compile(
                declaration,
                definition,
                asset,
                sample_rate,
            )?));
        }
        Ok(Self { tables })
    }

    pub(crate) fn get(&self, asset_id: &str) -> Option<&Arc<CompiledWavetable>> {
        self.tables.iter().find(|table| table.asset_id == asset_id)
    }

    pub(crate) fn inspection(&self) -> Vec<WavetableInspection> {
        self.tables.iter().map(|table| table.inspection()).collect()
    }
}

fn referenced_wavetable_asset_ids(project: &Project) -> BTreeSet<&str> {
    let runtime_instruments = project
        .song
        .tracks
        .iter()
        .map(|track| track.instrument.as_str())
        .collect::<BTreeSet<_>>();
    let mut referenced = BTreeSet::new();
    for instrument in &project.song.instruments {
        if !runtime_instruments.contains(instrument.id.as_str()) {
            continue;
        }
        for graph in instrument
            .graph
            .iter()
            .chain(instrument.shared_graph.iter())
        {
            for node in &graph.nodes {
                if node.kind != NodeKind::Wavetable {
                    continue;
                }
                if let Some(SettingValue::Text(asset_id)) = node.setting("asset") {
                    referenced.insert(asset_id.as_str());
                }
            }
        }
    }
    referenced
}

/// One derived, replaceable table cache shared by every voice of one compiled graph program.
#[derive(Debug, Clone)]
pub(crate) struct CompiledWavetable {
    asset_id: String,
    source_sha256: String,
    cache_identity: [u8; 32],
    sample_rate: u32,
    frame_length: usize,
    frame_count: usize,
    normalization: WavetableNormalization,
    morph: WavetableMorph,
    max_harmonics: Arc<[u32]>,
    samples: Arc<[f32]>,
    estimated_storage_bytes: u64,
}

impl CompiledWavetable {
    fn compile(
        declaration: &MediaAsset,
        definition: &WavetableAssetDefinition,
        asset: &LoadedAsset,
        sample_rate: u32,
    ) -> Result<Self, WavetableCompileError> {
        if asset.metadata().channels != 1 {
            return Err(WavetableCompileError::StereoAsset(declaration.id.clone()));
        }
        let decoded = decode_wav(asset)?;
        let samples = decoded
            .into_iter()
            .map(|frame| f64::from(frame[0]))
            .collect::<Vec<_>>();
        Self::compile_samples(declaration, definition, &samples, sample_rate)
    }

    fn compile_samples(
        declaration: &MediaAsset,
        definition: &WavetableAssetDefinition,
        source: &[f64],
        sample_rate: u32,
    ) -> Result<Self, WavetableCompileError> {
        let frame_length = usize::try_from(definition.frame_length)
            .map_err(|_| WavetableCompileError::TooLarge)?;
        let frame_count = usize::from(definition.frames);
        let expected = frame_length
            .checked_mul(frame_count)
            .ok_or(WavetableCompileError::TooLarge)?;
        if source.len() != expected {
            return Err(WavetableCompileError::FrameCount {
                asset_id: declaration.id.clone(),
                expected,
                actual: source.len(),
            });
        }
        if !frame_length.is_power_of_two()
            || !(crate::validate::MIN_WAVETABLE_FRAME_LENGTH
                ..=crate::validate::MAX_WAVETABLE_FRAME_LENGTH)
                .contains(&definition.frame_length)
            || !(1..=crate::validate::MAX_WAVETABLE_FRAMES).contains(&definition.frames)
            || expected as u64 > crate::validate::MAX_WAVETABLE_SAMPLES
            || sample_rate == 0
        {
            return Err(WavetableCompileError::InvalidDefinition(
                declaration.id.clone(),
            ));
        }
        if source.iter().any(|sample| !sample.is_finite()) {
            return Err(WavetableCompileError::NonFinite(declaration.id.clone()));
        }
        let global_peak = if definition.normalization == WavetableNormalization::GlobalPeak {
            source.iter().map(|sample| sample.abs()).fold(0.0, f64::max)
        } else {
            0.0
        };

        let max_harmonics = harmonic_levels(frame_length);
        let sample_count = expected
            .checked_mul(max_harmonics.len())
            .ok_or(WavetableCompileError::TooLarge)?;
        let estimated_storage_bytes = (sample_count as u64)
            .saturating_mul(std::mem::size_of::<f32>() as u64)
            .saturating_add(
                (max_harmonics.len() as u64).saturating_mul(std::mem::size_of::<u32>() as u64),
            );
        let mut compiled = Vec::with_capacity(sample_count);
        for authored in source.chunks_exact(frame_length) {
            let mut normalized = authored.to_vec();
            let peak = match definition.normalization {
                WavetableNormalization::Preserve => 0.0,
                WavetableNormalization::GlobalPeak => global_peak,
                WavetableNormalization::FramePeak => normalized
                    .iter()
                    .map(|sample| sample.abs())
                    .fold(0.0, f64::max),
            };
            if peak > 0.0 {
                for sample in &mut normalized {
                    *sample /= peak;
                }
            }
            let mut spectrum = normalized
                .into_iter()
                .map(|re| Complex { re, im: 0.0 })
                .collect::<Vec<_>>();
            fft(&mut spectrum, false);
            for &maximum in &max_harmonics {
                let mut limited = spectrum.clone();
                for (bin, value) in limited.iter_mut().enumerate() {
                    let harmonic = bin.min(frame_length - bin) as u32;
                    if harmonic > maximum {
                        *value = Complex::default();
                    }
                }
                fft(&mut limited, true);
                compiled.extend(limited.into_iter().map(|value| value.re as f32));
            }
        }

        // Compilation above emits frame-major levels. Reorder to level-major frames so
        // one selected mip has contiguous frame reads during callback processing.
        let mut level_major = vec![0.0_f32; compiled.len()];
        for frame in 0..frame_count {
            for level in 0..max_harmonics.len() {
                let source_offset = (frame * max_harmonics.len() + level) * frame_length;
                let destination_offset = (level * frame_count + frame) * frame_length;
                level_major[destination_offset..destination_offset + frame_length]
                    .copy_from_slice(&compiled[source_offset..source_offset + frame_length]);
            }
        }

        Ok(Self {
            asset_id: declaration.id.clone(),
            source_sha256: declaration.sha256.clone(),
            cache_identity: cache_identity(declaration, definition, sample_rate),
            sample_rate,
            frame_length,
            frame_count,
            normalization: definition.normalization,
            morph: definition.morph,
            max_harmonics: Arc::from(max_harmonics),
            samples: Arc::from(level_major),
            estimated_storage_bytes,
        })
    }

    /// Creates a non-runnable, allocation-free shape used only by model resource estimation.
    pub(crate) fn placeholder(
        declaration: &MediaAsset,
        definition: &WavetableAssetDefinition,
        sample_rate: u32,
    ) -> Result<Self, WavetableCompileError> {
        let frame_length = usize::try_from(definition.frame_length)
            .map_err(|_| WavetableCompileError::TooLarge)?;
        let frame_count = usize::from(definition.frames);
        if !frame_length.is_power_of_two()
            || !(crate::validate::MIN_WAVETABLE_FRAME_LENGTH
                ..=crate::validate::MAX_WAVETABLE_FRAME_LENGTH)
                .contains(&definition.frame_length)
            || !(1..=crate::validate::MAX_WAVETABLE_FRAMES).contains(&definition.frames)
            || (frame_length as u64).saturating_mul(frame_count as u64)
                > crate::validate::MAX_WAVETABLE_SAMPLES
        {
            return Err(WavetableCompileError::InvalidDefinition(
                declaration.id.clone(),
            ));
        }
        let max_harmonics = harmonic_levels(frame_length);
        let samples = frame_length
            .checked_mul(frame_count)
            .and_then(|value| value.checked_mul(max_harmonics.len()))
            .ok_or(WavetableCompileError::TooLarge)?;
        let estimated_storage_bytes = (samples as u64)
            .saturating_mul(std::mem::size_of::<f32>() as u64)
            .saturating_add(
                (max_harmonics.len() as u64).saturating_mul(std::mem::size_of::<u32>() as u64),
            );
        Ok(Self {
            asset_id: declaration.id.clone(),
            source_sha256: declaration.sha256.clone(),
            cache_identity: cache_identity(declaration, definition, sample_rate),
            sample_rate,
            frame_length,
            frame_count,
            normalization: definition.normalization,
            morph: definition.morph,
            max_harmonics: Arc::from(max_harmonics),
            samples: Arc::from([]),
            estimated_storage_bytes,
        })
    }

    pub(crate) fn sample(
        &self,
        frame_position: f64,
        phase: f64,
        frequency: f64,
        sample_rate: f64,
    ) -> f64 {
        if self.samples.is_empty() || self.frame_count == 0 {
            return 0.0;
        }
        let allowed = (sample_rate * 0.5 / frequency.max(1.0)).floor().max(1.0) as u32;
        let level = self
            .max_harmonics
            .iter()
            .position(|maximum| *maximum <= allowed)
            .unwrap_or(self.max_harmonics.len() - 1);
        let frame_position = frame_position.clamp(0.0, 1.0) * (self.frame_count - 1) as f64;
        let first_frame = frame_position.floor() as usize;
        let second_frame = (first_frame + 1).min(self.frame_count - 1);
        let fraction = frame_position - first_frame as f64;
        let first = self.frame_sample(level, first_frame, phase);
        let second = self.frame_sample(level, second_frame, phase);
        match self.morph {
            WavetableMorph::Linear => first + (second - first) * fraction,
            WavetableMorph::CatmullRom => {
                let before = self.frame_sample(level, first_frame.saturating_sub(1), phase);
                let after =
                    self.frame_sample(level, (second_frame + 1).min(self.frame_count - 1), phase);
                catmull_rom(before, first, second, after, fraction)
            }
        }
    }

    fn frame_sample(&self, level: usize, frame: usize, phase: f64) -> f64 {
        let position = phase.rem_euclid(1.0) * self.frame_length as f64;
        let first = position.floor() as usize % self.frame_length;
        let second = (first + 1) % self.frame_length;
        let fraction = position - position.floor();
        let offset = (level * self.frame_count + frame) * self.frame_length;
        let first = f64::from(self.samples[offset + first]);
        let second = f64::from(self.samples[offset + second]);
        first + (second - first) * fraction
    }

    pub(crate) fn cache_identity(&self) -> [u8; 32] {
        self.cache_identity
    }

    pub(crate) fn allocation_bytes(&self) -> u64 {
        // One Arc allocation owns this object, and two Arc slice allocations own the
        // harmonic index and sample payloads. Count both atomic reference words for
        // every allocation in addition to the payload and inline object storage.
        let arc_headers = (3 * 2 * std::mem::size_of::<usize>()) as u64;
        self.estimated_storage_bytes
            .saturating_add(std::mem::size_of::<Self>() as u64)
            .saturating_add(arc_headers)
            .saturating_add(self.asset_id.capacity() as u64)
            .saturating_add(self.source_sha256.capacity() as u64)
    }

    fn inspection(&self) -> WavetableInspection {
        WavetableInspection {
            asset_id: self.asset_id.clone(),
            source_sha256: self.source_sha256.clone(),
            cache_identity: self
                .cache_identity
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            algorithm: WAVETABLE_ALGORITHM,
            sample_rate: self.sample_rate,
            frame_length: self.frame_length,
            frames: self.frame_count,
            normalization: self.normalization,
            morph: self.morph,
            max_harmonics: self.max_harmonics.to_vec(),
            compiled_bytes: self.estimated_storage_bytes,
        }
    }

    #[cfg(test)]
    pub(crate) fn level_count(&self) -> usize {
        self.max_harmonics.len()
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum WavetableCompileError {
    #[error("wavetable asset `{0}` is not loaded")]
    MissingAsset(String),
    #[error("wavetable asset `{0}` must be mono")]
    StereoAsset(String),
    #[error("wavetable asset `{asset_id}` requires {expected} samples but contains {actual}")]
    FrameCount {
        asset_id: String,
        expected: usize,
        actual: usize,
    },
    #[error("wavetable asset `{0}` has an invalid structural definition")]
    InvalidDefinition(String),
    #[error("wavetable asset `{0}` contains a non-finite sample")]
    NonFinite(String),
    #[error("compiled wavetable exceeds addressable storage")]
    TooLarge,
    #[error(transparent)]
    Sample(#[from] SampleCompileError),
}

fn catmull_rom(before: f64, first: f64, second: f64, after: f64, fraction: f64) -> f64 {
    let fraction_squared = fraction * fraction;
    let fraction_cubed = fraction_squared * fraction;
    0.5 * (2.0 * first
        + (-before + second) * fraction
        + (2.0 * before - 5.0 * first + 4.0 * second - after) * fraction_squared
        + (-before + 3.0 * first - 3.0 * second + after) * fraction_cubed)
}

fn harmonic_levels(frame_length: usize) -> Vec<u32> {
    let mut maximum = (frame_length / 2) as u32;
    let mut levels = Vec::with_capacity(frame_length.trailing_zeros() as usize);
    loop {
        levels.push(maximum.max(1));
        if maximum <= 1 {
            break;
        }
        maximum /= 2;
    }
    levels
}

fn cache_identity(
    declaration: &MediaAsset,
    definition: &WavetableAssetDefinition,
    sample_rate: u32,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(WAVETABLE_ALGORITHM.as_bytes());
    hasher.update(declaration.sha256.as_bytes());
    hasher.update(sample_rate.to_le_bytes());
    hasher.update(definition.frame_length.to_le_bytes());
    hasher.update(definition.frames.to_le_bytes());
    hasher.update([definition.normalization as u8, definition.morph as u8]);
    hasher.finalize().into()
}

#[derive(Debug, Clone, Copy, Default)]
struct Complex {
    re: f64,
    im: f64,
}

fn fft(values: &mut [Complex], inverse: bool) {
    let length = values.len();
    debug_assert!(length.is_power_of_two());
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
        let angle = if inverse {
            std::f64::consts::TAU / width as f64
        } else {
            -std::f64::consts::TAU / width as f64
        };
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
    if inverse {
        for value in values {
            value.re /= length as f64;
            value.im /= length as f64;
        }
    }
}

const fn multiply(left: Complex, right: Complex) -> Complex {
    Complex {
        re: left.re * right.re - left.im * right.im,
        im: left.re * right.im + left.im * right.re,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{diagnostic::SourceLocation, model::ValueSite};
    use std::{fs, path::PathBuf};

    fn site(value: &str) -> ValueSite {
        ValueSite::new("table.crump", 0, value.len(), value)
    }

    fn declaration() -> (MediaAsset, WavetableAssetDefinition) {
        let definition = WavetableAssetDefinition {
            frame_length: 64,
            frames: 2,
            normalization: WavetableNormalization::Preserve,
            morph: crate::model::WavetableMorph::Linear,
            frame_length_site: site("64"),
            frames_site: site("2"),
            normalization_site: site("preserve"),
            morph_site: site("linear"),
        };
        let asset = MediaAsset {
            id: "table".to_owned(),
            name: "Table".to_owned(),
            path: PathBuf::from("media/table.wav"),
            sha256: "0".repeat(64),
            usages: vec![crate::model::MediaAssetUsage::Wavetable],
            wavetable: Some(definition.clone()),
            generated: None,
            path_site: site("media/table.wav"),
            sha256_site: site(&"0".repeat(64)),
            location: SourceLocation::file("table.crump"),
        };
        (asset, definition)
    }

    #[test]
    fn library_compiles_only_graph_referenced_tables() {
        let source = include_str!("../../fixtures/synth-alpha/wavetable-demo.crump")
            .replace(
                "  song {",
                r#"  asset unused-table "Unused table" wav "media/unused-table.wav" sha256 "2302bf70124b6017439eb385a62677b13284f112e968a2725d64e8f738bf3ca7" {
    usage wavetable
    frame_length 2048
    frames 4
    normalization preserve
    morph linear
  }
  song {"#,
            )
            .replace(
                "    pattern phrase 1bar {",
                r#"    instrument unused-instrument "Unused Instrument" graph {
      polyphony 1
      graph voice 2 {
        node unused-source wavetable asset unused-table
        node unused-out output
        bus unused-pitch note.pitch -> unused-source.pitch
        bus unused-audio unused-source.audio -> unused-out.audio
      }
    }
    pattern phrase 1bar {"#,
            );
        let root = tempfile::tempdir().expect("temporary Project");
        fs::create_dir(root.path().join("media")).expect("media directory");
        let bytes = include_bytes!("../../fixtures/synth-alpha/media/basic-table.wav");
        fs::write(root.path().join("media/basic-table.wav"), bytes).expect("used table");
        fs::write(root.path().join("media/unused-table.wav"), bytes).expect("unused table");
        let parsed = crate::dsl::parse_project(&root.path().join("project.crump"), &source);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("Project");
        let (inventory, diagnostics) =
            crate::media::load_asset_inventory(root.path(), &project.assets);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");

        let library = CompiledWavetableLibrary::compile(&project, &inventory, 48_000)
            .expect("compile referenced tables");
        assert!(library.get("wt-basic").is_some());
        assert!(library.get("unused-table").is_none());
        assert_eq!(library.inspection().len(), 1);
        assert_eq!(
            referenced_wavetable_asset_ids(&project),
            BTreeSet::from(["wt-basic"])
        );
    }

    #[test]
    fn normalization_policies_and_morph_laws_are_exact_and_structural() {
        let (asset, mut definition) = declaration();
        let source = (0..128)
            .map(|index| {
                let amplitude = if index < 64 { 0.5 } else { 0.25 };
                amplitude * (std::f64::consts::TAU * (index % 64) as f64 / 64.0).sin()
            })
            .collect::<Vec<_>>();

        definition.normalization = WavetableNormalization::GlobalPeak;
        let global = CompiledWavetable::compile_samples(&asset, &definition, &source, 48_000)
            .expect("compile global normalization");
        let first = global.sample(0.0, 0.25, 100.0, 48_000.0);
        let second = global.sample(1.0, 0.25, 100.0, 48_000.0);
        assert!((first - 1.0).abs() < 1.0e-6);
        assert!((second - 0.5).abs() < 1.0e-6);

        definition.normalization = WavetableNormalization::FramePeak;
        let per_frame = CompiledWavetable::compile_samples(&asset, &definition, &source, 48_000)
            .expect("compile per-frame normalization");
        assert!((per_frame.sample(0.0, 0.25, 100.0, 48_000.0) - 1.0).abs() < 1.0e-6);
        assert!((per_frame.sample(1.0, 0.25, 100.0, 48_000.0) - 1.0).abs() < 1.0e-6);

        definition.frames = 4;
        definition.normalization = WavetableNormalization::Preserve;
        let constant_frames = [0.0, 1.0, 0.0, 0.0]
            .into_iter()
            .flat_map(|value| std::iter::repeat_n(value, 64))
            .collect::<Vec<_>>();
        definition.morph = WavetableMorph::Linear;
        let linear =
            CompiledWavetable::compile_samples(&asset, &definition, &constant_frames, 48_000)
                .expect("compile linear morph");
        definition.morph = WavetableMorph::CatmullRom;
        let catmull_rom =
            CompiledWavetable::compile_samples(&asset, &definition, &constant_frames, 48_000)
                .expect("compile Catmull-Rom morph");
        assert!((linear.sample(0.5, 0.0, 100.0, 48_000.0) - 0.5).abs() < 1.0e-6);
        assert!((catmull_rom.sample(0.5, 0.0, 100.0, 48_000.0) - 0.5625).abs() < 1.0e-6);
        assert_ne!(linear.cache_identity(), catmull_rom.cache_identity());
    }

    #[test]
    fn every_compiled_mip_removes_harmonics_above_its_declared_alias_bound() {
        let (asset, mut definition) = declaration();
        definition.frames = 1;
        let source = (0..64)
            .map(|index| if index < 32 { 0.8 } else { -0.8 })
            .collect::<Vec<_>>();
        let table = CompiledWavetable::compile_samples(&asset, &definition, &source, 48_000)
            .expect("compile square table");
        for (level, maximum) in table.max_harmonics.iter().copied().enumerate() {
            let offset = level * table.frame_length;
            let mut spectrum = table.samples[offset..offset + table.frame_length]
                .iter()
                .map(|sample| Complex {
                    re: f64::from(*sample),
                    im: 0.0,
                })
                .collect::<Vec<_>>();
            fft(&mut spectrum, false);
            for (bin, value) in spectrum.iter().enumerate() {
                let harmonic = bin.min(table.frame_length - bin) as u32;
                if harmonic > maximum {
                    let magnitude = value.re.hypot(value.im);
                    assert!(
                        magnitude < 1.0e-4,
                        "level {level} (max {maximum}) retained harmonic {harmonic}: {magnitude}"
                    );
                }
            }
        }
    }

    #[test]
    fn radix_two_round_trip_and_mip_selection_are_deterministic() {
        let mut values = (0..64)
            .map(|index| Complex {
                re: (std::f64::consts::TAU * 7.0 * index as f64 / 64.0).sin(),
                im: 0.0,
            })
            .collect::<Vec<_>>();
        let original = values.clone();
        fft(&mut values, false);
        fft(&mut values, true);
        for (actual, expected) in values.iter().zip(original) {
            assert!((actual.re - expected.re).abs() < 1.0e-10);
            assert!(actual.im.abs() < 1.0e-10);
        }

        let (asset, definition) = declaration();
        let source = (0..128)
            .map(|index| {
                let local = (index % 64) as f64;
                let harmonic = if index < 64 { 15.0 } else { 3.0 };
                (std::f64::consts::TAU * harmonic * local / 64.0).sin()
            })
            .collect::<Vec<_>>();
        let first = CompiledWavetable::compile_samples(&asset, &definition, &source, 48_000)
            .expect("compile table");
        let second = CompiledWavetable::compile_samples(&asset, &definition, &source, 48_000)
            .expect("compile table");
        assert_eq!(first.cache_identity(), second.cache_identity());
        assert_eq!(first.samples.as_ref(), second.samples.as_ref());
        assert_eq!(first.level_count(), 6);
        assert!(first.sample(0.5, 0.125, 440.0, 48_000.0).is_finite());
    }
}
