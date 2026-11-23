//! Client-independent workspace actions and the session entry points that apply them.
//!
//! Every interactive change to a [`Workspace`] goes through [`Workspace::dispatch`]. Client
//! adapters own the mapping from their input (keys, pointer, menus) to [`Action`]; this module
//! depends on no terminal or windowing library.

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use super::{
    AuditionCue, HeldNote, Modal, TimelinePosition, View, Workspace, WorkspaceError, timeline,
};
use crate::{
    history::write_atomic,
    model::{ParameterTarget, TapPoint, Unit},
    production_view::{MoveDirection, Operation as ProductionViewOperation, PhysicalValue},
};

/// One client-independent request to change workspace state.
///
/// Actions act on the current selection. An adapter decides which actions it offers in which
/// [`View`]; the workspace itself reports an action that cannot apply through its status line.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Action {
    /// Request an orderly shutdown; see [`Workspace::quit_requested`].
    Quit,
    /// Leave Note Audition or a Timeline range selection and return to the normal mode.
    CancelMode,
    /// Close the open modal and report the cancellation.
    CancelModal,
    /// Replace the status line with an adapter-originated message.
    SetStatus(String),

    /// Show one view.
    ShowView(View),
    /// Step through the views in their fixed order.
    CycleView(isize),
    /// Step the view's secondary context, such as the selected Pattern or Instrument.
    CycleContext(isize),
    /// Move the view's primary selection by a signed row count.
    MoveSelection(isize),
    /// Move the view's primary selection to its first or last row.
    MoveSelectionToEdge {
        /// Whether to move to the first row instead of the last.
        first: bool,
    },
    /// Nudge the selected value by one signed step.
    AdjustSelected(isize),
    /// Open exact entry for the selected value.
    OpenExactEdit,
    /// Scroll the Source view back to its first line.
    ScrollSourceToTop,
    /// Ask the client to open the selected source file in an external editor.
    EditSource,

    /// Reload the Candidate Revision and Project History from disk.
    Reload,
    /// Open the Project Publication prompt.
    OpenPublish,
    /// Open the restore confirmation for the selected History entry.
    OpenRestore,
    /// Show the source diff of the selected History entry.
    OpenHistoryDiff,
    /// Show what restoring the selected History entry would change: the Active Revision's
    /// source against the selected entry's.
    CompareHistoryWithActive,
    /// Audition a History version (by entry ID) in place of the Active Revision, or return to
    /// the Active Revision with `None`. While playing, the transport position is kept.
    PreviewHistoryVersion(Option<String>),
    /// Show or hide workspace performance diagnostics.
    TogglePerformanceDiagnostics,

    /// Start, pause, or resume playback.
    TogglePlayback,
    /// Stop playback and release the audio stream.
    StopPlayback,
    /// Arm the selected Timeline or Event Grid item as the audition target.
    ArmSelectedAudition,
    /// Step the audition path of the current target.
    CycleAuditionPath,
    /// Open exact entry for the audition target.
    OpenAuditionTarget,
    /// Open exact entry for the audition range.
    OpenAuditionRange,
    /// Toggle looping of the current audition range.
    ToggleAuditionRangeLoop,
    /// Toggle the exact loop of the selected Pattern.
    TogglePatternLoop,

    /// Enter Note Audition mode.
    EnterNoteMode,
    /// Start one audition note on the audition track.
    NoteOn {
        /// Adapter-chosen identity of the held key, used to release the same note.
        key: char,
        /// MIDI pitch to sound.
        pitch: u8,
    },
    /// Start one audition note on the audition track with an explicit velocity (`0.0..=1.0`).
    NoteOnVelocity {
        /// Adapter-chosen identity of the held key, used to release the same note.
        key: char,
        /// MIDI pitch to sound.
        pitch: u8,
        /// Normalized note velocity in `0.0..=1.0`.
        velocity: f32,
    },
    /// Release the audition note started with the same key identity.
    NoteOff {
        /// Adapter-chosen identity passed to [`Action::NoteOn`].
        key: char,
    },

    /// Directly set any stable Parameter target (fader, pan, send, mute, processor parameter).
    SetParameterTargetValue {
        /// Stable parameter target string.
        target: String,
        /// Numeric physical value in the target's canonical unit.
        value: f64,
    },
    /// Directly set an Instrument Parameter to a numeric physical value.
    SetInstrumentParameterValue {
        /// Owning Instrument durable ID.
        instrument: String,
        /// Parameter key on the Instrument.
        parameter: String,
        /// Numeric physical value in the parameter's canonical unit.
        value: f64,
    },
    /// Directly set a Processor setting to a formatted physical or choice literal.
    SetProcessorSettingValue {
        /// Durable Processor ID.
        processor: String,
        /// Processor setting key.
        setting: String,
        /// Formatted setting literal.
        value: String,
    },
    /// Toggle durable source Mute on one specific channel (`track/id`, `group/id`, `return/id`).
    ToggleChannelMute {
        /// Channel identity string (`track/<id>`, `group/<id>`, `return/<id>`).
        identity: String,
    },
    /// Toggle transient Solo on one specific channel.
    ToggleChannelSolo {
        /// Channel identity string (`track/<id>`, `group/<id>`, `return/<id>`).
        identity: String,
    },
    /// Toggle transient Cue on one specific channel.
    ToggleChannelCue {
        /// Channel identity string (`track/<id>`, `group/<id>`, `return/<id>`).
        identity: String,
        /// Pre-fader or post-fader cue tap point.
        tap: TapPoint,
    },
    /// Clear all transient channel solos.
    ClearAllSolos,
    /// Toggle transient bypass on one specific Processor.
    ToggleProcessorTransientBypass {
        /// Durable Processor ID.
        processor: String,
    },
    /// Toggle durable source bypass on one specific Processor.
    ToggleProcessorDurableBypass {
        /// Durable Processor ID.
        processor: String,
    },
    /// Move one specific Processor earlier or later in its chain.
    MoveProcessor {
        /// Durable Processor ID.
        processor: String,
        /// Direction to move within the chain.
        direction: MoveDirection,
    },
    /// Set an Instrument modulation route depth directly.
    SetInstrumentRouteDepth {
        /// Owning Instrument durable ID.
        instrument: String,
        /// Modulation route ID.
        route: String,
        /// Formatted physical depth literal.
        depth: String,
    },
    /// Toggle durable bypass of one specific Instrument modulation route.
    ToggleInstrumentRouteBypassById {
        /// Owning Instrument durable ID.
        instrument: String,
        /// Modulation route ID.
        route: String,
    },
    /// Directly set one Automation point's physical value.
    SetAutomationPointValue {
        /// Automation lane ID.
        lane: String,
        /// Automation point ID.
        point: String,
        /// Formatted physical value literal.
        value: String,
    },
    /// Directly set one Automation point's Song time.
    SetAutomationPointTime {
        /// Automation lane ID.
        lane: String,
        /// Automation point ID.
        point: String,
        /// Musical time literal.
        time: String,
    },
    /// Directly set one Automation point's outgoing interpolation curve.
    SetAutomationPointCurve {
        /// Automation lane ID.
        lane: String,
        /// Automation point ID.
        point: String,
        /// Curve literal (`step`, `linear`, `power(2)`).
        curve: String,
    },
    /// Create a new durable Automation lane with an initial point.
    CreateAutomationLane {
        /// Fresh Automation lane ID.
        lane: String,
        /// Target parameter string.
        target: String,
        /// Initial physical value literal.
        initial_value: String,
    },
    /// Create a new durable Automation point in a lane.
    CreateAutomationPoint {
        /// Automation lane ID.
        lane: String,
        /// Fresh point ID.
        point: String,
        /// Musical time literal.
        at: String,
        /// Formatted physical value literal.
        value: String,
        /// Outgoing curve literal.
        curve: String,
    },
    /// Delete a durable Automation point from a lane.
    DeleteAutomationPoint {
        /// Automation lane ID.
        lane: String,
        /// Point ID to remove.
        point: String,
    },
    /// Directly set a Pattern Event's normalized velocity.
    SetEventVelocityValue {
        /// Pattern ID.
        pattern: String,
        /// Zero-based event index.
        event: usize,
        /// Normalized velocity in `0.0..=1.0`.
        velocity: f64,
    },
    /// Directly set a Pattern Event's onset time.
    SetEventTimeValue {
        /// Pattern ID.
        pattern: String,
        /// Zero-based event index.
        event: usize,
        /// Musical time literal.
        time: String,
    },
    /// Directly set a Pattern Note's duration.
    SetNoteDurationValue {
        /// Pattern ID.
        pattern: String,
        /// Zero-based event index.
        event: usize,
        /// Musical duration literal.
        duration: String,
    },
    /// Directly set a Pattern Note's pitch.
    SetNotePitchValue {
        /// Pattern ID.
        pattern: String,
        /// Zero-based event index.
        event: usize,
        /// Pitch literal (e.g. `C3`, `F#4`).
        pitch: String,
    },
    /// Directly set a Pattern Hit's drum voice.
    SetHitVoiceValue {
        /// Pattern ID.
        pattern: String,
        /// Zero-based event index.
        event: usize,
        /// Drum voice literal (`kick`, `snare`, `hat`, `clap`).
        voice: String,
    },
    /// Create a new pitched Note in a Pattern.
    CreatePatternNote {
        /// Pattern ID.
        pattern: String,
        /// Musical onset literal.
        at: String,
        /// Pitch literal.
        pitch: String,
        /// Duration literal.
        duration: String,
        /// Normalized velocity.
        velocity: f64,
    },
    /// Create a new drum Hit in a Pattern.
    CreatePatternHit {
        /// Pattern ID.
        pattern: String,
        /// Musical onset literal.
        at: String,
        /// Drum voice literal.
        voice: String,
        /// Normalized velocity.
        velocity: f64,
    },
    /// Delete one concrete Event from a Pattern.
    DeletePatternEvent {
        /// Pattern ID.
        pattern: String,
        /// Zero-based event index.
        event: usize,
    },
    /// Create a new arranged Clip on a Track.
    CreateClip {
        /// Track ID.
        track: String,
        /// Fresh Clip ID.
        clip: String,
        /// Existing Pattern ID.
        pattern: String,
        /// Musical start literal.
        start: String,
        /// Musical duration literal.
        duration: String,
    },
    /// Move one arranged Clip to a new start time.
    MoveClip {
        /// Durable Clip ID.
        clip: String,
        /// New musical start time literal.
        start: String,
    },
    /// Resize one arranged Clip's duration.
    ResizeClip {
        /// Durable Clip ID.
        clip: String,
        /// New musical duration literal.
        duration: String,
    },
    /// Duplicate one arranged Clip at a new start time.
    DuplicateClip {
        /// Source Clip ID.
        clip: String,
        /// Fresh Clip ID.
        new_id: String,
        /// Start time for the duplicate.
        start: String,
    },
    /// Split one arranged Clip at a Song time.
    SplitClip {
        /// Clip ID to split.
        clip: String,
        /// Fresh ID for the right-hand Clip.
        new_id: String,
        /// Song time at which to split.
        at: String,
    },
    /// Delete one arranged Clip.
    DeleteClip {
        /// Durable Clip ID to delete.
        clip: String,
    },
    /// Directly set the Song tempo in BPM.
    SetSongTempo {
        /// Tempo in beats per minute (`20.0..=300.0`).
        bpm: f64,
    },
    /// Restore the previous History entry as a quick Undo step.
    UndoHistory,
    /// Directly select a Mixer parameter row by index.
    SelectMixerParameter(usize),
    /// Directly select a Processor by index.
    SelectProcessor(usize),
    /// Directly select a Track by index.
    SelectTrack(usize),
    /// Directly select an Instrument by index.
    SelectInstrument(usize),
    /// Directly select a Pattern by index.
    SelectPattern(usize),
    /// Directly select an Automation lane by index.
    SelectAutomation(usize),
    /// Directly select an Automation point by index.
    SelectAutomationPoint(usize),
    /// Directly select an Event Grid event by index.
    SelectEvent(usize),
    /// Directly select a Graph item by index.
    SelectGraphNode(usize),

    /// Toggle mute on the selected Mixer channel.
    ToggleMixerMute,
    /// Toggle transient Solo on the selected Mixer channel.
    ToggleMixerSolo,
    /// Toggle transient cue of the selected Mixer channel at one tap.
    ToggleMixerCue(TapPoint),
    /// Toggle transient bypass of the selected Processor.
    ToggleTransientProcessorBypass,
    /// Toggle the authored bypass of the selected Processor.
    ToggleDurableProcessorBypass,
    /// Move the selected Processor one position within its chain.
    MoveSelectedProcessor(MoveDirection),
    /// Toggle bypass of the selected Instrument modulation route.
    ToggleInstrumentRouteBypass,
    /// Open exact entry for the selected Track's pan.
    OpenTrackPan,

    /// Open exact entry for the selected event's time.
    OpenEventTime,
    /// Open exact entry for the selected event's velocity.
    OpenEventVelocity,
    /// Open exact entry for the selected note's duration.
    OpenNoteDuration,
    /// Open exact entry for the selected sample occurrence's region.
    OpenSampleRegion,
    /// Open exact entry for the selected sample occurrence's fades.
    OpenSampleFade,
    /// Open exact entry for the selected sample occurrence's gain.
    OpenSampleGain,
    /// Open exact entry for the selected sample occurrence's pan.
    OpenSamplePan,
    /// Open exact entry for the selected automation point's time.
    OpenAutomationTime,
    /// Open exact entry for the selected automation point's curve.
    OpenAutomationCurve,

    /// Accept the open modal: commit its input, confirm it, or close it.
    ModalSubmit,
    /// Close the open modal without applying it.
    ModalDismiss,
    /// Delete the last character of the modal input.
    ModalBackspace,
    /// Select the whole modal input so the next typed character replaces it.
    ModalSelectAll,
    /// Append one character to the modal input.
    ModalInsert(char),
    /// Replace the whole modal input, for clients that edit it in a native text field.
    ModalSetInput(String),
    /// Scroll a text modal by a signed line count.
    ModalScroll(i16),

    /// A Timeline navigation or range action.
    Timeline(TimelineAction),
}

