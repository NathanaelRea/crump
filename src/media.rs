//! Portable media identity, metadata, and exact-byte Project inventory.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use rustix::fs::{Mode, OFlags, open, openat};
use sha2::{Digest, Sha256};

use crate::{
    diagnostic::{Diagnostic, Diagnostics},
    model::{
        ChannelRef, FormantPolicy, GeneratedAssetProvenance, MediaAsset, SampleTransformQuality,
        TapPoint,
    },
};

/// Maximum number of authored media assets in one Project.
pub const MAX_MEDIA_ASSETS: usize = 256;
/// Maximum exact bytes admitted for one media asset.
pub const MAX_MEDIA_ASSET_BYTES: u64 = 64 * 1_048_576;
/// Maximum exact bytes admitted across all media assets.
pub const MAX_PROJECT_MEDIA_BYTES: u64 = 256 * 1_048_576;

/// Immutable exact-byte inventory loaded away from the audio callback.
#[derive(Debug, Clone, Default)]
pub struct AssetInventory {
    assets: Vec<LoadedAsset>,
    total_bytes: u64,
}

impl AssetInventory {
    /// Returns the loaded asset with a durable Project-local ID.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&LoadedAsset> {
        self.assets.iter().find(|asset| asset.id == id)
    }

    /// Iterates in authored declaration order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &LoadedAsset> {
        self.assets.iter()
    }

    /// Returns whether the Project declares no loadable media.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// Exact media bytes retained by this inventory.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub(crate) fn identities(&self) -> impl Iterator<Item = (&Path, &str, u64)> {
        self.assets
            .iter()
            .map(|asset| (asset.path.as_path(), asset.sha256.as_str(), asset.byte_len))
    }
}

/// One exact portable WAV asset and its decoded-container metadata.
#[derive(Debug, Clone)]
pub struct LoadedAsset {
    id: String,
    name: String,
    path: PathBuf,
    sha256: String,
    byte_len: u64,
    bytes: Arc<[u8]>,
    metadata: WavMetadata,
}

impl LoadedAsset {
    /// Durable Project-local asset identity.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Human-facing asset name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Normalized Project-relative asset path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Lowercase SHA-256 of exact file bytes.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Exact file byte length.
    #[must_use]
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// Exact retained file bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// WAV container metadata checked during inventory loading.
    #[must_use]
    pub const fn metadata(&self) -> WavMetadata {
        self.metadata
    }
}

/// Bounded WAV container facts needed before sample compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavMetadata {
    /// One for mono or two for stereo.
    pub channels: u16,
    /// Source sample rate before deterministic offline conversion.
    pub sample_rate: u32,
    /// Source frames per channel.
    pub frames: u64,
    /// Integer or IEEE-float storage.
    pub sample_format: WavSampleFormat,
    /// Stored bits per sample.
    pub bits_per_sample: u16,
}

/// Supported WAV storage class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavSampleFormat {
    /// Signed or unsigned PCM as described by the WAV bit depth.
    Integer,
    /// IEEE floating point.
    Float,
}

pub(crate) enum AssetReadError {
    Unsafe(String),
    Unreadable(std::io::Error),
    TooLarge,
}

fn opened_file_error(error: rustix::io::Errno, path: &Path) -> AssetReadError {
    if matches!(error, rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) {
        AssetReadError::Unsafe(format!(
            "path `{}` traverses a symbolic link or non-directory component",
            path.display()
        ))
    } else {
        AssetReadError::Unreadable(error.into())
    }
}

