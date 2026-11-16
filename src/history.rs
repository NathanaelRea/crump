//! Persistent append-only Project History and Active Revision recovery.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use fs2::FileExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[cfg(test)]
use crate::project::load_project_sources;
use crate::{
    diagnostic::Diagnostic,
    media::{
        AssetInventory, AssetReadError, MAX_MEDIA_ASSET_BYTES, MAX_MEDIA_ASSETS,
        MAX_PROJECT_MEDIA_BYTES, read_project_asset_bytes, read_regular_asset_bytes,
        valid_asset_path, valid_sha256,
    },
    project::{
        MAX_PROJECT_SOURCE_BYTES, MAX_SOURCE_BYTES, PROJECT_FILE, ProjectBundle, RevisionId,
        hash_project_identity, load_project, load_project_sources_with_assets,
    },
};

const HISTORY_SCHEMA: u32 = 1;
const METADATA_DIR: &str = ".crump";
const STATE_FILE: &str = "state.json";
const SOURCE_TRANSACTION_FILE: &str = "source-transaction.json";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The actor responsible for a Project History Entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeOrigin {
    /// Stable origin class.
    pub kind: OriginKind,
    /// Human or Agent identity shown in history.
    pub name: String,
}

impl ChangeOrigin {
    /// Creates a human workspace origin.
    #[must_use]
    pub fn human(name: impl Into<String>) -> Self {
        Self {
            kind: OriginKind::Human,
            name: name.into(),
        }
    }

    /// Creates an external Authoring Agent origin.
    #[must_use]
    pub fn agent(name: impl Into<String>) -> Self {
        Self {
            kind: OriginKind::Agent,
            name: name.into(),
        }
    }

    /// Creates an internal bootstrap or recovery origin.
    #[must_use]
    pub fn system(name: impl Into<String>) -> Self {
        Self {
            kind: OriginKind::System,
            name: name.into(),
        }
    }
}

/// Project History origin class.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OriginKind {
    /// Structured or direct workspace action.
    Human,
    /// Explicit external Agent publication.
    Agent,
    /// Bootstrap or maintenance action performed by Crump.
    System,
}

/// Required provenance for one successful publication.
#[derive(Debug, Clone)]
pub struct Publication {
    /// Actor responsible for the intended save.
    pub origin: ChangeOrigin,
    /// Concise description shown in the history list.
    pub summary: String,
    /// Why the change was made; required for Agent Changes.
    pub explanation: String,
    /// Source entry selected by History Restore.
    pub restored_from: Option<String>,
}

impl Publication {
    /// Creates ordinary publication provenance.
    #[must_use]
    pub fn new(
        origin: ChangeOrigin,
        summary: impl Into<String>,
        explanation: impl Into<String>,
    ) -> Self {
        Self {
            origin,
            summary: summary.into(),
            explanation: explanation.into(),
            restored_from: None,
        }
    }
}

/// One exact, append-only Project transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Metadata schema version.
    pub schema_version: u32,
    /// Fixed-width sequence identity.
    pub id: String,
    /// Monotonic local sequence.
    pub sequence: u64,
    /// Candidate content promoted by this entry.
    pub revision: RevisionId,
    /// RFC 3339 UTC creation time.
    pub created_at: String,
    /// Actor provenance.
    pub origin: ChangeOrigin,
    /// Concise history-list text.
    pub summary: String,
    /// Human-readable rationale.
    pub explanation: String,
    /// Immediately preceding committed entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Earlier entry selected by History Restore.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_from: Option<String>,
    /// Exact source-file change summary.
    pub changes: Vec<SourceChange>,
}

/// One file-level transition in a History Entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceChange {
    /// Stable Project-relative path.
    pub path: PathBuf,
    /// Transition kind.
    pub kind: SourceChangeKind,
    /// Prior exact-byte hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_sha256: Option<String>,
    /// New exact-byte hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_sha256: Option<String>,
    /// Changed line count for quick inspection.
    pub changed_lines: usize,
}

/// File-level source transition kind.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceChangeKind {
    /// New authoritative source file.
    Added,
    /// Existing source bytes changed.
    Modified,
    /// Source ceased to belong to the Project.
    Removed,
}

