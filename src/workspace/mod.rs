//! Ratatui production workspace over the terminal-independent Project model.

use std::{
    collections::{BTreeMap, BTreeSet},
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
    engine::audition::{
        ActiveRefresh, AuditionCue, AuditionPath, AuditionPlan, AuditionPrepareOptions,
        AuditionRange, AuditionRuntimePolicy, AuditionStatus, AuditionTarget,
    },
    engine::{CompiledProject, mix_graph::MixGraphInspection},
    history::{HistoryEntry, HistoryStore, write_atomic},
    model::{
        AutomationLane, AutomationPoint, DrumVoice, Graph, InstrumentKind, ParameterSpec,
        ParameterTarget, PatternEvent, PresentationItemKind, Project, TapPoint, Unit,
    },
    production_view::{
        MoveDirection, Operation as ProductionViewOperation, PhysicalValue,
        Provenance as ProductionViewProvenance, Request as ProductionViewRequest,
        execute as execute_production_view,
    },
    project::{ProjectBundle, RevisionId, load_project},
    realtime::{PipeWirePlayer, PlayerTelemetry, StreamStatus},
};

const TICK_RATE: Duration = Duration::from_millis(40);
const REFRESH_RATE: Duration = Duration::from_millis(400);
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

mod input;
mod instrument;
mod rendering;
mod terminal;

use input::{parse_fade_pair, parse_frame_pair};
#[cfg(test)]
use instrument::instrument_section_views;
use instrument::{InstrumentSectionRow, instrument_section_views_with_assets};
use rendering::render;
pub use terminal::run;

