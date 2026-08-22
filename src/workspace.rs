//! Ratatui production workspace over the terminal-independent Project model.

use std::{
    collections::BTreeMap,
    io::{self, Stdout},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Table, TableState, Tabs, Wrap,
    },
};

use crate::{
    diagnostic::Severity,
    edit::{
        ProcessorMove, apply_structured_edit, fade_sample_occurrence, move_processor,
        set_automation_point_at, set_automation_point_interpolation, set_automation_point_value,
        set_event_at, set_event_velocity, set_hit_voice, set_note_duration, set_note_pitch,
        set_parameter, set_parameter_target, set_processor_setting, set_sample_occurrence_gain,
        set_sample_occurrence_pan, set_track_level, set_track_pan, trim_sample_occurrence,
    },
    engine::audition::{
        AuditionCue, AuditionPath, AuditionPlan, AuditionPrepareOptions, AuditionRange,
        AuditionStatus, AuditionTarget,
    },
    engine::{CompiledProject, mix_graph::MixGraphInspection},
    history::{ChangeOrigin, HistoryEntry, HistoryStore, Publication, write_atomic},
    model::{
        AutomationInterpolation, AutomationLane, AutomationPoint, DrumVoice, InstrumentKind,
        ParameterSpec, ParameterTarget, PatternEvent, Project, TapPoint, Unit, parse_pitch,
    },
    project::{ProjectBundle, RevisionId, load_project},
    realtime::{PipeWirePlayer, PlayerTelemetry, RealtimeError, StreamStatus},
};

const TICK_RATE: Duration = Duration::from_millis(40);
const REFRESH_RATE: Duration = Duration::from_millis(400);
const AUDITION_TIMEOUT: Duration = Duration::from_millis(700);
const VIEWS: [View; 12] = [
    View::Timeline,
    View::Mixer,
    View::Processing,
    View::EventGrid,
    View::Faceplate,
    View::Parameters,
    View::Automation,
    View::Graph,
    View::Source,
    View::Diagnostics,
    View::History,
    View::Help,
];

/// Runs the first-party terminal workspace for one Project.
pub fn run(path: impl AsRef<Path>) -> Result<(), WorkspaceError> {
    let mut app = Workspace::load(path.as_ref())?;
    let mut terminal = TerminalSession::enter()?;
    let result = run_loop(&mut terminal.terminal, &mut app);
    if let Some(player) = app.player.take() {
        let _ = player.shutdown();
    }
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut Workspace,
) -> Result<(), WorkspaceError> {
    let mut last_refresh = Instant::now();
    while !app.quit {
        app.poll_audio();
        app.release_stale_auditions();
        if last_refresh.elapsed() >= REFRESH_RATE {
            app.refresh();
            last_refresh = Instant::now();
        }
        terminal.draw(|frame| render(frame, app))?;
        if event::poll(TICK_RATE)? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => app.move_selection(1),
                    MouseEventKind::ScrollUp => app.move_selection(-1),
                    _ => {}
                },
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
            }
        }
        if app.editor_requested {
            app.editor_requested = false;
            launch_source_editor(terminal, app)?;
            last_refresh = Instant::now();
        }
    }
    Ok(())
}

fn launch_source_editor(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut Workspace,
) -> Result<(), WorkspaceError> {
    let Some(relative_path) = app.bundle.sources.keys().nth(app.selected_source).cloned() else {
        app.status = "No Project source file is selected".to_owned();
        return Ok(());
    };
    if let Some(player) = app.player.as_mut() {
        let _ = player.all_notes_off();
    }
    app.held_notes.clear();

    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    disable_raw_mode()?;
    let editor = std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| "vi".into());
    let source_path = app.root.join(&relative_path);
    if !source_path.exists()
        && let Some(source) = app.bundle.sources.get(&relative_path)
    {
        // Personal-library imports are snapshotted under a safe project-local
        // display path. Editing one materializes that exact source locally so
        // the shared library is never modified as a side effect.
        write_atomic(&source_path, source.as_bytes())?;
    }
    let result = Command::new(editor).arg(&source_path).status();

    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    let area = terminal.size()?.into();
    terminal.resize(area)?;
    app.status = match result {
        Ok(status) if status.success() => {
            format!("Editor closed · reloaded {}", relative_path.display())
        }
        Ok(status) => format!("Editor exited with {status} · reloaded Candidate"),
        Err(error) => format!("Could not launch $VISUAL/$EDITOR: {error}"),
    };
    app.refresh();
    Ok(())
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn enter() -> Result<Self, io::Error> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let mut stdout = io::stdout();
                let _ = execute!(stdout, DisableMouseCapture, LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Timeline,
    Mixer,
    Processing,
    EventGrid,
    Faceplate,
    Parameters,
    Automation,
    Graph,
    Source,
    Diagnostics,
    History,
    Help,
}

impl View {
    const fn title(self) -> &'static str {
        match self {
            Self::Timeline => "Timeline",
            Self::Mixer => "Mixer",
            Self::Processing => "Processing",
            Self::EventGrid => "Event Grid",
            Self::Faceplate => "Faceplate",
            Self::Parameters => "Parameters",
            Self::Automation => "Automation",
            Self::Graph => "Graph",
            Self::Source => "Source",
            Self::Diagnostics => "Diagnostics",
            Self::History => "History",
            Self::Help => "Help",
        }
    }
}

struct Workspace {
    root: PathBuf,
    bundle: ProjectBundle,
    active_revision: Option<RevisionId>,
    active_entry: Option<String>,
    active_project: Option<Project>,
    history: Vec<HistoryEntry>,
    view: View,
    selected_track: usize,
    selected_mixer_parameter: usize,
    selected_processor: usize,
    selected_pattern: usize,
    selected_event: usize,
    selected_instrument: usize,
    selected_parameter: usize,
    selected_automation: usize,
    selected_automation_point: usize,
    selected_graph_node: usize,
    selected_history: usize,
    selected_source: usize,
    source_scroll: usize,
    modal: Option<Modal>,
    status: String,
    quit: bool,
    player: Option<PipeWirePlayer>,
    playback_paused: bool,
    pending_audio_revision: Option<RevisionId>,
    telemetry: PlayerTelemetry,
    mix_channel_identities: Vec<String>,
    processing_graph: Option<MixGraphInspection>,
    audition_plan: AuditionPlan,
    audition_status: Option<AuditionStatus>,
    held_notes: BTreeMap<char, HeldNote>,
    next_note_id: u64,
    audition_loop: Option<String>,
    editor_requested: bool,
}

struct HeldNote {
    track_index: usize,
    note_id: u64,
    pressed_at: Instant,
}

enum Modal {
    Input {
        title: String,
        prompt: String,
        value: String,
        replace_on_type: bool,
        action: InputAction,
    },
    ConfirmRestore {
        entry_id: String,
    },
    Text {
        title: String,
        body: String,
        scroll: u16,
    },
}

enum InputAction {
    Target {
        target: ParameterTarget,
        unit: Unit,
    },
    Parameter {
        instrument: String,
        parameter: String,
        unit: Unit,
    },
    TrackLevel {
        track: String,
    },
    TrackPan {
        track: String,
    },
    ProcessorSetting {
        processor: String,
    },
    AuditionTarget,
    AuditionRange,
    AutomationValue {
        lane: String,
        point: String,
    },
    AutomationTime {
        lane: String,
        point: String,
    },
    AutomationCurve {
        lane: String,
        point: String,
    },
    NotePitch {
        pattern: String,
        event: usize,
    },
    EventTime {
        pattern: String,
        event: usize,
    },
    NoteDuration {
        pattern: String,
        event: usize,
    },
    EventVelocity {
        pattern: String,
        event: usize,
    },
    HitVoice {
        pattern: String,
        event: usize,
    },
    SampleRegion {
        pattern: String,
        id: String,
    },
    SampleFade {
        pattern: String,
        id: String,
    },
    SampleGain {
        pattern: String,
        id: String,
    },
    SamplePan {
        pattern: String,
        id: String,
    },
    Publish,
}

impl Workspace {
    fn load(path: &Path) -> Result<Self, WorkspaceError> {
        let bundle = load_project(path)?;
        let root = bundle.root.clone();
        let (active_entry, active_revision, history, active_project) = history_state(&root)?;
        let processing_graph = candidate_processing_graph(&bundle);
        let mut workspace = Self {
            root,
            bundle,
            active_revision,
            active_entry,
            active_project,
            history,
            view: View::Timeline,
            selected_track: 0,
            selected_mixer_parameter: 0,
            selected_processor: 0,
            selected_pattern: 0,
            selected_event: 0,
            selected_instrument: 0,
            selected_parameter: 0,
            selected_automation: 0,
            selected_automation_point: 0,
            selected_graph_node: 0,
            selected_history: 0,
            selected_source: 0,
            source_scroll: 0,
            modal: None,
            status: "Ready · ? opens keyboard help".to_owned(),
            quit: false,
            player: None,
            playback_paused: false,
            pending_audio_revision: None,
            telemetry: PlayerTelemetry::default(),
            mix_channel_identities: Vec::new(),
            processing_graph,
            audition_plan: AuditionPlan::default(),
            audition_status: None,
            held_notes: BTreeMap::new(),
            next_note_id: 1 << 32,
            audition_loop: None,
            editor_requested: false,
        };
        workspace.clamp_selections();
        Ok(workspace)
    }

    fn refresh(&mut self) {
        let candidate = match load_project(&self.root) {
            Ok(candidate) => candidate,
            Err(error) => {
                self.status = format!("Reload failed: {error}");
                return;
            }
        };
        let state = match history_state(&self.root) {
            Ok(state) => state,
            Err(error) => {
                self.status = format!("History refresh failed: {error}");
                return;
            }
        };
        let active_changed = state.1 != self.active_revision;
        let runtime_publication_attempted = active_changed && self.player.is_some();
        if active_changed {
            self.publish_runtime_for_active(state.1.as_ref(), false);
        }
        let candidate_changed = candidate.revision != self.bundle.revision;
        self.bundle = candidate;
        self.active_entry = state.0;
        self.active_revision = state.1;
        self.history = state.2;
        self.active_project = state.3;
        if self.player.is_none() {
            self.processing_graph = candidate_processing_graph(&self.bundle);
        }
        self.clamp_selections();
        if candidate_changed {
            if self.bundle.diagnostics.has_errors() {
                self.status =
                    "External Candidate is invalid; prior Active Revision remains audible"
                        .to_owned();
                self.view = View::Diagnostics;
            } else if self.candidate_is_active() {
                if !runtime_publication_attempted {
                    self.status = "Published revision loaded".to_owned();
                }
            } else {
                self.status = "Valid external Candidate loaded · press P to publish".to_owned();
            }
        }
    }