fn read_opened_regular_file(file: File, maximum: u64) -> Result<Vec<u8>, AssetReadError> {
    let metadata = file.metadata().map_err(AssetReadError::Unreadable)?;
    if !metadata.file_type().is_file() {
        return Err(AssetReadError::Unsafe(
            "does not identify a regular file".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(AssetReadError::Unreadable)?;
    if bytes.len() as u64 > maximum {
        return Err(AssetReadError::TooLarge);
    }
    Ok(bytes)
}

/// Opens a Project-owned asset beneath a held root directory descriptor.
///
/// Every component is opened with `O_NOFOLLOW`, so renaming a checked path to
/// a symlink cannot redirect the subsequent read. The read itself is bounded.
pub(crate) fn read_project_asset_bytes(
    root: &Path,
    relative: &Path,
) -> Result<Vec<u8>, AssetReadError> {
    if !valid_asset_path(relative) {
        return Err(AssetReadError::Unsafe(format!(
            "path `{}` is not normalized",
            relative.display()
        )));
    }
    let mut directory = File::open(root).map_err(AssetReadError::Unreadable)?;
    let component_count = relative.components().count();
    for (index, component) in relative.components().enumerate() {
        let Component::Normal(component) = component else {
            return Err(AssetReadError::Unsafe(
                "has a non-normalized path component".to_owned(),
            ));
        };
        let final_component = index + 1 == component_count;
        let mut flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
        if !final_component {
            flags |= OFlags::DIRECTORY;
        }
        let descriptor = openat(&directory, component, flags, Mode::empty())
            .map_err(|error| opened_file_error(error, relative))?;
        let opened = File::from(descriptor);
        if final_component {
            return read_opened_regular_file(opened, MAX_MEDIA_ASSET_BYTES);
        }
        directory = opened;
    }
    Err(AssetReadError::Unsafe("has an empty path".to_owned()))
}

/// Opens one external import input without following a final symbolic link and
/// reads at most the admitted per-asset byte ceiling plus one sentinel byte.
pub(crate) fn read_regular_asset_bytes(path: &Path) -> Result<Vec<u8>, AssetReadError> {
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| opened_file_error(error, path))?;
    read_opened_regular_file(File::from(descriptor), MAX_MEDIA_ASSET_BYTES)
}

/// Loads and verifies every declared asset from a canonical Project root.
///
/// Unsafe, missing, changed, duplicate-ID/path, unsupported-container, and size
/// conditions are returned as stable diagnostics. Equal content under distinct
/// paths is intentionally a warning rather than a publication blocker.
#[must_use]
pub fn load_asset_inventory(
    root: &Path,
    declarations: &[MediaAsset],
) -> (AssetInventory, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    if declarations.len() > MAX_MEDIA_ASSETS {
        diagnostics.push(Diagnostic::error(
            "CRM007",
            format!(
                "Project declares {} media assets; maximum is {MAX_MEDIA_ASSETS}",
                declarations.len()
            ),
        ));
    }

    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut content_paths: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut assets = Vec::with_capacity(declarations.len().min(MAX_MEDIA_ASSETS));
    let mut total_bytes = 0_u64;

    for declaration in declarations.iter().take(MAX_MEDIA_ASSETS) {
        if !ids.insert(declaration.id.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    "CRM004",
                    format!("asset ID `{}` is declared more than once", declaration.id),
                )
                .at(declaration.location.clone()),
            );
            continue;
        }
        if !valid_asset_path(&declaration.path) {
            diagnostics.push(
                Diagnostic::error(
                    "CRM001",
                    format!(
                        "asset `{}` path `{}` is not a normalized Project-relative WAV path",
                        declaration.id,
                        declaration.path.display()
                    ),
                )
                .at(declaration.location.clone())
                .with_help("import external files into the Project media/ directory"),
            );
            continue;
        }
        if !paths.insert(declaration.path.clone()) {
            diagnostics.push(
                Diagnostic::error(
                    "CRM004",
                    format!(
                        "asset `{}` repeats Project-relative path `{}`",
                        declaration.id,
                        declaration.path.display()
                    ),
                )
                .at(declaration.location.clone()),
            );
            continue;
        }

        let bytes = match read_project_asset_bytes(root, &declaration.path) {
            Ok(bytes) => bytes,
            Err(AssetReadError::Unsafe(message)) => {
                diagnostics.push(
                    Diagnostic::error("CRM001", format!("asset `{}` {message}", declaration.id))
                        .at(declaration.location.clone()),
                );
                continue;
            }
            Err(AssetReadError::Unreadable(error)) => {
                diagnostics.push(
                    Diagnostic::error(
                        "CRM002",
                        format!(
                            "asset `{}` file `{}` cannot be read: {error}",
                            declaration.id,
                            declaration.path.display()
                        ),
                    )
                    .at(declaration.location.clone()),
                );
                continue;
            }
            Err(AssetReadError::TooLarge) => {
                diagnostics.push(
                    Diagnostic::error(
                        "CRM007",
                        format!(
                            "asset `{}` exceeds the {MAX_MEDIA_ASSET_BYTES}-byte per-asset maximum",
                            declaration.id
                        ),
                    )
                    .at(declaration.location.clone()),
                );
                continue;
            }
        };
        if let Some(asset) =
            load_asset_bytes(declaration, bytes, &mut content_paths, &mut diagnostics)
        {
            let Some(next_total_bytes) = total_bytes.checked_add(asset.byte_len) else {
                diagnostics.push(Diagnostic::error(
                    "CRM007",
                    "Project media byte count overflow",
                ));
                break;
            };
            if next_total_bytes > MAX_PROJECT_MEDIA_BYTES {
                diagnostics.push(Diagnostic::error(
                    "CRM007",
                    format!(
                        "Project media exceeds the {MAX_PROJECT_MEDIA_BYTES}-byte aggregate maximum"
                    ),
                ));
                break;
            }
            total_bytes = next_total_bytes;
            assets.push(asset);
        }
    }

    (
        AssetInventory {
            assets,
            total_bytes,
        },
        diagnostics,
    )
}

