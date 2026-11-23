//! Production workspace over the terminal-independent Project model.
//!
//! [`Workspace`] is the client-independent session: it owns Candidate and Active Revision
//! tracking, background reload, playback, audition, selection, and modal state, and it changes
//! only through [`Workspace::dispatch`] and [`Workspace::tick`]. A client adapter translates its
//! own input into [`Action`] values and draws the resulting state. The first-party adapter is the
//! Ratatui terminal client in the `tui` submodule, compiled with the `tui` feature; the optional
//! egui desktop client is the `gui` submodule behind the `gui` feature. Nothing outside those
//! submodules depends on a terminal or windowing library, and both share the key bindings in
//! the `keymap` submodule.

// The read side of the session (projections, labels, timing statistics) is consumed only by a
// client adapter, so a build without one would otherwise report it as dead.
#![cfg_attr(not(feature = "tui"), allow(dead_code))]

use std::{
    cell::RefCell,
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    diagnostic::SourceLocation,
    engine::audition::{
        AuditionCue, AuditionPath, AuditionPlan, AuditionRange, AuditionStatus, AuditionTarget,
    },
    engine::{CompiledProject, control::ControlInspector, mix_graph::MixGraphInspection},
    history::{HistoryEntry, HistoryStore},
    model::{
        AutomationLane, AutomationPoint, DrumVoice, Graph, InstrumentKind, ParameterSpec,
        ParameterTarget, PatternEvent, PresentationItemKind, Project, TapPoint, Unit,
    },
    production_view::{
        MoveDirection, Operation as ProductionViewOperation, PhysicalValue,
        Provenance as ProductionViewProvenance, Request as ProductionViewRequest,
        execute as execute_production_view,
    },
    project::{ProjectBundle, RevisionId},
    realtime::{PlaybackBackend, Player, PlayerTelemetry, RealtimeError, StreamStatus},
};

const AUDITION_TIMEOUT: Duration = Duration::from_millis(700);
const VIEWS: [View; 13] = [
    View::Timeline,
    View::Mixer,
    View::Processing,
    View::EventGrid,
    View::Faceplate,
    View::Instrument,
    View::Parameters,
    View::Automation,
    View::Graph,
    View::Source,
    View::Diagnostics,
    View::History,
    View::Help,
];

mod action;
mod active;
mod background;
#[cfg(feature = "gui")]
mod gui;
mod input;
mod instrument;
#[cfg(any(feature = "tui", feature = "gui"))]
mod keymap;
mod performance;
mod reload;
mod timeline;
#[cfg(feature = "tui")]
mod tui;
mod watch;

#[cfg(test)]
use crate::project::load_project;

pub use action::{Action, TimelineAction};
use active::{
    ActiveMode, ActiveRequest, ActiveRequestKey, ActiveResult, PreparedActiveInspection,
    PreparedActiveMode,
};
use background::{BackgroundPoll, BackgroundWorker, RetireQueue};
#[cfg(feature = "gui")]
pub use gui::run as run_gui;
use input::{parse_fade_pair, parse_frame_pair};
#[cfg(test)]
use instrument::instrument_section_views;
use instrument::{InstrumentSectionRow, instrument_section_views_with_assets};
use performance::WorkspacePerformance;
use reload::{CandidateReload, CandidateUpdate, HistoryReload, HistoryUpdate, ReloadBatch};
use timeline::{TimelineCache, TimelinePosition, TimelineRange, TimelineState, TimelineViewport};
#[cfg(feature = "tui")]
pub use tui::run;
use watch::WorkspaceWatch;

fn timeline_ascii_fallback() -> bool {
    if let Ok(value) = std::env::var("CRUMP_TIMELINE_ASCII") {
        return matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        );
    }
    let locale_is_ascii = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .is_some_and(|locale| matches!(locale.as_str(), "C" | "POSIX"));
    locale_is_ascii || std::env::var("TERM").is_ok_and(|term| term.eq_ignore_ascii_case("dumb"))
}

/// Graph View order: the order the runtime processes Nodes in. A graph that has no valid
/// order (only possible for an unpublished candidate) falls back to declaration order.
fn graph_processing_node_indexes(
    instrument: &crate::model::Instrument,
    graph: &Graph,
) -> Vec<usize> {
    crate::graph_schedule::SameFrameDependencies::of(instrument, graph)
        .order()
        .unwrap_or_else(|_| (0..graph.nodes.len()).collect())
}

fn graph_processing_bus_indexes(graph: &Graph, node_order: &[usize]) -> Vec<usize> {
    let mut order = Vec::with_capacity(graph.connections.len());
    let mut included = vec![false; graph.connections.len()];
    for node_index in node_order {
        let node_id = graph.nodes[*node_index].id.as_str();
        let mut incoming = graph
            .connections
            .iter()
            .enumerate()
            .filter(|(_, connection)| connection.to.node == node_id)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        incoming.sort_by(|left, right| {
            graph.connections[*left]
                .id
                .cmp(&graph.connections[*right].id)
        });
        for index in incoming {
            included[index] = true;
            order.push(index);
        }
    }
    let mut remainder = included
        .iter()
        .enumerate()
        .filter_map(|(index, included)| (!included).then_some(index))
        .collect::<Vec<_>>();
    remainder.sort_by(|left, right| {
        graph.connections[*left]
            .id
            .cmp(&graph.connections[*right].id)
    });
    order.extend(remainder);
    order
}

/// One Production View or inspection surface a client can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Arrangement of Tracks and Clips along the Song.
    Timeline,
    /// Level, pan, mute, Solo, and cue for every routing scope.
    Mixer,
    /// Processor chains and audition paths of the mix graph.
    Processing,
    /// Events of the selected Pattern.
    EventGrid,
    /// Authored Faceplate controls of the selected Instrument.
    Faceplate,
    /// Generated sections of the selected Instrument.
    Instrument,
    /// Every Parameter of the selected Instrument.
    Parameters,
    /// Automation lanes and their points.
    Automation,
    /// Nodes and buses of the selected Instrument graph.
    Graph,
    /// Authoritative Source text.
    Source,
    /// Diagnostics of the Candidate Revision.
    Diagnostics,
    /// Project History entries.
    History,
    /// Client help.
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
            Self::Instrument => "Instrument",
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

#[derive(Debug, Clone)]
struct MixerParameterView {
    target: ParameterTarget,
    spec: ParameterSpec,
    channel_identity: Option<String>,
    muted: bool,
}