fn graph_processing_node_indexes(
    instrument: &crate::model::Instrument,
    graph: &Graph,
) -> Vec<usize> {
    let indexes = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), BTreeSet::<&str>::new()))
        .collect::<BTreeMap<_, _>>();
    for connection in &graph.connections {
        if connection.from.node != "note"
            && let Some(destinations) = outgoing.get_mut(connection.from.node.as_str())
        {
            destinations.insert(connection.to.node.as_str());
        }
    }
    if graph.domain == crate::model::GraphDomain::Voice {
        for route in &instrument.modulation_routes {
            let source_nodes = std::iter::once(route.source.as_str())
                .chain(route.auxiliary.as_deref())
                .filter_map(|source_id| {
                    instrument
                        .modulation_sources
                        .iter()
                        .find(|source| source.id == source_id)
                })
                .filter_map(|source| match &source.kind {
                    crate::model::InstrumentModulationSourceKind::GraphPort { node, .. } => {
                        Some(node.as_str())
                    }
                    _ => None,
                });
            for source_node in source_nodes {
                if let Some(destinations) = outgoing.get_mut(source_node) {
                    destinations.extend(graph.nodes.iter().filter_map(|node| {
                        node.references_parameter(&route.target)
                            .then_some(node.id.as_str())
                    }));
                }
            }
        }
    }
    let mut incoming = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    for destinations in outgoing.values() {
        for destination in destinations {
            if let Some(count) = incoming.get_mut(destination) {
                *count += 1;
            }
        }
    }
    let mut ready = incoming
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(*id))
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(graph.nodes.len());
    while let Some(id) = ready.pop_first() {
        order.push(indexes[id]);
        for destination in &outgoing[id] {
            if let Some(count) = incoming.get_mut(destination) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    ready.insert(destination);
                }
            }
        }
    }
    if order.len() == graph.nodes.len() {
        order
    } else {
        (0..graph.nodes.len()).collect()
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Timeline,
    Mixer,
    Processing,
    EventGrid,
    Faceplate,
    Instrument,
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
    selected_instrument_section: usize,
    selected_instrument_row: usize,
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
    audition_runtime: AuditionRuntimePolicy,
    telemetry: PlayerTelemetry,
    mix_channel_identities: Vec<String>,
    processing_graph: Option<MixGraphInspection>,
    instrument_section_cache: Vec<Vec<instrument::InstrumentSectionView>>,
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
        let instrument_section_cache = candidate_instrument_sections(&bundle);
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
            selected_instrument_section: 0,
            selected_instrument_row: 0,
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
            audition_runtime: AuditionRuntimePolicy::default(),
            telemetry: PlayerTelemetry::default(),
            mix_channel_identities: Vec::new(),
            processing_graph,
            instrument_section_cache,
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
        self.instrument_section_cache = candidate_instrument_sections(&candidate);
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
        let (Some(revision), Some(player)) = (revision, self.player.as_mut()) else {
            return;
        };
        let refresh = self.audition_runtime.refresh_active(
            &self.root,
            revision,
            &self.audition_plan,
            self.telemetry.engine,
            explicit_seek,
            player,
        );
        match refresh {
            ActiveRefresh::NoActiveRevision => {}
            ActiveRefresh::Delayed => {
                self.status =
                    "Runtime publication delayed; the newest Active Revision will retry".to_owned();
            }
            ActiveRefresh::ActiveChanged(_) => {
                self.status = "Active Revision changed during reload; retrying".to_owned();
            }
            ActiveRefresh::ReloadFailed(error) => {
                self.status = format!("Active Revision reload failed: {error}");
            }
            ActiveRefresh::InvalidActiveRevision => {
                self.status = "Active Revision snapshot is invalid".to_owned();
            }
            ActiveRefresh::PreparationFailed(error) => {
                self.status = format!(
                    "Active audition target no longer compiles; prior revision remains audible: {error}"
                );
            }
            ActiveRefresh::PublicationFailed(error) => {
                self.status = format!("Runtime publication failed: {error}");
            }
            ActiveRefresh::Published { range_moved } => {
                let processing_graph = self
                    .audition_runtime
                    .processing_graph()
                    .expect("published audition graph")
                    .clone();
                self.mix_channel_identities = processing_graph
                    .channels
                    .iter()
                    .map(|channel| channel.identity.clone())
                    .collect();
                self.processing_graph = Some(processing_graph);
                self.audition_status = self.audition_runtime.status().cloned();
                self.status = if range_moved {
                    "Active audition queued · moved range will map the callback's exact relative offset"
                        .to_owned()
                } else {
                    "Active audition queued · range-relative transport retained; compatible controls ramp"
                        .to_owned()
                };
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
        if publication_ready
            && let Some(revision) = self.audition_runtime.pending_revision().cloned()
        {
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
                self.audition_runtime
                    .accept_snapshot(status.clone(), processing_graph.clone());
                self.audition_runtime.clear_pending();
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
        self.audition_runtime.clear_pending();
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
            (KeyCode::Char('b'), _) if self.view == View::Instrument => {
                self.toggle_instrument_route_bypass()
            }
            (KeyCode::Char('B'), _) if self.view == View::Processing => {
                self.toggle_durable_processor_bypass()
            }
            (KeyCode::Char('K'), _) if self.view == View::Processing => {
                self.move_selected_processor(MoveDirection::Earlier)
            }
            (KeyCode::Char('J'), _) if self.view == View::Processing => {
                self.move_selected_processor(MoveDirection::Later)
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
            View::Instrument => {
                self.selected_instrument_row = wrap_selection(
                    self.selected_instrument_row,
                    self.instrument_row_count(),
                    delta,
                )
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
                self.activate_audition_plan(plan);
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
                match execute_production_view(
                    &self.root,
                    ProductionViewRequest {
                        expected_revision: Some(self.bundle.revision.clone()),
                        operation: ProductionViewOperation::PublishCandidate,
                        provenance: ProductionViewProvenance::human(
                            "workspace",
                            summary,
                            "Candidate inspected and published from the Production Workspace",
                        ),
                    },
                ) {
                    Ok(outcome) => {
                        self.status =
                            format!("Published as history entry {}", outcome.history_entry.id);
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
        match execute_production_view(
            &self.root,
            ProductionViewRequest {
                expected_revision: Some(self.bundle.revision.clone()),
                operation,
                provenance: ProductionViewProvenance::human(
                    "workspace",
                    summary,
                    "Checked source-backed Production View edit",
                ),
            },
        ) {
            Ok(outcome) => {
                self.status = format!(
                    "Saved and published history entry {}",
                    outcome.history_entry.id
                );
                self.refresh();
            }
            Err(error) => self.status = format!("Edit transaction failed: {error}"),
        }
    }

    fn restore_history(&mut self, entry_id: &str) {
        match execute_production_view(
            &self.root,
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
        ) {
            Ok(outcome) => {
                self.status = format!(
                    "Restored {entry_id} as new entry {}",
                    outcome.history_entry.id
                );
                self.refresh();
            }
            Err(error) => self.status = format!("Restore failed: {error}"),
        }
    }

    fn handle_audition_key(&mut self, key: KeyEvent) -> bool {
        if !matches!(
            self.view,
            View::Faceplate | View::Instrument | View::Parameters | View::Graph
        ) {
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

    fn instrument_sections(&self) -> Vec<instrument::InstrumentSectionView> {
        self.instrument_section_cache
            .get(self.selected_instrument)
            .cloned()
            .unwrap_or_default()
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
mod tests;
