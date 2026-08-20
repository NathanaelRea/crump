//! Checked, structure-preserving source transactions for Production Views.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    history::{HistoryEntry, HistoryError, HistoryStore, Publication, write_atomic},
    model::{DrumVoice, PatternEvent, Unit, ValueSite, format_pitch},
    project::{ProjectBundle, ProjectIoError, RevisionId, load_project, load_project_sources},
    time::parse_musical_time,
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

/// Applies one atomic, checked Production View transaction and publishes it.
pub fn apply_structured_edit(
    root: impl AsRef<Path>,
    expected_revision: &RevisionId,
    replacements: Vec<CheckedReplacement>,
    publication: Publication,
) -> Result<HistoryEntry, EditError> {
    if replacements.is_empty() {
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

    let changed_sources = apply_in_memory(&current, &replacements)?;
    let candidate = load_project_sources(current.root.clone(), changed_sources.clone());
    if candidate.diagnostics.has_errors() || candidate.project.is_none() {
        return Err(EditError::InvalidResult(candidate.diagnostics.into_vec()));
    }

    let changed_paths: Vec<_> = replacements
        .iter()
        .map(|replacement| replacement.file.clone())
        .collect();
    store.begin_source_transaction(&current, &candidate)?;
    if let Err(error) = write_changed_sources(&candidate.root, &changed_paths, &changed_sources) {
        if let Err(rollback) = store.abort_source_transaction() {
            return Err(EditError::History(rollback));
        }
        return Err(error);
    }
    let entry = match store.publish(&candidate, publication) {
        Ok(entry) => entry,
        Err(error) => {
            if let Err(rollback) = store.abort_source_transaction() {
                return Err(EditError::History(rollback));
            }
            return Err(EditError::History(error));
        }
    };
    store.finish_source_transaction()?;
    Ok(entry)
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
        &track.level_site,
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
    Ok(CheckedReplacement::at(
        &track.pan_site,
        Unit::Pan.format_value(pan),
    ))
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
        PatternEvent::Hit { .. } => Err(EditError::WrongTargetKind(
            "a percussion hit has no pitched-note value".to_owned(),
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
    };
    Ok(CheckedReplacement::at(
        site,
        Unit::Ratio.format_value(velocity),
    ))
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
        PatternEvent::Note { .. } => Err(EditError::WrongTargetKind(
            "a pitched note has no percussion voice".to_owned(),
        )),
    }
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
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent).map_err(|source| EditError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
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

    use crate::history::{ChangeOrigin, Publication};

    use super::*;

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