/// Persistent pointer to the Active Revision. Source remains authoritative.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveState {
    /// Metadata schema.
    pub schema_version: u32,
    /// Committed entry producing runtime audio.
    pub active_entry: String,
    /// Exact source revision represented by that entry.
    pub active_revision: RevisionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SnapshotAsset {
    sha256: String,
    byte_len: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SourceSnapshot {
    schema_version: u32,
    revision: RevisionId,
    sources: BTreeMap<PathBuf, String>,
    #[serde(default)]
    assets: BTreeMap<PathBuf, SnapshotAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SourceTransaction {
    schema_version: u32,
    previous_active_entry: Option<String>,
    before: SourceSnapshot,
    after: SourceSnapshot,
    #[serde(default)]
    before_asset_files: Option<BTreeMap<PathBuf, SnapshotAsset>>,
}

#[derive(Debug, Clone, Copy)]
enum SourcePublicationMode {
    Candidate,
    Restore,
}

struct SourceTransactionContext<'a> {
    root: &'a Path,
    before_asset_files: &'a BTreeMap<PathBuf, SnapshotAsset>,
}

/// Locked access to one Project's persistent history.
pub struct HistoryStore {
    root: PathBuf,
    metadata: PathBuf,
    _lock: File,
}

impl HistoryStore {
    /// Opens a Project History, creating its private metadata directories.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, HistoryError> {
        let root = fs::canonicalize(root.as_ref()).map_err(|source| HistoryError::Io {
            path: root.as_ref().to_path_buf(),
            source,
        })?;
        let metadata = root.join(METADATA_DIR);
        fs::create_dir_all(metadata.join("history/entries")).map_err(|source| {
            HistoryError::Io {
                path: metadata.clone(),
                source,
            }
        })?;
        fs::create_dir_all(metadata.join("history/snapshots")).map_err(|source| {
            HistoryError::Io {
                path: metadata.clone(),
                source,
            }
        })?;
        fs::create_dir_all(metadata.join("history/committed")).map_err(|source| {
            HistoryError::Io {
                path: metadata.clone(),
                source,
            }
        })?;
        fs::create_dir_all(metadata.join("history/assets")).map_err(|source| HistoryError::Io {
            path: metadata.clone(),
            source,
        })?;
        let lock_path = metadata.join("lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|source| HistoryError::Io {
                path: lock_path,
                source,
            })?;
        lock.lock_exclusive().map_err(|source| HistoryError::Io {
            path: metadata.join("lock"),
            source,
        })?;
        let store = Self {
            root,
            metadata,
            _lock: lock,
        };
        store.remove_metadata_temporary_files()?;
        store.recover_uncommitted_state()?;
        store.recover_source_transaction()?;
        Ok(store)
    }

    /// Returns the current Active Revision pointer.
    ///
    /// This validates committed entry and snapshot metadata. Exact historical
    /// asset bytes are verified only when an operation consumes the snapshot.
    pub fn active_state(&self) -> Result<Option<ActiveState>, HistoryError> {
        let state: Option<ActiveState> = read_json_optional(&self.metadata.join(STATE_FILE))?;
        let Some(state) = state else {
            return Ok(None);
        };
        validate_state_identity(&state)?;
        let entry = self.entry_metadata(&state.active_entry)?;
        if entry.revision != state.active_revision {
            return Err(HistoryError::Corrupt(format!(
                "Active state revision {} does not match entry {} revision {}",
                state.active_revision, state.active_entry, entry.revision
            )));
        }
        Ok(Some(state))
    }

    /// Loads the exact Active Revision independently of current Candidate files.
    pub fn active_bundle(&self) -> Result<Option<ProjectBundle>, HistoryError> {
        let Some(state) = self.active_state()? else {
            return Ok(None);
        };
        let snapshot = self.snapshot_metadata(&state.active_revision)?;
        let asset_bytes = self.snapshot_asset_bytes(&snapshot)?;
        let bundle =
            load_project_sources_with_assets(self.root.clone(), snapshot.sources, &asset_bytes);
        if bundle.revision != state.active_revision || bundle.diagnostics.has_errors() {
            return Err(HistoryError::Corrupt(format!(
                "Active Revision {} no longer validates from its snapshot",
                state.active_revision
            )));
        }
        Ok(Some(bundle))
    }

    /// Publishes one complete, valid Candidate Revision and appends history.
    pub fn publish(
        &self,
        bundle: &ProjectBundle,
        publication: Publication,
    ) -> Result<HistoryEntry, HistoryError> {
        self.validate_publication(bundle, &publication)?;
        if self
            .active_state()?
            .is_some_and(|state| state.active_revision == bundle.revision)
        {
            return Err(HistoryError::AlreadyActive(bundle.revision.clone()));
        }
        self.publish_inner(bundle, publication)
    }

    /// Lists every committed entry in sequence order.
    ///
    /// Listing verifies entry and snapshot metadata without rereading immutable
    /// content-addressed asset blobs for every historical entry.
    pub fn list(&self) -> Result<Vec<HistoryEntry>, HistoryError> {
        let mut ids = committed_ids(&self.metadata)?;
        ids.sort();
        ids.into_iter().map(|id| self.entry_metadata(&id)).collect()
    }

    /// Reads one committed entry and verifies its referenced asset blobs.
    pub fn entry(&self, id: &str) -> Result<HistoryEntry, HistoryError> {
        let entry = self.entry_metadata(id)?;
        self.snapshot(&entry.revision)?;
        Ok(entry)
    }

    fn entry_metadata(&self, id: &str) -> Result<HistoryEntry, HistoryError> {
        if !valid_entry_id(id) {
            return Err(HistoryError::UnknownEntry(id.to_owned()));
        }
        let marker = self.commit_path(id);
        match fs::read(&marker) {
            Ok(bytes) if bytes == b"committed\n" => {}
            Ok(_) => {
                return Err(HistoryError::Corrupt(format!(
                    "history marker `{id}` has invalid content"
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(HistoryError::UnknownEntry(id.to_owned()));
            }
            Err(source) => {
                return Err(HistoryError::Io {
                    path: marker,
                    source,
                });
            }
        }
        let entry: HistoryEntry = read_json(&self.entry_path(id))?;
        validate_entry_identity(&entry, id)?;
        self.snapshot_metadata(&entry.revision)?;
        Ok(entry)
    }

    /// Produces an exact unified source diff against an entry's parent.
    pub fn diff(&self, id: &str) -> Result<String, HistoryError> {
        let entry = self.entry_metadata(id)?;
        let after = self.snapshot_metadata(&entry.revision)?;
        let before = match &entry.parent {
            Some(parent) => {
                let parent = self.entry_metadata(parent)?;
                Some(self.snapshot_metadata(&parent.revision)?)
            }
            None => None,
        };
        Ok(diff_snapshots(before.as_ref(), &after))
    }

    /// Restores an earlier exact state as a new append-only entry.
    pub fn restore(
        &self,
        target_id: &str,
        origin: ChangeOrigin,
        explanation: impl Into<String>,
    ) -> Result<HistoryEntry, HistoryError> {
        let target = self.entry_metadata(target_id)?;
        let snapshot = self.snapshot_metadata(&target.revision)?;
        let asset_bytes = self.snapshot_asset_bytes(&snapshot)?;
        let previous = load_project(&self.root).map_err(HistoryError::Project)?;
        let target_bundle = load_project_sources_with_assets(
            self.root.clone(),
            snapshot.sources.clone(),
            &asset_bytes,
        );
        if target_bundle.revision != target.revision || target_bundle.diagnostics.has_errors() {
            return Err(HistoryError::Corrupt(format!(
                "restore snapshot produced revision {}, expected {}",
                target_bundle.revision, target.revision
            )));
        }
        let publication = Publication {
            origin,
            summary: format!("Restore history entry {target_id}"),
            explanation: explanation.into(),
            restored_from: Some(target_id.to_owned()),
        };
        self.source_transaction(
            &previous,
            &target_bundle,
            publication,
            SourcePublicationMode::Restore,
            |transaction| {
                reconcile_source_set(transaction.root, &snapshot.sources, &previous.sources)?;
                reconcile_asset_set(
                    transaction.root,
                    &snapshot.assets,
                    transaction.before_asset_files,
                    false,
                )
            },
        )
    }

    fn publish_inner(
        &self,
        bundle: &ProjectBundle,
        publication: Publication,
    ) -> Result<HistoryEntry, HistoryError> {
        self.persist_asset_blobs(&bundle.assets)?;
        let previous_state = self.active_state()?;
        let previous_snapshot = match &previous_state {
            Some(state) => Some(self.snapshot(&state.active_revision)?),
            None => None,
        };
        let snapshot = SourceSnapshot {
            schema_version: HISTORY_SCHEMA,
            revision: bundle.revision.clone(),
            sources: bundle.sources.clone(),
            assets: snapshot_assets(&bundle.assets),
        };
        let snapshot_path = self.snapshot_path(&bundle.revision);
        if snapshot_path.exists() {
            let existing = self.snapshot(&bundle.revision)?;
            if existing.sources != snapshot.sources || existing.assets != snapshot.assets {
                return Err(HistoryError::Corrupt(format!(
                    "snapshot {} does not match its content-addressed Project state",
                    bundle.revision
                )));
            }
        } else {
            write_json_atomic(&snapshot_path, &snapshot)?;
        }
        let sequence = self.next_sequence()?;
        let id = format!("{sequence:012}");
        let created_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| HistoryError::Clock(error.to_string()))?;
        let entry = HistoryEntry {
            schema_version: HISTORY_SCHEMA,
            id: id.clone(),
            sequence,
            revision: bundle.revision.clone(),
            created_at,
            origin: publication.origin,
            summary: publication.summary,
            explanation: publication.explanation,
            parent: previous_state
                .as_ref()
                .map(|state| state.active_entry.clone()),
            restored_from: publication.restored_from,
            changes: summarize_changes(previous_snapshot.as_ref(), &snapshot),
        };
        write_json_atomic(&self.entry_path(&id), &entry)?;
        let state = ActiveState {
            schema_version: HISTORY_SCHEMA,
            active_entry: id.clone(),
            active_revision: bundle.revision.clone(),
        };
        write_json_atomic(&self.metadata.join(STATE_FILE), &state)?;
        write_atomic(&self.commit_path(&id), b"committed\n")?;
        Ok(entry)
    }

    fn validate_publication(
        &self,
        bundle: &ProjectBundle,
        publication: &Publication,
    ) -> Result<(), HistoryError> {
        if bundle.root != self.root {
            return Err(HistoryError::WrongProject);
        }
        let current = load_project(&self.root)?;
        if current.revision != bundle.revision {
            return Err(HistoryError::StaleCandidate {
                expected: bundle.revision.clone(),
                actual: current.revision,
            });
        }
        if bundle.project.is_none() || bundle.diagnostics.has_errors() {
            return Err(HistoryError::InvalidCandidate(
                bundle.diagnostics.as_slice().to_vec(),
            ));
        }
        if publication.origin.name.trim().is_empty() || publication.summary.trim().is_empty() {
            return Err(HistoryError::InvalidProvenance(
                "origin name and summary must not be empty".to_owned(),
            ));
        }
        if publication.origin.kind == OriginKind::Agent && publication.explanation.trim().is_empty()
        {
            return Err(HistoryError::InvalidProvenance(
                "Agent publication requires a non-empty explanation".to_owned(),
            ));
        }
        Ok(())
    }

    fn next_sequence(&self) -> Result<u64, HistoryError> {
        let directory = self.metadata.join("history/entries");
        let mut maximum = 0;
        for item in fs::read_dir(&directory).map_err(|source| HistoryError::Io {
            path: directory.clone(),
            source,
        })? {
            let item = item.map_err(|source| HistoryError::Io {
                path: directory.clone(),
                source,
            })?;
            if let Some(sequence) = item
                .path()
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<u64>().ok())
            {
                maximum = maximum.max(sequence);
            }
        }
        maximum
            .checked_add(1)
            .ok_or_else(|| HistoryError::Corrupt("history sequence overflow".to_owned()))
    }

    fn snapshot(&self, revision: &RevisionId) -> Result<SourceSnapshot, HistoryError> {
        let snapshot = self.snapshot_metadata(revision)?;
        self.snapshot_asset_bytes(&snapshot)?;
        Ok(snapshot)
    }

    fn snapshot_metadata(&self, revision: &RevisionId) -> Result<SourceSnapshot, HistoryError> {
        validate_revision_id(revision)?;
        let snapshot: SourceSnapshot = read_json(&self.snapshot_path(revision))?;
        if snapshot.revision != *revision {
            return Err(HistoryError::Corrupt(format!(
                "snapshot {revision} has invalid identity"
            )));
        }
        validate_snapshot_integrity(&snapshot)?;
        Ok(snapshot)
    }

    fn asset_blob_path(&self, sha256: &str) -> PathBuf {
        self.metadata.join("history/assets").join(sha256)
    }

    fn persist_asset_blob(
        &self,
        sha256: &str,
        byte_len: u64,
        bytes: &[u8],
    ) -> Result<(), HistoryError> {
        if bytes.len() as u64 != byte_len || hash_bytes(bytes) != sha256 {
            return Err(HistoryError::Corrupt(format!(
                "asset blob {sha256} does not match its exact-byte identity"
            )));
        }
        let path = self.asset_blob_path(sha256);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                let existing = read_regular_file(&path)?;
                if existing.len() as u64 != byte_len || hash_bytes(&existing) != sha256 {
                    return Err(HistoryError::Corrupt(format!(
                        "history asset blob {sha256} is corrupt"
                    )));
                }
            }
            Ok(_) => {
                return Err(HistoryError::Corrupt(format!(
                    "history asset blob {sha256} is not a regular file"
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                write_atomic(&path, bytes)?;
            }
            Err(source) => return Err(HistoryError::Io { path, source }),
        }
        Ok(())
    }

    fn persist_asset_blobs(&self, inventory: &AssetInventory) -> Result<(), HistoryError> {
        for asset in inventory.iter() {
            self.persist_asset_blob(asset.sha256(), asset.byte_len(), asset.bytes())?;
        }
        Ok(())
    }

    fn capture_before_asset_files(
        &self,
        before: &ProjectBundle,
        after: &ProjectBundle,
    ) -> Result<BTreeMap<PathBuf, SnapshotAsset>, HistoryError> {
        let mut paths = before
            .assets
            .iter()
            .chain(after.assets.iter())
            .map(|asset| asset.path().to_path_buf())
            .collect::<BTreeSet<_>>();
        for project in [before.project.as_ref(), after.project.as_ref()]
            .into_iter()
            .flatten()
        {
            paths.extend(project.assets.iter().map(|asset| asset.path.clone()));
        }

        let mut captured = BTreeMap::new();
        let mut captured_bytes = 0_u64;
        for relative in paths {
            let bytes = match read_project_asset_bytes(&self.root, &relative) {
                Ok(bytes) => bytes,
                Err(AssetReadError::Unreadable(error))
                    if error.kind() == std::io::ErrorKind::NotFound =>
                {
                    continue;
                }
                Err(AssetReadError::Unreadable(source)) => {
                    return Err(HistoryError::Io {
                        path: self.root.join(&relative),
                        source,
                    });
                }
                Err(AssetReadError::Unsafe(message)) => {
                    return Err(HistoryError::UnsupportedRestore(format!(
                        "cannot journal media path `{}`: {message}",
                        relative.display()
                    )));
                }
                Err(AssetReadError::TooLarge) => {
                    return Err(HistoryError::UnsupportedRestore(format!(
                        "cannot journal media path `{}` because it exceeds the {MAX_MEDIA_ASSET_BYTES}-byte maximum",
                        relative.display()
                    )));
                }
            };
            if captured.len() == MAX_MEDIA_ASSETS {
                return Err(HistoryError::UnsupportedRestore(format!(
                    "cannot journal more than {MAX_MEDIA_ASSETS} existing media files"
                )));
            }
            let byte_len = bytes.len() as u64;
            captured_bytes = captured_bytes.checked_add(byte_len).ok_or_else(|| {
                HistoryError::UnsupportedRestore("transaction media byte count overflow".to_owned())
            })?;
            if captured_bytes > MAX_PROJECT_MEDIA_BYTES {
                return Err(HistoryError::UnsupportedRestore(format!(
                    "cannot journal more than {MAX_PROJECT_MEDIA_BYTES} media bytes"
                )));
            }
            let identity = SnapshotAsset {
                sha256: hash_bytes(&bytes),
                byte_len,
            };
            self.persist_asset_blob(&identity.sha256, identity.byte_len, &bytes)?;
            captured.insert(relative, identity);
        }
        Ok(captured)
    }

    fn snapshot_asset_bytes(
        &self,
        snapshot: &SourceSnapshot,
    ) -> Result<BTreeMap<PathBuf, Vec<u8>>, HistoryError> {
        read_snapshot_asset_bytes(&self.metadata, snapshot)
    }

    /// Runs an edit-specific write inside the complete durable source transaction.
    pub(crate) fn commit_source_transaction<E>(
        &self,
        before: &ProjectBundle,
        after: &ProjectBundle,
        publication: Publication,
        write: impl FnOnce() -> Result<(), E>,
    ) -> Result<HistoryEntry, E>
    where
        E: From<HistoryError>,
    {
        self.source_transaction(
            before,
            after,
            publication,
            SourcePublicationMode::Candidate,
            |_| write(),
        )
    }

    fn source_transaction<E>(
        &self,
        before: &ProjectBundle,
        after: &ProjectBundle,
        publication: Publication,
        publication_mode: SourcePublicationMode,
        write: impl FnOnce(SourceTransactionContext<'_>) -> Result<(), E>,
    ) -> Result<HistoryEntry, E>
    where
        E: From<HistoryError>,
    {
        let before_asset_files = self
            .begin_source_transaction_with_assets(before, after)
            .map_err(E::from)?;
        let operation = (|| -> Result<HistoryEntry, E> {
            write(SourceTransactionContext {
                root: &self.root,
                before_asset_files: &before_asset_files,
            })?;
            match publication_mode {
                SourcePublicationMode::Candidate => {
                    self.publish(after, publication).map_err(E::from)
                }
                SourcePublicationMode::Restore => {
                    let restored = load_project(&self.root)
                        .map_err(|error| E::from(HistoryError::Project(error)))?;
                    if restored.revision != after.revision || restored.diagnostics.has_errors() {
                        return Err(E::from(HistoryError::Corrupt(format!(
                            "restored source produced revision {}, expected {}",
                            restored.revision, after.revision
                        ))));
                    }
                    self.validate_publication(&restored, &publication)
                        .map_err(E::from)?;
                    self.publish_inner(&restored, publication).map_err(E::from)
                }
            }
        })();
        match operation {
            Ok(entry) => {
                self.finish_source_transaction().map_err(E::from)?;
                Ok(entry)
            }
            Err(error) => {
                if let Err(rollback) = self.abort_source_transaction() {
                    return Err(E::from(rollback));
                }
                Err(error)
            }
        }
    }

    #[cfg(test)]
    fn begin_source_transaction(
        &self,
        before: &ProjectBundle,
        after: &ProjectBundle,
    ) -> Result<(), HistoryError> {
        self.begin_source_transaction_with_assets(before, after)
            .map(drop)
    }

    fn begin_source_transaction_with_assets(
        &self,
        before: &ProjectBundle,
        after: &ProjectBundle,
    ) -> Result<BTreeMap<PathBuf, SnapshotAsset>, HistoryError> {
        let path = self.metadata.join(SOURCE_TRANSACTION_FILE);
        if path.exists() {
            return Err(HistoryError::Corrupt(
                "a prior source transaction was not recovered".to_owned(),
            ));
        }
        self.persist_asset_blobs(&before.assets)?;
        self.persist_asset_blobs(&after.assets)?;
        let before_asset_files = self.capture_before_asset_files(before, after)?;
        let transaction = SourceTransaction {
            schema_version: HISTORY_SCHEMA,
            previous_active_entry: self.active_state()?.map(|state| state.active_entry),
            before: SourceSnapshot {
                schema_version: HISTORY_SCHEMA,
                revision: before.revision.clone(),
                sources: before.sources.clone(),
                assets: snapshot_assets(&before.assets),
            },
            after: SourceSnapshot {
                schema_version: HISTORY_SCHEMA,
                revision: after.revision.clone(),
                sources: after.sources.clone(),
                assets: snapshot_assets(&after.assets),
            },
            before_asset_files: Some(before_asset_files.clone()),
        };
        validate_transaction_snapshot(&self.metadata, &transaction.before, &before_asset_files)?;
        validate_transaction_snapshot(
            &self.metadata,
            &transaction.after,
            &transaction.after.assets,
        )?;
        write_json_atomic(&path, &transaction)?;
        Ok(before_asset_files)
    }

    fn finish_source_transaction(&self) -> Result<(), HistoryError> {
        self.clear_source_transaction()
    }

    fn abort_source_transaction(&self) -> Result<(), HistoryError> {
        let path = self.metadata.join(SOURCE_TRANSACTION_FILE);
        let transaction: SourceTransaction = read_json(&path)?;
        let before_assets = transaction
            .before_asset_files
            .as_ref()
            .unwrap_or(&transaction.before.assets);
        validate_transaction_snapshot(&self.metadata, &transaction.before, before_assets)?;
        reconcile_interrupted_source_set(
            &self.root,
            &transaction.before.sources,
            &transaction.after.sources,
        )?;
        reconcile_asset_set(&self.root, before_assets, &transaction.after.assets, true)?;
        self.clear_source_transaction()
    }

    fn recover_source_transaction(&self) -> Result<(), HistoryError> {
        let path = self.metadata.join(SOURCE_TRANSACTION_FILE);
        let Some(transaction): Option<SourceTransaction> = read_json_optional(&path)? else {
            return Ok(());
        };
        if transaction.schema_version != HISTORY_SCHEMA {
            return Err(HistoryError::Corrupt(
                "source transaction has an unsupported schema".to_owned(),
            ));
        }
        let active = self.active_state()?;
        let committed_after = active.as_ref().is_some_and(|state| {
            state.active_revision == transaction.after.revision
                && Some(state.active_entry.as_str()) != transaction.previous_active_entry.as_deref()
                && self.commit_path(&state.active_entry).exists()
        });
        let before_assets = transaction
            .before_asset_files
            .as_ref()
            .unwrap_or(&transaction.before.assets);
        let (target, other, target_assets, other_assets) = if committed_after {
            (
                &transaction.after,
                &transaction.before,
                &transaction.after.assets,
                before_assets,
            )
        } else {
            (
                &transaction.before,
                &transaction.after,
                before_assets,
                &transaction.after.assets,
            )
        };
        validate_transaction_snapshot(&self.metadata, target, target_assets)?;
        reconcile_interrupted_source_set(&self.root, &target.sources, &other.sources)?;
        reconcile_asset_set(&self.root, target_assets, other_assets, true)?;
        self.clear_source_transaction()
    }

    fn clear_source_transaction(&self) -> Result<(), HistoryError> {
        let path = self.metadata.join(SOURCE_TRANSACTION_FILE);
        fs::remove_file(&path).map_err(|source| HistoryError::Io {
            path: path.clone(),
            source,
        })?;
        sync_directory(&self.metadata)
    }

    fn recover_uncommitted_state(&self) -> Result<(), HistoryError> {
        let state: Option<ActiveState> = read_json_optional(&self.metadata.join(STATE_FILE))?;
        if let Some(state) = state {
            validate_state_identity(&state)?;
            if self.commit_path(&state.active_entry).exists() {
                let entry = self.entry(&state.active_entry)?;
                if entry.revision != state.active_revision {
                    return Err(HistoryError::Corrupt(format!(
                        "Active state does not match committed entry {}",
                        state.active_entry
                    )));
                }
            } else {
                let interrupted: HistoryEntry = read_json(&self.entry_path(&state.active_entry))?;
                validate_entry_identity(&interrupted, &state.active_entry)?;
                if interrupted.revision != state.active_revision {
                    return Err(HistoryError::Corrupt(format!(
                        "interrupted entry {} does not match Active state",
                        state.active_entry
                    )));
                }
                match interrupted.parent {
                    Some(parent_id) => {
                        let parent = self.entry(&parent_id)?;
                        let recovered = ActiveState {
                            schema_version: HISTORY_SCHEMA,
                            active_entry: parent.id,
                            active_revision: parent.revision,
                        };
                        write_json_atomic(&self.metadata.join(STATE_FILE), &recovered)?;
                    }
                    None => {
                        fs::remove_file(self.metadata.join(STATE_FILE)).map_err(|source| {
                            HistoryError::Io {
                                path: self.metadata.join(STATE_FILE),
                                source,
                            }
                        })?;
                        sync_directory(&self.metadata)?;
                    }
                }
            }
        }
        self.remove_uncommitted_entries()
    }

    fn remove_metadata_temporary_files(&self) -> Result<(), HistoryError> {
        for directory in [
            self.metadata.clone(),
            self.metadata.join("history/entries"),
            self.metadata.join("history/snapshots"),
            self.metadata.join("history/committed"),
            self.metadata.join("history/assets"),
        ] {
            for item in fs::read_dir(&directory).map_err(|source| HistoryError::Io {
                path: directory.clone(),
                source,
            })? {
                let item = item.map_err(|source| HistoryError::Io {
                    path: directory.clone(),
                    source,
                })?;
                let name = item.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') && name.contains(".tmp-") {
                    let path = item.path();
                    if item
                        .file_type()
                        .map_err(|source| HistoryError::Io {
                            path: path.clone(),
                            source,
                        })?
                        .is_dir()
                    {
                        return Err(HistoryError::Corrupt(format!(
                            "temporary metadata path `{}` is unexpectedly a directory",
                            path.display()
                        )));
                    }
                    fs::remove_file(&path).map_err(|source| HistoryError::Io {
                        path: path.clone(),
                        source,
                    })?;
                }
            }
            sync_directory(&directory)?;
        }
        Ok(())
    }

    fn remove_uncommitted_entries(&self) -> Result<(), HistoryError> {
        let directory = self.metadata.join("history/entries");
        for item in fs::read_dir(&directory).map_err(|source| HistoryError::Io {
            path: directory.clone(),
            source,
        })? {
            let item = item.map_err(|source| HistoryError::Io {
                path: directory.clone(),
                source,
            })?;
            let path = item.path();
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                return Err(HistoryError::Corrupt(format!(
                    "history entry path `{}` is not UTF-8",
                    path.display()
                )));
            };
            if !valid_entry_id(id) {
                return Err(HistoryError::Corrupt(format!(
                    "history entry path `{}` has an invalid identity",
                    path.display()
                )));
            }
            if !self.commit_path(id).exists() {
                fs::remove_file(&path).map_err(|source| HistoryError::Io {
                    path: path.clone(),
                    source,
                })?;
            }
        }
        sync_directory(&directory)
    }

    fn entry_path(&self, id: &str) -> PathBuf {
        self.metadata
            .join("history/entries")
            .join(format!("{id}.json"))
    }

    fn commit_path(&self, id: &str) -> PathBuf {
        self.metadata.join("history/committed").join(id)
    }

    fn snapshot_path(&self, revision: &RevisionId) -> PathBuf {
        self.metadata
            .join("history/snapshots")
            .join(format!("{}.json", revision.0))
    }
}

