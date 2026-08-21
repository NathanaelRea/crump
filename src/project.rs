//! Deep Project-loading module: imports, diagnostics, validation, and revision identity.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    diagnostic::{Diagnostic, Diagnostics, SourceLocation},
    dsl::{ImportDeclaration, parse_module, parse_project},
    media::{AssetInventory, load_asset_inventory, load_asset_inventory_from_bytes},
    model::Project,
    validate::{validate_project, validate_project_assets},
};

/// Conventional root source filename.
pub const PROJECT_FILE: &str = "project.crump";
/// Per-file source size ceiling.
pub const MAX_SOURCE_BYTES: u64 = 1_048_576;
/// Whole-Project source size ceiling.
pub const MAX_PROJECT_SOURCE_BYTES: usize = 4 * 1_048_576;

/// A complete Candidate Revision loaded from readable source.
#[derive(Debug)]
pub struct ProjectBundle {
    /// Directory containing `project.crump`.
    pub root: PathBuf,
    /// Recovered semantic Project, absent after unrecoverable parse errors.
    pub project: Option<Project>,
    /// Every authoritative source file keyed by its stable display path.
    pub sources: BTreeMap<PathBuf, String>,
    /// Exact-byte portable media retained with this Candidate.
    pub assets: AssetInventory,
    /// SHA-256 identity of source paths/bytes and declared media identity.
    pub revision: RevisionId,
    /// Complete parse, import, media, and semantic diagnostics.
    pub diagnostics: Diagnostics,
}

impl ProjectBundle {
    /// Returns the validated Project only when publication is legal.
    #[must_use]
    pub fn publishable(&self) -> Option<&Project> {
        (!self.diagnostics.has_errors())
            .then_some(self.project.as_ref())
            .flatten()
    }

    /// Looks up exact authoritative source bytes by display path.
    #[must_use]
    pub fn source(&self, path: &Path) -> Option<&str> {
        self.sources.get(path).map(String::as_str)
    }
}

/// Stable content identity of a Candidate or Active Revision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct RevisionId(pub String);

impl std::fmt::Display for RevisionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Loads a complete Project candidate from a directory or root source path.
pub fn load_project(path: impl AsRef<Path>) -> Result<ProjectBundle, ProjectIoError> {
    let supplied = path.as_ref();
    let (root, root_file) = if supplied.is_dir() {
        (supplied.to_path_buf(), supplied.join(PROJECT_FILE))
    } else {
        let parent = supplied.parent().unwrap_or_else(|| Path::new("."));
        (parent.to_path_buf(), supplied.to_path_buf())
    };
    if !root_file.exists() {
        return Err(ProjectIoError::NotFound(root_file));
    }
    let root = fs::canonicalize(&root).map_err(|source| ProjectIoError::Read {
        path: root.clone(),
        source,
    })?;
    let root_file = fs::canonicalize(&root_file).map_err(|source| ProjectIoError::Read {
        path: root_file.clone(),
        source,
    })?;
    let root_source = read_source(&root_file)?;
    let root_key = PathBuf::from(PROJECT_FILE);
    let document = parse_project(&root_key, &root_source);
    let mut diagnostics = document.diagnostics;
    let mut sources = BTreeMap::new();
    sources.insert(root_key, root_source);
    let mut project = document.project;
    let mut real_imports = BTreeSet::new();
    let mut canonical_imports = Vec::new();

    for import in &document.imports {
        match resolve_import(&root, import) {
            Ok(resolved) => {
                if !real_imports.insert(resolved.real_path.clone()) {
                    diagnostics.push(
                        Diagnostic::error(
                            "CRL001",
                            format!(
                                "module `{}` resolves to a source that is already imported",
                                resolved.display_path.display()
                            ),
                        )
                        .at(import.location.clone()),
                    );
                    continue;
                }
                match read_source(&resolved.real_path) {
                    Ok(source) => {
                        let module = parse_module(&resolved.display_path, &source);
                        diagnostics.extend(module.diagnostics);
                        if let Some(project) = project.as_mut() {
                            project.song.instruments.extend(module.instruments);
                        }
                        canonical_imports.push(resolved.display_path.clone());
                        if sources
                            .insert(resolved.display_path.clone(), source)
                            .is_some()
                        {
                            diagnostics.push(
                                Diagnostic::error(
                                    "CRL001",
                                    format!(
                                        "module `{}` is imported more than once",
                                        resolved.display_path.display()
                                    ),
                                )
                                .at(import.location.clone()),
                            );
                        }
                    }
                    Err(error) => diagnostics.push(
                        Diagnostic::error("CRL002", error.to_string()).at(import.location.clone()),
                    ),
                }
            }
            Err((code, message)) => {
                diagnostics.push(Diagnostic::error(code, message).at(import.location.clone()))
            }
        }
    }
    if let Some(project) = project.as_mut() {
        project.imports = canonical_imports;
    }

    let total_bytes: usize = sources.values().map(String::len).sum();
    if total_bytes > MAX_PROJECT_SOURCE_BYTES {
        diagnostics.push(Diagnostic::error(
            "CRL004",
            format!("Project source is {total_bytes} bytes; maximum is {MAX_PROJECT_SOURCE_BYTES}"),
        ));
    }
    let assets = if let Some(candidate) = &project {
        let (assets, asset_diagnostics) = load_asset_inventory(&root, &candidate.assets);
        diagnostics.extend(asset_diagnostics);
        diagnostics.extend(validate_project(candidate));
        diagnostics.extend(validate_project_assets(candidate, &assets));
        assets
    } else {
        AssetInventory::default()
    };
    resolve_locations(&mut diagnostics, &sources);
    let revision = hash_project_contents(&sources, &assets);
    Ok(ProjectBundle {
        root,
        project,
        sources,
        assets,
        revision,
        diagnostics,
    })
}

