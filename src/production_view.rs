//! Terminal-independent execution seam for durable Production View operations.
//!
//! Adapters translate their input into [`Operation`]. This module owns shared value parsing,
//! structure-preserving edit planning, checked application, publication, and history provenance.

use std::path::Path;

use crate::{
    diagnostic::Diagnostic,
    edit::{
        CheckedReplacement, EditError, ProcessorMove, apply_structured_edit,
        fade_sample_occurrence, move_processor, set_automation_point_at,
        set_automation_point_interpolation, set_automation_point_value, set_event_at,
        set_event_velocity, set_hit_voice, set_note_duration, set_note_pitch, set_parameter,
        set_parameter_target, set_processor_setting, set_sample_occurrence_gain,
        set_sample_occurrence_pan, set_track_level, set_track_pan, trim_sample_occurrence,
    },
    history::{ChangeOrigin, HistoryEntry, HistoryError, HistoryStore, Publication},
    model::{AutomationInterpolation, DrumVoice, ParameterTarget, Unit, parse_pitch},
    project::{ProjectBundle, ProjectIoError, RevisionId, load_project},
};

/// A physical value supplied either as an adapter-parsed number or exact authored text.
#[derive(Debug, Clone, PartialEq)]
pub enum PhysicalValue {
    /// A number already parsed by an adapter whose public syntax requires a bare number.
    Number(f64),
    /// Exact physical-unit text entered by a Production View.
    Authored(String),
}

impl From<f64> for PhysicalValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<String> for PhysicalValue {
    fn from(value: String) -> Self {
        Self::Authored(value)
    }
}

impl From<&str> for PhysicalValue {
    fn from(value: &str) -> Self {
        Self::Authored(value.to_owned())
    }
}

/// Direction of a checked adjacent processor move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDirection {
    /// Move toward the start of the authored chain.
    Earlier,
    /// Move toward the end of the authored chain.
    Later,
}

/// One terminal-independent durable operation requested by a Production View adapter.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum Operation {
    /// Set any stable Parameter target.
    SetParameterTarget {
        target: String,
        value: PhysicalValue,
    },
    /// Set one exact Instrument Parameter.
    SetInstrumentParameter {
        instrument: String,
        parameter: String,
        value: PhysicalValue,
    },
    /// Set a Track level.
    SetTrackLevel { track: String, value: PhysicalValue },
    /// Set a Track pan position.
    SetTrackPan { track: String, value: PhysicalValue },
    /// Set or insert one processor setting literal.
    SetProcessorSetting {
        processor: String,
        setting: String,
        value: String,
    },
    /// Set one durable processor bypass.
    SetProcessorBypass { processor: String, bypass: bool },
    /// Move one processor to an adjacent position in its chain.
    MoveProcessor {
        processor: String,
        direction: MoveDirection,
    },
    /// Set one Automation point's exact Song time.
    SetAutomationPointTime {
        lane: String,
        point: String,
        time: String,
        /// Optional exact grid requested by an input adapter.
        snap_to: Option<String>,
    },
    /// Set one Automation point's exact physical value.
    SetAutomationPointValue {
        lane: String,
        point: String,
        value: String,
    },
    /// Set the interpolation leaving one Automation point.
    SetAutomationPointCurve {
        lane: String,
        point: String,
        curve: String,
    },
    /// Set a pitched Event's pitch.
    SetNotePitch {
        pattern: String,
        event: usize,
        pitch: String,
    },
    /// Set a concrete Event onset.
    SetEventTime {
        pattern: String,
        event: usize,
        time: String,
        /// Optional exact grid requested by an input adapter.
        snap_to: Option<String>,
    },
    /// Set a pitched-note or sample duration.
    SetNoteDuration {
        pattern: String,
        event: usize,
        duration: String,
        /// Optional exact grid requested by an input adapter.
        snap_to: Option<String>,
    },
    /// Set a concrete Event velocity.
    SetEventVelocity {
        pattern: String,
        event: usize,
        value: PhysicalValue,
    },
    /// Set a concrete percussion Event voice.
    SetHitVoice {
        pattern: String,
        event: usize,
        voice: String,
    },
    /// Trim one sample occurrence to exact source frames.
    TrimSample {
        pattern: String,
        occurrence: String,
        start_frame: u64,
        end_frame: u64,
    },
    /// Set one sample occurrence's source-edge fades.
    FadeSample {
        pattern: String,
        occurrence: String,
        fade_in_frames: u32,
        fade_out_frames: u32,
    },
    /// Set one sample occurrence's gain trim.
    SetSampleGain {
        pattern: String,
        occurrence: String,
        value: PhysicalValue,
    },
    /// Set one sample occurrence's pan.
    SetSamplePan {
        pattern: String,
        occurrence: String,
        value: PhysicalValue,
    },
    /// Publish an already-authored valid Candidate Revision.
    PublishCandidate,
    /// Restore an exact historical snapshot as a new append-only entry.
    RestoreHistory { entry_id: String },
}

