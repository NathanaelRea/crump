//! Deterministic stereo float-WAV rendering with failure-atomic bundle publication.

use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use rustix::fs::{renameat_with, RenameFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::project::{ProjectBundle, RevisionId};

use super::{CompiledProject, EngineError, ProcessingMode};

const RENDER_SCHEMA: u32 = 1;
const WAV_FILE: &str = "master.wav";
const MANIFEST_FILE: &str = "manifest.json";
static RENDER_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Offline render publication settings.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Destination bundle directory containing `master.wav` and `manifest.json`.
    pub output: PathBuf,
    /// Atomically replace an existing complete bundle.
    pub replace: bool,
}

impl RenderOptions {
    /// Creates options that atomically replace an earlier bundle.
    #[must_use]
    pub fn new(output: impl Into<PathBuf>) -> Self {
        Self {
            output: output.into(),
            replace: true,
        }
    }
}

/// Deterministic machine-readable render manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Stable Project ID.
    pub project_id: String,
    /// Exact Active Revision rendered.
    pub project_revision: RevisionId,
    /// Crump package version.
    pub crump_version: String,
    /// SHA-256 of the exact running executable.
    pub executable_sha256: String,
    /// Compilation target OS.
    pub target_os: String,
    /// Compilation target architecture.
    pub target_arch: String,
    /// Distribution identity from `/etc/os-release`, when available.
    pub host_os_release: Option<String>,
    /// Running kernel release, when available.
    pub host_kernel_release: Option<String>,
    /// True only on the promised Ubuntu 26.04 Linux amd64 comparison environment.
    pub reference_environment: bool,
    /// PCM sample rate.
    pub sample_rate: u32,
    /// Always two for the First Public Release.
    pub channels: u16,
    /// Always IEEE stereo 32-bit float WAV.
    pub sample_format: String,
    /// Frames from Song zero through arrangement end.
    pub arrangement_frames: u64,
    /// Explicit authored Render Tail frames.
    pub render_tail_frames: u64,
    /// Complete output frame count.
    pub frame_count: u64,
    /// Complete Render Range in seconds.
    pub duration_seconds: f64,
    /// Maximum absolute authored output sample; no monitor limiter is applied.
    pub peak: f32,
    /// Hash of exact WAV bytes.
    pub wav_sha256: String,
}

/// Published render artifact paths and manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderResult {
    /// Complete bundle directory.
    pub output: PathBuf,
    /// Stereo float WAV path.
    pub wav: PathBuf,
    /// Deterministic manifest path.
    pub manifest_path: PathBuf,
    /// Parsed manifest.
    pub manifest: RenderManifest,
    /// Hash of exact manifest bytes.
    pub manifest_sha256: String,
}