enum WorkspaceWorkResult {
    Probe(WatchProbe),
    Reload(ReloadBatch),
    Active(ActiveResult, Duration),
    Mutation(MutationResult, Duration),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkspaceWorkKind {
    Probe,
    CandidateVerification,
    HistoryVerification,
    Reload {
        candidate: bool,
        history: bool,
    },
    Active {
        key: ActiveRequestKey,
        explicit_seek: bool,
        initial_frame: Option<u64>,
    },
    Mutation,
}

struct WatchProbe {
    candidate_dirty: bool,
    history_dirty: bool,
    elapsed: Duration,
}

struct PendingMutation {
    request: ProductionViewRequest,
    success: String,
    failure: &'static str,
}

struct MutationResult {
    result: Result<String, String>,
    success: String,
    failure: &'static str,
}

struct PendingActiveInspection {
    key: ActiveRequestKey,
    inspection: PreparedActiveInspection,
}

/// Client-independent workspace session for one Project.
///
/// Create it with [`Workspace::open`], feed it [`Action`] values through
/// [`Workspace::dispatch`], and call [`Workspace::tick`] regularly so audio telemetry and
/// background work are observed.
pub struct Workspace {
    root: PathBuf,
    bundle: ProjectBundle,
    watch: WorkspaceWatch,
    worker: BackgroundWorker<WorkspaceWorkResult>,
    retire_queue: RetireQueue,
    watch_probe_requested: bool,
    candidate_reload_requested: bool,
    history_reload_requested: bool,
    candidate_verification: Option<Box<CandidateUpdate>>,
    history_verification: Option<Box<HistoryUpdate>>,
    work_in_flight: Option<WorkspaceWorkKind>,
    pending_mutation: Option<PendingMutation>,
    mutation_in_flight: bool,
    active_prepare_requested: bool,
    active_prepare_explicit_seek: bool,
    active_prepare_initial_frame: Option<u64>,
    active_build_in_flight: bool,
    audition_epoch: u64,
    playback_epoch: u64,
    next_publication_generation: u64,
    playback_requested: bool,
    performance: WorkspacePerformance,
    active_revision: Option<RevisionId>,
    active_entry: Option<String>,
    active_bundle: Option<Arc<ProjectBundle>>,
    audible_bundle: Option<Arc<ProjectBundle>>,
    pending_active_inspection: Option<PendingActiveInspection>,
    history: Vec<HistoryEntry>,
    view: View,
    selected_track: usize,
    timeline_state: TimelineState,
    timeline_viewport: TimelineViewport,
    timeline_cache: TimelineCache,
    timeline_ascii: bool,
    mixer_parameters: Vec<MixerParameterView>,
    selected_mixer_parameter: usize,
    selected_processor: usize,
    selected_pattern: usize,
    selected_event: usize,
    selected_instrument: usize,
    selected_instrument_section: usize,
    selected_instrument_row: usize,
    selected_parameter: usize,
    selected_automation: usize,
    selected_automation_point: usize,
    selected_graph_node: usize,
    selected_history: usize,
    selected_diagnostic: usize,
    help_scroll: u16,
    help_line_count: usize,
    selected_source: usize,
    source_scroll: usize,
    modal: Option<Modal>,
    status: String,
    quit: bool,
    player: Option<Player>,
    playback_backend: PlaybackBackend,
    playback_paused: bool,
    telemetry: PlayerTelemetry,
    mix_channel_indexes: BTreeMap<String, usize>,
    candidate_processing_graph: Option<MixGraphInspection>,
    audition_processing_graph: Option<MixGraphInspection>,
    candidate_control_inspector: Option<RefCell<ControlInspector>>,
    audition_control_inspector: Option<RefCell<ControlInspector>>,
    instrument_section_cache: Vec<Vec<instrument::InstrumentSectionView>>,
    audition_plan: AuditionPlan,
    audition_status: Option<AuditionStatus>,
    held_notes: BTreeMap<char, HeldNote>,
    next_note_id: u64,
    note_mode: bool,
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
    },
    Parameter {
        instrument: String,
        parameter: String,
    },
    InstrumentRouteDepth {
        instrument: String,
        route: String,
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
    TimelineGoto,
    TimelineSearch,
    TimelineLocator {
        at: String,
    },
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
    #[cfg(test)]
    fn load(path: &Path) -> Result<Self, WorkspaceError> {
        let mut performance = WorkspacePerformance::from_environment();
        let started = Instant::now();
        let bundle = load_project(path)?;
        performance.observe_candidate_load(started.elapsed());
        let started = Instant::now();
        let history = history_state(&bundle.root)?;
        performance.observe_history_load(started.elapsed());
        Self::from_state(
            bundle,
            history,
            performance,
            true,
            "Ready · ? opens keyboard help",
        )
    }

    fn begin_load(path: &Path) -> Result<Self, WorkspaceError> {
        let root = crate::project::project_root(path)?;
        let placeholder = ProjectBundle {
            root,
            project: None,
            sources: BTreeMap::new(),
            assets: crate::media::AssetInventory::default(),
            revision: RevisionId(String::new()),
            diagnostics: crate::diagnostic::Diagnostics::new(),
        };
        let mut workspace = Self::from_state(
            placeholder,
            (None, None, Vec::new(), None),
            WorkspacePerformance::from_environment(),
            false,
            "Loading exact Candidate and History in the background",
        )?;
        workspace.queue_reload(true, true);
        workspace.status = "Loading exact Candidate and History in the background".to_owned();
        Ok(workspace)
    }

    fn from_state(
        bundle: ProjectBundle,
        history_state: HistorySnapshot,
        mut performance: WorkspacePerformance,
        measure_derived: bool,
        status: &str,
    ) -> Result<Self, WorkspaceError> {
        let (active_entry, active_revision, history, active_bundle) = history_state;
        let root = bundle.root.clone();
        let started = Instant::now();
        let (candidate_processing_graph, candidate_control_inspector) =
            candidate_runtime_inspection(&bundle);
        let candidate_control_inspector = candidate_control_inspector.map(RefCell::new);
        let instrument_section_cache = candidate_instrument_sections(&bundle);
        let mixer_parameters = candidate_mixer_parameters(&bundle);
        let mut timeline_cache = TimelineCache::default();
        timeline_cache.rebuild(bundle.publishable(), &bundle.assets);
        if measure_derived {
            performance.observe_derived_rebuild(started.elapsed());
        }
        let watch = if measure_derived {
            WorkspaceWatch::capture(&root, &bundle)
        } else {
            WorkspaceWatch::seed(&root, &bundle)
        };
        let worker = BackgroundWorker::spawn("crump-workspace-builder")?;
        let retire_queue = RetireQueue::spawn("crump-workspace-reclaimer")?;
        let mut workspace = Self {
            root,
            bundle,
            watch,
            worker,
            retire_queue,
            watch_probe_requested: false,
            candidate_reload_requested: false,
            history_reload_requested: false,
            candidate_verification: None,
            history_verification: None,
            work_in_flight: None,
            pending_mutation: None,
            mutation_in_flight: false,
            active_prepare_requested: false,
            active_prepare_explicit_seek: false,
            active_prepare_initial_frame: None,
            active_build_in_flight: false,
            audition_epoch: 0,
            playback_epoch: 0,
            next_publication_generation: 0,
            playback_requested: false,
            performance,
            active_revision,
            active_entry,
            active_bundle,
            audible_bundle: None,
            pending_active_inspection: None,
            history,
            view: View::Timeline,
            selected_track: 0,
            timeline_state: TimelineState::default(),
            timeline_viewport: TimelineViewport::default(),
            timeline_cache,
            timeline_ascii: timeline_ascii_fallback(),
            mixer_parameters,
            selected_mixer_parameter: 0,
            selected_processor: 0,
            selected_pattern: 0,
            selected_event: 0,
            selected_instrument: 0,
            selected_instrument_section: 0,
            selected_instrument_row: 0,
            selected_parameter: 0,
            selected_automation: 0,
            selected_automation_point: 0,
            selected_graph_node: 0,
            selected_history: 0,
            selected_diagnostic: 0,
            help_scroll: 0,
            help_line_count: 0,
            selected_source: 0,
            source_scroll: 0,
            modal: None,
            status: status.to_owned(),
            quit: false,
            player: None,
            playback_backend: PlaybackBackend::default(),
            playback_paused: false,
            telemetry: PlayerTelemetry::default(),
            mix_channel_indexes: BTreeMap::new(),
            candidate_processing_graph,
            audition_processing_graph: None,
            candidate_control_inspector,
            audition_control_inspector: None,
            instrument_section_cache,
            audition_plan: AuditionPlan::default(),
            audition_status: None,
            held_notes: BTreeMap::new(),
            next_note_id: 1 << 32,
            note_mode: false,
            audition_loop: None,
            editor_requested: false,
        };
        workspace.clamp_selections();
        Ok(workspace)
    }

    fn refresh(&mut self) -> bool {
        let queued = self.queue_reload(true, true);
        if queued {
            self.status = "Exact Project and History reload queued".to_owned();
        }
        queued
    }

    fn refresh_if_changed(&mut self) -> bool {
        if self.watch_probe_requested {
            self.performance.coalesced_requests =
                self.performance.coalesced_requests.saturating_add(1);
        }
        self.watch_probe_requested = true;
        self.dispatch_work();
        self.performance.visible()
    }

    fn queue_reload(&mut self, candidate: bool, history: bool) -> bool {
        if candidate {
            if self.candidate_reload_requested {
                self.performance.coalesced_requests =
                    self.performance.coalesced_requests.saturating_add(1);
            }
            self.candidate_reload_requested = true;
        }
        if history {
            if self.history_reload_requested {
                self.performance.coalesced_requests =
                    self.performance.coalesced_requests.saturating_add(1);
            }
            self.history_reload_requested = true;
        }
        let queued = candidate || history;
        self.dispatch_work();
        queued || self.performance.visible()
    }

    fn queue_active_preparation(&mut self, explicit_seek: bool) {
        self.queue_active_preparation_with_frame(explicit_seek, None);
    }

    fn queue_active_preparation_with_frame(
        &mut self,
        explicit_seek: bool,
        initial_frame: Option<u64>,
    ) {
        if self.active_prepare_requested {
            self.performance.coalesced_requests =
                self.performance.coalesced_requests.saturating_add(1);
        }
        self.active_prepare_requested = true;
        self.active_prepare_explicit_seek |= explicit_seek || initial_frame.is_some();
        if initial_frame.is_some() {
            self.active_prepare_initial_frame = initial_frame;
        }
        self.dispatch_work();
    }

    fn dispatch_work(&mut self) {
        self.retire_queue.flush();
        if self.worker.busy() || self.retire_queue.at_capacity() {
            return;
        }
        if let Some(update) = self.candidate_verification.take() {
            match self.worker.submit(move || {
                WorkspaceWorkResult::Reload(ReloadBatch {
                    candidate: Some(reload::verify_candidate(update)),
                    history: None,
                })
            }) {
                Ok(()) => self.work_in_flight = Some(WorkspaceWorkKind::CandidateVerification),
                Err(error) => {
                    self.candidate_reload_requested = true;
                    self.status = format!("Could not verify Candidate reload: {error}");
                }
            }
            return;
        }
        if let Some(update) = self.history_verification.take() {
            match self.worker.submit(move || {
                WorkspaceWorkResult::Reload(ReloadBatch {
                    candidate: None,
                    history: Some(reload::verify_history(update)),
                })
            }) {
                Ok(()) => self.work_in_flight = Some(WorkspaceWorkKind::HistoryVerification),
                Err(error) => {
                    self.history_reload_requested = true;
                    self.status = format!("Could not verify History reload: {error}");
                }
            }
            return;
        }
        if self.try_dispatch_active() {
            return;
        }
        if let Some(mutation) = self.pending_mutation.take() {
            let root = self.root.clone();
            match self.worker.submit(move || {
                let started = Instant::now();
                let result = execute_production_view(root, mutation.request)
                    .map(|outcome| outcome.history_entry.id)
                    .map_err(|error| error.to_string());
                WorkspaceWorkResult::Mutation(
                    MutationResult {
                        result,
                        success: mutation.success,
                        failure: mutation.failure,
                    },
                    started.elapsed(),
                )
            }) {
                Ok(()) => {
                    self.mutation_in_flight = true;
                    self.work_in_flight = Some(WorkspaceWorkKind::Mutation);
                }
                Err(error) => {
                    self.status = format!("Could not start Project mutation: {error}");
                }
            }
            return;
        }
        if self.candidate_reload_requested || self.history_reload_requested {
            let candidate_requested = self.candidate_reload_requested;
            let history_requested = self.history_reload_requested;
            let candidate =
                candidate_requested.then(|| (self.bundle.revision.clone(), self.watch.clone()));
            let history = history_requested.then(|| {
                (
                    self.active_revision.clone(),
                    self.active_bundle.is_some(),
                    self.watch.clone(),
                )
            });
            let root = self.root.clone();
            match self.worker.submit(move || {
                WorkspaceWorkResult::Reload(ReloadBatch {
                    candidate: candidate.map(|(revision, watch)| {
                        let before = watch.recapture_candidate();
                        reload::load_candidate(root.clone(), revision, before)
                    }),
                    history: history.map(|(revision, has_bundle, watch)| {
                        let before = watch.recapture_history();
                        reload::load_history(root, revision, has_bundle, before)
                    }),
                })
            }) {
                Ok(()) => {
                    self.candidate_reload_requested = false;
                    self.history_reload_requested = false;
                    self.work_in_flight = Some(WorkspaceWorkKind::Reload {
                        candidate: candidate_requested,
                        history: history_requested,
                    });
                }
                Err(error) => {
                    self.status = format!("Could not start background reload: {error}");
                }
            }
            return;
        }
        if !self.watch_probe_requested {
            return;
        }
        let watch = self.watch.clone();
        match self.worker.submit(move || {
            let started = Instant::now();
            let candidate = watch.recapture_candidate();
            let history = watch.recapture_history();
            WorkspaceWorkResult::Probe(WatchProbe {
                candidate_dirty: !watch.same_candidate(&candidate),
                history_dirty: !watch.same_history(&history),
                elapsed: started.elapsed(),
            })
        }) {
            Ok(()) => {
                self.watch_probe_requested = false;
                self.work_in_flight = Some(WorkspaceWorkKind::Probe);
            }
            Err(error) => self.status = format!("Could not start workspace probe: {error}"),
        }
    }

    fn try_dispatch_active(&mut self) -> bool {
        if !self.active_prepare_requested {
            return false;
        }
        let Some(active) = self.active_bundle.clone() else {
            if self.history_reload_requested {
                return false;
            }
            self.active_prepare_requested = false;
            self.status = "Publish a valid Candidate before preparing an audition".to_owned();
            return false;
        };
        if self.active_revision.as_ref() != Some(&active.revision) {
            if self.history_reload_requested {
                return false;
            }
            self.active_prepare_requested = false;
            self.status = "Cached Active Revision is stale; reload History and retry".to_owned();
            return false;
        }

        let mode = if let Some(player) = self.player.as_mut() {
            match player.reserve_publication() {
                Ok(reservation) => ActiveMode::Replace { reservation },
                Err(RealtimeError::QueueFull) => return false,
                Err(error) => {
                    self.active_prepare_requested = false;
                    self.status = format!("Runtime publication unavailable: {error}");
                    return false;
                }
            }
        } else if self.playback_requested {
            ActiveMode::Start
        } else {
            ActiveMode::Arm
        };
        self.next_publication_generation = self.next_publication_generation.saturating_add(1);
        let key = ActiveRequestKey {
            active_revision: active.revision.clone(),
            audition_epoch: self.audition_epoch,
            playback_epoch: self.playback_epoch,
            generation: self.next_publication_generation,
        };
        let explicit_seek = self.active_prepare_explicit_seek;
        let initial_frame = self.active_prepare_initial_frame;
        let request = ActiveRequest {
            key: key.clone(),
            active,
            plan: self.audition_plan.clone(),
            telemetry: self.telemetry.engine,
            previous_status: self.audition_status.clone(),
            initial_frame,
            explicit_seek,
            mode,
            backend: self.playback_backend,
        };
        match self.worker.submit(move || {
            let started = Instant::now();
            let result = active::prepare_active(request);
            WorkspaceWorkResult::Active(result, started.elapsed())
        }) {
            Ok(()) => {
                self.active_prepare_requested = false;
                self.active_prepare_explicit_seek = false;
                self.active_prepare_initial_frame = None;
                self.active_build_in_flight = true;
                self.work_in_flight = Some(WorkspaceWorkKind::Active {
                    key,
                    explicit_seek,
                    initial_frame,
                });
                true
            }
            Err(error) => {
                self.status = format!("Could not start Active preparation: {error}");
                false
            }
        }
    }

    fn poll_background_work(&mut self) -> bool {
        self.dispatch_work();
        match self.worker.poll() {
            BackgroundPoll::Pending => false,
            BackgroundPoll::Complete { result, ready_for } => {
                self.work_in_flight = None;
                self.performance.observe_background_commit_wait(ready_for);
                let started = Instant::now();
                let changed = match result {
                    WorkspaceWorkResult::Probe(probe) => {
                        self.performance.observe_refresh_probe(probe.elapsed);
                        self.candidate_reload_requested |= probe.candidate_dirty;
                        self.history_reload_requested |= probe.history_dirty;
                        self.performance.visible()
                    }
                    WorkspaceWorkResult::Reload(batch) => {
                        let mut changed = false;
                        if let Some(candidate) = batch.candidate {
                            changed |= self.finish_candidate_reload(candidate);
                        }
                        if let Some(history) = batch.history {
                            changed |= self.finish_history_reload(history);
                        }
                        changed
                    }
                    WorkspaceWorkResult::Active(result, elapsed) => {
                        self.performance.observe_active_preparation(elapsed);
                        self.active_build_in_flight = false;
                        self.finish_active_preparation(result)
                    }
                    WorkspaceWorkResult::Mutation(result, elapsed) => {
                        self.performance.observe_mutation(elapsed);
                        self.finish_mutation(result)
                    }
                };
                self.performance.observe_background_apply(started.elapsed());
                self.dispatch_work();
                changed
            }
            BackgroundPoll::Failed(error) => {
                self.recover_failed_work();
                self.status = format!("Workspace builder failed: {error}");
                true
            }
        }
    }

    fn recover_failed_work(&mut self) {
        match self.work_in_flight.take() {
            Some(WorkspaceWorkKind::Probe) => self.watch_probe_requested = true,
            Some(WorkspaceWorkKind::CandidateVerification) => {
                self.candidate_reload_requested = true;
            }
            Some(WorkspaceWorkKind::HistoryVerification) => {
                self.history_reload_requested = true;
            }
            Some(WorkspaceWorkKind::Reload { candidate, history }) => {
                self.candidate_reload_requested |= candidate;
                self.history_reload_requested |= history;
            }
            Some(WorkspaceWorkKind::Active {
                key,
                explicit_seek,
                initial_frame,
            }) => {
                self.active_build_in_flight = false;
                if self.active_request_is_current(&key) {
                    self.active_prepare_requested = true;
                    self.active_prepare_explicit_seek |= explicit_seek;
                    if initial_frame.is_some() {
                        self.active_prepare_initial_frame = initial_frame;
                    }
                }
            }
            Some(WorkspaceWorkKind::Mutation) => {
                self.mutation_in_flight = false;
                self.candidate_reload_requested = true;
                self.history_reload_requested = true;
            }
            None => {}
        }
    }

    fn finish_durable_mutation_before_shutdown(&mut self) {
        while self.pending_mutation.is_some() || self.mutation_in_flight {
            self.watch_probe_requested = false;
            self.candidate_reload_requested = false;
            self.history_reload_requested = false;
            self.active_prepare_requested = false;
            if let Some(update) = self.candidate_verification.take() {
                self.retire_queue.retire(update);
            }
            if let Some(update) = self.history_verification.take() {
                self.retire_queue.retire(update);
            }
            self.poll_background_work();
            std::thread::yield_now();
        }
    }

    fn queue_mutation(
        &mut self,
        request: ProductionViewRequest,
        success: impl Into<String>,
        failure: &'static str,
    ) {
        if self.pending_mutation.is_some() || self.mutation_in_flight {
            self.status =
                "A Project mutation is already in flight; wait for its exact reload".to_owned();
            return;
        }
        self.pending_mutation = Some(PendingMutation {
            request,
            success: success.into(),
            failure,
        });
        self.dispatch_work();
        self.status = "Project mutation queued".to_owned();
    }

    fn finish_mutation(&mut self, result: MutationResult) -> bool {
        self.mutation_in_flight = false;
        match result.result {
            Ok(entry) => {
                self.status = format!("{} {entry}", result.success);
                self.queue_reload(true, true);
            }
            Err(error) => self.status = format!("{}: {error}", result.failure),
        }
        true
    }

    fn finish_active_preparation(&mut self, result: ActiveResult) -> bool {
        match result {
            ActiveResult::Failed { key, error } => {
                if self.active_request_is_current(&key) {
                    if self.player.is_none() && self.playback_requested {
                        self.playback_requested = false;
                    }
                    self.status = format!(
                        "Active audition preparation failed; prior audible state retained: {error}"
                    );
                }
                true
            }
            ActiveResult::Prepared {
                key,
                inspection,
                mode,
            } => {
                if !self.active_request_is_current(&key) {
                    self.retire_queue.retire((mode, inspection));
                    return true;
                }
                let inspection = *inspection;
                match *mode {
                    PreparedActiveMode::Armed => {
                        if self.player.is_none() && !self.playback_requested {
                            self.adopt_active_inspection(inspection);
                            self.status = "Audition target prepared".to_owned();
                        }
                    }
                    PreparedActiveMode::Started(mut player) => {
                        if !self.playback_requested || self.player.is_some() {
                            self.retire_queue.retire((player, inspection));
                            return true;
                        }
                        player.set_audition_track(self.audition_track());
                        self.playback_paused = false;
                        self.player = Some(*player);
                        if let Some(previous) = self
                            .pending_active_inspection
                            .replace(PendingActiveInspection { key, inspection })
                        {
                            self.retire_queue.retire(previous);
                        }
                        self.status =
                            "Audition starting · waiting for callback confirmation".to_owned();
                    }
                    PreparedActiveMode::Replacement {
                        runtime,
                        reservation,
                    } => {
                        let Some(player) = self.player.as_mut() else {
                            self.retire_queue.retire((runtime, reservation, inspection));
                            return true;
                        };
                        match player.publish_prepared(runtime, reservation, key.generation) {
                            Ok(()) => {
                                let range_moved = inspection.range_moved;
                                if let Some(previous) = self
                                    .pending_active_inspection
                                    .replace(PendingActiveInspection { key, inspection })
                                {
                                    self.retire_queue.retire(previous);
                                }
                                self.status = if range_moved {
                                    "Active audition queued · callback will map the moved range"
                                        .to_owned()
                                } else {
                                    "Active audition queued · callback-relative transport retained"
                                        .to_owned()
                                };
                            }
                            Err((RealtimeError::QueueFull, runtime, reservation)) => {
                                if inspection.requested_initial_frame.is_some() {
                                    self.active_prepare_initial_frame =
                                        inspection.requested_initial_frame;
                                }
                                self.active_prepare_explicit_seek |= inspection
                                    .requested_explicit_seek
                                    || inspection.requested_initial_frame.is_some();
                                self.retire_queue.retire((runtime, reservation, inspection));
                                self.active_prepare_requested = true;
                                self.status =
                                    "Runtime publication queue busy · newest intent retained"
                                        .to_owned();
                            }
                            Err((error, runtime, reservation)) => {
                                self.retire_queue.retire((runtime, reservation, inspection));
                                self.status = format!("Runtime publication failed: {error}");
                            }
                        }
                    }
                }
                true
            }
        }
    }

    fn active_request_is_current(&self, key: &ActiveRequestKey) -> bool {
        self.active_revision.as_ref() == Some(&key.active_revision)
            && self.audition_epoch == key.audition_epoch
            && self.playback_epoch == key.playback_epoch
    }

    fn adopt_active_inspection(&mut self, inspection: PreparedActiveInspection) {
        let retired = (
            std::mem::replace(
                &mut self.mix_channel_indexes,
                inspection.mix_channel_indexes,
            ),
            self.audible_bundle.replace(inspection.active),
            self.audition_processing_graph
                .replace(inspection.processing_graph),
            self.audition_control_inspector
                .replace(RefCell::new(inspection.control_inspector)),
            self.audition_status.replace(inspection.status),
        );
        self.retire_queue.retire(retired);
        self.clamp_selections();
    }

    fn finish_candidate_reload(&mut self, result: CandidateReload) -> bool {
        let update = match result {
            CandidateReload::Failed {
                error,
                load_elapsed,
            } => {
                self.performance.observe_candidate_load(load_elapsed);
                self.status = format!("Reload failed: {error}");
                return true;
            }
            CandidateReload::Loaded(update) => {
                self.performance.observe_candidate_load(update.load_elapsed);
                if let Some(elapsed) = update.derive_elapsed {
                    self.performance.observe_derived_rebuild(elapsed);
                }
                if !update.stable || self.candidate_reload_requested {
                    return self.retry_candidate_reload(update);
                }
                self.candidate_verification = Some(update);
                self.status =
                    "Candidate reconstructed · verifying latest filesystem generation".to_owned();
                return true;
            }
            CandidateReload::Verified(update) => update,
        };
        if !update.stable || self.candidate_reload_requested {
            return self.retry_candidate_reload(update);
        }

        let update = *update;
        let candidate_changed = update.bundle.revision != self.bundle.revision;
        let retired_derived = update.derived.map(|derived| {
            (
                std::mem::replace(
                    &mut self.instrument_section_cache,
                    derived.instrument_sections,
                ),
                std::mem::replace(
                    &mut self.candidate_control_inspector,
                    derived.control_inspector.map(RefCell::new),
                ),
                std::mem::replace(&mut self.mixer_parameters, derived.mixer_parameters),
                std::mem::replace(&mut self.timeline_cache, derived.timeline),
                std::mem::replace(
                    &mut self.candidate_processing_graph,
                    derived.processing_graph,
                ),
            )
        });
        let retired_bundle = std::mem::replace(&mut self.bundle, update.bundle);
        self.retire_queue.retire((retired_bundle, retired_derived));
        self.watch.adopt_candidate(update.watch);
        self.clamp_selections();
        if candidate_changed {
            self.performance.candidate_changes =
                self.performance.candidate_changes.saturating_add(1);
            if self.bundle.diagnostics.has_errors() {
                self.status =
                    "External Candidate is invalid; prior Active Revision remains audible"
                        .to_owned();
                self.view = View::Diagnostics;
            } else if self.candidate_is_active() {
                self.status = "Published revision loaded".to_owned();
            } else {
                self.status = "Valid external Candidate loaded · press P to publish".to_owned();
            }
        } else {
            self.status = "Candidate reload complete · no byte changes".to_owned();
        }
        true
    }

    fn retry_candidate_reload(&mut self, update: Box<CandidateUpdate>) -> bool {
        self.performance.unstable_retries = self.performance.unstable_retries.saturating_add(1);
        self.watch.adopt_candidate(update.watch.clone());
        self.candidate_reload_requested = true;
        self.retire_queue.retire(update);
        self.status = "Candidate changes still arriving · retaining prior stable state".to_owned();
        true
    }

    fn finish_history_reload(&mut self, result: HistoryReload) -> bool {
        let update = match result {
            HistoryReload::Failed {
                error,
                load_elapsed,
            } => {
                self.performance.observe_history_load(load_elapsed);
                self.status = format!("History refresh failed: {error}");
                return true;
            }
            HistoryReload::Loaded(update) => {
                self.performance.observe_history_load(update.load_elapsed);
                if !update.stable || self.history_reload_requested {
                    return self.retry_history_reload(update);
                }
                self.history_verification = Some(update);
                self.status =
                    "History reconstructed · verifying latest filesystem generation".to_owned();
                return true;
            }
            HistoryReload::Verified(update) => update,
        };
        if !update.stable || self.history_reload_requested {
            return self.retry_history_reload(update);
        }

        let update = *update;
        let active_changed = update.state.active_revision != self.active_revision;
        let retired_active = update
            .state
            .active_bundle_update
            .map(|active_bundle| std::mem::replace(&mut self.active_bundle, active_bundle));
        let retired_state = (
            std::mem::replace(&mut self.active_entry, update.state.active_entry),
            std::mem::replace(&mut self.active_revision, update.state.active_revision),
            std::mem::replace(&mut self.history, update.state.entries),
            retired_active,
        );
        self.retire_queue.retire(retired_state);
        self.watch.adopt_history(update.watch);
        self.clamp_selections();
        if active_changed {
            self.performance.active_changes = self.performance.active_changes.saturating_add(1);
            if self.player.is_some() || self.playback_requested || self.audition_status.is_some() {
                let revision = self.active_revision.clone();
                self.publish_runtime_for_active(revision.as_ref(), false);
            }
        }
        self.status = if active_changed {
            "Published revision loaded".to_owned()
        } else {
            "History reload complete · Active Revision unchanged".to_owned()
        };
        true
    }

    fn retry_history_reload(&mut self, update: Box<HistoryUpdate>) -> bool {
        self.performance.unstable_retries = self.performance.unstable_retries.saturating_add(1);
        self.watch.adopt_history(update.watch.clone());
        self.history_reload_requested = true;
        self.retire_queue.retire(update);
        self.status = "History changes still arriving · retaining prior stable state".to_owned();
        true
    }

    fn background_work_pending(&self) -> bool {
        self.worker.busy()
            || self.watch_probe_requested
            || self.candidate_reload_requested
            || self.history_reload_requested
            || self.candidate_verification.is_some()
            || self.history_verification.is_some()
            || self.active_prepare_requested
            || self.pending_mutation.is_some()
            || self.mutation_in_flight
            || self.retire_queue.pending()
    }

    fn audition_intent_changed(&mut self) {
        self.audition_epoch = self.audition_epoch.saturating_add(1);
        if let Some(status) = self.audition_status.as_mut() {
            status.soloed_channels = self.audition_plan.soloed_channels.clone();
            status.cue = self.audition_plan.cue.clone();
        }
        if let Some(pending) = self.pending_active_inspection.as_mut() {
            pending.inspection.status.soloed_channels = self.audition_plan.soloed_channels.clone();
            pending.inspection.status.cue = self.audition_plan.cue.clone();
        }
        if self.active_build_in_flight {
            self.queue_active_preparation(false);
        }
    }

    fn publish_runtime_for_active(&mut self, revision: Option<&RevisionId>, explicit_seek: bool) {
        let Some(revision) = revision else {
            return;
        };
        if self.active_revision.as_ref() != Some(revision) {
            self.status =
                "Active Revision changed before preparation; newest state retained".to_owned();
            return;
        }
        self.queue_active_preparation(explicit_seek);
        self.status = "Active audition preparation queued".to_owned();
    }

    fn poll_audio(&mut self) {
        let audition_track = self.audition_track();
        let (player_slot, retire_queue) = (&mut self.player, &mut self.retire_queue);
        if let Some(player) = player_slot.as_mut() {
            self.telemetry = player.poll_with_reclaimer(|runtime| retire_queue.retire(runtime));
            player.set_audition_track(audition_track);
        }
        if self.timeline_state.follow_playhead()
            && self.player.is_some()
            && let Some(project) = self
                .audible_bundle
                .as_ref()
                .and_then(|bundle| bundle.publishable())
            && let Ok(end_frame) = project
                .song
                .end
                .to_frames(project.song.tempo_bpm, project.sample_rate)
        {
            self.timeline_viewport
                .center_frame(self.telemetry.engine.frame, end_frame);
        }
        let failure = stream_failure_message(self.telemetry.stream_state);
        if let Some(status) = failure
            && self.player.is_some()
        {
            self.handle_stream_failure(status);
            return;
        }

        let promoted = self
            .pending_active_inspection
            .as_ref()
            .is_some_and(|pending| pending.key.generation == self.telemetry.publication_generation);
        if promoted && let Some(pending) = self.pending_active_inspection.take() {
            self.adopt_active_inspection(pending.inspection);
            self.status = "Active audition is now audible".to_owned();
        }
    }

    fn handle_stream_failure(&mut self, status: &str) {
        self.playback_epoch = self.playback_epoch.saturating_add(1);
        self.playback_requested = false;
        self.playback_paused = false;
        self.active_prepare_requested = false;
        self.active_prepare_explicit_seek = false;
        self.active_prepare_initial_frame = None;
        self.held_notes.clear();
        let retired = (
            self.player.take(),
            self.pending_active_inspection.take(),
            self.audible_bundle.take(),
            self.audition_processing_graph.take(),
            self.audition_control_inspector.take(),
            std::mem::take(&mut self.mix_channel_indexes),
        );
        self.retire_queue.retire(retired);
        self.status = status.to_owned();
    }

    fn toggle_mixer_solo(&mut self) {
        if !self.runtime_matches_candidate() {
            self.status = if self.player.is_some() {
                "Wait for the Candidate to become audible before changing transient Solo"
            } else {
                "Publish the Candidate before changing transient Solo"
            }
            .to_owned();
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
        self.audition_intent_changed();
        self.status = format!(
            "Solo {} for `{identity}` (transient monitoring only)",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    fn toggle_mixer_cue(&mut self, tap: TapPoint) {
        if !self.runtime_matches_candidate() {
            self.status = if self.player.is_some() {
                "Wait for the Candidate to become audible before changing transient cue"
            } else {
                "Publish the Candidate before changing transient cue"
            }
            .to_owned();
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
        self.audition_intent_changed();
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
        self.apply_operation(
            ProductionViewOperation::SetParameterTarget {
                target: target.to_string(),
                value: PhysicalValue::Number(value),
            },
            format!("Set {target} to {}", Unit::Boolean.format_value(value)),
        );
    }

    fn selected_processor_inspection(
        &self,
    ) -> Option<&crate::engine::mix_graph::CompiledProcessorInspection> {
        self.processing_graph()?
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
        if !self.runtime_matches_candidate() {
            self.status = if self.player.is_some() {
                "Wait for the Candidate to become audible before changing transient Active audition state"
            } else {
                "Publish the Candidate before changing transient Active audition state"
            }
            .to_owned();
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
        self.audition_intent_changed();
    }

    fn toggle_durable_processor_bypass(&mut self) {
        let Some(inspection) = self.selected_processor_inspection() else {
            self.status = "Select a processor before changing durable bypass".to_owned();
            return;
        };
        let processor = inspection.id.clone();
        let bypass = !inspection.authored_bypass;
        self.apply_operation(
            ProductionViewOperation::SetProcessorBypass {
                processor: processor.clone(),
                bypass,
            },
            format!("Set Processor {processor} durable bypass to {bypass}"),
        );
    }

    fn move_selected_processor(&mut self, direction: MoveDirection) {
        let Some(processor) = self
            .selected_processor_inspection()
            .map(|processor| processor.id.clone())
        else {
            self.status = "Select a processor before reordering the chain".to_owned();
            return;
        };
        self.apply_operation(
            ProductionViewOperation::MoveProcessor {
                processor: processor.clone(),
                direction,
            },
            format!("Move Processor {processor} {direction:?}"),
        );
    }

    fn activate_audition_plan(&mut self, plan: AuditionPlan) {
        self.activate_audition_plan_at(plan, None);
    }

    fn activate_audition_plan_at(&mut self, plan: AuditionPlan, initial_frame: Option<u64>) {
        if self.active_bundle.is_none() {
            self.status = "Publish a valid Candidate before selecting an audition".to_owned();
            return;
        }
        self.audition_loop = match (&plan.target, plan.range.looping) {
            (AuditionTarget::Pattern { pattern, .. }, true) => Some(pattern.clone()),
            _ => None,
        };
        self.audition_plan = plan;
        if initial_frame.is_some() {
            self.timeline_state.arm_cursor();
        } else {
            self.timeline_state.disarm_cursor();
        }
        self.audition_epoch = self.audition_epoch.saturating_add(1);
        self.active_prepare_initial_frame = None;
        self.queue_active_preparation_with_frame(true, initial_frame);
        self.status = "Audition target queued for exact Active preparation".to_owned();
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
        if self.playback_requested {
            self.playback_requested = false;
            self.playback_epoch = self.playback_epoch.saturating_add(1);
            self.active_prepare_requested = false;
            self.active_prepare_initial_frame = None;
            self.status = "Pending playback start cancelled".to_owned();
            return;
        }
        if self.active_bundle.is_none() {
            self.status = "Publish a valid Candidate before playback".to_owned();
            return;
        }
        let initial_frame = if self.timeline_state.cursor_armed() {
            match self.timeline_cursor_frame_for_plan() {
                Ok(frame) => Some(frame),
                Err(error) => {
                    self.status = error;
                    return;
                }
            }
        } else {
            None
        };
        self.playback_requested = true;
        self.playback_epoch = self.playback_epoch.saturating_add(1);
        self.queue_active_preparation_with_frame(true, initial_frame);
        self.status = "Preparing Active audition in the background · Space cancels".to_owned();
    }

    fn stop_playback(&mut self) {
        let player = self.player.take();
        if player.is_none() && !self.playback_requested {
            self.status = "Audition is already stopped".to_owned();
            return;
        }
        self.playback_requested = false;
        self.playback_epoch = self.playback_epoch.saturating_add(1);
        self.active_prepare_requested = false;
        self.active_prepare_explicit_seek = false;
        self.active_prepare_initial_frame = None;
        if let Some(pending) = self.pending_active_inspection.take() {
            self.retire_queue.retire(pending);
        }
        if let Some(player) = player {
            self.retire_queue.retire(player);
        }
        self.playback_paused = false;
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

        self.audition_epoch = self.audition_epoch.saturating_add(1);
        self.queue_active_preparation(true);
        self.status = if let Some(pattern) = self.audition_loop.as_deref() {
            format!("Pattern audition `{pattern}` queued with an exact loop")
        } else {
            "Audition loop disable queued".to_owned()
        };
    }

    fn request_quit(&mut self) {
        if self.pending_mutation.is_some() || self.mutation_in_flight {
            self.status = "Finishing the durable Project mutation before shutdown".to_owned();
        }
        self.quit = true;
    }

    fn cancel_workspace_mode(&mut self) {
        if self.note_mode {
            if let Some(player) = self.player.as_mut() {
                let _ = player.all_notes_off();
            }
            self.held_notes.clear();
            self.note_mode = false;
            self.status = "NORMAL mode".to_owned();
        } else if self.timeline_state.visual() {
            self.cancel_timeline_visual();
        } else {
            self.status = "NORMAL mode · Ctrl-C exits".to_owned();
        }
    }

    fn move_selection_extreme(&mut self, first: bool) {
        let edge = |count: usize| if first { 0 } else { count.saturating_sub(1) };
        match self.view {
            View::Timeline => {
                self.selected_track = edge(self.track_count());
                self.timeline_state.set_track(self.selected_track);
                self.sync_instrument_to_track();
            }
            View::Mixer => self.selected_mixer_parameter = edge(self.mixer_parameter_count()),
            View::Processing => {
                let count = self
                    .processing_graph()
                    .map_or(0, |graph| graph.processors.len());
                self.selected_processor = edge(count);
            }
            View::EventGrid => self.selected_event = edge(self.event_count()),
            View::Faceplate => self.selected_parameter = edge(self.faceplate_count()),
            View::Instrument => self.move_instrument_selection_extreme(first),
            View::Parameters => self.selected_parameter = edge(self.parameter_count()),
            View::Automation => {
                self.selected_automation_point = edge(self.automation_point_count())
            }
            View::Graph => self.selected_graph_node = edge(self.graph_node_count()),
            View::Source => self.source_scroll = edge(self.source_line_count()),
            View::Diagnostics => {
                self.selected_diagnostic = edge(self.bundle.diagnostics.as_slice().len())
            }
            View::History => self.selected_history = edge(self.history.len()),
            View::Help => {
                self.help_scroll = u16::try_from(edge(self.help_line_count)).unwrap_or(u16::MAX)
            }
        }
        self.status = if first {
            "Moved to first row"
        } else {
            "Moved to last row"
        }
        .to_owned();
    }

    fn pan_timeline_window(&mut self, direction: isize) {
        self.status = if self.timeline_viewport.pan_window(direction) {
            "Timeline panned by one viewport".to_owned()
        } else {
            "Timeline viewport reached the Song boundary".to_owned()
        };
    }

    fn zoom_timeline(&mut self, direction: isize) {
        let changed = if direction > 0 {
            self.timeline_viewport.zoom_in()
        } else {
            self.timeline_viewport.zoom_out()
        };
        self.status = if changed {
            format!(
                "Timeline zoom {}× · ,/. pan · Home fit",
                self.timeline_viewport.zoom_factor()
            )
        } else if direction > 0 {
            "Timeline is at maximum zoom".to_owned()
        } else {
            "Timeline already fits the complete Song".to_owned()
        };
    }

    fn pan_timeline(&mut self, direction: isize) {
        self.status = if self.timeline_viewport.pan(direction) {
            format!(
                "Timeline panned · {}× zoom · Home fits the complete Song",
                self.timeline_viewport.zoom_factor()
            )
        } else if self.timeline_viewport.is_fit() {
            "Zoom the Timeline with + before panning".to_owned()
        } else {
            "Timeline viewport reached the Song boundary".to_owned()
        };
    }

    fn fit_timeline(&mut self) {
        self.timeline_viewport.fit();
        self.status = "Timeline fitted to the complete Song".to_owned();
    }

    fn timeline_position(&self) -> TimelinePosition {
        self.timeline_state.position()
    }

    fn set_timeline_position(
        &mut self,
        mut position: TimelinePosition,
        record_jump: bool,
        seek: bool,
        description: &str,
    ) {
        let Some((song_end, meter, track_count)) = self.bundle.project.as_ref().map(|project| {
            (
                project.song.end,
                project.song.meter,
                project.song.tracks.len(),
            )
        }) else {
            self.status =
                "Timeline navigation is unavailable while the Candidate is invalid".to_owned();
            return;
        };
        position.at = position.at.clamp(crate::time::MusicalTime::ZERO, song_end);
        position.track = position.track.min(track_count.saturating_sub(1));
        if record_jump {
            self.timeline_state.jump_to(position);
        } else {
            self.timeline_state.set_position(position);
        }
        self.selected_track = position.track;
        self.timeline_viewport.reveal(position.at, song_end);
        self.sync_instrument_to_track();
        if seek && !self.timeline_state.visual() {
            self.timeline_state.arm_cursor();
            let _ = self.queue_timeline_seek();
            return;
        }
        self.status = format!(
            "{description} · cursor {}",
            timeline_time_label(position.at, meter)
        );
    }

    fn timeline_frame_at(&self, at: crate::time::MusicalTime) -> Option<u64> {
        let project = self.active_bundle.as_ref()?.publishable()?;
        let frame = at
            .to_frames(project.song.tempo_bpm, project.sample_rate)
            .ok()?;
        let song_end = project
            .song
            .end
            .to_frames(project.song.tempo_bpm, project.sample_rate)
            .ok()?;
        Some(frame.min(song_end.saturating_sub(1)))
    }

    fn timeline_cursor_frame_for_plan(&self) -> Result<u64, String> {
        let frame = self
            .timeline_frame_at(self.timeline_state.position().at)
            .ok_or_else(|| {
                "The exact Active frame for the Timeline cursor is unavailable".to_owned()
            })?;
        if !self.active_build_in_flight
            && !self.active_prepare_requested
            && self.pending_active_inspection.is_none()
            && let Some(status) = self.audition_status.as_ref()
            && status.target == self.audition_plan.target
            && status.path == self.audition_plan.path
            && (frame < status.start_frame || frame >= status.end_frame)
        {
            return Err(format!(
                "Timeline cursor frame {frame} lies outside the armed range [{}, {}); select a containing range before seeking",
                status.start_frame, status.end_frame
            ));
        }
        Ok(frame)
    }

    fn queue_timeline_seek(&mut self) -> bool {
        if !self.candidate_is_active() {
            self.status = "Cursor moved; publish the Candidate before seeking playback".to_owned();
            return false;
        }
        let frame = match self.timeline_cursor_frame_for_plan() {
            Ok(frame) => frame,
            Err(error) => {
                self.status = format!("Cursor moved without seeking: {error}");
                return false;
            }
        };
        self.audition_epoch = self.audition_epoch.saturating_add(1);
        self.queue_active_preparation_with_frame(true, Some(frame));
        self.status = if self.playback_paused {
            "Exact seek queued; paused playback adopts it when the callback resumes".to_owned()
        } else if self.player.is_some() {
            "Exact Timeline seek queued".to_owned()
        } else {
            "Timeline cursor armed as the next playback start".to_owned()
        };
        true
    }

    fn move_timeline_grid(&mut self, direction: isize) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let position = self.timeline_position();
        let grid = self.timeline_state.grid(project);
        let at = if direction < 0 {
            position
                .at
                .checked_sub(grid)
                .unwrap_or(crate::time::MusicalTime::ZERO)
                .max(crate::time::MusicalTime::ZERO)
        } else {
            position
                .at
                .checked_add(grid)
                .unwrap_or(project.song.end)
                .min(project.song.end)
        };
        self.set_timeline_position(
            TimelinePosition { at, ..position },
            false,
            true,
            "Timeline cursor moved",
        );
    }

    fn move_timeline_boundary(&mut self, direction: isize) {
        let position = self.timeline_position();
        let Some(at) = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| timeline::adjacent_boundary(project, position, direction))
        else {
            self.status = "No further Clip or Locator boundary".to_owned();
            return;
        };
        self.set_timeline_position(
            TimelinePosition { at, ..position },
            false,
            true,
            "Moved to arrangement boundary",
        );
    }

    fn move_timeline_locator(&mut self, direction: isize) {
        let position = self.timeline_position();
        let Some(at) = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| timeline::adjacent_locator(project, position, direction))
        else {
            self.status = "No further Locator".to_owned();
            return;
        };
        self.set_timeline_position(
            TimelinePosition { at, ..position },
            true,
            true,
            "Moved to Locator",
        );
    }

    fn move_timeline_clip_end(&mut self) {
        let position = self.timeline_position();
        let Some(at) = self.bundle.project.as_ref().and_then(|project| {
            timeline::clip_at(project, position)
                .and_then(|(_, clip)| clip.start.checked_add(clip.duration).ok())
        }) else {
            self.status = "No Clip lies under the Timeline cursor".to_owned();
            return;
        };
        self.set_timeline_position(
            TimelinePosition { at, ..position },
            false,
            true,
            "Moved to Clip end",
        );
    }

    fn toggle_timeline_clip_edge(&mut self) {
        let position = self.timeline_position();
        let Some((start, end)) = self.bundle.project.as_ref().and_then(|project| {
            timeline::clip_at(project, position).and_then(|(_, clip)| {
                clip.start
                    .checked_add(clip.duration)
                    .ok()
                    .map(|end| (clip.start, end))
            })
        }) else {
            self.status = "No Clip lies under the Timeline cursor".to_owned();
            return;
        };
        let at = if position.at == start { end } else { start };
        self.set_timeline_position(
            TimelinePosition { at, ..position },
            false,
            true,
            "Moved to matching Clip edge",
        );
    }

    fn move_timeline_track_edge(&mut self, end: bool) {
        let position = self.timeline_position();
        let Some(at) = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| timeline::track_material_edge(project, position.track, end))
        else {
            self.status = "Selected Track has no arranged material".to_owned();
            return;
        };
        self.set_timeline_position(
            TimelinePosition { at, ..position },
            true,
            true,
            if end {
                "Moved to last material on Track"
            } else {
                "Moved to first material on Track"
            },
        );
    }

    fn move_timeline_matching_pattern(&mut self, direction: isize) {
        let position = self.timeline_position();
        let Some(next) = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| timeline::matching_pattern_clip(project, position, direction))
        else {
            self.status = "No further Clip uses the Pattern under the cursor".to_owned();
            return;
        };
        self.set_timeline_position(next, true, true, "Moved to matching Pattern Clip");
    }

    fn move_timeline_song_edge(&mut self, end: bool) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let position = TimelinePosition {
            at: if end {
                project.song.end
            } else {
                crate::time::MusicalTime::ZERO
            },
            ..self.timeline_position()
        };
        self.set_timeline_position(
            position,
            true,
            true,
            if end {
                "Moved to Song end"
            } else {
                "Moved to Song start"
            },
        );
    }

