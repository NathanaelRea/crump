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

mod automation;
mod media;
mod timeline;

pub use automation::*;
pub use media::*;
use media::{quote_source_string, valid_source_id};
pub use timeline::*;

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
mod tests;