/// Renders a valid Active Revision into an atomic artifact bundle.
pub fn render_project(
    bundle: &ProjectBundle,
    options: &RenderOptions,
) -> Result<RenderResult, EngineError> {
    let project = bundle
        .publishable()
        .ok_or_else(|| EngineError::Validation(bundle.diagnostics.as_slice().to_vec()))?;
    let compiled = CompiledProject::compile(project)?;
    let destination = resolved_destination(&options.output)?;
    validate_render_destination(bundle, &destination)?;
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "render output has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "render output {} exists and is not a real directory",
                        destination.display()
                    ),
                )
                .into());
            }
            if !options.replace {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("render bundle {} already exists", destination.display()),
                )
                .into());
            }
            validate_existing_render_bundle(&destination)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let staging = staging_path(&destination)?;
    fs::create_dir(&staging)?;
    let result = render_staging(bundle, &compiled, &staging).and_then(|manifest| {
        publish_directory(&staging, &destination, options.replace)?;
        let manifest_bytes = fs::read(destination.join(MANIFEST_FILE))?;
        Ok(RenderResult {
            output: destination.clone(),
            wav: destination.join(WAV_FILE),
            manifest_path: destination.join(MANIFEST_FILE),
            manifest,
            manifest_sha256: hash_bytes(&manifest_bytes),
        })
    });
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn render_staging(
    bundle: &ProjectBundle,
    compiled: &CompiledProject,
    staging: &Path,
) -> Result<RenderManifest, EngineError> {
    let wav_path = staging.join(WAV_FILE);
    let specification = hound::WavSpec {
        channels: 2,
        sample_rate: compiled.sample_rate(),
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&wav_path, specification)?;
    let mut runtime = compiled.runtime();
    let mut block = vec![[0.0_f32, 0.0_f32]; 1_024];
    while !runtime.finished() {
        let count = runtime.process_block(&mut block, ProcessingMode::Offline)?;
        for frame in &block[..count] {
            writer.write_sample(frame[0])?;
            writer.write_sample(frame[1])?;
        }
    }
    writer.finalize()?;
    File::open(&wav_path)?.sync_all()?;
    let wav_sha256 = hash_file(&wav_path)?;
    let executable_path = std::env::current_exe()?;
    let executable_sha256 = hash_file(&executable_path)?;
    let frame_count = compiled.total_frames();
    let arrangement_frames = compiled.arrangement_frames();
    let host_os_release = host_os_release();
    let host_kernel_release = fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|release| release.trim().to_owned())
        .filter(|release| !release.is_empty());
    let reference_environment = std::env::consts::OS == "linux"
        && std::env::consts::ARCH == "x86_64"
        && host_os_release.as_deref() == Some("ubuntu/26.04");
    let project_id = bundle
        .project
        .as_ref()
        .ok_or_else(|| EngineError::Compile("render bundle has no parsed Project".to_owned()))?
        .id
        .clone();
    let manifest = RenderManifest {
        schema_version: RENDER_SCHEMA,
        project_id,
        project_revision: bundle.revision.clone(),
        crump_version: env!("CARGO_PKG_VERSION").to_owned(),
        executable_sha256,
        target_os: std::env::consts::OS.to_owned(),
        target_arch: std::env::consts::ARCH.to_owned(),
        host_os_release,
        host_kernel_release,
        reference_environment,
        sample_rate: compiled.sample_rate(),
        channels: 2,
        sample_format: "ieee_float_32_le".to_owned(),
        arrangement_frames,
        render_tail_frames: frame_count - arrangement_frames,
        frame_count,
        duration_seconds: frame_count as f64 / f64::from(compiled.sample_rate()),
        peak: runtime.telemetry().peak,
        wav_sha256,
    };
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    let manifest_path = staging.join(MANIFEST_FILE);
    let mut manifest_file = File::create(&manifest_path)?;
    manifest_file.write_all(&manifest_bytes)?;
    manifest_file.sync_all()?;
    File::open(staging)?.sync_all()?;
    Ok(manifest)
}

fn publish_directory(staging: &Path, destination: &Path, replace: bool) -> Result<(), EngineError> {
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "render destination has no parent",
        )
    })?;
    let directory = File::open(parent)?;
    let staging_name = staging.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "staging path has no name")
    })?;
    let destination_name = destination.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "output path has no name")
    })?;
    if destination.exists() {
        if !replace {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "render bundle exists",
            )
            .into());
        }
        validate_existing_render_bundle(destination)?;
        renameat_with(
            &directory,
            staging_name,
            &directory,
            destination_name,
            RenameFlags::EXCHANGE,
        )
        .map_err(std::io::Error::from)?;
        directory.sync_all()?;
        fs::remove_dir_all(staging)?;
        directory.sync_all()?;
    } else {
        renameat_with(
            &directory,
            staging_name,
            &directory,
            destination_name,
            RenameFlags::NOREPLACE,
        )
        .map_err(std::io::Error::from)?;
        directory.sync_all()?;
    }
    Ok(())
}

fn resolved_destination(output: &Path) -> Result<PathBuf, std::io::Error> {
    let absolute = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()?.join(output)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    let destination_name = normalized.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "render output requires a directory name",
        )
    })?;
    let mut cursor = normalized.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "render output has no parent",
        )
    })?;
    let mut missing = Vec::new();
    while !cursor.exists() {
        let name = cursor.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "render output has no existing ancestor",
            )
        })?;
        missing.push(name.to_os_string());
        cursor = cursor.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "render output has no existing ancestor",
            )
        })?;
    }
    let mut resolved = fs::canonicalize(cursor)?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    resolved.push(destination_name);
    Ok(resolved)
}