    fn publish_runtime_for_active(&mut self, revision: Option<&RevisionId>, explicit_seek: bool) {
        let Some(revision) = revision else {
            return;
        };
        let reservation = match self.player.as_mut() {
            Some(player) => match player.reserve_publication() {
                Ok(reservation) => reservation,
                Err(RealtimeError::QueueFull) => {
                    self.pending_audio_revision = Some(revision.clone());
                    self.status =
                        "Runtime publication delayed; the newest Active Revision will retry"
                            .to_owned();
                    return;
                }
                Err(error) => {
                    self.pending_audio_revision = None;
                    self.status = format!("Runtime publication failed: {error}");
                    return;
                }
            },
            None => return,
        };
        let active = match HistoryStore::open(&self.root).and_then(|store| store.active_bundle()) {
            Ok(Some(active)) => active,
            Ok(None) => return,
            Err(error) => {
                self.status = format!("Active Revision reload failed: {error}");
                return;
            }
        };
        if &active.revision != revision {
            self.pending_audio_revision = Some(active.revision.clone());
            self.status = "Active Revision changed during reload; retrying".to_owned();
            return;
        }
        let Some(project) = active.publishable() else {
            self.status = "Active Revision snapshot is invalid".to_owned();
            return;
        };
        let previous_range = self
            .audition_status
            .as_ref()
            .map(|status| (status.start_frame, status.end_frame));
        let resume_range_offset = if explicit_seek {
            None
        } else {
            self.audition_status.as_ref().map(|status| {
                let next_frame = self.telemetry.engine.frame.saturating_add(1);
                if status.looping {
                    let length = status.end_frame.saturating_sub(status.start_frame).max(1);
                    next_frame.saturating_sub(status.start_frame) % length
                } else {
                    next_frame
                        .clamp(status.start_frame, status.end_frame.saturating_sub(1))
                        .saturating_sub(status.start_frame)
                }
            })
        };
        let prepared = match self.audition_plan.prepare_with_options(
            &active.revision,
            project,
            &active.assets,
            AuditionPrepareOptions {
                initial_frame: None,
                initial_range_offset: resume_range_offset,
                explicit_seek,
            },
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.status = format!(
                    "Active audition target no longer compiles; prior revision remains audible: {error}"
                );
                return;
            }
        };
        let status = prepared.status().clone();
        let range_moved =
            previous_range.is_some_and(|range| range != (status.start_frame, status.end_frame));
        let processing_graph = prepared.mix_graph_inspection().clone();
        let mix_channel_identities = processing_graph
            .channels
            .iter()
            .map(|channel| channel.identity.clone())
            .collect::<Vec<_>>();
        let runtime = prepared.into_runtime();
        let Some(player) = self.player.as_mut() else {
            self.status = "Playback stopped before publication could be queued".to_owned();
            return;
        };
        match player.publish_reserved(runtime, reservation) {
            Ok(()) => {
                self.pending_audio_revision = None;
                self.mix_channel_identities = mix_channel_identities;
                self.processing_graph = Some(processing_graph);
                self.audition_status = Some(status);
                self.status = if range_moved {
                    "Active audition queued · moved range will map the callback's exact relative offset"
                        .to_owned()
                } else {
                    "Active audition queued · range-relative transport retained; compatible controls ramp"
                        .to_owned()
                };
            }
            Err(RealtimeError::QueueFull) => {
                self.pending_audio_revision = Some(revision.clone());
                self.status =
                    "Runtime publication delayed; the newest Active Revision will retry".to_owned();
            }
            Err(error) => {
                self.pending_audio_revision = None;
                self.status = format!("Runtime publication failed: {error}");
            }
        }
    }

    fn poll_audio(&mut self) {
        let audition_track = self.audition_track();
        let publication_ready = if let Some(player) = self.player.as_mut() {
            self.telemetry = player.poll();
            player.set_audition_track(audition_track);
            if self.telemetry.stream_state == StreamStatus::Error {
                self.status = "PipeWire stream failed; press Space to restart".to_owned();
            } else if self.telemetry.stream_state == StreamStatus::Stopped
                && self.telemetry.callbacks > 0
            {
                self.status =
                    "PipeWire stream stopped or disconnected; press Space to restart".to_owned();
            }
            !self.telemetry.transition_active
        } else {
            false
        };
        if publication_ready && let Some(revision) = self.pending_audio_revision.clone() {
            self.publish_runtime_for_active(Some(&revision), false);
        }
    }

    fn toggle_mixer_solo(&mut self) {
        if !self.candidate_is_active() {
            self.status = "Publish the Candidate before changing transient Solo".to_owned();
            return;
        }
        let Some(identity) = self.selected_mixer_channel_identity() else {
            self.status = "Selected mixer target has no singular channel to Solo".to_owned();
            return;
        };
        if identity == "master" {
            self.status = "Master Solo is undefined; Solo a Track, Group, or Return".to_owned();
            return;
        }
        let enabled = !self.audition_plan.soloed_channels.contains(&identity);
        if let Some(player) = self.player.as_mut()
            && let Err(error) = player.set_channel_solo(&identity, enabled)
        {
            self.status = format!("Solo command delayed: {error}");
            return;
        }
        if enabled {
            self.audition_plan.soloed_channels.insert(identity.clone());
        } else {
            self.audition_plan.soloed_channels.remove(&identity);
        }
        self.status = format!(
            "Solo {} for `{identity}` (transient monitoring only)",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    fn toggle_mixer_cue(&mut self, tap: TapPoint) {
        if !self.candidate_is_active() {
            self.status = "Publish the Candidate before changing transient cue".to_owned();
            return;
        }
        let Some(identity) = self.selected_mixer_channel_identity() else {
            self.status = "Selected mixer target has no singular channel to cue".to_owned();
            return;
        };
        let next = if self
            .audition_plan
            .cue
            .as_ref()
            .is_some_and(|current| current.channel == identity && current.tap == tap)
        {
            None
        } else {
            Some(AuditionCue {
                channel: identity.clone(),
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
        self.status = self.audition_plan.cue.as_ref().map_or_else(
            || "Cue cleared (transient monitoring only)".to_owned(),
            |cue| {
                format!(
                    "Cue {:?} for `{}` (transient monitoring only)",
                    cue.tap, cue.channel
                )
            },
        );
    }

    fn toggle_mixer_mute(&mut self) {
        let Some(target) = self.selected_mixer_mute_target() else {
            self.status = "Selected mixer target has no source-backed channel Mute".to_owned();
            return;
        };
        let Some(spec) = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| project.parameter_spec(&target))
        else {
            self.status = "Selected channel Mute target is unavailable".to_owned();
            return;
        };
        let value = if spec.base >= 0.5 { 0.0 } else { 1.0 };
        self.apply_replacement(
            set_parameter_target(&self.bundle, &target, value),
            format!("Set {target} to {}", Unit::Boolean.format_value(value)),
        );
    }

    fn selected_processor_inspection(
        &self,
    ) -> Option<&crate::engine::mix_graph::CompiledProcessorInspection> {
        self.processing_graph
            .as_ref()?
            .processors
            .get(self.selected_processor)
    }

    fn open_processor_setting(&mut self) {
        let Some(inspection) = self.selected_processor_inspection() else {
            self.status = "Select a processor before editing settings".to_owned();
            return;
        };
        let processor_id = inspection.id.clone();
        let authored = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| project_processor(project, &processor_id))
            .and_then(|processor| processor.settings.first())
            .map(|setting| format!("{} {}", setting.key, setting.value));
        let resolved = inspection.settings.first().map(|setting| {
            let value = setting.choice.clone().unwrap_or_else(|| {
                setting
                    .unit
                    .zip(setting.value)
                    .map_or_else(|| "0".to_owned(), |(unit, value)| unit.format_value(value))
            });
            format!("{} {value}", setting.key)
        });
        let Some(value) = authored.or(resolved) else {
            self.status = format!("Processor `{processor_id}` has no editable settings");
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("Processor Setting · {processor_id}"),
            prompt: "Enter exact `KEY VALUE`; missing supported keys are inserted".to_owned(),
            value,
            replace_on_type: false,
            action: InputAction::ProcessorSetting {
                processor: processor_id,
            },
        });
    }

    fn toggle_transient_processor_bypass(&mut self) {
        if self.player.is_none() && !self.candidate_is_active() {
            self.status =
                "Publish the Candidate before changing transient Active audition state".to_owned();
            return;
        }
        let Some(inspection) = self.selected_processor_inspection() else {
            self.status = "Select a processor before changing audition bypass".to_owned();
            return;
        };
        let processor = inspection.id.clone();
        let authored = inspection.authored_bypass;
        let next =
            (!self.audition_plan.transient_bypass.contains_key(&processor)).then_some(!authored);
        if let Some(player) = self.player.as_mut()
            && let Err(error) = player.set_processor_bypass(&processor, next)
        {
            self.status = format!("Transient bypass command delayed: {error}");
            return;
        }
        if let Some(bypass) = next {
            self.audition_plan
                .transient_bypass
                .insert(processor.clone(), bypass);
            self.status = format!(
                "Processor `{processor}` transient bypass override = {bypass} · source unchanged"
            );
        } else {
            self.audition_plan.transient_bypass.remove(&processor);
            self.status = format!(
                "Processor `{processor}` follows durable bypass = {authored} · source unchanged"
            );
        }
    }

    fn toggle_durable_processor_bypass(&mut self) {
        let Some(inspection) = self.selected_processor_inspection() else {
            self.status = "Select a processor before changing durable bypass".to_owned();
            return;
        };
        let processor = inspection.id.clone();
        let bypass = !inspection.authored_bypass;
        let target = ParameterTarget::Processor {
            processor: processor.clone(),
            parameter: "bypass".to_owned(),
        };
        self.apply_replacement(
            set_parameter_target(&self.bundle, &target, f64::from(bypass)),
            format!("Set Processor {processor} durable bypass to {bypass}"),
        );
    }

    fn move_selected_processor(&mut self, direction: ProcessorMove) {
        let Some(processor) = self
            .selected_processor_inspection()
            .map(|processor| processor.id.clone())
        else {
            self.status = "Select a processor before reordering the chain".to_owned();
            return;
        };
        self.apply_replacement(
            move_processor(&self.bundle, &processor, direction),
            format!("Move Processor {processor} {direction:?}"),
        );
    }

    fn activate_audition_plan(&mut self, plan: AuditionPlan) {
        let active = match HistoryStore::open(&self.root).and_then(|store| store.active_bundle()) {
            Ok(Some(active)) => active,
            Ok(None) => {
                self.status = "Publish a valid Candidate before selecting an audition".to_owned();
                return;
            }
            Err(error) => {
                self.status = format!("Cannot load Active Revision: {error}");
                return;
            }
        };
        let Some(project) = active.publishable() else {
            self.status = "Active Revision snapshot is invalid".to_owned();
            return;
        };
        let prepared = match plan.prepare_with_options(
            &active.revision,
            project,
            &active.assets,
            AuditionPrepareOptions {
                initial_frame: None,
                initial_range_offset: None,
                explicit_seek: true,
            },
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.status = format!("Audition selection rejected: {error}");
                return;
            }
        };
        let status = prepared.status().clone();
        let graph = prepared.mix_graph_inspection().clone();
        self.mix_channel_identities = graph
            .channels
            .iter()
            .map(|channel| channel.identity.clone())
            .collect();
        self.processing_graph = Some(graph);
        self.audition_status = Some(status.clone());
        self.audition_plan = plan;
        self.audition_loop = match (&status.target, status.looping) {
            (AuditionTarget::Pattern { pattern, .. }, true) => Some(pattern.clone()),
            _ => None,
        };
        self.clamp_selections();
        if self.player.is_some() {
            let revision = self.active_revision.clone();
            self.publish_runtime_for_active(revision.as_ref(), true);
        } else {
            self.status = format!(
                "Audition armed: {:?} via {:?} · frames {}..{}{}",
                status.target,
                status.path,
                status.start_frame,
                status.end_frame,
                if status.looping { " · looped" } else { "" }
            );
        }
    }

    fn cycle_audition_path(&mut self) {
        let paths: &[AuditionPath] = match &self.audition_plan.target {
            AuditionTarget::Song => &[AuditionPath::Master],
            AuditionTarget::Track { .. } | AuditionTarget::Clip { .. } => &[
                AuditionPath::Master,
                AuditionPath::Channel {
                    tap: TapPoint::PreFader,
                },
                AuditionPath::Channel {
                    tap: TapPoint::PostFader,
                },
            ],
            AuditionTarget::Pattern { track: Some(_), .. } => &[
                AuditionPath::Master,
                AuditionPath::Channel {
                    tap: TapPoint::PreFader,
                },
                AuditionPath::Channel {
                    tap: TapPoint::PostFader,
                },
            ],
            AuditionTarget::Pattern { track: None, .. } => &[AuditionPath::Master],
            AuditionTarget::Asset { .. } => &[AuditionPath::Raw],
            AuditionTarget::Mapping { track: Some(_), .. }
            | AuditionTarget::Occurrence { track: Some(_), .. } => &[
                AuditionPath::Raw,
                AuditionPath::Mapping,
                AuditionPath::Channel {
                    tap: TapPoint::PreFader,
                },
                AuditionPath::Channel {
                    tap: TapPoint::PostFader,
                },
                AuditionPath::Master,
            ],
            AuditionTarget::Mapping { track: None, .. }
            | AuditionTarget::Occurrence { track: None, .. } => &[
                AuditionPath::Raw,
                AuditionPath::Mapping,
                AuditionPath::Master,
            ],
        };
        let current = paths
            .iter()
            .position(|path| *path == self.audition_plan.path)
            .unwrap_or(0);
        let mut plan = self.audition_plan.clone();
        plan.path = paths[(current + 1) % paths.len()];
        self.activate_audition_plan(plan);
    }

    fn arm_selected_audition(&mut self) {
        if !self.candidate_is_active() {
            self.status = "Publish the selected Candidate before auditioning it".to_owned();
            return;
        }
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let mut plan = self.audition_plan.clone();
        plan.range = AuditionRange::default();
        match self.view {
            View::Timeline => {
                let Some(track) = project.song.tracks.get(self.selected_track) else {
                    self.status = "Select a Track before auditioning".to_owned();
                    return;
                };
                plan.target = AuditionTarget::Track {
                    track: track.id.clone(),
                };
                plan.path = AuditionPath::Channel {
                    tap: TapPoint::PostFader,
                };
            }
            View::EventGrid => {
                let Some(pattern) = project.song.patterns.get(self.selected_pattern) else {
                    self.status = "Select a Pattern before auditioning".to_owned();
                    return;
                };
                if let Some(PatternEvent::Sample { id, mapping, .. }) =
                    pattern.events.get(self.selected_event)
                {
                    let mut matching_tracks = project.song.tracks.iter().filter(|track| {
                        track.clips.iter().any(|clip| clip.pattern == pattern.id)
                            && project.song.instruments.iter().any(|instrument| {
                                instrument.id == track.instrument
                                    && instrument.sampler.as_ref().is_some_and(|sampler| {
                                        sampler
                                            .mappings
                                            .iter()
                                            .any(|candidate| candidate.id == *mapping)
                                    })
                            })
                    });
                    let track = matching_tracks.next().and_then(|first| {
                        matching_tracks.next().is_none().then(|| first.id.clone())
                    });
                    plan.target = AuditionTarget::Occurrence {
                        pattern: pattern.id.clone(),
                        occurrence: id.clone(),
                        track,
                    };
                    plan.path = AuditionPath::Mapping;
                } else {
                    plan.target = AuditionTarget::Pattern {
                        pattern: pattern.id.clone(),
                        track: None,
                    };
                    plan.path = AuditionPath::Master;
                }
            }
            _ => return,
        }
        self.activate_audition_plan(plan);
    }

    fn toggle_playback(&mut self) {
        if let Some(player) = self.player.as_mut() {
            let paused = !self.playback_paused;
            match player.set_paused(paused) {
                Ok(()) => {
                    self.playback_paused = paused;
                    self.status = if paused {
                        "Audition paused · Space resumes; S stops".to_owned()
                    } else {
                        "Audition resumed".to_owned()
                    };
                }
                Err(error) => self.status = format!("Pause command delayed: {error}"),
            }
            return;
        }
        let active = match HistoryStore::open(&self.root).and_then(|store| store.active_bundle()) {
            Ok(Some(active)) => active,
            Ok(None) => {
                self.status = "Publish a valid Candidate before playback".to_owned();
                return;
            }
            Err(error) => {
                self.status = format!("Cannot load Active Revision: {error}");
                return;
            }
        };
        let Some(project) = active.publishable() else {
            self.status = "Active Revision snapshot is invalid".to_owned();
            return;
        };
        let prepared = match self
            .audition_plan
            .prepare(&active.revision, project, &active.assets)
        {
            Ok(prepared) => prepared,
            Err(error) => {
                self.status = format!("Cannot prepare audition: {error}");
                return;
            }
        };
        let status = prepared.status().clone();
        let processing_graph = prepared.mix_graph_inspection().clone();
        let mix_channel_identities = processing_graph
            .channels
            .iter()
            .map(|channel| channel.identity.clone())
            .collect::<Vec<_>>();
        let runtime = prepared.into_runtime();
        match PipeWirePlayer::start(runtime) {
            Ok(mut player) => {
                player.set_audition_track(self.audition_track());
                self.pending_audio_revision = None;
                self.playback_paused = false;
                self.mix_channel_identities = mix_channel_identities;
                self.processing_graph = Some(processing_graph);
                self.audition_status = Some(status.clone());
                self.player = Some(player);
                self.status = format!(
                    "Auditioning {:?} via {:?}{} · Space pauses; S stops",
                    status.target,
                    status.path,
                    if status.looping { " · looped" } else { "" }
                );
            }
            Err(error) => self.status = format!("Playback unavailable: {error}"),
        }
    }

    fn stop_playback(&mut self) {
        let Some(player) = self.player.take() else {
            self.status = "Audition is already stopped".to_owned();
            return;
        };
        let _ = player.shutdown();
        self.playback_paused = false;
        self.pending_audio_revision = None;
        self.telemetry = PlayerTelemetry::default();
        self.held_notes.clear();
        self.status = "Audition stopped · target, range, and bypass state remain armed".to_owned();
    }

    fn toggle_audition_loop(&mut self) {
        if self.audition_loop.is_none() && !self.candidate_is_active() {
            self.status =
                "Publish the selected Candidate before arming its Audition Phrase".to_owned();
            return;
        }
        if self.audition_loop.is_some() {
            self.audition_loop = None;
            self.audition_plan.range.looping = false;
        } else {
            let Some(pattern_id) = self
                .bundle
                .project
                .as_ref()
                .and_then(|project| project.song.patterns.get(self.selected_pattern))
                .map(|pattern| pattern.id.clone())
            else {
                self.status = "Select a Pattern before enabling an Audition Phrase loop".to_owned();
                return;
            };
            self.audition_loop = Some(pattern_id.clone());
            self.audition_plan.target = AuditionTarget::Pattern {
                pattern: pattern_id,
                track: None,
            };
            self.audition_plan.path = AuditionPath::Master;
            self.audition_plan.range = AuditionRange {
                looping: true,
                ..AuditionRange::default()
            };
        }

        if self.player.is_some() {
            let revision = self.active_revision.clone();
            self.publish_runtime_for_active(revision.as_ref(), true);
        } else if let Some(pattern) = self.audition_loop.as_deref() {
            self.status =
                format!("Pattern audition `{pattern}` armed · exact loop starts with Space");
        } else {
            self.status = "Audition loop disabled; target remains armed".to_owned();
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.modal.is_some() {
            self.handle_modal_key(key);
            return;
        }
        if self.handle_audition_key(key) {
            return;
        }
        if key.kind == KeyEventKind::Release {
            return;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.quit = true
            }
            (KeyCode::Char('?'), _) => self.view = View::Help,
            (KeyCode::Char('H'), _) => self.view = View::History,
            (KeyCode::Tab, KeyModifiers::SHIFT) | (KeyCode::BackTab, _) => self.change_view(-1),
            (KeyCode::Tab, _) => self.change_view(1),
            (KeyCode::Char(character @ '1'..='9'), _) => {
                self.view = VIEWS[usize::from(character as u8 - b'1')]
            }
            (KeyCode::Char('0'), _) => self.view = VIEWS[9],
            (KeyCode::Char('r'), _) => self.refresh(),
            (KeyCode::Char('P'), _) => self.open_publish(),
            (KeyCode::Char(' '), _) => self.toggle_playback(),
            (KeyCode::Char('S'), _) => self.stop_playback(),
            (KeyCode::Char('L'), _) if self.view == View::EventGrid => self.toggle_audition_loop(),
            (KeyCode::Char('A'), _) if matches!(self.view, View::Timeline | View::EventGrid) => {
                self.arm_selected_audition()
            }
            (KeyCode::Char('a'), _) if self.view == View::Processing => self.cycle_audition_path(),
            (KeyCode::Char('A'), _) if self.view == View::Processing => self.open_audition_target(),
            (KeyCode::Char('F'), _) if self.view == View::Processing => self.open_audition_range(),
            (KeyCode::Char('L'), _) if self.view == View::Processing => {
                self.toggle_current_audition_loop()
            }
            (KeyCode::Char('b'), _) if self.view == View::Processing => {
                self.toggle_transient_processor_bypass()
            }
            (KeyCode::Char('B'), _) if self.view == View::Processing => {
                self.toggle_durable_processor_bypass()
            }
            (KeyCode::Char('K'), _) if self.view == View::Processing => {
                self.move_selected_processor(ProcessorMove::Earlier)
            }
            (KeyCode::Char('J'), _) if self.view == View::Processing => {
                self.move_selected_processor(ProcessorMove::Later)
            }
            (KeyCode::Char('m'), _) if self.view == View::Mixer => self.toggle_mixer_mute(),
            (KeyCode::Char('s'), _) if self.view == View::Mixer => self.toggle_mixer_solo(),
            (KeyCode::Char('c'), _) if self.view == View::Mixer => {
                self.toggle_mixer_cue(TapPoint::PostFader)
            }
            (KeyCode::Char('C'), _) if self.view == View::Mixer => {
                self.toggle_mixer_cue(TapPoint::PreFader)
            }
            (KeyCode::Char('t'), _) if self.view == View::EventGrid => self.open_event_time(),
            (KeyCode::Char('v'), _) if self.view == View::EventGrid => self.open_event_velocity(),
            (KeyCode::Char('u'), _) if self.view == View::EventGrid => self.open_note_duration(),
            (KeyCode::Char('x'), _) if self.view == View::EventGrid => self.open_sample_region(),
            (KeyCode::Char('f'), _) if self.view == View::EventGrid => self.open_sample_fade(),
            (KeyCode::Char('g'), _) if self.view == View::EventGrid => self.open_sample_gain(),
            (KeyCode::Char('p'), _) if self.view == View::EventGrid => self.open_sample_pan(),
            (KeyCode::Char('t'), _) if self.view == View::Automation => self.open_automation_time(),
            (KeyCode::Char('c'), _) if self.view == View::Automation => {
                self.open_automation_curve()
            }
            (KeyCode::Char('['), _) => self.change_context(-1),
            (KeyCode::Char(']'), _) => self.change_context(1),
            (KeyCode::Up, _) => self.move_selection(-1),
            (KeyCode::Down, _) => self.move_selection(1),
            (KeyCode::PageUp, _) => self.move_selection(-8),
            (KeyCode::PageDown, _) => self.move_selection(8),
            (KeyCode::Left, _) => self.adjust_selected(-1),
            (KeyCode::Right, _) => self.adjust_selected(1),
            (KeyCode::Enter, _) => self.open_exact_edit(),
            (KeyCode::Char('l'), _) if self.view == View::Timeline => self.open_track_level(),
            (KeyCode::Char('p'), _) if self.view == View::Timeline => self.open_track_pan(),
            (KeyCode::Char('R'), _) if self.view == View::History => self.open_restore(),
            (KeyCode::Char('d'), _) if self.view == View::History => self.open_history_diff(),
            (KeyCode::Char('e'), _) if self.view == View::Source => self.editor_requested = true,
            (KeyCode::Home, _) if self.view == View::Source => self.source_scroll = 0,
            _ => {}
        }
    }

    fn handle_modal_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        let Some(mut modal) = self.modal.take() else {
            return;
        };
        match &mut modal {
            Modal::Input {
                value,
                replace_on_type,
                ..
            } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    if let Modal::Input { value, action, .. } = modal {
                        self.commit_input(value, action);
                    }
                }
                KeyCode::Backspace => {
                    if *replace_on_type {
                        value.clear();
                        *replace_on_type = false;
                    } else {
                        value.pop();
                    }
                    self.modal = Some(modal);
                }
                KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    *replace_on_type = true;
                    self.modal = Some(modal);
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if *replace_on_type {
                        value.clear();
                        *replace_on_type = false;
                    }
                    value.push(character);
                    self.modal = Some(modal);
                }
                _ => self.modal = Some(modal),
            },
            Modal::ConfirmRestore { entry_id } => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    let entry_id = entry_id.clone();
                    self.restore_history(&entry_id);
                }
                KeyCode::Esc | KeyCode::Char('n') => {}
                _ => self.modal = Some(modal),
            },
            Modal::Text { scroll, .. } => match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {}
                KeyCode::Up => {
                    *scroll = scroll.saturating_sub(1);
                    self.modal = Some(modal);
                }
                KeyCode::Down => {
                    *scroll = scroll.saturating_add(1);
                    self.modal = Some(modal);
                }
                KeyCode::PageUp => {
                    *scroll = scroll.saturating_sub(10);
                    self.modal = Some(modal);
                }
                KeyCode::PageDown => {
                    *scroll = scroll.saturating_add(10);
                    self.modal = Some(modal);
                }
                _ => self.modal = Some(modal),
            },
        }
    }

    fn change_view(&mut self, delta: isize) {
        let current = VIEWS
            .iter()
            .position(|view| *view == self.view)
            .unwrap_or(0);
        self.view = VIEWS[wrap_index(current, VIEWS.len(), delta)];
        self.clamp_selections();
    }

    fn change_context(&mut self, delta: isize) {
        match self.view {
            View::EventGrid => {
                self.selected_pattern =
                    wrap_selection(self.selected_pattern, self.pattern_count(), delta);
                self.selected_event = 0;
            }
            View::Faceplate | View::Parameters | View::Graph => {
                self.selected_instrument =
                    wrap_selection(self.selected_instrument, self.instrument_count(), delta);
                self.selected_parameter = 0;
                self.selected_graph_node = 0;
            }
            View::Automation => {
                self.selected_automation =
                    wrap_selection(self.selected_automation, self.automation_count(), delta);
                self.selected_automation_point = 0;
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: isize) {
        match self.view {
            View::Timeline => {
                self.selected_track =
                    wrap_selection(self.selected_track, self.track_count(), delta);
                self.sync_instrument_to_track();
            }
            View::Mixer => {
                self.selected_mixer_parameter = wrap_selection(
                    self.selected_mixer_parameter,
                    self.mixer_parameter_count(),
                    delta,
                );
            }
            View::Processing => {
                let count = self
                    .processing_graph
                    .as_ref()
                    .map_or(0, |graph| graph.processors.len());
                self.selected_processor = wrap_selection(self.selected_processor, count, delta);
            }
            View::EventGrid => {
                self.selected_event = wrap_selection(self.selected_event, self.event_count(), delta)
            }
            View::Faceplate => {
                self.selected_parameter =
                    wrap_selection(self.selected_parameter, self.faceplate_count(), delta)
            }
            View::Parameters => {
                self.selected_parameter =
                    wrap_selection(self.selected_parameter, self.parameter_count(), delta)
            }
            View::Automation => {
                self.selected_automation_point = wrap_selection(
                    self.selected_automation_point,
                    self.automation_point_count(),
                    delta,
                )
            }
            View::Graph => {
                self.selected_graph_node =
                    wrap_selection(self.selected_graph_node, self.graph_node_count(), delta)
            }
            View::Source => self.source_scroll = self.source_scroll.saturating_add_signed(delta),
            View::History => {
                self.selected_history =
                    wrap_selection(self.selected_history, self.history.len(), delta)
            }
            View::Diagnostics | View::Help => {}
        }
    }

    fn adjust_selected(&mut self, direction: isize) {
        match self.view {
            View::Timeline => self.step_track_level(direction as f64),
            View::Mixer => self.step_mixer_parameter(direction as f64),
            View::Processing => {}
            View::EventGrid => self.step_note(direction),
            View::Faceplate | View::Parameters => self.step_parameter(direction as f64),
            View::Automation => self.step_automation(direction as f64),
            View::Graph => {
                self.selected_instrument =
                    wrap_selection(self.selected_instrument, self.instrument_count(), direction);
                self.selected_parameter = 0;
                self.selected_graph_node = 0;
            }
            View::Source => {
                self.selected_source =
                    wrap_selection(self.selected_source, self.bundle.sources.len(), direction);
                self.source_scroll = 0;
            }
            View::History | View::Diagnostics | View::Help => {}
        }
    }

    fn open_audition_target(&mut self) {
        self.modal = Some(Modal::Input {
            title: "Audition Target".to_owned(),
            prompt: "song | track/ID | clip/[TRACK/]ID | pattern/ID[/TRACK] | asset/ID[/START/END] | mapping/INSTRUMENT/MAPPING[/TRACK] | occurrence/PATTERN/ID[/TRACK]".to_owned(),
            value: audition_target_spec(&self.audition_plan.target),
            replace_on_type: true,
            action: InputAction::AuditionTarget,
        });
    }

    fn open_audition_range(&mut self) {
        let value = match (&self.audition_plan.range.from, &self.audition_plan.range.to) {
            (Some(from), Some(to)) => format!("{from} {to}"),
            _ => String::new(),
        };
        self.modal = Some(Modal::Input {
            title: "Exact Audition Range".to_owned(),
            prompt:
                "Enter exact `FROM TO` musical times or Locator IDs; blank restores target bounds"
                    .to_owned(),
            value,
            replace_on_type: true,
            action: InputAction::AuditionRange,
        });
    }

    fn toggle_current_audition_loop(&mut self) {
        let mut plan = self.audition_plan.clone();
        plan.range.looping = !plan.range.looping;
        self.activate_audition_plan(plan);
    }

    fn open_exact_edit(&mut self) {
        match self.view {
            View::Timeline => self.open_track_level(),
            View::Mixer => self.open_mixer_parameter(),
            View::Processing => self.open_processor_setting(),
            View::EventGrid => self.open_event_primary(),
            View::Faceplate | View::Parameters => self.open_parameter(),
            View::Automation => self.open_automation_value(),
            View::Graph => self.open_graph_source(),
            View::History => self.open_history_diff(),
            _ => {}
        }
    }

    fn open_graph_source(&mut self) {
        let Some(instrument) = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| project.song.instruments.get(self.selected_instrument))
        else {
            self.status = "Selected Instrument is unavailable".to_owned();
            return;
        };
        let Some(graph) = instrument.graph.as_ref() else {
            self.status = "Selected Instrument has no graph source".to_owned();
            return;
        };
        let index = self.selected_graph_node;
        let (location, description) = if let Some(node) = graph.nodes.get(index) {
            (&node.location, format!("Graph Node `{}`", node.id))
        } else if let Some(connection) = index
            .checked_sub(graph.nodes.len())
            .and_then(|index| graph.connections.get(index))
        {
            (
                &connection.location,
                format!(
                    "Bus `{}.{} -> {}.{}`",
                    connection.from.node,
                    connection.from.port,
                    connection.to.node,
                    connection.to.port
                ),
            )
        } else if let Some(parameter) = index
            .checked_sub(graph.nodes.len() + graph.connections.len())
            .and_then(|index| instrument.parameters.get(index))
        {
            (
                &parameter.location,
                format!("Parameter `{}`", parameter.key),
            )
        } else if let Some(control) = index
            .checked_sub(graph.nodes.len() + graph.connections.len() + instrument.parameters.len())
            .and_then(|index| instrument.faceplate.get(index))
        {
            (
                &control.location,
                format!("Faceplate mapping `{}`", control.key),
            )
        } else {
            self.status = "Selected graph item is unavailable".to_owned();
            return;
        };
        let Some(source_index) = self
            .bundle
            .sources
            .keys()
            .position(|path| path == &location.file)
        else {
            self.status = format!("Source {} is unavailable", location.file.display());
            return;
        };
        self.selected_source = source_index;
        self.source_scroll = location.line.saturating_sub(1);
        self.view = View::Source;
        self.status = format!(
            "{description} at {}:{}",
            location.file.display(),
            location.line
        );
    }

    fn open_parameter(&mut self) {
        let Some((instrument, parameter)) = self.selected_parameter_pair() else {
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · {}", instrument.name, parameter.name),
            prompt: format!(
                "Exact {} value ({} … {})",
                parameter.unit, parameter.minimum, parameter.maximum
            ),
            value: parameter.unit.format_value(parameter.value),
            replace_on_type: true,
            action: InputAction::Parameter {
                instrument: instrument.id.clone(),
                parameter: parameter.key.clone(),
                unit: parameter.unit,
            },
        });
    }

    fn open_mixer_parameter(&mut self) {
        let Some((target, spec)) = self.selected_mixer_parameter() else {
            self.status = "No continuous Mixer Parameter is selected".to_owned();
            return;
        };
        self.modal = Some(Modal::Input {
            title: target.to_string(),
            prompt: format!(
                "Exact {} value ({} … {})",
                spec.unit,
                spec.unit.format_value(spec.minimum),
                spec.unit.format_value(spec.maximum)
            ),
            value: spec.unit.format_value(spec.base),
            replace_on_type: true,
            action: InputAction::Target {
                target,
                unit: spec.unit,
            },
        });
    }

    fn open_track_level(&mut self) {
        let Some(track) = self.selected_track_ref() else {
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · Level", track.name),
            prompt: "Exact level (-96db … 12db)".to_owned(),
            value: Unit::Decibels.format_value(track.channel.level_db),
            replace_on_type: true,
            action: InputAction::TrackLevel {
                track: track.id.clone(),
            },
        });
    }

    fn open_track_pan(&mut self) {
        let Some(track) = self.selected_track_ref() else {
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · Pan", track.name),
            prompt: "Exact pan (-1 … 1)".to_owned(),
            value: Unit::Pan.format_value(track.channel.panner.primary_position()),
            replace_on_type: true,
            action: InputAction::TrackPan {
                track: track.id.clone(),
            },
        });
    }

    fn open_automation_value(&mut self) {
        let Some((lane, point)) = self.selected_automation_point_pair() else {
            self.status = "Select an Automation point first".to_owned();
            return;
        };
        let lane_id = lane.id.clone();
        let point_id = point.id.clone();
        let authored_value = point.value.clone();
        let prompt = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| project.parameter_spec(&lane.target))
            .map_or_else(
                || "Exact physical Parameter value".to_owned(),
                |spec| {
                    format!(
                        "Exact {} value ({} … {})",
                        spec.unit,
                        spec.unit.format_value(spec.minimum),
                        spec.unit.format_value(spec.maximum)
                    )
                },
            );
        self.modal = Some(Modal::Input {
            title: format!("{lane_id} · {point_id} · Value"),
            prompt,
            value: authored_value,
            replace_on_type: true,
            action: InputAction::AutomationValue {
                lane: lane_id,
                point: point_id,
            },
        });
    }

    fn open_automation_time(&mut self) {
        let Some((lane, point)) = self.selected_automation_point_pair() else {
            self.status = "Select an Automation point first".to_owned();
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · {} · Song Time", lane.id, point.id),
            prompt: "Exact absolute musical time (for example 2bar or 3/2beat)".to_owned(),
            value: point.at.to_string(),
            replace_on_type: true,
            action: InputAction::AutomationTime {
                lane: lane.id.clone(),
                point: point.id.clone(),
            },
        });
    }

    fn open_automation_curve(&mut self) {
        let Some((lane, point)) = self.selected_automation_point_pair() else {
            self.status = "Select an Automation point first".to_owned();
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · {} · Curve", lane.id, point.id),
            prompt: "Interpolation leaving this point: step, linear, or power(P)".to_owned(),
            value: point.interpolation.to_string(),
            replace_on_type: true,
            action: InputAction::AutomationCurve {
                lane: lane.id.clone(),
                point: point.id.clone(),
            },
        });
    }

    fn open_event_primary(&mut self) {
        let Some(event) = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| project.song.patterns.get(self.selected_pattern))
            .and_then(|pattern| pattern.events.get(self.selected_event))
        else {
            return;
        };
        match event {
            PatternEvent::Note { .. } => self.open_note_pitch(),
            PatternEvent::Hit { .. } => self.open_hit_voice(),
            PatternEvent::Sample { .. } => self.open_note_duration(),
        }
    }

    fn open_note_pitch(&mut self) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(pattern) = project.song.patterns.get(self.selected_pattern) else {
            return;
        };
        let Some(PatternEvent::Note { pitch, .. }) = pattern.events.get(self.selected_event) else {
            self.status = "Selected percussion hit has no pitch".to_owned();
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · Event {}", pattern.id, self.selected_event),
            prompt: "Pitch name (for example C4 or F#3)".to_owned(),
            value: crate::model::format_pitch(*pitch),
            replace_on_type: true,
            action: InputAction::NotePitch {
                pattern: pattern.id.clone(),
                event: self.selected_event,
            },
        });
    }

    fn open_event_time(&mut self) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(pattern) = project.song.patterns.get(self.selected_pattern) else {
            return;
        };
        let Some(event) = pattern.events.get(self.selected_event) else {
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · Event {} onset", pattern.id, self.selected_event),
            prompt: "Exact musical time (for example 1/2beat or 2beat)".to_owned(),
            value: event.at().to_string(),
            replace_on_type: true,
            action: InputAction::EventTime {
                pattern: pattern.id.clone(),
                event: self.selected_event,
            },
        });
    }

    fn open_note_duration(&mut self) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(pattern) = project.song.patterns.get(self.selected_pattern) else {
            return;
        };
        let duration = match pattern.events.get(self.selected_event) {
            Some(PatternEvent::Note { duration, .. } | PatternEvent::Sample { duration, .. }) => {
                *duration
            }
            _ => {
                self.status = "Selected percussion hit has no duration".to_owned();
                return;
            }
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · Event {} duration", pattern.id, self.selected_event),
            prompt: "Positive exact musical duration".to_owned(),
            value: duration.to_string(),
            replace_on_type: true,
            action: InputAction::NoteDuration {
                pattern: pattern.id.clone(),
                event: self.selected_event,
            },
        });
    }

    fn open_event_velocity(&mut self) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(pattern) = project.song.patterns.get(self.selected_pattern) else {
            return;
        };
        let Some(event) = pattern.events.get(self.selected_event) else {
            return;
        };
        let velocity = match event {
            PatternEvent::Note { velocity, .. }
            | PatternEvent::Hit { velocity, .. }
            | PatternEvent::Sample { velocity, .. } => *velocity,
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · Event {} velocity", pattern.id, self.selected_event),
            prompt: "Velocity ratio from 0 to 1".to_owned(),
            value: Unit::Ratio.format_value(velocity),
            replace_on_type: true,
            action: InputAction::EventVelocity {
                pattern: pattern.id.clone(),
                event: self.selected_event,
            },
        });
    }

    fn open_hit_voice(&mut self) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(pattern) = project.song.patterns.get(self.selected_pattern) else {
            return;
        };
        let Some(PatternEvent::Hit { voice, .. }) = pattern.events.get(self.selected_event) else {
            self.status = "Selected pitched note has no percussion voice".to_owned();
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · Event {} voice", pattern.id, self.selected_event),
            prompt: "kick, snare, hat, or clap".to_owned(),
            value: format!("{voice:?}").to_lowercase(),
            replace_on_type: true,
            action: InputAction::HitVoice {
                pattern: pattern.id.clone(),
                event: self.selected_event,
            },
        });
    }

    fn open_sample_region(&mut self) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(pattern) = project.song.patterns.get(self.selected_pattern) else {
            return;
        };
        let Some(PatternEvent::Sample {
            id,
            start_frame,
            end_frame,
            ..
        }) = pattern.events.get(self.selected_event)
        else {
            self.status = "Selected Event is not a sample occurrence".to_owned();
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · {id} source region", pattern.id),
            prompt: "Half-open original-source frames as START:END".to_owned(),
            value: format!("{start_frame}:{end_frame}"),
            replace_on_type: true,
            action: InputAction::SampleRegion {
                pattern: pattern.id.clone(),
                id: id.clone(),
            },
        });
    }

    fn open_sample_fade(&mut self) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(pattern) = project.song.patterns.get(self.selected_pattern) else {
            return;
        };
        let Some(PatternEvent::Sample {
            id,
            fade_in_frames,
            fade_out_frames,
            ..
        }) = pattern.events.get(self.selected_event)
        else {
            self.status = "Selected Event is not a sample occurrence".to_owned();
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · {id} source fades", pattern.id),
            prompt: "Original-source fade frames as IN:OUT".to_owned(),
            value: format!("{fade_in_frames}:{fade_out_frames}"),
            replace_on_type: true,
            action: InputAction::SampleFade {
                pattern: pattern.id.clone(),
                id: id.clone(),
            },
        });
    }

    fn open_sample_gain(&mut self) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(pattern) = project.song.patterns.get(self.selected_pattern) else {
            return;
        };
        let Some(PatternEvent::Sample { id, gain_db, .. }) =
            pattern.events.get(self.selected_event)
        else {
            self.status = "Selected Event is not a sample occurrence".to_owned();
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · {id} gain", pattern.id),
            prompt: "Gain trim in dB (-96db to 24db)".to_owned(),
            value: Unit::Decibels.format_value(*gain_db),
            replace_on_type: true,
            action: InputAction::SampleGain {
                pattern: pattern.id.clone(),
                id: id.clone(),
            },
        });
    }

    fn open_sample_pan(&mut self) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(pattern) = project.song.patterns.get(self.selected_pattern) else {
            return;
        };
        let Some(PatternEvent::Sample { id, pan, .. }) = pattern.events.get(self.selected_event)
        else {
            self.status = "Selected Event is not a sample occurrence".to_owned();
            return;
        };
        self.modal = Some(Modal::Input {
            title: format!("{} · {id} pan", pattern.id),
            prompt: "Pan from -1 to 1".to_owned(),
            value: Unit::Pan.format_value(*pan),
            replace_on_type: true,
            action: InputAction::SamplePan {
                pattern: pattern.id.clone(),
                id: id.clone(),
            },
        });
    }

    fn open_publish(&mut self) {
        if self.bundle.diagnostics.has_errors() || self.bundle.project.is_none() {
            self.status = "Invalid Candidate cannot publish; inspect Diagnostics".to_owned();
            self.view = View::Diagnostics;
            return;
        }
        if self.candidate_is_active() {
            self.status = "Candidate is already the Active Revision".to_owned();
            return;
        }
        self.modal = Some(Modal::Input {
            title: "Publish Candidate".to_owned(),
            prompt: "History summary".to_owned(),
            value: "Publish inspected Candidate".to_owned(),
            replace_on_type: true,
            action: InputAction::Publish,
        });
    }

    fn open_restore(&mut self) {
        if let Some(entry) = self.history.get(self.selected_history) {
            self.modal = Some(Modal::ConfirmRestore {
                entry_id: entry.id.clone(),
            });
        }
    }

    fn open_history_diff(&mut self) {
        let Some(entry) = self.history.get(self.selected_history) else {
            return;
        };
        let title = format!("{} · {}", entry.id, entry.summary);
        let body = HistoryStore::open(&self.root)
            .and_then(|store| store.diff(&entry.id))
            .unwrap_or_else(|error| format!("Unable to read diff: {error}"));
        self.modal = Some(Modal::Text {
            title,
            body,
            scroll: 0,
        });
    }

    fn commit_input(&mut self, value: String, action: InputAction) {
        match action {
            InputAction::Target { target, unit } => match unit.parse_value(value.trim()) {
                Ok(value) => {
                    let replacement = set_parameter_target(&self.bundle, &target, value);
                    self.apply_replacement(replacement, format!("Set {target}"));
                }
                Err(error) => self.status = format!("Invalid physical value: {error}"),
            },
            InputAction::Parameter {
                instrument,
                parameter,
                unit,
            } => match unit.parse_value(value.trim()) {
                Ok(value) => {
                    let replacement = set_parameter(&self.bundle, &instrument, &parameter, value);
                    self.apply_replacement(replacement, format!("Set {instrument}.{parameter}"));
                }
                Err(error) => self.status = format!("Invalid physical value: {error}"),
            },
            InputAction::TrackLevel { track } => match Unit::Decibels.parse_value(value.trim()) {
                Ok(value) => {
                    let replacement = set_track_level(&self.bundle, &track, value);
                    self.apply_replacement(replacement, format!("Set {track} level"));
                }
                Err(error) => self.status = format!("Invalid level: {error}"),
            },
            InputAction::TrackPan { track } => match Unit::Pan.parse_value(value.trim()) {
                Ok(value) => {
                    let replacement = set_track_pan(&self.bundle, &track, value);
                    self.apply_replacement(replacement, format!("Set {track} pan"));
                }
                Err(error) => self.status = format!("Invalid pan: {error}"),
            },
            InputAction::ProcessorSetting { processor } => {
                if let Some((setting, value)) = value.trim().split_once(char::is_whitespace) {
                    let replacement =
                        set_processor_setting(&self.bundle, &processor, setting, value.trim());
                    self.apply_replacement(replacement, format!("Set {processor}.{setting}"));
                } else {
                    self.status = "Enter a processor setting as `KEY VALUE`".to_owned();
                }
            }
            InputAction::AuditionTarget => match parse_audition_target(value.trim()) {
                Ok(target) => {
                    let mut plan = self.audition_plan.clone();
                    plan.path = default_audition_path(&target);
                    plan.target = target;
                    plan.range = AuditionRange::default();
                    self.activate_audition_plan(plan);
                }
                Err(error) => self.status = error,
            },
            InputAction::AuditionRange => {
                let trimmed = value.trim();
                let mut plan = self.audition_plan.clone();
                if trimmed.is_empty() {
                    plan.range.from = None;
                    plan.range.to = None;
                } else {
                    let values = trimmed.split_whitespace().collect::<Vec<_>>();
                    if values.len() != 2 {
                        self.status = "Audition range requires exact `FROM TO` values".to_owned();
                        return;
                    }
                    plan.range.from = Some(values[0].to_owned());
                    plan.range.to = Some(values[1].to_owned());
                }
                self.activate_audition_plan(plan);
            }
            InputAction::AutomationValue { lane, point } => {
                let replacement =
                    set_automation_point_value(&self.bundle, &lane, &point, value.trim());
                self.apply_replacement(replacement, format!("Set {lane}.{point} Automation value"));
            }
            InputAction::AutomationTime { lane, point } => {
                let replacement =
                    set_automation_point_at(&self.bundle, &lane, &point, value.trim());
                self.apply_replacement(
                    replacement,
                    format!("Move {lane}.{point} Automation point"),
                );
            }
            InputAction::AutomationCurve { lane, point } => {
                match value.trim().parse::<AutomationInterpolation>() {
                    Ok(interpolation) => {
                        let replacement = set_automation_point_interpolation(
                            &self.bundle,
                            &lane,
                            &point,
                            interpolation,
                        );
                        self.apply_replacement(
                            replacement,
                            format!("Set {lane}.{point} Automation curve"),
                        );
                    }
                    Err(error) => self.status = format!("Invalid Automation curve: {error}"),
                }
            }
            InputAction::NotePitch { pattern, event } => match parse_pitch(value.trim()) {
                Ok(pitch) => {
                    let replacement = set_note_pitch(&self.bundle, &pattern, event, pitch);
                    self.apply_replacement(replacement, format!("Change Event in {pattern}"));
                }
                Err(error) => self.status = format!("Invalid pitch: {error}"),
            },
            InputAction::EventTime { pattern, event } => {
                let replacement = set_event_at(&self.bundle, &pattern, event, value.trim());
                self.apply_replacement(replacement, format!("Move Event in {pattern}"));
            }
            InputAction::NoteDuration { pattern, event } => {
                let replacement = set_note_duration(&self.bundle, &pattern, event, value.trim());
                self.apply_replacement(replacement, format!("Resize Event in {pattern}"));
            }
            InputAction::EventVelocity { pattern, event } => {
                match Unit::Ratio.parse_value(value.trim()) {
                    Ok(velocity) => {
                        let replacement =
                            set_event_velocity(&self.bundle, &pattern, event, velocity);
                        self.apply_replacement(
                            replacement,
                            format!("Set Event velocity in {pattern}"),
                        );
                    }
                    Err(error) => self.status = format!("Invalid velocity: {error}"),
                }
            }
            InputAction::HitVoice { pattern, event } => match value.trim().parse::<DrumVoice>() {
                Ok(voice) => {
                    let replacement = set_hit_voice(&self.bundle, &pattern, event, voice);
                    self.apply_replacement(
                        replacement,
                        format!("Change percussion voice in {pattern}"),
                    );
                }
                Err(error) => self.status = format!("Invalid percussion voice: {error}"),
            },
            InputAction::SampleRegion { pattern, id } => match parse_frame_pair(value.trim()) {
                Ok((start, end)) => {
                    let replacements =
                        trim_sample_occurrence(&self.bundle, &pattern, &id, start, end);
                    self.apply_replacements(replacements, format!("Trim sample {pattern}.{id}"));
                }
                Err(error) => self.status = error,
            },
            InputAction::SampleFade { pattern, id } => match parse_fade_pair(value.trim()) {
                Ok((fade_in, fade_out)) => {
                    let replacements =
                        fade_sample_occurrence(&self.bundle, &pattern, &id, fade_in, fade_out);
                    self.apply_replacements(replacements, format!("Fade sample {pattern}.{id}"));
                }
                Err(error) => self.status = error,
            },
            InputAction::SampleGain { pattern, id } => {
                match Unit::Decibels.parse_value(value.trim()) {
                    Ok(gain_db) => {
                        let replacement =
                            set_sample_occurrence_gain(&self.bundle, &pattern, &id, gain_db);
                        self.apply_replacement(
                            replacement,
                            format!("Set sample {pattern}.{id} gain"),
                        );
                    }
                    Err(error) => self.status = format!("Invalid sample gain: {error}"),
                }
            }
            InputAction::SamplePan { pattern, id } => match Unit::Pan.parse_value(value.trim()) {
                Ok(pan) => {
                    let replacement = set_sample_occurrence_pan(&self.bundle, &pattern, &id, pan);
                    self.apply_replacement(replacement, format!("Set sample {pattern}.{id} pan"));
                }
                Err(error) => self.status = format!("Invalid sample pan: {error}"),
            },
            InputAction::Publish => {
                let summary = value.trim();
                if summary.is_empty() {
                    self.status = "Publication summary cannot be empty".to_owned();
                    return;
                }
                match HistoryStore::open(&self.root).and_then(|store| {
                    store.publish(
                        &self.bundle,
                        Publication::new(
                            ChangeOrigin::human("workspace"),
                            summary,
                            "Candidate inspected and published from the Production Workspace",
                        ),
                    )
                }) {
                    Ok(entry) => {
                        self.status = format!("Published as history entry {}", entry.id);
                        self.refresh();
                    }
                    Err(error) => self.status = format!("Publication failed: {error}"),
                }
            }
        }
    }

    fn step_parameter(&mut self, direction: f64) {
        let Some((instrument, parameter)) = self.selected_parameter_pair() else {
            return;
        };
        let instrument_id = instrument.id.clone();
        let parameter_key = parameter.key.clone();
        let step = ((parameter.maximum - parameter.minimum) / 100.0).max(f64::EPSILON);
        let value =
            (parameter.value + direction * step).clamp(parameter.minimum, parameter.maximum);
        let replacement = set_parameter(&self.bundle, &instrument_id, &parameter_key, value);
        self.apply_replacement(
            replacement,
            format!("Adjust {instrument_id}.{parameter_key}"),
        );
    }

    fn step_mixer_parameter(&mut self, direction: f64) {
        let Some((target, spec)) = self.selected_mixer_parameter() else {
            return;
        };
        let value = if spec.unit == Unit::Boolean {
            if direction < 0.0 { 0.0 } else { 1.0 }
        } else {
            let step = ((spec.maximum - spec.minimum) / 100.0).max(f64::EPSILON);
            (spec.base + direction * step).clamp(spec.minimum, spec.maximum)
        };
        let replacement = set_parameter_target(&self.bundle, &target, value);
        self.apply_replacement(replacement, format!("Adjust {target}"));
    }

    fn step_track_level(&mut self, direction: f64) {
        let Some(track) = self.selected_track_ref() else {
            return;
        };
        let id = track.id.clone();
        let value = (track.channel.level_db + direction).clamp(-96.0, 12.0);
        let replacement = set_track_level(&self.bundle, &id, value);
        self.apply_replacement(replacement, format!("Adjust {id} level"));
    }

    fn step_automation(&mut self, direction: f64) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(lane) = project.song.automations.get(self.selected_automation) else {
            return;
        };
        let Some(point) = lane.points.get(self.selected_automation_point) else {
            return;
        };
        let Some(spec) = project.parameter_spec(&lane.target) else {
            self.status = "Selected Automation target is unavailable".to_owned();
            return;
        };
        let Ok(current) = spec.unit.parse_value(&point.value) else {
            self.status = "Selected Automation value is invalid".to_owned();
            return;
        };
        let lane_id = lane.id.clone();
        let point_id = point.id.clone();
        let step = ((spec.maximum - spec.minimum) / 100.0).max(f64::EPSILON);
        let next = (current + direction * step).clamp(spec.minimum, spec.maximum);
        let authored = spec.unit.format_value(next);
        let replacement = set_automation_point_value(&self.bundle, &lane_id, &point_id, &authored);
        self.apply_replacement(
            replacement,
            format!("Adjust {lane_id}.{point_id} Automation value"),
        );
    }

    fn step_note(&mut self, direction: isize) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(pattern) = project.song.patterns.get(self.selected_pattern) else {
            return;
        };
        let Some(event) = pattern.events.get(self.selected_event) else {
            return;
        };
        let pattern_id = pattern.id.clone();
        let replacement = match event {
            PatternEvent::Note { pitch, .. } => {
                let next = pitch
                    .saturating_add_signed(direction.clamp(-1, 1) as i8)
                    .min(127);
                set_note_pitch(&self.bundle, &pattern_id, self.selected_event, next)
            }
            PatternEvent::Hit { voice, .. } => {
                let voices = [
                    DrumVoice::Kick,
                    DrumVoice::Snare,
                    DrumVoice::Hat,
                    DrumVoice::Clap,
                ];
                let current = voices
                    .iter()
                    .position(|candidate| candidate == voice)
                    .unwrap_or(0);
                let next = wrap_index(current, voices.len(), direction);
                set_hit_voice(&self.bundle, &pattern_id, self.selected_event, voices[next])
            }
            PatternEvent::Sample { .. } => Err(crate::edit::EditError::WrongTargetKind(
                "sample occurrences use exact region/velocity edits".to_owned(),
            )),
        };
        self.apply_replacement(replacement, format!("Change Event in {pattern_id}"));
    }

    fn apply_replacement(
        &mut self,
        replacement: Result<crate::edit::CheckedReplacement, crate::edit::EditError>,
        summary: String,
    ) {
        self.apply_replacements(replacement.map(|replacement| vec![replacement]), summary);
    }

    fn apply_replacements(
        &mut self,
        replacements: Result<Vec<crate::edit::CheckedReplacement>, crate::edit::EditError>,
        summary: String,
    ) {
        let replacements = match replacements {
            Ok(replacements) => replacements,
            Err(error) => {
                self.status = format!("Edit rejected: {error}");
                return;
            }
        };
        match apply_structured_edit(
            &self.root,
            &self.bundle.revision,
            replacements,
            Publication::new(
                ChangeOrigin::human("workspace"),
                summary,
                "Checked source-backed Production View edit",
            ),
        ) {
            Ok(entry) => {
                self.status = format!("Saved and published history entry {}", entry.id);
                self.refresh();
            }
            Err(error) => self.status = format!("Edit transaction failed: {error}"),
        }
    }

    fn restore_history(&mut self, entry_id: &str) {
        match HistoryStore::open(&self.root).and_then(|store| {
            store.restore(
                entry_id,
                ChangeOrigin::human("workspace"),
                "Restore selected state from Project History",
            )
        }) {
            Ok(entry) => {
                self.status = format!("Restored {entry_id} as new entry {}", entry.id);
                self.refresh();
            }
            Err(error) => self.status = format!("Restore failed: {error}"),
        }
    }

    fn handle_audition_key(&mut self, key: KeyEvent) -> bool {
        if !matches!(self.view, View::Faceplate | View::Parameters | View::Graph) {
            return false;
        }
        let KeyCode::Char(character) = key.code else {
            return false;
        };
        let Some(pitch) = self.keyboard_audition_pitch(character) else {
            return false;
        };
        let track_index = self.audition_track();
        match key.kind {
            KeyEventKind::Press if !self.held_notes.contains_key(&character) => {
                let note_id = self.next_note_id;
                self.next_note_id = self.next_note_id.saturating_add(1);
                if let Some(player) = self.player.as_mut() {
                    if player.note_on(track_index, note_id, pitch, 0.8).is_ok() {
                        self.held_notes.insert(
                            character,
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
            KeyEventKind::Release => self.release_audition(character),
            _ => {}
        }
        true
    }

    fn release_audition(&mut self, character: char) {
        let Some(note) = self.held_notes.remove(&character) else {
            return;
        };
        if let Some(player) = self.player.as_mut() {
            let _ = player.note_off(note.track_index, note.note_id);
        }
    }

    fn release_stale_auditions(&mut self) {
        let stale: Vec<_> = self
            .held_notes
            .iter()
            .filter_map(|(key, note)| {
                (note.pressed_at.elapsed() >= AUDITION_TIMEOUT).then_some(*key)
            })
            .collect();
        for key in stale {
            self.release_audition(key);
        }
    }

    fn keyboard_audition_pitch(&self, character: char) -> Option<u8> {
        let is_drums = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| project.song.instruments.get(self.selected_instrument))
            .is_some_and(|instrument| instrument.kind == InstrumentKind::StockDrums);
        if is_drums {
            match character.to_ascii_lowercase() {
                'a' | 'z' => Some(36),
                's' | 'x' => Some(38),
                'd' | 'c' => Some(42),
                'f' | 'v' => Some(39),
                _ => None,
            }
        } else {
            keyboard_pitch(character)
        }
    }

    fn audition_track(&self) -> usize {
        let Some(candidate) = self.bundle.project.as_ref() else {
            return 0;
        };
        let runtime = self.active_project.as_ref().unwrap_or(candidate);
        let candidate_track = candidate.song.tracks.get(self.selected_track);
        if self.view == View::Timeline
            && let Some(track_id) = candidate_track.map(|track| track.id.as_str())
            && let Some(index) = runtime
                .song
                .tracks
                .iter()
                .position(|track| track.id == track_id)
        {
            return index;
        }
        let instrument_id = candidate
            .song
            .instruments
            .get(self.selected_instrument)
            .map(|instrument| instrument.id.as_str())
            .or_else(|| candidate_track.map(|track| track.instrument.as_str()));
        instrument_id
            .and_then(|id| {
                runtime
                    .song
                    .tracks
                    .iter()
                    .position(|track| track.instrument == id)
            })
            .unwrap_or(0)
    }

    fn sync_instrument_to_track(&mut self) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let Some(track) = project.song.tracks.get(self.selected_track) else {
            return;
        };
        if let Some(index) = project
            .song
            .instruments
            .iter()
            .position(|instrument| instrument.id == track.instrument)
        {
            self.selected_instrument = index;
            self.selected_parameter = 0;
        }
    }

    fn selected_mixer_parameter(&self) -> Option<(ParameterTarget, ParameterSpec)> {
        self.bundle
            .project
            .as_ref()?
            .mix_parameter_specs()
            .get(self.selected_mixer_parameter)
            .cloned()
    }

    fn selected_mixer_channel_identity(&self) -> Option<String> {
        let project = self.bundle.project.as_ref()?;
        let (target, _) = self.selected_mixer_parameter()?;
        mixer_channel_identity(project, &target)
    }

    fn selected_mixer_mute_target(&self) -> Option<ParameterTarget> {
        let identity = self.selected_mixer_channel_identity()?;
        mixer_mute_target(&identity)
    }

    fn selected_track_ref(&self) -> Option<&crate::model::Track> {
        self.bundle
            .project
            .as_ref()?
            .song
            .tracks
            .get(self.selected_track)
    }

    fn selected_automation_point_pair(&self) -> Option<(&AutomationLane, &AutomationPoint)> {
        let lane = self
            .bundle
            .project
            .as_ref()?
            .song
            .automations
            .get(self.selected_automation)?;
        let point = lane.points.get(self.selected_automation_point)?;
        Some((lane, point))
    }

    fn selected_parameter_pair(
        &self,
    ) -> Option<(&crate::model::Instrument, &crate::model::Parameter)> {
        let project = self.bundle.project.as_ref()?;
        let instrument = project.song.instruments.get(self.selected_instrument)?;
        let parameter = if self.view == View::Faceplate {
            let control = instrument.faceplate.get(self.selected_parameter)?;
            instrument
                .parameters
                .iter()
                .find(|parameter| parameter.key == control.parameter)?
        } else {
            instrument.parameters.get(self.selected_parameter)?
        };
        Some((instrument, parameter))
    }

    fn candidate_is_active(&self) -> bool {
        self.active_revision.as_ref() == Some(&self.bundle.revision)
    }

    fn clamp_selections(&mut self) {
        self.selected_track = clamp_index(self.selected_track, self.track_count());
        self.selected_mixer_parameter =
            clamp_index(self.selected_mixer_parameter, self.mixer_parameter_count());
        self.selected_processor = clamp_index(
            self.selected_processor,
            self.processing_graph
                .as_ref()
                .map_or(0, |graph| graph.processors.len()),
        );
        self.selected_pattern = clamp_index(self.selected_pattern, self.pattern_count());
        self.selected_event = clamp_index(self.selected_event, self.event_count());
        self.selected_instrument = clamp_index(self.selected_instrument, self.instrument_count());
        self.selected_parameter = clamp_index(
            self.selected_parameter,
            if self.view == View::Faceplate {
                self.faceplate_count()
            } else {
                self.parameter_count()
            },
        );
        self.selected_automation = clamp_index(self.selected_automation, self.automation_count());
        self.selected_automation_point = clamp_index(
            self.selected_automation_point,
            self.automation_point_count(),
        );
        self.selected_graph_node = clamp_index(self.selected_graph_node, self.graph_node_count());
        self.selected_history = clamp_index(self.selected_history, self.history.len());
        self.selected_source = clamp_index(self.selected_source, self.bundle.sources.len());
    }

    fn track_count(&self) -> usize {
        self.bundle
            .project
            .as_ref()
            .map_or(0, |project| project.song.tracks.len())
    }

    fn mixer_parameter_count(&self) -> usize {
        self.bundle
            .project
            .as_ref()
            .map_or(0, |project| project.mix_parameter_specs().len())
    }

    fn pattern_count(&self) -> usize {
        self.bundle
            .project
            .as_ref()
            .map_or(0, |project| project.song.patterns.len())
    }

    fn event_count(&self) -> usize {
        self.bundle
            .project
            .as_ref()
            .and_then(|project| project.song.patterns.get(self.selected_pattern))
            .map_or(0, |pattern| pattern.events.len())
    }

    fn instrument_count(&self) -> usize {
        self.bundle
            .project
            .as_ref()
            .map_or(0, |project| project.song.instruments.len())
    }

    fn automation_count(&self) -> usize {
        self.bundle
            .project
            .as_ref()
            .map_or(0, |project| project.song.automations.len())
    }

    fn automation_point_count(&self) -> usize {
        self.bundle
            .project
            .as_ref()
            .and_then(|project| project.song.automations.get(self.selected_automation))
            .map_or(0, |lane| lane.points.len())
    }

    fn graph_node_count(&self) -> usize {
        self.bundle
            .project
            .as_ref()
            .and_then(|project| project.song.instruments.get(self.selected_instrument))
            .and_then(|instrument| {
                instrument.graph.as_ref().map(|graph| {
                    graph.nodes.len()
                        + graph.connections.len()
                        + instrument.parameters.len()
                        + instrument.faceplate.len()
                })
            })
            .unwrap_or(0)
    }

    fn parameter_count(&self) -> usize {
        self.bundle
            .project
            .as_ref()
            .and_then(|project| project.song.instruments.get(self.selected_instrument))
            .map_or(0, |instrument| instrument.parameters.len())
    }

    fn faceplate_count(&self) -> usize {
        self.bundle
            .project
            .as_ref()
            .and_then(|project| project.song.instruments.get(self.selected_instrument))
            .map_or(0, |instrument| instrument.faceplate.len())
    }
}

type HistorySnapshot = (
    Option<String>,
    Option<RevisionId>,
    Vec<HistoryEntry>,
    Option<Project>,
);

fn history_state(root: &Path) -> Result<HistorySnapshot, crate::history::HistoryError> {
    let store = HistoryStore::open(root)?;
    let state = store.active_state()?;
    let history = store.list()?;
    let active_project = store.active_bundle()?.and_then(|bundle| bundle.project);
    Ok(if let Some(state) = state {
        (
            Some(state.active_entry),
            Some(state.active_revision),
            history,
            active_project,
        )
    } else {
        (None, None, history, None)
    })
}

fn render(frame: &mut Frame<'_>, app: &Workspace) {
    let area = frame.area();
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);
    render_header(frame, app, regions[0]);
    render_tabs(frame, app, regions[1]);
    match app.view {
        View::Timeline => render_timeline(frame, app, regions[2]),
        View::Mixer => render_mixer(frame, app, regions[2]),
        View::Processing => render_processing(frame, app, regions[2]),
        View::EventGrid => render_event_grid(frame, app, regions[2]),
        View::Faceplate => render_faceplate(frame, app, regions[2]),
        View::Parameters => render_parameters(frame, app, regions[2]),
        View::Automation => render_automation(frame, app, regions[2]),
        View::Graph => render_graph(frame, app, regions[2]),
        View::Source => render_source(frame, app, regions[2]),
        View::Diagnostics => render_diagnostics(frame, app, regions[2]),
        View::History => render_history(frame, app, regions[2]),
        View::Help => render_help(frame, regions[2]),
    }
    render_footer(frame, app, regions[3]);
    if let Some(modal) = &app.modal {
        render_modal(frame, modal, area);
    }
}