/// History provenance supplied by an adapter without exposing presentation concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Durable origin and actor identity.
    pub origin: ChangeOrigin,
    /// Concise History list description.
    pub summary: String,
    /// Durable rationale or operation explanation.
    pub explanation: String,
}

impl Provenance {
    /// Creates provenance for a human adapter.
    #[must_use]
    pub fn human(
        actor: impl Into<String>,
        summary: impl Into<String>,
        explanation: impl Into<String>,
    ) -> Self {
        Self {
            origin: ChangeOrigin::human(actor),
            summary: summary.into(),
            explanation: explanation.into(),
        }
    }

    fn publication(&self) -> Publication {
        Publication::new(
            self.origin.clone(),
            self.summary.clone(),
            self.explanation.clone(),
        )
    }
}

/// One request sent through the durable Production View executor seam.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Candidate revision observed by an interactive adapter, when it has one.
    pub expected_revision: Option<RevisionId>,
    /// Typed durable operation.
    pub operation: Operation,
    /// History provenance for the resulting publication.
    pub provenance: Provenance,
}

/// Presentation-neutral class of a completed durable operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeKind {
    /// A Structured Source Edit was planned, checked, and published.
    EditApplied,
    /// An external Candidate was published.
    CandidatePublished,
    /// A historical state was restored as a new entry.
    HistoryRestored,
}

/// Presentation-neutral durable-operation result.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// Operation class.
    pub kind: OutcomeKind,
    /// Appended Project History entry.
    pub history_entry: HistoryEntry,
}

/// Shared operation-value parse category, allowing adapters to retain their public error labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// Stable Parameter target syntax.
    ParameterTarget,
    /// Physical-unit value syntax.
    PhysicalValue,
    /// Exact musical-time syntax.
    MusicalTime,
    /// Exact snap-grid syntax or operation.
    TimeGrid,
    /// Automation interpolation syntax.
    AutomationCurve,
    /// Pitch-name syntax.
    Pitch,
    /// Percussion voice syntax.
    DrumVoice,
}