pub(crate) fn inspect_render_destination(
    bundle: &ProjectBundle,
    output: &Path,
) -> Result<(PathBuf, bool), EngineError> {
    let destination = resolved_destination(output)?;
    validate_render_destination(bundle, &destination)?;
    match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "render output {} exists and is not a real directory",
                        destination.display()
                    ),
                )
                .into());
            }
            validate_existing_render_bundle(&destination)?;
            Ok((destination, true))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((destination, false)),
        Err(error) => Err(error.into()),
    }
}

fn validate_render_destination(
    bundle: &ProjectBundle,
    destination: &Path,
) -> Result<(), std::io::Error> {
    let root = fs::canonicalize(&bundle.root)?;
    let metadata = root.join(".crump");
    let contains_project = root.starts_with(destination);
    let contains_source = bundle
        .sources
        .keys()
        .map(|path| root.join(path))
        .any(|path| path.starts_with(destination));
    if contains_project || contains_source || destination.starts_with(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "render destination {} overlaps Project source or history",
                destination.display()
            ),
        ));
    }
    Ok(())
}

fn validate_existing_render_bundle(destination: &Path) -> Result<(), EngineError> {
    let mut names = fs::read_dir(destination)?
        .map(|entry| {
            entry.and_then(|entry| {
                entry.file_name().into_string().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "render bundle contains a non-UTF-8 entry name",
                    )
                })
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    if names != [MANIFEST_FILE, WAV_FILE] {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "refusing to replace {} because it is not an exact complete Crump render bundle",
                destination.display()
            ),
        )
        .into());
    }
    for name in [MANIFEST_FILE, WAV_FILE] {
        let metadata = fs::symlink_metadata(destination.join(name))?;
        if !metadata.file_type().is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("render bundle artifact `{name}` is not a regular file"),
            )
            .into());
        }
    }
    let manifest_bytes = fs::read(destination.join(MANIFEST_FILE))?;
    let manifest: RenderManifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.schema_version != RENDER_SCHEMA
        || manifest.channels != 2
        || manifest.sample_format != "ieee_float_32_le"
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "existing render manifest has an unsupported format",
        )
        .into());
    }
    let wav_path = destination.join(WAV_FILE);
    if hash_file(&wav_path)? != manifest.wav_sha256 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "existing render WAV does not match its manifest hash",
        )
        .into());
    }
    let reader = hound::WavReader::open(&wav_path)?;
    let specification = reader.spec();
    if specification.channels != manifest.channels
        || specification.sample_rate != manifest.sample_rate
        || specification.bits_per_sample != 32
        || specification.sample_format != hound::SampleFormat::Float
        || u64::from(reader.duration()) != manifest.frame_count
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "existing render WAV format or frame count does not match its manifest",
        )
        .into());
    }
    Ok(())
}

fn staging_path(destination: &Path) -> Result<PathBuf, std::io::Error> {
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "render output has no parent",
        )
    })?;
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "render output name is not UTF-8",
            )
        })?;
    for _ in 0..100 {
        let counter = RENDER_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".{name}.rendering-{}-{counter}",
            std::process::id()
        ));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a render staging directory",
    ))
}

fn host_os_release() -> Option<String> {
    let source = fs::read_to_string("/etc/os-release").ok()?;
    let value = |key: &str| {
        source.lines().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name == key).then(|| value.trim().trim_matches('"').to_owned())
        })
    };
    let id = value("ID")?;
    let version = value("VERSION_ID")
        .or_else(|| value("BUILD_ID"))
        .unwrap_or_default();
    Some(format!("{id}/{version}"))
}