fn valid_entry_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|byte| byte.is_ascii_digit()) && id != "000000000000"
}

fn validate_revision_id(revision: &RevisionId) -> Result<(), HistoryError> {
    if revision.0.len() == 64
        && revision
            .0
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(HistoryError::Corrupt(format!(
            "invalid Revision ID `{revision}`"
        )))
    }
}

fn validate_state_identity(state: &ActiveState) -> Result<(), HistoryError> {
    if state.schema_version != HISTORY_SCHEMA {
        return Err(HistoryError::Corrupt(
            "Active Revision state has an unsupported schema".to_owned(),
        ));
    }
    if !valid_entry_id(&state.active_entry) {
        return Err(HistoryError::Corrupt(format!(
            "Active state has invalid entry identity `{}`",
            state.active_entry
        )));
    }
    validate_revision_id(&state.active_revision)
}

fn validate_entry_identity(entry: &HistoryEntry, expected_id: &str) -> Result<(), HistoryError> {
    if entry.schema_version != HISTORY_SCHEMA || entry.id != expected_id {
        return Err(HistoryError::Corrupt(format!(
            "history entry `{expected_id}` has invalid identity or schema"
        )));
    }
    let expected_sequence = expected_id.parse::<u64>().map_err(|_| {
        HistoryError::Corrupt(format!("history entry `{expected_id}` is not numeric"))
    })?;
    if entry.sequence != expected_sequence {
        return Err(HistoryError::Corrupt(format!(
            "history entry `{expected_id}` has sequence {}",
            entry.sequence
        )));
    }
    validate_revision_id(&entry.revision)?;
    for relation in [entry.parent.as_deref(), entry.restored_from.as_deref()]
        .into_iter()
        .flatten()
    {
        if !valid_entry_id(relation) || relation >= expected_id {
            return Err(HistoryError::Corrupt(format!(
                "history entry `{expected_id}` has invalid prior entry `{relation}`"
            )));
        }
    }
    Ok(())
}