/// One Timeline cursor, viewport, or range action.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TimelineAction {
    /// Move the cursor by signed grid steps.
    MoveGrid(isize),
    /// Move the cursor to the next or previous Clip boundary.
    MoveBoundary(isize),
    /// Move the cursor to the end of the Clip under it.
    MoveClipEnd,
    /// Move the cursor to the opposite edge of the Clip under it.
    ToggleClipEdge,
    /// Move the cursor to the first or last Clip edge of the selected Track.
    MoveTrackEdge {
        /// Whether to move to the end instead of the start.
        end: bool,
    },
    /// Move the cursor to the start or end of the Song.
    MoveSongEdge {
        /// Whether to move to the end instead of the start.
        end: bool,
    },
    /// Move the cursor to the next or previous Clip of the same Pattern.
    MoveMatchingPattern(isize),
    /// Move the cursor to the next or previous Song Locator.
    MoveLocator(isize),
    /// Jump the cursor directly to a specific Song Locator by index.
    JumpToLocator(usize),
    /// Step back or forward through the cursor jump history.
    JumpHistory(isize),
    /// Remember the cursor under a named mark.
    SetMark(char),
    /// Return the cursor to a named mark.
    JumpMark(char),
    /// Move the cursor to a Track and to the grid position nearest a pointer.
    ///
    /// The pointer is given as one column of a `columns`-wide division of the viewport, so a
    /// client needs no Musical Time arithmetic of its own.
    PlaceCursor {
        /// Index of the Track under the pointer.
        track: usize,
        /// Zero-based column under the pointer.
        column: usize,
        /// Number of equal columns the client divides the viewport into.
        columns: usize,
    },

    /// Make the grid finer or coarser.
    ChangeGrid(isize),
    /// Zoom the viewport in or out.
    Zoom(isize),
    /// Pan the viewport by one step.
    Pan(isize),
    /// Pan the viewport by one full window.
    PanWindow(isize),
    /// Fit the whole Song in the viewport.
    Fit,
    /// Fit the selected range in the viewport.
    FitRange,
    /// Center the viewport on the cursor.
    CenterCursor,
    /// Align the viewport so the cursor sits at its start or end.
    AlignCursor {
        /// Whether to align the cursor to the end instead of the start.
        end: bool,
    },
    /// Toggle whether the viewport follows playback.
    ToggleFollow,

    /// Start a range selection at the cursor.
    BeginRange,
    /// Swap which edge of the range selection the cursor moves.
    SwapRangeEdge,
    /// Bring back the last range selection.
    RestoreRange,
    /// Audition the range selection, optionally as a loop.
    ApplyRange {
        /// Whether the range loops.
        looping: bool,
    },

    /// Open exact entry for a cursor position.
    OpenGoto,
    /// Open the Timeline search prompt.
    OpenSearch,
    /// Repeat the last Timeline search forward or backward.
    RepeatSearch(isize),
    /// Open the prompt that creates a durable Song Locator at the cursor.
    OpenLocator,

    /// Open the source definition of the item under the cursor.
    GoDefinition,
    /// Open the source file of the item under the cursor.
    GoSource,
    /// Show the Instrument of the selected Track.
    GoInstrument,
    /// Show the Mixer channel of the selected Track.
    GoMixer,
    /// Show the Processing chain of the selected Track.
    GoProcessing,
}