pub(crate) fn load_asset_inventory_from_bytes(
    declarations: &[MediaAsset],
    provided: &BTreeMap<PathBuf, Vec<u8>>,
) -> (AssetInventory, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    if declarations.len() > MAX_MEDIA_ASSETS {
        diagnostics.push(Diagnostic::error(
            "CRM007",
            format!(
                "Project declares {} media assets; maximum is {MAX_MEDIA_ASSETS}",
                declarations.len()
            ),
        ));
    }
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut content_paths = BTreeMap::new();
    let mut assets = Vec::with_capacity(declarations.len().min(MAX_MEDIA_ASSETS));
    let mut admitted_bytes = 0_u64;
    let mut total_bytes = 0_u64;
    for declaration in declarations.iter().take(MAX_MEDIA_ASSETS) {
        if !ids.insert(declaration.id.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    "CRM004",
                    format!("asset ID `{}` is declared more than once", declaration.id),
                )
                .at(declaration.location.clone()),
            );
            continue;
        }
        if !valid_asset_path(&declaration.path) {
            diagnostics.push(
                Diagnostic::error(
                    "CRM001",
                    format!(
                        "asset `{}` path `{}` is not a normalized Project-relative WAV path",
                        declaration.id,
                        declaration.path.display()
                    ),
                )
                .at(declaration.location.clone()),
            );
            continue;
        }
        if !paths.insert(declaration.path.clone()) {
            diagnostics.push(
                Diagnostic::error(
                    "CRM004",
                    format!(
                        "asset `{}` repeats Project-relative path `{}`",
                        declaration.id,
                        declaration.path.display()
                    ),
                )
                .at(declaration.location.clone()),
            );
            continue;
        }
        let Some(bytes) = provided.get(&declaration.path) else {
            diagnostics.push(
                Diagnostic::error(
                    "CRM002",
                    format!(
                        "asset `{}` snapshot bytes for `{}` are missing",
                        declaration.id,
                        declaration.path.display()
                    ),
                )
                .at(declaration.location.clone()),
            );
            continue;
        };
        let byte_len = bytes.len() as u64;
        if byte_len > MAX_MEDIA_ASSET_BYTES {
            diagnostics.push(
                Diagnostic::error(
                    "CRM007",
                    format!(
                        "asset `{}` is {byte_len} bytes; per-asset maximum is {MAX_MEDIA_ASSET_BYTES}",
                        declaration.id
                    ),
                )
                .at(declaration.location.clone()),
            );
            continue;
        }
        let Some(next_admitted_bytes) = admitted_bytes.checked_add(byte_len) else {
            diagnostics.push(Diagnostic::error(
                "CRM007",
                "Project media byte count overflow",
            ));
            break;
        };
        if next_admitted_bytes > MAX_PROJECT_MEDIA_BYTES {
            diagnostics.push(Diagnostic::error(
                "CRM007",
                format!(
                    "Project media exceeds the {MAX_PROJECT_MEDIA_BYTES}-byte aggregate maximum"
                ),
            ));
            break;
        }
        admitted_bytes = next_admitted_bytes;
        if let Some(asset) = load_asset_bytes(
            declaration,
            bytes.clone(),
            &mut content_paths,
            &mut diagnostics,
        ) {
            let Some(next_total_bytes) = total_bytes.checked_add(asset.byte_len) else {
                diagnostics.push(Diagnostic::error(
                    "CRM007",
                    "Project media byte count overflow",
                ));
                break;
            };
            if next_total_bytes > MAX_PROJECT_MEDIA_BYTES {
                diagnostics.push(Diagnostic::error(
                    "CRM007",
                    format!(
                        "Project media exceeds the {MAX_PROJECT_MEDIA_BYTES}-byte aggregate maximum"
                    ),
                ));
                break;
            }
            total_bytes = next_total_bytes;
            assets.push(asset);
        }
    }
    (
        AssetInventory {
            assets,
            total_bytes,
        },
        diagnostics,
    )
}