fn render_header(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let project_name = app
        .bundle
        .project
        .as_ref()
        .map_or("Unparseable Project", |project| project.name.as_str());
    let candidate_state = if app.bundle.diagnostics.has_errors() {
        Span::styled(
            "INVALID CANDIDATE",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else if app.candidate_is_active() {
        Span::styled(
            "ACTIVE",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "VALID CANDIDATE",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    };
    let audio = if app.player.is_some() {
        let transport = format!(
            "{}{}",
            transport_label(app),
            if app.playback_paused { " PAUSED" } else { "" }
        );
        let audition = app
            .audition_status
            .as_ref()
            .map_or_else(String::new, |status| {
                format!(
                    " {} via {:?}{}",
                    audition_target_spec(&status.target),
                    status.path,
                    if status.looping { " LOOP" } else { "" }
                )
            });
        format!(
            " {transport}{audition}  {:?}{}  peak {:>5.2}  voices {:>2}  MIDI {}",
            app.telemetry.stream_state,
            if app.telemetry.transition_active {
                " XFADE/RESTART"
            } else {
                ""
            },
            app.telemetry.engine.current_peak,
            app.telemetry.engine.active_voices,
            if app.telemetry.midi_connected {
                "●"
            } else {
                "○"
            }
        )
    } else {
        format!(
            " stopped · armed {} via {:?}",
            audition_target_spec(&app.audition_plan.target),
            app.audition_plan.path
        )
    };
    let line = Line::from(vec![
        Span::styled(
            format!(" CRUMP · {project_name}  "),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        candidate_state,
        Span::raw(format!(
            "  C:{}  A:{}  │{}",
            short_revision(&app.bundle.revision),
            short_optional(app.active_revision.as_ref()),
            audio
        )),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_tabs(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let titles = VIEWS
        .iter()
        .enumerate()
        .map(|(index, view)| {
            let key = match index {
                0..=8 => (index + 1).to_string(),
                9 => "0".to_owned(),
                10 => "H".to_owned(),
                _ => "?".to_owned(),
            };
            Line::from(format!("{key} {}", view.title()))
        })
        .collect::<Vec<_>>();
    let selected = VIEWS.iter().position(|view| *view == app.view).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::TOP | Borders::BOTTOM))
        .select(selected)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .divider(" │ ");
    frame.render_widget(tabs, area);
}

fn render_timeline(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(
            frame,
            area,
            "Timeline unavailable while Project source is unparseable",
        );
        return;
    };
    let context_height = if project.song.locators.is_empty() && project.song.grooves.is_empty() {
        0
    } else {
        3
    };
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(context_height), Constraint::Min(5)])
        .split(area);
    if context_height > 0 {
        let locators = project
            .song
            .locators
            .iter()
            .map(|locator| format!("{}={} ({})", locator.id, locator.at, locator.name))
            .collect::<Vec<_>>()
            .join("  ");
        let grooves = project
            .song
            .grooves
            .iter()
            .map(|groove| groove.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        frame.render_widget(
            Paragraph::new(format!(
                "Locators: {}\nGrooves: {}",
                if locators.is_empty() {
                    "—"
                } else {
                    &locators
                },
                if grooves.is_empty() { "—" } else { &grooves }
            ))
            .block(
                Block::default()
                    .title(" Arrangement context ")
                    .borders(Borders::ALL),
            ),
            regions[0],
        );
    }
    let rows = project.song.tracks.iter().map(|track| {
        let clips = track
            .clips
            .iter()
            .map(|clip| {
                format!(
                    "{}:{}+{} {:?} off={} tr={:+}st gain={:+.1}db fade={}/{}{}",
                    clip.id,
                    clip.start,
                    clip.duration,
                    clip.mode,
                    clip.source_offset,
                    clip.transpose_semitones,
                    clip.gain_db,
                    clip.fade_in,
                    clip.fade_out,
                    clip.groove
                        .as_deref()
                        .map_or_else(String::new, |groove| format!(" groove={groove}"))
                )
            })
            .collect::<Vec<_>>()
            .join("  ");
        Row::new(vec![
            Cell::from(track.name.clone()),
            Cell::from(track.instrument.clone()),
            Cell::from(format!("{:.1} dB", track.channel.level_db)),
            Cell::from(format!("{:+.2}", track.channel.panner.primary_position())),
            Cell::from(clips),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(20),
        ],
    )
    .header(
        Row::new(["Track", "Instrument", "Level", "Pan", "Procedural Clips"])
            .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(format!(
                " Timeline · playhead {} · {} BPM · {} · end {} ",
                transport_label(app),
                project.song.tempo_bpm,
                project.song.meter,
                project.song.end
            ))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.selected_track));
    frame.render_stateful_widget(table, regions[1], &mut state);
}

fn render_mixer(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(
            frame,
            area,
            "Mixer unavailable while Project source is unparseable",
        );
        return;
    };
    let parameters = project.mix_parameter_specs();
    let rows = parameters.iter().map(|(target, spec)| {
        let identity = mixer_channel_identity(project, target);
        let meter = identity
            .as_ref()
            .and_then(|identity| {
                app.mix_channel_identities
                    .iter()
                    .position(|candidate| candidate == identity)
            })
            .filter(|index| *index < app.telemetry.engine.channel_count)
            .map(|index| app.telemetry.engine.channel_activity[index]);
        let muted = identity
            .as_deref()
            .and_then(mixer_mute_target)
            .and_then(|target| project.parameter_spec(&target))
            .is_some_and(|mute| mute.base >= 0.5);
        let audition = identity.as_ref().map_or_else(
            || "—".to_owned(),
            |identity| {
                format!(
                    "{}{}{}",
                    if muted { 'M' } else { '·' },
                    if app.audition_plan.soloed_channels.contains(identity) {
                        'S'
                    } else {
                        '·'
                    },
                    if app
                        .audition_plan
                        .cue
                        .as_ref()
                        .is_some_and(|cue| cue.channel == *identity)
                    {
                        'C'
                    } else {
                        '·'
                    }
                )
            },
        );
        Row::new(vec![
            Cell::from(target.to_string()),
            Cell::from(spec.unit.format_value(spec.base)),
            Cell::from(format!(
                "{} … {}",
                spec.unit.format_value(spec.minimum),
                spec.unit.format_value(spec.maximum)
            )),
            Cell::from(
                meter
                    .map(|meter| format!("{:.3}", meter.current_peak))
                    .unwrap_or_else(|| "—".to_owned()),
            ),
            Cell::from(
                meter
                    .map(|meter| format!("{:.3}", meter.average))
                    .unwrap_or_else(|| "—".to_owned()),
            ),
            Cell::from(audition),
            Cell::from(format!(
                "{}/{}",
                if spec.automatable { "A" } else { "·" },
                if spec.modulatable { "M" } else { "·" }
            )),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(31),
            Constraint::Length(12),
            Constraint::Percentage(24),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(6),
        ],
    )
    .header(
        Row::new([
            "Stable target",
            "Base",
            "Legal domain",
            "Peak",
            "Average",
            "M/S/C",
            "A/M",
        ])
        .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(format!(
                " Mixer · {} Tracks · {} Groups · {} Returns · {} Sidechains · m source Mute · s transient Solo · c/C post/pre cue · Enter exact · ←/→ step ",
                project.song.tracks.len(),
                project.song.groups.len(),
                project.song.returns.len(),
                project.song.sidechains.len()
            ))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.selected_mixer_parameter));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_processing(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(graph) = app.processing_graph.as_ref() else {
        render_unavailable(
            frame,
            area,
            "Processing inspection unavailable while the selected Project cannot compile",
        );
        return;
    };
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(5)])
        .split(area);
    let status = app.audition_status.as_ref();
    let target = status
        .map(|status| format!("{:?}", status.target))
        .unwrap_or_else(|| format!("{:?}", app.audition_plan.target));
    let path = status
        .map(|status| format!("{:?}", status.path))
        .unwrap_or_else(|| format!("{:?}", app.audition_plan.path));
    let range = status.map_or_else(
        || "target default".to_owned(),
        |status| {
            format!(
                "{}..{} ({} frames){}",
                status.start_frame,
                status.end_frame,
                status.end_frame.saturating_sub(status.start_frame),
                if status.looping { " loop" } else { "" }
            )
        },
    );
    let revision = status
        .map(|status| status.active_revision.to_string())
        .or_else(|| app.active_revision.as_ref().map(ToString::to_string))
        .unwrap_or_else(|| "none".to_owned());
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!("Target {target}  ·  Path {path}")),
            Line::from(format!(
                "Active Revision {revision}  ·  Range {range}  ·  Transport {}{}",
                transport_label(app),
                if app.playback_paused { " paused" } else { "" }
            )),
            Line::from(
                "A target · a path · F exact range · L loop · b transient bypass · B durable bypass · Enter setting · K/J reorder",
            ),
        ])
        .block(
            Block::default()
                .title(" Unified Audition ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true }),
        regions[0],
    );

    let rows = graph.processors.iter().map(|processor| {
        let meter = (processor.index < app.telemetry.engine.processor_count)
            .then(|| app.telemetry.engine.processor_activity[processor.index]);
        let transient = app
            .audition_plan
            .transient_bypass
            .get(&processor.id)
            .copied();
        let effective = meter.map_or_else(
            || transient.unwrap_or(processor.authored_bypass),
            |meter| meter.effective_bypass,
        );
        let bypass = format!(
            "D:{} T:{} E:{}{}",
            bool_mark(processor.authored_bypass),
            transient.map_or("—", bool_mark),
            bool_mark(effective),
            meter.map_or_else(String::new, |meter| format!(
                " {:.0}%",
                meter.bypass_mix * 100.0
            ))
        );
        let settings = processor
            .settings
            .iter()
            .map(|setting| {
                let value = setting.choice.clone().unwrap_or_else(|| {
                    setting
                        .unit
                        .zip(setting.value)
                        .map_or_else(|| "—".to_owned(), |(unit, value)| unit.format_value(value))
                });
                format!(
                    "{}={}{}",
                    setting.key,
                    value,
                    if setting.authored { "*" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        Row::new(vec![
            processor.index.to_string(),
            format!("{} / {}", processor.owner, processor.id),
            format!("{:?}", processor.kind).to_lowercase(),
            bypass,
            meter.map_or_else(
                || "—".to_owned(),
                |meter| format!("{:.3}", meter.input_peak),
            ),
            meter.map_or_else(
                || "—".to_owned(),
                |meter| format!("{:.3}", meter.output_peak),
            ),
            meter.map_or_else(
                || "—".to_owned(),
                |meter| {
                    format!(
                        "{:.1}/{:.1}",
                        meter.gain_reduction_db, meter.maximum_gain_reduction_db
                    )
                },
            ),
            processor.output_latency_frames.to_string(),
            settings,
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Percentage(22),
            Constraint::Length(11),
            Constraint::Length(24),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(7),
            Constraint::Min(18),
        ],
    )
    .header(
        Row::new([
            "#",
            "Owner / durable ID",
            "Kind",
            "Bypass D/T/E",
            "Input",
            "Output",
            "GR now/max",
            "Latency",
            "Resolved settings (* authored)",
        ])
        .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(format!(
                " Processing chain · {} instances · state continues while bypassed · aligned dry path ",
                graph.processors.len()
            ))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default()
        .with_selected((!graph.processors.is_empty()).then_some(app.selected_processor));
    frame.render_stateful_widget(table, regions[1], &mut state);
}

fn bool_mark(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn render_event_grid(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(frame, area, "Event Grid unavailable");
        return;
    };
    let Some(pattern) = project.song.patterns.get(app.selected_pattern) else {
        render_unavailable(frame, area, "No Pattern");
        return;
    };
    let mut procedure_lines = Vec::new();
    if let Some(variation) = &pattern.variation {
        procedure_lines.push(Line::from(format!(
            "Variation from {} · rotate {} · transpose {:+}st · velocity ×{} · reverse {} · probability {} · seed {}",
            variation.source,
            variation.rotate,
            variation.transpose_semitones,
            variation.velocity_scale,
            variation.reverse,
            variation.probability,
            variation.seed
        )));
    }
    procedure_lines.extend(pattern.locks.iter().map(|lock| {
        Line::from(format!(
            "Lock {} · {} · {} = {}",
            lock.id, lock.at, lock.parameter, lock.value
        ))
    }));
    let context_height = if procedure_lines.is_empty() {
        0
    } else {
        u16::try_from(procedure_lines.len())
            .unwrap_or(u16::MAX)
            .saturating_add(2)
            .min(area.height.saturating_sub(5))
    };
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(context_height)])
        .split(area);
    let rows = pattern.events.iter().enumerate().map(|(index, event)| {
        let row = match event {
            PatternEvent::Note {
                at,
                pitch,
                duration,
                velocity,
                ..
            } => Row::new(vec![
                index.to_string(),
                at.to_string(),
                "note".to_owned(),
                crate::model::format_pitch(*pitch),
                duration.to_string(),
                format!("{velocity:.2}"),
            ]),
            PatternEvent::Hit {
                at,
                voice,
                velocity,
                ..
            } => Row::new(vec![
                index.to_string(),
                at.to_string(),
                "hit".to_owned(),
                format!("{voice:?}").to_lowercase(),
                "—".to_owned(),
                format!("{velocity:.2}"),
            ]),
            PatternEvent::Sample {
                id,
                at,
                mapping,
                duration,
                velocity,
                ..
            } => Row::new(vec![
                index.to_string(),
                at.to_string(),
                "sample".to_owned(),
                format!("{id}:{mapping}"),
                duration.to_string(),
                format!("{velocity:.2}"),
            ]),
        };
        if event_is_active(app, &pattern.id, event) {
            row.style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            row
        }
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(["#", "At", "Kind", "Pitch / Voice", "Duration", "Velocity"])
            .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(format!(
                " Pattern {} · playhead {} · span {} · [/ ] Pattern · ←/→ note/voice · Enter pitch/voice · t time · u duration · v velocity ",
                pattern.id,
                transport_label(app),
                pattern.span
            ))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.selected_event));
    frame.render_stateful_widget(table, regions[0], &mut state);
    if context_height > 0 {
        frame.render_widget(
            Paragraph::new(procedure_lines).block(
                Block::default()
                    .title(" Procedural source & Parameter locks ")
                    .borders(Borders::ALL),
            ),
            regions[1],
        );
    }
}

fn render_faceplate(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(frame, area, "Instrument Faceplate unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        render_unavailable(frame, area, "No Instrument");
        return;
    };
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(format!(
            "{}  ·  {}  ·  polyphony {}  ·  ←/→ adjust  ·  Enter exact value",
            instrument.name, instrument.kind, instrument.polyphony
        ))
        .block(
            Block::default()
                .title(" Instrument Faceplate ")
                .borders(Borders::ALL),
        ),
        vertical[0],
    );
    let constraints = if instrument.faceplate.is_empty() {
        vec![Constraint::Min(1)]
    } else {
        vec![Constraint::Ratio(1, instrument.faceplate.len() as u32); instrument.faceplate.len()]
    };
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(vertical[1]);
    for (index, control) in instrument.faceplate.iter().enumerate() {
        let parameter = instrument
            .parameters
            .iter()
            .find(|parameter| parameter.key == control.parameter);
        let selected = index == app.selected_parameter;
        let value = parameter.map_or_else(
            || "missing".to_owned(),
            |parameter| parameter.unit.format_value(parameter.value),
        );
        let range = parameter.map_or_else(
            || "".to_owned(),
            |parameter| {
                format!(
                    "{} … {}",
                    parameter.unit.format_value(parameter.minimum),
                    parameter.unit.format_value(parameter.maximum)
                )
            },
        );
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(Span::styled(
                    value,
                    Style::default()
                        .fg(if selected {
                            Color::Black
                        } else {
                            Color::Yellow
                        })
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(range),
                Line::from(control.explanation.clone()),
            ]))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .title(format!(" {} ", control.label))
                    .borders(Borders::ALL)
                    .style(if selected {
                        selected_style()
                    } else {
                        Style::default()
                    }),
            ),
            columns[index],
        );
    }
    frame.render_widget(
        Paragraph::new("Note Audition: A W S E D F T G Y H U J K · transient only · PipeWire MIDI follows this Instrument")
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::TOP)),
        vertical[2],
    );
}