impl From<TimelineAction> for Action {
    fn from(action: TimelineAction) -> Self {
        Self::Timeline(action)
    }
}

impl Workspace {
    /// Opens the workspace for one Project and starts loading it in the background.
    ///
    /// The session is usable immediately; the exact Candidate Revision and Project History
    /// arrive through later [`Workspace::tick`] calls.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        Self::begin_load(path.as_ref())
    }

    /// Observes audio telemetry and finished background work.
    ///
    /// Returns whether visible state changed. Call this on every pass of the client's event
    /// loop, before drawing.
    pub fn tick(&mut self) -> bool {
        self.poll_audio();
        let mut changed = self.poll_background_work();
        changed |= self.release_stale_auditions();
        changed
    }

    /// Requests a check for external changes to Project source and History.
    ///
    /// The check runs in the background and is cheap to repeat; the first-party client calls it
    /// a few times per second. Returns whether the client should redraw now.
    pub fn poll_project_files(&mut self) -> bool {
        self.refresh_if_changed()
    }

    /// Whether background work is in flight, so the client should tick again soon.
    pub fn has_pending_work(&self) -> bool {
        self.background_work_pending()
    }

    /// Whether visible state advances without input, so the client should redraw continuously.
    pub fn is_animating(&self) -> bool {
        self.player.is_some() || self.performance.visible() || !self.held_notes.is_empty()
    }

    /// Whether [`Action::Quit`] was dispatched and the client should leave its event loop.
    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// Takes a pending [`Action::EditSource`] request, if any.
    ///
    /// The client owns how a source file is edited; the workspace only records the request.
    pub fn take_editor_request(&mut self) -> bool {
        std::mem::take(&mut self.editor_requested)
    }

    /// Prepares the selected source file for editing outside the workspace.
    ///
    /// Releases held audition notes and returns the absolute path to edit, writing the file
    /// first when it exists only as an imported snapshot. Call this after
    /// [`Workspace::take_editor_request`] reports a request; `None` means no file is selected.
    pub fn source_edit_path(&mut self) -> Result<Option<PathBuf>, WorkspaceError> {
        let Some(relative_path) = self
            .bundle
            .sources
            .keys()
            .nth(self.selected_source)
            .cloned()
        else {
            self.status = "No Project source file is selected".to_owned();
            return Ok(None);
        };
        if let Some(player) = self.player.as_mut() {
            let _ = player.all_notes_off();
        }
        self.held_notes.clear();
        let source_path = self.root.join(&relative_path);
        if !source_path.exists()
            && let Some(source) = self.bundle.sources.get(&relative_path)
        {
            // Personal-library imports are snapshotted under a safe project-local
            // display path. Editing one materializes that exact source locally so
            // the shared library is never modified as a side effect.
            write_atomic(&source_path, source.as_bytes())?;
        }
        Ok(Some(source_path))
    }

    /// Tells the workspace how many lines of help the client shows, bounding Help scrolling.
    pub fn set_help_line_count(&mut self, lines: usize) {
        self.help_line_count = lines;
    }

    /// Finishes any durable Project mutation and releases the audio stream.
    ///
    /// Call this once after the event loop ends and before the session is dropped.
    pub fn shutdown(&mut self) {
        self.finish_durable_mutation_before_shutdown();
        if let Some(player) = self.player.take() {
            self.retire_queue.retire(player);
        }
    }

    /// Applies one client-independent action to the workspace.
    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::Quit => self.request_quit(),
            Action::CancelMode => self.cancel_workspace_mode(),
            Action::CancelModal => {
                if self.modal.take().is_some() {
                    self.status = "Input cancelled".to_owned();
                }
            }
            Action::SetStatus(status) => self.status = status,

            Action::ShowView(view) => self.view = view,
            Action::CycleView(delta) => self.change_view(delta),
            Action::CycleContext(delta) => self.change_context(delta),
            Action::MoveSelection(delta) => self.move_selection(delta),
            Action::MoveSelectionToEdge { first } => self.move_selection_extreme(first),
            Action::AdjustSelected(direction) => self.adjust_selected(direction),
            Action::OpenExactEdit => self.open_exact_edit(),
            Action::ScrollSourceToTop => self.source_scroll = 0,
            Action::EditSource => self.editor_requested = true,

            Action::Reload => {
                self.refresh();
            }
            Action::OpenPublish => self.open_publish(),
            Action::OpenRestore => self.open_restore(),
            Action::OpenHistoryDiff => self.open_history_diff(),
            Action::CompareHistoryWithActive => self.compare_history_with_active(),
            Action::PreviewHistoryVersion(entry) => self.preview_history_version(entry),
            Action::TogglePerformanceDiagnostics => {
                let visible = self.performance.toggle();
                self.status = format!(
                    "Workspace diagnostics {} · summary prints after exit",
                    if visible { "enabled" } else { "hidden" }
                );
            }

            Action::TogglePlayback => self.toggle_playback(),
            Action::StopPlayback => self.stop_playback(),
            Action::ArmSelectedAudition => self.arm_selected_audition(),
            Action::CycleAuditionPath => self.cycle_audition_path(),
            Action::OpenAuditionTarget => self.open_audition_target(),
            Action::OpenAuditionRange => self.open_audition_range(),
            Action::ToggleAuditionRangeLoop => self.toggle_current_audition_loop(),
            Action::TogglePatternLoop => self.toggle_audition_loop(),

            Action::EnterNoteMode => {
                self.note_mode = true;
                self.status =
                    "NOTE mode · A W S E D F T G Y H U J K · Esc returns to Normal".to_owned();
            }
            Action::NoteOn { key, pitch } => self.audition_note_on_velocity(key, pitch, 0.8),
            Action::NoteOnVelocity {
                key,
                pitch,
                velocity,
            } => self.audition_note_on_velocity(key, pitch, velocity),
            Action::NoteOff { key } => self.release_audition(key),

            Action::SetParameterTargetValue { target, value } => {
                self.apply_operation(
                    ProductionViewOperation::SetParameterTarget {
                        target: target.clone(),
                        value: PhysicalValue::Number(value),
                    },
                    format!("Set {target}"),
                );
            }
            Action::SetInstrumentParameterValue {
                instrument,
                parameter,
                value,
            } => {
                self.apply_operation(
                    ProductionViewOperation::SetInstrumentParameter {
                        instrument: instrument.clone(),
                        parameter: parameter.clone(),
                        value: PhysicalValue::Number(value),
                    },
                    format!("Set {instrument}.{parameter}"),
                );
            }
            Action::SetProcessorSettingValue {
                processor,
                setting,
                value,
            } => {
                self.apply_operation(
                    ProductionViewOperation::SetProcessorSetting {
                        processor: processor.clone(),
                        setting: setting.clone(),
                        value: value.clone(),
                    },
                    format!("Set {processor}.{setting} = {value}"),
                );
            }
            Action::ToggleChannelMute { identity } => self.toggle_channel_mute_by_id(&identity),
            Action::ToggleChannelSolo { identity } => self.toggle_channel_solo_by_id(&identity),
            Action::ToggleChannelCue { identity, tap } => {
                self.toggle_channel_cue_by_id(&identity, tap);
            }
            Action::ClearAllSolos => self.clear_all_solos(),
            Action::ToggleProcessorTransientBypass { processor } => {
                self.toggle_processor_transient_bypass_by_id(&processor);
            }
            Action::ToggleProcessorDurableBypass { processor } => {
                self.toggle_processor_durable_bypass_by_id(&processor);
            }
            Action::MoveProcessor {
                processor,
                direction,
            } => {
                self.apply_operation(
                    ProductionViewOperation::MoveProcessor {
                        processor: processor.clone(),
                        direction,
                    },
                    format!("Move Processor {processor} {direction:?}"),
                );
            }
            Action::SetInstrumentRouteDepth {
                instrument,
                route,
                depth,
            } => {
                self.apply_operation(
                    ProductionViewOperation::SetInstrumentRouteDepth {
                        instrument: instrument.clone(),
                        route: route.clone(),
                        depth: depth.clone(),
                    },
                    format!("Set {instrument} route {route} depth = {depth}"),
                );
            }
            Action::ToggleInstrumentRouteBypassById { instrument, route } => {
                let current_bypass = self
                    .bundle
                    .project
                    .as_ref()
                    .and_then(|project| {
                        project
                            .song
                            .instruments
                            .iter()
                            .find(|item| item.id == instrument)
                    })
                    .and_then(|item| item.modulation_routes.iter().find(|item| item.id == route))
                    .is_some_and(|item| item.bypass);
                let bypass = !current_bypass;
                self.apply_operation(
                    ProductionViewOperation::SetInstrumentRouteBypass {
                        instrument: instrument.clone(),
                        route: route.clone(),
                        bypass,
                    },
                    format!("Set {instrument} route {route} bypass = {bypass}"),
                );
            }
            Action::SetAutomationPointValue { lane, point, value } => {
                self.apply_operation(
                    ProductionViewOperation::SetAutomationPointValue {
                        lane: lane.clone(),
                        point: point.clone(),
                        value: value.clone(),
                    },
                    format!("Set {lane}.{point} = {value}"),
                );
            }
            Action::SetAutomationPointTime { lane, point, time } => {
                self.apply_operation(
                    ProductionViewOperation::SetAutomationPointTime {
                        lane: lane.clone(),
                        point: point.clone(),
                        time: time.clone(),
                        snap_to: None,
                    },
                    format!("Move {lane}.{point} to {time}"),
                );
            }
            Action::SetAutomationPointCurve { lane, point, curve } => {
                self.apply_operation(
                    ProductionViewOperation::SetAutomationPointCurve {
                        lane: lane.clone(),
                        point: point.clone(),
                        curve: curve.clone(),
                    },
                    format!("Set {lane}.{point} curve = {curve}"),
                );
            }
            Action::CreateAutomationLane {
                lane,
                target,
                initial_value,
            } => {
                self.apply_operation(
                    ProductionViewOperation::CreateAutomationLane {
                        lane: lane.clone(),
                        target: target.clone(),
                        initial_value: initial_value.clone(),
                    },
                    format!("Create Automation lane {lane}"),
                );
                if let Some(project) = self.bundle.project.as_ref()
                    && let Some(idx) = project.song.automations.iter().position(|l| l.id == lane)
                {
                    self.selected_automation = idx;
                    self.selected_automation_point = 0;
                }
            }
            Action::CreateAutomationPoint {
                lane,
                point,
                at,
                value,
                curve,
            } => {
                self.apply_operation(
                    ProductionViewOperation::CreateAutomationPoint {
                        lane: lane.clone(),
                        point: point.clone(),
                        at: at.clone(),
                        value: value.clone(),
                        curve: curve.clone(),
                    },
                    format!("Add point {point} in {lane}"),
                );
            }
            Action::DeleteAutomationPoint { lane, point } => {
                self.apply_operation(
                    ProductionViewOperation::DeleteAutomationPoint {
                        lane: lane.clone(),
                        point: point.clone(),
                    },
                    format!("Delete point {point} in {lane}"),
                );
            }
            Action::SetEventVelocityValue {
                pattern,
                event,
                velocity,
            } => {
                self.apply_operation(
                    ProductionViewOperation::SetEventVelocity {
                        pattern: pattern.clone(),
                        event,
                        value: PhysicalValue::Number(velocity.clamp(0.01, 1.0)),
                    },
                    format!("Set {pattern}[{event}] velocity"),
                );
            }
            Action::SetEventTimeValue {
                pattern,
                event,
                time,
            } => {
                self.apply_operation(
                    ProductionViewOperation::SetEventTime {
                        pattern: pattern.clone(),
                        event,
                        time: time.clone(),
                        snap_to: None,
                    },
                    format!("Move {pattern}[{event}] to {time}"),
                );
            }
            Action::SetNoteDurationValue {
                pattern,
                event,
                duration,
            } => {
                self.apply_operation(
                    ProductionViewOperation::SetNoteDuration {
                        pattern: pattern.clone(),
                        event,
                        duration: duration.clone(),
                        snap_to: None,
                    },
                    format!("Resize {pattern}[{event}] to {duration}"),
                );
            }
            Action::SetNotePitchValue {
                pattern,
                event,
                pitch,
            } => {
                self.apply_operation(
                    ProductionViewOperation::SetNotePitch {
                        pattern: pattern.clone(),
                        event,
                        pitch: pitch.clone(),
                    },
                    format!("Set {pattern}[{event}] pitch to {pitch}"),
                );
            }
            Action::SetHitVoiceValue {
                pattern,
                event,
                voice,
            } => {
                self.apply_operation(
                    ProductionViewOperation::SetHitVoice {
                        pattern: pattern.clone(),
                        event,
                        voice: voice.clone(),
                    },
                    format!("Set {pattern}[{event}] voice to {voice}"),
                );
            }
            Action::CreatePatternNote {
                pattern,
                at,
                pitch,
                duration,
                velocity,
            } => {
                self.apply_operation(
                    ProductionViewOperation::CreatePatternNote {
                        pattern: pattern.clone(),
                        at: at.clone(),
                        pitch: pitch.clone(),
                        duration: duration.clone(),
                        velocity,
                    },
                    format!("Add note {pitch} in {pattern}"),
                );
            }
            Action::CreatePatternHit {
                pattern,
                at,
                voice,
                velocity,
            } => {
                self.apply_operation(
                    ProductionViewOperation::CreatePatternHit {
                        pattern: pattern.clone(),
                        at: at.clone(),
                        voice: voice.clone(),
                        velocity,
                    },
                    format!("Add hit {voice} in {pattern}"),
                );
            }
            Action::DeletePatternEvent { pattern, event } => {
                self.apply_operation(
                    ProductionViewOperation::DeletePatternEvent {
                        pattern: pattern.clone(),
                        event,
                    },
                    format!("Delete event {event} in {pattern}"),
                );
            }
            Action::CreateClip {
                track,
                clip,
                pattern,
                start,
                duration,
            } => {
                self.apply_operation(
                    ProductionViewOperation::CreateClip {
                        track: track.clone(),
                        clip: clip.clone(),
                        pattern: pattern.clone(),
                        start: start.clone(),
                        duration: duration.clone(),
                    },
                    format!("Add Clip {clip} on {track}"),
                );
            }
            Action::MoveClip { clip, start } => {
                self.apply_operation(
                    ProductionViewOperation::MoveClip {
                        clip: clip.clone(),
                        start: start.clone(),
                    },
                    format!("Move Clip {clip} to {start}"),
                );
            }
            Action::ResizeClip { clip, duration } => {
                self.apply_operation(
                    ProductionViewOperation::ResizeClip {
                        clip: clip.clone(),
                        duration: duration.clone(),
                    },
                    format!("Resize Clip {clip} to {duration}"),
                );
            }
            Action::DuplicateClip {
                clip,
                new_id,
                start,
            } => {
                self.apply_operation(
                    ProductionViewOperation::DuplicateClip {
                        clip: clip.clone(),
                        new_id: new_id.clone(),
                        start: start.clone(),
                    },
                    format!("Duplicate Clip {clip} as {new_id}"),
                );
            }
            Action::SplitClip { clip, new_id, at } => {
                self.apply_operation(
                    ProductionViewOperation::SplitClip {
                        clip: clip.clone(),
                        new_id: new_id.clone(),
                        at: at.clone(),
                    },
                    format!("Split Clip {clip} at {at}"),
                );
            }
            Action::DeleteClip { clip } => {
                self.apply_operation(
                    ProductionViewOperation::DeleteClip { clip: clip.clone() },
                    format!("Delete Clip {clip}"),
                );
            }
            Action::SetSongTempo { bpm } => {
                self.apply_operation(
                    ProductionViewOperation::SetSongTempo { bpm },
                    format!("Set tempo {bpm:.1} BPM"),
                );
            }
            Action::UndoHistory => self.undo_previous_history(),
            Action::SelectMixerParameter(index) => {
                if !self.mixer_parameters.is_empty() {
                    self.selected_mixer_parameter = index.min(self.mixer_parameters.len() - 1);
                }
            }
            Action::SelectProcessor(index) => {
                if let Some(count) = self.processing_graph().map(|g| g.processors.len())
                    && count > 0
                {
                    self.selected_processor = index.min(count - 1);
                }
            }
            Action::SelectTrack(index) => {
                if let Some(count) = self.bundle.project.as_ref().map(|p| p.song.tracks.len())
                    && count > 0
                {
                    self.selected_track = index.min(count - 1);
                }
            }
            Action::SelectInstrument(index) => {
                if let Some(count) = self
                    .bundle
                    .project
                    .as_ref()
                    .map(|p| p.song.instruments.len())
                    && count > 0
                {
                    self.selected_instrument = index.min(count - 1);
                    self.selected_parameter = 0;
                    self.selected_instrument_section = 0;
                    self.selected_instrument_row = 0;
                    self.selected_graph_node = 0;
                }
            }
            Action::SelectPattern(index) => {
                if let Some(count) = self.bundle.project.as_ref().map(|p| p.song.patterns.len())
                    && count > 0
                {
                    self.selected_pattern = index.min(count - 1);
                    self.selected_event = 0;
                }
            }
            Action::SelectAutomation(index) => {
                if let Some(count) = self
                    .bundle
                    .project
                    .as_ref()
                    .map(|p| p.song.automations.len())
                    && count > 0
                {
                    self.selected_automation = index.min(count - 1);
                    self.selected_automation_point = 0;
                }
            }
            Action::SelectAutomationPoint(index) => {
                if let Some(count) = self
                    .bundle
                    .project
                    .as_ref()
                    .and_then(|p| p.song.automations.get(self.selected_automation))
                    .map(|lane| lane.points.len())
                    && count > 0
                {
                    self.selected_automation_point = index.min(count - 1);
                }
            }
            Action::SelectEvent(index) => {
                if let Some(count) = self
                    .bundle
                    .project
                    .as_ref()
                    .and_then(|p| p.song.patterns.get(self.selected_pattern))
                    .map(|pattern| pattern.events.len())
                    && count > 0
                {
                    self.selected_event = index.min(count - 1);
                }
            }
            Action::SelectGraphNode(index) => {
                self.selected_graph_node = index;
            }

            Action::ToggleMixerMute => self.toggle_mixer_mute(),
            Action::ToggleMixerSolo => self.toggle_mixer_solo(),
            Action::ToggleMixerCue(tap) => self.toggle_mixer_cue(tap),
            Action::ToggleTransientProcessorBypass => self.toggle_transient_processor_bypass(),
            Action::ToggleDurableProcessorBypass => self.toggle_durable_processor_bypass(),
            Action::MoveSelectedProcessor(direction) => self.move_selected_processor(direction),
            Action::ToggleInstrumentRouteBypass => self.toggle_instrument_route_bypass(),
            Action::OpenTrackPan => self.open_track_pan(),

            Action::OpenEventTime => self.open_event_time(),
            Action::OpenEventVelocity => self.open_event_velocity(),
            Action::OpenNoteDuration => self.open_note_duration(),
            Action::OpenSampleRegion => self.open_sample_region(),
            Action::OpenSampleFade => self.open_sample_fade(),
            Action::OpenSampleGain => self.open_sample_gain(),
            Action::OpenSamplePan => self.open_sample_pan(),
            Action::OpenAutomationTime => self.open_automation_time(),
            Action::OpenAutomationCurve => self.open_automation_curve(),

            Action::ModalSubmit => self.submit_modal(),
            Action::ModalDismiss => self.modal = None,
            Action::ModalBackspace => {
                if let Some(Modal::Input {
                    value,
                    replace_on_type,
                    ..
                }) = &mut self.modal
                {
                    if *replace_on_type {
                        value.clear();
                        *replace_on_type = false;
                    } else {
                        value.pop();
                    }
                }
            }
            Action::ModalSelectAll => {
                if let Some(Modal::Input {
                    replace_on_type, ..
                }) = &mut self.modal
                {
                    *replace_on_type = true;
                }
            }
            Action::ModalInsert(character) => {
                if let Some(Modal::Input {
                    value,
                    replace_on_type,
                    ..
                }) = &mut self.modal
                {
                    if *replace_on_type {
                        value.clear();
                        *replace_on_type = false;
                    }
                    value.push(character);
                }
            }
            Action::ModalSetInput(input) => {
                if let Some(Modal::Input {
                    value,
                    replace_on_type,
                    ..
                }) = &mut self.modal
                {
                    *value = input;
                    *replace_on_type = false;
                }
            }
            Action::ModalScroll(delta) => {
                if let Some(Modal::Text { scroll, .. }) = &mut self.modal {
                    *scroll = scroll.saturating_add_signed(delta);
                }
            }

            Action::Timeline(action) => self.dispatch_timeline(action),
        }
    }

    fn dispatch_timeline(&mut self, action: TimelineAction) {
        match action {
            TimelineAction::MoveGrid(direction) => self.move_timeline_grid(direction),
            TimelineAction::MoveBoundary(direction) => self.move_timeline_boundary(direction),
            TimelineAction::MoveClipEnd => self.move_timeline_clip_end(),
            TimelineAction::ToggleClipEdge => self.toggle_timeline_clip_edge(),
            TimelineAction::MoveTrackEdge { end } => self.move_timeline_track_edge(end),
            TimelineAction::MoveSongEdge { end } => self.move_timeline_song_edge(end),
            TimelineAction::MoveMatchingPattern(direction) => {
                self.move_timeline_matching_pattern(direction)
            }
            TimelineAction::MoveLocator(direction) => self.move_timeline_locator(direction),
            TimelineAction::JumpToLocator(index) => self.jump_timeline_locator(index),
            TimelineAction::JumpHistory(direction) => self.jump_timeline_history(direction),
            TimelineAction::SetMark(mark) => self.set_timeline_mark(mark),
            TimelineAction::JumpMark(mark) => self.jump_timeline_mark(mark),
            TimelineAction::PlaceCursor {
                track,
                column,
                columns,
            } => self.place_timeline_cursor(track, column, columns),

            TimelineAction::ChangeGrid(direction) => self.change_timeline_grid(direction),
            TimelineAction::Zoom(direction) => self.zoom_timeline(direction),
            TimelineAction::Pan(direction) => self.pan_timeline(direction),
            TimelineAction::PanWindow(direction) => self.pan_timeline_window(direction),
            TimelineAction::Fit => self.fit_timeline(),
            TimelineAction::FitRange => self.fit_timeline_range(),
            TimelineAction::CenterCursor => self.center_timeline_cursor(),
            TimelineAction::AlignCursor { end } => self.align_timeline_cursor(end),
            TimelineAction::ToggleFollow => self.toggle_timeline_follow(),

            TimelineAction::BeginRange => self.begin_timeline_visual(),
            TimelineAction::SwapRangeEdge => {
                if self.timeline_state.swap_visual_edge() {
                    self.reveal_timeline_cursor();
                    self.status = "Timeline RANGE active edge swapped".to_owned();
                }
            }
            TimelineAction::RestoreRange => {
                if self.timeline_state.restore_visual() {
                    self.reveal_timeline_cursor();
                    self.status = "Restored the last Timeline range selection".to_owned();
                } else {
                    self.status = "No prior Timeline range selection".to_owned();
                }
            }
            TimelineAction::ApplyRange { looping } => self.apply_timeline_visual(looping),

            TimelineAction::OpenGoto => self.open_timeline_goto(),
            TimelineAction::OpenSearch => self.open_timeline_search(),
            TimelineAction::RepeatSearch(direction) => self.repeat_timeline_search(direction),
            TimelineAction::OpenLocator => self.open_timeline_locator(),

            TimelineAction::GoDefinition => self.timeline_go_definition(),
            TimelineAction::GoSource => self.timeline_go_source(),
            TimelineAction::GoInstrument => self.timeline_go_instrument(),
            TimelineAction::GoMixer => self.timeline_go_mixer(),
            TimelineAction::GoProcessing => self.timeline_go_processing(),
        }
    }

    fn place_timeline_cursor(&mut self, track: usize, column: usize, columns: usize) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let window = self.timeline_viewport.window(project.song.end);
        let grid = self.timeline_state.grid(project);
        let Some(at) = timeline::column_time_span(window, column, columns)
            .and_then(|(left, _)| left.snap_to_grid(grid).ok())
        else {
            return;
        };
        self.set_timeline_position(
            TimelinePosition { at, track },
            false,
            true,
            "Timeline cursor placed",
        );
    }

    fn reveal_timeline_cursor(&mut self) {
        if let Some(song_end) = self.bundle.project.as_ref().map(|project| project.song.end) {
            self.timeline_viewport
                .reveal(self.timeline_position().at, song_end);
        }
    }

    fn submit_modal(&mut self) {
        match self.modal.take() {
            Some(Modal::Input { value, action, .. }) => self.commit_input(value, action),
            Some(Modal::ConfirmRestore { entry_id }) => self.restore_history(&entry_id),
            Some(Modal::Text { .. }) | None => {}
        }
    }

    fn audition_note_on_velocity(&mut self, key: char, pitch: u8, velocity: f32) {
        if self.held_notes.contains_key(&key) {
            return;
        }
        let track_index = self.audition_track();
        let note_id = self.next_note_id;
        self.next_note_id = self.next_note_id.saturating_add(1);
        let velocity = velocity.clamp(0.05, 1.0);
        if let Some(player) = self.player.as_mut() {
            if player
                .note_on(track_index, note_id, pitch, velocity)
                .is_ok()
            {
                self.held_notes.insert(
                    key,
                    HeldNote {
                        track_index,
                        note_id,
                        pressed_at: Instant::now(),
                    },
                );
            }
        } else {
            self.status = "Start playback with Space before Note Audition".to_owned();
        }
    }

    fn toggle_channel_mute_by_id(&mut self, identity: &str) {
        let target_str = match identity.split_once('/') {
            Some(("track", id)) => format!("track:{id}.mute"),
            Some(("group", id)) => format!("group:{id}.mute"),
            Some(("return", id)) => format!("return:{id}.mute"),
            _ => {
                self.status = format!("Channel `{identity}` has no source-backed Mute");
                return;
            }
        };
        let Ok(target) = target_str.parse::<ParameterTarget>() else {
            self.status = format!("Invalid mute target `{target_str}`");
            return;
        };
        let Some(spec) = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| project.parameter_spec(&target))
        else {
            self.status = format!("Mute target `{target}` is unavailable");
            return;
        };
        let value = if spec.base >= 0.5 { 0.0 } else { 1.0 };
        self.apply_operation(
            ProductionViewOperation::SetParameterTarget {
                target: target.to_string(),
                value: PhysicalValue::Number(value),
            },
            format!("Set {target} to {}", Unit::Boolean.format_value(value)),
        );
    }

    fn toggle_channel_solo_by_id(&mut self, identity: &str) {
        if !self.runtime_matches_candidate() {
            self.status = if self.player.is_some() {
                "Wait for the Candidate to become audible before changing transient Solo"
            } else {
                "Publish the Candidate before changing transient Solo"
            }
            .to_owned();
            return;
        }
        if identity == "master" {
            self.status = "Master Solo is undefined; Solo a Track, Group, or Return".to_owned();
            return;
        }
        let enabled = !self.audition_plan.soloed_channels.contains(identity);
        if let Some(player) = self.player.as_mut()
            && let Err(error) = player.set_channel_solo(identity, enabled)
        {
            self.status = format!("Solo command delayed: {error}");
            return;
        }
        if enabled {
            self.audition_plan
                .soloed_channels
                .insert(identity.to_owned());
        } else {
            self.audition_plan.soloed_channels.remove(identity);
        }
        self.audition_intent_changed();
        self.status = format!(
            "Solo {} for `{identity}`",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    fn toggle_channel_cue_by_id(&mut self, identity: &str, tap: TapPoint) {
        if !self.runtime_matches_candidate() {
            self.status = if self.player.is_some() {
                "Wait for the Candidate to become audible before changing transient cue"
            } else {
                "Publish the Candidate before changing transient cue"
            }
            .to_owned();
            return;
        }
        let next = if self
            .audition_plan
            .cue
            .as_ref()
            .is_some_and(|current| current.channel == identity && current.tap == tap)
        {
            None
        } else {
            Some(AuditionCue {
                channel: identity.to_owned(),
                tap,
            })
        };
        if let Some(player) = self.player.as_mut()
            && let Err(error) =
                player.set_channel_cue(next.as_ref().map(|cue| cue.channel.as_str()), tap)
        {
            self.status = format!("Cue command delayed: {error}");
            return;
        }
        self.audition_plan.cue = next;
        self.audition_intent_changed();
        self.status = self.audition_plan.cue.as_ref().map_or_else(
            || "Cue cleared".to_owned(),
            |cue| format!("Cueing `{}`", cue.channel),
        );
    }

    fn clear_all_solos(&mut self) {
        if self.audition_plan.soloed_channels.is_empty() {
            return;
        }
        let channels = self
            .audition_plan
            .soloed_channels
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        if let Some(player) = self.player.as_mut() {
            for channel in &channels {
                let _ = player.set_channel_solo(channel, false);
            }
        }
        self.audition_plan.soloed_channels.clear();
        self.audition_intent_changed();
        self.status = "All channel solos cleared".to_owned();
    }

    fn toggle_processor_transient_bypass_by_id(&mut self, processor: &str) {
        if !self.runtime_matches_candidate() {
            self.status = if self.player.is_some() {
                "Wait for the Candidate to become audible before changing transient bypass"
            } else {
                "Publish the Candidate before changing transient bypass"
            }
            .to_owned();
            return;
        }
        let Some(authored) = self
            .processing_graph()
            .and_then(|graph| graph.processors.iter().find(|item| item.id == processor))
            .map(|item| item.authored_bypass)
        else {
            self.status = format!("Processor `{processor}` not found");
            return;
        };
        let next =
            (!self.audition_plan.transient_bypass.contains_key(processor)).then_some(!authored);
        if let Some(player) = self.player.as_mut()
            && let Err(error) = player.set_processor_bypass(processor, next)
        {
            self.status = format!("Transient bypass command delayed: {error}");
            return;
        }
        if let Some(bypass) = next {
            self.audition_plan
                .transient_bypass
                .insert(processor.to_owned(), bypass);
            self.status = format!("Processor `{processor}` A/B bypass = {bypass}");
        } else {
            self.audition_plan.transient_bypass.remove(processor);
            self.status = format!("Processor `{processor}` restored to authored bypass");
        }
        self.audition_intent_changed();
    }

    fn toggle_processor_durable_bypass_by_id(&mut self, processor: &str) {
        let Some(authored) = self
            .processing_graph()
            .and_then(|graph| graph.processors.iter().find(|item| item.id == processor))
            .map(|item| item.authored_bypass)
        else {
            self.status = format!("Processor `{processor}` not found");
            return;
        };
        let bypass = !authored;
        self.apply_operation(
            ProductionViewOperation::SetProcessorBypass {
                processor: processor.to_owned(),
                bypass,
            },
            format!("Set Processor {processor} bypass to {bypass}"),
        );
    }

    fn undo_previous_history(&mut self) {
        if self.history.len() < 2 {
            self.status = "Nothing earlier in Project History to undo".to_owned();
            return;
        }
        // History is newest first, so the entry before the latest is the second one.
        let entry_id = self.history[1].id.clone();
        self.restore_history(&entry_id);
    }
}