fn read_regular_file(path: &Path) -> Result<Vec<u8>, HistoryError> {
    match read_regular_asset_bytes(path) {
        Ok(bytes) => Ok(bytes),
        Err(AssetReadError::Unsafe(message)) => Err(HistoryError::Corrupt(format!(
            "history asset blob `{}` is unsafe: {message}",
            path.display()
        ))),
        Err(AssetReadError::TooLarge) => Err(HistoryError::Corrupt(format!(
            "history asset blob `{}` exceeds the {MAX_MEDIA_ASSET_BYTES}-byte maximum",
            path.display()
        ))),
        Err(AssetReadError::Unreadable(source)) => Err(HistoryError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_asset_manifest(assets: &BTreeMap<PathBuf, SnapshotAsset>) -> Result<(), HistoryError> {
    if assets.len() > MAX_MEDIA_ASSETS {
        return Err(HistoryError::Corrupt(format!(
            "asset manifest contains {} files; maximum is {MAX_MEDIA_ASSETS}",
            assets.len()
        )));
    }
    let mut total_bytes = 0_u64;
    for (path, asset) in assets {
        if !valid_asset_path(path) || !valid_sha256(&asset.sha256) {
            return Err(HistoryError::Corrupt(format!(
                "asset manifest contains invalid identity `{}`",
                path.display()
            )));
        }
        if asset.byte_len > MAX_MEDIA_ASSET_BYTES {
            return Err(HistoryError::Corrupt(format!(
                "asset manifest file `{}` exceeds the per-file size limit",
                path.display()
            )));
        }
        total_bytes = total_bytes
            .checked_add(asset.byte_len)
            .ok_or_else(|| HistoryError::Corrupt("asset byte count overflow".to_owned()))?;
    }
    if total_bytes > MAX_PROJECT_MEDIA_BYTES {
        return Err(HistoryError::Corrupt(
            "asset manifest exceeds the Project media size limit".to_owned(),
        ));
    }
    Ok(())
}

fn read_asset_manifest_bytes(
    metadata: &Path,
    assets: &BTreeMap<PathBuf, SnapshotAsset>,
) -> Result<BTreeMap<PathBuf, Vec<u8>>, HistoryError> {
    validate_asset_manifest(assets)?;
    let mut bytes = BTreeMap::new();
    for (relative, asset) in assets {
        let path = metadata.join("history/assets").join(&asset.sha256);
        let content = read_regular_file(&path)?;
        if content.len() as u64 != asset.byte_len || hash_bytes(&content) != asset.sha256 {
            return Err(HistoryError::Corrupt(format!(
                "history asset blob {} does not match `{}`",
                asset.sha256,
                relative.display()
            )));
        }
        bytes.insert(relative.clone(), content);
    }
    Ok(bytes)
}

fn read_snapshot_asset_bytes(
    metadata: &Path,
    snapshot: &SourceSnapshot,
) -> Result<BTreeMap<PathBuf, Vec<u8>>, HistoryError> {
    read_asset_manifest_bytes(metadata, &snapshot.assets)
}

fn snapshot_assets(inventory: &AssetInventory) -> BTreeMap<PathBuf, SnapshotAsset> {
    inventory
        .iter()
        .map(|asset| {
            (
                asset.path().to_path_buf(),
                SnapshotAsset {
                    sha256: asset.sha256().to_owned(),
                    byte_len: asset.byte_len(),
                },
            )
        })
        .collect()
}

fn safe_source_path(path: &Path) -> bool {
    if path == Path::new(PROJECT_FILE) {
        return true;
    }
    let reserved_metadata = matches!(
        path.components().next(),
        Some(Component::Normal(component)) if component == METADATA_DIR
    );
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && !reserved_metadata
        && path.extension().and_then(|extension| extension.to_str()) == Some("crump")
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn validate_snapshot_integrity(snapshot: &SourceSnapshot) -> Result<(), HistoryError> {
    if snapshot.schema_version != HISTORY_SCHEMA {
        return Err(HistoryError::Corrupt(
            "source snapshot has an unsupported schema".to_owned(),
        ));
    }
    validate_revision_id(&snapshot.revision)?;
    if !snapshot.sources.contains_key(Path::new(PROJECT_FILE)) {
        return Err(HistoryError::Corrupt(
            "source snapshot is missing `project.crump`".to_owned(),
        ));
    }
    let mut total_bytes = 0usize;
    for (path, source) in &snapshot.sources {
        if !safe_source_path(path) {
            return Err(HistoryError::Corrupt(format!(
                "source snapshot contains unsafe path `{}`",
                path.display()
            )));
        }
        if source.len() as u64 > MAX_SOURCE_BYTES {
            return Err(HistoryError::Corrupt(format!(
                "snapshot source `{}` exceeds the per-file size limit",
                path.display()
            )));
        }
        total_bytes = total_bytes.checked_add(source.len()).ok_or_else(|| {
            HistoryError::Corrupt("source snapshot byte count overflow".to_owned())
        })?;
    }
    if total_bytes > MAX_PROJECT_SOURCE_BYTES {
        return Err(HistoryError::Corrupt(
            "source snapshot exceeds the Project size limit".to_owned(),
        ));
    }
    if snapshot.assets.len() > MAX_MEDIA_ASSETS {
        return Err(HistoryError::Corrupt(format!(
            "source snapshot declares {} media assets; maximum is {MAX_MEDIA_ASSETS}",
            snapshot.assets.len()
        )));
    }
    let mut media_bytes = 0_u64;
    for (path, asset) in &snapshot.assets {
        if !valid_asset_path(path) || !valid_sha256(&asset.sha256) {
            return Err(HistoryError::Corrupt(format!(
                "source snapshot contains invalid asset identity `{}`",
                path.display()
            )));
        }
        if asset.byte_len > MAX_MEDIA_ASSET_BYTES {
            return Err(HistoryError::Corrupt(format!(
                "snapshot asset `{}` exceeds the per-file size limit",
                path.display()
            )));
        }
        media_bytes = media_bytes
            .checked_add(asset.byte_len)
            .ok_or_else(|| HistoryError::Corrupt("asset byte count overflow".to_owned()))?;
    }
    if media_bytes > MAX_PROJECT_MEDIA_BYTES {
        return Err(HistoryError::Corrupt(
            "source snapshot exceeds the Project media size limit".to_owned(),
        ));
    }
    let actual = hash_project_identity(
        &snapshot.sources,
        snapshot
            .assets
            .iter()
            .map(|(path, asset)| (path.as_path(), asset.sha256.as_str(), asset.byte_len)),
    );
    if actual != snapshot.revision {
        return Err(HistoryError::Corrupt(format!(
            "source snapshot identity mismatch: expected {}, found {actual}",
            snapshot.revision
        )));
    }
    Ok(())
}

fn committed_ids(metadata: &Path) -> Result<Vec<String>, HistoryError> {
    let directory = metadata.join("history/committed");
    fs::read_dir(&directory)
        .map_err(|source| HistoryError::Io {
            path: directory.clone(),
            source,
        })?
        .map(|item| {
            let item = item.map_err(|source| HistoryError::Io {
                path: directory.clone(),
                source,
            })?;
            item.file_name()
                .into_string()
                .map_err(|_| HistoryError::Corrupt("history marker is not UTF-8".to_owned()))
        })
        .collect()
}

fn summarize_changes(before: Option<&SourceSnapshot>, after: &SourceSnapshot) -> Vec<SourceChange> {
    let empty_sources = BTreeMap::new();
    let before_sources = before.map_or(&empty_sources, |snapshot| &snapshot.sources);
    let source_paths: BTreeSet<_> = before_sources
        .keys()
        .chain(after.sources.keys())
        .cloned()
        .collect();
    let mut changes = source_paths
        .into_iter()
        .filter_map(|path| {
            let before = before_sources.get(&path);
            let after = after.sources.get(&path);
            if before == after {
                return None;
            }
            let kind = match (before, after) {
                (None, Some(_)) => SourceChangeKind::Added,
                (Some(_), None) => SourceChangeKind::Removed,
                (Some(_), Some(_)) => SourceChangeKind::Modified,
                (None, None) => return None,
            };
            Some(SourceChange {
                path,
                kind,
                before_sha256: before.map(|source| hash_bytes(source.as_bytes())),
                after_sha256: after.map(|source| hash_bytes(source.as_bytes())),
                changed_lines: changed_line_count(
                    before.map(String::as_str).unwrap_or(""),
                    after.map(String::as_str).unwrap_or(""),
                ),
            })
        })
        .collect::<Vec<_>>();

    let empty_assets = BTreeMap::new();
    let before_assets = before.map_or(&empty_assets, |snapshot| &snapshot.assets);
    let asset_paths: BTreeSet<_> = before_assets
        .keys()
        .chain(after.assets.keys())
        .cloned()
        .collect();
    changes.extend(asset_paths.into_iter().filter_map(|path| {
        let before = before_assets.get(&path);
        let after = after.assets.get(&path);
        if before == after {
            return None;
        }
        let kind = match (before, after) {
            (None, Some(_)) => SourceChangeKind::Added,
            (Some(_), None) => SourceChangeKind::Removed,
            (Some(_), Some(_)) => SourceChangeKind::Modified,
            (None, None) => return None,
        };
        Some(SourceChange {
            path,
            kind,
            before_sha256: before.map(|asset| asset.sha256.clone()),
            after_sha256: after.map(|asset| asset.sha256.clone()),
            changed_lines: 0,
        })
    }));
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    changes
}

fn changed_line_count(before: &str, after: &str) -> usize {
    TextDiff::from_lines(before, after)
        .iter_all_changes()
        .filter(|change| change.tag() != ChangeTag::Equal)
        .count()
}

fn diff_snapshots(before: Option<&SourceSnapshot>, after: &SourceSnapshot) -> String {
    let empty_sources = BTreeMap::new();
    let before_sources = before.map_or(&empty_sources, |snapshot| &snapshot.sources);
    let paths: BTreeSet<_> = before_sources
        .keys()
        .chain(after.sources.keys())
        .cloned()
        .collect();
    let mut output = String::new();
    for path in paths {
        let old = before_sources.get(&path).map(String::as_str).unwrap_or("");
        let new = after.sources.get(&path).map(String::as_str).unwrap_or("");
        if old == new {
            continue;
        }
        output.push_str(
            &TextDiff::from_lines(old, new)
                .unified_diff()
                .header(
                    &format!("a/{}", path.display()),
                    &format!("b/{}", path.display()),
                )
                .to_string(),
        );
    }

    let empty_assets = BTreeMap::new();
    let before_assets = before.map_or(&empty_assets, |snapshot| &snapshot.assets);
    let asset_paths: BTreeSet<_> = before_assets
        .keys()
        .chain(after.assets.keys())
        .cloned()
        .collect();
    for path in asset_paths {
        let old = before_assets.get(&path);
        let new = after.assets.get(&path);
        if old == new {
            continue;
        }
        let identity = |asset: Option<&SnapshotAsset>| {
            asset.map_or_else(
                || "absent".to_owned(),
                |asset| format!("sha256 {} ({} bytes)", asset.sha256, asset.byte_len),
            )
        };
        output.push_str(&format!(
            "binary {}: {} -> {}\n",
            path.display(),
            identity(old),
            identity(new)
        ));
    }
    output
}

fn validate_transaction_snapshot(
    metadata: &Path,
    snapshot: &SourceSnapshot,
    materialized_assets: &BTreeMap<PathBuf, SnapshotAsset>,
) -> Result<(), HistoryError> {
    validate_snapshot_integrity(snapshot)?;
    read_asset_manifest_bytes(metadata, materialized_assets)?;
    Ok(())
}

fn reconcile_interrupted_source_set(
    root: &Path,
    target: &BTreeMap<PathBuf, String>,
    other: &BTreeMap<PathBuf, String>,
) -> Result<(), HistoryError> {
    let paths: BTreeSet<_> = target.keys().chain(other.keys()).cloned().collect();
    validate_transaction_paths(&paths)?;
    for relative in paths {
        let path = root.join(&relative);
        ensure_transaction_parent(root, &path)?;
        remove_atomic_temporary_files(&path)?;
        let current = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => return Err(HistoryError::Io { path, source }),
        };
        let target_bytes = target.get(&relative).map(|source| source.as_bytes());
        let other_bytes = other.get(&relative).map(|source| source.as_bytes());
        if current.as_deref() == target_bytes || current.as_deref() != other_bytes {
            continue;
        }
        match target_bytes {
            Some(bytes) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|source| HistoryError::Io {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                write_atomic(&path, bytes)?;
            }
            None => {
                fs::remove_file(&path).map_err(|source| HistoryError::Io {
                    path: path.clone(),
                    source,
                })?;
                if let Some(parent) = path.parent() {
                    sync_directory(parent)?;
                }
            }
        }
    }
    Ok(())
}

fn reconcile_asset_set(
    root: &Path,
    target: &BTreeMap<PathBuf, SnapshotAsset>,
    other: &BTreeMap<PathBuf, SnapshotAsset>,
    interrupted: bool,
) -> Result<(), HistoryError> {
    let paths: BTreeSet<_> = target.keys().chain(other.keys()).cloned().collect();
    for relative in &paths {
        if !valid_asset_path(relative) {
            return Err(HistoryError::UnsupportedRestore(format!(
                "cannot transactionally write media path `{}`",
                relative.display()
            )));
        }
    }
    let metadata = root.join(METADATA_DIR);
    for relative in paths {
        let path = root.join(&relative);
        ensure_transaction_parent(root, &path)?;
        remove_atomic_temporary_files(&path)?;
        let current = match read_project_asset_bytes(root, &relative) {
            Ok(bytes) => Some(bytes),
            Err(AssetReadError::Unreadable(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                None
            }
            Err(AssetReadError::Unreadable(source)) => {
                return Err(HistoryError::Io {
                    path: path.clone(),
                    source,
                });
            }
            Err(AssetReadError::Unsafe(message)) if interrupted => {
                return Err(HistoryError::UnsupportedRestore(format!(
                    "cannot recover media path `{}` safely: {message}",
                    relative.display()
                )));
            }
            Err(AssetReadError::TooLarge) if interrupted => {
                return Err(HistoryError::UnsupportedRestore(format!(
                    "cannot recover media path `{}` safely because it exceeds the {MAX_MEDIA_ASSET_BYTES}-byte maximum",
                    relative.display()
                )));
            }
            Err(AssetReadError::Unsafe(_) | AssetReadError::TooLarge) => Some(Vec::new()),
        };
        let target_bytes = target
            .get(&relative)
            .map(|asset| read_asset_blob(&metadata, asset))
            .transpose()?;
        let other_bytes = other
            .get(&relative)
            .map(|asset| read_asset_blob(&metadata, asset))
            .transpose()?;
        if interrupted
            && (current.as_deref() == target_bytes.as_deref()
                || current.as_deref() != other_bytes.as_deref())
        {
            continue;
        }
        match target_bytes {
            Some(bytes) => write_atomic(&path, &bytes)?,
            None if current.is_some() => {
                fs::remove_file(&path).map_err(|source| HistoryError::Io {
                    path: path.clone(),
                    source,
                })?;
                if let Some(parent) = path.parent() {
                    sync_directory(parent)?;
                }
            }
            None => {}
        }
    }
    Ok(())
}

fn read_asset_blob(metadata: &Path, asset: &SnapshotAsset) -> Result<Vec<u8>, HistoryError> {
    let path = metadata.join("history/assets").join(&asset.sha256);
    let bytes = read_regular_file(&path)?;
    if bytes.len() as u64 != asset.byte_len || hash_bytes(&bytes) != asset.sha256 {
        return Err(HistoryError::Corrupt(format!(
            "history asset blob {} is corrupt",
            asset.sha256
        )));
    }
    Ok(bytes)
}

fn validate_transaction_paths(paths: &BTreeSet<PathBuf>) -> Result<(), HistoryError> {
    for relative in paths {
        if !safe_source_path(relative) {
            return Err(HistoryError::UnsupportedRestore(format!(
                "cannot transactionally write source path `{}`",
                relative.display()
            )));
        }
    }
    Ok(())
}

fn reconcile_source_set(
    root: &Path,
    target: &BTreeMap<PathBuf, String>,
    other: &BTreeMap<PathBuf, String>,
) -> Result<(), HistoryError> {
    let paths: BTreeSet<_> = target.keys().chain(other.keys()).cloned().collect();
    validate_transaction_paths(&paths)?;
    for relative in paths {
        let path = root.join(&relative);
        ensure_transaction_parent(root, &path)?;
        remove_atomic_temporary_files(&path)?;
        if let Some(source) = target.get(&relative) {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| HistoryError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            write_atomic(&path, source.as_bytes())?;
        } else if path.exists() {
            fs::remove_file(&path).map_err(|source| HistoryError::Io {
                path: path.clone(),
                source,
            })?;
            if let Some(parent) = path.parent() {
                sync_directory(parent)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_transaction_parent(root: &Path, path: &Path) -> Result<(), HistoryError> {
    transaction_parent(root, path, false)
}

pub(crate) fn ensure_transaction_parent(root: &Path, path: &Path) -> Result<(), HistoryError> {
    transaction_parent(root, path, true)
}

fn transaction_parent(root: &Path, path: &Path, create_missing: bool) -> Result<(), HistoryError> {
    let parent = path
        .parent()
        .ok_or_else(|| HistoryError::Corrupt(format!("path {} has no parent", path.display())))?;
    let relative_parent = parent.strip_prefix(root).map_err(|_| {
        HistoryError::UnsupportedRestore(format!(
            "transaction path `{}` is outside the Project",
            path.display()
        ))
    })?;
    let mut current = root.to_path_buf();
    for component in relative_parent.components() {
        let Component::Normal(component) = component else {
            return Err(HistoryError::UnsupportedRestore(format!(
                "transaction parent `{}` is not normalized",
                parent.display()
            )));
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(HistoryError::UnsupportedRestore(format!(
                    "transaction parent `{}` traverses a symbolic link",
                    current.display()
                )));
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(HistoryError::UnsupportedRestore(format!(
                    "transaction parent `{}` is not a directory",
                    current.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create_missing => {
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|source| HistoryError::Io {
                    path: current.clone(),
                    source,
                })?;
                if let Some(created_parent) = current.parent() {
                    sync_directory(created_parent)?;
                }
            }
            Err(source) => {
                return Err(HistoryError::Io {
                    path: current.clone(),
                    source,
                });
            }
        }
    }
    Ok(())
}

fn remove_atomic_temporary_files(path: &Path) -> Result<(), HistoryError> {
    let parent = path
        .parent()
        .ok_or_else(|| HistoryError::Corrupt(format!("path {} has no parent", path.display())))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            HistoryError::UnsupportedRestore("source filename is not UTF-8".to_owned())
        })?;
    let prefix = format!(".{filename}.tmp-");
    for item in fs::read_dir(parent).map_err(|source| HistoryError::Io {
        path: parent.to_path_buf(),
        source,
    })? {
        let item = item.map_err(|source| HistoryError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        if item.file_name().to_string_lossy().starts_with(&prefix) {
            let temporary = item.path();
            if item
                .file_type()
                .map_err(|source| HistoryError::Io {
                    path: temporary.clone(),
                    source,
                })?
                .is_dir()
            {
                return Err(HistoryError::Corrupt(format!(
                    "temporary source path `{}` is unexpectedly a directory",
                    temporary.display()
                )));
            }
            fs::remove_file(&temporary).map_err(|source| HistoryError::Io {
                path: temporary,
                source,
            })?;
        }
    }
    sync_directory(parent)
}

/// Atomically replaces one file and synchronizes its containing directory.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), HistoryError> {
    let parent = path
        .parent()
        .ok_or_else(|| HistoryError::Corrupt(format!("path {} has no parent", path.display())))?;
    fs::create_dir_all(parent).map_err(|source| HistoryError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("crump");
    let temporary = parent.join(format!(".{filename}.tmp-{}-{counter}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|source| HistoryError::Io {
                path: temporary.clone(),
                source,
            })?;
        file.write_all(bytes).map_err(|source| HistoryError::Io {
            path: temporary.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| HistoryError::Io {
            path: temporary.clone(),
            source,
        })?;
        fs::rename(&temporary, path).map_err(|source| HistoryError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Atomically creates one file without replacing an existing destination.
pub(crate) fn write_new_atomic(path: &Path, bytes: &[u8]) -> Result<(), HistoryError> {
    let parent = path
        .parent()
        .ok_or_else(|| HistoryError::Corrupt(format!("path {} has no parent", path.display())))?;
    fs::create_dir_all(parent).map_err(|source| HistoryError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("crump");
    let temporary = parent.join(format!(".{filename}.tmp-{}-{counter}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|source| HistoryError::Io {
                path: temporary.clone(),
                source,
            })?;
        file.write_all(bytes).map_err(|source| HistoryError::Io {
            path: temporary.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| HistoryError::Io {
            path: temporary.clone(),
            source,
        })?;
        fs::hard_link(&temporary, path).map_err(|source| HistoryError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        fs::remove_file(&temporary).map_err(|source| HistoryError::Io {
            path: temporary.clone(),
            source,
        })?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn sync_directory(path: &Path) -> Result<(), HistoryError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| HistoryError::Io {
            path: path.to_path_buf(),
            source,
        })
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), HistoryError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(HistoryError::Json)?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, HistoryError> {
    let mut file = File::open(path).map_err(|source| HistoryError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| HistoryError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    serde_json::from_slice(&bytes).map_err(HistoryError::Json)
}

fn read_json_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, HistoryError> {
    match read_json(path) {
        Ok(value) => Ok(Some(value)),
        Err(HistoryError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Persistent-history failures.
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    /// Filesystem operation failed.
    #[error("history I/O failed at {}: {source}", path.display())]
    Io {
        /// Affected path.
        path: PathBuf,
        /// Operating-system error.
        source: std::io::Error,
    },
    /// JSON metadata was malformed.
    #[error("history metadata is invalid: {0}")]
    Json(serde_json::Error),
    /// Current Candidate cannot publish.
    #[error("Candidate Revision is invalid")]
    InvalidCandidate(Vec<Diagnostic>),
    /// Publication provenance was incomplete.
    #[error("invalid publication provenance: {0}")]
    InvalidProvenance(String),
    /// Bundle belongs to another Project root.
    #[error("Candidate Revision belongs to another Project")]
    WrongProject,
    /// Source changed after the publication caller loaded its Candidate.
    #[error("stale Candidate Revision: expected {expected}, found {actual}")]
    StaleCandidate {
        /// Revision the caller intended to publish.
        expected: RevisionId,
        /// Revision now present on disk.
        actual: RevisionId,
    },
    /// Candidate is already the exact Active Revision.
    #[error("Revision {0} is already Active")]
    AlreadyActive(RevisionId),
    /// Entry does not exist or was never committed.
    #[error("unknown Project History Entry `{0}`")]
    UnknownEntry(String),
    /// Metadata failed an identity or ordering invariant.
    #[error("Project History is corrupt: {0}")]
    Corrupt(String),
    /// Source restoration is intentionally unsupported for unsafe shared paths.
    #[error("cannot restore this entry: {0}")]
    UnsupportedRestore(String),
    /// Project loading failed.
    #[error(transparent)]
    Project(#[from] crate::project::ProjectIoError),
    /// Wall-clock formatting failed.
    #[error("could not record history time: {0}")]
    Clock(String),
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

    fn source_with_asset(bytes: &[u8]) -> String {
        SOURCE.replacen(
            "  song {",
            &format!(
                "  asset voice \"Voice\" wav \"media/voice.wav\" sha256 \"{}\"\n  song {{",
                hash_bytes(bytes)
            ),
            1,
        )
    }

    fn bundle_with_asset(before: &ProjectBundle, bytes: &[u8]) -> ProjectBundle {
        let mut sources = before.sources.clone();
        sources.insert(PathBuf::from(PROJECT_FILE), source_with_asset(bytes));
        let assets = BTreeMap::from([(PathBuf::from("media/voice.wav"), bytes.to_vec())]);
        load_project_sources_with_assets(before.root.clone(), sources, &assets)
    }

    const SOURCE: &str = r#"crump 1
project "History" {
  id history
  seed 1
  sample_rate 48000
  render_tail 1s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument drums "Drums" stock_drums {
      polyphony 8
      parameter tone "Tone" ratio 0 1 0.5 0.5
      faceplate {
        control tone "Tone" "Percussion brightness" maps tone
      }
    }
    pattern beat 1bar {
      hit 0beat kick 1
    }
    track drums-track "Drums" instrument drums level -6db pan 0 {
      clip beat-clip pattern beat start 0bar duration 1bar mode once
    }
  }
}
"#;

    #[test]
    fn active_history_owns_and_restores_exact_media_bytes() {
        let directory = tempdir().expect("Project root");
        let wav = directory.path().join("media/voice.wav");
        let first_bytes = write_test_wav(&wav, &[0, 100, -100]);
        fs::write(
            directory.path().join(PROJECT_FILE),
            source_with_asset(&first_bytes),
        )
        .expect("source");
        let first = load_project(directory.path()).expect("first Candidate");
        assert!(!first.diagnostics.has_errors());
        let store = HistoryStore::open(directory.path()).expect("History");
        let first_entry = store
            .publish(
                &first,
                Publication::new(ChangeOrigin::system("test"), "First media", "bootstrap"),
            )
            .expect("publish first");

        let second_bytes = write_test_wav(&wav, &[0, 200, -200, 50]);
        fs::write(
            directory.path().join(PROJECT_FILE),
            source_with_asset(&second_bytes),
        )
        .expect("source");
        let second = load_project(directory.path()).expect("second Candidate");
        let second_entry = store
            .publish(
                &second,
                Publication::new(ChangeOrigin::human("test"), "Second media", "replace"),
            )
            .expect("publish second");
        assert!(
            second_entry
                .changes
                .iter()
                .any(|change| change.path == Path::new("media/voice.wav")
                    && change.kind == SourceChangeKind::Modified
                    && change.changed_lines == 0)
        );
        let diff = store.diff(&second_entry.id).expect("binary diff");
        assert!(diff.contains("binary media/voice.wav: sha256"));
        assert!(diff.contains("bytes) -> sha256"));

        fs::remove_file(&wav).expect("remove mutable Candidate media");
        let active = store
            .active_bundle()
            .expect("Active bundle")
            .expect("Active state");
        assert_eq!(
            active.assets.get("voice").expect("Active asset").bytes(),
            second_bytes
        );

        store
            .restore(
                &first_entry.id,
                ChangeOrigin::human("test"),
                "Restore exact first media",
            )
            .expect("restore");
        assert_eq!(fs::read(&wav).expect("restored WAV"), first_bytes);
        let restored = store
            .active_bundle()
            .expect("restored bundle")
            .expect("restored Active");
        assert_eq!(
            restored
                .assets
                .get("voice")
                .expect("restored asset")
                .bytes(),
            first_bytes
        );
    }

    #[test]
    fn asset_mutation_after_load_is_a_stale_publication() {
        let directory = tempdir().expect("Project root");
        let wav = directory.path().join("media/voice.wav");
        let original = write_test_wav(&wav, &[0, 100]);
        fs::write(
            directory.path().join(PROJECT_FILE),
            source_with_asset(&original),
        )
        .expect("source");
        let loaded = load_project(directory.path()).expect("Candidate");
        let store = HistoryStore::open(directory.path()).expect("History");
        write_test_wav(&wav, &[0, 200]);
        assert!(matches!(
            store.publish(
                &loaded,
                Publication::new(ChangeOrigin::human("test"), "Stale", "mutated media")
            ),
            Err(HistoryError::StaleCandidate { .. })
        ));
    }

    #[test]
    fn tampered_history_asset_blob_is_rejected() {
        let directory = tempdir().expect("Project root");
        let wav = directory.path().join("media/voice.wav");
        let bytes = write_test_wav(&wav, &[0, 100]);
        fs::write(
            directory.path().join(PROJECT_FILE),
            source_with_asset(&bytes),
        )
        .expect("source");
        let loaded = load_project(directory.path()).expect("Candidate");
        let digest = hash_bytes(&bytes);
        let store = HistoryStore::open(directory.path()).expect("History");
        store
            .publish(
                &loaded,
                Publication::new(ChangeOrigin::system("test"), "Media", "bootstrap"),
            )
            .expect("publish");
        fs::write(
            directory.path().join(".crump/history/assets").join(digest),
            b"tampered",
        )
        .expect("tamper blob");
        assert!(store.active_state().expect("metadata state").is_some());
        assert_eq!(store.list().expect("metadata list").len(), 1);
        assert!(matches!(
            store.entry("000000000001"),
            Err(HistoryError::Corrupt(_))
        ));
        assert!(matches!(
            store.active_bundle(),
            Err(HistoryError::Corrupt(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn history_asset_blobs_must_be_owned_regular_files() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("Project root");
        let wav = directory.path().join("media/voice.wav");
        let bytes = write_test_wav(&wav, &[0, 100]);
        fs::write(
            directory.path().join(PROJECT_FILE),
            source_with_asset(&bytes),
        )
        .expect("source");
        let loaded = load_project(directory.path()).expect("Candidate");
        let digest = hash_bytes(&bytes);
        let store = HistoryStore::open(directory.path()).expect("History");
        store
            .publish(
                &loaded,
                Publication::new(ChangeOrigin::system("test"), "Media", "bootstrap"),
            )
            .expect("publish");
        let blob = directory.path().join(".crump/history/assets").join(digest);
        fs::remove_file(&blob).expect("remove owned blob");
        let outside = tempdir().expect("outside history");
        let external_blob = outside.path().join("blob");
        fs::write(&external_blob, bytes).expect("external blob");
        symlink(external_blob, blob).expect("blob symlink");

        assert!(matches!(
            store.active_bundle(),
            Err(HistoryError::Corrupt(_))
        ));
    }

    #[test]
    fn publishes_lists_diffs_and_restores_exact_source() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("project.crump"), SOURCE).unwrap();
        let first = load_project(directory.path()).unwrap();
        let store = HistoryStore::open(directory.path()).unwrap();
        let first_entry = store
            .publish(
                &first,
                Publication::new(
                    ChangeOrigin::system("test"),
                    "Initial state",
                    "Test bootstrap",
                ),
            )
            .unwrap();
        fs::write(
            directory.path().join("project.crump"),
            SOURCE.replace("-6db", "-3db"),
        )
        .unwrap();
        let second = load_project(directory.path()).unwrap();
        let second_entry = store
            .publish(
                &second,
                Publication::new(
                    ChangeOrigin::human("tester"),
                    "Raise drums",
                    "Audible balance change",
                ),
            )
            .unwrap();
        assert!(store.diff(&second_entry.id).unwrap().contains("-3db"));
        let restored = store
            .restore(
                &first_entry.id,
                ChangeOrigin::human("tester"),
                "Undo level experiment",
            )
            .unwrap();
        assert_eq!(
            restored.restored_from.as_deref(),
            Some(first_entry.id.as_str())
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("project.crump")).unwrap(),
            SOURCE
        );
        assert_eq!(store.list().unwrap().len(), 3);
    }

    #[test]
    fn rejects_a_candidate_that_changed_after_loading() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("project.crump");
        fs::write(&path, SOURCE).unwrap();
        let stale = load_project(directory.path()).unwrap();
        fs::write(&path, SOURCE.replace("0.5 0.5", "0.5 0.75")).unwrap();
        let store = HistoryStore::open(directory.path()).unwrap();
        let error = store
            .publish(
                &stale,
                Publication::new(ChangeOrigin::human("test"), "stale", "stale"),
            )
            .unwrap_err();
        assert!(matches!(error, HistoryError::StaleCandidate { .. }));
    }

    #[test]
    fn complete_source_transaction_rolls_back_a_failed_writer() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(PROJECT_FILE);
        fs::write(&path, SOURCE).unwrap();
        let before = load_project(directory.path()).unwrap();
        let store = HistoryStore::open(directory.path()).unwrap();
        store
            .publish(
                &before,
                Publication::new(ChangeOrigin::system("test"), "initial", "initial"),
            )
            .unwrap();

        let changed = SOURCE.replace("0.5 0.5", "0.5 0.75");
        let mut sources = before.sources.clone();
        sources.insert(PathBuf::from(PROJECT_FILE), changed.clone());
        let after = load_project_sources(before.root.clone(), sources);
        let result = store.commit_source_transaction(
            &before,
            &after,
            Publication::new(ChangeOrigin::human("test"), "change", "change"),
            || -> Result<(), HistoryError> {
                write_atomic(&path, changed.as_bytes())?;
                Err(HistoryError::Corrupt("injected writer failure".to_owned()))
            },
        );

        assert!(
            matches!(result, Err(HistoryError::Corrupt(message)) if message == "injected writer failure")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), SOURCE);
        assert_eq!(
            store.active_state().unwrap().unwrap().active_revision,
            before.revision
        );
        assert!(!store.metadata.join(SOURCE_TRANSACTION_FILE).exists());
    }

    #[test]
    fn legacy_sample_free_snapshots_without_an_asset_manifest_still_load() {
        let directory = tempdir().expect("Project root");
        fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
        let bundle = load_project(directory.path()).expect("Candidate");
        let revision = bundle.revision.clone();
        let store = HistoryStore::open(directory.path()).expect("History");
        store
            .publish(
                &bundle,
                Publication::new(ChangeOrigin::system("test"), "Initial", "legacy fixture"),
            )
            .expect("publish");
        drop(store);

        let snapshot_path = directory
            .path()
            .join(METADATA_DIR)
            .join("history/snapshots")
            .join(format!("{revision}.json"));
        let mut snapshot: serde_json::Value =
            serde_json::from_slice(&fs::read(&snapshot_path).expect("snapshot"))
                .expect("snapshot JSON");
        snapshot
            .as_object_mut()
            .expect("snapshot object")
            .remove("assets");
        fs::write(
            &snapshot_path,
            serde_json::to_vec_pretty(&snapshot).expect("snapshot JSON"),
        )
        .expect("legacy snapshot");

        let store = HistoryStore::open(directory.path()).expect("legacy History");
        let active = store
            .active_bundle()
            .expect("Active bundle")
            .expect("Active state");
        assert_eq!(active.revision, revision);
        assert!(active.assets.is_empty());
    }

    #[test]
    fn startup_rolls_back_uncommitted_source_and_asset_bytes() {
        let directory = tempdir().expect("Project root");
        fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
        let external = tempdir().expect("external media");
        let bytes = write_test_wav(&external.path().join("voice.wav"), &[0, 100, -100]);
        let before = load_project(directory.path()).expect("before Candidate");
        let after = bundle_with_asset(&before, &bytes);
        assert!(!after.diagnostics.has_errors());
        let store = HistoryStore::open(directory.path()).expect("History");
        store
            .publish(
                &before,
                Publication::new(ChangeOrigin::system("test"), "Initial", "bootstrap"),
            )
            .expect("publish before");
        store
            .begin_source_transaction(&before, &after)
            .expect("begin transaction");
        write_atomic(
            &directory.path().join(PROJECT_FILE),
            after
                .sources
                .get(Path::new(PROJECT_FILE))
                .expect("after source")
                .as_bytes(),
        )
        .expect("write source");
        write_atomic(&directory.path().join("media/voice.wav"), &bytes).expect("write asset");
        drop(store);

        let _recovered = HistoryStore::open(directory.path()).expect("recover rollback");
        assert_eq!(
            fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source"),
            SOURCE
        );
        assert!(!directory.path().join("media/voice.wav").exists());
    }

    #[test]
    fn startup_can_roll_back_to_an_invalid_candidate_source() {
        let directory = tempdir().expect("Project root");
        let source_path = directory.path().join(PROJECT_FILE);
        fs::write(&source_path, SOURCE).expect("Project source");
        let valid = load_project(directory.path()).expect("valid Candidate");
        let store = HistoryStore::open(directory.path()).expect("History");
        store
            .publish(
                &valid,
                Publication::new(ChangeOrigin::system("test"), "Initial", "bootstrap"),
            )
            .expect("publish");

        let invalid_source = "crump 1\nproject \"unfinished";
        fs::write(&source_path, invalid_source).expect("invalid Candidate source");
        let invalid = load_project(directory.path()).expect("invalid Candidate bundle");
        assert!(invalid.diagnostics.has_errors());
        store
            .begin_source_transaction(&invalid, &valid)
            .expect("journal repair transaction");
        drop(store);

        let _recovered = HistoryStore::open(directory.path()).expect("recover invalid Candidate");
        assert_eq!(
            fs::read_to_string(&source_path).expect("Candidate source"),
            invalid_source
        );
    }

    #[test]
    fn startup_rollback_preserves_invalid_media_bytes_touched_by_restore() {
        let directory = tempdir().expect("Project root");
        let wav = directory.path().join("media/voice.wav");
        let valid_bytes = write_test_wav(&wav, &[0, 100]);
        fs::write(
            directory.path().join(PROJECT_FILE),
            source_with_asset(&valid_bytes),
        )
        .expect("Project source");
        let valid = load_project(directory.path()).expect("valid Candidate");
        let store = HistoryStore::open(directory.path()).expect("History");
        store
            .publish(
                &valid,
                Publication::new(ChangeOrigin::system("test"), "Initial", "bootstrap"),
            )
            .expect("publish");
        let active = store
            .active_bundle()
            .expect("Active bundle")
            .expect("Active state");

        let invalid_bytes = b"not a WAV, but still user-owned bytes";
        fs::write(&wav, invalid_bytes).expect("invalid Candidate media");
        let invalid = load_project(directory.path()).expect("invalid Candidate bundle");
        assert!(invalid.diagnostics.has_errors());
        store
            .begin_source_transaction(&invalid, &active)
            .expect("journal restore transaction");
        write_atomic(&wav, &valid_bytes).expect("partial restore write");
        drop(store);

        let _recovered = HistoryStore::open(directory.path()).expect("recover invalid media");
        assert_eq!(fs::read(&wav).expect("Candidate media"), invalid_bytes);
    }

    #[test]
    fn startup_completes_committed_source_and_asset_bytes() {
        let directory = tempdir().expect("Project root");
        fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
        let external = tempdir().expect("external media");
        let bytes = write_test_wav(&external.path().join("voice.wav"), &[0, 100, -100]);
        let before = load_project(directory.path()).expect("before Candidate");
        let after = bundle_with_asset(&before, &bytes);
        let store = HistoryStore::open(directory.path()).expect("History");
        store
            .publish(
                &before,
                Publication::new(ChangeOrigin::system("test"), "Initial", "bootstrap"),
            )
            .expect("publish before");
        store
            .begin_source_transaction(&before, &after)
            .expect("begin transaction");
        write_atomic(
            &directory.path().join(PROJECT_FILE),
            after
                .sources
                .get(Path::new(PROJECT_FILE))
                .expect("after source")
                .as_bytes(),
        )
        .expect("write source");
        write_atomic(&directory.path().join("media/voice.wav"), &bytes).expect("write asset");
        store
            .publish(
                &after,
                Publication::new(ChangeOrigin::human("test"), "Import", "commit media"),
            )
            .expect("publish after");
        drop(store);

        let recovered = HistoryStore::open(directory.path()).expect("recover commit");
        assert_eq!(
            fs::read(directory.path().join("media/voice.wav")).expect("asset"),
            bytes
        );
        assert_eq!(
            recovered
                .active_bundle()
                .expect("Active bundle")
                .expect("Active state")
                .revision,
            after.revision
        );
    }

    #[test]
    fn startup_rolls_back_an_uncommitted_source_transaction() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("project.crump");
        fs::write(&path, SOURCE).unwrap();
        let before = load_project(directory.path()).unwrap();
        let store = HistoryStore::open(directory.path()).unwrap();
        store
            .publish(
                &before,
                Publication::new(ChangeOrigin::system("test"), "initial", "initial"),
            )
            .unwrap();
        drop(store);

        let changed = SOURCE.replace("0.5 0.5", "0.5 0.75");
        let mut sources = before.sources.clone();
        sources.insert(PathBuf::from("project.crump"), changed.clone());
        let after = load_project_sources(before.root.clone(), sources);
        let store = HistoryStore::open(directory.path()).unwrap();
        store.begin_source_transaction(&before, &after).unwrap();
        write_atomic(&path, changed.as_bytes()).unwrap();
        drop(store);

        let _recovered = HistoryStore::open(directory.path()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), SOURCE);
        assert!(
            !directory
                .path()
                .join(METADATA_DIR)
                .join(SOURCE_TRANSACTION_FILE)
                .exists()
        );
    }

    #[test]
    fn restores_materialized_personal_library_modules_exactly() {
        const MODULE: &str = r#"crump-module 1
instrument shared "Shared" stock_drums {
  polyphony 4
  parameter tone "Tone" ratio 0 1 0.4 0.4
  faceplate {
    control tone "Tone" "Brightness" maps tone
  }
}
"#;
        let directory = tempdir().unwrap();
        let root_source = SOURCE.replacen("crump 1\n", "crump 1\nimport \"library:shared\"\n", 1);
        let module_path = directory.path().join("library/shared.crump");
        fs::create_dir_all(module_path.parent().unwrap()).unwrap();
        fs::write(directory.path().join(PROJECT_FILE), root_source).unwrap();
        fs::write(&module_path, MODULE).unwrap();

        let first = load_project(directory.path()).unwrap();
        assert!(!first.diagnostics.has_errors());
        let store = HistoryStore::open(directory.path()).unwrap();
        let first_entry = store
            .publish(
                &first,
                Publication::new(ChangeOrigin::system("test"), "initial", "initial"),
            )
            .unwrap();
        fs::write(&module_path, MODULE.replace("0.4 0.4", "0.4 0.7")).unwrap();
        let second = load_project(directory.path()).unwrap();
        store
            .publish(
                &second,
                Publication::new(ChangeOrigin::human("test"), "change", "change"),
            )
            .unwrap();

        store
            .restore(
                &first_entry.id,
                ChangeOrigin::human("test"),
                "restore library module",
            )
            .unwrap();
        assert_eq!(fs::read_to_string(module_path).unwrap(), MODULE);
    }

    #[test]
    fn rejects_tampered_content_addressed_snapshots() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join(PROJECT_FILE), SOURCE).unwrap();
        let bundle = load_project(directory.path()).unwrap();
        let revision = bundle.revision.clone();
        let store = HistoryStore::open(directory.path()).unwrap();
        store
            .publish(
                &bundle,
                Publication::new(ChangeOrigin::system("test"), "initial", "initial"),
            )
            .unwrap();
        drop(store);

        let snapshot = directory
            .path()
            .join(METADATA_DIR)
            .join("history/snapshots")
            .join(format!("{revision}.json"));
        let tampered = fs::read_to_string(&snapshot)
            .unwrap()
            .replace("-6db", "-5db");
        fs::write(snapshot, tampered).unwrap();
        let error = match HistoryStore::open(directory.path()) {
            Ok(_) => panic!("tampered snapshot was accepted"),
            Err(error) => error,
        };
        assert!(matches!(error, HistoryError::Corrupt(_)));
    }

    #[test]
    fn rejects_noncanonical_entry_ids_before_path_resolution() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join(PROJECT_FILE), SOURCE).unwrap();
        let store = HistoryStore::open(directory.path()).unwrap();
        let error = store.entry("../../outside").unwrap_err();
        assert!(matches!(error, HistoryError::UnknownEntry(_)));
    }

    #[test]
    fn startup_removes_interrupted_atomic_metadata_temps() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join(PROJECT_FILE), SOURCE).unwrap();
        let store = HistoryStore::open(directory.path()).unwrap();
        drop(store);
        let temporary = directory
            .path()
            .join(METADATA_DIR)
            .join("history/entries/.entry.json.tmp-100-1");
        fs::write(&temporary, b"partial").unwrap();
        let _recovered = HistoryStore::open(directory.path()).unwrap();
        assert!(!temporary.exists());
    }

    #[test]
    fn startup_completes_a_committed_source_transaction() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("project.crump");
        fs::write(&path, SOURCE).unwrap();
        let before = load_project(directory.path()).unwrap();
        let store = HistoryStore::open(directory.path()).unwrap();
        store
            .publish(
                &before,
                Publication::new(ChangeOrigin::system("test"), "initial", "initial"),
            )
            .unwrap();

        let changed = SOURCE.replace("0.5 0.5", "0.5 0.75");
        let mut sources = before.sources.clone();
        sources.insert(PathBuf::from("project.crump"), changed.clone());
        let after = load_project_sources(before.root.clone(), sources);
        store.begin_source_transaction(&before, &after).unwrap();
        write_atomic(&path, changed.as_bytes()).unwrap();
        store
            .publish(
                &after,
                Publication::new(ChangeOrigin::human("test"), "change", "change"),
            )
            .unwrap();
        drop(store);

        let recovered = HistoryStore::open(directory.path()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), changed);
        assert_eq!(recovered.list().unwrap().len(), 2);
        assert!(
            !directory
                .path()
                .join(METADATA_DIR)
                .join(SOURCE_TRANSACTION_FILE)
                .exists()
        );
    }
}