/// Failure from the terminal-independent Production View executor.
#[derive(Debug, thiserror::Error)]
pub enum ExecuteError {
    /// The current Candidate cannot be a Structured Source Edit base.
    #[error("Production View edits require a valid Active Revision")]
    InvalidBase {
        /// Candidate diagnostics retained for presentation by an adapter.
        diagnostics: Vec<Diagnostic>,
    },
    /// A shared operation value could not be parsed.
    #[error("{message}")]
    InvalidOperationValue {
        /// Value category.
        kind: ValueKind,
        /// Parser message.
        message: String,
    },
    /// Structured edit planning or checked application failed.
    #[error(transparent)]
    Edit(#[from] EditError),
    /// Project loading failed.
    #[error(transparent)]
    Project(#[from] ProjectIoError),
    /// Publication or restoration failed.
    #[error(transparent)]
    History(#[from] HistoryError),
}

impl ExecuteError {
    /// Returns diagnostics carried by an invalid base or invalid edited result.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::InvalidBase { diagnostics }
            | Self::Edit(EditError::InvalidResult(diagnostics)) => diagnostics,
            _ => &[],
        }
    }

    /// Returns the shared value category when failure happened during value parsing.
    #[must_use]
    pub const fn value_kind(&self) -> Option<ValueKind> {
        match self {
            Self::InvalidOperationValue { kind, .. } => Some(*kind),
            _ => None,
        }
    }
}

/// Executes one durable operation independently of CLI or terminal rendering.
pub fn execute(root: impl AsRef<Path>, request: Request) -> Result<Outcome, ExecuteError> {
    let root = root.as_ref();
    match &request.operation {
        Operation::RestoreHistory { entry_id } => {
            let store = HistoryStore::open(root)?;
            let bundle = load_project(root)?;
            ensure_expected_revision(&bundle, request.expected_revision.as_ref())?;
            let entry = store.restore(
                entry_id,
                request.provenance.origin,
                &request.provenance.explanation,
            )?;
            Ok(Outcome {
                kind: OutcomeKind::HistoryRestored,
                history_entry: entry,
            })
        }
        Operation::PublishCandidate => {
            let bundle = load_project(root)?;
            ensure_expected_revision(&bundle, request.expected_revision.as_ref())?;
            if bundle.diagnostics.has_errors() || bundle.project.is_none() {
                return Err(ExecuteError::InvalidBase {
                    diagnostics: bundle.diagnostics.into_vec(),
                });
            }
            let store = HistoryStore::open(&bundle.root)?;
            let entry = store.publish(&bundle, request.provenance.publication())?;
            Ok(Outcome {
                kind: OutcomeKind::CandidatePublished,
                history_entry: entry,
            })
        }
        operation => {
            let bundle = load_project(root)?;
            ensure_expected_revision(&bundle, request.expected_revision.as_ref())?;
            if bundle.diagnostics.has_errors() || bundle.project.is_none() {
                return Err(ExecuteError::InvalidBase {
                    diagnostics: bundle.diagnostics.into_vec(),
                });
            }
            let replacements = plan(&bundle, operation)?;
            let entry = apply_structured_edit(
                &bundle.root,
                &bundle.revision,
                replacements,
                request.provenance.publication(),
            )?;
            Ok(Outcome {
                kind: OutcomeKind::EditApplied,
                history_entry: entry,
            })
        }
    }
}

fn ensure_expected_revision(
    bundle: &ProjectBundle,
    expected: Option<&RevisionId>,
) -> Result<(), ExecuteError> {
    if let Some(expected) = expected
        && expected != &bundle.revision
    {
        return Err(EditError::StaleRevision {
            expected: expected.clone(),
            actual: bundle.revision.clone(),
        }
        .into());
    }
    Ok(())
}

fn plan(
    bundle: &ProjectBundle,
    operation: &Operation,
) -> Result<Vec<CheckedReplacement>, ExecuteError> {
    let replacements = match operation {
        Operation::SetParameterTarget { target, value } => {
            let target = target
                .parse::<ParameterTarget>()
                .map_err(|error| invalid(ValueKind::ParameterTarget, error))?;
            let unit = bundle
                .project
                .as_ref()
                .and_then(|project| project.parameter_spec(&target))
                .map(|spec| spec.unit);
            vec![set_parameter_target(
                bundle,
                &target,
                physical_value(value, unit)?,
            )?]
        }
        Operation::SetInstrumentParameter {
            instrument,
            parameter,
            value,
        } => {
            let unit = bundle
                .project
                .as_ref()
                .and_then(|project| {
                    project
                        .song
                        .instruments
                        .iter()
                        .find(|item| item.id == *instrument)
                })
                .and_then(|item| item.parameters.iter().find(|item| item.key == *parameter))
                .map(|parameter| parameter.unit);
            vec![set_parameter(
                bundle,
                instrument,
                parameter,
                physical_value(value, unit)?,
            )?]
        }
        Operation::SetTrackLevel { track, value } => {
            vec![set_track_level(
                bundle,
                track,
                physical_value(value, Some(Unit::Decibels))?,
            )?]
        }
        Operation::SetTrackPan { track, value } => {
            vec![set_track_pan(
                bundle,
                track,
                physical_value(value, Some(Unit::Pan))?,
            )?]
        }
        Operation::SetProcessorSetting {
            processor,
            setting,
            value,
        } => {
            vec![set_processor_setting(bundle, processor, setting, value)?]
        }
        Operation::SetProcessorBypass { processor, bypass } => {
            let target = ParameterTarget::Processor {
                processor: processor.clone(),
                parameter: "bypass".to_owned(),
            };
            vec![set_parameter_target(bundle, &target, f64::from(*bypass))?]
        }
        Operation::MoveProcessor {
            processor,
            direction,
        } => vec![move_processor(
            bundle,
            processor,
            match direction {
                MoveDirection::Earlier => ProcessorMove::Earlier,
                MoveDirection::Later => ProcessorMove::Later,
            },
        )?],
        Operation::SetAutomationPointTime {
            lane,
            point,
            time,
            snap_to,
        } => {
            let time = resolve_time(bundle, time, snap_to.as_deref())?;
            vec![set_automation_point_at(bundle, lane, point, &time)?]
        }
        Operation::SetAutomationPointValue { lane, point, value } => {
            vec![set_automation_point_value(bundle, lane, point, value)?]
        }
        Operation::SetAutomationPointCurve { lane, point, curve } => {
            let curve = curve
                .parse::<AutomationInterpolation>()
                .map_err(|error| invalid(ValueKind::AutomationCurve, error))?;
            vec![set_automation_point_interpolation(
                bundle, lane, point, curve,
            )?]
        }
        Operation::SetNotePitch {
            pattern,
            event,
            pitch,
        } => {
            let pitch = parse_pitch(pitch).map_err(|error| invalid(ValueKind::Pitch, error))?;
            vec![set_note_pitch(bundle, pattern, *event, pitch)?]
        }
        Operation::SetEventTime {
            pattern,
            event,
            time,
            snap_to,
        } => {
            let time = resolve_time(bundle, time, snap_to.as_deref())?;
            vec![set_event_at(bundle, pattern, *event, &time)?]
        }
        Operation::SetNoteDuration {
            pattern,
            event,
            duration,
            snap_to,
        } => {
            let duration = resolve_time(bundle, duration, snap_to.as_deref())?;
            vec![set_note_duration(bundle, pattern, *event, &duration)?]
        }
        Operation::SetEventVelocity {
            pattern,
            event,
            value,
        } => vec![set_event_velocity(
            bundle,
            pattern,
            *event,
            physical_value(value, Some(Unit::Ratio))?,
        )?],
        Operation::SetHitVoice {
            pattern,
            event,
            voice,
        } => {
            let voice = voice
                .parse::<DrumVoice>()
                .map_err(|error| invalid(ValueKind::DrumVoice, error))?;
            vec![set_hit_voice(bundle, pattern, *event, voice)?]
        }
        Operation::TrimSample {
            pattern,
            occurrence,
            start_frame,
            end_frame,
        } => trim_sample_occurrence(bundle, pattern, occurrence, *start_frame, *end_frame)?,
        Operation::FadeSample {
            pattern,
            occurrence,
            fade_in_frames,
            fade_out_frames,
        } => fade_sample_occurrence(
            bundle,
            pattern,
            occurrence,
            *fade_in_frames,
            *fade_out_frames,
        )?,
        Operation::SetSampleGain {
            pattern,
            occurrence,
            value,
        } => vec![set_sample_occurrence_gain(
            bundle,
            pattern,
            occurrence,
            physical_value(value, Some(Unit::Decibels))?,
        )?],
        Operation::SetSamplePan {
            pattern,
            occurrence,
            value,
        } => vec![set_sample_occurrence_pan(
            bundle,
            pattern,
            occurrence,
            physical_value(value, Some(Unit::Pan))?,
        )?],
        Operation::PublishCandidate | Operation::RestoreHistory { .. } => {
            unreachable!("non-edit operations are handled before planning")
        }
    };
    Ok(replacements)
}

fn resolve_time(
    bundle: &ProjectBundle,
    value: &str,
    snap_to: Option<&str>,
) -> Result<String, ExecuteError> {
    let project = bundle.project.as_ref().ok_or(EditError::InvalidBase)?;
    let resolved = project
        .song
        .locators
        .iter()
        .find(|locator| locator.id == value)
        .map_or_else(|| value.to_owned(), |locator| locator.at.to_string());
    let Some(grid) = snap_to else {
        return Ok(resolved);
    };
    let value = crate::time::parse_musical_time(&resolved, project.song.meter)
        .map_err(|error| invalid(ValueKind::MusicalTime, error))?;
    let grid = crate::time::parse_musical_time(grid, project.song.meter)
        .map_err(|error| invalid(ValueKind::TimeGrid, error))?;
    value
        .snap_to_grid(grid)
        .map(|value| value.to_string())
        .map_err(|error| invalid(ValueKind::TimeGrid, error))
}

fn physical_value(value: &PhysicalValue, unit: Option<Unit>) -> Result<f64, ExecuteError> {
    match value {
        PhysicalValue::Number(value) => Ok(*value),
        PhysicalValue::Authored(value) => unit
            .ok_or_else(|| invalid(ValueKind::PhysicalValue, "operation target is unavailable"))?
            .parse_value(value.trim())
            .map_err(|error| invalid(ValueKind::PhysicalValue, error)),
    }
}

fn invalid(kind: ValueKind, error: impl std::fmt::Display) -> ExecuteError {
    ExecuteError::InvalidOperationValue {
        kind,
        message: error.to_string(),
    }
}

/// Cohesive CLI-only mutation entry points stay internal rather than widening the public editor
/// inventory. This marker keeps the module boundary explicit while those adapters are migrated.
pub(crate) fn is_shared_edit(operation: &Operation) -> bool {
    !matches!(
        operation,
        Operation::PublishCandidate | Operation::RestoreHistory { .. }
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn project(root: &Path) {
        fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/minimal.crump"),
            root.join(crate::project::PROJECT_FILE),
        )
        .expect("copy Project fixture");
    }

    #[test]
    fn cli_and_ratatui_values_execute_through_the_same_operation_seam() {
        let cli_root = tempfile::tempdir().expect("CLI Project");
        let tui_root = tempfile::tempdir().expect("Ratatui Project");
        project(cli_root.path());
        project(tui_root.path());

        for (root, value, actor, explanation) in [
            (
                cli_root.path(),
                PhysicalValue::Number(-7.25),
                "cli",
                "Checked Production View edit",
            ),
            (
                tui_root.path(),
                PhysicalValue::Authored("-7.25db".to_owned()),
                "workspace",
                "Checked source-backed Production View edit",
            ),
        ] {
            let outcome = execute(
                root,
                Request {
                    expected_revision: None,
                    operation: Operation::SetTrackLevel {
                        track: "lead".to_owned(),
                        value,
                    },
                    provenance: Provenance::human(actor, "Shared level change", explanation),
                },
            )
            .expect("execute shared operation");
            assert_eq!(outcome.kind, OutcomeKind::EditApplied);
            assert_eq!(outcome.history_entry.summary, "Shared level change");
            assert_eq!(outcome.history_entry.origin.name, actor);
        }

        let cli_source = fs::read(cli_root.path().join(crate::project::PROJECT_FILE)).unwrap();
        let tui_source = fs::read(tui_root.path().join(crate::project::PROJECT_FILE)).unwrap();
        assert_eq!(cli_source, tui_source);
    }

    #[test]
    fn stale_history_restore_is_atomic() {
        let root = tempfile::tempdir().expect("Project");
        project(root.path());
        let published = load_project(root.path()).expect("published Candidate");
        let target_id = {
            let store = HistoryStore::open(root.path()).expect("History");
            store
                .publish(
                    &published,
                    Publication::new(
                        ChangeOrigin::human("test"),
                        "Initial publication",
                        "Create a restore target",
                    ),
                )
                .expect("publish restore target")
                .id
        };

        let source_path = root.path().join(crate::project::PROJECT_FILE);
        let mut newer_source = fs::read_to_string(&source_path).expect("Candidate source");
        newer_source.push_str("\n# newer Candidate\n");
        fs::write(&source_path, newer_source).expect("write newer Candidate");
        let newer = load_project(root.path()).expect("newer Candidate");
        assert_ne!(published.revision, newer.revision);
        let before = fs::read(&source_path).expect("newer Candidate bytes");

        let error = execute(
            root.path(),
            Request {
                expected_revision: Some(published.revision.clone()),
                operation: Operation::RestoreHistory {
                    entry_id: target_id,
                },
                provenance: Provenance::human("workspace", "Restore", "Checked restore"),
            },
        )
        .expect_err("stale restore request");
        assert!(matches!(
            error,
            ExecuteError::Edit(EditError::StaleRevision {
                expected,
                actual,
            }) if expected == published.revision && actual == newer.revision
        ));
        assert_eq!(fs::read(source_path).expect("Candidate bytes"), before);
    }

    #[test]
    fn stale_interactive_request_is_atomic() {
        let root = tempfile::tempdir().expect("Project");
        project(root.path());
        let before = fs::read(root.path().join(crate::project::PROJECT_FILE)).unwrap();
        let error = execute(
            root.path(),
            Request {
                expected_revision: Some(RevisionId("0".repeat(64))),
                operation: Operation::SetNotePitch {
                    pattern: "lead-line".to_owned(),
                    event: 0,
                    pitch: "D4".to_owned(),
                },
                provenance: Provenance::human("workspace", "Pitch", "Checked edit"),
            },
        )
        .expect_err("stale request");
        assert!(matches!(
            error,
            ExecuteError::Edit(EditError::StaleRevision { .. })
        ));
        assert_eq!(
            fs::read(root.path().join(crate::project::PROJECT_FILE)).unwrap(),
            before
        );
    }
}