    fn change_timeline_grid(&mut self, direction: isize) {
        if self.timeline_state.change_grid(direction) {
            self.status = format!(
                "Timeline navigation grid {}",
                self.timeline_state.grid_label()
            );
        } else {
            self.status = format!(
                "Timeline grid already at {}",
                self.timeline_state.grid_label()
            );
        }
    }

    fn begin_timeline_visual(&mut self) {
        self.timeline_state.begin_visual();
        self.status =
            "RANGE mode · move an edge, L loops, Enter plays once, Esc cancels".to_owned();
    }

    fn cancel_timeline_visual(&mut self) {
        self.timeline_state.cancel_visual();
        self.status = "Timeline RANGE selection cancelled".to_owned();
    }

    fn apply_timeline_visual(&mut self, looping: bool) {
        if !self.candidate_is_active() {
            self.status = "Publish the Candidate before arming a Timeline range".to_owned();
            return;
        }
        let Some(range) = self.timeline_state.commit_visual() else {
            self.status = "Timeline range must have a positive duration".to_owned();
            return;
        };
        self.timeline_state.set_position(TimelinePosition {
            at: range.start,
            ..self.timeline_position()
        });
        self.timeline_state.arm_cursor();
        let mut plan = self.audition_plan.clone();
        plan.range = AuditionRange {
            from: Some(range.start.to_string()),
            to: Some(range.end.to_string()),
            looping,
        };
        let initial_frame = self.timeline_frame_at(range.start);
        self.activate_audition_plan_at(plan, initial_frame);
        self.status = format!(
            "Timeline range {}–{} armed{}",
            range.start,
            range.end,
            if looping {
                " as a loop"
            } else {
                " for one-shot playback"
            }
        );
    }

