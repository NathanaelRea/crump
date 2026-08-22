//! Checked, structure-preserving source transactions for Production Views.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{
    diagnostic::SourceLocation,
    history::{
        HistoryEntry, HistoryError, HistoryStore, Publication, ensure_transaction_parent,
        validate_transaction_parent, write_atomic, write_new_atomic,
    },
    media::{AssetReadError, MAX_MEDIA_ASSET_BYTES, read_regular_asset_bytes, valid_asset_path},
    model::{
        AutomationInterpolation, AutomationLane, AutomationPoint, Clip, ClipMode, DrumVoice,
        ParameterTarget, PatternEvent, Processor, ProcessorSetting, Track, Unit, ValueSite,
        format_pitch,
    },
    processor_semantics::resolve_settings,
    project::{
        ProjectBundle, ProjectIoError, RevisionId, load_project, load_project_sources_with_assets,
    },
    time::{MusicalTime, parse_musical_time},
};

/// The smallest checked replacement unit used by a Structured Source Edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedReplacement {
    /// Project-relative authoritative source path.
    pub file: PathBuf,
    /// Inclusive UTF-8 byte offset in the expected Candidate Revision.
    pub start: usize,
    /// Exclusive UTF-8 byte offset.
    pub end: usize,
    /// Exact source text that must still occupy the range.
    pub expected: String,
    /// New exact source text.
    pub replacement: String,
}

impl CheckedReplacement {
    /// Creates a replacement for a parsed literal site.
    #[must_use]
    pub fn at(site: &ValueSite, replacement: impl Into<String>) -> Self {
        Self {
            file: site.file.clone(),
            start: site.start,
            end: site.end,
            expected: site.expected.clone(),
            replacement: replacement.into(),
        }
    }
}

/// One exact binary asset write staged with checked source changes.
#[derive(Debug, Clone)]
pub struct StagedAssetWrite {
    /// Normalized Project-relative destination.
    pub path: PathBuf,
    /// Exact bytes retained through validation and commit.
    pub bytes: Vec<u8>,
    /// Whether an existing declared asset may be atomically replaced.
    pub replace: bool,
}

/// One atomic authoritative source-and-media mutation.
#[derive(Debug, Clone, Default)]
pub struct ProjectMutation {
    /// Checked UTF-8 source replacements.
    pub replacements: Vec<CheckedReplacement>,
    /// Exact binary writes.
    pub asset_writes: Vec<StagedAssetWrite>,
    /// Declared asset paths to remove after source validation.
    pub asset_removals: Vec<PathBuf>,
}

/// Applies one atomic, checked Production View transaction and publishes it.
pub fn apply_structured_edit(
    root: impl AsRef<Path>,
    expected_revision: &RevisionId,
    replacements: Vec<CheckedReplacement>,
    publication: Publication,
) -> Result<HistoryEntry, EditError> {
    apply_project_mutation(
        root,
        expected_revision,
        ProjectMutation {
            replacements,
            ..ProjectMutation::default()
        },
        publication,
    )
}

/// Applies checked source and exact binary changes as one History transaction.
pub fn apply_project_mutation(
    root: impl AsRef<Path>,
    expected_revision: &RevisionId,
    mutation: ProjectMutation,
    publication: Publication,
) -> Result<HistoryEntry, EditError> {
    if mutation.replacements.is_empty()
        && mutation.asset_writes.is_empty()
        && mutation.asset_removals.is_empty()
    {
        return Err(EditError::Empty);
    }
    let store = HistoryStore::open(root.as_ref())?;
    let current = load_project(root.as_ref())?;
    if &current.revision != expected_revision {
        return Err(EditError::StaleRevision {
            expected: expected_revision.clone(),
            actual: current.revision,
        });
    }
    if let Some(active) = store.active_state()?
        && active.active_revision != current.revision
    {
        return Err(EditError::CandidateNotActive {
            active: active.active_revision,
            candidate: current.revision,
        });
    }
    if current.diagnostics.has_errors() {
        return Err(EditError::InvalidBase);
    }

    let changed_sources = apply_in_memory(&current, &mutation.replacements)?;
    let current_project = current.project.as_ref().ok_or(EditError::InvalidBase)?;
    let current_declared_assets = current_project
        .assets
        .iter()
        .map(|asset| asset.path.as_path())
        .collect::<BTreeSet<_>>();
    let mut asset_bytes = current
        .assets
        .iter()
        .map(|asset| (asset.path().to_path_buf(), asset.bytes().to_vec()))
        .collect::<BTreeMap<_, _>>();
    let mut changed_asset_paths = BTreeSet::new();
    for path in &mutation.asset_removals {
        if !valid_asset_path(path) || !changed_asset_paths.insert(path.clone()) {
            return Err(EditError::UnsafePath(path.clone()));
        }
        if !current_declared_assets.contains(path.as_path()) {
            return Err(EditError::UndeclaredAssetPath(path.clone()));
        }
        validate_transaction_parent(&current.root, &current.root.join(path))?;
        asset_bytes.remove(path);
    }
    for write in &mutation.asset_writes {
        if !valid_asset_path(&write.path) || !changed_asset_paths.insert(write.path.clone()) {
            return Err(EditError::UnsafePath(write.path.clone()));
        }
        validate_transaction_parent(&current.root, &current.root.join(&write.path))?;
        if write.replace && !current_declared_assets.contains(write.path.as_path()) {
            return Err(EditError::UndeclaredAssetPath(write.path.clone()));
        }
        if !write.replace && fs::symlink_metadata(current.root.join(&write.path)).is_ok() {
            return Err(EditError::AssetDestinationExists(write.path.clone()));
        }
        asset_bytes.insert(write.path.clone(), write.bytes.clone());
    }
    let candidate = load_project_sources_with_assets(
        current.root.clone(),
        changed_sources.clone(),
        &asset_bytes,
    );
    if candidate.diagnostics.has_errors() || candidate.project.is_none() {
        return Err(EditError::InvalidResult(candidate.diagnostics.into_vec()));
    }
    let Some(candidate_project) = candidate.project.as_ref() else {
        return Err(EditError::InvalidResult(Vec::new()));
    };
    let candidate_declared_assets = candidate_project
        .assets
        .iter()
        .map(|asset| asset.path.as_path())
        .collect::<BTreeSet<_>>();
    if let Some(write) = mutation
        .asset_writes
        .iter()
        .find(|write| !candidate_declared_assets.contains(write.path.as_path()))
    {
        return Err(EditError::UndeclaredAssetPath(write.path.clone()));
    }

    let changed_source_paths = mutation
        .replacements
        .iter()
        .map(|replacement| replacement.file.clone())
        .collect::<Vec<_>>();
    store.commit_source_transaction(&current, &candidate, publication, || {
        write_asset_changes(
            &candidate.root,
            &mutation.asset_writes,
            &mutation.asset_removals,
        )
        .and_then(|()| {
            write_changed_sources(&candidate.root, &changed_source_paths, &changed_sources)
        })
    })
}

/// Copies one external WAV into `media/`, declares it, and publishes atomically.
pub fn import_media_asset(
    root: impl AsRef<Path>,
    expected_revision: &RevisionId,
    source: impl AsRef<Path>,
    id: &str,
    name: &str,
    publication: Publication,
) -> Result<HistoryEntry, EditError> {
    if !valid_source_id(id) {
        return Err(EditError::InvalidValue(
            "media asset id must start with an ASCII letter and contain only letters, digits, `_`, or `-`"
                .to_owned(),
        ));
    }
    if name.trim().is_empty() || name.chars().any(|character| character.is_control()) {
        return Err(EditError::InvalidValue(
            "media asset name must be non-empty and contain no control characters".to_owned(),
        ));
    }
    let source = source.as_ref();
    let bytes = match read_regular_asset_bytes(source) {
        Ok(bytes) => bytes,
        Err(AssetReadError::Unsafe(message)) => {
            return Err(EditError::InvalidValue(format!(
                "media import input `{}` is unsafe: {message}",
                source.display()
            )));
        }
        Err(AssetReadError::Unreadable(source_error)) => {
            return Err(EditError::Io {
                path: source.to_path_buf(),
                source: source_error,
            });
        }
        Err(AssetReadError::TooLarge) => {
            return Err(EditError::InvalidValue(format!(
                "media import exceeds the {MAX_MEDIA_ASSET_BYTES}-byte maximum"
            )));
        }
    };
    let bundle = load_project(root.as_ref())?;
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if project.assets.iter().any(|asset| asset.id == id) {
        return Err(EditError::InvalidValue(format!(
            "media asset id `{id}` already exists"
        )));
    }
    let destination = PathBuf::from(format!("media/{id}.wav"));
    if project.assets.iter().any(|asset| asset.path == destination) {
        return Err(EditError::AssetDestinationExists(destination));
    }
    let (file, position, indentation) = line_insertion(&bundle, &project.song.location, "Song")?;
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let declaration = format!(
        "{indentation}asset {id} {} wav \"{}\" sha256 \"{digest}\"\n",
        quote_source_string(name),
        destination.display()
    );
    apply_project_mutation(
        root,
        expected_revision,
        ProjectMutation {
            replacements: vec![CheckedReplacement {
                file,
                start: position,
                end: position,
                expected: String::new(),
                replacement: declaration,
            }],
            asset_writes: vec![StagedAssetWrite {
                path: destination,
                bytes,
                replace: false,
            }],
            asset_removals: Vec::new(),
        },
        publication,
    )
}

/// Collects every declared WAV into the canonical `media/<asset-id>.wav` layout.
///
/// Exact bytes, hashes, durable IDs, and history provenance are preserved; only
/// normalized source paths and corresponding owned files change.
pub fn collect_media_assets(
    root: impl AsRef<Path>,
    expected_revision: &RevisionId,
    publication: Publication,
) -> Result<HistoryEntry, EditError> {
    let bundle = load_project(root.as_ref())?;
    if bundle.diagnostics.has_errors() {
        return Err(EditError::InvalidBase);
    }
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let mut mutation = ProjectMutation::default();
    for asset in &project.assets {
        let destination = PathBuf::from("media").join(format!("{}.wav", asset.id));
        if asset.path == destination {
            continue;
        }
        let loaded = bundle
            .assets
            .get(&asset.id)
            .ok_or_else(|| EditError::UnknownTarget(format!("media asset `{}`", asset.id)))?;
        mutation.replacements.push(CheckedReplacement::at(
            &asset.path_site,
            quote_source_string(&destination.to_string_lossy()),
        ));
        mutation.asset_writes.push(StagedAssetWrite {
            path: destination,
            bytes: loaded.bytes().to_vec(),
            replace: false,
        });
        mutation.asset_removals.push(asset.path.clone());
    }
    apply_project_mutation(root, expected_revision, mutation, publication)
}

fn valid_source_id(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn quote_source_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Builds a checked Faceplate or exact-Parameter value replacement.
pub fn set_parameter(
    bundle: &ProjectBundle,
    instrument_id: &str,
    parameter_key: &str,
    value: f64,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let instrument = project
        .song
        .instruments
        .iter()
        .find(|instrument| instrument.id == instrument_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("instrument `{instrument_id}`")))?;
    let parameter = instrument
        .parameters
        .iter()
        .find(|parameter| parameter.key == parameter_key)
        .ok_or_else(|| {
            EditError::UnknownTarget(format!("Parameter `{instrument_id}.{parameter_key}`"))
        })?;
    if !value.is_finite() || !(parameter.minimum..=parameter.maximum).contains(&value) {
        return Err(EditError::OutOfRange {
            target: format!("{instrument_id}.{parameter_key}"),
            minimum: parameter.minimum,
            maximum: parameter.maximum,
            unit: parameter.unit,
        });
    }
    Ok(CheckedReplacement::at(
        &parameter.value_site,
        parameter.unit.format_value(value),
    ))
}

/// Builds a checked replacement for any continuous stable Parameter target.
pub fn set_parameter_target(
    bundle: &ProjectBundle,
    target: &ParameterTarget,
    value: f64,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let spec = project
        .parameter_spec(target)
        .ok_or_else(|| EditError::UnknownTarget(target.to_string()))?;
    if !value.is_finite() || !(spec.minimum..=spec.maximum).contains(&value) {
        return Err(EditError::OutOfRange {
            target: target.to_string(),
            minimum: spec.minimum,
            maximum: spec.maximum,
            unit: spec.unit,
        });
    }
    if spec.unit == Unit::Boolean && value != 0.0 && value != 1.0 {
        return Err(EditError::InvalidValue(format!(
            "{} requires exactly false (0) or true (1)",
            target
        )));
    }
    if let Some(site) = parameter_target_site(project, target) {
        return Ok(CheckedReplacement::at(site, spec.unit.format_value(value)));
    }
    if let ParameterTarget::Processor {
        processor,
        parameter,
    } = target
        && parameter == "bypass"
    {
        return insert_processor_value(
            bundle,
            project,
            processor,
            "bypass",
            if value >= 0.5 { "true" } else { "false" },
        );
    }
    if matches!(
        target,
        ParameterTarget::Track {
            parameter: crate::model::TrackParameter::Mute,
            ..
        } | ParameterTarget::Group {
            parameter: crate::model::TrackParameter::Mute,
            ..
        } | ParameterTarget::Return {
            parameter: crate::model::TrackParameter::Mute,
            ..
        }
    ) {
        return insert_channel_mute(bundle, project, target, value >= 0.5);
    }
    Err(EditError::UnknownTarget(target.to_string()))
}

/// Builds a checked exact-literal edit for one processor setting.
///
/// Existing settings replace only their value token. Missing supported settings are inserted at
/// the end of the processor block without rewriting neighboring source or comments.
pub fn set_processor_setting(
    bundle: &ProjectBundle,
    processor_id: &str,
    key: &str,
    value: &str,
) -> Result<CheckedReplacement, EditError> {
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return Err(EditError::InvalidValue(
            "processor setting values must be one non-empty source token".to_owned(),
        ));
    }
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let processor = project_processors(project)
        .find(|candidate| candidate.id == processor_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Processor `{processor_id}`")))?;
    let mut candidate = processor.clone();
    if let Some(setting) = candidate
        .settings
        .iter_mut()
        .find(|setting| setting.key == key)
    {
        setting.value = value.to_owned();
    } else {
        candidate.settings.push(ProcessorSetting {
            key: key.to_owned(),
            value: value.to_owned(),
            value_site: ValueSite::new(
                processor.location.file.clone(),
                processor.location.start,
                processor.location.start,
                value,
            ),
            location: processor.location.clone(),
        });
    }
    resolve_settings(&candidate, project.song.tempo_bpm)
        .map_err(|error| EditError::InvalidValue(error.compile_message(&candidate)))?;
    if let Some(setting) = processor.settings.iter().find(|setting| setting.key == key) {
        Ok(CheckedReplacement::at(&setting.value_site, value))
    } else {
        insert_processor_value(
            bundle,
            project,
            processor_id,
            "setting",
            &format!("{key} {value}"),
        )
    }
}

/// Direction of a one-position processor-chain move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorMove {
    /// Move toward the start of the authored chain.
    Earlier,
    /// Move toward the end of the authored chain.
    Later,
}

/// Moves one processor block by one position while retaining its exact block text and immediately
/// attached leading comments.
pub fn move_processor(
    bundle: &ProjectBundle,
    processor_id: &str,
    direction: ProcessorMove,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (chain, index) = processor_chain(project, processor_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Processor `{processor_id}`")))?;
    let neighbor_index = match direction {
        ProcessorMove::Earlier => index.checked_sub(1),
        ProcessorMove::Later => (index + 1 < chain.len()).then_some(index + 1),
    }
    .ok_or_else(|| {
        EditError::InvalidValue(format!(
            "Processor `{processor_id}` is already at that end of its chain"
        ))
    })?;
    let current = source_block_with_attached_comments(bundle, &chain[index].location)?;
    let neighbor = source_block_with_attached_comments(bundle, &chain[neighbor_index].location)?;
    if current.file != neighbor.file {
        return Err(EditError::InvalidValue(
            "one processor chain cannot be reordered across source files".to_owned(),
        ));
    }
    let source = bundle
        .sources
        .get(&current.file)
        .ok_or_else(|| EditError::UnknownSource(current.file.clone()))?;
    let (first, second) = if current.start < neighbor.start {
        (&current, &neighbor)
    } else {
        (&neighbor, &current)
    };
    if first.end > second.start {
        return Err(EditError::OverlappingOrInvalidRange(current.file));
    }
    let first_text = &source[first.start..first.end];
    let gap = &source[first.end..second.start];
    let second_text = &source[second.start..second.end];
    Ok(CheckedReplacement {
        file: first.file.clone(),
        start: first.start,
        end: second.end,
        expected: source[first.start..second.end].to_owned(),
        replacement: format!("{second_text}{gap}{first_text}"),
    })
}

fn processor_chain<'a>(
    project: &'a crate::model::Project,
    processor_id: &str,
) -> Option<(&'a [Processor], usize)> {
    for chain in project
        .song
        .instruments
        .iter()
        .map(|instrument| instrument.post_processors.as_slice())
        .chain(
            project
                .song
                .tracks
                .iter()
                .map(|track| track.channel.inserts.as_slice()),
        )
        .chain(
            project
                .song
                .groups
                .iter()
                .map(|group| group.channel.inserts.as_slice()),
        )
        .chain(
            project
                .song
                .returns
                .iter()
                .map(|channel| channel.channel.inserts.as_slice()),
        )
        .chain(
            project
                .song
                .master
                .iter()
                .map(|master| master.inserts.as_slice()),
        )
    {
        if let Some(index) = chain
            .iter()
            .position(|processor| processor.id == processor_id)
        {
            return Some((chain, index));
        }
    }
    None
}