/// Parses and validates an in-memory authoritative source set.
///
/// This is the transaction seam used by Structured Source Edits and History
/// Restore before any candidate bytes replace files on disk.
#[must_use]
pub fn load_project_sources(root: PathBuf, sources: BTreeMap<PathBuf, String>) -> ProjectBundle {
    load_project_sources_inner(root, sources, None)
}

pub(crate) fn load_project_sources_with_assets(
    root: PathBuf,
    sources: BTreeMap<PathBuf, String>,
    asset_bytes: &BTreeMap<PathBuf, Vec<u8>>,
) -> ProjectBundle {
    load_project_sources_inner(root, sources, Some(asset_bytes))
}

fn load_project_sources_inner(
    root: PathBuf,
    sources: BTreeMap<PathBuf, String>,
    asset_bytes: Option<&BTreeMap<PathBuf, Vec<u8>>>,
) -> ProjectBundle {
    let revision = hash_sources(&sources);
    let Some(root_source) = sources.get(Path::new(PROJECT_FILE)) else {
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::error(
            "CRL005",
            "source set is missing `project.crump`",
        ));
        return ProjectBundle {
            root,
            project: None,
            sources,
            assets: AssetInventory::default(),
            revision,
            diagnostics,
        };
    };
    let document = parse_project(Path::new(PROJECT_FILE), root_source);
    let mut diagnostics = document.diagnostics;
    let mut project = document.project;
    let mut imported = BTreeSet::new();
    let mut canonical_imports = Vec::new();
    for import in &document.imports {
        let display_path = match import_display_path(&import.path) {
            Ok(path) => path,
            Err(message) => {
                diagnostics.push(Diagnostic::error("CRL003", message).at(import.location.clone()));
                continue;
            }
        };
        if !imported.insert(display_path.clone()) {
            diagnostics.push(
                Diagnostic::error(
                    "CRL001",
                    format!(
                        "module `{}` is imported more than once",
                        display_path.display()
                    ),
                )
                .at(import.location.clone()),
            );
            continue;
        }
        let Some(source) = sources.get(&display_path) else {
            diagnostics.push(
                Diagnostic::error(
                    "CRL006",
                    format!("imported module `{}` is missing", display_path.display()),
                )
                .at(import.location.clone()),
            );
            continue;
        };
        canonical_imports.push(display_path.clone());
        let module = parse_module(&display_path, source);
        diagnostics.extend(module.diagnostics);
        if let Some(project) = project.as_mut() {
            project.song.instruments.extend(module.instruments);
        }
    }
    if let Some(project) = project.as_mut() {
        project.imports = canonical_imports;
    }
    let mut authoritative = imported;
    authoritative.insert(PathBuf::from(PROJECT_FILE));
    for (path, source) in &sources {
        if source.len() as u64 > MAX_SOURCE_BYTES {
            diagnostics.push(Diagnostic::error(
                "CRL007",
                format!(
                    "source file {} is {} bytes; maximum is {MAX_SOURCE_BYTES}",
                    path.display(),
                    source.len()
                ),
            ));
        }
        if !authoritative.contains(path) {
            diagnostics.push(Diagnostic::error(
                "CRL008",
                format!(
                    "source set contains unreferenced authoritative path `{}`",
                    path.display()
                ),
            ));
        }
    }
    let total_bytes: usize = sources.values().map(String::len).sum();
    if total_bytes > MAX_PROJECT_SOURCE_BYTES {
        diagnostics.push(Diagnostic::error(
            "CRL004",
            format!("Project source is {total_bytes} bytes; maximum is {MAX_PROJECT_SOURCE_BYTES}"),
        ));
    }
    let assets = if let Some(candidate) = &project {
        let (assets, asset_diagnostics) = if let Some(asset_bytes) = asset_bytes {
            load_asset_inventory_from_bytes(&candidate.assets, asset_bytes)
        } else {
            load_asset_inventory(&root, &candidate.assets)
        };
        diagnostics.extend(asset_diagnostics);
        diagnostics.extend(validate_project(candidate));
        diagnostics.extend(validate_project_assets(candidate, &assets));
        assets
    } else {
        AssetInventory::default()
    };
    resolve_locations(&mut diagnostics, &sources);
    let revision = hash_project_contents(&sources, &assets);
    ProjectBundle {
        root,
        project,
        sources,
        assets,
        revision,
        diagnostics,
    }
}