    fn fit_timeline_range(&mut self) {
        let range = self.timeline_state.visual_range().or_else(|| {
            let project = self.bundle.project.as_ref()?;
            Some(TimelineRange {
                start: timeline::parse_destination(
                    self.audition_plan.range.from.as_deref()?,
                    self.timeline_position().at,
                    project,
                )
                .ok()?,
                end: timeline::parse_destination(
                    self.audition_plan.range.to.as_deref()?,
                    self.timeline_position().at,
                    project,
                )
                .ok()?,
            })
        });
        let Some(range) = range else {
            self.status = "Select or arm an exact Timeline range before fitting it".to_owned();
            return;
        };
        let Some(song_end) = self.bundle.project.as_ref().map(|project| project.song.end) else {
            return;
        };
        self.timeline_viewport.fit_range(range, song_end);
        self.status = "Timeline viewport fitted to the selected range".to_owned();
    }

    fn center_timeline_cursor(&mut self) {
        let Some(song_end) = self.bundle.project.as_ref().map(|project| project.song.end) else {
            return;
        };
        self.timeline_viewport
            .center_on(self.timeline_position().at, song_end);
        self.status = "Timeline cursor centered".to_owned();
    }

    fn align_timeline_cursor(&mut self, end: bool) {
        let Some(song_end) = self.bundle.project.as_ref().map(|project| project.song.end) else {
            return;
        };
        if end {
            self.timeline_viewport
                .align_end(self.timeline_position().at, song_end);
        } else {
            self.timeline_viewport
                .align_start(self.timeline_position().at, song_end);
        }
        self.status = if end {
            "Timeline cursor aligned to the right edge"
        } else {
            "Timeline cursor aligned to the left edge"
        }
        .to_owned();
    }