fn parameter_target_site<'a>(
    project: &'a crate::model::Project,
    target: &ParameterTarget,
) -> Option<&'a ValueSite> {
    match target {
        ParameterTarget::Instrument {
            instrument,
            parameter,
        } => project
            .song
            .instruments
            .iter()
            .find(|candidate| candidate.id == *instrument)?
            .parameters
            .iter()
            .find(|candidate| candidate.key == *parameter)
            .map(|parameter| &parameter.value_site),
        ParameterTarget::Track { track, parameter } => channel_parameter_site(
            &project
                .song
                .tracks
                .iter()
                .find(|candidate| candidate.id == *track)?
                .channel,
            *parameter,
        ),
        ParameterTarget::Group { group, parameter } => channel_parameter_site(
            &project
                .song
                .groups
                .iter()
                .find(|candidate| candidate.id == *group)?
                .channel,
            *parameter,
        ),
        ParameterTarget::Return {
            r#return,
            parameter,
        } => channel_parameter_site(
            &project
                .song
                .returns
                .iter()
                .find(|candidate| candidate.id == *r#return)?
                .channel,
            *parameter,
        ),
        ParameterTarget::Send { send } => project
            .song
            .tracks
            .iter()
            .map(|channel| &channel.channel)
            .chain(project.song.groups.iter().map(|channel| &channel.channel))
            .chain(project.song.returns.iter().map(|channel| &channel.channel))
            .flat_map(|channel| &channel.sends)
            .find(|candidate| candidate.id == *send)
            .map(|send| &send.level_site),
        ParameterTarget::Processor {
            processor,
            parameter,
        } => {
            let processor =
                project_processors(project).find(|candidate| candidate.id == *processor)?;
            if parameter == "bypass" {
                processor.bypass_site.as_ref()
            } else {
                processor
                    .settings
                    .iter()
                    .find(|setting| setting.key == *parameter)
                    .map(|setting| &setting.value_site)
            }
        }
        ParameterTarget::Master => project
            .song
            .master
            .as_ref()
            .map(|master| &master.level_site),
    }
}

fn project_processors(project: &crate::model::Project) -> impl Iterator<Item = &Processor> {
    project
        .song
        .instruments
        .iter()
        .flat_map(|instrument| &instrument.post_processors)
        .chain(
            project
                .song
                .tracks
                .iter()
                .flat_map(|track| &track.channel.inserts),
        )
        .chain(
            project
                .song
                .groups
                .iter()
                .flat_map(|group| &group.channel.inserts),
        )
        .chain(
            project
                .song
                .returns
                .iter()
                .flat_map(|channel| &channel.channel.inserts),
        )
        .chain(
            project
                .song
                .master
                .iter()
                .flat_map(|master| &master.inserts),
        )
}