fn render_parameters(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(frame, area, "Parameters unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        render_unavailable(frame, area, "No Instrument");
        return;
    };
    let rows = instrument.parameters.iter().map(|parameter| {
        Row::new(vec![
            parameter.name.clone(),
            parameter.key.clone(),
            parameter.unit.to_string(),
            parameter.unit.format_value(parameter.value),
            format!(
                "{} … {}",
                parameter.unit.format_value(parameter.minimum),
                parameter.unit.format_value(parameter.maximum)
            ),
            parameter.unit.format_value(parameter.default),
            if parameter.automatable { "yes" } else { "no" }.to_owned(),
            if parameter.modulatable { "yes" } else { "no" }.to_owned(),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(14),
            Constraint::Length(11),
            Constraint::Length(14),
            Constraint::Length(24),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new([
            "Parameter",
            "Key",
            "Unit",
            "Value",
            "Legal domain",
            "Default",
            "Automate",
            "Modulate",
        ])
        .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(format!(" Exact Parameters · {} ", instrument.name))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.selected_parameter));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_automation(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(
            frame,
            area,
            "Automation unavailable while Project source is unparseable",
        );
        return;
    };
    let Some(lane) = project.song.automations.get(app.selected_automation) else {
        render_unavailable(
            frame,
            area,
            "No Song Automation lanes · use the CLI or readable source to create one",
        );
        return;
    };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(4)])
        .split(area);
    let spec = project.parameter_spec(&lane.target);
    let frame_position = app.telemetry.engine.frame;
    let snapshot = crate::engine::control::CompiledControlProgram::compile(project)
        .ok()
        .and_then(|program| {
            program
                .snapshot_at(frame_position)
                .into_iter()
                .find(|value| value.target == lane.target)
        });
    let base = spec.map_or_else(
        || "unresolved".to_owned(),
        |spec| spec.unit.format_value(spec.base),
    );
    let provenance = snapshot.map_or_else(
        || "effective unavailable".to_owned(),
        |value| {
            let automation = value.automation.map_or_else(
                || "none".to_owned(),
                |automation| value.unit.format_value(automation),
            );
            let routes = value
                .modulation_routes
                .iter()
                .map(|route| route.route.as_str())
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "base {base}  →  automation {automation}  +  modulation {} [{}]  →  effective {}{}",
                value.unit.format_value(value.modulation),
                if routes.is_empty() { "none" } else { &routes },
                value.unit.format_value(value.effective),
                if value.clamped { "  CLAMPED" } else { "" }
            )
        },
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!(
                "Lane {}  target {}  points {}  source {}:{}",
                lane.id,
                lane.target,
                lane.points.len(),
                lane.location.file.display(),
                lane.location.line
            )),
            Line::from(format!("frame {frame_position}  {provenance}")),
        ])
        .block(
            Block::default()
                .title(" Effective Parameter Provenance ")
                .borders(Borders::ALL),
        ),
        sections[0],
    );
    let rows = lane.points.iter().map(|point| {
        Row::new(vec![
            Cell::from(point.id.clone()),
            Cell::from(point.at.to_string()),
            Cell::from(point.value.clone()),
            Cell::from(point.interpolation.to_string()),
            Cell::from(format!(
                "{}:{}",
                point.location.file.display(),
                point.location.line
            )),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(14),
            Constraint::Length(16),
            Constraint::Length(14),
            Constraint::Min(18),
        ],
    )
    .header(Row::new(["Point", "Song time", "Physical value", "Curve out", "Source"]))
    .block(
        Block::default()
            .title(format!(
                " Automation Points · lane {}/{} · [ ] lane  ↑↓ point  ←→ value  Enter exact  t time  c curve ",
                app.selected_automation + 1,
                project.song.automations.len()
            ))
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default();
    if !lane.points.is_empty() {
        state.select(Some(app.selected_automation_point));
    }
    frame.render_stateful_widget(table, sections[1], &mut state);
}

fn render_graph(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some(project) = app.bundle.project.as_ref() else {
        render_unavailable(frame, area, "Graph unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        render_unavailable(frame, area, "No Instrument");
        return;
    };
    let body = if let Some(graph) = &instrument.graph {
        let processing = app.player.is_some()
            && app.candidate_is_active()
            && app.telemetry.engine.active_voices > 0;
        let selected = app.selected_graph_node;
        let mut lines = vec![Line::from(Span::styled(
            format!(
                "NODES  {} / {} declared ceiling  ·  {}",
                graph.nodes.len(),
                graph.max_nodes,
                if processing { "processing" } else { "idle" }
            ),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        lines.extend(graph.nodes.iter().enumerate().map(|(index, node)| {
            let settings = node
                .settings
                .iter()
                .map(|(key, value)| format!("{key}={value:?}"))
                .collect::<Vec<_>>()
                .join("  ");
            let line = Line::from(vec![
                Span::styled(
                    if processing { " ● " } else { " ○ " },
                    Style::default().fg(if processing {
                        Color::Green
                    } else {
                        Color::DarkGray
                    }),
                ),
                Span::styled(
                    format!("{:<16}", node.id),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("{:<12}", format!("{:?}", node.kind).to_lowercase()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(format!(
                    " {settings}  · {}:{}",
                    node.location.file.display(),
                    node.location.line
                )),
            ]);
            if index == selected {
                line.style(selected_style())
            } else {
                line
            }
        }));

        let connection_offset = graph.nodes.len();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "BUSES",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.extend(
            graph
                .connections
                .iter()
                .enumerate()
                .map(|(index, connection)| {
                    let line = Line::from(format!(
                        "  {}.{}  ──▶  {}.{}  · {}:{}",
                        connection.from.node,
                        connection.from.port,
                        connection.to.node,
                        connection.to.port,
                        connection.location.file.display(),
                        connection.location.line
                    ));
                    if connection_offset + index == selected {
                        line.style(selected_style())
                    } else {
                        line
                    }
                }),
        );

        let parameter_offset = connection_offset + graph.connections.len();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "PARAMETERS",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.extend(
            instrument
                .parameters
                .iter()
                .enumerate()
                .map(|(index, parameter)| {
                    let line = Line::from(format!(
                        "  {:<16} {} = {}  [{} … {}]  · {}:{}",
                        parameter.key,
                        parameter.unit,
                        parameter.unit.format_value(parameter.value),
                        parameter.unit.format_value(parameter.minimum),
                        parameter.unit.format_value(parameter.maximum),
                        parameter.location.file.display(),
                        parameter.location.line
                    ));
                    if parameter_offset + index == selected {
                        line.style(selected_style())
                    } else {
                        line
                    }
                }),
        );

        let faceplate_offset = parameter_offset + instrument.parameters.len();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "FACEPLATE MAPPINGS",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.extend(
            instrument
                .faceplate
                .iter()
                .enumerate()
                .map(|(index, control)| {
                    let line = Line::from(format!(
                        "  {:<16} {}  ──▶  ${}  · {}:{}",
                        control.key,
                        control.label,
                        control.parameter,
                        control.location.file.display(),
                        control.location.line
                    ));
                    if faceplate_offset + index == selected {
                        line.style(selected_style())
                    } else {
                        line
                    }
                }),
        );
        Text::from(lines)
    } else {
        Text::from(vec![
            Line::from("Stock synthesized percussion"),
            Line::from("kick · snare · hat · clap"),
            Line::from("No sample assets and no editable topology"),
        ])
    };
    let selected_row = instrument.graph.as_ref().map_or(0, |graph| {
        let selected = app.selected_graph_node;
        if selected < graph.nodes.len() {
            1 + selected
        } else if selected < graph.nodes.len() + graph.connections.len() {
            3 + selected
        } else if selected
            < graph.nodes.len() + graph.connections.len() + instrument.parameters.len()
        {
            5 + selected
        } else {
            7 + selected
        }
    });
    let scroll = selected_row
        .saturating_sub(usize::from(area.height.saturating_sub(4)) / 2)
        .min(u16::MAX as usize) as u16;
    frame.render_widget(
        Paragraph::new(body)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(
                Block::default()
                    .title(format!(
                        " Inspectable Graph · {} · ↑/↓ Item · ←/→ Instrument · Enter Source ",
                        instrument.name
                    ))
                    .borders(Borders::ALL),
            ),
        area,
    );
}

fn render_source(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let Some((path, source)) = app.bundle.sources.iter().nth(app.selected_source) else {
        render_unavailable(frame, area, "No source file");
        return;
    };
    let height = area.height.saturating_sub(2) as usize;
    let lines = source
        .lines()
        .enumerate()
        .skip(app.source_scroll)
        .take(height)
        .map(|(index, line)| {
            Line::from(vec![
                Span::styled(
                    format!("{:>5} │ ", index + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(line.to_owned()),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(format!(
                    " Authoritative Source · {} · ←/→ file · scroll {} ",
                    path.display(),
                    app.source_scroll + 1
                ))
                .borders(Borders::ALL),
        ),
        area,
    );
    let line_count = source.lines().count();
    let mut scrollbar =
        ScrollbarState::new(line_count.saturating_sub(height)).position(app.source_scroll);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut scrollbar,
    );
}

fn render_diagnostics(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let diagnostics = app.bundle.diagnostics.as_slice();
    let items = if diagnostics.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "✓ Candidate has no diagnostics and may publish",
            Style::default().fg(Color::Green),
        )))]
    } else {
        diagnostics
            .iter()
            .map(|diagnostic| {
                let location = diagnostic.location.as_ref().map_or_else(
                    || "".to_owned(),
                    |location| {
                        format!(
                            "{}:{}:{}  ",
                            location.file.display(),
                            location.line,
                            location.column
                        )
                    },
                );
                let color = if diagnostic.severity == Severity::Error {
                    Color::Red
                } else {
                    Color::Yellow
                };
                let mut lines = vec![Line::from(vec![
                    Span::styled(
                        format!("{:?} {}  ", diagnostic.severity, diagnostic.code),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(location),
                    Span::raw(diagnostic.message.clone()),
                ])];
                if let Some(help) = &diagnostic.help {
                    lines.push(Line::from(vec![
                        Span::styled("  help: ", Style::default().fg(Color::Cyan)),
                        Span::raw(help.clone()),
                    ]));
                }
                ListItem::new(lines)
            })
            .collect()
    };
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .title(format!(
                    " Candidate Diagnostics · {} finding(s) ",
                    diagnostics.len()
                ))
                .borders(Borders::ALL),
        ),
        area,
    );
}

fn render_history(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let rows = app.history.iter().map(|entry| {
        let active = if app.active_entry.as_ref() == Some(&entry.id) {
            "●"
        } else {
            " "
        };
        Row::new(vec![
            active.to_owned(),
            entry.id.clone(),
            format!("{:?}", entry.origin.kind).to_lowercase(),
            entry.origin.name.clone(),
            entry.summary.clone(),
            entry.changes.len().to_string(),
            entry.created_at.clone(),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(2),
            Constraint::Length(14),
            Constraint::Length(8),
            Constraint::Length(14),
            Constraint::Min(24),
            Constraint::Length(8),
            Constraint::Length(24),
        ],
    )
    .header(
        Row::new([
            "",
            "Entry",
            "Origin",
            "Actor",
            "Summary",
            "Files",
            "Created UTC",
        ])
        .style(Style::default().fg(Color::Cyan)),
    )
    .block(
        Block::default()
            .title(" Append-only Project History · Enter/d diff · R restore ")
            .borders(Borders::ALL),
    )
    .row_highlight_style(selected_style())
    .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.selected_history));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    let help = vec![
        Line::from(Span::styled(
            "WORKSPACE",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("1–9/0 views · H History · ? Help · Tab / Shift-Tab next/previous · ↑/↓ select"),
        Line::from(
            "Space start/pause/resume exact Active Revision · S stop · P inspect/publish Candidate",
        ),
        Line::from(""),
        Line::from(Span::styled(
            "PRODUCTION VIEWS",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(
            "Timeline: ←/→ level ±1 dB · Enter/l exact level · p exact pan · A Track audition",
        ),
        Line::from(
            "Processing: A target · a path · F exact range · L loop · b transient/B durable bypass",
        ),
        Line::from(
            "Processing: Enter exact setting · K/J move processor earlier/later · meters show D/T/E",
        ),
        Line::from(
            "Event Grid: A Pattern/sample audition · t onset · u duration · v velocity · L loop",
        ),
        Line::from("Sample occurrence: x region · f fade · g gain · p pan"),
        Line::from("Faceplate / Parameters: ←/→ bounded step · Enter exact physical value"),
        Line::from("Automation: [/] lane · ↑/↓ point · ←/→ value · Enter exact · t time · c curve"),
        Line::from(
            "Graph: ↑/↓ Node/Bus/Parameter/Mapping · ←/→ Instrument · Enter Source location",
        ),
        Line::from("Source: ←/→ file · ↑/↓ scroll · e open $VISUAL/$EDITOR"),
        Line::from("History: Enter/d exact source diff · R exact restore as a new entry"),
        Line::from(""),
        Line::from(Span::styled(
            "NOTE AUDITION",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("In Instrument views: A W S E D F T G Y H U J K plays C4–C5 transiently."),
        Line::from(
            "PipeWire MIDI Note On/Off targets the selected Instrument. Disconnect releases all notes.",
        ),
        Line::from(
            "Transient bypass and audition selection never edit source; B/settings/reorder are historical edits.",
        ),
        Line::from(""),
        Line::from(Span::styled(
            "SAFETY",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(
            "Invalid Candidate source is never made audible. The previous Active Revision continues.",
        ),
        Line::from(
            "Every Production View edit is a checked minimal source replacement and atomic publication.",
        ),
        Line::from(
            "External Agents use readable .crump files plus `crump --json` commands; no model is embedded.",
        ),
    ];
    frame.render_widget(
        Paragraph::new(help).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Keyboard-complete Help ")
                .borders(Borders::ALL),
        ),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, app: &Workspace, area: Rect) {
    let ratio = app.telemetry.engine.current_peak.clamp(0.0, 1.0) as f64;
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(24)])
        .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().bg(Color::Cyan)),
            Span::styled(
                format!(" {} ", app.status),
                Style::default().fg(Color::White),
            ),
        ]))
        .block(Block::default().borders(Borders::TOP)),
        horizontal[0],
    );
    frame.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .title(" MONITOR PEAK ")
                    .borders(Borders::TOP),
            )
            .gauge_style(Style::default().fg(if ratio > 0.95 {
                Color::Red
            } else {
                Color::Green
            }))
            .ratio(ratio)
            .label(format!("{:.3}", app.telemetry.engine.current_peak)),
        horizontal[1],
    );
}

fn render_modal(frame: &mut Frame<'_>, modal: &Modal, area: Rect) {
    let popup = centered_rect(76, 50, area);
    frame.render_widget(Clear, popup);
    match modal {
        Modal::Input {
            title,
            prompt,
            value,
            ..
        } => {
            let body = Text::from(vec![
                Line::from(prompt.clone()),
                Line::from(""),
                Line::from(Span::styled(
                    value.clone(),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("Enter apply · Esc cancel · Backspace edit"),
            ]);
            frame.render_widget(
                Paragraph::new(body).wrap(Wrap { trim: false }).block(
                    Block::default()
                        .title(format!(" {title} "))
                        .borders(Borders::ALL)
                        .style(Style::default().bg(Color::Black)),
                ),
                popup,
            );
        }
        Modal::ConfirmRestore { entry_id } => {
            frame.render_widget(
                Paragraph::new(format!(
                    "Restore exact state from {entry_id}?\n\nCurrent source will be replaced atomically and the restored state appended as a new History Entry.\n\ny / Enter restore · n / Esc cancel"
                ))
                .wrap(Wrap { trim: true })
                .block(Block::default().title(" Confirm History Restore ").borders(Borders::ALL).style(Style::default().bg(Color::Black))),
                popup,
            );
        }
        Modal::Text {
            title,
            body,
            scroll,
        } => {
            frame.render_widget(
                Paragraph::new(body.as_str())
                    .scroll((*scroll, 0))
                    .wrap(Wrap { trim: false })
                    .block(
                        Block::default()
                            .title(format!(" {title} · Esc close "))
                            .borders(Borders::ALL)
                            .style(Style::default().bg(Color::Black)),
                    ),
                popup,
            );
        }
    }
}

fn render_unavailable(frame: &mut Frame<'_>, area: Rect, message: &str) {
    frame.render_widget(
        Paragraph::new(message)
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn selected_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn audition_target_spec(target: &AuditionTarget) -> String {
    match target {
        AuditionTarget::Song => "song".to_owned(),
        AuditionTarget::Track { track } => format!("track/{track}"),
        AuditionTarget::Clip { clip, track: None } => format!("clip/{clip}"),
        AuditionTarget::Clip {
            clip,
            track: Some(track),
        } => format!("clip/{track}/{clip}"),
        AuditionTarget::Pattern {
            pattern,
            track: None,
        } => format!("pattern/{pattern}"),
        AuditionTarget::Pattern {
            pattern,
            track: Some(track),
        } => format!("pattern/{pattern}/{track}"),
        AuditionTarget::Asset {
            asset,
            start_frame: None,
            end_frame: None,
        } => format!("asset/{asset}"),
        AuditionTarget::Asset {
            asset,
            start_frame,
            end_frame,
        } => format!(
            "asset/{asset}/{}/{}",
            start_frame.map_or_else(|| "*".to_owned(), |frame| frame.to_string()),
            end_frame.map_or_else(|| "*".to_owned(), |frame| frame.to_string())
        ),
        AuditionTarget::Mapping {
            instrument,
            mapping,
            track: None,
        } => format!("mapping/{instrument}/{mapping}"),
        AuditionTarget::Mapping {
            instrument,
            mapping,
            track: Some(track),
        } => format!("mapping/{instrument}/{mapping}/{track}"),
        AuditionTarget::Occurrence {
            pattern,
            occurrence,
            track: None,
        } => format!("occurrence/{pattern}/{occurrence}"),
        AuditionTarget::Occurrence {
            pattern,
            occurrence,
            track: Some(track),
        } => format!("occurrence/{pattern}/{occurrence}/{track}"),
    }
}

fn parse_audition_target(value: &str) -> Result<AuditionTarget, String> {
    let parts = value.split('/').collect::<Vec<_>>();
    let target = match parts.as_slice() {
        ["song"] => AuditionTarget::Song,
        ["track", track] if !track.is_empty() => AuditionTarget::Track {
            track: (*track).to_owned(),
        },
        ["clip", clip] if !clip.is_empty() => AuditionTarget::Clip {
            clip: (*clip).to_owned(),
            track: None,
        },
        ["clip", track, clip] if !track.is_empty() && !clip.is_empty() => AuditionTarget::Clip {
            clip: (*clip).to_owned(),
            track: Some((*track).to_owned()),
        },
        ["pattern", pattern] if !pattern.is_empty() => AuditionTarget::Pattern {
            pattern: (*pattern).to_owned(),
            track: None,
        },
        ["pattern", pattern, track] if !pattern.is_empty() && !track.is_empty() => {
            AuditionTarget::Pattern {
                pattern: (*pattern).to_owned(),
                track: Some((*track).to_owned()),
            }
        }
        ["asset", asset] if !asset.is_empty() => AuditionTarget::Asset {
            asset: (*asset).to_owned(),
            start_frame: None,
            end_frame: None,
        },
        ["asset", asset, start, end] if !asset.is_empty() => {
            let parse_bound = |bound: &str| -> Result<Option<u64>, String> {
                if bound == "*" {
                    Ok(None)
                } else {
                    bound.parse::<u64>().map(Some).map_err(|_| {
                        format!("Asset source-frame bound `{bound}` is not an unsigned integer")
                    })
                }
            };
            let start_frame = parse_bound(start)?;
            let end_frame = parse_bound(end)?;
            if let (Some(start), Some(end)) = (start_frame, end_frame)
                && start >= end
            {
                return Err("Asset source-frame start must precede its exclusive end".to_owned());
            }
            AuditionTarget::Asset {
                asset: (*asset).to_owned(),
                start_frame,
                end_frame,
            }
        }
        ["mapping", instrument, mapping] if !instrument.is_empty() && !mapping.is_empty() => {
            AuditionTarget::Mapping {
                instrument: (*instrument).to_owned(),
                mapping: (*mapping).to_owned(),
                track: None,
            }
        }
        ["mapping", instrument, mapping, track]
            if !instrument.is_empty() && !mapping.is_empty() && !track.is_empty() =>
        {
            AuditionTarget::Mapping {
                instrument: (*instrument).to_owned(),
                mapping: (*mapping).to_owned(),
                track: Some((*track).to_owned()),
            }
        }
        ["occurrence", pattern, occurrence] if !pattern.is_empty() && !occurrence.is_empty() => {
            AuditionTarget::Occurrence {
                pattern: (*pattern).to_owned(),
                occurrence: (*occurrence).to_owned(),
                track: None,
            }
        }
        ["occurrence", pattern, occurrence, track]
            if !pattern.is_empty() && !occurrence.is_empty() && !track.is_empty() =>
        {
            AuditionTarget::Occurrence {
                pattern: (*pattern).to_owned(),
                occurrence: (*occurrence).to_owned(),
                track: Some((*track).to_owned()),
            }
        }
        _ => {
            return Err("Invalid audition target; use song, track/ID, clip/[TRACK/]ID, pattern/ID[/TRACK], asset/ID[/START/END], mapping/INSTRUMENT/MAPPING[/TRACK], or occurrence/PATTERN/ID[/TRACK]".to_owned());
        }
    };
    Ok(target)
}

fn default_audition_path(target: &AuditionTarget) -> AuditionPath {
    match target {
        AuditionTarget::Asset { .. } => AuditionPath::Raw,
        AuditionTarget::Mapping { .. } | AuditionTarget::Occurrence { .. } => AuditionPath::Mapping,
        _ => AuditionPath::Master,
    }
}

fn project_processor<'a>(
    project: &'a Project,
    processor_id: &str,
) -> Option<&'a crate::model::Processor> {
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
        .find(|processor| processor.id == processor_id)
}

fn candidate_processing_graph(bundle: &ProjectBundle) -> Option<MixGraphInspection> {
    let project = bundle.publishable()?;
    CompiledProject::compile_with_assets(project, &bundle.assets)
        .ok()
        .map(|compiled| compiled.mix_graph_inspection().clone())
}

fn mixer_channel_identity(project: &Project, target: &ParameterTarget) -> Option<String> {
    match target {
        ParameterTarget::Track { track, .. } => Some(format!("track/{track}")),
        ParameterTarget::Group { group, .. } => Some(format!("group/{group}")),
        ParameterTarget::Return { r#return, .. } => {
            Some(format!("return/{return}", r#return = r#return))
        }
        ParameterTarget::Send { send } => project
            .song
            .tracks
            .iter()
            .find(|track| {
                track
                    .channel
                    .sends
                    .iter()
                    .any(|candidate| candidate.id == *send)
            })
            .map(|track| format!("track/{}", track.id))
            .or_else(|| {
                project
                    .song
                    .groups
                    .iter()
                    .find(|group| {
                        group
                            .channel
                            .sends
                            .iter()
                            .any(|candidate| candidate.id == *send)
                    })
                    .map(|group| format!("group/{}", group.id))
            })
            .or_else(|| {
                project
                    .song
                    .returns
                    .iter()
                    .find(|channel| {
                        channel
                            .channel
                            .sends
                            .iter()
                            .any(|candidate| candidate.id == *send)
                    })
                    .map(|channel| format!("return/{}", channel.id))
            }),
        ParameterTarget::Processor { processor, .. } => project
            .song
            .tracks
            .iter()
            .find(|track| {
                track
                    .channel
                    .inserts
                    .iter()
                    .any(|candidate| candidate.id == *processor)
            })
            .map(|track| format!("track/{}", track.id))
            .or_else(|| {
                project
                    .song
                    .groups
                    .iter()
                    .find(|group| {
                        group
                            .channel
                            .inserts
                            .iter()
                            .any(|candidate| candidate.id == *processor)
                    })
                    .map(|group| format!("group/{}", group.id))
            })
            .or_else(|| {
                project
                    .song
                    .returns
                    .iter()
                    .find(|channel| {
                        channel
                            .channel
                            .inserts
                            .iter()
                            .any(|candidate| candidate.id == *processor)
                    })
                    .map(|channel| format!("return/{}", channel.id))
            })
            .or_else(|| {
                project
                    .song
                    .master
                    .as_ref()
                    .filter(|master| {
                        master
                            .inserts
                            .iter()
                            .any(|candidate| candidate.id == *processor)
                    })
                    .map(|_| "master".to_owned())
            }),
        ParameterTarget::Master => Some("master".to_owned()),
        ParameterTarget::Instrument { .. } => None,
    }
}

fn mixer_mute_target(identity: &str) -> Option<ParameterTarget> {
    if let Some(track) = identity.strip_prefix("track/") {
        return Some(ParameterTarget::Track {
            track: track.to_owned(),
            parameter: crate::model::TrackParameter::Mute,
        });
    }
    if let Some(group) = identity.strip_prefix("group/") {
        return Some(ParameterTarget::Group {
            group: group.to_owned(),
            parameter: crate::model::TrackParameter::Mute,
        });
    }
    identity
        .strip_prefix("return/")
        .map(|r#return| ParameterTarget::Return {
            r#return: r#return.to_owned(),
            parameter: crate::model::TrackParameter::Mute,
        })
}

fn transport_quarter_notes(app: &Workspace) -> f64 {
    let Some(project) = app.active_project.as_ref() else {
        return 0.0;
    };
    app.telemetry.engine.frame as f64 * project.song.tempo_bpm
        / (f64::from(project.sample_rate) * 60.0)
}

fn transport_label(app: &Workspace) -> String {
    let Some(project) = app.active_project.as_ref() else {
        return "--:--".to_owned();
    };
    let quarter_notes = transport_quarter_notes(app);
    let bar_length = project
        .song
        .meter
        .bar_length()
        .map_or(4.0, crate::time::MusicalTime::as_f64);
    let bar = (quarter_notes / bar_length).floor() as u64 + 1;
    let beat = quarter_notes.rem_euclid(bar_length) + 1.0;
    format!("{bar:02}:{beat:04.1}")
}

fn event_is_active(app: &Workspace, pattern_id: &str, event: &PatternEvent) -> bool {
    if app.player.is_none() || !app.candidate_is_active() {
        return false;
    }
    let Some(project) = app.bundle.project.as_ref() else {
        return false;
    };
    let Some(pattern) = project
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == pattern_id)
    else {
        return false;
    };
    let position = transport_quarter_notes(app);
    project.song.tracks.iter().any(|track| {
        track.clips.iter().any(|clip| {
            if clip.pattern != pattern_id {
                return false;
            }
            let start = clip.start.as_f64();
            let end = start + clip.duration.as_f64();
            if !(start..end).contains(&position) {
                return false;
            }
            let local = position - start;
            let onset = event.at().as_f64();
            let activity_duration = match event {
                PatternEvent::Note { duration, .. } | PatternEvent::Sample { duration, .. } => {
                    duration.as_f64()
                }
                PatternEvent::Hit { .. } => 0.125,
            };
            match clip.mode {
                crate::model::ClipMode::Once => (onset..onset + activity_duration).contains(&local),
                crate::model::ClipMode::Loop => {
                    let span = pattern.span.as_f64();
                    if local < onset || span <= 0.0 {
                        return false;
                    }
                    let iteration = ((local - onset) / span).floor();
                    let instance_onset = iteration * span + onset;
                    (instance_onset..instance_onset + activity_duration).contains(&local)
                }
            }
        })
    })
}

fn keyboard_pitch(character: char) -> Option<u8> {
    match character.to_ascii_lowercase() {
        'a' => Some(60),
        'w' => Some(61),
        's' => Some(62),
        'e' => Some(63),
        'd' => Some(64),
        'f' => Some(65),
        't' => Some(66),
        'g' => Some(67),
        'y' => Some(68),
        'h' => Some(69),
        'u' => Some(70),
        'j' => Some(71),
        'k' => Some(72),
        _ => None,
    }
}

fn parse_frame_pair(value: &str) -> Result<(u64, u64), String> {
    let (start, end) = value
        .split_once(':')
        .ok_or_else(|| "Source region must use START:END".to_owned())?;
    let start = start
        .trim()
        .parse::<u64>()
        .map_err(|_| "Source-region start must be an unsigned frame".to_owned())?;
    let end = end
        .trim()
        .parse::<u64>()
        .map_err(|_| "Source-region end must be an unsigned frame".to_owned())?;
    Ok((start, end))
}

fn parse_fade_pair(value: &str) -> Result<(u32, u32), String> {
    let (fade_in, fade_out) = value
        .split_once(':')
        .ok_or_else(|| "Source fades must use IN:OUT".to_owned())?;
    let fade_in = fade_in
        .trim()
        .parse::<u32>()
        .map_err(|_| "Fade-in must be an unsigned frame count".to_owned())?;
    let fade_out = fade_out
        .trim()
        .parse::<u32>()
        .map_err(|_| "Fade-out must be an unsigned frame count".to_owned())?;
    Ok((fade_in, fade_out))
}

fn wrap_index(current: usize, count: usize, delta: isize) -> usize {
    if count == 0 {
        return 0;
    }
    (current as isize + delta).rem_euclid(count as isize) as usize
}

fn wrap_selection(current: usize, count: usize, delta: isize) -> usize {
    wrap_index(current.min(count.saturating_sub(1)), count, delta)
}

fn clamp_index(current: usize, count: usize) -> usize {
    current.min(count.saturating_sub(1))
}

fn short_revision(revision: &RevisionId) -> &str {
    revision.0.get(..8).unwrap_or(revision.0.as_str())
}

fn short_optional(revision: Option<&RevisionId>) -> &str {
    revision.map_or("none", short_revision)
}

/// Workspace startup or terminal failures.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    /// Terminal setup, event, or drawing failed.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Project source could not be loaded.
    #[error(transparent)]
    Project(#[from] crate::project::ProjectIoError),
    /// Project History could not initialize.
    #[error(transparent)]
    History(#[from] crate::history::HistoryError),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn exact_automation_entry_preserves_value_and_curve_precision() {
        let root = tempfile::tempdir().expect("temporary Project");
        let source = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/minimal.crump"
        ))
        .expect("read fixture")
        .replace(
            "    track lead \"Lead\" instrument lead level -9db pan -0.1 {",
            r#"    automation lead-level target track/lead/level {
      point start 0bar -18db linear
      point lift 2beat -6db step
    }
    track lead "Lead" instrument lead level -9db pan -0.1 {"#,
        );
        fs::write(root.path().join("project.crump"), source).expect("write fixture");
        let mut workspace = Workspace::load(root.path()).expect("load workspace");

        workspace.commit_input(
            "-6.123456789db".to_owned(),
            InputAction::AutomationValue {
                lane: "lead-level".to_owned(),
                point: "lift".to_owned(),
            },
        );
        workspace.commit_input(
            "power(0.0000004)".to_owned(),
            InputAction::AutomationCurve {
                lane: "lead-level".to_owned(),
                point: "lift".to_owned(),
            },
        );

        let source = fs::read_to_string(root.path().join("project.crump"))
            .expect("read exact Automation source");
        assert!(source.contains("point lift 2beat -6.123456789db power(0.0000004)"));
    }

    #[test]
    fn mixer_exposes_source_backed_controls_for_every_routing_scope() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("project.crump"),
            include_str!("../tests/fixtures/production_mix.crump"),
        )
        .unwrap();
        let mut workspace = Workspace::load(directory.path()).unwrap();
        workspace.view = View::Mixer;
        let parameters = workspace
            .bundle
            .project
            .as_ref()
            .expect("Project")
            .mix_parameter_specs();
        for expected in [
            "track/bass/left_pan",
            "track/drums/mute",
            "group/mix_bus/level",
            "group/mix_bus/mute",
            "return/room/level",
            "return/room/mute",
            "send/drum_room/level",
            "processor/bass_comp/threshold",
            "master/level",
        ] {
            assert!(
                parameters
                    .iter()
                    .any(|(target, _)| target.to_string() == expected),
                "{expected}"
            );
        }
        let project = workspace.bundle.project.as_ref().expect("Project");
        assert_eq!(
            mixer_channel_identity(
                project,
                &"send/drum_room/level".parse().expect("Send target")
            )
            .as_deref(),
            Some("track/drums")
        );
        assert_eq!(
            mixer_channel_identity(
                project,
                &"processor/bass_comp/threshold"
                    .parse()
                    .expect("processor target")
            )
            .as_deref(),
            Some("track/bass")
        );
        assert_eq!(
            mixer_mute_target("return/room")
                .expect("mute target")
                .to_string(),
            "return/room/mute"
        );

        workspace.selected_mixer_parameter = parameters
            .iter()
            .position(|(target, _)| *target == ParameterTarget::Master)
            .expect("Master target");
        workspace.open_exact_edit();
        assert!(matches!(
            workspace.modal,
            Some(Modal::Input {
                action: InputAction::Target {
                    target: ParameterTarget::Master,
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn audition_target_text_round_trips_every_workspace_target() {
        let targets = [
            AuditionTarget::Song,
            AuditionTarget::Track {
                track: "drums".to_owned(),
            },
            AuditionTarget::Clip {
                clip: "verse".to_owned(),
                track: None,
            },
            AuditionTarget::Clip {
                clip: "verse".to_owned(),
                track: Some("drums".to_owned()),
            },
            AuditionTarget::Pattern {
                pattern: "beat".to_owned(),
                track: Some("drums".to_owned()),
            },
            AuditionTarget::Asset {
                asset: "snare".to_owned(),
                start_frame: Some(12),
                end_frame: Some(240),
            },
            AuditionTarget::Mapping {
                instrument: "kit".to_owned(),
                mapping: "snare".to_owned(),
                track: Some("drums".to_owned()),
            },
            AuditionTarget::Occurrence {
                pattern: "beat".to_owned(),
                occurrence: "snare-2".to_owned(),
                track: None,
            },
        ];
        for target in targets {
            let spec = audition_target_spec(&target);
            assert_eq!(parse_audition_target(&spec), Ok(target), "{spec}");
        }
        assert!(parse_audition_target("asset/snare/20/10").is_err());
    }

    #[test]
    fn processing_view_exposes_source_settings_and_transient_bypass_without_editing() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("project.crump"),
            include_str!("../tests/fixtures/production_mix.crump"),
        )
        .unwrap();
        let mut workspace = Workspace::load(directory.path()).unwrap();
        workspace.view = View::Processing;
        let source_before = fs::read_to_string(directory.path().join("project.crump")).unwrap();
        let processor = workspace
            .selected_processor_inspection()
            .expect("compiled processor")
            .id
            .clone();

        workspace.toggle_transient_processor_bypass();
        assert!(workspace.audition_plan.transient_bypass.is_empty());
        assert!(workspace.status.contains("Publish the Candidate"));
        workspace.commit_input("Initial publication".to_owned(), InputAction::Publish);
        assert!(workspace.candidate_is_active());

        workspace.open_exact_edit();
        assert!(matches!(
            workspace.modal,
            Some(Modal::Input {
                action: InputAction::ProcessorSetting { .. },
                ..
            })
        ));
        workspace.modal = None;
        workspace.toggle_transient_processor_bypass();
        assert!(
            workspace
                .audition_plan
                .transient_bypass
                .contains_key(&processor)
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("project.crump")).unwrap(),
            source_before
        );
        workspace.toggle_transient_processor_bypass();
        assert!(
            !workspace
                .audition_plan
                .transient_bypass
                .contains_key(&processor)
        );
    }

    #[test]
    fn every_graph_item_class_is_selectable_and_source_navigable() {
        let root = tempfile::tempdir().expect("temporary Project");
        fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/minimal.crump"),
            root.path().join("project.crump"),
        )
        .expect("copy fixture");
        let mut workspace = Workspace::load(root.path()).expect("load workspace");
        workspace.view = View::Graph;

        let instrument = &workspace
            .bundle
            .project
            .as_ref()
            .expect("parsed Project")
            .song
            .instruments[0];
        let graph = instrument.graph.as_ref().expect("graph Instrument");
        let node_count = graph.nodes.len();
        let connection_count = graph.connections.len();
        let parameter_count = instrument.parameters.len();
        let faceplate_count = instrument.faceplate.len();
        assert_eq!(
            workspace.graph_node_count(),
            node_count + connection_count + parameter_count + faceplate_count
        );

        for (index, expected) in [
            (0, "Graph Node"),
            (node_count, "Bus"),
            (node_count + connection_count, "Parameter"),
            (
                node_count + connection_count + parameter_count,
                "Faceplate mapping",
            ),
        ] {
            workspace.view = View::Graph;
            workspace.selected_graph_node = index;
            workspace.open_graph_source();
            assert_eq!(workspace.view, View::Source);
            assert!(workspace.status.contains(expected), "{}", workspace.status);
            assert_eq!(workspace.selected_source, 0);
        }
    }
}
