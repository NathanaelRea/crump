//! Cheap filesystem change probes for the background workspace reload seam.

use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
};

use crate::{
    dsl::parse_project,
    media::valid_asset_path,
    project::{PROJECT_FILE, ProjectBundle, import_display_path, instrument_library_dir},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
    mode: u32,
    length: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetadataStamp {
    Present(FileIdentity),
    Missing,
    Error(Option<i32>),
}

impl MetadataStamp {
    fn capture(result: std::io::Result<fs::Metadata>) -> Self {
        match result {
            Ok(metadata) => Self::Present(FileIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
                mode: metadata.mode(),
                length: metadata.len(),
                modified_seconds: metadata.mtime(),
                modified_nanoseconds: metadata.mtime_nsec(),
                changed_seconds: metadata.ctime(),
                changed_nanoseconds: metadata.ctime_nsec(),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::Missing,
            Err(error) => Self::Error(error.raw_os_error()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    link: MetadataStamp,
    target: Option<MetadataStamp>,
}

impl FileStamp {
    fn capture(path: &Path) -> Self {
        let link_metadata = fs::symlink_metadata(path);
        let follows_link = link_metadata
            .as_ref()
            .is_ok_and(|metadata| metadata.file_type().is_symlink());
        Self {
            link: MetadataStamp::capture(link_metadata),
            target: follows_link.then(|| MetadataStamp::capture(fs::metadata(path))),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WatchSet {
    files: Vec<(PathBuf, FileStamp)>,
}

impl WatchSet {
    fn seed(paths: BTreeSet<PathBuf>) -> Self {
        Self {
            files: paths
                .into_iter()
                .map(|path| {
                    (
                        path,
                        FileStamp {
                            link: MetadataStamp::Missing,
                            target: None,
                        },
                    )
                })
                .collect(),
        }
    }

    fn capture(paths: BTreeSet<PathBuf>) -> Self {
        Self {
            files: paths
                .into_iter()
                .map(|path| {
                    let stamp = FileStamp::capture(&path);
                    (path, stamp)
                })
                .collect(),
        }
    }

    fn recapture(&self) -> Self {
        Self {
            files: self
                .files
                .iter()
                .map(|(path, _)| (path.clone(), FileStamp::capture(path)))
                .collect(),
        }
    }

    #[cfg(test)]
    fn changed(&self) -> bool {
        self.files
            .iter()
            .any(|(path, stamp)| FileStamp::capture(path) != *stamp)
    }
}

/// Metadata probe that decides when exact byte reconstruction is necessary.
///
/// File metadata is only an invalidation signal. Candidate identity remains the
/// exact source/media revision produced by `load_project` after invalidation.
#[derive(Debug, Clone, Default)]
pub(super) struct WorkspaceWatch {
    candidate: WatchSet,
    history: WatchSet,
}

impl WorkspaceWatch {
    pub(super) fn seed(root: &Path, bundle: &ProjectBundle) -> Self {
        Self {
            candidate: WatchSet::seed(candidate_paths(root, bundle)),
            history: WatchSet::seed(history_paths(root)),
        }
    }

    pub(super) fn capture(root: &Path, bundle: &ProjectBundle) -> Self {
        Self {
            candidate: WatchSet::capture(candidate_paths(root, bundle)),
            history: WatchSet::capture(history_paths(root)),
        }
    }

    pub(super) fn capture_candidate(root: &Path, bundle: &ProjectBundle) -> Self {
        Self {
            candidate: WatchSet::capture(candidate_paths(root, bundle)),
            history: WatchSet::default(),
        }
    }

    pub(super) fn capture_history(root: &Path) -> Self {
        Self {
            candidate: WatchSet::default(),
            history: WatchSet::capture(history_paths(root)),
        }
    }

    pub(super) fn recapture_candidate(&self) -> Self {
        Self {
            candidate: self.candidate.recapture(),
            history: WatchSet::default(),
        }
    }

    pub(super) fn recapture_history(&self) -> Self {
        Self {
            candidate: WatchSet::default(),
            history: self.history.recapture(),
        }
    }

    pub(super) fn same_candidate(&self, other: &Self) -> bool {
        self.candidate == other.candidate
    }

    pub(super) fn same_history(&self, other: &Self) -> bool {
        self.history == other.history
    }

    pub(super) fn adopt_candidate(&mut self, other: Self) {
        self.candidate = other.candidate;
    }

    pub(super) fn adopt_history(&mut self, other: Self) {
        self.history = other.history;
    }

    #[cfg(test)]
    pub(super) fn candidate_changed(&self) -> bool {
        self.candidate.changed()
    }
}

fn candidate_paths(root: &Path, bundle: &ProjectBundle) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::from([root.join(PROJECT_FILE)]);
    if let Some(source) = bundle.sources.get(Path::new(PROJECT_FILE)) {
        for import in parse_project(Path::new(PROJECT_FILE), source).imports {
            let Ok(display_path) = import_display_path(&import.path) else {
                continue;
            };
            paths.insert(root.join(&display_path));
            if import.path.starts_with("library:")
                && let Some(file_name) = display_path.file_name()
                && let Ok(library) = instrument_library_dir()
            {
                paths.insert(library.join(file_name));
            }
        }
    }
    for source in bundle.sources.keys() {
        paths.insert(root.join(source));
        if let Ok(relative) = source.strip_prefix("library")
            && relative.components().count() == 1
            && let Ok(library) = instrument_library_dir()
        {
            // A Project-local materialization takes precedence over the
            // personal library. Track both so creating either invalidates the
            // exact import resolution used by the next load.
            paths.insert(library.join(relative));
        }
    }
    if let Some(project) = bundle.project.as_ref() {
        paths.extend(
            project
                .assets
                .iter()
                .filter(|asset| valid_asset_path(&asset.path))
                .map(|asset| root.join(&asset.path)),
        );
    }
    paths.extend(bundle.assets.iter().map(|asset| root.join(asset.path())));
    paths
}

fn history_paths(root: &Path) -> BTreeSet<PathBuf> {
    BTreeSet::from([
        root.join(".crump/state.json"),
        root.join(".crump/history/committed"),
    ])
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn file_stamp_detects_same_length_replacement() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("project.crump");
        fs::write(&path, "aaaa")?;
        let watch = WatchSet::capture(BTreeSet::from([path.clone()]));
        let replacement = directory.path().join("replacement");
        fs::write(&replacement, "bbbb")?;
        fs::rename(&replacement, &path)?;
        assert!(watch.changed());
        Ok(())
    }

    #[test]
    fn symlink_target_changes_invalidate_the_watch() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir()?;
        let target = directory.path().join("target.crump");
        let link = directory.path().join("linked.crump");
        fs::write(&target, "aaaa")?;
        symlink(&target, &link)?;
        let watch = WatchSet::capture(BTreeSet::from([link]));

        fs::write(&target, "bbbb")?;

        assert!(watch.changed());
        Ok(())
    }

    #[test]
    fn missing_declared_import_is_watched_for_creation() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        fs::write(
            directory.path().join(PROJECT_FILE),
            "crump 1\nimport \"missing.crump\"\nproject \"Watch\" {\n  sample_rate 48000\n  tempo 120\n  meter 4/4\n}\n",
        )?;
        let bundle = crate::project::load_project(directory.path())?;
        assert!(!bundle.sources.contains_key(Path::new("missing.crump")));
        let watch = WorkspaceWatch::capture(directory.path(), &bundle);

        fs::write(directory.path().join("missing.crump"), "crump-module 1\n")?;

        assert!(watch.candidate_changed());
        Ok(())
    }

    #[test]
    fn file_stamp_ignores_unchanged_files() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("project.crump");
        fs::write(&path, "unchanged")?;
        let watch = WatchSet::capture(BTreeSet::from([path]));
        assert!(!watch.changed());
        Ok(())
    }
}