fn insert_processor_value(
    bundle: &ProjectBundle,
    project: &crate::model::Project,
    processor_id: &str,
    key: &str,
    value: &str,
) -> Result<CheckedReplacement, EditError> {
    let processor = project_processors(project)
        .find(|candidate| candidate.id == processor_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Processor `{processor_id}`")))?;
    let (file, start, indentation) = block_insertion(
        bundle,
        &processor.location,
        &format!("Processor `{processor_id}`"),
    )?;
    Ok(CheckedReplacement {
        file,
        start,
        end: start,
        expected: String::new(),
        replacement: format!("{indentation}{key} {value}\n"),
    })
}

fn insert_channel_mute(
    bundle: &ProjectBundle,
    project: &crate::model::Project,
    target: &ParameterTarget,
    muted: bool,
) -> Result<CheckedReplacement, EditError> {
    let (location, owner) = match target {
        ParameterTarget::Track { track, .. } => project
            .song
            .tracks
            .iter()
            .find(|candidate| candidate.id == *track)
            .map(|channel| (&channel.location, format!("Track `{track}`"))),
        ParameterTarget::Group { group, .. } => project
            .song
            .groups
            .iter()
            .find(|candidate| candidate.id == *group)
            .map(|channel| (&channel.location, format!("Group `{group}`"))),
        ParameterTarget::Return { r#return, .. } => project
            .song
            .returns
            .iter()
            .find(|candidate| candidate.id == *r#return)
            .map(|channel| {
                (
                    &channel.location,
                    format!("Return `{return}`", r#return = r#return),
                )
            }),
        _ => None,
    }
    .ok_or_else(|| EditError::UnknownTarget(target.to_string()))?;
    let (file, start, indentation) = block_insertion(bundle, location, &owner)?;
    Ok(CheckedReplacement {
        file,
        start,
        end: start,
        expected: String::new(),
        replacement: format!("{indentation}mute {muted}\n"),
    })
}

fn channel_parameter_site(
    channel: &crate::model::ChannelStrip,
    parameter: crate::model::TrackParameter,
) -> Option<&ValueSite> {
    match parameter {
        crate::model::TrackParameter::Level => Some(&channel.level_site),
        crate::model::TrackParameter::Mute => channel.mute_site.as_ref(),
        parameter => channel
            .panner
            .parameter_value_site(parameter)
            .map(|(_, site)| site),
    }
}

/// Builds a checked Track level replacement.
pub fn set_track_level(
    bundle: &ProjectBundle,
    track_id: &str,
    level_db: f64,
) -> Result<CheckedReplacement, EditError> {
    if !level_db.is_finite() || !(-96.0..=12.0).contains(&level_db) {
        return Err(EditError::OutOfRange {
            target: format!("Track `{track_id}` level"),
            minimum: -96.0,
            maximum: 12.0,
            unit: Unit::Decibels,
        });
    }
    let track = bundle
        .project
        .as_ref()
        .ok_or(EditError::InvalidBase)?
        .song
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Track `{track_id}`")))?;
    Ok(CheckedReplacement::at(
        &track.channel.level_site,
        Unit::Decibels.format_value(level_db),
    ))
}

/// Builds a checked Track pan replacement.
pub fn set_track_pan(
    bundle: &ProjectBundle,
    track_id: &str,
    pan: f64,
) -> Result<CheckedReplacement, EditError> {
    if !pan.is_finite() || !(-1.0..=1.0).contains(&pan) {
        return Err(EditError::OutOfRange {
            target: format!("Track `{track_id}` pan"),
            minimum: -1.0,
            maximum: 1.0,
            unit: Unit::Pan,
        });
    }
    let track = bundle
        .project
        .as_ref()
        .ok_or(EditError::InvalidBase)?
        .song
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Track `{track_id}`")))?;
    let parameter = match track.channel.panner {
        crate::model::Panner::MonoPan { .. } => crate::model::TrackParameter::Pan,
        crate::model::Panner::Balance { .. } => crate::model::TrackParameter::Balance,
        crate::model::Panner::StereoPan { .. } => {
            return Err(EditError::WrongTargetKind(
                "stereo_pan requires an explicit left_pan or right_pan target".to_owned(),
            ));
        }
    };
    set_parameter_target(
        bundle,
        &ParameterTarget::Track {
            track: track_id.to_owned(),
            parameter,
        },
        pan,
    )
}

/// Policy for Song-scoped Automation during arrangement-time transformations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrangementAutomation {
    /// Move Automation points with the transformed Timeline material.
    FollowArrangement,
    /// Leave absolute Automation times fixed; invalid post-edit ranges are rejected.
    LockToSong,
}

/// Inserts empty arrangement time and atomically shifts later Clips, Locators, and Automation.
///
/// A Clip spanning the insertion boundary must be split first so this operation never invents
/// hidden source traversal. The required Automation point at Song zero remains pinned at zero.
pub fn insert_arrangement_time(
    bundle: &ProjectBundle,
    at: &str,
    duration: &str,
    automation: ArrangementAutomation,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let at = parse_clip_time(project, at, "arrangement insertion time")?;
    let duration = parse_clip_time(project, duration, "arrangement insertion duration")?;
    plan_arrangement_insertion(bundle, at, duration, automation)
}

/// Deletes a half-open arrangement-time range and closes the resulting gap atomically.
///
/// Fully covered Clips and Automation points are deleted; later objects shift left. Clips that
/// cross either deletion boundary must be split first, making the crop policy explicit in source.
pub fn delete_arrangement_time(
    bundle: &ProjectBundle,
    at: &str,
    duration: &str,
    automation: ArrangementAutomation,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let at = parse_clip_time(project, at, "arrangement deletion time")?;
    let duration = parse_clip_time(project, duration, "arrangement deletion duration")?;
    if at < MusicalTime::ZERO || !duration.is_positive() {
        return Err(EditError::InvalidValue(
            "arrangement deletion requires non-negative time and positive duration".to_owned(),
        ));
    }
    let end = at
        .checked_add(duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if end > project.song.end || duration >= project.song.end {
        return Err(EditError::InvalidValue(
            "arrangement deletion must leave a positive Song and remain within its end".to_owned(),
        ));
    }
    let new_song_end = project
        .song
        .end
        .checked_sub(duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let mut replacements = vec![CheckedReplacement::at(
        &project.song.end_site,
        new_song_end.to_string(),
    )];

    for track in &project.song.tracks {
        for clip in &track.clips {
            let clip_end = clip
                .start
                .checked_add(clip.duration)
                .map_err(|error| EditError::InvalidValue(error.to_string()))?;
            let crosses_left = clip.start < at && clip_end > at;
            let crosses_right = clip.start < end && clip_end > end;
            if crosses_left || crosses_right {
                return Err(EditError::InvalidValue(format!(
                    "Clip `{}` crosses a deletion boundary; split it at {} and/or {} first",
                    clip.id, at, end
                )));
            }
            if clip.start >= at && clip_end <= end {
                replacements.push(line_replacement(bundle, &clip.location)?);
            } else if clip.start >= end {
                let shifted = clip
                    .start
                    .checked_sub(duration)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                replacements.push(CheckedReplacement::at(
                    &clip.sites.start,
                    shifted.to_string(),
                ));
            }
        }
    }
    for locator in &project.song.locators {
        let shifted = if locator.at < at {
            None
        } else if locator.at < end {
            Some(at)
        } else {
            Some(
                locator
                    .at
                    .checked_sub(duration)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?,
            )
        };
        if let Some(shifted) = shifted
            && shifted != locator.at
        {
            replacements.push(CheckedReplacement::at(
                &locator.at_site,
                shifted.to_string(),
            ));
        }
    }
    match automation {
        ArrangementAutomation::FollowArrangement => {
            if at == MusicalTime::ZERO
                && project
                    .song
                    .automations
                    .iter()
                    .any(|lane| !lane.points.iter().any(|point| point.at == end))
            {
                return Err(EditError::InvalidValue(
                    "deleting from Song zero requires every Automation lane to have a point at the deletion end"
                        .to_owned(),
                ));
            }
            for lane in &project.song.automations {
                for point in &lane.points {
                    if point.at >= at && point.at < end {
                        replacements.push(line_replacement(bundle, &point.location)?);
                    } else if point.at >= end {
                        let shifted = point
                            .at
                            .checked_sub(duration)
                            .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                        replacements
                            .push(CheckedReplacement::at(&point.at_site, shifted.to_string()));
                    }
                }
            }
        }
        ArrangementAutomation::LockToSong => {
            if project
                .song
                .automations
                .iter()
                .flat_map(|lane| &lane.points)
                .any(|point| point.at > new_song_end)
            {
                return Err(EditError::InvalidValue(
                    "locked Automation would extend beyond the shortened Song".to_owned(),
                ));
            }
        }
    }
    Ok(replacements)
}

/// Duplicates all complete Clips in a source range into newly inserted arrangement time.
///
/// Copies receive `ORIGINAL-SUFFIX` identities. Clips crossing the source or insertion boundaries
/// must be split first. Locators remain unique navigation objects; Automation follows the insertion
/// policy but is not copied, so section Automation can be authored explicitly after duplication.
pub fn duplicate_arrangement_time(
    bundle: &ProjectBundle,
    source_at: &str,
    duration: &str,
    destination: &str,
    id_suffix: &str,
    automation: ArrangementAutomation,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if !valid_source_id(id_suffix) {
        return Err(EditError::InvalidValue(
            "arrangement duplicate suffix must be source-safe".to_owned(),
        ));
    }
    let source_at = parse_clip_time(project, source_at, "duplicate source time")?;
    let duration = parse_clip_time(project, duration, "duplicate duration")?;
    let destination = parse_clip_time(project, destination, "duplicate destination")?;
    if source_at < MusicalTime::ZERO || destination < MusicalTime::ZERO || !duration.is_positive() {
        return Err(EditError::InvalidValue(
            "arrangement duplication requires non-negative times and positive duration".to_owned(),
        ));
    }
    let source_end = source_at
        .checked_add(duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if source_end > project.song.end || destination > project.song.end {
        return Err(EditError::InvalidValue(
            "arrangement duplicate range and destination must lie within the Song".to_owned(),
        ));
    }
    if destination > source_at && destination < source_end {
        return Err(EditError::InvalidValue(
            "arrangement duplicate destination cannot lie inside its source range".to_owned(),
        ));
    }

    let mut selected = Vec::new();
    let existing_ids = project
        .song
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .map(|clip| clip.id.as_str())
        .collect::<BTreeSet<_>>();
    for track in &project.song.tracks {
        let mut copies = Vec::new();
        for clip in &track.clips {
            let clip_end = clip
                .start
                .checked_add(clip.duration)
                .map_err(|error| EditError::InvalidValue(error.to_string()))?;
            let overlaps = clip.start < source_end && clip_end > source_at;
            let contained = clip.start >= source_at && clip_end <= source_end;
            if overlaps && !contained {
                return Err(EditError::InvalidValue(format!(
                    "Clip `{}` crosses the duplicate source boundary; split it first",
                    clip.id
                )));
            }
            if contained {
                let id = format!("{}-{id_suffix}", clip.id);
                if !valid_source_id(&id) || existing_ids.contains(id.as_str()) {
                    return Err(EditError::InvalidValue(format!(
                        "duplicated Clip ID `{id}` is invalid or already exists"
                    )));
                }
                let relative = clip
                    .start
                    .checked_sub(source_at)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                let start = destination
                    .checked_add(relative)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                copies.push((id, clip, start));
            }
        }
        if !copies.is_empty() {
            selected.push((track, copies));
        }
    }
    if selected.is_empty() {
        return Err(EditError::InvalidValue(
            "arrangement duplicate range contains no complete Clips".to_owned(),
        ));
    }

    let mut replacements = plan_arrangement_insertion(bundle, destination, duration, automation)?;
    for (track, copies) in selected {
        let (file, offset, indentation) = block_insertion(bundle, &track.location, "Track")?;
        let mut replacement = String::new();
        for (id, clip, start) in copies {
            let mut values = ClipValues::from(clip);
            values.id = &id;
            values.start = start;
            validate_clip_values_with_end(
                project,
                values,
                project
                    .song
                    .end
                    .checked_add(duration)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?,
            )?;
            replacement.push_str(&canonical_clip(values, &indentation, &indentation));
            replacement.push('\n');
        }
        replacements.push(CheckedReplacement {
            file,
            start: offset,
            end: offset,
            expected: String::new(),
            replacement,
        });
    }
    Ok(replacements)
}

fn plan_arrangement_insertion(
    bundle: &ProjectBundle,
    at: MusicalTime,
    duration: MusicalTime,
    automation: ArrangementAutomation,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if at < MusicalTime::ZERO || at > project.song.end || !duration.is_positive() {
        return Err(EditError::InvalidValue(
            "arrangement insertion requires a position within the Song and positive duration"
                .to_owned(),
        ));
    }
    let new_song_end = project
        .song
        .end
        .checked_add(duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let mut replacements = vec![CheckedReplacement::at(
        &project.song.end_site,
        new_song_end.to_string(),
    )];
    for track in &project.song.tracks {
        for clip in &track.clips {
            let clip_end = clip
                .start
                .checked_add(clip.duration)
                .map_err(|error| EditError::InvalidValue(error.to_string()))?;
            if clip.start < at && clip_end > at {
                return Err(EditError::InvalidValue(format!(
                    "Clip `{}` crosses the insertion boundary; split it at {at} first",
                    clip.id
                )));
            }
            if clip.start >= at {
                let shifted = clip
                    .start
                    .checked_add(duration)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                replacements.push(CheckedReplacement::at(
                    &clip.sites.start,
                    shifted.to_string(),
                ));
            }
        }
    }
    for locator in &project.song.locators {
        if locator.at >= at {
            let shifted = locator
                .at
                .checked_add(duration)
                .map_err(|error| EditError::InvalidValue(error.to_string()))?;
            replacements.push(CheckedReplacement::at(
                &locator.at_site,
                shifted.to_string(),
            ));
        }
    }
    if automation == ArrangementAutomation::FollowArrangement {
        for point in project
            .song
            .automations
            .iter()
            .flat_map(|lane| &lane.points)
        {
            if point.at >= at && point.at > MusicalTime::ZERO {
                let shifted = point
                    .at
                    .checked_add(duration)
                    .map_err(|error| EditError::InvalidValue(error.to_string()))?;
                replacements.push(CheckedReplacement::at(&point.at_site, shifted.to_string()));
            }
        }
    }
    Ok(replacements)
}

/// Complete source-backed values for one reusable deterministic Groove Template.
#[derive(Debug, Clone)]
pub struct GroovePlacement {
    /// Fresh or existing Groove identity.
    pub id: String,
    /// Positive subdivision grid literal.
    pub grid: String,
    /// Odd-subdivision swing ratio from 0.5 to 0.75.
    pub swing: f64,
    /// Non-negative maximum timing displacement literal.
    pub timing: String,
    /// Maximum normalized signed velocity displacement.
    pub velocity: f64,
    /// Deterministic event inclusion probability.
    pub probability: f64,
    /// Deterministic template seed.
    pub seed: u64,
}

/// Creates one reusable deterministic Groove Template at the Song scope.
pub fn create_groove(
    bundle: &ProjectBundle,
    placement: &GroovePlacement,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if !valid_source_id(&placement.id) {
        return Err(EditError::InvalidValue(
            "Groove Template ID must be source-safe".to_owned(),
        ));
    }
    if project
        .song
        .grooves
        .iter()
        .any(|groove| groove.id == placement.id)
    {
        return Err(EditError::InvalidValue(format!(
            "Groove Template `{}` already exists",
            placement.id
        )));
    }
    let (grid, timing) = parse_groove_placement(project, placement)?;
    let (file, offset, indentation) = block_insertion(bundle, &project.song.location, "Song")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!(
            "{indentation}{}\n",
            canonical_groove(placement, grid, timing)
        ),
    })
}

/// Replaces every checked value of one existing Groove Template without flattening Clips.
pub fn update_groove(
    bundle: &ProjectBundle,
    placement: &GroovePlacement,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let groove = project
        .song
        .grooves
        .iter()
        .find(|groove| groove.id == placement.id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Groove Template `{}`", placement.id)))?;
    let (grid, timing) = parse_groove_placement(project, placement)?;
    Ok(vec![
        CheckedReplacement::at(&groove.sites.grid, grid.to_string()),
        CheckedReplacement::at(&groove.sites.swing, placement.swing.to_string()),
        CheckedReplacement::at(&groove.sites.timing, timing.to_string()),
        CheckedReplacement::at(&groove.sites.velocity, placement.velocity.to_string()),
        CheckedReplacement::at(&groove.sites.probability, placement.probability.to_string()),
        CheckedReplacement::at(&groove.sites.seed, placement.seed.to_string()),
    ])
}

/// Deletes one unused reusable Groove Template declaration.
pub fn delete_groove(
    bundle: &ProjectBundle,
    groove_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let groove = project
        .song
        .grooves
        .iter()
        .find(|groove| groove.id == groove_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Groove Template `{groove_id}`")))?;
    if project
        .song
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .any(|clip| clip.groove.as_deref() == Some(groove_id))
    {
        return Err(EditError::InvalidValue(format!(
            "Groove Template `{groove_id}` is still referenced by a Clip"
        )));
    }
    line_replacement(bundle, &groove.location)
}

/// Assigns or clears one Clip's reusable Groove Template while preserving procedural source.
pub fn set_clip_groove(
    bundle: &ProjectBundle,
    clip_id: &str,
    groove: Option<&str>,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let mut values = ClipValues::from(clip);
    values.groove = groove;
    validate_clip_values(project, values)?;
    replace_clip_declaration(bundle, clip, values)
}

fn parse_groove_placement(
    project: &crate::model::Project,
    placement: &GroovePlacement,
) -> Result<(MusicalTime, MusicalTime), EditError> {
    let grid = parse_clip_time(project, &placement.grid, "Groove grid")?;
    let timing = parse_clip_time(project, &placement.timing, "Groove timing")?;
    if !grid.is_positive() {
        return Err(EditError::InvalidValue(
            "Groove grid must be positive".to_owned(),
        ));
    }
    if !placement.swing.is_finite() || !(0.5..=0.75).contains(&placement.swing) {
        return Err(EditError::InvalidValue(
            "Groove swing must be between 0.5 and 0.75".to_owned(),
        ));
    }
    if timing < MusicalTime::ZERO || !timing.checked_mul(2).is_ok_and(|timing| timing <= grid) {
        return Err(EditError::InvalidValue(
            "Groove timing must be non-negative and at most half the grid".to_owned(),
        ));
    }
    if !placement.velocity.is_finite() || !(0.0..=1.0).contains(&placement.velocity) {
        return Err(EditError::InvalidValue(
            "Groove velocity humanization must be between 0 and 1".to_owned(),
        ));
    }
    if !placement.probability.is_finite() || !(0.0..=1.0).contains(&placement.probability) {
        return Err(EditError::InvalidValue(
            "Groove probability must be between 0 and 1".to_owned(),
        ));
    }
    Ok((grid, timing))
}

fn canonical_groove(placement: &GroovePlacement, grid: MusicalTime, timing: MusicalTime) -> String {
    format!(
        "groove {} grid {} swing {} timing {} velocity {} probability {} seed {}",
        placement.id,
        grid,
        placement.swing,
        timing,
        placement.velocity,
        placement.probability,
        placement.seed
    )
}

/// Complete source-backed values for one procedural Pattern Variation.
#[derive(Debug, Clone)]
pub struct PatternVariationPlacement {
    /// Fresh or existing Pattern identity.
    pub id: String,
    /// Referenced Pattern lineage.
    pub source: String,
    /// Exact cyclic onset rotation literal.
    pub rotate: String,
    /// Whole-semitone note transposition.
    pub transpose_semitones: i16,
    /// Normalized velocity multiplier.
    pub velocity_scale: f64,
    /// Whether onsets are mirrored within the Pattern span.
    pub reverse: bool,
    /// Deterministic event inclusion probability.
    pub probability: f64,
    /// Authored deterministic seed.
    pub seed: u64,
}

/// Creates one procedural Pattern Variation without flattening source Events.
pub fn create_pattern_variation(
    bundle: &ProjectBundle,
    placement: &PatternVariationPlacement,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if !valid_source_id(&placement.id) {
        return Err(EditError::InvalidValue(
            "Pattern Variation ID must be source-safe".to_owned(),
        ));
    }
    if project
        .song
        .patterns
        .iter()
        .any(|pattern| pattern.id == placement.id)
    {
        return Err(EditError::InvalidValue(format!(
            "Pattern `{}` already exists",
            placement.id
        )));
    }
    let (source, rotate) = parse_pattern_variation_placement(project, placement)?;
    if pattern_lineage_reaches(project, &placement.source, &placement.id) {
        return Err(EditError::InvalidValue(
            "Pattern Variation lineage cannot contain a cycle".to_owned(),
        ));
    }
    let (file, offset, indentation) = block_insertion(bundle, &project.song.location, "Song")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: canonical_pattern_variation(placement, source.span, rotate, &indentation),
    })
}

/// Replaces every checked transform of one procedural Pattern Variation.
pub fn update_pattern_variation(
    bundle: &ProjectBundle,
    placement: &PatternVariationPlacement,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == placement.id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", placement.id)))?;
    let variation = pattern.variation.as_ref().ok_or_else(|| {
        EditError::WrongTargetKind(format!("Pattern `{}` is not a Variation", placement.id))
    })?;
    let (source, rotate) = parse_pattern_variation_placement(project, placement)?;
    if source.span != pattern.span {
        return Err(EditError::InvalidValue(
            "Pattern Variation span must equal its source Pattern span".to_owned(),
        ));
    }
    if pattern_lineage_reaches(project, &placement.source, &placement.id) {
        return Err(EditError::InvalidValue(
            "Pattern Variation lineage cannot contain a cycle".to_owned(),
        ));
    }
    Ok(vec![
        CheckedReplacement::at(&variation.sites.source, placement.source.clone()),
        CheckedReplacement::at(&variation.sites.rotate, rotate.to_string()),
        CheckedReplacement::at(
            &variation.sites.transpose,
            format!("{}st", placement.transpose_semitones),
        ),
        CheckedReplacement::at(
            &variation.sites.velocity,
            placement.velocity_scale.to_string(),
        ),
        CheckedReplacement::at(&variation.sites.reverse, placement.reverse.to_string()),
        CheckedReplacement::at(
            &variation.sites.probability,
            placement.probability.to_string(),
        ),
        CheckedReplacement::at(&variation.sites.seed, placement.seed.to_string()),
    ])
}

/// Deletes one unreferenced procedural Pattern Variation declaration.
pub fn delete_pattern_variation(
    bundle: &ProjectBundle,
    pattern_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == pattern_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{pattern_id}`")))?;
    if pattern.variation.is_none() {
        return Err(EditError::WrongTargetKind(format!(
            "Pattern `{pattern_id}` is not a Variation"
        )));
    }
    if project
        .song
        .patterns
        .iter()
        .filter_map(|candidate| candidate.variation.as_ref())
        .any(|variation| variation.source == pattern_id)
        || project
            .song
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .any(|clip| clip.pattern == pattern_id)
    {
        return Err(EditError::InvalidValue(format!(
            "Pattern Variation `{pattern_id}` is still referenced"
        )));
    }
    line_replacement(bundle, &pattern.location)
}

fn parse_pattern_variation_placement<'a>(
    project: &'a crate::model::Project,
    placement: &PatternVariationPlacement,
) -> Result<(&'a crate::model::Pattern, MusicalTime), EditError> {
    let source = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == placement.source)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", placement.source)))?;
    let rotate = parse_clip_time(project, &placement.rotate, "Pattern Variation rotation")?;
    if rotate < MusicalTime::ZERO || rotate >= source.span {
        return Err(EditError::InvalidValue(
            "Pattern Variation rotation must lie inside its half-open span".to_owned(),
        ));
    }
    if !(-127..=127).contains(&placement.transpose_semitones) {
        return Err(EditError::InvalidValue(
            "Pattern Variation transpose must be between -127st and 127st".to_owned(),
        ));
    }
    if !placement.velocity_scale.is_finite() || !(0.0..=2.0).contains(&placement.velocity_scale) {
        return Err(EditError::InvalidValue(
            "Pattern Variation velocity scale must be between 0 and 2".to_owned(),
        ));
    }
    if !placement.probability.is_finite() || !(0.0..=1.0).contains(&placement.probability) {
        return Err(EditError::InvalidValue(
            "Pattern Variation probability must be between 0 and 1".to_owned(),
        ));
    }
    Ok((source, rotate))
}

fn pattern_lineage_reaches(
    project: &crate::model::Project,
    source_id: &str,
    target_id: &str,
) -> bool {
    let mut current = source_id;
    let mut seen = BTreeSet::new();
    loop {
        if current == target_id || !seen.insert(current) {
            return true;
        }
        let Some(variation) = project
            .song
            .patterns
            .iter()
            .find(|pattern| pattern.id == current)
            .and_then(|pattern| pattern.variation.as_ref())
        else {
            return false;
        };
        current = &variation.source;
    }
}

fn canonical_pattern_variation(
    placement: &PatternVariationPlacement,
    span: MusicalTime,
    rotate: MusicalTime,
    indentation: &str,
) -> String {
    format!(
        "{indentation}pattern {} {span} from {} {{\n\
         {indentation}  rotate {rotate}\n\
         {indentation}  transpose {}st\n\
         {indentation}  velocity {}\n\
         {indentation}  reverse {}\n\
         {indentation}  probability {}\n\
         {indentation}  seed {}\n\
         {indentation}}}\n",
        placement.id,
        placement.source,
        placement.transpose_semitones,
        placement.velocity_scale,
        placement.reverse,
        placement.probability,
        placement.seed,
    )
}

/// Complete source-backed values for one durable Pattern Parameter lock.
#[derive(Debug, Clone)]
pub struct ParameterLockPlacement {
    /// Owning source Pattern.
    pub pattern: String,
    /// Fresh or existing lock identity.
    pub id: String,
    /// Pattern-relative onset literal.
    pub at: String,
    /// Instrument Parameter key.
    pub parameter: String,
    /// Physical value literal.
    pub value: String,
}

/// Creates one checked Pattern Parameter lock declaration.
pub fn create_parameter_lock(
    bundle: &ProjectBundle,
    placement: &ParameterLockPlacement,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == placement.pattern)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", placement.pattern)))?;
    if pattern.variation.is_some() {
        return Err(EditError::WrongTargetKind(
            "Parameter locks are authored on source Patterns and inherited by Variations"
                .to_owned(),
        ));
    }
    if !valid_source_id(&placement.id) || pattern.locks.iter().any(|lock| lock.id == placement.id) {
        return Err(EditError::InvalidValue(
            "Parameter lock ID must be fresh and source-safe within the Pattern".to_owned(),
        ));
    }
    let at = validate_parameter_lock_placement(project, pattern, placement, None)?;
    let (file, offset, indentation) = block_insertion(bundle, &pattern.location, "Pattern")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!(
            "{indentation}lock {} at {at} parameter {} value {}\n",
            placement.id, placement.parameter, placement.value
        ),
    })
}

/// Updates one durable Pattern Parameter lock without flattening Events.
pub fn update_parameter_lock(
    bundle: &ProjectBundle,
    placement: &ParameterLockPlacement,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == placement.pattern)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", placement.pattern)))?;
    let lock = pattern
        .locks
        .iter()
        .find(|lock| lock.id == placement.id)
        .ok_or_else(|| {
            EditError::UnknownTarget(format!(
                "Parameter lock `{}` in Pattern `{}`",
                placement.id, placement.pattern
            ))
        })?;
    let at = validate_parameter_lock_placement(project, pattern, placement, Some(&placement.id))?;
    Ok(vec![
        CheckedReplacement::at(&lock.sites.at, at.to_string()),
        CheckedReplacement::at(&lock.sites.parameter, placement.parameter.clone()),
        CheckedReplacement::at(&lock.sites.value, placement.value.clone()),
    ])
}

/// Deletes one durable Pattern Parameter lock declaration.
pub fn delete_parameter_lock(
    bundle: &ProjectBundle,
    pattern_id: &str,
    lock_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let pattern = bundle
        .project
        .as_ref()
        .ok_or(EditError::InvalidBase)?
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == pattern_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{pattern_id}`")))?;
    let lock = pattern
        .locks
        .iter()
        .find(|lock| lock.id == lock_id)
        .ok_or_else(|| {
            EditError::UnknownTarget(format!(
                "Parameter lock `{lock_id}` in Pattern `{pattern_id}`"
            ))
        })?;
    line_replacement(bundle, &lock.location)
}

fn validate_parameter_lock_placement(
    project: &crate::model::Project,
    pattern: &crate::model::Pattern,
    placement: &ParameterLockPlacement,
    updating: Option<&str>,
) -> Result<MusicalTime, EditError> {
    if !valid_source_id(&placement.parameter) || placement.value.is_empty() {
        return Err(EditError::InvalidValue(
            "Parameter lock key must be source-safe and value must be non-empty".to_owned(),
        ));
    }
    let at = parse_clip_time(project, &placement.at, "Parameter lock onset")?;
    if at < MusicalTime::ZERO || at >= pattern.span {
        return Err(EditError::InvalidValue(
            "Parameter lock onset must lie inside the Pattern's half-open span".to_owned(),
        ));
    }
    if pattern.locks.iter().any(|lock| {
        Some(lock.id.as_str()) != updating && lock.at == at && lock.parameter == placement.parameter
    }) {
        return Err(EditError::InvalidValue(
            "Pattern cannot lock the same Parameter more than once at one onset".to_owned(),
        ));
    }
    for track in &project.song.tracks {
        let consumes_pattern = track
            .clips
            .iter()
            .any(|clip| pattern_lineage_reaches(project, &clip.pattern, &pattern.id));
        if !consumes_pattern {
            continue;
        }
        let target = crate::model::ParameterTarget::Instrument {
            instrument: track.instrument.clone(),
            parameter: placement.parameter.clone(),
        };
        let spec = project.parameter_spec(&target).ok_or_else(|| {
            EditError::InvalidValue(format!(
                "Parameter lock targets missing Parameter `{target}`"
            ))
        })?;
        if !spec.automatable {
            return Err(EditError::InvalidValue(format!(
                "Parameter `{target}` is not declared automatable"
            )));
        }
        let value = spec.unit.parse_value(&placement.value).map_err(|error| {
            EditError::InvalidValue(format!(
                "invalid Parameter lock value for {}: {error}",
                spec.unit
            ))
        })?;
        if !(spec.minimum..=spec.maximum).contains(&value) {
            return Err(EditError::InvalidValue(format!(
                "Parameter lock value must be within {} … {}",
                spec.unit.format_value(spec.minimum),
                spec.unit.format_value(spec.maximum)
            )));
        }
    }
    Ok(at)
}

/// Creates one checked named Song Locator declaration.
pub fn create_locator(
    bundle: &ProjectBundle,
    locator_id: &str,
    name: &str,
    at: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if !valid_source_id(locator_id) {
        return Err(EditError::InvalidValue(
            "Locator ID must be source-safe".to_owned(),
        ));
    }
    if project
        .song
        .locators
        .iter()
        .any(|locator| locator.id == locator_id)
    {
        return Err(EditError::InvalidValue(format!(
            "Locator `{locator_id}` already exists"
        )));
    }
    let at = parse_clip_time(project, at, "Locator time")?;
    if at < MusicalTime::ZERO || at > project.song.end {
        return Err(EditError::InvalidValue(
            "Locator time must lie within the Song arrangement".to_owned(),
        ));
    }
    let (file, offset, indentation) = block_insertion(bundle, &project.song.location, "Song")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!(
            "{indentation}locator {locator_id} {} at {at}\n",
            quote_source_string(name)
        ),
    })
}

/// Moves one named Song Locator to an exact arrangement time.
pub fn move_locator(
    bundle: &ProjectBundle,
    locator_id: &str,
    at: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let locator = project
        .song
        .locators
        .iter()
        .find(|locator| locator.id == locator_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Locator `{locator_id}`")))?;
    let at = parse_clip_time(project, at, "Locator time")?;
    if at < MusicalTime::ZERO || at > project.song.end {
        return Err(EditError::InvalidValue(
            "Locator time must lie within the Song arrangement".to_owned(),
        ));
    }
    Ok(CheckedReplacement::at(&locator.at_site, at.to_string()))
}

/// Deletes one complete named Song Locator declaration.
pub fn delete_locator(
    bundle: &ProjectBundle,
    locator_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let locator = project
        .song
        .locators
        .iter()
        .find(|locator| locator.id == locator_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Locator `{locator_id}`")))?;
    line_replacement(bundle, &locator.location)
}

/// Complete source-backed placement for one new Timeline Clip.
#[derive(Debug, Clone)]
pub struct ClipPlacement {
    /// Fresh Song-wide Clip identity.
    pub id: String,
    /// Existing Pattern identity.
    pub pattern: String,
    /// Timeline start literal.
    pub start: String,
    /// Positive duration literal.
    pub duration: String,
    /// Once or looping traversal.
    pub mode: ClipMode,
    /// Pattern-local phase at the Clip's left edge.
    pub source_offset: String,
    /// Whole-semitone transposition.
    pub transpose_semitones: i16,
    /// Event-excitation gain.
    pub gain_db: f64,
    /// Left-edge fade duration.
    pub fade_in: String,
    /// Right-edge fade duration.
    pub fade_out: String,
    /// Optional reusable Groove Template identity.
    pub groove: Option<String>,
}

#[derive(Clone, Copy)]
struct ClipValues<'a> {
    id: &'a str,
    pattern: &'a str,
    start: MusicalTime,
    duration: MusicalTime,
    mode: ClipMode,
    source_offset: MusicalTime,
    transpose_semitones: i16,
    gain_db: f64,
    fade_in: MusicalTime,
    fade_out: MusicalTime,
    groove: Option<&'a str>,
}

impl<'a> From<&'a Clip> for ClipValues<'a> {
    fn from(clip: &'a Clip) -> Self {
        Self {
            id: &clip.id,
            pattern: &clip.pattern,
            start: clip.start,
            duration: clip.duration,
            mode: clip.mode,
            source_offset: clip.source_offset,
            transpose_semitones: clip.transpose_semitones,
            gain_db: clip.gain_db,
            fade_in: clip.fade_in,
            fade_out: clip.fade_out,
            groove: clip.groove.as_deref(),
        }
    }
}

/// Creates one checked Clip declaration at the end of a Track block.
pub fn create_clip(
    bundle: &ProjectBundle,
    track_id: &str,
    placement: &ClipPlacement,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if !valid_source_id(&placement.id) {
        return Err(EditError::InvalidValue(
            "Clip ID must be source-safe".to_owned(),
        ));
    }
    if project
        .song
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .any(|clip| clip.id == placement.id)
    {
        return Err(EditError::InvalidValue(format!(
            "Clip `{}` already exists",
            placement.id
        )));
    }
    let track = project
        .song
        .tracks
        .iter()
        .find(|track| track.id == track_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Track `{track_id}`")))?;
    let values = ClipValues {
        id: &placement.id,
        pattern: &placement.pattern,
        start: parse_clip_time(project, &placement.start, "Clip start")?,
        duration: parse_clip_time(project, &placement.duration, "Clip duration")?,
        mode: placement.mode,
        source_offset: parse_clip_time(project, &placement.source_offset, "Clip source offset")?,
        transpose_semitones: placement.transpose_semitones,
        gain_db: placement.gain_db,
        fade_in: parse_clip_time(project, &placement.fade_in, "Clip fade in")?,
        fade_out: parse_clip_time(project, &placement.fade_out, "Clip fade out")?,
        groove: placement.groove.as_deref(),
    };
    validate_clip_values(project, values)?;
    let (file, offset, indentation) = block_insertion(bundle, &track.location, "Track")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!("{}\n", canonical_clip(values, &indentation, &indentation)),
    })
}

/// Deletes one durably identified Clip declaration.
pub fn delete_clip(bundle: &ProjectBundle, clip_id: &str) -> Result<CheckedReplacement, EditError> {
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    line_replacement(bundle, &clip.location)
}

/// Moves one Clip without changing its Pattern traversal or duration.
pub fn move_clip(
    bundle: &ProjectBundle,
    clip_id: &str,
    start: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let start = parse_clip_time(project, start, "Clip start")?;
    let mut values = ClipValues::from(clip);
    values.start = start;
    validate_clip_values(project, values)?;
    Ok(CheckedReplacement::at(&clip.sites.start, start.to_string()))
}

/// Resizes one Clip's half-open Timeline duration.
pub fn resize_clip(
    bundle: &ProjectBundle,
    clip_id: &str,
    duration: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let duration = parse_clip_time(project, duration, "Clip duration")?;
    let mut values = ClipValues::from(clip);
    values.duration = duration;
    validate_clip_values(project, values)?;
    Ok(CheckedReplacement::at(
        &clip.sites.duration,
        duration.to_string(),
    ))
}

/// Duplicates one Clip with a fresh identity and independent Timeline start.
pub fn duplicate_clip(
    bundle: &ProjectBundle,
    clip_id: &str,
    new_id: &str,
    start: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    ensure_fresh_clip_id(project, new_id)?;
    let (track, clip) = arrangement_clip(bundle, clip_id)?;
    let mut values = ClipValues::from(clip);
    values.id = new_id;
    values.start = parse_clip_time(project, start, "Clip start")?;
    validate_clip_values(project, values)?;
    let (file, offset, indentation) = block_insertion(bundle, &track.location, "Track")?;
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!("{}\n", canonical_clip(values, &indentation, &indentation)),
    })
}