    fn toggle_timeline_follow(&mut self) {
        let enabled = self.timeline_state.toggle_follow_playhead();
        self.status = format!(
            "Timeline playhead follow {}",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    fn open_timeline_goto(&mut self) {
        self.modal = Some(Modal::Input {
            title: "Go to Timeline Position".to_owned(),
            prompt: "BAR | BAR:BEAT | LOCATOR | +DURATION | -DURATION | exact musical time"
                .to_owned(),
            value: String::new(),
            replace_on_type: false,
            action: InputAction::TimelineGoto,
        });
    }

    fn open_timeline_search(&mut self) {
        self.modal = Some(Modal::Input {
            title: "Search Timeline".to_owned(),
            prompt: "Search Track, Clip, Pattern, Instrument, or Locator names and IDs".to_owned(),
            value: String::new(),
            replace_on_type: false,
            action: InputAction::TimelineSearch,
        });
    }

    fn open_timeline_locator(&mut self) {
        self.modal = Some(Modal::Input {
            title: "Create Locator".to_owned(),
            prompt: "Enter `ID NAME`; the Locator is created at the exact Timeline cursor"
                .to_owned(),
            value: String::new(),
            replace_on_type: false,
            action: InputAction::TimelineLocator {
                at: self.timeline_position().at.to_string(),
            },
        });
    }

    fn search_timeline(&mut self, query: String) {
        let Some(project) = self.bundle.project.as_ref() else {
            return;
        };
        let hits = timeline::search_project(project, &query);
        let count = hits.len();
        let Some(hit) = self.timeline_state.set_search(query.clone(), hits) else {
            self.status = format!("No Timeline object matches `{query}`");
            return;
        };
        self.set_timeline_position(
            hit.position,
            true,
            true,
            &format!("{}/{} {label}", 1, count, label = hit.label),
        );
    }

    fn repeat_timeline_search(&mut self, direction: isize) {
        let Some(hit) = self.timeline_state.repeat_search(direction) else {
            self.status = "Search the Timeline with / before using n or N".to_owned();
            return;
        };
        let summary = self
            .timeline_state
            .search_summary()
            .map(|(query, index, count)| format!("{index}/{count} `{query}` · {}", hit.label))
            .unwrap_or_else(|| hit.label.clone());
        self.set_timeline_position(hit.position, true, true, &summary);
    }

    fn set_timeline_mark(&mut self, mark: char) {
        self.timeline_state.set_mark(mark);
        self.status = format!(
            "Set transient Timeline mark `{}`",
            mark.to_ascii_lowercase()
        );
    }

    fn jump_timeline_mark(&mut self, mark: char) {
        if mark == '\'' {
            self.jump_timeline_history(-1);
            return;
        }
        let Some(position) = self.timeline_state.mark(mark) else {
            self.status = format!("Timeline mark `{}` is unset", mark.to_ascii_lowercase());
            return;
        };
        self.set_timeline_position(position, true, true, "Jumped to transient Timeline mark");
    }

    fn jump_timeline_history(&mut self, direction: isize) {
        let position = if direction < 0 {
            self.timeline_state.jump_back()
        } else {
            self.timeline_state.jump_forward()
        };
        let Some(position) = position else {
            self.status = if direction < 0 {
                "Timeline jump list is at its oldest position"
            } else {
                "Timeline jump list is at its newest position"
            }
            .to_owned();
            return;
        };
        self.selected_track = position.track;
        if self.view != View::Timeline {
            self.view = View::Timeline;
        }
        if let Some(song_end) = self.bundle.project.as_ref().map(|project| project.song.end) {
            self.timeline_viewport.reveal(position.at, song_end);
        }
        self.sync_instrument_to_track();
        self.timeline_state.arm_cursor();
        let _ = self.queue_timeline_seek();
    }

    fn open_source_location(&mut self, location: SourceLocation, description: String) {
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

    fn timeline_go_definition(&mut self) {
        let position = self.timeline_position();
        let Some(pattern_id) = self.bundle.project.as_ref().and_then(|project| {
            timeline::clip_at(project, position).map(|(_, clip)| clip.pattern.clone())
        }) else {
            self.status = "No Clip lies under the Timeline cursor".to_owned();
            return;
        };
        let Some(index) = self.bundle.project.as_ref().and_then(|project| {
            project
                .song
                .patterns
                .iter()
                .position(|pattern| pattern.id == pattern_id)
        }) else {
            self.status = format!("Pattern `{pattern_id}` is unavailable");
            return;
        };
        self.timeline_state.remember_for_return();
        self.selected_pattern = index;
        self.selected_event = 0;
        self.view = View::EventGrid;
        self.status = format!("Opened Pattern `{pattern_id}` from Timeline Clip");
    }

    fn timeline_go_source(&mut self) {
        let position = self.timeline_position();
        let Some((location, id)) = self.bundle.project.as_ref().and_then(|project| {
            timeline::clip_at(project, position)
                .map(|(_, clip)| (clip.location.clone(), clip.id.clone()))
        }) else {
            self.status = "No Clip lies under the Timeline cursor".to_owned();
            return;
        };
        self.timeline_state.remember_for_return();
        self.open_source_location(location, format!("Clip `{id}`"));
    }

    fn timeline_go_instrument(&mut self) {
        let Some(instrument_id) = self
            .selected_track_ref()
            .map(|track| track.instrument.clone())
        else {
            return;
        };
        let Some(index) = self.bundle.project.as_ref().and_then(|project| {
            project
                .song
                .instruments
                .iter()
                .position(|instrument| instrument.id == instrument_id)
        }) else {
            self.status = format!("Instrument `{instrument_id}` is unavailable");
            return;
        };
        self.timeline_state.remember_for_return();
        self.selected_instrument = index;
        self.selected_instrument_section = 0;
        self.selected_instrument_row = 0;
        self.view = View::Instrument;
        self.status = format!("Opened Instrument `{instrument_id}`");
    }

    fn timeline_go_mixer(&mut self) {
        let Some(track_id) = self.selected_track_ref().map(|track| track.id.clone()) else {
            return;
        };
        let identity = format!("track/{track_id}");
        let Some(index) = self
            .mixer_parameters
            .iter()
            .position(|parameter| parameter.channel_identity.as_deref() == Some(&identity))
        else {
            self.status = format!("Mixer channel `{identity}` is unavailable");
            return;
        };
        self.timeline_state.remember_for_return();
        self.selected_mixer_parameter = index;
        self.view = View::Mixer;
        self.status = format!("Opened Mixer channel `{identity}`");
    }

    fn timeline_go_processing(&mut self) {
        if !self.candidate_is_active() {
            self.status = "Publish the Candidate before opening its Processing route".to_owned();
            return;
        }
        let Some(track_id) = self.selected_track_ref().map(|track| track.id.clone()) else {
            return;
        };
        let mut plan = self.audition_plan.clone();
        plan.target = AuditionTarget::Track {
            track: track_id.clone(),
        };
        plan.path = AuditionPath::Channel {
            tap: TapPoint::PostFader,
        };
        plan.range = AuditionRange::default();
        self.timeline_state.remember_for_return();
        self.timeline_state.disarm_cursor();
        self.activate_audition_plan(plan);
        self.view = View::Processing;
        self.status = format!("Opened Processing route for Track `{track_id}`");
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
            View::Faceplate | View::Instrument | View::Parameters | View::Graph => {
                self.selected_instrument =
                    wrap_selection(self.selected_instrument, self.instrument_count(), delta);
                self.selected_instrument_section = 0;
                self.selected_instrument_row = 0;
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
                self.timeline_state.set_track(self.selected_track);
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
                    .processing_graph()
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
            View::Instrument => self.move_instrument_selection(delta),
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
            View::Source => {
                self.source_scroll = self
                    .source_scroll
                    .saturating_add_signed(delta)
                    .min(self.source_line_count().saturating_sub(1))
            }
            View::History => {
                self.selected_history =
                    wrap_selection(self.selected_history, self.history.len(), delta)
            }
            View::Diagnostics => {
                self.selected_diagnostic = wrap_selection(
                    self.selected_diagnostic,
                    self.bundle.diagnostics.as_slice().len(),
                    delta,
                )
            }
            View::Help => {
                self.help_scroll = self
                    .help_scroll
                    .saturating_add_signed(i16::try_from(delta).unwrap_or(if delta < 0 {
                        i16::MIN
                    } else {
                        i16::MAX
                    }))
                    .min(u16::try_from(self.help_line_count.saturating_sub(1)).unwrap_or(u16::MAX))
            }
        }
    }

    fn move_instrument_selection(&mut self, delta: isize) {
        let Some(sections) = self.instrument_section_cache.get(self.selected_instrument) else {
            return;
        };
        let total = sections
            .iter()
            .map(|section| section.rows.len())
            .sum::<usize>();
        if total == 0 {
            self.selected_instrument_section = 0;
            self.selected_instrument_row = 0;
            return;
        }
        let section_index = self
            .selected_instrument_section
            .min(sections.len().saturating_sub(1));
        let current = sections
            .iter()
            .take(section_index)
            .map(|section| section.rows.len())
            .sum::<usize>()
            .saturating_add(
                self.selected_instrument_row
                    .min(sections[section_index].rows.len().saturating_sub(1)),
            );
        let selected = wrap_selection(current, total, delta);
        let mut offset = 0;
        for (index, section) in sections.iter().enumerate() {
            if selected < offset + section.rows.len() {
                self.selected_instrument_section = index;
                self.selected_instrument_row = selected - offset;
                return;
            }
            offset += section.rows.len();
        }
    }

    fn move_instrument_selection_extreme(&mut self, first: bool) {
        let Some(sections) = self.instrument_section_cache.get(self.selected_instrument) else {
            return;
        };
        let selected = if first {
            sections
                .iter()
                .position(|section| !section.rows.is_empty())
                .map(|section| (section, 0))
        } else {
            sections
                .iter()
                .rposition(|section| !section.rows.is_empty())
                .map(|section| (section, sections[section].rows.len().saturating_sub(1)))
        };
        if let Some((section, row)) = selected {
            self.selected_instrument_section = section;
            self.selected_instrument_row = row;
        } else {
            self.selected_instrument_section = 0;
            self.selected_instrument_row = 0;
        }
    }

    fn adjust_selected(&mut self, direction: isize) {
        match self.view {
            View::Timeline => self.step_track_level(direction as f64),
            View::Mixer => self.step_mixer_parameter(direction as f64),
            View::Processing => {}
            View::EventGrid => self.step_note(direction),
            View::Faceplate | View::Parameters => self.step_parameter(direction as f64),
            View::Instrument => {
                self.selected_instrument_section = wrap_selection(
                    self.selected_instrument_section,
                    self.instrument_section_count(),
                    direction,
                );
                self.selected_instrument_row = 0;
            }
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
            View::Instrument => self.open_instrument_item(),
            View::Automation => self.open_automation_value(),
            View::Graph => self.open_graph_source(),
            View::History => self.open_history_diff(),
            _ => {}
        }
    }

    fn open_instrument_item(&mut self) {
        let Some(row) = self.selected_instrument_row() else {
            self.status = "Selected Instrument section item is unavailable".to_owned();
            return;
        };
        match row.kind {
            Some(PresentationItemKind::Parameter) => {
                let Some(index) = self
                    .bundle
                    .project
                    .as_ref()
                    .and_then(|project| project.song.instruments.get(self.selected_instrument))
                    .and_then(|instrument| {
                        instrument
                            .parameters
                            .iter()
                            .position(|parameter| parameter.key == row.id)
                    })
                else {
                    self.status = "Selected Instrument Parameter is unavailable".to_owned();
                    return;
                };
                self.selected_parameter = index;
                self.open_parameter();
            }
            Some(PresentationItemKind::Route) => {
                let Some((instrument, route, depth)) = self
                    .bundle
                    .project
                    .as_ref()
                    .and_then(|project| project.song.instruments.get(self.selected_instrument))
                    .and_then(|instrument| {
                        instrument
                            .modulation_routes
                            .iter()
                            .find(|route| route.id == row.id)
                            .map(|route| {
                                (instrument.id.clone(), route.id.clone(), route.depth.clone())
                            })
                    })
                else {
                    self.status = "Selected Instrument Modulation route is unavailable".to_owned();
                    return;
                };
                self.modal = Some(Modal::Input {
                    title: format!("Exact route depth · {instrument}.{route}"),
                    prompt: "Enter a physical depth literal in the target Parameter unit"
                        .to_owned(),
                    value: depth,
                    replace_on_type: true,
                    action: InputAction::InstrumentRouteDepth { instrument, route },
                });
            }
            _ => self.open_instrument_source(&row),
        }
    }

    fn open_instrument_source(&mut self, row: &InstrumentSectionRow) {
        let Some(source_index) = self
            .bundle
            .sources
            .keys()
            .position(|path| path == &row.source_file)
        else {
            self.status = format!("Source {} is unavailable", row.source_file.display());
            return;
        };
        let line = self
            .bundle
            .sources
            .get(&row.source_file)
            .map_or(1, |source| {
                source[..row.source_offset.min(source.len())]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1
            });
        self.selected_source = source_index;
        self.source_scroll = line.saturating_sub(1);
        self.view = View::Source;
        self.status = format!("{} at {}:{line}", row.id, row.source_file.display());
    }

    fn toggle_instrument_route_bypass(&mut self) {
        let Some(row) = self.selected_instrument_row() else {
            return;
        };
        if row.kind != Some(PresentationItemKind::Route) {
            self.status = "Select a Modulation route to toggle bypass".to_owned();
            return;
        }
        let Some((instrument, route, bypass)) = self
            .bundle
            .project
            .as_ref()
            .and_then(|project| project.song.instruments.get(self.selected_instrument))
            .and_then(|instrument| {
                instrument
                    .modulation_routes
                    .iter()
                    .find(|route| route.id == row.id)
                    .map(|route| (instrument.id.clone(), route.id.clone(), !route.bypass))
            })
        else {
            self.status = "Selected Instrument Modulation route is unavailable".to_owned();
            return;
        };
        self.apply_operation(
            ProductionViewOperation::SetInstrumentRouteBypass {
                instrument: instrument.clone(),
                route: route.clone(),
                bypass,
            },
            format!("Set {instrument}.{route} bypass to {bypass}"),
        );
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
        let node_order = graph_processing_node_indexes(instrument, graph);
        let bus_order = graph_processing_bus_indexes(graph, &node_order);
        let voice_items = graph.nodes.len() + graph.connections.len();
        let shared = instrument.shared_graph.as_ref();
        let shared_node_order = shared
            .map(|graph| graph_processing_node_indexes(instrument, graph))
            .unwrap_or_default();
        let shared_bus_order = shared
            .map(|graph| graph_processing_bus_indexes(graph, &shared_node_order))
            .unwrap_or_default();
        let shared_items = shared
            .map(|graph| graph.nodes.len() + graph.connections.len())
            .unwrap_or(0);
        let (location, description) = if let Some(node) = node_order
            .get(index)
            .and_then(|node_index| graph.nodes.get(*node_index))
        {
            (&node.location, format!("Voice Graph Node `{}`", node.id))
        } else if let Some(connection) = index
            .checked_sub(graph.nodes.len())
            .and_then(|index| bus_order.get(index))
            .and_then(|connection_index| graph.connections.get(*connection_index))
        {
            (
                &connection.location,
                format!(
                    "Voice Bus `{}` `{}.{} -> {}.{}`",
                    connection.id,
                    connection.from.node,
                    connection.from.port,
                    connection.to.node,
                    connection.to.port
                ),
            )
        } else if let Some(node) =
            shared
                .zip(index.checked_sub(voice_items))
                .and_then(|(graph, index)| {
                    shared_node_order
                        .get(index)
                        .and_then(|node_index| graph.nodes.get(*node_index))
                })
        {
            (&node.location, format!("Shared Graph Node `{}`", node.id))
        } else if let Some(connection) = shared
            .zip(index.checked_sub(voice_items + shared_node_order.len()))
            .and_then(|(graph, index)| {
                shared_bus_order
                    .get(index)
                    .and_then(|bus_index| graph.connections.get(*bus_index))
            })
        {
            (
                &connection.location,
                format!(
                    "Shared Bus `{}` `{}.{} -> {}.{}`",
                    connection.id,
                    connection.from.node,
                    connection.from.port,
                    connection.to.node,
                    connection.to.port
                ),
            )
        } else if let Some(parameter) = index
            .checked_sub(voice_items + shared_items)
            .and_then(|index| instrument.parameters.get(index))
        {
            (
                &parameter.location,
                format!("Parameter `{}`", parameter.key),
            )
        } else if let Some(control) = index
            .checked_sub(voice_items + shared_items + instrument.parameters.len())
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
            action: InputAction::Target { target },
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
            InputAction::Target { target } => self.apply_operation(
                ProductionViewOperation::SetParameterTarget {
                    target: target.to_string(),
                    value: PhysicalValue::Authored(value),
                },
                format!("Set {target}"),
            ),
            InputAction::Parameter {
                instrument,
                parameter,
            } => self.apply_operation(
                ProductionViewOperation::SetInstrumentParameter {
                    instrument: instrument.clone(),
                    parameter: parameter.clone(),
                    value: PhysicalValue::Authored(value),
                },
                format!("Set {instrument}.{parameter}"),
            ),
            InputAction::InstrumentRouteDepth { instrument, route } => self.apply_operation(
                ProductionViewOperation::SetInstrumentRouteDepth {
                    instrument: instrument.clone(),
                    route: route.clone(),
                    depth: value,
                },
                format!("Set {instrument}.{route} depth"),
            ),
            InputAction::TrackLevel { track } => self.apply_operation(
                ProductionViewOperation::SetTrackLevel {
                    track: track.clone(),
                    value: PhysicalValue::Authored(value),
                },
                format!("Set {track} level"),
            ),
            InputAction::TrackPan { track } => self.apply_operation(
                ProductionViewOperation::SetTrackPan {
                    track: track.clone(),
                    value: PhysicalValue::Authored(value),
                },
                format!("Set {track} pan"),
            ),
            InputAction::ProcessorSetting { processor } => {
                if let Some((setting, setting_value)) = value.trim().split_once(char::is_whitespace)
                {
                    self.apply_operation(
                        ProductionViewOperation::SetProcessorSetting {
                            processor: processor.clone(),
                            setting: setting.to_owned(),
                            value: setting_value.trim().to_owned(),
                        },
                        format!("Set {processor}.{setting}"),
                    );
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
                self.timeline_state.disarm_cursor();
                self.activate_audition_plan(plan);
            }
            InputAction::TimelineGoto => {
                let Some(project) = self.bundle.project.as_ref() else {
                    self.status = "Timeline navigation is unavailable".to_owned();
                    return;
                };
                match timeline::parse_destination(
                    value.trim(),
                    self.timeline_position().at,
                    project,
                ) {
                    Ok(at) => self.set_timeline_position(
                        TimelinePosition {
                            at,
                            ..self.timeline_position()
                        },
                        true,
                        true,
                        "Timeline jump",
                    ),
                    Err(error) => self.status = error,
                }
            }
            InputAction::TimelineSearch => self.search_timeline(value),
            InputAction::TimelineLocator { at } => {
                let Some((id, name)) = value.trim().split_once(char::is_whitespace) else {
                    self.status = "Create a Locator as `ID NAME`".to_owned();
                    return;
                };
                let name = name.trim();
                if id.is_empty() || name.is_empty() {
                    self.status = "Create a Locator as `ID NAME`".to_owned();
                    return;
                }
                self.apply_operation(
                    ProductionViewOperation::CreateLocator {
                        id: id.to_owned(),
                        name: name.to_owned(),
                        at,
                    },
                    format!("Create Locator {id}"),
                );
            }
            InputAction::AutomationValue { lane, point } => self.apply_operation(
                ProductionViewOperation::SetAutomationPointValue {
                    lane: lane.clone(),
                    point: point.clone(),
                    value,
                },
                format!("Set {lane}.{point} Automation value"),
            ),
            InputAction::AutomationTime { lane, point } => self.apply_operation(
                ProductionViewOperation::SetAutomationPointTime {
                    lane: lane.clone(),
                    point: point.clone(),
                    time: value,
                    snap_to: None,
                },
                format!("Move {lane}.{point} Automation point"),
            ),
            InputAction::AutomationCurve { lane, point } => self.apply_operation(
                ProductionViewOperation::SetAutomationPointCurve {
                    lane: lane.clone(),
                    point: point.clone(),
                    curve: value,
                },
                format!("Set {lane}.{point} Automation curve"),
            ),
            InputAction::NotePitch { pattern, event } => self.apply_operation(
                ProductionViewOperation::SetNotePitch {
                    pattern: pattern.clone(),
                    event,
                    pitch: value,
                },
                format!("Change Event in {pattern}"),
            ),
            InputAction::EventTime { pattern, event } => self.apply_operation(
                ProductionViewOperation::SetEventTime {
                    pattern: pattern.clone(),
                    event,
                    time: value,
                    snap_to: None,
                },
                format!("Move Event in {pattern}"),
            ),
            InputAction::NoteDuration { pattern, event } => self.apply_operation(
                ProductionViewOperation::SetNoteDuration {
                    pattern: pattern.clone(),
                    event,
                    duration: value,
                    snap_to: None,
                },
                format!("Resize Event in {pattern}"),
            ),
            InputAction::EventVelocity { pattern, event } => self.apply_operation(
                ProductionViewOperation::SetEventVelocity {
                    pattern: pattern.clone(),
                    event,
                    value: PhysicalValue::Authored(value),
                },
                format!("Set Event velocity in {pattern}"),
            ),
            InputAction::HitVoice { pattern, event } => self.apply_operation(
                ProductionViewOperation::SetHitVoice {
                    pattern: pattern.clone(),
                    event,
                    voice: value,
                },
                format!("Change percussion voice in {pattern}"),
            ),
            InputAction::SampleRegion { pattern, id } => match parse_frame_pair(value.trim()) {
                Ok((start_frame, end_frame)) => self.apply_operation(
                    ProductionViewOperation::TrimSample {
                        pattern: pattern.clone(),
                        occurrence: id.clone(),
                        start_frame,
                        end_frame,
                    },
                    format!("Trim sample {pattern}.{id}"),
                ),
                Err(error) => self.status = error,
            },
            InputAction::SampleFade { pattern, id } => match parse_fade_pair(value.trim()) {
                Ok((fade_in_frames, fade_out_frames)) => self.apply_operation(
                    ProductionViewOperation::FadeSample {
                        pattern: pattern.clone(),
                        occurrence: id.clone(),
                        fade_in_frames,
                        fade_out_frames,
                    },
                    format!("Fade sample {pattern}.{id}"),
                ),
                Err(error) => self.status = error,
            },
            InputAction::SampleGain { pattern, id } => self.apply_operation(
                ProductionViewOperation::SetSampleGain {
                    pattern: pattern.clone(),
                    occurrence: id.clone(),
                    value: PhysicalValue::Authored(value),
                },
                format!("Set sample {pattern}.{id} gain"),
            ),
            InputAction::SamplePan { pattern, id } => self.apply_operation(
                ProductionViewOperation::SetSamplePan {
                    pattern: pattern.clone(),
                    occurrence: id.clone(),
                    value: PhysicalValue::Authored(value),
                },
                format!("Set sample {pattern}.{id} pan"),
            ),
            InputAction::Publish => {
                let summary = value.trim();
                if summary.is_empty() {
                    self.status = "Publication summary cannot be empty".to_owned();
                    return;
                }
                self.queue_mutation(
                    ProductionViewRequest {
                        expected_revision: Some(self.bundle.revision.clone()),
                        operation: ProductionViewOperation::PublishCandidate,
                        provenance: ProductionViewProvenance::human(
                            "workspace",
                            summary,
                            "Candidate inspected and published from the Production Workspace",
                        ),
                    },
                    "Published as history entry",
                    "Publication failed",
                );
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
        self.apply_operation(
            ProductionViewOperation::SetInstrumentParameter {
                instrument: instrument_id.clone(),
                parameter: parameter_key.clone(),
                value: PhysicalValue::Number(value),
            },
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
        self.apply_operation(
            ProductionViewOperation::SetParameterTarget {
                target: target.to_string(),
                value: PhysicalValue::Number(value),
            },
            format!("Adjust {target}"),
        );
    }

    fn step_track_level(&mut self, direction: f64) {
        let Some(track) = self.selected_track_ref() else {
            return;
        };
        let id = track.id.clone();
        let value = (track.channel.level_db + direction).clamp(-96.0, 12.0);
        self.apply_operation(
            ProductionViewOperation::SetTrackLevel {
                track: id.clone(),
                value: PhysicalValue::Number(value),
            },
            format!("Adjust {id} level"),
        );
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
        self.apply_operation(
            ProductionViewOperation::SetAutomationPointValue {
                lane: lane_id.clone(),
                point: point_id.clone(),
                value: authored,
            },
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
        let operation = match event {
            PatternEvent::Note { pitch, .. } => {
                let next = pitch
                    .saturating_add_signed(direction.clamp(-1, 1) as i8)
                    .min(127);
                ProductionViewOperation::SetNotePitch {
                    pattern: pattern_id.clone(),
                    event: self.selected_event,
                    pitch: crate::model::format_pitch(next),
                }
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
                ProductionViewOperation::SetHitVoice {
                    pattern: pattern_id.clone(),
                    event: self.selected_event,
                    voice: format!("{:?}", voices[next]).to_lowercase(),
                }
            }
            PatternEvent::Sample { .. } => {
                self.status = "Edit rejected: wrong edit target kind: sample occurrences use exact region/velocity edits".to_owned();
                return;
            }
        };
        self.apply_operation(operation, format!("Change Event in {pattern_id}"));
    }

    fn apply_operation(&mut self, operation: ProductionViewOperation, summary: String) {
        self.queue_mutation(
            ProductionViewRequest {
                expected_revision: Some(self.bundle.revision.clone()),
                operation,
                provenance: ProductionViewProvenance::human(
                    "workspace",
                    summary,
                    "Checked source-backed Production View edit",
                ),
            },
            "Saved and published history entry",
            "Edit transaction failed",
        );
    }

    fn restore_history(&mut self, entry_id: &str) {
        self.queue_mutation(
            ProductionViewRequest {
                expected_revision: Some(self.bundle.revision.clone()),
                operation: ProductionViewOperation::RestoreHistory {
                    entry_id: entry_id.to_owned(),
                },
                provenance: ProductionViewProvenance::human(
                    "workspace",
                    "Restore selected Project state",
                    "Restore selected state from Project History",
                ),
            },
            format!("Restored {entry_id} as new entry"),
            "Restore failed",
        );
    }

    fn release_audition(&mut self, character: char) {
        let Some(note) = self.held_notes.remove(&character) else {
            return;
        };
        if let Some(player) = self.player.as_mut() {
            let _ = player.note_off(note.track_index, note.note_id);
        }
    }

    fn release_stale_auditions(&mut self) -> bool {
        let mut released = false;
        while let Some(key) = self
            .held_notes
            .iter()
            .find_map(|(key, note)| (note.pressed_at.elapsed() >= AUDITION_TIMEOUT).then_some(*key))
        {
            self.release_audition(key);
            released = true;
        }
        released
    }

    fn selected_instrument_is_drums(&self) -> bool {
        self.bundle
            .project
            .as_ref()
            .and_then(|project| project.song.instruments.get(self.selected_instrument))
            .is_some_and(|instrument| instrument.kind == InstrumentKind::StockDrums)
    }

    fn audition_track(&self) -> usize {
        let Some(candidate) = self.bundle.project.as_ref() else {
            return 0;
        };
        let runtime = self
            .audible_bundle
            .as_ref()
            .or(self.active_bundle.as_ref())
            .and_then(|bundle| bundle.project.as_ref())
            .unwrap_or(candidate);
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
        self.mixer_parameters
            .get(self.selected_mixer_parameter)
            .map(|parameter| (parameter.target.clone(), parameter.spec))
    }

    fn selected_mixer_channel_identity(&self) -> Option<String> {
        self.mixer_parameters
            .get(self.selected_mixer_parameter)?
            .channel_identity
            .clone()
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

    fn candidate_is_audible(&self) -> bool {
        self.player.is_some()
            && self
                .audible_bundle
                .as_ref()
                .is_some_and(|bundle| bundle.revision == self.bundle.revision)
    }

    fn runtime_matches_candidate(&self) -> bool {
        if self.player.is_some() {
            self.candidate_is_audible()
        } else {
            self.candidate_is_active()
        }
    }

    fn processing_graph(&self) -> Option<&MixGraphInspection> {
        if self.candidate_is_audible()
            || (self.player.is_none()
                && self.candidate_is_active()
                && self.audition_status.is_some())
        {
            self.audition_processing_graph
                .as_ref()
                .or(self.candidate_processing_graph.as_ref())
        } else {
            self.candidate_processing_graph.as_ref()
        }
    }

    fn control_inspection(&self) -> (Option<&RefCell<ControlInspector>>, u64, bool) {
        let live = self.player.is_some()
            && self
                .audible_bundle
                .as_ref()
                .is_some_and(|bundle| bundle.revision == self.bundle.revision);
        if live {
            (
                self.audition_control_inspector
                    .as_ref()
                    .or(self.candidate_control_inspector.as_ref()),
                self.telemetry.engine.frame,
                true,
            )
        } else {
            (self.candidate_control_inspector.as_ref(), 0, false)
        }
    }

    fn clamp_selections(&mut self) {
        self.selected_track = clamp_index(self.selected_track, self.track_count());
        if let Some(project) = self.bundle.project.as_ref() {
            self.timeline_state.clamp(project);
            self.timeline_state.set_track(self.selected_track);
        }
        self.selected_mixer_parameter =
            clamp_index(self.selected_mixer_parameter, self.mixer_parameter_count());
        self.selected_processor = clamp_index(
            self.selected_processor,
            self.processing_graph()
                .map_or(0, |graph| graph.processors.len()),
        );
        self.selected_pattern = clamp_index(self.selected_pattern, self.pattern_count());
        self.selected_event = clamp_index(self.selected_event, self.event_count());
        self.selected_instrument = clamp_index(self.selected_instrument, self.instrument_count());
        self.selected_instrument_section = clamp_index(
            self.selected_instrument_section,
            self.instrument_section_count(),
        );
        self.selected_instrument_row =
            clamp_index(self.selected_instrument_row, self.instrument_row_count());
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
        self.selected_diagnostic = clamp_index(
            self.selected_diagnostic,
            self.bundle.diagnostics.as_slice().len(),
        );
        self.selected_source = clamp_index(self.selected_source, self.bundle.sources.len());
    }

    fn track_count(&self) -> usize {
        self.bundle
            .project
            .as_ref()
            .map_or(0, |project| project.song.tracks.len())
    }

    fn mixer_parameter_count(&self) -> usize {
        self.mixer_parameters.len()
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

    fn instrument_sections(&self) -> &[instrument::InstrumentSectionView] {
        self.instrument_section_cache
            .get(self.selected_instrument)
            .map_or(&[], Vec::as_slice)
    }

    fn instrument_section_count(&self) -> usize {
        self.instrument_sections().len()
    }

    fn instrument_row_count(&self) -> usize {
        self.instrument_sections()
            .get(self.selected_instrument_section)
            .map_or(0, |section| section.rows.len())
    }

    fn selected_instrument_row(&self) -> Option<InstrumentSectionRow> {
        self.instrument_sections()
            .get(self.selected_instrument_section)
            .and_then(|section| section.rows.get(self.selected_instrument_row))
            .cloned()
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
                    let shared_items = instrument
                        .shared_graph
                        .as_ref()
                        .map_or(0, |shared| shared.nodes.len() + shared.connections.len());
                    graph.nodes.len()
                        + graph.connections.len()
                        + shared_items
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

    fn source_line_count(&self) -> usize {
        self.bundle
            .sources
            .values()
            .nth(self.selected_source)
            .map_or(0, |source| source.lines().count())
    }
}

type HistorySnapshot = (
    Option<String>,
    Option<RevisionId>,
    Vec<HistoryEntry>,
    Option<Arc<ProjectBundle>>,
);

#[cfg(test)]
fn history_state(root: &Path) -> Result<HistorySnapshot, crate::history::HistoryError> {
    let store = HistoryStore::open(root)?;
    let state = store.active_state()?;
    let history = store.list()?;
    let active_bundle = store.active_bundle()?.map(Arc::new);
    Ok(if let Some(state) = state {
        (
            Some(state.active_entry),
            Some(state.active_revision),
            history,
            active_bundle,
        )
    } else {
        (None, None, history, None)
    })
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

fn candidate_instrument_sections(
    bundle: &ProjectBundle,
) -> Vec<Vec<instrument::InstrumentSectionView>> {
    let Some(project) = bundle.project.as_ref() else {
        return Vec::new();
    };
    project
        .song
        .instruments
        .iter()
        .map(|instrument| instrument_section_views_with_assets(project, instrument, &bundle.assets))
        .collect()
}

fn candidate_runtime_inspection(
    bundle: &ProjectBundle,
) -> (Option<MixGraphInspection>, Option<ControlInspector>) {
    let compiled = bundle
        .publishable()
        .and_then(|project| CompiledProject::compile_with_assets(project, &bundle.assets).ok());
    let processing_graph = compiled
        .as_ref()
        .map(|compiled| compiled.mix_graph_inspection().clone());
    let control_inspector = compiled
        .as_ref()
        .map(|compiled| compiled.control_program().clone().into_inspector());
    (processing_graph, control_inspector)
}

fn candidate_mixer_parameters(bundle: &ProjectBundle) -> Vec<MixerParameterView> {
    let Some(project) = bundle.project.as_ref() else {
        return Vec::new();
    };
    project
        .mix_parameter_specs()
        .into_iter()
        .map(|(target, spec)| {
            let channel_identity = mixer_channel_identity(project, &target);
            let muted = channel_identity
                .as_deref()
                .and_then(mixer_mute_target)
                .and_then(|target| project.parameter_spec(&target))
                .is_some_and(|mute| mute.base >= 0.5);
            MixerParameterView {
                target,
                spec,
                channel_identity,
                muted,
            }
        })
        .collect()
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
    let Some(project) = app
        .audible_bundle
        .as_ref()
        .and_then(|bundle| bundle.project.as_ref())
    else {
        return 0.0;
    };
    app.telemetry.engine.frame as f64 * project.song.tempo_bpm
        / (f64::from(project.sample_rate) * 60.0)
}

fn transport_label(app: &Workspace) -> String {
    let Some(project) = app
        .audible_bundle
        .as_ref()
        .and_then(|bundle| bundle.project.as_ref())
    else {
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

fn stream_failure_message(status: StreamStatus) -> Option<&'static str> {
    match status {
        StreamStatus::Error => Some("PipeWire stream failed; press Space to restart"),
        StreamStatus::Stopped => {
            Some("PipeWire stream stopped or disconnected; press Space to restart")
        }
        StreamStatus::Starting
        | StreamStatus::Connecting
        | StreamStatus::Paused
        | StreamStatus::Streaming => None,
    }
}

fn event_is_active(app: &Workspace, pattern_id: &str, event: &PatternEvent) -> bool {
    if !app.candidate_is_audible() {
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

fn timeline_time_label(time: crate::time::MusicalTime, meter: crate::time::Meter) -> String {
    let bar_length = meter
        .bar_length()
        .map_or(4.0, crate::time::MusicalTime::as_f64);
    let beat_length = 4.0 / f64::from(meter.denominator.max(1));
    let quarter_notes = time.as_f64();
    let bar = (quarter_notes / bar_length).floor() as u64 + 1;
    let beat = quarter_notes.rem_euclid(bar_length) / beat_length + 1.0;
    format!("{bar}:{beat:.1}")
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
    /// The desktop window could not be created or failed while running.
    #[cfg(feature = "gui")]
    #[error("desktop client failed: {0}")]
    Gui(String),
}

#[cfg(test)]
mod tests;