fn hash_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1_024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(hasher.finalize().as_slice()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    hex_digest(Sha256::digest(bytes).as_slice())
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::project::load_project;

    use super::*;

    #[test]
    fn replacement_is_complete_and_no_replace_preserves_the_old_bundle() {
        let directory = tempdir().unwrap();
        let source = include_str!("../../tests/fixtures/minimal.crump");
        let source_path = directory.path().join("project.crump");
        fs::write(&source_path, source).unwrap();
        let bundle = load_project(directory.path()).unwrap();
        let output = directory.path().join("master");
        let first = render_project(&bundle, &RenderOptions::new(&output)).unwrap();
        let first_wav = fs::read(&first.wav).unwrap();
        let first_manifest = fs::read(&first.manifest_path).unwrap();

        let mut no_replace = RenderOptions::new(&output);
        no_replace.replace = false;
        assert!(render_project(&bundle, &no_replace).is_err());
        assert_eq!(fs::read(&first.wav).unwrap(), first_wav);
        assert_eq!(fs::read(&first.manifest_path).unwrap(), first_manifest);

        fs::write(&source_path, source.replacen("C4", "D4", 1)).unwrap();
        let changed = load_project(directory.path()).unwrap();
        let replaced = render_project(&changed, &RenderOptions::new(&output)).unwrap();
        assert_ne!(
            replaced.manifest.project_revision,
            first.manifest.project_revision
        );
        assert_ne!(fs::read(replaced.wav).unwrap(), first_wav);
        assert!(fs::read_dir(directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().contains(".rendering-")));
    }

    #[test]
    fn render_refuses_to_replace_source_history_or_arbitrary_directories() {
        let directory = tempdir().unwrap();
        let source = include_str!("../../tests/fixtures/minimal.crump");
        let source_path = directory.path().join("project.crump");
        fs::write(&source_path, source).unwrap();
        let bundle = load_project(directory.path()).unwrap();

        let root_error =
            render_project(&bundle, &RenderOptions::new(directory.path())).unwrap_err();
        assert!(root_error.to_string().contains("overlaps Project source"));
        assert_eq!(fs::read_to_string(&source_path).unwrap(), source);

        let metadata = directory.path().join(".crump");
        fs::create_dir(&metadata).unwrap();
        fs::write(metadata.join("keep"), b"history").unwrap();
        let metadata_error = render_project(&bundle, &RenderOptions::new(&metadata)).unwrap_err();
        assert!(metadata_error
            .to_string()
            .contains("overlaps Project source"));
        assert_eq!(fs::read(metadata.join("keep")).unwrap(), b"history");

        let arbitrary = directory.path().join("documents");
        fs::create_dir(&arbitrary).unwrap();
        fs::write(arbitrary.join("keep"), b"user data").unwrap();
        let arbitrary_error = render_project(&bundle, &RenderOptions::new(&arbitrary)).unwrap_err();
        assert!(arbitrary_error.to_string().contains("refusing to replace"));
        assert_eq!(fs::read(arbitrary.join("keep")).unwrap(), b"user data");
    }

    #[test]
    fn render_refuses_to_replace_a_tampered_bundle() {
        let directory = tempdir().unwrap();
        let source = include_str!("../../tests/fixtures/minimal.crump");
        fs::write(directory.path().join("project.crump"), source).unwrap();
        let bundle = load_project(directory.path()).unwrap();
        let output = directory.path().join("master");
        let rendered = render_project(&bundle, &RenderOptions::new(&output)).unwrap();
        let mut tampered = fs::read(&rendered.wav).unwrap();
        tampered.push(0);
        fs::write(&rendered.wav, &tampered).unwrap();

        let error = render_project(&bundle, &RenderOptions::new(&output)).unwrap_err();
        assert!(error
            .to_string()
            .contains("does not match its manifest hash"));
        assert_eq!(fs::read(&rendered.wav).unwrap(), tampered);
    }

    #[test]
    fn unchanged_renders_are_byte_identical() {
        let directory = tempdir().unwrap();
        let source = include_str!("../../tests/fixtures/minimal.crump");
        fs::write(directory.path().join("project.crump"), source).unwrap();
        let bundle = load_project(directory.path()).unwrap();
        assert!(!bundle.diagnostics.has_errors());
        let first =
            render_project(&bundle, &RenderOptions::new(directory.path().join("first"))).unwrap();
        let second = render_project(
            &bundle,
            &RenderOptions::new(directory.path().join("second")),
        )
        .unwrap();
        assert_eq!(fs::read(first.wav).unwrap(), fs::read(second.wav).unwrap());
        assert_eq!(
            fs::read(first.manifest_path).unwrap(),
            fs::read(second.manifest_path).unwrap()
        );
    }
}