/// Splits one Clip into a shortened left declaration and a fresh right declaration.
///
/// The split must lie after any left-edge fade. The right Clip carries the original right-edge
/// fade, and loop traversal continues from the exact Pattern-local phase.
pub fn split_clip(
    bundle: &ProjectBundle,
    clip_id: &str,
    new_id: &str,
    at: &str,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    ensure_fresh_clip_id(project, new_id)?;
    let (track, clip) = arrangement_clip(bundle, clip_id)?;
    let split = parse_clip_time(project, at, "Clip split")?;
    let end = clip
        .start
        .checked_add(clip.duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if split <= clip.start || split >= end {
        return Err(EditError::InvalidValue(
            "Clip split must lie strictly inside the Clip".to_owned(),
        ));
    }
    let left_duration = split
        .checked_sub(clip.start)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if left_duration < clip.fade_in {
        return Err(EditError::InvalidValue(
            "Clip split cannot cut through its fade-in".to_owned(),
        ));
    }
    let right_duration = end
        .checked_sub(split)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == clip.pattern)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", clip.pattern)))?;
    let advanced = clip
        .source_offset
        .checked_add(left_duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let right_offset = match clip.mode {
        ClipMode::Loop => musical_time_mod(advanced, pattern.span)?,
        ClipMode::Once if advanced < pattern.span => advanced,
        ClipMode::Once => {
            return Err(EditError::InvalidValue(
                "cannot split an exhausted once Clip after its Pattern span".to_owned(),
            ));
        }
    };

    let mut left = ClipValues::from(clip);
    left.duration = left_duration;
    left.fade_out = MusicalTime::ZERO;
    validate_clip_values(project, left)?;
    let mut right = ClipValues::from(clip);
    right.id = new_id;
    right.start = split;
    right.duration = right_duration;
    right.source_offset = right_offset;
    right.fade_in = MusicalTime::ZERO;
    validate_clip_values(project, right)?;

    let indentation = declaration_indentation(bundle, &clip.location)?;
    let left_replacement =
        CheckedReplacement::at(&clip.sites.site, canonical_clip(left, "", &indentation));
    let (file, offset, track_indentation) = block_insertion(bundle, &track.location, "Track")?;
    let right_insertion = CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!(
            "{}\n",
            canonical_clip(right, &track_indentation, &track_indentation)
        ),
    };
    Ok(vec![left_replacement, right_insertion])
}

/// Sets a Clip's Pattern-local source offset.
pub fn set_clip_source_offset(
    bundle: &ProjectBundle,
    clip_id: &str,
    source_offset: &str,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let source_offset = parse_clip_time(project, source_offset, "Clip source offset")?;
    let mut values = ClipValues::from(clip);
    values.source_offset = source_offset;
    validate_clip_values(project, values)?;
    replace_clip_field_or_declaration(
        bundle,
        clip,
        clip.sites.source_offset.as_ref(),
        source_offset.to_string(),
        values,
    )
}

/// Sets a Clip's whole-semitone transposition.
pub fn set_clip_transpose(
    bundle: &ProjectBundle,
    clip_id: &str,
    semitones: i16,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let mut values = ClipValues::from(clip);
    values.transpose_semitones = semitones;
    validate_clip_values(project, values)?;
    replace_clip_field_or_declaration(
        bundle,
        clip,
        clip.sites.transpose.as_ref(),
        format!("{semitones}st"),
        values,
    )
}

/// Sets a Clip's event-excitation gain in decibels.
pub fn set_clip_gain(
    bundle: &ProjectBundle,
    clip_id: &str,
    gain_db: f64,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let mut values = ClipValues::from(clip);
    values.gain_db = gain_db;
    validate_clip_values(project, values)?;
    replace_clip_field_or_declaration(
        bundle,
        clip,
        clip.sites.gain.as_ref(),
        Unit::Decibels.format_value(gain_db),
        values,
    )
}

/// Sets both source-backed Clip fade durations.
pub fn set_clip_fades(
    bundle: &ProjectBundle,
    clip_id: &str,
    fade_in: &str,
    fade_out: &str,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, clip) = arrangement_clip(bundle, clip_id)?;
    let fade_in = parse_clip_time(project, fade_in, "Clip fade in")?;
    let fade_out = parse_clip_time(project, fade_out, "Clip fade out")?;
    let mut values = ClipValues::from(clip);
    values.fade_in = fade_in;
    values.fade_out = fade_out;
    validate_clip_values(project, values)?;
    if let (Some(fade_in_site), Some(fade_out_site)) =
        (clip.sites.fade_in.as_ref(), clip.sites.fade_out.as_ref())
    {
        Ok(vec![
            CheckedReplacement::at(fade_in_site, fade_in.to_string()),
            CheckedReplacement::at(fade_out_site, fade_out.to_string()),
        ])
    } else {
        Ok(vec![replace_clip_declaration(bundle, clip, values)?])
    }
}

fn replace_clip_field_or_declaration(
    bundle: &ProjectBundle,
    clip: &Clip,
    site: Option<&ValueSite>,
    literal: String,
    values: ClipValues<'_>,
) -> Result<Vec<CheckedReplacement>, EditError> {
    if let Some(site) = site {
        Ok(vec![CheckedReplacement::at(site, literal)])
    } else {
        Ok(vec![replace_clip_declaration(bundle, clip, values)?])
    }
}

fn replace_clip_declaration(
    bundle: &ProjectBundle,
    clip: &Clip,
    values: ClipValues<'_>,
) -> Result<CheckedReplacement, EditError> {
    let indentation = declaration_indentation(bundle, &clip.location)?;
    Ok(CheckedReplacement::at(
        &clip.sites.site,
        canonical_clip(values, "", &indentation),
    ))
}

fn arrangement_clip<'a>(
    bundle: &'a ProjectBundle,
    clip_id: &str,
) -> Result<(&'a Track, &'a Clip), EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    project
        .song
        .tracks
        .iter()
        .find_map(|track| {
            track
                .clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .map(|clip| (track, clip))
        })
        .ok_or_else(|| EditError::UnknownTarget(format!("Clip `{clip_id}`")))
}

fn ensure_fresh_clip_id(project: &crate::model::Project, clip_id: &str) -> Result<(), EditError> {
    if !valid_source_id(clip_id) {
        return Err(EditError::InvalidValue(
            "Clip ID must be source-safe".to_owned(),
        ));
    }
    if project
        .song
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .any(|clip| clip.id == clip_id)
    {
        return Err(EditError::InvalidValue(format!(
            "Clip `{clip_id}` already exists"
        )));
    }
    Ok(())
}

fn parse_clip_time(
    project: &crate::model::Project,
    value: &str,
    label: &str,
) -> Result<MusicalTime, EditError> {
    parse_musical_time(value, project.song.meter)
        .map_err(|error| EditError::InvalidValue(format!("{label}: {error}")))
}

fn validate_clip_values(
    project: &crate::model::Project,
    values: ClipValues<'_>,
) -> Result<(), EditError> {
    validate_clip_values_with_end(project, values, project.song.end)
}