/// Computes the deterministic identity of exact source paths and bytes.
#[must_use]
pub fn hash_sources(sources: &BTreeMap<PathBuf, String>) -> RevisionId {
    let mut hasher = Sha256::new();
    hasher.update(b"crump-project-source-v1\0");
    hash_source_entries(&mut hasher, sources);
    let digest = hasher.finalize();
    RevisionId(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(crate) fn hash_project_contents(
    sources: &BTreeMap<PathBuf, String>,
    assets: &AssetInventory,
) -> RevisionId {
    hash_project_identity(sources, assets.identities())
}

pub(crate) fn hash_project_identity<'a>(
    sources: &BTreeMap<PathBuf, String>,
    assets: impl IntoIterator<Item = (&'a Path, &'a str, u64)>,
) -> RevisionId {
    let mut identities = assets.into_iter().collect::<Vec<_>>();
    if identities.is_empty() {
        return hash_sources(sources);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"crump-project-content-v1\0");
    hash_source_entries(&mut hasher, sources);
    identities.sort_by(|left, right| left.0.cmp(right.0));
    for (path, sha256, byte_len) in identities {
        let path = path
            .to_str()
            .expect("validated asset identities use UTF-8 paths");
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update(byte_len.to_le_bytes());
        hasher.update(sha256.as_bytes());
    }
    let digest = hasher.finalize();
    RevisionId(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn hash_source_entries(hasher: &mut Sha256, sources: &BTreeMap<PathBuf, String>) {
    for (path, source) in sources {
        let path = path.to_string_lossy();
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update((source.len() as u64).to_le_bytes());
        hasher.update(source.as_bytes());
    }
}

/// Returns Crump's personal Instrument Module directory.
pub fn instrument_library_dir() -> Result<PathBuf, ProjectIoError> {
    let directories =
        ProjectDirs::from("org", "Crump", "Crump").ok_or(ProjectIoError::NoDataDirectory)?;
    Ok(directories.data_dir().join("instruments"))
}

fn resolve_import(
    root: &Path,
    import: &ImportDeclaration,
) -> Result<ResolvedImport, (&'static str, String)> {
    let display_path = import_display_path(&import.path).map_err(|message| ("CRL003", message))?;
    let (requested, allowed_root) = if let Some(name) = import.path.strip_prefix("library:") {
        // A restored Project can contain a project-local materialization of a
        // personal-library Module. Prefer that exact snapshot when present;
        // otherwise resolve the explicitly named personal-library source.
        let materialized = root.join(&display_path);
        if fs::symlink_metadata(&materialized).is_ok() {
            (materialized, root.to_path_buf())
        } else {
            let library =
                instrument_library_dir().map_err(|error| ("CRL002", error.to_string()))?;
            (library.join(format!("{name}.crump")), library)
        }
    } else {
        (root.join(&display_path), root.to_path_buf())
    };
    let allowed_root = fs::canonicalize(&allowed_root).map_err(|error| {
        (
            "CRL002",
            format!(
                "could not resolve import root {}: {error}",
                allowed_root.display()
            ),
        )
    })?;
    let real_path = fs::canonicalize(&requested).map_err(|error| {
        (
            "CRL002",
            format!(
                "could not resolve imported source {}: {error}",
                requested.display()
            ),
        )
    })?;
    if !real_path.starts_with(&allowed_root) {
        return Err((
            "CRL003",
            format!(
                "import `{}` escapes its allowed source root through a symbolic link",
                import.path
            ),
        ));
    }
    Ok(ResolvedImport {
        real_path,
        display_path,
    })
}

fn import_display_path(import: &str) -> Result<PathBuf, String> {
    if let Some(name) = import.strip_prefix("library:") {
        if !valid_library_name(name) {
            return Err(format!("invalid library module name `{name}`"));
        }
        return Ok(PathBuf::from("library").join(format!("{name}.crump")));
    }
    let relative = Path::new(import);
    let reserved_metadata = matches!(
        relative.components().next(),
        Some(Component::Normal(component)) if component == ".crump"
    );
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative == Path::new(PROJECT_FILE)
        || reserved_metadata
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(
            "imports must be normalized Project-relative paths outside `.crump/`".to_owned(),
        );
    }
    if relative
        .extension()
        .is_none_or(|extension| extension != "crump")
    {
        return Err("imports must reference a `.crump` Instrument Module".to_owned());
    }
    Ok(relative.to_path_buf())
}

fn read_source(path: &Path) -> Result<String, ProjectIoError> {
    let metadata = fs::metadata(path).map_err(|source| ProjectIoError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(ProjectIoError::TooLarge {
            path: path.to_path_buf(),
            bytes: metadata.len(),
        });
    }
    fs::read_to_string(path).map_err(|source| ProjectIoError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn resolve_locations(diagnostics: &mut Diagnostics, sources: &BTreeMap<PathBuf, String>) {
    // Diagnostics owns a compact Vec intentionally; rebuild to keep its public
    // interface immutable while enriching locations in one place.
    let mut rebuilt = Diagnostics::new();
    for mut diagnostic in std::mem::take(diagnostics).into_vec() {
        if let Some(location) = diagnostic.location.as_mut()
            && let Some(source) = sources.get(&location.file)
        {
            *location = SourceLocation::from_offsets(
                location.file.clone(),
                source,
                location.start,
                location.end,
            );
        }
        rebuilt.push(diagnostic);
    }
    *diagnostics = rebuilt;
}

fn valid_library_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

struct ResolvedImport {
    real_path: PathBuf,
    display_path: PathBuf,
}

/// Filesystem failures at the Project loading seam.
#[derive(Debug, thiserror::Error)]
pub enum ProjectIoError {
    /// No root source exists at the requested location.
    #[error("Project source not found: {}", .0.display())]
    NotFound(PathBuf),
    /// A source file could not be read as UTF-8 text.
    #[error("could not read {}: {source}", path.display())]
    Read {
        /// Affected path.
        path: PathBuf,
        /// Operating-system failure.
        source: std::io::Error,
    },
    /// A source file exceeded its resource bound.
    #[error("source file {} is {bytes} bytes; maximum is {MAX_SOURCE_BYTES}", path.display())]
    TooLarge {
        /// Affected path.
        path: PathBuf,
        /// Observed length.
        bytes: u64,
    },
    /// The platform did not expose a standard user data directory.
    #[error("could not determine the personal Crump data directory")]
    NoDataDirectory,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn write_test_wav(path: &Path, samples: &[i16]) -> Vec<u8> {
        fs::create_dir_all(path.parent().expect("media parent")).expect("media directory");
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .expect("WAV writer");
        for sample in samples {
            writer.write_sample(*sample).expect("sample");
        }
        writer.finalize().expect("finalize");
        fs::read(path).expect("WAV bytes")
    }

    fn digest(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn source_hash_covers_paths_and_exact_bytes() {
        let mut sources = BTreeMap::new();
        sources.insert(PathBuf::from("project.crump"), "one".to_owned());
        let first = hash_sources(&sources);
        sources.insert(PathBuf::from("project.crump"), "one\n".to_owned());
        assert_ne!(first, hash_sources(&sources));
    }

    #[test]
    fn asset_free_project_identity_preserves_the_source_v1_revision() {
        let mut sources = BTreeMap::new();
        sources.insert(PathBuf::from(PROJECT_FILE), SOURCE_FOR_HASH.to_owned());
        assert_eq!(
            hash_project_contents(&sources, &AssetInventory::default()),
            hash_sources(&sources)
        );
    }

    const SOURCE_FOR_HASH: &str = "crump 1\n";

    #[test]
    fn project_revision_and_diagnostics_cover_exact_media_bytes() {
        let root = tempdir().expect("Project root");
        let wav_path = root.path().join("media/voice.wav");
        let original = write_test_wav(&wav_path, &[0, 100, -100]);
        let source = include_str!("../tests/fixtures/minimal.crump").replacen(
            "  song {",
            &format!(
                "  asset voice \"Voice\" wav \"media/voice.wav\" sha256 \"{}\"\n  song {{",
                digest(&original)
            ),
            1,
        );
        fs::write(root.path().join(PROJECT_FILE), source).expect("Project source");
        let loaded = load_project(root.path()).expect("load asset Project");
        assert!(!loaded.diagnostics.has_errors());
        assert_eq!(loaded.assets.iter().len(), 1);

        write_test_wav(&wav_path, &[0, 200, -200]);
        let changed = load_project(root.path()).expect("reload changed asset");
        assert_ne!(loaded.revision, changed.revision);
        assert!(
            changed
                .diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRM003")
        );

        fs::remove_file(&wav_path).expect("remove asset");
        let missing = load_project(root.path()).expect("reload missing asset");
        assert!(
            missing
                .diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRM002")
        );
    }

    #[test]
    fn authored_asset_identity_errors_are_source_located() {
        let root = tempdir().expect("Project root");
        let source = include_str!("../tests/fixtures/minimal.crump").replacen(
            "  song {",
            &format!(
                "  asset duplicate \"One\" wav \"media/shared.wav\" sha256 \"{}\"\n  asset duplicate \"Two\" wav \"media/shared.wav\" sha256 \"{}\"\n  song {{",
                "0".repeat(64),
                "A".repeat(64)
            ),
            1,
        );
        fs::write(root.path().join(PROJECT_FILE), source).expect("Project source");
        let bundle = load_project(root.path()).expect("load invalid asset declarations");
        for code in ["CRV140", "CRV141", "CRV142"] {
            let diagnostic = bundle
                .diagnostics
                .as_slice()
                .iter()
                .find(|diagnostic| diagnostic.code == code)
                .unwrap_or_else(|| panic!("missing diagnostic {code}"));
            assert_eq!(
                diagnostic
                    .location
                    .as_ref()
                    .map(|location| location.file.as_path()),
                Some(Path::new(PROJECT_FILE))
            );
        }
    }

    #[test]
    fn missing_imports_are_reported_as_source_io_failures() {
        let directory = tempdir().unwrap();
        let source = include_str!("../tests/fixtures/minimal.crump").replacen(
            "crump 1\n",
            "crump 1\nimport \"missing.crump\"\n",
            1,
        );
        fs::write(directory.path().join(PROJECT_FILE), source).unwrap();
        let bundle = load_project(directory.path()).unwrap();
        assert!(
            bundle
                .diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRL002")
        );
        assert!(
            !bundle
                .diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRL003")
        );
    }

    #[test]
    fn rejects_parent_imports() {
        let root = tempdir().unwrap();
        let source = r#"crump 1
import "../outside.crump"
project "Bad Import" {
  id bad-import
  seed 1
  sample_rate 48000
  render_tail 1s
}
"#;
        fs::write(root.path().join(PROJECT_FILE), source).unwrap();
        let bundle = load_project(root.path()).unwrap();
        assert!(
            bundle
                .diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRL003")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_imports_that_escape_through_symlinks() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_module = outside.path().join("outside.crump");
        fs::write(&outside_module, "crump-module 1\n").unwrap();
        symlink(&outside_module, root.path().join("escape.crump")).unwrap();
        let source = include_str!("../tests/fixtures/minimal.crump").replacen(
            "crump 1\n",
            "crump 1\nimport \"escape.crump\"\n",
            1,
        );
        fs::write(root.path().join(PROJECT_FILE), source).unwrap();

        let bundle = load_project(root.path()).unwrap();
        assert!(
            bundle
                .diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRL003")
        );
        assert!(!bundle.sources.contains_key(Path::new("escape.crump")));
    }

    #[test]
    fn in_memory_source_sets_reject_unreferenced_files() {
        let root = tempdir().unwrap();
        let mut sources = BTreeMap::new();
        sources.insert(
            PathBuf::from(PROJECT_FILE),
            include_str!("../tests/fixtures/minimal.crump").to_owned(),
        );
        sources.insert(PathBuf::from("unused.crump"), "crump-module 1\n".to_owned());
        let bundle = load_project_sources(root.path().to_path_buf(), sources);
        assert!(
            bundle
                .diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRL008")
        );
    }
}
