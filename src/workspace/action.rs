//! Client-independent workspace actions and the session entry points that apply them.
//!
//! Every interactive change to a [`Workspace`] goes through [`Workspace::dispatch`]. Client
//! adapters own the mapping from their input (keys, pointer, menus) to [`Action`]; this module
//! depends on no terminal or windowing library.

use std::{path::Path, time::Instant};

use super::{HeldNote, Modal, View, Workspace, WorkspaceError};
use crate::{model::TapPoint, production_view::MoveDirection};

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
    /// Release the audition note started with the same key identity.
    NoteOff {
        /// Adapter-chosen identity passed to [`Action::NoteOn`].
        key: char,
    },

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
    /// Step back or forward through the cursor jump history.
    JumpHistory(isize),
    /// Remember the cursor under a named mark.
    SetMark(char),
    /// Return the cursor to a named mark.
    JumpMark(char),

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
            Action::NoteOn { key, pitch } => self.audition_note_on(key, pitch),
            Action::NoteOff { key } => self.release_audition(key),

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
            TimelineAction::JumpHistory(direction) => self.jump_timeline_history(direction),
            TimelineAction::SetMark(mark) => self.set_timeline_mark(mark),
            TimelineAction::JumpMark(mark) => self.jump_timeline_mark(mark),

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

    fn audition_note_on(&mut self, key: char, pitch: u8) {
        if self.held_notes.contains_key(&key) {
            return;
        }
        let track_index = self.audition_track();
        let note_id = self.next_note_id;
        self.next_note_id = self.next_note_id.saturating_add(1);
        if let Some(player) = self.player.as_mut() {
            if player.note_on(track_index, note_id, pitch, 0.8).is_ok() {
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
}