fn validate_clip_values_with_end(
    project: &crate::model::Project,
    values: ClipValues<'_>,
    arrangement_end: MusicalTime,
) -> Result<(), EditError> {
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == values.pattern)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{}`", values.pattern)))?;
    if values.start < MusicalTime::ZERO || !values.duration.is_positive() {
        return Err(EditError::InvalidValue(
            "Clip start must be non-negative and duration must be positive".to_owned(),
        ));
    }
    let end = values
        .start
        .checked_add(values.duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if end > arrangement_end {
        return Err(EditError::InvalidValue(
            "Clip extends beyond the Song arrangement end".to_owned(),
        ));
    }
    if values.source_offset < MusicalTime::ZERO || values.source_offset >= pattern.span {
        return Err(EditError::InvalidValue(
            "Clip source offset must lie inside the Pattern span".to_owned(),
        ));
    }
    if !(-127..=127).contains(&values.transpose_semitones) {
        return Err(EditError::InvalidValue(
            "Clip transpose must be between -127st and 127st".to_owned(),
        ));
    }
    if pattern.events.iter().any(|event| {
        matches!(
            event,
            PatternEvent::Note { pitch, .. }
                if !(0..=127).contains(&(i16::from(*pitch) + values.transpose_semitones))
        )
    }) {
        return Err(EditError::InvalidValue(
            "Clip transpose moves a note outside the MIDI pitch domain".to_owned(),
        ));
    }
    if let Some(groove) = values.groove
        && !project
            .song
            .grooves
            .iter()
            .any(|template| template.id == groove)
    {
        return Err(EditError::UnknownTarget(format!(
            "Groove Template `{groove}`"
        )));
    }
    if !values.gain_db.is_finite() || !(-96.0..=24.0).contains(&values.gain_db) {
        return Err(EditError::OutOfRange {
            target: "Clip gain".to_owned(),
            minimum: -96.0,
            maximum: 24.0,
            unit: Unit::Decibels,
        });
    }
    let fades = values
        .fade_in
        .checked_add(values.fade_out)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if values.fade_in < MusicalTime::ZERO
        || values.fade_out < MusicalTime::ZERO
        || fades > values.duration
    {
        return Err(EditError::InvalidValue(
            "Clip fades must be non-negative and cannot exceed Clip duration".to_owned(),
        ));
    }
    Ok(())
}

fn declaration_indentation(
    bundle: &ProjectBundle,
    location: &SourceLocation,
) -> Result<String, EditError> {
    let source = bundle
        .sources
        .get(&location.file)
        .ok_or_else(|| EditError::UnknownSource(location.file.clone()))?;
    let line_start = source[..location.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let indentation = source
        .get(line_start..location.start)
        .ok_or_else(|| EditError::OverlappingOrInvalidRange(location.file.clone()))?;
    if !indentation.chars().all(char::is_whitespace) {
        return Err(EditError::InvalidValue(
            "Clip declaration indentation is not whitespace".to_owned(),
        ));
    }
    Ok(indentation.to_owned())
}

fn canonical_clip(values: ClipValues<'_>, first_indent: &str, indentation: &str) -> String {
    let mode = match values.mode {
        ClipMode::Once => "once",
        ClipMode::Loop => "loop",
    };
    let child = format!("{indentation}  ");
    let mut source = format!(
        "{first_indent}clip {} pattern {} start {} duration {} mode {mode} {{\n{child}offset {}\n{child}transpose {}st\n{child}gain {}\n{child}fade {} {}",
        values.id,
        values.pattern,
        values.start,
        values.duration,
        values.source_offset,
        values.transpose_semitones,
        Unit::Decibels.format_value(values.gain_db),
        values.fade_in,
        values.fade_out,
    );
    if let Some(groove) = values.groove {
        source.push_str(&format!("\n{child}groove {groove}"));
    }
    source.push_str(&format!("\n{indentation}}}"));
    source
}

fn musical_time_mod(value: MusicalTime, modulus: MusicalTime) -> Result<MusicalTime, EditError> {
    if !modulus.is_positive() || value < MusicalTime::ZERO {
        return Err(EditError::InvalidValue(
            "musical modulo requires non-negative time and positive span".to_owned(),
        ));
    }
    let numerator = i128::from(value.numerator()) * i128::from(modulus.denominator());
    let denominator = i128::from(value.denominator()) * i128::from(modulus.numerator());
    let quotient = u64::try_from(numerator / denominator)
        .map_err(|_| EditError::InvalidValue("musical modulo overflows".to_owned()))?;
    let consumed = modulus
        .checked_mul(quotient)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    value
        .checked_sub(consumed)
        .map_err(|error| EditError::InvalidValue(error.to_string()))
}

/// Builds a checked Event Grid pitch replacement.
pub fn set_note_pitch(
    bundle: &ProjectBundle,
    pattern_id: &str,
    event_index: usize,
    pitch: u8,
) -> Result<CheckedReplacement, EditError> {
    match pattern_event(bundle, pattern_id, event_index)?.1 {
        PatternEvent::Note { pitch_site, .. } => {
            Ok(CheckedReplacement::at(pitch_site, format_pitch(pitch)))
        }
        PatternEvent::Hit { .. } | PatternEvent::Sample { .. } => Err(EditError::WrongTargetKind(
            "this Event has no pitched-note value".to_owned(),
        )),
    }
}

/// Builds a checked Event Grid onset replacement in authored musical units.
pub fn set_event_at(
    bundle: &ProjectBundle,
    pattern_id: &str,
    event_index: usize,
    value: &str,
) -> Result<CheckedReplacement, EditError> {
    let (pattern, event) = pattern_event(bundle, pattern_id, event_index)?;
    let meter = bundle
        .project
        .as_ref()
        .ok_or(EditError::InvalidBase)?
        .song
        .meter;
    let parsed = parse_musical_time(value, meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if parsed < crate::time::MusicalTime::ZERO || parsed >= pattern.span {
        return Err(EditError::InvalidValue(format!(
            "Event onset must be within Pattern `{pattern_id}` range 0..{}",
            pattern.span
        )));
    }
    let site = match event {
        PatternEvent::Note { at_site, .. } | PatternEvent::Hit { at_site, .. } => at_site,
        PatternEvent::Sample { sites, .. } => &sites.at,
    };
    Ok(CheckedReplacement::at(site, parsed.to_string()))
}

/// Builds a checked pitched-note duration replacement.
pub fn set_note_duration(
    bundle: &ProjectBundle,
    pattern_id: &str,
    event_index: usize,
    value: &str,
) -> Result<CheckedReplacement, EditError> {
    let meter = bundle
        .project
        .as_ref()
        .ok_or(EditError::InvalidBase)?
        .song
        .meter;
    let parsed = parse_musical_time(value, meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if !parsed.is_positive() {
        return Err(EditError::InvalidValue(
            "note duration must be greater than zero".to_owned(),
        ));
    }
    match pattern_event(bundle, pattern_id, event_index)?.1 {
        PatternEvent::Note { duration_site, .. } => {
            Ok(CheckedReplacement::at(duration_site, parsed.to_string()))
        }
        PatternEvent::Sample { sites, .. } => {
            Ok(CheckedReplacement::at(&sites.duration, parsed.to_string()))
        }
        PatternEvent::Hit { .. } => Err(EditError::WrongTargetKind(
            "a percussion hit has no duration".to_owned(),
        )),
    }
}

/// Builds a checked note-or-hit velocity replacement.
pub fn set_event_velocity(
    bundle: &ProjectBundle,
    pattern_id: &str,
    event_index: usize,
    velocity: f64,
) -> Result<CheckedReplacement, EditError> {
    if !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
        return Err(EditError::OutOfRange {
            target: format!("Event {event_index} velocity"),
            minimum: 0.0,
            maximum: 1.0,
            unit: Unit::Ratio,
        });
    }
    let site = match pattern_event(bundle, pattern_id, event_index)?.1 {
        PatternEvent::Note { velocity_site, .. } | PatternEvent::Hit { velocity_site, .. } => {
            velocity_site
        }
        PatternEvent::Sample { sites, .. } => &sites.velocity,
    };
    Ok(CheckedReplacement::at(
        site,
        Unit::Ratio.format_value(velocity),
    ))
}

/// Source-backed arguments for placing one durable sample occurrence.
#[derive(Debug, Clone)]
pub struct SamplePlacement {
    /// Durable occurrence identity within the Pattern.
    pub id: String,
    /// Durable mapping identity within the Track's sampler Instrument.
    pub mapping: String,
    /// Pattern-local onset in authored musical units.
    pub at: String,
    /// Occurrence duration in authored musical units.
    pub duration: String,
    /// Inclusive source start in original asset frames.
    pub start_frame: u64,
    /// Exclusive source end in original asset frames.
    pub end_frame: u64,
    /// Occurrence gain trim in decibels.
    pub gain_db: f64,
    /// Occurrence pan in the inclusive -1..=1 domain.
    pub pan: f64,
    /// Click-prevention fade-in in original asset frames.
    pub fade_in_frames: u32,
    /// Click-prevention fade-out in original asset frames.
    pub fade_out_frames: u32,
    /// Occurrence velocity in the inclusive 0..=1 domain.
    pub velocity: f64,
}

/// Places one non-destructive sample occurrence into a Pattern block.
pub fn place_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    placement: &SamplePlacement,
) -> Result<CheckedReplacement, EditError> {
    if !valid_source_id(&placement.id) || !valid_source_id(&placement.mapping) {
        return Err(EditError::InvalidValue(
            "sample occurrence and mapping IDs must be source-safe".to_owned(),
        ));
    }
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == pattern_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{pattern_id}`")))?;
    if pattern.events.iter().any(
        |event| matches!(event, PatternEvent::Sample { id, .. } if id == placement.id.as_str()),
    ) {
        return Err(EditError::InvalidValue(format!(
            "sample occurrence `{}` already exists in Pattern `{pattern_id}`",
            placement.id
        )));
    }
    let at = parse_musical_time(&placement.at, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let duration = parse_musical_time(&placement.duration, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    validate_sample_values(
        pattern,
        at,
        duration,
        placement.start_frame,
        placement.end_frame,
        placement.gain_db,
        placement.pan,
        placement.fade_in_frames,
        placement.fade_out_frames,
        placement.velocity,
    )?;
    let event = canonical_sample_event(
        &placement.id,
        at,
        &placement.mapping,
        duration,
        placement.start_frame,
        placement.end_frame,
        placement.gain_db,
        placement.pan,
        placement.fade_in_frames,
        placement.fade_out_frames,
        placement.velocity,
    );
    let (file, start, indentation) = block_insertion(bundle, &pattern.location, "Pattern")?;
    Ok(CheckedReplacement {
        file,
        start,
        end: start,
        expected: String::new(),
        replacement: format!("{indentation}{event}\n"),
    })
}

/// Moves one durable sample occurrence without depending on Event ordering.
pub fn move_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    value: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (pattern, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let parsed = parse_musical_time(value, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if parsed < crate::time::MusicalTime::ZERO || parsed >= pattern.span {
        return Err(EditError::InvalidValue(format!(
            "sample onset must be within Pattern `{pattern_id}` range 0..{}",
            pattern.span
        )));
    }
    let PatternEvent::Sample { sites, .. } = event else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    Ok(CheckedReplacement::at(&sites.at, parsed.to_string()))
}

/// Trims one sample occurrence to an explicit half-open source-frame region.
pub fn trim_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    start_frame: u64,
    end_frame: u64,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample {
        fade_in_frames,
        fade_out_frames,
        sites,
        ..
    } = event
    else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    validate_source_region(start_frame, end_frame, *fade_in_frames, *fade_out_frames)?;
    Ok(vec![
        CheckedReplacement::at(&sites.start, start_frame.to_string()),
        CheckedReplacement::at(&sites.end, end_frame.to_string()),
    ])
}

/// Slips one occurrence through its source while preserving selected length.
pub fn slip_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    frame_delta: i64,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample {
        start_frame,
        end_frame,
        ..
    } = event
    else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    let shifted = |frame: u64| {
        if frame_delta >= 0 {
            frame.checked_add(frame_delta as u64)
        } else {
            frame.checked_sub(frame_delta.unsigned_abs())
        }
        .ok_or_else(|| EditError::InvalidValue("sample slip exceeds source-frame range".to_owned()))
    };
    trim_sample_occurrence(
        bundle,
        pattern_id,
        occurrence_id,
        shifted(*start_frame)?,
        shifted(*end_frame)?,
    )
}

/// Sets click-prevention fades for one durable sample occurrence.
pub fn fade_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    fade_in_frames: u32,
    fade_out_frames: u32,
) -> Result<Vec<CheckedReplacement>, EditError> {
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample {
        start_frame,
        end_frame,
        sites,
        ..
    } = event
    else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    validate_source_region(*start_frame, *end_frame, fade_in_frames, fade_out_frames)?;
    Ok(vec![
        CheckedReplacement::at(&sites.fade_in, fade_in_frames.to_string()),
        CheckedReplacement::at(&sites.fade_out, fade_out_frames.to_string()),
    ])
}

/// Sets one occurrence's gain trim in decibels.
pub fn set_sample_occurrence_gain(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    gain_db: f64,
) -> Result<CheckedReplacement, EditError> {
    if !gain_db.is_finite() || !(-96.0..=24.0).contains(&gain_db) {
        return Err(EditError::InvalidValue(
            "sample gain must be -96db..=24db".to_owned(),
        ));
    }
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample { sites, .. } = event else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    Ok(CheckedReplacement::at(
        &sites.gain,
        Unit::Decibels.format_value(gain_db),
    ))
}

/// Sets one occurrence's stereo pan.
pub fn set_sample_occurrence_pan(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    pan: f64,
) -> Result<CheckedReplacement, EditError> {
    if !pan.is_finite() || !(-1.0..=1.0).contains(&pan) {
        return Err(EditError::InvalidValue(
            "sample pan must be -1..=1".to_owned(),
        ));
    }
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample { sites, .. } = event else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    Ok(CheckedReplacement::at(
        &sites.pan,
        Unit::Pan.format_value(pan),
    ))
}

/// Duplicates one occurrence under a new durable ID and onset.
pub fn duplicate_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    new_id: &str,
    at: &str,
) -> Result<CheckedReplacement, EditError> {
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    let PatternEvent::Sample {
        mapping,
        duration,
        start_frame,
        end_frame,
        gain_db,
        pan,
        fade_in_frames,
        fade_out_frames,
        velocity,
        ..
    } = event
    else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    place_sample_occurrence(
        bundle,
        pattern_id,
        &SamplePlacement {
            id: new_id.to_owned(),
            mapping: mapping.clone(),
            at: at.to_owned(),
            duration: duration.to_string(),
            start_frame: *start_frame,
            end_frame: *end_frame,
            gain_db: *gain_db,
            pan: *pan,
            fade_in_frames: *fade_in_frames,
            fade_out_frames: *fade_out_frames,
            velocity: *velocity,
        },
    )
}