fn load_asset_bytes(
    declaration: &MediaAsset,
    bytes: Vec<u8>,
    content_paths: &mut BTreeMap<String, PathBuf>,
    diagnostics: &mut Diagnostics,
) -> Option<LoadedAsset> {
    if let Some(provenance) = &declaration.generated {
        let valid = match provenance {
            GeneratedAssetProvenance::Bounce {
                source_revision,
                owner,
                tap,
                start_frame,
                end_frame,
                format,
                algorithm,
                build,
                ..
            } => {
                let channel_source =
                    owner.parse::<ChannelRef>().is_ok() && tap.parse::<TapPoint>().is_ok();
                let master_source = owner == "master" && tap == "master_output";
                valid_sha256(source_revision)
                    && start_frame < end_frame
                    && format == "stereo_f32_wav"
                    && !algorithm.trim().is_empty()
                    && !build.trim().is_empty()
                    && (master_source || channel_source)
            }
            GeneratedAssetProvenance::SampleTransform {
                source_revision,
                source_asset,
                source_sha256,
                source_start_frame,
                source_end_frame,
                output_frames,
                pitch_semitones,
                formants,
                quality,
                format,
                algorithm,
                deterministic_domain,
                build,
            } => {
                valid_sha256(source_revision)
                    && generated_source_id(source_asset)
                    && valid_sha256(source_sha256)
                    && source_start_frame < source_end_frame
                    && *output_frames > 0
                    && pitch_semitones.is_finite()
                    && (-24.0..=24.0).contains(pitch_semitones)
                    && *formants == FormantPolicy::Shift
                    && *quality == SampleTransformQuality::EffectV1
                    && format == "stereo_f32_wav"
                    && !algorithm.trim().is_empty()
                    && deterministic_domain == "x86_64-linux"
                    && !build.trim().is_empty()
            }
        };
        if !valid {
            diagnostics.push(
                Diagnostic::error(
                    "CRM008",
                    format!(
                        "asset `{}` has invalid generated-audio provenance",
                        declaration.id
                    ),
                )
                .at(declaration.location.clone()),
            );
            return None;
        }
    }
    let byte_len = bytes.len() as u64;
    if byte_len > MAX_MEDIA_ASSET_BYTES {
        diagnostics.push(
            Diagnostic::error(
                "CRM007",
                format!(
                    "asset `{}` is {byte_len} bytes; per-asset maximum is {MAX_MEDIA_ASSET_BYTES}",
                    declaration.id
                ),
            )
            .at(declaration.location.clone()),
        );
        return None;
    }
    let actual_hash = sha256(&bytes);
    if valid_sha256(&declaration.sha256) && actual_hash != declaration.sha256 {
        diagnostics.push(
            Diagnostic::error(
                "CRM003",
                format!(
                    "asset `{}` changed: declared SHA-256 {}, exact file bytes are {}",
                    declaration.id, declaration.sha256, actual_hash
                ),
            )
            .at(declaration.location.clone())
            .with_help("re-import the changed file to acknowledge its new exact-byte identity"),
        );
    }
    let wav_metadata = match inspect_wav(&bytes) {
        Ok(metadata) => metadata,
        Err(message) => {
            diagnostics.push(
                Diagnostic::error(
                    "CRM006",
                    format!(
                        "asset `{}` is not a supported WAV: {message}",
                        declaration.id
                    ),
                )
                .at(declaration.location.clone()),
            );
            return None;
        }
    };
    if let Some(provenance) = &declaration.generated
        && (provenance.expected_output_frames() != Some(wav_metadata.frames)
            || wav_metadata.channels != 2
            || wav_metadata.sample_format != WavSampleFormat::Float
            || wav_metadata.bits_per_sample != 32)
    {
        diagnostics.push(
            Diagnostic::error(
                "CRM008",
                format!(
                    "asset `{}` WAV metadata does not match its generated-audio provenance",
                    declaration.id
                ),
            )
            .at(declaration.location.clone()),
        );
        return None;
    }
    if let Some(first_path) = content_paths.get(&actual_hash) {
        diagnostics.push(
            Diagnostic::warning(
                "CRM005",
                format!(
                    "asset `{}` duplicates exact content already declared at `{}`",
                    declaration.id,
                    first_path.display()
                ),
            )
            .at(declaration.location.clone())
            .with_help("reuse one asset ID unless separate file identity is intentional"),
        );
    } else {
        content_paths.insert(actual_hash.clone(), declaration.path.clone());
    }
    Some(LoadedAsset {
        id: declaration.id.clone(),
        name: declaration.name.clone(),
        path: declaration.path.clone(),
        sha256: actual_hash,
        byte_len,
        bytes: Arc::from(bytes),
        metadata: wav_metadata,
    })
}