/// Splits one occurrence into two source-backed occurrences in one checked edit.
pub fn split_sample_occurrence(
    bundle: &ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
    new_id: &str,
    at: &str,
    source_frame: u64,
) -> Result<CheckedReplacement, EditError> {
    if !valid_source_id(new_id) {
        return Err(EditError::InvalidValue(
            "new sample occurrence ID is invalid".to_owned(),
        ));
    }
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (_, event) = sample_occurrence(bundle, pattern_id, occurrence_id)?;
    if sample_occurrence(bundle, pattern_id, new_id).is_ok() {
        return Err(EditError::InvalidValue(format!(
            "sample occurrence `{new_id}` already exists"
        )));
    }
    let PatternEvent::Sample {
        id,
        at: original_at,
        mapping,
        duration,
        start_frame,
        end_frame,
        gain_db,
        pan,
        fade_in_frames,
        fade_out_frames,
        velocity,
        sites,
        ..
    } = event
    else {
        unreachable!("sample_occurrence returns a sample Event");
    };
    let split_at = parse_musical_time(at, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let event_end = original_at
        .checked_add(*duration)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if split_at <= *original_at
        || split_at >= event_end
        || source_frame <= *start_frame
        || source_frame >= *end_frame
    {
        return Err(EditError::InvalidValue(
            "split must lie strictly inside both the occurrence time and source region".to_owned(),
        ));
    }
    let left_duration = split_at
        .checked_sub(*original_at)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let right_duration = event_end
        .checked_sub(split_at)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    let left_frames = source_frame - *start_frame;
    let right_frames = *end_frame - source_frame;
    let left_fade_in = u64::from(*fade_in_frames).min(left_frames) as u32;
    let left_fade_out =
        u64::from(*fade_out_frames).min(left_frames - u64::from(left_fade_in)) as u32;
    let right_fade_out = u64::from(*fade_out_frames).min(right_frames) as u32;
    let right_fade_in =
        u64::from(*fade_in_frames).min(right_frames - u64::from(right_fade_out)) as u32;
    let left = canonical_sample_event(
        id,
        *original_at,
        mapping,
        left_duration,
        *start_frame,
        source_frame,
        *gain_db,
        *pan,
        left_fade_in,
        left_fade_out,
        *velocity,
    );
    let right = canonical_sample_event(
        new_id,
        split_at,
        mapping,
        right_duration,
        source_frame,
        *end_frame,
        *gain_db,
        *pan,
        right_fade_in,
        right_fade_out,
        *velocity,
    );
    let source = bundle
        .source(&sites.site.file)
        .ok_or_else(|| EditError::UnknownSource(sites.site.file.clone()))?;
    let line_start = source[..sites.site.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let indentation = &source[line_start..sites.site.start];
    Ok(CheckedReplacement::at(
        &sites.site,
        format!("{left}\n{indentation}{right}"),
    ))
}

#[allow(clippy::too_many_arguments)]
fn canonical_sample_event(
    id: &str,
    at: crate::time::MusicalTime,
    mapping: &str,
    duration: crate::time::MusicalTime,
    start_frame: u64,
    end_frame: u64,
    gain_db: f64,
    pan: f64,
    fade_in_frames: u32,
    fade_out_frames: u32,
    velocity: f64,
) -> String {
    format!(
        "sample {id} {at} mapping {mapping} duration {duration} start {start_frame} end {end_frame} gain {} pan {} fade {fade_in_frames} {fade_out_frames} velocity {}",
        Unit::Decibels.format_value(gain_db),
        Unit::Pan.format_value(pan),
        Unit::Ratio.format_value(velocity),
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_sample_values(
    pattern: &crate::model::Pattern,
    at: crate::time::MusicalTime,
    duration: crate::time::MusicalTime,
    start_frame: u64,
    end_frame: u64,
    gain_db: f64,
    pan: f64,
    fade_in_frames: u32,
    fade_out_frames: u32,
    velocity: f64,
) -> Result<(), EditError> {
    if at < crate::time::MusicalTime::ZERO || at >= pattern.span || !duration.is_positive() {
        return Err(EditError::InvalidValue(
            "sample time must lie in the Pattern and duration must be positive".to_owned(),
        ));
    }
    if !gain_db.is_finite() || !(-96.0..=24.0).contains(&gain_db) {
        return Err(EditError::InvalidValue(
            "sample gain must be -96db..=24db".to_owned(),
        ));
    }
    if !pan.is_finite() || !(-1.0..=1.0).contains(&pan) {
        return Err(EditError::InvalidValue(
            "sample pan must be -1..=1".to_owned(),
        ));
    }
    if !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
        return Err(EditError::InvalidValue(
            "sample velocity must be 0..=1".to_owned(),
        ));
    }
    validate_source_region(start_frame, end_frame, fade_in_frames, fade_out_frames)
}

fn validate_source_region(
    start_frame: u64,
    end_frame: u64,
    fade_in_frames: u32,
    fade_out_frames: u32,
) -> Result<(), EditError> {
    if start_frame >= end_frame
        || u64::from(fade_in_frames).saturating_add(u64::from(fade_out_frames))
            > end_frame.saturating_sub(start_frame)
    {
        return Err(EditError::InvalidValue(
            "sample source region must have start < end and contain both fades".to_owned(),
        ));
    }
    Ok(())
}

fn sample_occurrence<'a>(
    bundle: &'a ProjectBundle,
    pattern_id: &str,
    occurrence_id: &str,
) -> Result<(&'a crate::model::Pattern, &'a PatternEvent), EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let pattern = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == pattern_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{pattern_id}`")))?;
    let event = pattern
        .events
        .iter()
        .find(|event| matches!(event, PatternEvent::Sample { id, .. } if id == occurrence_id))
        .ok_or_else(|| {
            EditError::InvalidValue(format!(
                "Pattern `{pattern_id}` has no sample occurrence `{occurrence_id}`"
            ))
        })?;
    Ok((pattern, event))
}

/// Builds a checked stock-percussion voice replacement.
pub fn set_hit_voice(
    bundle: &ProjectBundle,
    pattern_id: &str,
    event_index: usize,
    voice: DrumVoice,
) -> Result<CheckedReplacement, EditError> {
    match pattern_event(bundle, pattern_id, event_index)?.1 {
        PatternEvent::Hit { voice_site, .. } => Ok(CheckedReplacement::at(
            voice_site,
            format!("{voice:?}").to_lowercase(),
        )),
        PatternEvent::Note { .. } | PatternEvent::Sample { .. } => Err(EditError::WrongTargetKind(
            "this Event has no percussion voice".to_owned(),
        )),
    }
}

/// Builds a checked insertion for a new Song Automation lane and required zero point.
pub fn create_automation_lane(
    bundle: &ProjectBundle,
    lane_id: &str,
    target: &ParameterTarget,
    initial_value: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    if project
        .song
        .automations
        .iter()
        .any(|lane| lane.id == lane_id)
    {
        return Err(EditError::InvalidValue(format!(
            "Automation lane `{lane_id}` already exists"
        )));
    }
    if project
        .song
        .automations
        .iter()
        .any(|lane| lane.target == *target)
    {
        return Err(EditError::InvalidValue(format!(
            "Parameter `{target}` already has absolute Automation"
        )));
    }
    let spec = project
        .parameter_spec(target)
        .ok_or_else(|| EditError::UnknownTarget(format!("Parameter `{target}`")))?;
    if !spec.automatable {
        return Err(EditError::WrongTargetKind(format!(
            "Parameter `{target}` is not automatable"
        )));
    }
    let value = canonical_automation_value(target, spec, initial_value)?;
    let (file, offset, indentation) = block_insertion(bundle, &project.song.location, "Song")?;
    let point_indentation = format!("{indentation}  ");
    let replacement = format!(
        "{indentation}automation {lane_id} target {target} {{\n{point_indentation}point start 0qn {value} step\n{indentation}}}\n"
    );
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement,
    })
}

/// Builds a checked insertion for one durable Automation point.
pub fn create_automation_point(
    bundle: &ProjectBundle,
    lane_id: &str,
    point_id: &str,
    at: &str,
    value: &str,
    interpolation: AutomationInterpolation,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let lane = automation_lane(bundle, lane_id)?;
    if lane.points.iter().any(|point| point.id == point_id) {
        return Err(EditError::InvalidValue(format!(
            "Automation point `{point_id}` already exists in lane `{lane_id}`"
        )));
    }
    let at = parse_musical_time(at, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    validate_automation_time(project, lane, None, at)?;
    let spec = project
        .parameter_spec(&lane.target)
        .ok_or_else(|| EditError::UnknownTarget(format!("Parameter `{}`", lane.target)))?;
    let value = canonical_automation_value(&lane.target, spec, value)?;
    let (file, offset, indentation) =
        if let Some(next) = lane.points.iter().find(|point| point.at > at) {
            line_insertion(bundle, &next.location, "Automation point")?
        } else {
            block_insertion(bundle, &lane.location, "Automation")?
        };
    Ok(CheckedReplacement {
        file,
        start: offset,
        end: offset,
        expected: String::new(),
        replacement: format!("{indentation}point {point_id} {at} {value} {interpolation}\n"),
    })
}

/// Builds a checked physical-value replacement for one Automation point.
pub fn set_automation_point_value(
    bundle: &ProjectBundle,
    lane_id: &str,
    point_id: &str,
    value: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (lane, point) = automation_point(bundle, lane_id, point_id)?;
    let spec = project
        .parameter_spec(&lane.target)
        .ok_or_else(|| EditError::UnknownTarget(format!("Parameter `{}`", lane.target)))?;
    Ok(CheckedReplacement::at(
        &point.value_site,
        canonical_automation_value(&lane.target, spec, value)?,
    ))
}

/// Builds a checked exact-time replacement for one Automation point.
pub fn set_automation_point_at(
    bundle: &ProjectBundle,
    lane_id: &str,
    point_id: &str,
    at: &str,
) -> Result<CheckedReplacement, EditError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let (lane, point) = automation_point(bundle, lane_id, point_id)?;
    let at = parse_musical_time(at, project.song.meter)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    validate_automation_time(project, lane, Some(point_id), at)?;
    let current_index = lane
        .points
        .iter()
        .position(|candidate| candidate.id == point_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Automation point `{point_id}`")))?;
    if current_index == 0 && at != crate::time::MusicalTime::ZERO {
        return Err(EditError::InvalidValue(
            "Automation lane's first point must remain at Song time zero".to_owned(),
        ));
    }
    let remains_ordered = current_index
        .checked_sub(1)
        .and_then(|index| lane.points.get(index))
        .is_none_or(|previous| previous.at < at)
        && lane
            .points
            .get(current_index + 1)
            .is_none_or(|next| at < next.at);
    if remains_ordered {
        return Ok(CheckedReplacement::at(&point.at_site, at.to_string()));
    }

    let source = bundle
        .sources
        .get(&lane.location.file)
        .ok_or_else(|| EditError::UnknownSource(lane.location.file.clone()))?;
    let (moved_start, moved_end) = line_bounds(source, &point.location)?;
    let target_offset = if let Some(next) = lane
        .points
        .iter()
        .filter(|candidate| candidate.id != point_id)
        .find(|candidate| candidate.at > at)
    {
        line_bounds(source, &next.location)?.0
    } else {
        block_insertion(bundle, &lane.location, "Automation")?.1
    };
    if moved_start < lane.location.start
        || moved_end > lane.location.end
        || target_offset < lane.location.start
        || target_offset > lane.location.end
    {
        return Err(EditError::OverlappingOrInvalidRange(
            lane.location.file.clone(),
        ));
    }
    let mut moved_line = source[moved_start..moved_end].to_owned();
    moved_line.replace_range(
        point.at_site.start - moved_start..point.at_site.end - moved_start,
        &at.to_string(),
    );
    let moved_start = moved_start - lane.location.start;
    let moved_end = moved_end - lane.location.start;
    let mut insertion = target_offset - lane.location.start;
    let mut replacement = source[lane.location.start..lane.location.end].to_owned();
    replacement.replace_range(moved_start..moved_end, "");
    if insertion > moved_start {
        insertion -= moved_end - moved_start;
    }
    replacement.insert_str(insertion, &moved_line);
    Ok(CheckedReplacement {
        file: lane.location.file.clone(),
        start: lane.location.start,
        end: lane.location.end,
        expected: source[lane.location.start..lane.location.end].to_owned(),
        replacement,
    })
}

/// Builds a checked interpolation replacement for one Automation point.
pub fn set_automation_point_interpolation(
    bundle: &ProjectBundle,
    lane_id: &str,
    point_id: &str,
    interpolation: AutomationInterpolation,
) -> Result<CheckedReplacement, EditError> {
    let (_, point) = automation_point(bundle, lane_id, point_id)?;
    Ok(CheckedReplacement::at(
        &point.interpolation_site,
        interpolation.to_string(),
    ))
}

/// Builds a checked whole-directive deletion for one Automation point.
pub fn delete_automation_point(
    bundle: &ProjectBundle,
    lane_id: &str,
    point_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let (_, point) = automation_point(bundle, lane_id, point_id)?;
    line_replacement(bundle, &point.location)
}

/// Builds a checked whole-block deletion for one Automation lane.
pub fn delete_automation_lane(
    bundle: &ProjectBundle,
    lane_id: &str,
) -> Result<CheckedReplacement, EditError> {
    let lane = automation_lane(bundle, lane_id)?;
    line_replacement(bundle, &lane.location)
}

fn canonical_automation_value(
    target: &ParameterTarget,
    spec: crate::model::ParameterSpec,
    value: &str,
) -> Result<String, EditError> {
    let parsed = spec
        .unit
        .parse_value(value)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    if !parsed.is_finite() || !(spec.minimum..=spec.maximum).contains(&parsed) {
        return Err(EditError::OutOfRange {
            target: target.to_string(),
            minimum: spec.minimum,
            maximum: spec.maximum,
            unit: spec.unit,
        });
    }
    Ok(spec.unit.format_value(parsed))
}

fn automation_lane<'a>(
    bundle: &'a ProjectBundle,
    lane_id: &str,
) -> Result<&'a AutomationLane, EditError> {
    bundle
        .project
        .as_ref()
        .ok_or(EditError::InvalidBase)?
        .song
        .automations
        .iter()
        .find(|lane| lane.id == lane_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Automation lane `{lane_id}`")))
}

fn automation_point<'a>(
    bundle: &'a ProjectBundle,
    lane_id: &str,
    point_id: &str,
) -> Result<(&'a AutomationLane, &'a AutomationPoint), EditError> {
    let lane = automation_lane(bundle, lane_id)?;
    let point = lane
        .points
        .iter()
        .find(|point| point.id == point_id)
        .ok_or_else(|| {
            EditError::UnknownTarget(format!("Automation point `{point_id}` in lane `{lane_id}`"))
        })?;
    Ok((lane, point))
}

fn validate_automation_time(
    project: &crate::model::Project,
    lane: &AutomationLane,
    except_point: Option<&str>,
    at: crate::time::MusicalTime,
) -> Result<u64, EditError> {
    if at < crate::time::MusicalTime::ZERO || at > project.song.end {
        return Err(EditError::InvalidValue(
            "Automation point must lie between Song time zero and the arrangement end".to_owned(),
        ));
    }
    let frame = at
        .to_frames(project.song.tempo_bpm, project.sample_rate)
        .map_err(|error| EditError::InvalidValue(error.to_string()))?;
    for point in lane
        .points
        .iter()
        .filter(|point| Some(point.id.as_str()) != except_point)
    {
        if point.at == at {
            return Err(EditError::InvalidValue(format!(
                "Automation lane `{}` already has a point at {at}",
                lane.id
            )));
        }
        let point_frame = point
            .at
            .to_frames(project.song.tempo_bpm, project.sample_rate)
            .map_err(|error| EditError::InvalidValue(error.to_string()))?;
        if point_frame == frame {
            return Err(EditError::InvalidValue(format!(
                "Automation point at {at} would share sample frame {frame} in lane `{}`",
                lane.id
            )));
        }
    }
    Ok(frame)
}

fn line_bounds(source: &str, location: &SourceLocation) -> Result<(usize, usize), EditError> {
    if location.start > location.end || location.end > source.len() {
        return Err(EditError::OverlappingOrInvalidRange(location.file.clone()));
    }
    let start = source[..location.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let end = source[location.end..]
        .find('\n')
        .map_or(source.len(), |index| location.end + index + 1);
    Ok((start, end))
}

fn line_insertion(
    bundle: &ProjectBundle,
    location: &SourceLocation,
    owner: &str,
) -> Result<(PathBuf, usize, String), EditError> {
    let source = bundle
        .sources
        .get(&location.file)
        .ok_or_else(|| EditError::UnknownSource(location.file.clone()))?;
    let (line_start, _) = line_bounds(source, location)?;
    let indentation = &source[line_start..location.start];
    if !indentation.chars().all(char::is_whitespace) {
        return Err(EditError::InvalidValue(format!(
            "{owner} indentation is not structure-preserving whitespace"
        )));
    }
    Ok((location.file.clone(), line_start, indentation.to_owned()))
}

fn block_insertion(
    bundle: &ProjectBundle,
    location: &SourceLocation,
    owner: &str,
) -> Result<(PathBuf, usize, String), EditError> {
    let source = bundle
        .sources
        .get(&location.file)
        .ok_or_else(|| EditError::UnknownSource(location.file.clone()))?;
    let body = source
        .get(location.start..location.end)
        .ok_or_else(|| EditError::OverlappingOrInvalidRange(location.file.clone()))?;
    let relative_close = body
        .rfind('}')
        .ok_or_else(|| EditError::InvalidValue(format!("{owner} block has no closing brace")))?;
    let close = location.start + relative_close;
    let line_start = source[..close].rfind('\n').map_or(0, |index| index + 1);
    if !source[line_start..close].chars().all(char::is_whitespace) {
        return Err(EditError::InvalidValue(format!(
            "{owner} closing brace is not on a structure-preserving line"
        )));
    }
    let owner_line_start = source[..location.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let owner_indentation = &source[owner_line_start..location.start];
    if !owner_indentation.chars().all(char::is_whitespace) {
        return Err(EditError::InvalidValue(format!(
            "{owner} declaration indentation is not whitespace"
        )));
    }
    Ok((
        location.file.clone(),
        line_start,
        format!("{owner_indentation}  "),
    ))
}

fn source_block_with_attached_comments(
    bundle: &ProjectBundle,
    location: &SourceLocation,
) -> Result<CheckedReplacement, EditError> {
    let source = bundle
        .sources
        .get(&location.file)
        .ok_or_else(|| EditError::UnknownSource(location.file.clone()))?;
    let (mut start, end) = line_bounds(source, location)?;
    while start > 0 {
        let previous_end = start - 1;
        let previous_start = source[..previous_end]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        if !source[previous_start..previous_end]
            .trim_start()
            .starts_with('#')
        {
            break;
        }
        start = previous_start;
    }
    Ok(CheckedReplacement {
        file: location.file.clone(),
        start,
        end,
        expected: source[start..end].to_owned(),
        replacement: String::new(),
    })
}

fn line_replacement(
    bundle: &ProjectBundle,
    location: &SourceLocation,
) -> Result<CheckedReplacement, EditError> {
    let source = bundle
        .sources
        .get(&location.file)
        .ok_or_else(|| EditError::UnknownSource(location.file.clone()))?;
    if location.end > source.len() {
        return Err(EditError::OverlappingOrInvalidRange(location.file.clone()));
    }
    let start = source[..location.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let end = source[location.end..]
        .find('\n')
        .map_or(source.len(), |index| location.end + index + 1);
    Ok(CheckedReplacement {
        file: location.file.clone(),
        start,
        end,
        expected: source[start..end].to_owned(),
        replacement: String::new(),
    })
}

fn pattern_event<'a>(
    bundle: &'a ProjectBundle,
    pattern_id: &str,
    event_index: usize,
) -> Result<(&'a crate::model::Pattern, &'a PatternEvent), EditError> {
    let pattern = bundle
        .project
        .as_ref()
        .ok_or(EditError::InvalidBase)?
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == pattern_id)
        .ok_or_else(|| EditError::UnknownTarget(format!("Pattern `{pattern_id}`")))?;
    let event = pattern.events.get(event_index).ok_or_else(|| {
        EditError::UnknownTarget(format!("Event {event_index} in Pattern `{pattern_id}`"))
    })?;
    Ok((pattern, event))
}

fn apply_in_memory(
    bundle: &ProjectBundle,
    replacements: &[CheckedReplacement],
) -> Result<BTreeMap<PathBuf, String>, EditError> {
    let mut grouped: BTreeMap<&Path, Vec<&CheckedReplacement>> = BTreeMap::new();
    for replacement in replacements {
        if replacement.file.is_absolute()
            || replacement.file.to_string_lossy().starts_with("library:")
        {
            return Err(EditError::UnsafePath(replacement.file.clone()));
        }
        grouped
            .entry(&replacement.file)
            .or_default()
            .push(replacement);
    }
    let mut result = bundle.sources.clone();
    for (path, mut edits) in grouped {
        let source = bundle
            .sources
            .get(path)
            .ok_or_else(|| EditError::UnknownSource(path.to_path_buf()))?;
        edits.sort_by_key(|edit| (edit.start, edit.end));
        let mut previous_end = 0;
        for edit in &edits {
            if edit.start < previous_end || edit.start > edit.end || edit.end > source.len() {
                return Err(EditError::OverlappingOrInvalidRange(path.to_path_buf()));
            }
            if !source.is_char_boundary(edit.start) || !source.is_char_boundary(edit.end) {
                return Err(EditError::InvalidUtf8Boundary(path.to_path_buf()));
            }
            let actual = &source[edit.start..edit.end];
            if actual != edit.expected {
                return Err(EditError::StaleTarget {
                    path: path.to_path_buf(),
                    expected: edit.expected.clone(),
                    actual: actual.to_owned(),
                });
            }
            previous_end = edit.end;
        }
        let mut changed = source.clone();
        for edit in edits.into_iter().rev() {
            changed.replace_range(edit.start..edit.end, &edit.replacement);
        }
        result.insert(path.to_path_buf(), changed);
    }
    Ok(result)
}

fn write_asset_changes(
    root: &Path,
    writes: &[StagedAssetWrite],
    removals: &[PathBuf],
) -> Result<(), EditError> {
    for removal in removals {
        let path = root.join(removal);
        ensure_transaction_parent(root, &path)?;
        match fs::remove_file(&path) {
            Ok(()) => {
                if let Some(parent) = path.parent() {
                    fs::File::open(parent)
                        .and_then(|directory| directory.sync_all())
                        .map_err(|source| EditError::Io {
                            path: parent.to_path_buf(),
                            source,
                        })?;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(EditError::Io { path, source }),
        }
    }
    for write in writes {
        let path = root.join(&write.path);
        ensure_transaction_parent(root, &path)?;
        if write.replace {
            write_atomic(&path, &write.bytes)?;
        } else {
            write_new_atomic(&path, &write.bytes)?;
        }
    }
    Ok(())
}

fn write_changed_sources(
    root: &Path,
    paths: &[PathBuf],
    sources: &BTreeMap<PathBuf, String>,
) -> Result<(), EditError> {
    let unique: std::collections::BTreeSet<_> = paths.iter().collect();
    for path in unique {
        let source = sources
            .get(path)
            .ok_or_else(|| EditError::UnknownSource(path.clone()))?;
        let absolute = root.join(path);
        ensure_transaction_parent(root, &absolute)?;
        write_atomic(&absolute, source.as_bytes())?;
    }
    Ok(())
}

/// Structured-edit transaction failures.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// No replacement was requested.
    #[error("Structured Source Edit contains no replacements")]
    Empty,
    /// Candidate changed after the Production View read it.
    #[error("stale Candidate Revision: expected {expected}, found {actual}")]
    StaleRevision {
        /// Revision visible to the caller.
        expected: RevisionId,
        /// Revision now on disk.
        actual: RevisionId,
    },
    /// An unpublished external Candidate must be handled explicitly first.
    #[error(
        "Candidate Revision {candidate} is not the Active Revision {active}; publish or discard external edits first"
    )]
    CandidateNotActive {
        /// Runtime revision.
        active: RevisionId,
        /// Current disk revision.
        candidate: RevisionId,
    },
    /// Existing candidate is invalid.
    #[error("cannot perform a structured edit on an invalid Candidate Revision")]
    InvalidBase,
    /// Checked range no longer contains expected text.
    #[error("stale target in {}: expected `{expected}`, found `{actual}`", path.display())]
    StaleTarget {
        /// Source file.
        path: PathBuf,
        /// Parsed exact text.
        expected: String,
        /// Current exact text.
        actual: String,
    },
    /// Ranges overlap or lie outside source.
    #[error("replacement ranges overlap or are invalid in {}", .0.display())]
    OverlappingOrInvalidRange(PathBuf),
    /// A replacement split a UTF-8 scalar.
    #[error("replacement range is not on UTF-8 boundaries in {}", .0.display())]
    InvalidUtf8Boundary(PathBuf),
    /// Source path is not part of the candidate.
    #[error("unknown authoritative source `{}`", .0.display())]
    UnknownSource(PathBuf),
    /// Shared or absolute paths cannot be edited transactionally.
    #[error("Structured Source Edit cannot write `{}`", .0.display())]
    UnsafePath(PathBuf),
    /// An import never replaces an existing Project file implicitly.
    #[error("media import destination `{}` already exists", .0.display())]
    AssetDestinationExists(PathBuf),
    /// Binary mutations may only affect paths owned by corresponding source declarations.
    #[error("media mutation path `{}` is not declared by the corresponding Project state", .0.display())]
    UndeclaredAssetPath(PathBuf),
    /// Semantic target is missing.
    #[error("unknown edit target: {0}")]
    UnknownTarget(String),
    /// Target does not support the operation.
    #[error("wrong edit target kind: {0}")]
    WrongTargetKind(String),
    /// Authored exact value could not be parsed or falls outside its context.
    #[error("invalid edit value: {0}")]
    InvalidValue(String),
    /// Physical value falls outside its domain.
    #[error("{target} must be between {minimum} and {maximum} {unit}")]
    OutOfRange {
        /// Semantic target.
        target: String,
        /// Inclusive lower bound.
        minimum: f64,
        /// Inclusive upper bound.
        maximum: f64,
        /// Physical unit.
        unit: Unit,
    },
    /// Edited candidate failed parse or semantic validation.
    #[error("Structured Source Edit produced an invalid Candidate Revision")]
    InvalidResult(Vec<crate::diagnostic::Diagnostic>),
    /// Project loading failed.
    #[error(transparent)]
    Project(#[from] ProjectIoError),
    /// Persistent history failed.
    #[error(transparent)]
    History(#[from] HistoryError),
    /// Creating a source directory failed.
    #[error("source I/O failed at {}: {source}", path.display())]
    Io {
        /// Affected path.
        path: PathBuf,
        /// Operating-system error.
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::{
        history::{ChangeOrigin, Publication},
        project::PROJECT_FILE,
    };

    use super::*;

    fn write_test_wav(path: &Path, samples: &[i16]) -> Vec<u8> {
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

    const SOURCE: &str = r#"crump 1
project "Edit" {
  id edit
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
        control tone "Tone" "Brightness" maps tone
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

    fn sampler_source(sha256: &str) -> String {
        format!(
            r#"crump 1
project "Sampler Edits" {{
  id sampler-edits
  seed 9
  sample_rate 48000
  render_tail 10ms
  asset source "Source" wav "takes/raw.wav" sha256 "{sha256}"
  song {{
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument sampler "Sampler" sampler {{
      polyphony 4
      parameter gain "Gain" db -96db 24db 0db 0db
      faceplate {{
        control gain "Gain" "Sampler level" maps gain
      }}
      mapping zone asset source keys C4 C4 root C4 {{
        region 0 512
        playback gated
        gain 0db
        pan 0
        envelope 0s 10ms
        fade 4 4
      }}
    }}
    pattern phrase 1bar {{
      sample original 0beat mapping zone duration 1beat start 0 end 256 gain 0db pan 0 fade 4 4 velocity 1
    }}
    track samples "Samples" instrument sampler level 0db pan 0 {{
      clip phrase pattern phrase start 0bar duration 1bar mode once
    }}
  }}
}}
"#
        )
    }

    #[test]
    fn sampler_operations_and_collection_are_checked_atomic_and_renderable() {
        let directory = tempdir().expect("Project root");
        fs::create_dir_all(directory.path().join("takes")).expect("takes directory");
        let bytes = write_test_wav(
            &directory.path().join("takes/raw.wav"),
            &(0..512)
                .map(|index| {
                    if (index / 16) % 2 == 0 {
                        12_000
                    } else {
                        -12_000
                    }
                })
                .collect::<Vec<_>>(),
        );
        let sha256 = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        fs::write(directory.path().join(PROJECT_FILE), sampler_source(&sha256))
            .expect("Project source");

        let mut bundle = load_project(directory.path()).expect("load sampler edits");
        collect_media_assets(
            directory.path(),
            &bundle.revision,
            Publication::new(ChangeOrigin::human("tester"), "Collect", "normalize media"),
        )
        .expect("collect");
        assert_eq!(
            fs::read(directory.path().join("media/source.wav")).expect("collected bytes"),
            bytes
        );
        assert!(!directory.path().join("takes/raw.wav").exists());

        let apply =
            |bundle: &ProjectBundle, replacements: Vec<CheckedReplacement>, summary: &str| {
                apply_structured_edit(
                    &bundle.root,
                    &bundle.revision,
                    replacements,
                    Publication::new(ChangeOrigin::human("tester"), summary, "sampler edit"),
                )
                .expect("apply sampler edit");
            };

        bundle = load_project(directory.path()).expect("reload after collect");
        apply(
            &bundle,
            trim_sample_occurrence(&bundle, "phrase", "original", 8, 248).expect("trim"),
            "Trim",
        );
        bundle = load_project(directory.path()).expect("reload after trim");
        apply(
            &bundle,
            slip_sample_occurrence(&bundle, "phrase", "original", 8).expect("slip"),
            "Slip",
        );
        bundle = load_project(directory.path()).expect("reload after slip");
        apply(
            &bundle,
            fade_sample_occurrence(&bundle, "phrase", "original", 6, 10).expect("fade"),
            "Fade",
        );
        bundle = load_project(directory.path()).expect("reload after fade");
        apply(
            &bundle,
            vec![set_sample_occurrence_gain(&bundle, "phrase", "original", -3.0).expect("gain")],
            "Gain",
        );
        bundle = load_project(directory.path()).expect("reload after gain");
        apply(
            &bundle,
            vec![set_sample_occurrence_pan(&bundle, "phrase", "original", 0.25).expect("pan")],
            "Pan",
        );
        bundle = load_project(directory.path()).expect("reload after pan");
        apply(
            &bundle,
            vec![
                place_sample_occurrence(
                    &bundle,
                    "phrase",
                    &SamplePlacement {
                        id: "placed".to_owned(),
                        mapping: "zone".to_owned(),
                        at: "1beat".to_owned(),
                        duration: "1beat".to_owned(),
                        start_frame: 32,
                        end_frame: 300,
                        gain_db: -2.0,
                        pan: -0.2,
                        fade_in_frames: 5,
                        fade_out_frames: 7,
                        velocity: 0.8,
                    },
                )
                .expect("place"),
            ],
            "Place",
        );
        bundle = load_project(directory.path()).expect("reload after place");
        apply(
            &bundle,
            vec![
                duplicate_sample_occurrence(&bundle, "phrase", "original", "copy", "2beat")
                    .expect("duplicate"),
            ],
            "Duplicate",
        );
        bundle = load_project(directory.path()).expect("reload after duplicate");
        apply(
            &bundle,
            vec![
                split_sample_occurrence(&bundle, "phrase", "copy", "copy-tail", "5/2beat", 100)
                    .expect("split"),
            ],
            "Split",
        );
        bundle = load_project(directory.path()).expect("reload after split");
        apply(
            &bundle,
            vec![move_sample_occurrence(&bundle, "phrase", "copy-tail", "3beat").expect("move")],
            "Move",
        );

        let final_bundle = load_project(directory.path()).expect("final sampler Project");
        assert!(!final_bundle.diagnostics.has_errors());
        let source = fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source");
        for occurrence in ["original", "placed", "copy", "copy-tail"] {
            assert!(source.contains(&format!("sample {occurrence} ")));
        }
        assert!(source.contains("asset source \"Source\" wav \"media/source.wav\""));
        let project = final_bundle
            .publishable()
            .expect("publishable final sampler Project");
        let compiled =
            crate::engine::CompiledProject::compile_with_assets(project, &final_bundle.assets)
                .expect("compile edited sampler");
        let mut runtime = compiled.runtime();
        let rendered = (0..compiled.total_frames())
            .map(|_| {
                runtime
                    .process_frame(crate::engine::ProcessingMode::Offline)
                    .expect("finite edited sampler")
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().flatten().any(|sample| sample.abs() > 0.01));
    }

    #[test]
    fn media_import_copies_declares_validates_and_publishes_atomically() {
        let directory = tempdir().expect("Project root");
        fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
        let external = tempdir().expect("external media");
        let input = external.path().join("voice.wav");
        let bytes = write_test_wav(&input, &[0, 100, -100]);
        let before = load_project(directory.path()).expect("Candidate");
        import_media_asset(
            directory.path(),
            &before.revision,
            &input,
            "voice",
            "Voice \"fragment\"",
            Publication::new(
                ChangeOrigin::human("tester"),
                "Import voice",
                "portable copy",
            ),
        )
        .expect("import");

        assert_eq!(
            fs::read(directory.path().join("media/voice.wav")).expect("copied asset"),
            bytes
        );
        let source = fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source");
        assert!(
            source.contains(
                "asset voice \"Voice \\\"fragment\\\"\" wav \"media/voice.wav\" sha256 \""
            )
        );
        let after = load_project(directory.path()).expect("reloaded Project");
        assert!(!after.diagnostics.has_errors());
        assert_eq!(after.assets.get("voice").expect("asset").bytes(), bytes);
        let store = HistoryStore::open(directory.path()).expect("History");
        assert_eq!(
            store
                .active_state()
                .expect("state")
                .expect("Active")
                .active_revision,
            after.revision
        );
    }

    #[test]
    fn invalid_media_import_leaves_source_and_destination_untouched() {
        let directory = tempdir().expect("Project root");
        fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
        let invalid = directory.path().join("not-a-wave.bin");
        fs::write(&invalid, b"not a wave").expect("invalid input");
        let before = load_project(directory.path()).expect("Candidate");
        let source_before = before
            .source(Path::new(PROJECT_FILE))
            .expect("source")
            .to_owned();
        assert!(matches!(
            import_media_asset(
                directory.path(),
                &before.revision,
                &invalid,
                "broken",
                "Broken",
                Publication::new(ChangeOrigin::human("tester"), "Import", "invalid")
            ),
            Err(EditError::InvalidResult(_))
        ));
        assert_eq!(
            fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source"),
            source_before
        );
        assert!(!directory.path().join("media/broken.wav").exists());
    }

    #[test]
    fn media_import_never_replaces_an_undeclared_destination() {
        let directory = tempdir().expect("Project root");
        fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
        fs::create_dir_all(directory.path().join("media")).expect("media directory");
        fs::write(directory.path().join("media/voice.wav"), b"keep me").expect("destination");
        let external = tempdir().expect("external media");
        let input = external.path().join("voice.wav");
        write_test_wav(&input, &[0, 100]);
        let before = load_project(directory.path()).expect("Candidate");

        assert!(matches!(
            import_media_asset(
                directory.path(),
                &before.revision,
                &input,
                "voice",
                "Voice",
                Publication::new(ChangeOrigin::human("tester"), "Import", "no replace")
            ),
            Err(EditError::AssetDestinationExists(path)) if path == Path::new("media/voice.wav")
        ));
        assert_eq!(
            fs::read(directory.path().join("media/voice.wav")).expect("destination"),
            b"keep me"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source"),
            SOURCE
        );
    }

    #[test]
    fn undeclared_binary_mutations_cannot_create_or_delete_project_files() {
        let directory = tempdir().expect("Project root");
        fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
        fs::create_dir_all(directory.path().join("media")).expect("media directory");
        let existing = PathBuf::from("media/notes.wav");
        fs::write(directory.path().join(&existing), b"keep me").expect("existing file");
        let before = load_project(directory.path()).expect("Candidate");

        assert!(matches!(
            apply_project_mutation(
                directory.path(),
                &before.revision,
                ProjectMutation {
                    replacements: Vec::new(),
                    asset_writes: Vec::new(),
                    asset_removals: vec![existing.clone()],
                },
                Publication::new(ChangeOrigin::human("tester"), "Remove", "undeclared")
            ),
            Err(EditError::UndeclaredAssetPath(path)) if path == existing
        ));
        assert_eq!(
            fs::read(directory.path().join(&existing)).expect("preserved file"),
            b"keep me"
        );
        assert!(matches!(
            apply_project_mutation(
                directory.path(),
                &before.revision,
                ProjectMutation {
                    replacements: Vec::new(),
                    asset_writes: vec![StagedAssetWrite {
                        path: existing.clone(),
                        bytes: b"replacement".to_vec(),
                        replace: true,
                    }],
                    asset_removals: Vec::new(),
                },
                Publication::new(ChangeOrigin::human("tester"), "Replace", "undeclared")
            ),
            Err(EditError::UndeclaredAssetPath(path)) if path == existing
        ));
        assert_eq!(
            fs::read(directory.path().join(&existing)).expect("preserved file"),
            b"keep me"
        );

        let created = PathBuf::from("media/stray.wav");
        assert!(matches!(
            apply_project_mutation(
                directory.path(),
                &before.revision,
                ProjectMutation {
                    replacements: Vec::new(),
                    asset_writes: vec![StagedAssetWrite {
                        path: created.clone(),
                        bytes: b"stray".to_vec(),
                        replace: false,
                    }],
                    asset_removals: Vec::new(),
                },
                Publication::new(ChangeOrigin::human("tester"), "Write", "undeclared")
            ),
            Err(EditError::UndeclaredAssetPath(path)) if path == created
        ));
        assert!(!directory.path().join(created).exists());
        assert_eq!(
            fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source"),
            SOURCE
        );
        assert!(
            HistoryStore::open(directory.path())
                .expect("History")
                .list()
                .expect("entries")
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn media_import_rejects_symlink_inputs_and_project_parents() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("Project root");
        fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
        let external = tempdir().expect("external media");
        let input = external.path().join("voice.wav");
        write_test_wav(&input, &[0, 100]);
        let input_link = external.path().join("voice-link.wav");
        symlink(&input, &input_link).expect("input symlink");
        let before = load_project(directory.path()).expect("Candidate");
        assert!(matches!(
            import_media_asset(
                directory.path(),
                &before.revision,
                &input_link,
                "linked",
                "Linked",
                Publication::new(ChangeOrigin::human("tester"), "Import", "reject link")
            ),
            Err(EditError::InvalidValue(_))
        ));

        let outside = tempdir().expect("outside destination");
        symlink(outside.path(), directory.path().join("media")).expect("media parent symlink");
        assert!(matches!(
            import_media_asset(
                directory.path(),
                &before.revision,
                &input,
                "voice",
                "Voice",
                Publication::new(ChangeOrigin::human("tester"), "Import", "confined")
            ),
            Err(EditError::History(HistoryError::UnsupportedRestore(_)))
        ));
        assert!(!outside.path().join("voice.wav").exists());
        assert_eq!(
            fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source"),
            SOURCE
        );
    }

    #[test]
    fn arrangement_time_operations_shift_delete_and_duplicate_atomically() {
        let directory = tempdir().expect("Project root");
        let source =
            include_str!("../tests/fixtures/minimal.crump").replacen("end 1bar", "end 4bar", 1);
        fs::write(directory.path().join(PROJECT_FILE), source).expect("Project source");
        let publication = |summary: &str| {
            Publication::new(ChangeOrigin::human("tester"), summary, "Arrangement edit")
        };

        let mut bundle = load_project(directory.path()).expect("initial Project");
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![create_locator(&bundle, "marker", "Marker", "2bar").expect("Locator")],
            publication("Create Locator"),
        )
        .expect("apply Locator");

        bundle = load_project(directory.path()).expect("Project with Locator");
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            duplicate_arrangement_time(
                &bundle,
                "0bar",
                "1bar",
                "1bar",
                "copy",
                ArrangementAutomation::FollowArrangement,
            )
            .expect("duplicate arrangement time"),
            publication("Duplicate arrangement time"),
        )
        .expect("apply duplicate time");

        bundle = load_project(directory.path()).expect("duplicated Project");
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            insert_arrangement_time(
                &bundle,
                "2bar",
                "1bar",
                ArrangementAutomation::FollowArrangement,
            )
            .expect("insert arrangement time"),
            publication("Insert arrangement time"),
        )
        .expect("apply insert time");

        bundle = load_project(directory.path()).expect("inserted Project");
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            delete_arrangement_time(
                &bundle,
                "1bar",
                "1bar",
                ArrangementAutomation::FollowArrangement,
            )
            .expect("delete arrangement time"),
            publication("Delete arrangement time"),
        )
        .expect("apply delete time");

        let final_bundle = load_project(directory.path()).expect("final Project");
        assert!(!final_bundle.diagnostics.has_errors());
        let source = fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source");
        assert!(source.contains("end 20qn"));
        assert!(source.contains("locator marker \"Marker\" at 12qn"));
        assert!(!source.contains("clip melody-copy "));
        assert!(!source.contains("clip beat-copy "));
        let project = final_bundle.publishable().expect("publishable arrangement");
        crate::engine::CompiledProject::compile(project).expect("compile arrangement");
    }

    #[test]
    fn arrangement_time_automation_uses_half_open_follow_and_explicit_lock_policies() {
        let source = include_str!("../tests/fixtures/minimal.crump")
            .replacen("end 1bar", "end 4bar", 1)
            .replacen("3000hz 3200hz", "3000hz 3200hz automatable", 1)
            .replacen(
                "    track lead \"Lead\"",
                r#"    automation arrangement-tone target instrument/lead/tone {
      point start 0qn 3000hz step
      point at-left 4qn 3000hz step
      point inside 6qn 3000hz step
      point at-right 8qn 3000hz step
      point later 12qn 3000hz step
      point terminal 16qn 3000hz step
    }
    track lead "Lead""#,
                1,
            );
        let publication = |summary: &str| {
            Publication::new(ChangeOrigin::human("tester"), summary, "Automation policy")
        };

        let follow_directory = tempdir().expect("follow Project root");
        fs::write(follow_directory.path().join(PROJECT_FILE), &source).expect("follow source");
        let bundle = load_project(follow_directory.path()).expect("follow Project");
        assert!(matches!(
            delete_arrangement_time(
                &bundle,
                "1bar",
                "1bar",
                ArrangementAutomation::LockToSong,
            ),
            Err(EditError::InvalidValue(message))
                if message.contains("locked Automation would extend beyond")
        ));
        apply_structured_edit(
            follow_directory.path(),
            &bundle.revision,
            delete_arrangement_time(
                &bundle,
                "1bar",
                "1bar",
                ArrangementAutomation::FollowArrangement,
            )
            .expect("follow deletion"),
            publication("Follow arrangement"),
        )
        .expect("apply follow deletion");
        let followed = fs::read_to_string(follow_directory.path().join(PROJECT_FILE))
            .expect("followed source");
        assert!(!followed.contains("point at-left "));
        assert!(!followed.contains("point inside "));
        assert!(followed.contains("point at-right 4qn "));
        assert!(followed.contains("point later 8qn "));
        assert!(followed.contains("point terminal 12qn "));
        let followed = load_project(follow_directory.path()).expect("followed Project");
        assert!(!followed.diagnostics.has_errors());

        let locked_source = source.replace("      point terminal 16qn 3000hz step\n", "");
        let locked_directory = tempdir().expect("locked Project root");
        fs::write(locked_directory.path().join(PROJECT_FILE), locked_source)
            .expect("locked source");
        let bundle = load_project(locked_directory.path()).expect("locked Project");
        apply_structured_edit(
            locked_directory.path(),
            &bundle.revision,
            delete_arrangement_time(&bundle, "1bar", "1bar", ArrangementAutomation::LockToSong)
                .expect("locked deletion"),
            publication("Lock to Song"),
        )
        .expect("apply locked deletion");
        let locked =
            fs::read_to_string(locked_directory.path().join(PROJECT_FILE)).expect("locked source");
        assert!(locked.contains("point at-left 4qn "));
        assert!(locked.contains("point inside 6qn "));
        assert!(locked.contains("point at-right 8qn "));
        assert!(locked.contains("point later 12qn "));
        let locked = load_project(locked_directory.path()).expect("locked Project after deletion");
        assert!(!locked.diagnostics.has_errors());
    }

    #[test]
    fn clip_operations_are_checked_procedural_and_renderable() {
        let directory = tempdir().expect("Project root");
        fs::write(
            directory.path().join(PROJECT_FILE),
            include_str!("../tests/fixtures/minimal.crump"),
        )
        .expect("Project source");
        let publication = |summary: &str| {
            Publication::new(ChangeOrigin::human("tester"), summary, "Timeline edit")
        };

        let mut bundle = load_project(directory.path()).expect("initial Project");
        let placement = ClipPlacement {
            id: "alternate".to_owned(),
            pattern: "melody".to_owned(),
            start: "0bar".to_owned(),
            duration: "1bar".to_owned(),
            mode: ClipMode::Loop,
            source_offset: "1beat".to_owned(),
            transpose_semitones: 12,
            gain_db: -6.0,
            fade_in: "1/2beat".to_owned(),
            fade_out: "1/2beat".to_owned(),
            groove: None,
        };
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![create_clip(&bundle, "lead", &placement).expect("create Clip")],
            publication("Create Clip"),
        )
        .expect("apply create");

        bundle = load_project(directory.path()).expect("created Project");
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![
                duplicate_clip(&bundle, "alternate", "alternate-copy", "0bar")
                    .expect("duplicate Clip"),
            ],
            publication("Duplicate Clip"),
        )
        .expect("apply duplicate");

        bundle = load_project(directory.path()).expect("duplicated Project");
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            split_clip(&bundle, "alternate-copy", "alternate-tail", "2beat").expect("split Clip"),
            publication("Split Clip"),
        )
        .expect("apply split");

        bundle = load_project(directory.path()).expect("split Project");
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![
                move_clip(&bundle, "alternate-tail", "1beat").expect("move Clip"),
                resize_clip(&bundle, "alternate-copy", "1beat").expect("resize Clip"),
            ],
            publication("Move and resize Clips"),
        )
        .expect("apply move/resize");

        bundle = load_project(directory.path()).expect("moved Project");
        let mut replacements =
            set_clip_source_offset(&bundle, "alternate", "0beat").expect("set Clip offset");
        replacements.extend(set_clip_transpose(&bundle, "alternate", 7).expect("transpose Clip"));
        replacements.extend(set_clip_gain(&bundle, "alternate", -3.0).expect("gain Clip"));
        replacements
            .extend(set_clip_fades(&bundle, "alternate", "1beat", "0beat").expect("fade Clip"));
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            replacements,
            publication("Transform Clip"),
        )
        .expect("apply transforms");

        bundle = load_project(directory.path()).expect("transformed Project");
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![delete_clip(&bundle, "alternate-tail").expect("delete Clip")],
            publication("Delete Clip"),
        )
        .expect("apply delete");

        let final_bundle = load_project(directory.path()).expect("final Project");
        assert!(!final_bundle.diagnostics.has_errors());
        let source = fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source");
        assert!(source.contains("clip alternate pattern melody start 0qn duration 4qn mode loop"));
        assert!(source.contains("offset 0qn"));
        assert!(source.contains("transpose 7st"));
        assert!(source.contains("gain -3db"));
        assert!(source.contains("fade 1qn 0qn"));
        assert!(
            source.contains("clip alternate-copy pattern melody start 0qn duration 1qn mode loop")
        );
        assert!(!source.contains("clip alternate-tail "));
        let project = final_bundle
            .publishable()
            .expect("publishable Clip Project");
        let compiled = crate::engine::CompiledProject::compile(project).expect("compile Clips");
        let mut runtime = compiled.runtime();
        let sample = runtime
            .process_frame(crate::engine::ProcessingMode::Offline)
            .expect("finite Clip audio");
        assert!(sample.into_iter().all(f32::is_finite));
    }

    #[test]
    fn event_grid_edits_change_only_selected_literals() {
        let directory = tempdir().unwrap();
        let source = include_str!("../tests/fixtures/minimal.crump");
        fs::write(directory.path().join("project.crump"), source).unwrap();
        let bundle = load_project(directory.path()).unwrap();
        let replacements = vec![
            set_event_at(&bundle, "melody", 0, "1/2beat").unwrap(),
            set_note_pitch(&bundle, "melody", 0, 62).unwrap(),
            set_note_duration(&bundle, "melody", 0, "1/2beat").unwrap(),
            set_event_velocity(&bundle, "melody", 0, 0.65).unwrap(),
        ];
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            replacements,
            Publication::new(
                ChangeOrigin::human("tester"),
                "Edit note",
                "Event Grid change",
            ),
        )
        .unwrap();
        let changed = fs::read_to_string(directory.path().join("project.crump")).unwrap();
        assert_eq!(
            changed,
            source.replacen("note 0beat C4 1beat 0.8", "note 1/2qn D4 1/2qn 0.65", 1)
        );
    }

    #[test]
    fn automation_edits_preserve_readable_source_and_durable_point_identity() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("project.crump"), SOURCE).unwrap();
        let publication = || {
            Publication::new(
                ChangeOrigin::human("tester"),
                "Edit Automation",
                "Automation View change",
            )
        };

        let bundle = load_project(directory.path()).unwrap();
        let target: ParameterTarget = "track/drums-track/level".parse().unwrap();
        let replacement = create_automation_lane(&bundle, "drums-level", &target, "-12db").unwrap();
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![replacement],
            publication(),
        )
        .unwrap();

        let bundle = load_project(directory.path()).unwrap();
        let replacement = create_automation_point(
            &bundle,
            "drums-level",
            "lift",
            "3beat",
            "-6db",
            AutomationInterpolation::Linear,
        )
        .unwrap();
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![replacement],
            publication(),
        )
        .unwrap();

        let bundle = load_project(directory.path()).unwrap();
        let replacement = create_automation_point(
            &bundle,
            "drums-level",
            "middle",
            "2beat",
            "-9db",
            AutomationInterpolation::Step,
        )
        .unwrap();
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![replacement],
            publication(),
        )
        .unwrap();
        let inserted = fs::read_to_string(directory.path().join("project.crump")).unwrap();
        assert!(inserted.find("point middle").unwrap() < inserted.find("point lift").unwrap());

        let bundle = load_project(directory.path()).unwrap();
        let replacement = set_automation_point_at(&bundle, "drums-level", "lift", "1beat").unwrap();
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![replacement],
            publication(),
        )
        .unwrap();
        let moved = fs::read_to_string(directory.path().join("project.crump")).unwrap();
        assert!(moved.find("point lift").unwrap() < moved.find("point middle").unwrap());

        let bundle = load_project(directory.path()).unwrap();
        assert!(set_automation_point_at(&bundle, "drums-level", "middle", "1beat").is_err());
        assert_eq!(
            fs::read_to_string(directory.path().join("project.crump")).unwrap(),
            moved
        );

        let replacements = vec![
            set_automation_point_value(&bundle, "drums-level", "lift", "-3db").unwrap(),
            set_automation_point_interpolation(
                &bundle,
                "drums-level",
                "lift",
                AutomationInterpolation::Power(2.0),
            )
            .unwrap(),
        ];
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            replacements,
            publication(),
        )
        .unwrap();
        let changed = fs::read_to_string(directory.path().join("project.crump")).unwrap();
        assert!(changed.contains("automation drums-level target track/drums-track/level {"));
        assert!(changed.contains("point start 0qn -12db step"));
        assert!(changed.contains("point lift 1qn -3db power(2)"));
        assert!(changed.contains("point middle 2qn -9db step"));

        let bundle = load_project(directory.path()).unwrap();
        let replacement = delete_automation_point(&bundle, "drums-level", "lift").unwrap();
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![replacement],
            publication(),
        )
        .unwrap();
        let changed = fs::read_to_string(directory.path().join("project.crump")).unwrap();
        assert!(!changed.contains("point lift"));

        let bundle = load_project(directory.path()).unwrap();
        let replacement = delete_automation_lane(&bundle, "drums-level").unwrap();
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![replacement],
            publication(),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("project.crump")).unwrap(),
            SOURCE
        );
    }

    #[test]
    fn stable_mix_targets_edit_exact_source_literals() {
        let directory = tempdir().unwrap();
        fs::write(
            directory.path().join("project.crump"),
            include_str!("../tests/fixtures/production_mix.crump"),
        )
        .unwrap();
        let bundle = load_project(directory.path()).unwrap();
        let edits = [
            ("track/bass/left_pan", -0.75),
            ("track/drums/mute", 1.0),
            ("group/mix_bus/pan", 0.2),
            ("send/drum_room/level", -10.0),
            ("processor/bass_comp/threshold", -20.0),
            ("master/level", -2.0),
        ]
        .into_iter()
        .map(|(target, value)| {
            set_parameter_target(
                &bundle,
                &target.parse::<ParameterTarget>().expect("target"),
                value,
            )
            .expect("mix edit")
        })
        .collect();
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            edits,
            Publication::new(
                ChangeOrigin::human("tester"),
                "Edit production mix",
                "Stable target edits",
            ),
        )
        .unwrap();
        let changed = load_project(directory.path()).unwrap();
        let project = changed.project.expect("edited Project");
        for (target, value) in [
            ("track/bass/left_pan", -0.75),
            ("track/drums/mute", 1.0),
            ("group/mix_bus/pan", 0.2),
            ("send/drum_room/level", -10.0),
            ("processor/bass_comp/threshold", -20.0),
            ("master/level", -2.0),
        ] {
            let spec = project
                .parameter_spec(&target.parse().expect("target"))
                .expect("edited target");
            assert_eq!(spec.base, value, "{target}");
        }
    }

    #[test]
    fn processor_edits_preserve_exact_blocks_comments_and_history() {
        let directory = tempdir().unwrap();
        let source = include_str!("../tests/fixtures/production_mix.crump").replace(
            "      processor master_limiter limiter {",
            "      # mastering ceiling\n      processor master_limiter limiter {",
        );
        fs::write(directory.path().join("project.crump"), source).unwrap();
        let publication = |summary| {
            Publication::new(
                ChangeOrigin::human("tester"),
                summary,
                "Processing View change",
            )
        };

        let bundle = load_project(directory.path()).unwrap();
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![set_processor_setting(&bundle, "master_saturation", "drive", "1.75").unwrap()],
            publication("Set exact processor setting"),
        )
        .unwrap();

        let bundle = load_project(directory.path()).unwrap();
        let target = ParameterTarget::Processor {
            processor: "master_saturation".to_owned(),
            parameter: "bypass".to_owned(),
        };
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![set_parameter_target(&bundle, &target, 1.0).unwrap()],
            publication("Bypass processor"),
        )
        .unwrap();

        let bundle = load_project(directory.path()).unwrap();
        apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![move_processor(&bundle, "master_limiter", ProcessorMove::Earlier).unwrap()],
            publication("Reorder mastering processors"),
        )
        .unwrap();

        let changed = fs::read_to_string(directory.path().join("project.crump")).unwrap();
        assert!(changed.contains("setting drive 1.75"));
        assert!(changed.contains("bypass true"));
        let limiter = changed.find("processor master_limiter").unwrap();
        let comment = changed.find("# mastering ceiling").unwrap();
        let saturation = changed.find("processor master_saturation").unwrap();
        assert!(comment < limiter && limiter < saturation);

        let bundle = load_project(directory.path()).unwrap();
        let project = bundle.project.expect("edited Project");
        let master = project.song.master.expect("Master");
        assert_eq!(master.inserts[0].id, "master_limiter");
        assert_eq!(master.inserts[1].id, "master_saturation");
        assert!(master.inserts[1].bypass);
        assert_eq!(
            master.inserts[1]
                .settings
                .iter()
                .find(|setting| setting.key == "drive")
                .unwrap()
                .value,
            "1.75"
        );
        assert_eq!(
            HistoryStore::open(directory.path())
                .unwrap()
                .list()
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn changes_only_the_selected_literal_and_publishes() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("project.crump"), SOURCE).unwrap();
        let bundle = load_project(directory.path()).unwrap();
        let replacement = set_parameter(&bundle, "drums", "tone", 0.75).unwrap();
        let entry = apply_structured_edit(
            directory.path(),
            &bundle.revision,
            vec![replacement],
            Publication::new(
                ChangeOrigin::human("tester"),
                "Brighten drums",
                "Faceplate change",
            ),
        )
        .unwrap();
        let changed = fs::read_to_string(directory.path().join("project.crump")).unwrap();
        assert_eq!(changed, SOURCE.replacen("0.5\n", "0.75\n", 1));
        assert_eq!(entry.changes.len(), 1);
    }
}