fn generated_source_id(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

/// Returns whether a digest is canonical lowercase SHA-256 text.
#[must_use]
pub fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Returns whether an authored asset path is already canonical and portable.
#[must_use]
pub fn valid_asset_path(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    let components = path.components().collect::<Vec<_>>();
    let normalized = components
        .iter()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    !text.is_empty()
        && !text.contains('\\')
        && !path.is_absolute()
        && normalized == text
        && components
            .iter()
            .all(|component| matches!(component, Component::Normal(_)))
        && !matches!(components.first(), Some(Component::Normal(value)) if value.to_str() == Some(".crump"))
        && path.extension().and_then(|extension| extension.to_str()) == Some("wav")
}

pub(crate) fn validate_finite_wav_samples(asset: &LoadedAsset) -> Result<(), String> {
    if asset.metadata.sample_format != WavSampleFormat::Float {
        return Ok(());
    }
    let mut reader = hound::WavReader::new(Cursor::new(asset.bytes.as_ref()))
        .map_err(|error| error.to_string())?;
    for (index, sample) in reader.samples::<f32>().enumerate() {
        let sample = sample.map_err(|error| error.to_string())?;
        if !sample.is_finite() {
            return Err(format!(
                "non-finite float sample at interleaved index {index}"
            ));
        }
    }
    Ok(())
}

fn inspect_wav(bytes: &[u8]) -> Result<WavMetadata, String> {
    let reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let specification = reader.spec();
    if !(1..=2).contains(&specification.channels) {
        return Err(format!(
            "channel count {} is outside the supported mono/stereo domain",
            specification.channels
        ));
    }
    if specification.sample_rate == 0 {
        return Err("sample rate must be positive".to_owned());
    }
    let sample_format = match specification.sample_format {
        hound::SampleFormat::Int if matches!(specification.bits_per_sample, 8 | 16 | 24 | 32) => {
            WavSampleFormat::Integer
        }
        hound::SampleFormat::Float if specification.bits_per_sample == 32 => WavSampleFormat::Float,
        hound::SampleFormat::Int | hound::SampleFormat::Float => {
            return Err(format!(
                "{}-bit {:?} samples are unsupported",
                specification.bits_per_sample, specification.sample_format
            ));
        }
    };
    let frames = u64::from(reader.duration());
    if frames == 0 {
        return Err("audio data contains zero frames".to_owned());
    }
    Ok(WavMetadata {
        channels: specification.channels,
        sample_rate: specification.sample_rate,
        frames,
        sample_format,
        bits_per_sample: specification.bits_per_sample,
    })
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use crate::{diagnostic::SourceLocation, model::ValueSite};
    use tempfile::tempdir;

    use super::*;

    fn write_wav(path: &Path, samples: &[i16]) -> Vec<u8> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("media directory");
        }
        let specification = hound::WavSpec {
            channels: 1,
            sample_rate: 24_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, specification).expect("WAV writer");
        for sample in samples {
            writer.write_sample(*sample).expect("WAV sample");
        }
        writer.finalize().expect("WAV finalize");
        fs::read(path).expect("WAV bytes")
    }

    fn declaration(id: &str, path: &str, digest: String) -> MediaAsset {
        MediaAsset {
            id: id.to_owned(),
            name: id.to_owned(),
            path: PathBuf::from(path),
            sha256: digest,
            usages: Vec::new(),
            wavetable: None,
            generated: None,
            path_site: ValueSite::new("project.crump", 0, 0, ""),
            sha256_site: ValueSite::new("project.crump", 0, 0, ""),
            location: SourceLocation::file("project.crump"),
        }
    }

    #[test]
    fn portable_asset_paths_reject_external_or_ambiguous_forms() {
        assert!(valid_asset_path(Path::new("media/voice.wav")));
        assert!(!valid_asset_path(Path::new("/tmp/voice.wav")));
        assert!(!valid_asset_path(Path::new("../voice.wav")));
        assert!(!valid_asset_path(Path::new("media\\voice.wav")));
        assert!(!valid_asset_path(Path::new("media//voice.wav")));
        assert!(!valid_asset_path(Path::new(".crump/voice.wav")));
        assert!(!valid_asset_path(Path::new("media/voice.WAV")));
        assert!(!valid_asset_path(Path::new("media/voice.aiff")));
        assert!(valid_sha256(&"a".repeat(64)));
        assert!(!valid_sha256(&"A".repeat(64)));
        assert!(!valid_sha256("abc"));
    }

    #[test]
    fn inventory_verifies_exact_bytes_metadata_and_duplicate_content() {
        let root = tempdir().expect("Project root");
        let first = write_wav(&root.path().join("media/one.wav"), &[0, 1, -1, 2]);
        fs::copy(
            root.path().join("media/one.wav"),
            root.path().join("media/two.wav"),
        )
        .expect("duplicate content");
        let digest = sha256(&first);
        let declarations = [
            declaration("one", "media/one.wav", digest.clone()),
            declaration("two", "media/two.wav", digest),
        ];
        let (inventory, diagnostics) = load_asset_inventory(root.path(), &declarations);
        assert!(!diagnostics.has_errors());
        assert_eq!(inventory.iter().len(), 2);
        assert_eq!(inventory.get("one").expect("asset").metadata().frames, 4);
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRM005")
        );
    }

    #[test]
    fn inventory_rejects_duplicate_ids_and_unsupported_wav_bytes() {
        let root = tempdir().expect("Project root");
        let first = write_wav(&root.path().join("media/one.wav"), &[0, 1]);
        let second = write_wav(&root.path().join("media/two.wav"), &[0, 2]);
        fs::write(root.path().join("media/invalid.wav"), b"not a wave").expect("invalid WAV");
        let declarations = [
            declaration("same", "media/one.wav", sha256(&first)),
            declaration("same", "media/two.wav", sha256(&second)),
            declaration("invalid", "media/invalid.wav", sha256(b"not a wave")),
        ];
        let (_, diagnostics) = load_asset_inventory(root.path(), &declarations);
        for code in ["CRM004", "CRM006"] {
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .any(|diagnostic| diagnostic.code == code)
            );
        }
    }

    #[test]
    fn inventory_rejects_missing_changed_duplicate_paths_and_symlinks() {
        let root = tempdir().expect("Project root");
        let bytes = write_wav(&root.path().join("media/source.wav"), &[0, 1]);
        let declarations = [
            declaration("changed", "media/source.wav", "0".repeat(64)),
            declaration("duplicate", "media/source.wav", sha256(&bytes)),
            declaration("missing", "media/missing.wav", "1".repeat(64)),
        ];
        let (_, diagnostics) = load_asset_inventory(root.path(), &declarations);
        for code in ["CRM002", "CRM003", "CRM004"] {
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .any(|diagnostic| diagnostic.code == code)
            );
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(
                root.path().join("media/source.wav"),
                root.path().join("media/link.wav"),
            )
            .expect("symlink");
            let linked = [declaration("linked", "media/link.wav", sha256(&bytes))];
            let (_, diagnostics) = load_asset_inventory(root.path(), &linked);
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .any(|diagnostic| diagnostic.code == "CRM001")
            );

            let outside = tempdir().expect("outside directory");
            fs::write(outside.path().join("source.wav"), &bytes).expect("outside WAV");
            symlink(outside.path(), root.path().join("linked-media")).expect("parent symlink");
            let through_parent = [declaration(
                "parent-link",
                "linked-media/source.wav",
                sha256(&bytes),
            )];
            let (_, diagnostics) = load_asset_inventory(root.path(), &through_parent);
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .any(|diagnostic| diagnostic.code == "CRM001")
            );
        }
    }
}
