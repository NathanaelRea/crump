//! Compiled, inspectable processing graph shared by playback and rendering.
//!
//! The compiler lowers authored Track, Group, Return, Send, sidechain,
//! processor, panner, mute, and Master state into one stable-ID-ordered acyclic
//! program. Every runtime buffer and DSP state object is allocated before the
//! audio callback begins.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::{
    model::{
        InstrumentKind, Panner, ParameterTarget, ProcessorKind, Project, TapPoint, TrackParameter,
        Unit,
    },
    processor_semantics::{ResolvedSettingValue, resolve_settings},
    validate::{MAX_CONTROL_TARGETS, MAX_GRAPH_VOICES},
};

use super::{
    EngineError, GRAPH_VOICE_STORAGE_MULTIPLIER, SongRuntime, TrackRuntime,
    control::{CompiledControlProgram, ControlIndex},
    drums::DrumSynth,
    graph::{CompiledGraph, authored_graph_voice_state_bytes, authored_graph_work_units},
    processors::{ProcessorProgram, ProcessorRuntime, authored_processor_estimate},
    sampler::sampler_voice_state_bytes,
    schedule::{MAX_RUNTIME_TRACKS, compile_schedules},
};

/// Supported callback quantum sizes used for admission estimates and fixtures.
pub const SUPPORTED_QUANTA: [usize; 3] = [64, 128, 256];
/// Tracks, Groups, Returns, and the singleton Master admitted by the runtime.
pub const MAX_MIX_CHANNELS: usize = MAX_RUNTIME_TRACKS * 3 + 1;
/// Named maximum runtime bytes, including every admitted resident publication runtime.
pub const MAX_PUBLICATION_RUNTIME_BYTES: u64 = 256 * 1024 * 1024;
/// One Active runtime plus at most one guarded replacement/retired runtime.
pub const MAX_RESIDENT_PUBLICATION_RUNTIMES: u64 = 2;
/// Named abstract work-unit budget per rendered sample.
pub const MAX_WORK_UNITS_PER_FRAME: u64 = 65_536;
/// Maximum shared processor instances with fixed-capacity callback telemetry.
pub const MAX_PROCESSOR_INSTANCES: usize = 512;

/// One stable authored channel role exposed through inspection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelRole {
    /// Timeline Track source and channel strip.
    Track,
    /// Hierarchical summing channel.
    Group,
    /// Parallel effects channel fed by Sends.
    Return,
    /// Song singleton final output.
    Master,
}

/// Explicit panner mode compiled for one non-Master channel.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PannerMode {
    /// Collapse to mono and place with equal power.
    MonoPan,
    /// Attenuate stereo sides without crossfeed.
    Balance,
    /// Place the two input sides independently.
    StereoPan,
}

/// One inspectable compiled channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompiledChannelInspection {
    /// Dense callback-time execution index.
    pub index: usize,
    /// Stable source identity (`track/ID`, `group/ID`, `return/ID`, or `master`).
    pub identity: String,
    /// Domain role.
    pub role: ChannelRole,
    /// Explicit panner mode for non-Master channels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panner: Option<PannerMode>,
    /// Source-backed mute state.
    pub mute: bool,
    /// Ordered processor identities, including Instrument post-sum processors.
    pub processors: Vec<String>,
    /// Compensation applied to a direct Track source before shared processing.
    pub source_compensation_frames: u32,
    /// Deterministic latency at the channel output.
    pub output_latency_frames: u32,
}

/// Kind of one compiled Audio route.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteRole {
    /// Exactly one channel main output.
    Main,
    /// Durably identified parallel branch to a Return.
    Send,
    /// Named auxiliary Audio input on a processor.
    Sidechain,
}

/// One inspectable stable route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompiledRouteInspection {
    /// Stable route identity used for deterministic fan-in ordering.
    pub id: String,
    /// Route semantic role.
    pub role: RouteRole,
    /// Source channel identity.
    pub from: String,
    /// Destination channel or processor input identity.
    pub to: String,
    /// Explicit source tap.
    pub tap: TapPoint,
    /// Channel-count-aware Audio layout.
    pub layout: String,
    /// Compensation inserted on this Bus.
    pub compensation_frames: u32,
}

/// One resolved processor setting in physical or discrete form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledProcessorSettingInspection {
    /// Processor-local stable setting key.
    pub key: String,
    /// Canonical unit for a continuous setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<Unit>,
    /// Resolved physical value for a continuous setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Resolved choice for a discrete setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    /// Inclusive lower domain for a continuous setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    /// Inclusive upper domain for a continuous setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    /// Whether this value has an exact authored source literal.
    pub authored: bool,
    /// Whether the authored value also has a stable continuous Parameter target.
    pub parameter_target: bool,
}

/// One inspectable processor instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledProcessorInspection {
    /// Dense fixed-capacity runtime telemetry index.
    pub index: usize,
    /// Durable processor identity.
    pub id: String,
    /// Native implementation.
    pub kind: ProcessorKind,
    /// Owning Instrument or channel identity.
    pub owner: String,
    /// Durable source-backed bypass state.
    pub authored_bypass: bool,
    /// Bypassed processors continue processing so tails and detector state survive re-enable.
    pub state_policy: ProcessorStatePolicy,
    /// Every effective runtime setting, including explicit defaults.
    pub settings: Vec<CompiledProcessorSettingInspection>,
    /// Named auxiliary stereo inputs.
    pub sidechain_inputs: Vec<String>,
    /// Explicit generic latency authored on the processor declaration.
    pub authored_latency_frames: u32,
    /// Latency introduced by the native algorithm or kind-specific configuration.
    pub intrinsic_latency_frames: u32,
    /// Deterministic total output latency.
    pub output_latency_frames: u32,
    /// Nonlinear evaluations per input frame; one means base-rate processing.
    pub nonlinear_substeps: u8,
    /// Filtered oversampling ratio; one means no oversampling kernel.
    pub oversampling_factor: u8,
    /// Linear-phase FIR taps in the shared oversampling kernel; zero means none.
    pub oversampling_filter_taps: u16,
    /// Fixed detector headroom used for reconstructed output peaks, in decibels.
    pub true_peak_safety_margin_db: f64,
    /// Preallocated mutable processor-state bytes used by admission.
    pub state_bytes: u64,
    /// Abstract callback work units per rendered frame used by admission.
    pub work_units_per_frame: u64,
}

/// Mutable-state policy while a processor's output follows its aligned dry path.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorStatePolicy {
    /// The wet processor continues receiving Audio while bypassed.
    Continue,
}

/// One complete source-to-Master signal path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalPathInspection {
    /// Stable timeline or branch source.
    pub source: String,
    /// Ordered processing identities ending at Master.
    pub stages: Vec<String>,
    /// Total deterministic path latency after convergence compensation.
    pub total_latency_frames: u32,
}

/// Compile-time callback resource admission facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeResourceEstimate {
    /// Project sample rate represented by the estimate.
    pub sample_rate: u32,
    /// Supported callback quantum sizes.
    pub supported_quanta: [usize; 3],
    /// Timeline source count.
    pub tracks: usize,
    /// Group channel count.
    pub groups: usize,
    /// Return channel count.
    pub returns: usize,
    /// All channels including Master.
    pub channels: usize,
    /// Shared processor runtime instances after Instrument post-chain expansion.
    pub processors: usize,
    /// Main, Send, and sidechain routes.
    pub routes: usize,
    /// Actual graph voice-pool instances across Track uses.
    pub graph_voices: u64,
    /// Worst-case per-frame graph work in node-equivalent instances, including unison subvoices.
    pub graph_node_instances: u64,
    /// Dense effective Parameter targets.
    pub control_targets: usize,
    /// Abstract bounded work performed per output sample.
    pub work_units_per_frame: u64,
    /// Preallocated channel/bus block-buffer bytes at the largest quantum.
    pub bus_buffer_bytes: u64,
    /// Preallocated channel meter state bytes.
    pub meter_bytes: u64,
    /// Preallocated fixed-capacity processor telemetry state bytes.
    pub processor_meter_bytes: u64,
    /// Mutable preallocated state for every graph voice, including Node buffers.
    pub graph_voice_state_bytes: u64,
    /// Mutable preallocated stock-drum voice-vector backing.
    pub drum_voice_state_bytes: u64,
    /// Mutable preallocated sampler voice-vector backing across Track instances.
    pub sampler_voice_state_bytes: u64,
    /// Mutable preallocated state for shared native channel processors.
    pub processor_state_bytes: u64,
    /// Convergence compensation and source-alignment state bytes.
    pub compensation_state_bytes: u64,
    /// Immutable Arc-backed compiled mix-program allocation and owned metadata.
    pub compiled_program_bytes: u64,
    /// Arc-shared compiled control targets, Automation, and Modulation metadata.
    pub control_program_bytes: u64,
    /// Arc-shared per-Track schedules, including Event identity strings.
    pub schedule_program_bytes: u64,
    /// Arc-shared immutable graph programs used by runtime Tracks.
    pub instrument_program_bytes: u64,
    /// Mutable mix-runtime container allocations not represented by DSP state.
    pub runtime_structure_bytes: u64,
    /// Complete Song-runtime structure and mutable control/Track container backing.
    pub song_runtime_structure_bytes: u64,
    /// One complete runtime's conservatively admitted owned capacity.
    pub runtime_bytes: u64,
    /// Maximum simultaneously resident runtimes admitted by the publication protocol.
    pub publication_runtime_slots: u64,
    /// Complete resident-runtime publication peak bytes.
    pub publication_peak_bytes: u64,
    /// Named memory budget.
    pub publication_budget_bytes: u64,
    /// Named abstract callback work budget.
    pub work_budget_per_frame: u64,
    /// Whether every named structural, memory, and work budget admits the Project.
    pub admitted: bool,
}

impl RuntimeResourceEstimate {
    /// Produces a conservative estimate without allocating runtime DSP state.
    pub fn for_project(project: &Project, control_targets: usize) -> Result<Self, EngineError> {
        let mut graph_voices = 0_u64;
        let mut graph_node_instances = 0_u64;
        let mut graph_voice_state_bytes = 0_u64;
        let mut drum_voice_state_bytes = 0_u64;
        let mut sampler_state_bytes = 0_u64;
        let mut processor_state_bytes = 0_u64;
        let mut processor_work = 0_u64;
        let mut processors = 0_usize;
        for track in &project.song.tracks {
            let instrument = project
                .song
                .instruments
                .iter()
                .find(|instrument| instrument.id == track.instrument)
                .ok_or_else(|| {
                    EngineError::Compile(format!(
                        "Track `{}` references missing instrument `{}`",
                        track.id, track.instrument
                    ))
                })?;
            processors = processors.saturating_add(instrument.post_processors.len());
            for processor in &instrument.post_processors {
                let (bytes, work) = authored_processor_estimate(
                    processor,
                    project.sample_rate,
                    project.song.tempo_bpm,
                );
                processor_state_bytes = processor_state_bytes.saturating_add(bytes);
                processor_work = processor_work.saturating_add(work);
            }
            match instrument.kind {
                InstrumentKind::StockDrums => {
                    drum_voice_state_bytes = drum_voice_state_bytes
                        .saturating_add(DrumSynth::voice_state_bytes(instrument.polyphony));
                    continue;
                }
                InstrumentKind::Sampler => {
                    sampler_state_bytes = sampler_state_bytes
                        .saturating_add(sampler_voice_state_bytes(instrument.polyphony));
                    continue;
                }
                InstrumentKind::Graph => {}
            }
            let voices = u64::from(instrument.polyphony);
            let stored_voices = voices.saturating_mul(GRAPH_VOICE_STORAGE_MULTIPLIER);
            graph_voices = graph_voices.saturating_add(voices);
            if instrument.graph.is_some() {
                graph_node_instances = graph_node_instances.saturating_add(
                    stored_voices.saturating_mul(authored_graph_work_units(instrument)),
                );
                graph_voice_state_bytes = graph_voice_state_bytes.saturating_add(
                    stored_voices.saturating_mul(authored_graph_voice_state_bytes(
                        instrument,
                        project.sample_rate,
                        control_targets,
                    )),
                );
            }
        }
        for processor in channel_and_master_processors(project) {
            processors = processors.saturating_add(1);
            let (bytes, work) =
                authored_processor_estimate(processor, project.sample_rate, project.song.tempo_bpm);
            processor_state_bytes = processor_state_bytes.saturating_add(bytes);
            processor_work = processor_work.saturating_add(work);
        }
        let tracks = project.song.tracks.len();
        let groups = project.song.groups.len();
        let returns = project.song.returns.len();
        let channels = tracks
            .saturating_add(groups)
            .saturating_add(returns)
            .saturating_add(1);
        let sends = project
            .song
            .tracks
            .iter()
            .map(|track| track.channel.sends.len())
            .chain(
                project
                    .song
                    .groups
                    .iter()
                    .map(|group| group.channel.sends.len()),
            )
            .chain(
                project
                    .song
                    .returns
                    .iter()
                    .map(|channel| channel.channel.sends.len()),
            )
            .sum::<usize>();
        let routes = tracks
            .saturating_add(groups)
            .saturating_add(returns)
            .saturating_add(sends)
            .saturating_add(project.song.sidechains.len());
        let largest_quantum = SUPPORTED_QUANTA.into_iter().max().unwrap_or(256) as u64;
        let bus_buffer_bytes = (channels as u64)
            .saturating_mul(largest_quantum)
            .saturating_mul(2)
            .saturating_mul(4);
        let meter_bytes = (channels as u64)
            .saturating_add(MAX_MIX_CHANNELS as u64)
            .saturating_mul(std::mem::size_of::<ChannelMeter>() as u64);
        let processor_meter_bytes = (processors as u64)
            .saturating_add(MAX_PROCESSOR_INSTANCES as u64)
            .saturating_mul(std::mem::size_of::<ProcessorMeter>() as u64);
        let compensation_state_bytes = (routes as u64).saturating_mul(8);
        let fixed_runtime_bytes = (channels as u64)
            .saturating_mul(256)
            .saturating_add((control_targets as u64).saturating_mul(128));
        let runtime_bytes = graph_voice_state_bytes
            .saturating_add(drum_voice_state_bytes)
            .saturating_add(sampler_state_bytes)
            .saturating_add(processor_state_bytes)
            .saturating_add(bus_buffer_bytes)
            .saturating_add(meter_bytes)
            .saturating_add(processor_meter_bytes)
            .saturating_add(compensation_state_bytes)
            .saturating_add(fixed_runtime_bytes);
        let publication_peak_bytes =
            runtime_bytes.saturating_mul(MAX_RESIDENT_PUBLICATION_RUNTIMES);
        let work_units_per_frame = graph_node_instances
            .saturating_add((channels as u64).saturating_mul(8))
            .saturating_add((routes as u64).saturating_mul(4))
            .saturating_add(processor_work)
            .saturating_add(control_targets as u64);
        let admitted = tracks <= MAX_RUNTIME_TRACKS
            && channels <= MAX_MIX_CHANNELS
            && processors <= MAX_PROCESSOR_INSTANCES
            && graph_voices <= u64::from(MAX_GRAPH_VOICES)
            && control_targets <= MAX_CONTROL_TARGETS
            && publication_peak_bytes <= MAX_PUBLICATION_RUNTIME_BYTES
            && work_units_per_frame <= MAX_WORK_UNITS_PER_FRAME;
        Ok(Self {
            sample_rate: project.sample_rate,
            supported_quanta: SUPPORTED_QUANTA,
            tracks,
            groups,
            returns,
            channels,
            processors,
            routes,
            graph_voices,
            graph_node_instances,
            control_targets,
            work_units_per_frame,
            bus_buffer_bytes,
            meter_bytes,
            processor_meter_bytes,
            graph_voice_state_bytes,
            drum_voice_state_bytes,
            sampler_voice_state_bytes: sampler_state_bytes,
            processor_state_bytes,
            compensation_state_bytes,
            compiled_program_bytes: 0,
            control_program_bytes: 0,
            schedule_program_bytes: 0,
            instrument_program_bytes: 0,
            runtime_structure_bytes: fixed_runtime_bytes,
            song_runtime_structure_bytes: 0,
            runtime_bytes,
            publication_runtime_slots: MAX_RESIDENT_PUBLICATION_RUNTIMES,
            publication_peak_bytes,
            publication_budget_bytes: MAX_PUBLICATION_RUNTIME_BYTES,
            work_budget_per_frame: MAX_WORK_UNITS_PER_FRAME,
            admitted,
        })
    }

    fn include_compiled_state(&mut self, program: &CompiledMixProgram) {
        let channels = &program.channels;
        let routes = &program.routes;
        self.processor_state_bytes = channels
            .iter()
            .flat_map(|channel| {
                channel
                    .source_processors
                    .iter()
                    .chain(&channel.inserts)
                    .map(|processor| processor.program.state_bytes())
            })
            .fold(0_u64, u64::saturating_add);
        self.compensation_state_bytes = routes
            .iter()
            .map(|route| u64::from(route.compensation_frames.max(1)).saturating_mul(8))
            .chain(channels.iter().map(|channel| {
                u64::from(channel.source_compensation_frames.max(1)).saturating_mul(8)
            }))
            .fold(0_u64, u64::saturating_add);
        self.work_units_per_frame = self
            .graph_node_instances
            .saturating_add((self.channels as u64).saturating_mul(8))
            .saturating_add((self.routes as u64).saturating_mul(4))
            .saturating_add(
                channels
                    .iter()
                    .flat_map(|channel| channel.source_processors.iter().chain(&channel.inserts))
                    .map(|processor| processor.program.work_units())
                    .fold(0_u64, u64::saturating_add),
            )
            .saturating_add(self.control_targets as u64);
        self.compiled_program_bytes = program.allocation_bytes();
        self.runtime_structure_bytes = program.runtime_structure_bytes();
        self.recalculate_memory_and_admission();
    }

    fn include_song_runtime_state(
        &mut self,
        control_program_bytes: u64,
        schedule_program_bytes: u64,
        instrument_program_bytes: u64,
        song_runtime_structure_bytes: u64,
    ) {
        self.control_program_bytes = control_program_bytes;
        self.schedule_program_bytes = schedule_program_bytes;
        self.instrument_program_bytes = instrument_program_bytes;
        self.song_runtime_structure_bytes = song_runtime_structure_bytes;
        self.recalculate_memory_and_admission();
    }

    fn recalculate_memory_and_admission(&mut self) {
        self.runtime_bytes = self
            .bus_buffer_bytes
            .saturating_add(self.meter_bytes)
            .saturating_add(self.processor_meter_bytes)
            .saturating_add(self.graph_voice_state_bytes)
            .saturating_add(self.drum_voice_state_bytes)
            .saturating_add(self.sampler_voice_state_bytes)
            .saturating_add(self.processor_state_bytes)
            .saturating_add(self.compensation_state_bytes)
            .saturating_add(self.compiled_program_bytes)
            .saturating_add(self.control_program_bytes)
            .saturating_add(self.schedule_program_bytes)
            .saturating_add(self.instrument_program_bytes)
            .saturating_add(self.runtime_structure_bytes)
            .saturating_add(self.song_runtime_structure_bytes);
        self.publication_peak_bytes = self
            .runtime_bytes
            .saturating_mul(self.publication_runtime_slots);
        self.admitted = self.tracks <= MAX_RUNTIME_TRACKS
            && self.channels <= MAX_MIX_CHANNELS
            && self.processors <= MAX_PROCESSOR_INSTANCES
            && self.graph_voices <= u64::from(MAX_GRAPH_VOICES)
            && self.control_targets <= MAX_CONTROL_TARGETS
            && self.publication_peak_bytes <= self.publication_budget_bytes
            && self.work_units_per_frame <= self.work_budget_per_frame;
    }
}

/// Serializable inspection of the compiled processing graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MixGraphInspection {
    /// Stable deterministic execution channels.
    pub channels: Vec<CompiledChannelInspection>,
    /// Stable deterministic Audio routes.
    pub routes: Vec<CompiledRouteInspection>,
    /// Ordered native processors.
    pub processors: Vec<CompiledProcessorInspection>,
    /// Complete signal paths from timeline and branch sources.
    pub signal_paths: Vec<SignalPathInspection>,
    /// Resource admission estimate used before runtime allocation.
    pub resources: RuntimeResourceEstimate,
}

/// Immutable processing graph compiled away from the callback.
#[derive(Debug, Clone)]
pub struct CompiledMixGraph {
    program: Arc<CompiledMixProgram>,
    inspection: MixGraphInspection,
}

#[derive(Debug)]
struct CompiledMixProgram {
    channels: Vec<CompiledChannel>,
    routes: Vec<CompiledRoute>,
    track_channels: Vec<usize>,
    master_channel: usize,
    auxiliary_slots: usize,
    solo_masks: Vec<Vec<bool>>,
}

#[derive(Debug, Clone)]
struct ChannelDraft {
    identity: String,
    role: ChannelRole,
    source_index: Option<usize>,
    source_processors: Vec<CompiledProcessorInstance>,
    inserts: Vec<CompiledProcessorInstance>,
    level: MixScalar,
    panner: Option<CompiledPanner>,
    mute: MixScalar,
    output: Option<String>,
    sends: Vec<SendDraft>,
}

#[derive(Debug, Clone)]
struct SendDraft {
    id: String,
    target: String,
    tap: TapPoint,
    gain: MixScalar,
}

#[derive(Debug, Clone)]
struct RouteDraft {
    id: String,
    role: RouteRole,
    from: String,
    owner: String,
    destination: DraftDestination,
    tap: TapPoint,
    gain: Option<MixScalar>,
    processor_index: Option<usize>,
}

#[derive(Debug, Clone)]
enum DraftDestination {
    Bus(String),
    Auxiliary { slot: usize, display: String },
}

impl CompiledMixGraph {
    /// Compiles all authored processing scopes in a stable topological order.
    pub(crate) fn compile(
        project: &Project,
        controls: &CompiledControlProgram,
    ) -> Result<Self, EngineError> {
        Self::compile_with_admission(project, controls, true)
    }

    fn compile_with_admission(
        project: &Project,
        controls: &CompiledControlProgram,
        enforce_admission: bool,
    ) -> Result<Self, EngineError> {
        let instruments: BTreeMap<_, _> = project
            .song
            .instruments
            .iter()
            .map(|instrument| (instrument.id.as_str(), instrument))
            .collect();
        let processor_indexes = processor_telemetry_indices(project)?;
        let mut drafts = BTreeMap::new();
        let mut processor_owners: BTreeMap<String, (String, usize)> = BTreeMap::new();
        let mut processor_inspection = Vec::new();
        let mut authored_tracks = project.song.tracks.iter().collect::<Vec<_>>();
        authored_tracks.sort_by(|left, right| left.id.cmp(&right.id));
        for (source_index, track) in authored_tracks.into_iter().enumerate() {
            let instrument = instruments.get(track.instrument.as_str()).ok_or_else(|| {
                EngineError::Compile(format!(
                    "Track `{}` references missing instrument `{}`",
                    track.id, track.instrument
                ))
            })?;
            let identity = format!("track/{}", track.id);
            let source_processors = compile_processor_chain(
                &instrument.post_processors,
                &format!("instrument/{}@{}", instrument.id, track.id),
                controls,
                project.sample_rate,
                project.song.tempo_bpm,
                &processor_indexes,
                &mut processor_inspection,
            )?;
            let inserts = compile_processor_chain(
                &track.channel.inserts,
                &identity,
                controls,
                project.sample_rate,
                project.song.tempo_bpm,
                &processor_indexes,
                &mut processor_inspection,
            )?;
            for (index, processor) in track.channel.inserts.iter().enumerate() {
                processor_owners.insert(processor.id.clone(), (identity.clone(), index));
            }
            drafts.insert(
                identity.clone(),
                ChannelDraft {
                    identity,
                    role: ChannelRole::Track,
                    source_index: Some(source_index),
                    source_processors,
                    inserts,
                    level: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Track {
                            track: track.id.clone(),
                            parameter: TrackParameter::Level,
                        },
                    )?,
                    panner: Some(CompiledPanner::compile(
                        &track.channel.panner,
                        controls,
                        |parameter| ParameterTarget::Track {
                            track: track.id.clone(),
                            parameter,
                        },
                    )?),
                    mute: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Track {
                            track: track.id.clone(),
                            parameter: TrackParameter::Mute,
                        },
                    )?,
                    output: Some(track.channel.output.to_string()),
                    sends: compile_sends(&track.channel.sends, controls)?,
                },
            );
        }
        for group in &project.song.groups {
            let identity = format!("group/{}", group.id);
            let inserts = compile_processor_chain(
                &group.channel.inserts,
                &identity,
                controls,
                project.sample_rate,
                project.song.tempo_bpm,
                &processor_indexes,
                &mut processor_inspection,
            )?;
            for (index, processor) in group.channel.inserts.iter().enumerate() {
                processor_owners.insert(processor.id.clone(), (identity.clone(), index));
            }
            drafts.insert(
                identity.clone(),
                ChannelDraft {
                    identity,
                    role: ChannelRole::Group,
                    source_index: None,
                    source_processors: Vec::new(),
                    inserts,
                    level: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Group {
                            group: group.id.clone(),
                            parameter: TrackParameter::Level,
                        },
                    )?,
                    panner: Some(CompiledPanner::compile(
                        &group.channel.panner,
                        controls,
                        |parameter| ParameterTarget::Group {
                            group: group.id.clone(),
                            parameter,
                        },
                    )?),
                    mute: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Group {
                            group: group.id.clone(),
                            parameter: TrackParameter::Mute,
                        },
                    )?,
                    output: Some(group.channel.output.to_string()),
                    sends: compile_sends(&group.channel.sends, controls)?,
                },
            );
        }
        for channel in &project.song.returns {
            let identity = format!("return/{}", channel.id);
            let inserts = compile_processor_chain(
                &channel.channel.inserts,
                &identity,
                controls,
                project.sample_rate,
                project.song.tempo_bpm,
                &processor_indexes,
                &mut processor_inspection,
            )?;
            for (index, processor) in channel.channel.inserts.iter().enumerate() {
                processor_owners.insert(processor.id.clone(), (identity.clone(), index));
            }
            drafts.insert(
                identity.clone(),
                ChannelDraft {
                    identity,
                    role: ChannelRole::Return,
                    source_index: None,
                    source_processors: Vec::new(),
                    inserts,
                    level: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Return {
                            r#return: channel.id.clone(),
                            parameter: TrackParameter::Level,
                        },
                    )?,
                    panner: Some(CompiledPanner::compile(
                        &channel.channel.panner,
                        controls,
                        |parameter| ParameterTarget::Return {
                            r#return: channel.id.clone(),
                            parameter,
                        },
                    )?),
                    mute: MixScalar::from_target(
                        controls,
                        &ParameterTarget::Return {
                            r#return: channel.id.clone(),
                            parameter: TrackParameter::Mute,
                        },
                    )?,
                    output: Some(channel.channel.output.to_string()),
                    sends: compile_sends(&channel.channel.sends, controls)?,
                },
            );
        }
        let master_inserts = if let Some(master) = &project.song.master {
            let inserts = compile_processor_chain(
                &master.inserts,
                "master",
                controls,
                project.sample_rate,
                project.song.tempo_bpm,
                &processor_indexes,
                &mut processor_inspection,
            )?;
            for (index, processor) in master.inserts.iter().enumerate() {
                processor_owners.insert(processor.id.clone(), ("master".to_owned(), index));
            }
            inserts
        } else {
            Vec::new()
        };
        drafts.insert(
            "master".to_owned(),
            ChannelDraft {
                identity: "master".to_owned(),
                role: ChannelRole::Master,
                source_index: None,
                source_processors: Vec::new(),
                inserts: master_inserts,
                level: if project.song.master.is_some() {
                    MixScalar::from_target(controls, &ParameterTarget::Master)?
                } else {
                    MixScalar::Constant(0.0)
                },
                panner: None,
                mute: MixScalar::Constant(0.0),
                output: None,
                sends: Vec::new(),
            },
        );

        let mut route_drafts = Vec::new();
        for draft in drafts.values() {
            if let Some(output) = &draft.output {
                route_drafts.push(RouteDraft {
                    id: format!("main/{}", draft.identity),
                    role: RouteRole::Main,
                    from: draft.identity.clone(),
                    owner: output.clone(),
                    destination: DraftDestination::Bus(output.clone()),
                    tap: TapPoint::PostFader,
                    gain: None,
                    processor_index: None,
                });
            }
            for send in &draft.sends {
                let target = format!("return/{}", send.target);
                route_drafts.push(RouteDraft {
                    id: format!("send/{}", send.id),
                    role: RouteRole::Send,
                    from: draft.identity.clone(),
                    owner: target.clone(),
                    destination: DraftDestination::Bus(target),
                    tap: send.tap,
                    gain: Some(send.gain),
                    processor_index: None,
                });
            }
        }
        let mut auxiliary_slots = 0_usize;
        for sidechain in &project.song.sidechains {
            let (owner, processor_index) = processor_owners
                .get(&sidechain.processor)
                .cloned()
                .ok_or_else(|| {
                    EngineError::Compile(format!(
                        "sidechain `{}` references missing channel processor `{}`",
                        sidechain.id, sidechain.processor
                    ))
                })?;
            let slot = auxiliary_slots;
            auxiliary_slots = auxiliary_slots.saturating_add(1);
            let owner_draft = drafts.get_mut(&owner).ok_or_else(|| {
                EngineError::Compile(format!("sidechain owner `{owner}` is missing"))
            })?;
            let processor = &mut owner_draft.inserts[processor_index];
            if !processor
                .program
                .sidechain_inputs
                .iter()
                .any(|input| input == &sidechain.input)
            {
                return Err(EngineError::Compile(format!(
                    "sidechain `{}` targets undeclared input `{}` on processor `{}`",
                    sidechain.id, sidechain.input, sidechain.processor
                )));
            }
            if !processor.auxiliary_slots.is_empty() {
                return Err(EngineError::Compile(format!(
                    "processor `{}` has more than one sidechain route",
                    sidechain.processor
                )));
            }
            processor.auxiliary_slots.push(slot);
            route_drafts.push(RouteDraft {
                id: format!("sidechain/{}", sidechain.id),
                role: RouteRole::Sidechain,
                from: sidechain.from.to_string(),
                owner: owner.clone(),
                destination: DraftDestination::Auxiliary {
                    slot,
                    display: format!("processor/{}/{}", sidechain.processor, sidechain.input),
                },
                tap: sidechain.tap,
                gain: None,
                processor_index: Some(processor_index),
            });
        }
        for processor in drafts
            .values()
            .flat_map(|draft| draft.source_processors.iter().chain(draft.inserts.iter()))
        {
            if processor.auxiliary_slots.len() != processor.program.sidechain_inputs.len() {
                return Err(EngineError::Compile(format!(
                    "processor `{}` has an unconnected declared sidechain input",
                    processor.program.id
                )));
            }
        }

        let order = topological_order(drafts.keys(), &route_drafts)?;
        let mut channels = Vec::with_capacity(order.len());
        for identity in &order {
            let draft = drafts.remove(identity).ok_or_else(|| {
                EngineError::Compile(format!("compiled channel `{identity}` disappeared"))
            })?;
            channels.push(CompiledChannel {
                identity: draft.identity,
                role: draft.role,
                source_index: draft.source_index,
                source_processors: draft.source_processors,
                inserts: draft.inserts,
                level: draft.level,
                panner: draft.panner,
                mute: draft.mute,
                outgoing_routes: Vec::new(),
                source_compensation_frames: 0,
                pre_latency_frames: 0,
                post_latency_frames: 0,
            });
        }
        let indexes: BTreeMap<_, _> = channels
            .iter()
            .enumerate()
            .map(|(index, channel)| (channel.identity.clone(), index))
            .collect();
        route_drafts.sort_by(|left, right| left.id.cmp(&right.id));
        let mut routes = Vec::with_capacity(route_drafts.len());
        for draft in route_drafts {
            let from = *indexes.get(&draft.from).ok_or_else(|| {
                EngineError::Compile(format!("route `{}` has missing source", draft.id))
            })?;
            let owner = *indexes.get(&draft.owner).ok_or_else(|| {
                EngineError::Compile(format!("route `{}` has missing destination", draft.id))
            })?;
            let (destination, destination_display) = match draft.destination {
                DraftDestination::Bus(identity) => {
                    let destination = *indexes.get(&identity).ok_or_else(|| {
                        EngineError::Compile(format!("route `{}` has missing Bus", draft.id))
                    })?;
                    (RouteDestination::Bus(destination), identity)
                }
                DraftDestination::Auxiliary { slot, display } => {
                    (RouteDestination::Auxiliary(slot), display)
                }
            };
            let index = routes.len();
            channels[from].outgoing_routes.push(index);
            routes.push(CompiledRoute {
                id: draft.id,
                role: draft.role,
                from,
                owner,
                destination,
                destination_display,
                tap: draft.tap,
                gain: draft.gain,
                processor_index: draft.processor_index,
                compensation_frames: 0,
            });
        }
        compile_latency(&mut channels, &mut routes)?;
        let track_count = project.song.tracks.len();
        let mut track_channels = vec![0_usize; track_count];
        for (index, channel) in channels.iter().enumerate() {
            if let Some(source_index) = channel.source_index {
                track_channels[source_index] = index;
            }
        }
        let master_channel = *indexes
            .get("master")
            .ok_or_else(|| EngineError::Compile("missing compiled Master".to_owned()))?;
        let solo_masks = compile_solo_masks(&channels, &routes, master_channel);
        let program = Arc::new(CompiledMixProgram {
            channels,
            routes,
            track_channels,
            master_channel,
            auxiliary_slots,
            solo_masks,
        });
        let mut resources = RuntimeResourceEstimate::for_project(project, controls.target_count())?;
        resources.include_compiled_state(&program);
        if enforce_admission && !resources.admitted {
            return Err(EngineError::Compile(format!(
                "runtime resource estimate exceeds a named budget: {} bytes peak (budget {}), {} work units/frame (budget {})",
                resources.publication_peak_bytes,
                resources.publication_budget_bytes,
                resources.work_units_per_frame,
                resources.work_budget_per_frame
            )));
        }
        let inspection = compile_inspection(
            &program.channels,
            &program.routes,
            processor_inspection,
            program.master_channel,
            resources,
        );
        Ok(Self {
            program,
            inspection,
        })
    }

    pub(crate) fn resource_estimate_for_validation(
        project: &Project,
    ) -> Result<RuntimeResourceEstimate, EngineError> {
        let schedules = compile_schedules(project)?;
        let controls = CompiledControlProgram::compile(project)?;
        let mut graph = Self::compile_with_admission(project, &controls, false)?;
        let instruments: BTreeMap<_, _> = project
            .song
            .instruments
            .iter()
            .map(|instrument| (instrument.id.as_str(), instrument))
            .collect();
        let schedule_program_bytes = schedules
            .iter()
            .map(super::schedule::TrackSchedule::allocation_bytes)
            .fold(0_u64, u64::saturating_add);
        let mut instrument_program_bytes = 0_u64;
        for schedule in &schedules {
            let instrument = instruments
                .get(schedule.instrument_id.as_str())
                .ok_or_else(|| {
                    EngineError::Compile(format!("missing instrument `{}`", schedule.instrument_id))
                })?;
            if instrument.kind == InstrumentKind::Graph {
                instrument_program_bytes = instrument_program_bytes.saturating_add(
                    CompiledGraph::compile(instrument, project.sample_rate, &controls)?
                        .allocation_bytes(),
                );
            }
        }
        let song_runtime_structure_bytes = (std::mem::size_of::<SongRuntime>() as u64)
            .saturating_add(project.id.len() as u64)
            .saturating_add(
                (schedules.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<TrackRuntime>() as u64),
            )
            .saturating_add(controls.runtime_state_bytes());
        graph.inspection.resources.include_song_runtime_state(
            controls.allocation_bytes(),
            schedule_program_bytes,
            instrument_program_bytes,
            song_runtime_structure_bytes,
        );
        Ok(graph.inspection.resources)
    }

    pub(crate) fn include_song_runtime_state(
        &mut self,
        control_program_bytes: u64,
        schedule_program_bytes: u64,
        instrument_program_bytes: u64,
        song_runtime_structure_bytes: u64,
    ) -> Result<(), EngineError> {
        self.inspection.resources.include_song_runtime_state(
            control_program_bytes,
            schedule_program_bytes,
            instrument_program_bytes,
            song_runtime_structure_bytes,
        );
        let resources = &self.inspection.resources;
        if !resources.admitted {
            return Err(EngineError::Compile(format!(
                "runtime resource estimate exceeds a named budget: {} bytes peak (budget {}), {} work units/frame (budget {})",
                resources.publication_peak_bytes,
                resources.publication_budget_bytes,
                resources.work_units_per_frame,
                resources.work_budget_per_frame
            )));
        }
        Ok(())
    }

    /// Creates fully allocated mutable state for one playback/render runtime.
    #[must_use]
    pub(crate) fn runtime(&self) -> MixGraphRuntime {
        MixGraphRuntime {
            program: Arc::clone(&self.program),
            channels: self
                .program
                .channels
                .iter()
                .map(CompiledChannel::runtime)
                .collect(),
            route_delays: self
                .program
                .routes
                .iter()
                .map(|route| CompensationDelay::new(route.compensation_frames as usize))
                .collect(),
            route_gains: self
                .program
                .routes
                .iter()
                .map(|route| route.gain.map(GainRuntime::new))
                .collect(),
            bus_inputs: vec![[0.0, 0.0]; self.program.channels.len()],
            auxiliary_inputs: vec![[0.0, 0.0]; self.program.auxiliary_slots],
            meters: vec![ChannelMeter::default(); self.program.channels.len()],
            processor_meters: vec![ProcessorMeter::default(); self.inspection.processors.len()],
            channel_taps: [ChannelTapSamples::default(); MAX_MIX_CHANNELS],
            soloed: vec![false; self.program.channels.len()],
            cue: None,
            #[cfg(test)]
            reset_calls: 0,
        }
    }

    /// Returns the complete inspectable processing program and admission facts.
    #[must_use]
    pub fn inspection(&self) -> &MixGraphInspection {
        &self.inspection
    }
}

fn vec_allocation_bytes<T>(capacity: usize) -> u64 {
    (capacity as u64).saturating_mul(std::mem::size_of::<T>() as u64)
}

fn vec_bool_allocation_bytes(capacity_bits: usize) -> u64 {
    (capacity_bits.div_ceil(usize::BITS as usize) as u64)
        .saturating_mul(std::mem::size_of::<usize>() as u64)
}

impl CompiledMixProgram {
    fn allocation_bytes(&self) -> u64 {
        let arc_allocation = (std::mem::size_of::<Self>()
            + 2 * std::mem::size_of::<usize>()
            + std::mem::align_of::<Self>()) as u64;
        let channel_allocations = self
            .channels
            .iter()
            .map(|channel| {
                let processor_allocations = channel
                    .source_processors
                    .iter()
                    .chain(&channel.inserts)
                    .map(|processor| {
                        processor.program.heap_bytes().saturating_add(
                            vec_allocation_bytes::<usize>(processor.auxiliary_slots.capacity()),
                        )
                    })
                    .fold(0_u64, u64::saturating_add);
                (channel.identity.capacity() as u64)
                    .saturating_add(vec_allocation_bytes::<CompiledProcessorInstance>(
                        channel.source_processors.capacity(),
                    ))
                    .saturating_add(vec_allocation_bytes::<CompiledProcessorInstance>(
                        channel.inserts.capacity(),
                    ))
                    .saturating_add(vec_allocation_bytes::<usize>(
                        channel.outgoing_routes.capacity(),
                    ))
                    .saturating_add(processor_allocations)
            })
            .fold(0_u64, u64::saturating_add);
        let route_allocations = self
            .routes
            .iter()
            .map(|route| {
                (route.id.capacity() as u64)
                    .saturating_add(route.destination_display.capacity() as u64)
            })
            .fold(0_u64, u64::saturating_add);
        let solo_allocations = vec_allocation_bytes::<Vec<bool>>(self.solo_masks.capacity())
            .saturating_add(
                self.solo_masks
                    .iter()
                    .map(|mask| vec_bool_allocation_bytes(mask.capacity()))
                    .fold(0_u64, u64::saturating_add),
            );
        arc_allocation
            .saturating_add(vec_allocation_bytes::<CompiledChannel>(
                self.channels.capacity(),
            ))
            .saturating_add(channel_allocations)
            .saturating_add(vec_allocation_bytes::<CompiledRoute>(
                self.routes.capacity(),
            ))
            .saturating_add(route_allocations)
            .saturating_add(vec_allocation_bytes::<usize>(
                self.track_channels.capacity(),
            ))
            .saturating_add(solo_allocations)
    }

    fn runtime_structure_bytes(&self) -> u64 {
        let processor_allocations = self
            .channels
            .iter()
            .flat_map(|channel| channel.source_processors.iter().chain(&channel.inserts))
            .map(|processor| {
                processor
                    .program
                    .heap_bytes()
                    .saturating_add(vec_allocation_bytes::<usize>(
                        processor.auxiliary_slots.capacity(),
                    ))
            })
            .fold(0_u64, u64::saturating_add);
        let processor_vector_overhead = self
            .channels
            .iter()
            .map(|channel| {
                vec_allocation_bytes::<ProcessorInstanceRuntime>(
                    channel.source_processors.capacity(),
                )
                .saturating_add(vec_allocation_bytes::<ProcessorInstanceRuntime>(
                    channel.inserts.capacity(),
                ))
                .saturating_sub(
                    ((channel.source_processors.capacity() + channel.inserts.capacity()) as u64)
                        .saturating_mul(std::mem::size_of::<ProcessorRuntime>() as u64),
                )
            })
            .fold(0_u64, u64::saturating_add);
        (std::mem::size_of::<MixGraphRuntime>() as u64)
            .saturating_add(vec_allocation_bytes::<ChannelRuntime>(
                self.channels.capacity(),
            ))
            .saturating_add(processor_vector_overhead)
            .saturating_add(processor_allocations)
            .saturating_add(vec_allocation_bytes::<CompensationDelay>(
                self.routes.capacity(),
            ))
            .saturating_add(vec_allocation_bytes::<Option<GainRuntime>>(
                self.routes.capacity(),
            ))
            .saturating_add(
                (self.auxiliary_slots as u64)
                    .saturating_mul(std::mem::size_of::<[f32; 2]>() as u64),
            )
            .saturating_add(vec_bool_allocation_bytes(self.channels.len()))
    }

    fn topology_compatible(&self, other: &Self) -> bool {
        self.track_channels == other.track_channels
            && self.master_channel == other.master_channel
            && self.auxiliary_slots == other.auxiliary_slots
            && self.channels.len() == other.channels.len()
            && self.routes.len() == other.routes.len()
            && self
                .channels
                .iter()
                .zip(&other.channels)
                .all(|(left, right)| {
                    left.identity == right.identity
                        && left.role == right.role
                        && left.source_index == right.source_index
                        && left.level == right.level
                        && left.panner == right.panner
                        && left.mute == right.mute
                        && left.outgoing_routes == right.outgoing_routes
                        && left.source_compensation_frames == right.source_compensation_frames
                        && chains_compatible(&left.source_processors, &right.source_processors)
                        && chains_compatible(&left.inserts, &right.inserts)
                })
            && self.routes.iter().zip(&other.routes).all(|(left, right)| {
                left.id == right.id
                    && left.role == right.role
                    && left.from == right.from
                    && left.owner == right.owner
                    && left.destination == right.destination
                    && left.tap == right.tap
                    && left.gain == right.gain
                    && left.processor_index == right.processor_index
                    && left.compensation_frames == right.compensation_frames
            })
    }
}

#[derive(Debug, Clone)]
struct CompiledChannel {
    identity: String,
    role: ChannelRole,
    source_index: Option<usize>,
    source_processors: Vec<CompiledProcessorInstance>,
    inserts: Vec<CompiledProcessorInstance>,
    level: MixScalar,
    panner: Option<CompiledPanner>,
    mute: MixScalar,
    outgoing_routes: Vec<usize>,
    source_compensation_frames: u32,
    pre_latency_frames: u32,
    post_latency_frames: u32,
}

impl CompiledChannel {
    fn runtime(&self) -> ChannelRuntime {
        ChannelRuntime {
            source_delay: CompensationDelay::new(self.source_compensation_frames as usize),
            source_processors: self
                .source_processors
                .iter()
                .map(CompiledProcessorInstance::runtime)
                .collect(),
            inserts: self
                .inserts
                .iter()
                .map(CompiledProcessorInstance::runtime)
                .collect(),
            fader: GainRuntime::new(self.level),
            panner: self.panner.map(CompiledPanner::runtime),
        }
    }

    fn processor_input_offset(&self, insert_index: usize) -> u32 {
        self.source_processors
            .iter()
            .chain(self.inserts.iter().take(insert_index))
            .map(|processor| processor.program.output_latency_frames())
            .fold(0_u32, u32::saturating_add)
    }

    fn total_processor_latency(&self) -> u32 {
        self.source_processors
            .iter()
            .chain(&self.inserts)
            .map(|processor| processor.program.output_latency_frames())
            .fold(0_u32, u32::saturating_add)
    }
}

#[derive(Debug, Clone)]
struct CompiledProcessorInstance {
    program: ProcessorProgram,
    auxiliary_slots: Vec<usize>,
    telemetry_index: usize,
}

impl CompiledProcessorInstance {
    fn runtime(&self) -> ProcessorInstanceRuntime {
        ProcessorInstanceRuntime {
            processor: self.program.runtime(),
            auxiliary_slots: self.auxiliary_slots.clone(),
            telemetry_index: self.telemetry_index,
        }
    }
}

#[derive(Debug, Clone)]
struct ProcessorInstanceRuntime {
    processor: ProcessorRuntime,
    auxiliary_slots: Vec<usize>,
    telemetry_index: usize,
}

#[derive(Debug, Clone, Copy)]
enum MixScalar {
    Dynamic { index: ControlIndex, initial: f64 },
    Constant(f64),
}

impl PartialEq for MixScalar {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Dynamic { index: left, .. }, Self::Dynamic { index: right, .. }) => {
                left == right
            }
            (Self::Constant(left), Self::Constant(right)) => left.to_bits() == right.to_bits(),
            _ => false,
        }
    }
}

impl MixScalar {
    fn from_target(
        controls: &CompiledControlProgram,
        target: &ParameterTarget,
    ) -> Result<Self, EngineError> {
        let index = controls.index(target).ok_or_else(|| {
            EngineError::Compile(format!("missing compiled Parameter `{target}`"))
        })?;
        let initial = controls.initial_value(index).ok_or_else(|| {
            EngineError::Compile(format!("missing initial Parameter value `{target}`"))
        })?;
        Ok(Self::Dynamic { index, initial })
    }

    fn value(self, controls: &[f64]) -> f64 {
        match self {
            Self::Dynamic { index, initial } => controls.get(index.0).copied().unwrap_or(initial),
            Self::Constant(value) => value,
        }
    }

    const fn initial(self) -> f64 {
        match self {
            Self::Dynamic { initial, .. } | Self::Constant(initial) => initial,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CompiledPanner {
    MonoPan(MixScalar),
    Balance(MixScalar),
    StereoPan { left: MixScalar, right: MixScalar },
}

impl CompiledPanner {
    fn compile(
        panner: &Panner,
        controls: &CompiledControlProgram,
        target: impl Fn(TrackParameter) -> ParameterTarget,
    ) -> Result<Self, EngineError> {
        match panner {
            Panner::MonoPan { .. } => Ok(Self::MonoPan(MixScalar::from_target(
                controls,
                &target(TrackParameter::Pan),
            )?)),
            Panner::Balance { .. } => Ok(Self::Balance(MixScalar::from_target(
                controls,
                &target(TrackParameter::Balance),
            )?)),
            Panner::StereoPan { .. } => Ok(Self::StereoPan {
                left: MixScalar::from_target(controls, &target(TrackParameter::LeftPan))?,
                right: MixScalar::from_target(controls, &target(TrackParameter::RightPan))?,
            }),
        }
    }

    const fn mode(self) -> PannerMode {
        match self {
            Self::MonoPan(_) => PannerMode::MonoPan,
            Self::Balance(_) => PannerMode::Balance,
            Self::StereoPan { .. } => PannerMode::StereoPan,
        }
    }

    fn initial_positions(self) -> [f64; 2] {
        match self {
            Self::MonoPan(position) | Self::Balance(position) => [position.initial(), 0.0],
            Self::StereoPan { left, right } => [left.initial(), right.initial()],
        }
    }

    fn positions(self, controls: &[f64]) -> [f64; 2] {
        match self {
            Self::MonoPan(position) | Self::Balance(position) => [position.value(controls), 0.0],
            Self::StereoPan { left, right } => [left.value(controls), right.value(controls)],
        }
    }

    fn runtime(self) -> PannerRuntime {
        PannerRuntime::new(self)
    }

    #[cfg(test)]
    fn process(self, sample: [f32; 2], controls: &[f64]) -> [f32; 2] {
        self.runtime().process(sample, controls)
    }
}

#[derive(Debug, Clone)]
struct GainRuntime {
    scalar: MixScalar,
    last_value: f64,
    coefficient: f32,
    #[cfg(test)]
    coefficient_updates: usize,
}

impl GainRuntime {
    fn new(scalar: MixScalar) -> Self {
        let initial = scalar.initial();
        Self {
            scalar,
            last_value: initial,
            coefficient: db_gain(initial),
            #[cfg(test)]
            coefficient_updates: 0,
        }
    }

    fn reset(&mut self) {
        self.last_value = self.scalar.initial();
        self.coefficient = db_gain(self.last_value);
        #[cfg(test)]
        {
            self.coefficient_updates = 0;
        }
    }

    fn coefficient(&mut self, controls: &[f64]) -> f32 {
        let value = self.scalar.value(controls);
        if value.to_bits() != self.last_value.to_bits() {
            self.last_value = value;
            self.coefficient = db_gain(value);
            #[cfg(test)]
            {
                self.coefficient_updates = self.coefficient_updates.saturating_add(1);
            }
        }
        self.coefficient
    }
}

#[derive(Debug, Clone)]
struct PannerRuntime {
    program: CompiledPanner,
    last_positions: [f64; 2],
    matrix: [[f32; 2]; 2],
    #[cfg(test)]
    coefficient_updates: usize,
}

impl PannerRuntime {
    fn new(program: CompiledPanner) -> Self {
        let last_positions = program.initial_positions();
        Self {
            program,
            last_positions,
            matrix: panner_matrix(program.mode(), last_positions),
            #[cfg(test)]
            coefficient_updates: 0,
        }
    }

    fn reset(&mut self) {
        self.last_positions = self.program.initial_positions();
        self.matrix = panner_matrix(self.program.mode(), self.last_positions);
        #[cfg(test)]
        {
            self.coefficient_updates = 0;
        }
    }

    fn process(&mut self, sample: [f32; 2], controls: &[f64]) -> [f32; 2] {
        let positions = self.program.positions(controls);
        if positions
            .iter()
            .zip(self.last_positions)
            .any(|(current, previous)| current.to_bits() != previous.to_bits())
        {
            self.last_positions = positions;
            self.matrix = panner_matrix(self.program.mode(), positions);
            #[cfg(test)]
            {
                self.coefficient_updates = self.coefficient_updates.saturating_add(1);
            }
        }
        [
            sample[0] * self.matrix[0][0] + sample[1] * self.matrix[0][1],
            sample[0] * self.matrix[1][0] + sample[1] * self.matrix[1][1],
        ]
    }
}

fn panner_matrix(mode: PannerMode, positions: [f64; 2]) -> [[f32; 2]; 2] {
    match mode {
        PannerMode::MonoPan => {
            let angle = (positions[0].clamp(-1.0, 1.0) + 1.0) * std::f64::consts::FRAC_PI_4;
            let left = angle.cos() as f32 * 0.5;
            let right = angle.sin() as f32 * 0.5;
            [[left, left], [right, right]]
        }
        PannerMode::Balance => {
            let position = positions[0].clamp(-1.0, 1.0);
            let left = if position > 0.0 {
                (position * std::f64::consts::FRAC_PI_2).cos() as f32
            } else {
                1.0
            };
            let right = if position < 0.0 {
                (-position * std::f64::consts::FRAC_PI_2).cos() as f32
            } else {
                1.0
            };
            [[left, 0.0], [0.0, right]]
        }
        PannerMode::StereoPan => {
            let left_angle = (positions[0].clamp(-1.0, 1.0) + 1.0) * std::f64::consts::FRAC_PI_4;
            let right_angle = (positions[1].clamp(-1.0, 1.0) + 1.0) * std::f64::consts::FRAC_PI_4;
            [
                [left_angle.cos() as f32, right_angle.cos() as f32],
                [left_angle.sin() as f32, right_angle.sin() as f32],
            ]
        }
    }
}

#[derive(Debug, Clone)]
struct CompiledRoute {
    id: String,
    role: RouteRole,
    from: usize,
    owner: usize,
    destination: RouteDestination,
    destination_display: String,
    tap: TapPoint,
    gain: Option<MixScalar>,
    processor_index: Option<usize>,
    compensation_frames: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteDestination {
    Bus(usize),
    Auxiliary(usize),
}

/// Preallocated mutable processing state shared by monitoring and rendering.
#[derive(Debug, Clone)]
pub(crate) struct MixGraphRuntime {
    program: Arc<CompiledMixProgram>,
    channels: Vec<ChannelRuntime>,
    route_delays: Vec<CompensationDelay>,
    route_gains: Vec<Option<GainRuntime>>,
    bus_inputs: Vec<[f32; 2]>,
    auxiliary_inputs: Vec<[f32; 2]>,
    meters: Vec<ChannelMeter>,
    processor_meters: Vec<ProcessorMeter>,
    channel_taps: [ChannelTapSamples; MAX_MIX_CHANNELS],
    soloed: Vec<bool>,
    cue: Option<(usize, TapPoint)>,
    #[cfg(test)]
    reset_calls: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct ChannelTapSamples {
    source: [f32; 2],
    pre_fader: [f32; 2],
    post_fader: [f32; 2],
}

#[derive(Debug, Clone)]
struct ChannelRuntime {
    source_delay: CompensationDelay,
    source_processors: Vec<ProcessorInstanceRuntime>,
    inserts: Vec<ProcessorInstanceRuntime>,
    fader: GainRuntime,
    panner: Option<PannerRuntime>,
}

impl MixGraphRuntime {
    /// Reports whether mutable processing state can transfer without changing semantics.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn topology_compatible(&self, other: &Self) -> bool {
        self.program.topology_compatible(&other.program)
    }

    /// Executes one sample through every processing scope in topological order.
    pub(crate) fn process(
        &mut self,
        sources: &[[f32; 2]],
        controls: &[f64],
        audition: bool,
    ) -> [f32; 2] {
        debug_assert_eq!(sources.len(), self.program.track_channels.len());
        self.bus_inputs.fill([0.0, 0.0]);
        self.auxiliary_inputs.fill([0.0, 0.0]);
        let any_solo = audition && self.soloed.iter().any(|selected| *selected);
        let mut master = [0.0_f32, 0.0_f32];
        let mut cue_output = None;
        let program = &self.program;
        let channel_states = &mut self.channels;
        let route_delays = &mut self.route_delays;
        let route_gains = &mut self.route_gains;
        let bus_inputs = &mut self.bus_inputs;
        let auxiliary_inputs = &mut self.auxiliary_inputs;
        let meters = &mut self.meters;
        let processor_meters = &mut self.processor_meters;
        let channel_taps = &mut self.channel_taps;
        for index in 0..program.channels.len() {
            let channel = &program.channels[index];
            let state = &mut channel_states[index];
            let mut sample = bus_inputs[index];
            if let Some(source_index) = channel.source_index {
                let source = sources.get(source_index).copied().unwrap_or([0.0, 0.0]);
                let source = state.source_delay.process(source);
                sample[0] += source[0];
                sample[1] += source[1];
            }
            for processor in &mut state.source_processors {
                sample = process_processor(
                    processor,
                    sample,
                    auxiliary_inputs,
                    controls,
                    processor_meters,
                    audition,
                );
            }
            let source = sample;
            for processor in &mut state.inserts {
                sample = process_processor(
                    processor,
                    sample,
                    auxiliary_inputs,
                    controls,
                    processor_meters,
                    audition,
                );
            }
            let pre_fader = sample;
            let gain = state.fader.coefficient(controls);
            sample[0] *= gain;
            sample[1] *= gain;
            if let Some(panner) = &mut state.panner {
                sample = panner.process(sample, controls);
            }
            if channel.mute.value(controls) >= 0.5 {
                sample = [0.0, 0.0];
            }
            let post_fader = sample;
            channel_taps[index] = ChannelTapSamples {
                source,
                pre_fader,
                post_fader,
            };
            meters[index].observe(post_fader);
            if audition && self.cue == Some((index, TapPoint::PreFader)) {
                cue_output = Some(pre_fader);
            } else if audition && self.cue == Some((index, TapPoint::PostFader)) {
                cue_output = Some(post_fader);
            }
            let audible = !any_solo
                || self
                    .soloed
                    .iter()
                    .enumerate()
                    .any(|(selected, enabled)| *enabled && program.solo_masks[selected][index]);
            for route_index in &channel.outgoing_routes {
                let route = &program.routes[*route_index];
                let mut routed = match route.tap {
                    TapPoint::PreFader => pre_fader,
                    TapPoint::PostFader => post_fader,
                };
                if !audible {
                    routed = [0.0, 0.0];
                }
                if let Some(gain) = &mut route_gains[*route_index] {
                    let gain = gain.coefficient(controls);
                    routed[0] *= gain;
                    routed[1] *= gain;
                }
                routed = route_delays[*route_index].process(routed);
                match route.destination {
                    RouteDestination::Bus(destination) => {
                        bus_inputs[destination][0] += routed[0];
                        bus_inputs[destination][1] += routed[1];
                    }
                    RouteDestination::Auxiliary(slot) => auxiliary_inputs[slot] = routed,
                }
            }
            if index == program.master_channel {
                master = post_fader;
            }
        }
        cue_output.unwrap_or(master)
    }

    /// Resets all time-dependent state after an explicit transport seek.
    ///
    /// Realtime publication preparation calls this on the controller thread; it
    /// clears existing storage and never replaces heap-owning state.
    pub(crate) fn reset(&mut self) {
        #[cfg(test)]
        {
            self.reset_calls = self.reset_calls.saturating_add(1);
        }
        for runtime in &mut self.channels {
            runtime.source_delay.reset();
            for processor in runtime
                .source_processors
                .iter_mut()
                .chain(&mut runtime.inserts)
            {
                processor.processor.reset();
            }
            runtime.fader.reset();
            if let Some(panner) = &mut runtime.panner {
                panner.reset();
            }
        }
        for delay in &mut self.route_delays {
            delay.reset();
        }
        for gain in self.route_gains.iter_mut().flatten() {
            gain.reset();
        }
        self.bus_inputs.fill([0.0, 0.0]);
        self.auxiliary_inputs.fill([0.0, 0.0]);
        self.meters.fill(ChannelMeter::default());
        self.processor_meters.fill(ProcessorMeter::default());
        self.channel_taps.fill(ChannelTapSamples::default());
    }

    #[cfg(test)]
    pub(crate) const fn reset_calls(&self) -> usize {
        self.reset_calls
    }

    /// Enables or disables one transient solo without changing source or render state.
    pub(crate) fn set_solo(&mut self, identity: &str, enabled: bool) -> bool {
        let Some(index) = self
            .program
            .channels
            .iter()
            .position(|channel| channel.identity == identity)
        else {
            return false;
        };
        self.soloed[index] = enabled;
        true
    }

    /// Selects one transient cue tap, or clears cue when `identity` is `None`.
    pub(crate) fn set_cue(&mut self, identity: Option<&str>, tap: TapPoint) -> bool {
        let Some(identity) = identity else {
            self.cue = None;
            return true;
        };
        let Some(index) = self
            .program
            .channels
            .iter()
            .position(|channel| channel.identity == identity)
        else {
            return false;
        };
        self.cue = Some((index, tap));
        true
    }

    /// Sets or clears a monitoring-only bypass override on every runtime instance
    /// of one song-wide durable processor ID.
    pub(crate) fn set_processor_bypass(
        &mut self,
        id: &str,
        bypass: Option<bool>,
        controls: &[f64],
    ) -> usize {
        let mut changed = 0;
        let meters = &mut self.processor_meters;
        for processor in self.channels.iter_mut().flat_map(|channel| {
            channel
                .source_processors
                .iter_mut()
                .chain(&mut channel.inserts)
        }) {
            if processor.processor.id() == id {
                processor.processor.set_transient_bypass(bypass);
                let (authored, transient, effective, mix) =
                    processor.processor.bypass_state(controls, true);
                if let Some(meter) = meters.get_mut(processor.telemetry_index) {
                    meter.authored_bypass = authored;
                    meter.transient_bypass = transient;
                    meter.effective_bypass = effective;
                    meter.bypass_mix = mix;
                }
                changed += 1;
            }
        }
        changed
    }

    /// Moves compatible shared DSP, compensation, meter, and audition state.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn adopt_state_from(&mut self, current: &mut Self) {
        if !self.program.topology_compatible(&current.program) {
            return;
        }
        std::mem::swap(&mut self.channels, &mut current.channels);
        std::mem::swap(&mut self.route_delays, &mut current.route_delays);
        std::mem::swap(&mut self.route_gains, &mut current.route_gains);
        std::mem::swap(&mut self.meters, &mut current.meters);
        std::mem::swap(&mut self.processor_meters, &mut current.processor_meters);
        std::mem::swap(&mut self.channel_taps, &mut current.channel_taps);
        std::mem::swap(&mut self.soloed, &mut current.soloed);
        std::mem::swap(&mut self.cue, &mut current.cue);
    }

    pub(crate) fn channel_index(&self, identity: &str) -> Option<usize> {
        self.program
            .channels
            .iter()
            .position(|channel| channel.identity == identity)
    }

    pub(crate) fn channel_tap(&self, index: usize, tap: TapPoint) -> Option<[f32; 2]> {
        self.channel_taps.get(index).map(|samples| match tap {
            TapPoint::PreFader => samples.pre_fader,
            TapPoint::PostFader => samples.post_fader,
        })
    }

    pub(crate) fn source_tap(&self, index: usize) -> Option<[f32; 2]> {
        self.channel_taps.get(index).map(|samples| samples.source)
    }

    pub(crate) fn channel_activity(&self) -> ([ChannelMeter; MAX_MIX_CHANNELS], usize) {
        let mut activity = [ChannelMeter::default(); MAX_MIX_CHANNELS];
        let count = self.meters.len().min(MAX_MIX_CHANNELS);
        activity[..count].copy_from_slice(&self.meters[..count]);
        (activity, count)
    }

    pub(crate) fn copy_processor_activity(
        &self,
        activity: &mut [ProcessorMeter; MAX_PROCESSOR_INSTANCES],
    ) -> usize {
        let count = self.processor_meters.len().min(MAX_PROCESSOR_INSTANCES);
        activity[..count].copy_from_slice(&self.processor_meters[..count]);
        count
    }

    pub(crate) fn clear_current_activity(&mut self) {
        for meter in &mut self.meters {
            meter.current_peak = 0.0;
        }
        for meter in &mut self.processor_meters {
            meter.gain_reduction_db = 0.0;
        }
    }

    #[cfg(test)]
    pub(crate) fn heap_storage_fingerprint(&self) -> u64 {
        fn include(seed: u64, pointer: *const (), capacity: usize) -> u64 {
            seed.rotate_left(7) ^ (pointer as usize as u64) ^ (capacity as u64).rotate_left(31)
        }
        let mut fingerprint = include(0xCBF2_9CE4_8422_2325, Arc::as_ptr(&self.program).cast(), 1);
        fingerprint = include(
            fingerprint,
            self.channels.as_ptr().cast(),
            self.channels.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.route_delays.as_ptr().cast(),
            self.route_delays.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.route_gains.as_ptr().cast(),
            self.route_gains.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.bus_inputs.as_ptr().cast(),
            self.bus_inputs.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.auxiliary_inputs.as_ptr().cast(),
            self.auxiliary_inputs.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.meters.as_ptr().cast(),
            self.meters.capacity(),
        );
        fingerprint = include(
            fingerprint,
            self.processor_meters.as_ptr().cast(),
            self.processor_meters.capacity(),
        );
        for channel in &self.channels {
            fingerprint = include(
                fingerprint,
                channel.source_delay.buffer.as_ptr().cast(),
                channel.source_delay.buffer.capacity(),
            );
            fingerprint = include(
                fingerprint,
                channel.source_processors.as_ptr().cast(),
                channel.source_processors.capacity(),
            );
            fingerprint = include(
                fingerprint,
                channel.inserts.as_ptr().cast(),
                channel.inserts.capacity(),
            );
            for processor in channel.source_processors.iter().chain(&channel.inserts) {
                fingerprint ^= processor
                    .processor
                    .heap_storage_fingerprint()
                    .rotate_left(13);
                fingerprint = include(
                    fingerprint,
                    processor.auxiliary_slots.as_ptr().cast(),
                    processor.auxiliary_slots.capacity(),
                );
            }
        }
        for delay in &self.route_delays {
            fingerprint = include(
                fingerprint,
                delay.buffer.as_ptr().cast(),
                delay.buffer.capacity(),
            );
        }
        fingerprint
    }

    #[cfg(test)]
    pub(crate) fn poison_track_level_for_test(&mut self, index: usize) {
        if let Some(channel_index) = self.program.track_channels.get(index).copied() {
            self.channels[channel_index].fader.scalar = MixScalar::Constant(f64::NAN);
        }
    }
}

fn process_processor(
    processor: &mut ProcessorInstanceRuntime,
    sample: [f32; 2],
    auxiliary_inputs: &[[f32; 2]],
    controls: &[f64],
    processor_meters: &mut [ProcessorMeter],
    audition: bool,
) -> [f32; 2] {
    let mut sidechain: Option<[f32; 2]> = None;
    for slot in &processor.auxiliary_slots {
        let input = auxiliary_inputs.get(*slot).copied().unwrap_or([0.0, 0.0]);
        if let Some(sum) = &mut sidechain {
            sum[0] += input[0];
            sum[1] += input[1];
        } else {
            sidechain = Some(input);
        }
    }
    let output = processor
        .processor
        .process_with_audition(sample, sidechain, controls, audition);
    if let Some(meter) = processor_meters.get_mut(processor.telemetry_index) {
        if let Some(gain_reduction_db) = processor.processor.gain_reduction_db() {
            meter.observe_gain_reduction(gain_reduction_db);
        }
        let (authored, transient, effective, mix) =
            processor.processor.bypass_state(controls, audition);
        meter.observe_processing(sample, output, authored, transient, effective, mix);
    }
    output
}

#[derive(Debug, Clone)]
struct CompensationDelay {
    buffer: Vec<[f32; 2]>,
    cursor: usize,
    frames: usize,
}

impl CompensationDelay {
    fn new(frames: usize) -> Self {
        Self {
            buffer: vec![[0.0, 0.0]; frames.max(1)],
            cursor: 0,
            frames,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill([0.0; 2]);
        self.cursor = 0;
    }

    fn process(&mut self, sample: [f32; 2]) -> [f32; 2] {
        if self.frames == 0 {
            return sample;
        }
        let output = self.buffer[self.cursor];
        self.buffer[self.cursor] = sample;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        output
    }
}

/// Callback-cheap processor activity accumulator aligned to processor inspection indexes.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct ProcessorMeter {
    /// Current compressor gain reduction in decibels; zero for other processor kinds.
    pub gain_reduction_db: f32,
    /// Maximum compressor gain reduction observed since runtime creation.
    pub maximum_gain_reduction_db: f32,
    /// Current absolute stereo input peak.
    pub input_peak: f32,
    /// Current absolute stereo output peak.
    pub output_peak: f32,
    /// Authored bypass value at the current control frame.
    pub authored_bypass: bool,
    /// Monitoring-only override, when active.
    pub transient_bypass: Option<bool>,
    /// Effective bypass target for the current processing mode.
    pub effective_bypass: bool,
    /// Click-guard blend, where zero is wet and one is latency-aligned dry.
    pub bypass_mix: f32,
}

impl ProcessorMeter {
    fn observe_gain_reduction(&mut self, gain_reduction_db: f32) {
        self.gain_reduction_db = gain_reduction_db.max(0.0);
        self.maximum_gain_reduction_db = self.maximum_gain_reduction_db.max(self.gain_reduction_db);
    }

    fn observe_processing(
        &mut self,
        input: [f32; 2],
        output: [f32; 2],
        authored: bool,
        transient: Option<bool>,
        effective: bool,
        mix: f32,
    ) {
        self.input_peak = input[0].abs().max(input[1].abs());
        self.output_peak = output[0].abs().max(output[1].abs());
        self.authored_bypass = authored;
        self.transient_bypass = transient;
        self.effective_bypass = effective;
        self.bypass_mix = mix;
    }
}

/// Callback-cheap channel activity accumulator.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelMeter {
    /// Current absolute stereo peak.
    pub current_peak: f32,
    /// Maximum absolute peak since runtime creation.
    pub peak: f32,
    /// Exponential moving average of stereo absolute activity.
    pub average: f32,
}

impl ChannelMeter {
    fn observe(&mut self, sample: [f32; 2]) {
        let peak = sample[0].abs().max(sample[1].abs());
        self.current_peak = peak;
        self.peak = self.peak.max(peak);
        self.average += (peak - self.average) * 0.001;
    }
}

fn resolved_processor_settings(
    processor: &crate::model::Processor,
    tempo_bpm: f64,
) -> Result<Vec<CompiledProcessorSettingInspection>, EngineError> {
    let settings = resolve_settings(processor, tempo_bpm)
        .map_err(|error| EngineError::Compile(error.compile_message(processor)))?;
    Ok(settings
        .iter()
        .map(|setting| {
            let numeric = setting.descriptor.contract.numeric();
            let (unit, minimum, maximum) =
                numeric.map_or((None, None, None), |(unit, minimum, maximum, _)| {
                    (
                        Some(unit),
                        (!setting.derived).then_some(minimum),
                        (!setting.derived).then_some(maximum),
                    )
                });
            let (value, choice) = match &setting.value {
                ResolvedSettingValue::Numeric(value) => (Some(*value), None),
                ResolvedSettingValue::Choice(choice) => (None, Some(choice.clone())),
                ResolvedSettingValue::Unsigned(value) => (None, Some(value.to_string())),
            };
            CompiledProcessorSettingInspection {
                key: setting.descriptor.key.to_owned(),
                unit,
                value,
                choice,
                minimum,
                maximum,
                authored: setting.authored,
                parameter_target: setting.parameter_target(),
            }
        })
        .collect())
}

fn processor_telemetry_indices(
    project: &Project,
) -> Result<BTreeMap<(String, String), usize>, EngineError> {
    let instruments = project
        .song
        .instruments
        .iter()
        .map(|instrument| (instrument.id.as_str(), instrument))
        .collect::<BTreeMap<_, _>>();
    let mut identities = BTreeSet::new();
    let mut insert_chain = |owner: String, processors: &[crate::model::Processor]| {
        for processor in processors {
            if !identities.insert((owner.clone(), processor.id.clone())) {
                return Err(EngineError::Compile(format!(
                    "duplicate processor telemetry identity `{owner}/{}`",
                    processor.id
                )));
            }
        }
        Ok(())
    };
    for track in &project.song.tracks {
        if let Some(instrument) = instruments.get(track.instrument.as_str()) {
            insert_chain(
                format!("instrument/{}@{}", instrument.id, track.id),
                &instrument.post_processors,
            )?;
        }
        insert_chain(format!("track/{}", track.id), &track.channel.inserts)?;
    }
    for group in &project.song.groups {
        insert_chain(format!("group/{}", group.id), &group.channel.inserts)?;
    }
    for channel in &project.song.returns {
        insert_chain(format!("return/{}", channel.id), &channel.channel.inserts)?;
    }
    if let Some(master) = &project.song.master {
        insert_chain("master".to_owned(), &master.inserts)?;
    }
    Ok(identities
        .into_iter()
        .enumerate()
        .map(|(index, identity)| (identity, index))
        .collect())
}

fn compile_processor_chain(
    processors: &[crate::model::Processor],
    owner: &str,
    controls: &CompiledControlProgram,
    sample_rate: u32,
    tempo_bpm: f64,
    processor_indexes: &BTreeMap<(String, String), usize>,
    inspection: &mut Vec<CompiledProcessorInspection>,
) -> Result<Vec<CompiledProcessorInstance>, EngineError> {
    processors
        .iter()
        .map(|processor| {
            let program = ProcessorProgram::compile(processor, controls, sample_rate, tempo_bpm)?;
            let telemetry_index = *processor_indexes
                .get(&(owner.to_owned(), processor.id.clone()))
                .ok_or_else(|| {
                    EngineError::Compile(format!(
                        "processor `{}` has no stable telemetry index for `{owner}`",
                        processor.id
                    ))
                })?;
            inspection.push(CompiledProcessorInspection {
                index: telemetry_index,
                id: processor.id.clone(),
                kind: processor.kind,
                owner: owner.to_owned(),
                authored_bypass: processor.bypass,
                state_policy: ProcessorStatePolicy::Continue,
                settings: resolved_processor_settings(processor, tempo_bpm)?,
                sidechain_inputs: processor.sidechain_inputs.clone(),
                authored_latency_frames: program.authored_latency_frames(),
                intrinsic_latency_frames: program.intrinsic_latency_frames(),
                output_latency_frames: program.output_latency_frames(),
                nonlinear_substeps: program.nonlinear_substeps(),
                oversampling_factor: program.oversampling_factor(),
                oversampling_filter_taps: program.oversampling_filter_taps(),
                true_peak_safety_margin_db: program.true_peak_safety_margin_db(),
                state_bytes: program.state_bytes(),
                work_units_per_frame: program.work_units(),
            });
            Ok(CompiledProcessorInstance {
                program,
                auxiliary_slots: Vec::new(),
                telemetry_index,
            })
        })
        .collect()
}

fn compile_sends(
    sends: &[crate::model::Send],
    controls: &CompiledControlProgram,
) -> Result<Vec<SendDraft>, EngineError> {
    sends
        .iter()
        .map(|send| {
            Ok(SendDraft {
                id: send.id.clone(),
                target: send.target.clone(),
                tap: send.tap,
                gain: MixScalar::from_target(
                    controls,
                    &ParameterTarget::Send {
                        send: send.id.clone(),
                    },
                )?,
            })
        })
        .collect()
}

fn topological_order<'a>(
    identities: impl Iterator<Item = &'a String>,
    routes: &[RouteDraft],
) -> Result<Vec<String>, EngineError> {
    let mut indegree = BTreeMap::new();
    let mut outgoing: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for identity in identities {
        indegree.insert(identity.clone(), 0_usize);
        outgoing.insert(identity.clone(), BTreeSet::new());
    }
    for route in routes {
        if outgoing
            .entry(route.from.clone())
            .or_default()
            .insert(route.owner.clone())
        {
            *indegree.entry(route.owner.clone()).or_default() += 1;
        }
    }
    let mut ready: BTreeSet<_> = indegree
        .iter()
        .filter_map(|(identity, degree)| (*degree == 0).then_some(identity.clone()))
        .collect();
    let mut order = Vec::with_capacity(indegree.len());
    while let Some(identity) = ready.pop_first() {
        order.push(identity.clone());
        if let Some(destinations) = outgoing.get(&identity) {
            for destination in destinations {
                let degree = indegree.get_mut(destination).ok_or_else(|| {
                    EngineError::Compile(format!("route destination `{destination}` is missing"))
                })?;
                *degree = degree.saturating_sub(1);
                if *degree == 0 {
                    ready.insert(destination.clone());
                }
            }
        }
    }
    if order.len() != indegree.len() {
        return Err(EngineError::Compile(
            "compiled channel graph contains a route cycle".to_owned(),
        ));
    }
    Ok(order)
}

fn compile_latency(
    channels: &mut [CompiledChannel],
    routes: &mut [CompiledRoute],
) -> Result<(), EngineError> {
    for channel_index in 0..channels.len() {
        let mut input_latency = 0_u32;
        for route in routes.iter().filter(|route| {
            route.owner == channel_index && matches!(route.destination, RouteDestination::Bus(_))
        }) {
            input_latency = input_latency.max(route_source_latency(channels, route));
        }
        for route in routes.iter().filter(|route| {
            route.owner == channel_index
                && matches!(route.destination, RouteDestination::Auxiliary(_))
        }) {
            let processor_index = route.processor_index.ok_or_else(|| {
                EngineError::Compile(format!("sidechain route `{}` has no processor", route.id))
            })?;
            let offset = channels[channel_index].processor_input_offset(processor_index);
            input_latency =
                input_latency.max(route_source_latency(channels, route).saturating_sub(offset));
        }
        if channels[channel_index].source_index.is_some() {
            channels[channel_index].source_compensation_frames = input_latency;
        }
        for route in routes
            .iter_mut()
            .filter(|route| route.owner == channel_index)
        {
            let arrival = match route.tap {
                TapPoint::PreFader => channels[route.from].pre_latency_frames,
                TapPoint::PostFader => channels[route.from].post_latency_frames,
            };
            let target = if let Some(processor_index) = route.processor_index {
                input_latency
                    .saturating_add(channels[channel_index].processor_input_offset(processor_index))
            } else {
                input_latency
            };
            route.compensation_frames = target.checked_sub(arrival).ok_or_else(|| {
                EngineError::Compile(format!(
                    "latency convergence for route `{}` could not be represented",
                    route.id
                ))
            })?;
        }
        let output_latency =
            input_latency.saturating_add(channels[channel_index].total_processor_latency());
        channels[channel_index].pre_latency_frames = output_latency;
        channels[channel_index].post_latency_frames = output_latency;
    }
    Ok(())
}

fn route_source_latency(channels: &[CompiledChannel], route: &CompiledRoute) -> u32 {
    match route.tap {
        TapPoint::PreFader => channels[route.from].pre_latency_frames,
        TapPoint::PostFader => channels[route.from].post_latency_frames,
    }
}

fn compile_solo_masks(
    channels: &[CompiledChannel],
    routes: &[CompiledRoute],
    master: usize,
) -> Vec<Vec<bool>> {
    let mut masks = Vec::with_capacity(channels.len());
    for selected in 0..channels.len() {
        let mut mask = vec![false; channels.len()];
        mask[selected] = true;
        // A Group solo includes descendants on main-output paths, but never
        // walks backward through Master and accidentally selects all channels.
        let mut changed = true;
        while changed {
            changed = false;
            for route in routes.iter().filter(|route| route.role == RouteRole::Main) {
                if route.owner != master && mask[route.owner] && !mask[route.from] {
                    mask[route.from] = true;
                    changed = true;
                }
            }
        }
        // Preserve every selected channel's main ancestry and Return branches.
        changed = true;
        while changed {
            changed = false;
            for route in routes
                .iter()
                .filter(|route| route.role != RouteRole::Sidechain)
            {
                if mask[route.from] && !mask[route.owner] {
                    mask[route.owner] = true;
                    changed = true;
                }
            }
        }
        mask[master] = true;
        masks.push(mask);
    }
    masks
}

fn compile_inspection(
    channels: &[CompiledChannel],
    routes: &[CompiledRoute],
    mut processors: Vec<CompiledProcessorInspection>,
    master_channel: usize,
    resources: RuntimeResourceEstimate,
) -> MixGraphInspection {
    let mut channel_inspection = channels
        .iter()
        .enumerate()
        .map(|(index, channel)| CompiledChannelInspection {
            index,
            identity: channel.identity.clone(),
            role: channel.role,
            panner: channel.panner.map(CompiledPanner::mode),
            mute: channel.mute.initial() >= 0.5,
            processors: channel
                .source_processors
                .iter()
                .chain(&channel.inserts)
                .map(|processor| processor.program.id.clone())
                .collect(),
            source_compensation_frames: channel.source_compensation_frames,
            output_latency_frames: channel.post_latency_frames,
        })
        .collect::<Vec<_>>();
    channel_inspection.sort_by_key(|channel| channel.index);
    processors.sort_by_key(|processor| processor.index);
    let route_inspection = routes
        .iter()
        .map(|route| CompiledRouteInspection {
            id: route.id.clone(),
            role: route.role,
            from: channels[route.from].identity.clone(),
            to: route.destination_display.clone(),
            tap: route.tap,
            layout: "stereo".to_owned(),
            compensation_frames: route.compensation_frames,
        })
        .collect::<Vec<_>>();
    let mut signal_paths = Vec::new();
    for (source_index, track_channel) in channels
        .iter()
        .enumerate()
        .filter(|(_, channel)| channel.role == ChannelRole::Track)
    {
        let mut stages = Vec::new();
        append_main_signal_path(
            channels,
            routes,
            source_index,
            0,
            true,
            master_channel,
            &mut stages,
        );
        signal_paths.push(SignalPathInspection {
            source: track_channel.identity.clone(),
            stages,
            total_latency_frames: channels[master_channel].post_latency_frames,
        });
    }
    for route in routes.iter().filter(|route| route.role != RouteRole::Main) {
        let source = &channels[route.from];
        let mut stages = source
            .source_processors
            .iter()
            .chain(&source.inserts)
            .map(|processor| format!("processor/{}", processor.program.id))
            .collect::<Vec<_>>();
        stages.extend([
            source.identity.clone(),
            format!("{:?}", route.tap).to_lowercase(),
            route.id.clone(),
        ]);
        append_main_signal_path(
            channels,
            routes,
            route.owner,
            route.processor_index.unwrap_or(0),
            route.processor_index.is_none(),
            master_channel,
            &mut stages,
        );
        signal_paths.push(SignalPathInspection {
            source: route.id.clone(),
            stages,
            total_latency_frames: channels[master_channel].post_latency_frames,
        });
    }
    signal_paths.sort_by(|left, right| left.source.cmp(&right.source));
    MixGraphInspection {
        channels: channel_inspection,
        routes: route_inspection,
        processors,
        signal_paths,
        resources,
    }
}

fn append_main_signal_path(
    channels: &[CompiledChannel],
    routes: &[CompiledRoute],
    mut current: usize,
    first_insert: usize,
    include_first_source_processors: bool,
    master_channel: usize,
    stages: &mut Vec<String>,
) {
    let mut visited = BTreeSet::new();
    let mut first = true;
    loop {
        if !visited.insert(current) {
            break;
        }
        let channel = &channels[current];
        if !first || include_first_source_processors {
            stages.extend(
                channel
                    .source_processors
                    .iter()
                    .map(|processor| format!("processor/{}", processor.program.id)),
            );
        }
        stages.extend(
            channel
                .inserts
                .iter()
                .skip(if first { first_insert } else { 0 })
                .map(|processor| format!("processor/{}", processor.program.id)),
        );
        stages.push(channel.identity.clone());
        if current == master_channel {
            break;
        }
        let Some(route) = routes
            .iter()
            .find(|route| route.from == current && route.role == RouteRole::Main)
        else {
            break;
        };
        let RouteDestination::Bus(destination) = route.destination else {
            break;
        };
        current = destination;
        first = false;
    }
}

fn chains_compatible(
    left: &[CompiledProcessorInstance],
    right: &[CompiledProcessorInstance],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.program.topology_compatible(&right.program)
                && left.auxiliary_slots == right.auxiliary_slots
                && left.telemetry_index == right.telemetry_index
        })
}

fn channel_and_master_processors(
    project: &Project,
) -> impl Iterator<Item = &crate::model::Processor> {
    project
        .song
        .tracks
        .iter()
        .flat_map(|track| &track.channel.inserts)
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

fn db_gain(decibels: f64) -> f32 {
    10.0_f64.powf(decibels.clamp(-192.0, 96.0) / 20.0) as f32
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{
        dsl::parse_project,
        model::{OutputTarget, Panner},
        validate::validate_project,
    };

    use super::*;

    fn project(track_order: &str) -> Project {
        let source = format!(
            "crump 1\nproject \"Mix\" {{\n  id mix\n  seed 1\n  sample_rate 48000\n  render_tail 0s\n  song {{\n    tempo 120\n    meter 4/4\n    end 2bar\n    instrument drums \"Drums\" stock_drums {{\n      polyphony 8\n      parameter tone \"Tone\" ratio 0 1 0.5 0.5\n      faceplate {{\n        control tone \"Tone\" \"Percussion brightness\" maps tone\n      }}\n    }}\n    pattern beat 1bar {{\n      hit 1.1 kick 1\n    }}\n{track_order}\n  }}\n}}\n"
        );
        let parsed = parse_project(Path::new("project.crump"), &source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("Project");
        let diagnostics = validate_project(&project);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        project
    }

    #[test]
    fn packed_boolean_admission_rounds_to_machine_words() {
        let word = std::mem::size_of::<usize>() as u64;
        assert_eq!(vec_bool_allocation_bytes(0), 0);
        assert_eq!(vec_bool_allocation_bytes(1), word);
        assert_eq!(vec_bool_allocation_bytes(usize::BITS as usize), word);
        assert_eq!(
            vec_bool_allocation_bytes(usize::BITS as usize + 1),
            word * 2
        );
        assert_eq!(vec_bool_allocation_bytes(97), word * 2);
    }

    #[test]
    fn stable_track_identity_controls_float_summing_order() {
        let first = project(
            "    track b \"B\" instrument drums level -1db pan 0 {\n      clip cb pattern beat start 1.1 duration 1bar mode once\n    }\n    track a \"A\" instrument drums level -2db pan -0.25 {\n      clip ca pattern beat start 1.1 duration 1bar mode once\n    }",
        );
        let second = project(
            "    track a \"A\" instrument drums level -2db pan -0.25 {\n      clip ca pattern beat start 1.1 duration 1bar mode once\n    }\n    track b \"B\" instrument drums level -1db pan 0 {\n      clip cb pattern beat start 1.1 duration 1bar mode once\n    }",
        );
        let first_controls = CompiledControlProgram::compile(&first).expect("controls");
        let second_controls = CompiledControlProgram::compile(&second).expect("controls");
        let first_mix = CompiledMixGraph::compile(&first, &first_controls).expect("mix");
        let second_mix = CompiledMixGraph::compile(&second, &second_controls).expect("mix");
        assert_eq!(first_mix.inspection.routes, second_mix.inspection.routes);
        assert_eq!(
            first_mix.inspection.signal_paths,
            second_mix.inspection.signal_paths
        );
    }

    #[test]
    fn compilation_rejects_unconnected_duplicate_or_undeclared_sidechain_inputs() {
        let parsed = parse_project(
            Path::new("production_mix.crump"),
            include_str!("../../tests/fixtures/production_mix.crump"),
        );
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("Project");

        let mut disconnected = project.clone();
        disconnected.song.sidechains.clear();
        let controls = CompiledControlProgram::compile(&disconnected).expect("controls");
        let error = CompiledMixGraph::compile(&disconnected, &controls)
            .expect_err("declared sidechain without a route must not compile");
        assert!(error.to_string().contains("unconnected declared sidechain"));

        let mut duplicate = project.clone();
        let mut duplicate_route = duplicate.song.sidechains[0].clone();
        duplicate_route.id = "duplicate_detector".to_owned();
        duplicate.song.sidechains.push(duplicate_route);
        let controls = CompiledControlProgram::compile(&duplicate).expect("controls");
        let error = CompiledMixGraph::compile(&duplicate, &controls)
            .expect_err("two routes to one sidechain input must not compile");
        assert!(error.to_string().contains("more than one sidechain route"));

        let mut undeclared = project;
        undeclared.song.sidechains[0].input = "missing".to_owned();
        let controls = CompiledControlProgram::compile(&undeclared).expect("controls");
        let error = CompiledMixGraph::compile(&undeclared, &controls)
            .expect_err("route to an undeclared sidechain input must not compile");
        assert!(error.to_string().contains("targets undeclared input"));
    }

    #[test]
    fn integrated_sidechain_fixture_creates_kick_driven_bass_movement() {
        let parsed = parse_project(
            Path::new("compressor_sidechain.crump"),
            include_str!("../../tests/fixtures/compressor_sidechain.crump"),
        );
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let mut project = parsed.project.expect("Project");
        project
            .song
            .tracks
            .iter_mut()
            .find(|track| track.id == "kick")
            .expect("kick Track")
            .channel
            .level_db = -96.0;
        let diagnostics = validate_project(&project);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let graph = CompiledMixGraph::compile(&project, &controls).expect("mix graph");
        let control_runtime = controls.runtime();
        let control_values = control_runtime.values();
        assert!(graph.inspection.routes.iter().any(|route| {
            route.role == RouteRole::Sidechain
                && route.from == "track/kick"
                && route.to == "processor/bass_comp/detector"
                && route.tap == TapPoint::PreFader
        }));
        let compressor_index = graph
            .inspection
            .processors
            .iter()
            .find(|processor| processor.id == "bass_comp")
            .expect("bass compressor")
            .index;
        let mut runtime = graph.runtime();
        let bass = [0.25, -0.25];
        let silence = [0.0, 0.0];
        let kick = [1.0, 1.0];
        let mut baseline = [0.0, 0.0];
        for _ in 0..480 {
            baseline = runtime.process(&[bass, silence], control_values, false);
        }
        let mut ducked = baseline;
        for _ in 0..480 {
            ducked = runtime.process(&[bass, kick], control_values, false);
        }
        assert!(ducked[0].abs() < baseline[0].abs() * 0.14);
        assert!(runtime.processor_meters[compressor_index].gain_reduction_db > 17.9);

        let mut recovering = ducked;
        for _ in 0..4_800 {
            recovering = runtime.process(&[bass, silence], control_values, false);
        }
        assert!(recovering[0].abs() > ducked[0].abs() * 1.9);
        assert!(recovering[0].abs() < baseline[0].abs() * 0.4);
        let mut recovered = recovering;
        for _ in 4_800..14_400 {
            recovered = runtime.process(&[bass, silence], control_values, false);
        }
        assert!((recovered[0] - baseline[0]).abs() < 1.0e-5);
        assert!(runtime.processor_meters[compressor_index].gain_reduction_db < 1.0e-4);
        assert!(runtime.processor_meters[compressor_index].maximum_gain_reduction_db > 17.9);
    }

    #[test]
    fn processor_telemetry_indexes_survive_declaration_reordering() {
        let parsed = parse_project(
            Path::new("production_mix.crump"),
            include_str!("../../tests/fixtures/production_mix.crump"),
        );
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let mut first = parsed.project.expect("Project");

        let mut extra_group = first.song.groups[0].clone();
        extra_group.id = "aux_group".to_owned();
        extra_group.channel.sends.clear();
        for processor in &mut extra_group.channel.inserts {
            processor.id = format!("aux_{}", processor.id);
        }
        first.song.groups.push(extra_group);
        let mut extra_return = first.song.returns[0].clone();
        extra_return.id = "aux_return".to_owned();
        for processor in &mut extra_return.channel.inserts {
            processor.id = format!("aux_{}", processor.id);
        }
        first.song.returns.push(extra_return);

        let mut second = first.clone();
        second.song.groups.reverse();
        second.song.returns.reverse();
        for project in [&first, &second] {
            let diagnostics = validate_project(project);
            assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        }
        let first_controls = CompiledControlProgram::compile(&first).expect("controls");
        let second_controls = CompiledControlProgram::compile(&second).expect("controls");
        let first_graph = CompiledMixGraph::compile(&first, &first_controls).expect("mix");
        let second_graph = CompiledMixGraph::compile(&second, &second_controls).expect("mix");
        assert!(
            first_graph
                .program
                .topology_compatible(&second_graph.program)
        );

        let first_indexes = first_graph
            .inspection
            .processors
            .iter()
            .map(|processor| {
                (
                    (processor.owner.clone(), processor.id.clone()),
                    processor.index,
                )
            })
            .collect::<BTreeMap<_, _>>();
        let second_indexes = second_graph
            .inspection
            .processors
            .iter()
            .map(|processor| {
                (
                    (processor.owner.clone(), processor.id.clone()),
                    processor.index,
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(first_indexes, second_indexes);

        let expected_index = first_indexes
            .get(&("group/aux_group".to_owned(), "aux_bus_eq".to_owned()))
            .copied()
            .expect("auxiliary Group processor index");
        let mut current = first_graph.runtime();
        current.processor_meters[expected_index].gain_reduction_db = 7.5;
        let mut replacement = second_graph.runtime();
        replacement.adopt_state_from(&mut current);
        let published_index = second_indexes
            .get(&("group/aux_group".to_owned(), "aux_bus_eq".to_owned()))
            .copied()
            .expect("published auxiliary Group processor index");
        assert_eq!(
            replacement.processor_meters[published_index].gain_reduction_db,
            7.5
        );
    }

    #[test]
    fn estimate_includes_voice_nodes_buffers_and_transition_headroom() {
        let project = project(
            "    track drums \"Drums\" instrument drums level 0db pan 0 {\n      clip beat_clip pattern beat start 1.1 duration 1bar mode once\n    }",
        );
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let graph = CompiledMixGraph::compile(&project, &controls).expect("mix");
        let resources = &graph.inspection.resources;
        assert_eq!(resources.channels, 2);
        assert!(resources.bus_buffer_bytes > 0);
        assert!(resources.meter_bytes > 0);
        assert_eq!(
            resources.publication_peak_bytes,
            resources.runtime_bytes * 2
        );
        assert!(resources.admitted);
    }

    #[test]
    fn production_fixture_compiles_every_slice_two_route_role() {
        let source = include_str!("../../tests/fixtures/production_mix.crump");
        let parsed = parse_project(Path::new("production_mix.crump"), source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("Project");
        let diagnostics = validate_project(&project);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        for processor in project
            .song
            .instruments
            .iter()
            .flat_map(|instrument| &instrument.post_processors)
            .chain(channel_and_master_processors(&project))
        {
            let program = ProcessorProgram::compile(
                processor,
                &controls,
                project.sample_rate,
                project.song.tempo_bpm,
            )
            .expect("DSP");
            assert_eq!(
                authored_processor_estimate(processor, project.sample_rate, project.song.tempo_bpm,),
                (program.state_bytes(), program.work_units()),
                "authored admission estimate for {}",
                processor.id
            );
        }
        let graph = CompiledMixGraph::compile(&project, &controls).expect("mix");
        let inspection = graph.inspection();
        let processor_kinds = inspection
            .processors
            .iter()
            .map(|processor| processor.kind)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            processor_kinds,
            [
                ProcessorKind::Utility,
                ProcessorKind::Eq,
                ProcessorKind::Compressor,
                ProcessorKind::Delay,
                ProcessorKind::Reverb,
                ProcessorKind::Saturation,
                ProcessorKind::Limiter,
                ProcessorKind::Latency,
            ]
            .into_iter()
            .collect()
        );
        assert!(
            inspection
                .channels
                .iter()
                .any(|channel| channel.role == ChannelRole::Group)
        );
        assert!(
            inspection
                .channels
                .iter()
                .any(|channel| channel.role == ChannelRole::Return)
        );
        assert!(
            inspection
                .routes
                .iter()
                .any(|route| route.role == RouteRole::Send)
        );
        assert!(
            inspection
                .routes
                .iter()
                .any(|route| route.role == RouteRole::Sidechain)
        );
        assert!(
            inspection
                .channels
                .iter()
                .any(|channel| channel.source_compensation_frames > 0)
        );
        assert_eq!(inspection.resources.supported_quanta, [64, 128, 256]);
        assert!(inspection.resources.admitted);
        assert_eq!(
            inspection.resources.compiled_program_bytes,
            graph.program.allocation_bytes()
        );
        assert_eq!(
            inspection.resources.runtime_structure_bytes,
            graph.program.runtime_structure_bytes()
        );
        assert_eq!(
            inspection.resources.runtime_bytes,
            inspection
                .resources
                .bus_buffer_bytes
                .saturating_add(inspection.resources.meter_bytes)
                .saturating_add(inspection.resources.processor_meter_bytes)
                .saturating_add(inspection.resources.compiled_program_bytes)
                .saturating_add(inspection.resources.runtime_structure_bytes)
                .saturating_add(inspection.resources.graph_voice_state_bytes)
                .saturating_add(inspection.resources.drum_voice_state_bytes)
                .saturating_add(inspection.resources.sampler_voice_state_bytes)
                .saturating_add(inspection.resources.processor_state_bytes)
                .saturating_add(inspection.resources.compensation_state_bytes)
                .saturating_add(inspection.resources.control_program_bytes)
                .saturating_add(inspection.resources.schedule_program_bytes)
                .saturating_add(inspection.resources.instrument_program_bytes)
                .saturating_add(inspection.resources.song_runtime_structure_bytes)
        );
        assert_eq!(
            inspection.resources.processor_meter_bytes,
            (inspection.processors.len() + MAX_PROCESSOR_INSTANCES) as u64
                * std::mem::size_of::<ProcessorMeter>() as u64
        );
        for (index, processor) in inspection.processors.iter().enumerate() {
            assert_eq!(processor.index, index);
            assert!(processor.work_units_per_frame > 0);
        }
        let compressor = inspection
            .processors
            .iter()
            .find(|processor| processor.id == "bass_comp")
            .expect("compressor inspection");
        let threshold = compressor
            .settings
            .iter()
            .find(|setting| setting.key == "threshold")
            .expect("resolved threshold");
        assert_eq!(threshold.unit, Some(Unit::Decibels));
        assert_eq!(threshold.value, Some(-24.0));
        assert_eq!(threshold.minimum, Some(-96.0));
        assert!(threshold.authored);
        assert!(threshold.parameter_target);
        let detector = compressor
            .settings
            .iter()
            .find(|setting| setting.key == "detector")
            .expect("resolved default detector");
        assert_eq!(detector.choice.as_deref(), Some("peak"));
        assert!(!detector.authored);
        assert!(!detector.parameter_target);
        let reverb = inspection
            .processors
            .iter()
            .find(|processor| processor.id == "room_reverb")
            .expect("Reverb inspection");
        assert!(reverb.state_bytes >= (u64::from(project.sample_rate) + 1) * 8);
        let saturation = inspection
            .processors
            .iter()
            .find(|processor| processor.id == "master_saturation")
            .expect("Saturation inspection");
        assert_eq!(saturation.nonlinear_substeps, 4);
        assert_eq!(saturation.oversampling_factor, 4);
        assert_eq!(saturation.oversampling_filter_taps, 129);
        assert_eq!(saturation.intrinsic_latency_frames, 32);
        let limiter = inspection
            .processors
            .iter()
            .find(|processor| processor.id == "master_limiter")
            .expect("Limiter inspection");
        assert_eq!(limiter.authored_latency_frames, 0);
        assert_eq!(limiter.oversampling_factor, 4);
        assert_eq!(limiter.oversampling_filter_taps, 129);
        assert_eq!(limiter.true_peak_safety_margin_db, 0.1);
        assert_eq!(limiter.intrinsic_latency_frames, 256);
        assert_eq!(limiter.output_latency_frames, 256);
        let lookahead = limiter
            .settings
            .iter()
            .find(|setting| setting.key == "lookahead")
            .expect("resolved structural lookahead");
        assert!(lookahead.authored);
        assert!(!lookahead.parameter_target);
        let send_path = inspection
            .signal_paths
            .iter()
            .find(|path| path.source == "send/drum_room")
            .expect("Send path");
        assert!(send_path.stages.windows(3).any(|stages| stages
            == [
                "processor/room_reverb",
                "return/room",
                "processor/master_saturation"
            ]));
        assert_eq!(send_path.stages.last().map(String::as_str), Some("master"));
        let sidechain_path = inspection
            .signal_paths
            .iter()
            .find(|path| path.source == "sidechain/kick_detector")
            .expect("sidechain path");
        for stage in [
            "processor/bass_comp",
            "track/bass",
            "processor/bus_eq",
            "group/mix_bus",
            "processor/master_limiter",
            "master",
        ] {
            assert!(sidechain_path.stages.iter().any(|actual| actual == stage));
        }
        let mut runtime = graph.runtime();
        assert!(Arc::ptr_eq(&runtime.program, &graph.program));
        assert_eq!(Arc::strong_count(&graph.program), 2);
        let (activity, channel_count) = runtime.channel_activity();
        assert_eq!(channel_count, inspection.channels.len());
        assert_eq!(
            activity[..channel_count],
            vec![ChannelMeter::default(); channel_count]
        );
        let mut processor_activity = [ProcessorMeter::default(); MAX_PROCESSOR_INSTANCES];
        assert_eq!(
            runtime.copy_processor_activity(&mut processor_activity),
            inspection.processors.len()
        );
        assert_eq!(
            processor_activity[..inspection.processors.len()],
            vec![ProcessorMeter::default(); inspection.processors.len()]
        );

        let bass_source = graph
            .program
            .channels
            .iter()
            .find(|channel| channel.identity == "track/bass")
            .and_then(|channel| channel.source_index)
            .expect("Bass source");
        let drums_source = graph
            .program
            .channels
            .iter()
            .find(|channel| channel.identity == "track/drums")
            .and_then(|channel| channel.source_index)
            .expect("Drums source");
        let control_runtime = controls.runtime();
        let mut sources = vec![[0.0_f32, 0.0_f32]; graph.program.track_channels.len()];
        for _ in 0..2_000 {
            sources[bass_source] = [0.01, 0.01];
            sources[drums_source] = [1.0, 1.0];
            let _ = runtime.process(&sources, control_runtime.values(), false);
        }
        runtime.copy_processor_activity(&mut processor_activity);
        assert!(
            processor_activity[compressor.index].gain_reduction_db > 1.0,
            "external sidechain should drive measurable compressor reduction"
        );
    }

    #[test]
    fn inspection_resolves_defaults_for_every_processor_kind() {
        let parsed = parse_project(
            Path::new("production_mix.crump"),
            include_str!("../../tests/fixtures/production_mix.crump"),
        );
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let mut project = parsed.project.expect("Project");
        let mut delay = project.song.groups[0].channel.inserts[0].clone();
        delay.id = "inspection_delay".to_owned();
        delay.kind = ProcessorKind::Delay;
        delay.settings.clear();
        delay.sidechain_inputs.clear();
        delay.latency_frames = 0;
        delay.latency_site = None;
        project.song.groups[0].channel.inserts.push(delay.clone());
        let diagnostics = validate_project(&project);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");

        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let graph = CompiledMixGraph::compile(&project, &controls).expect("mix");
        let kinds = graph
            .inspection
            .processors
            .iter()
            .map(|processor| processor.kind)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            kinds,
            BTreeSet::from([
                ProcessorKind::Utility,
                ProcessorKind::Eq,
                ProcessorKind::Compressor,
                ProcessorKind::Delay,
                ProcessorKind::Reverb,
                ProcessorKind::Saturation,
                ProcessorKind::Limiter,
                ProcessorKind::Latency,
            ])
        );
        let inspected = graph
            .inspection
            .processors
            .iter()
            .find(|processor| processor.id == delay.id)
            .expect("Delay inspection");
        assert_eq!(
            inspected
                .settings
                .iter()
                .map(|setting| {
                    (
                        setting.key.as_str(),
                        setting.value,
                        setting.choice.as_deref(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("time", Some(0.25), None),
                ("tempo_sync", None, Some("false")),
                ("feedback", Some(0.25), None),
                ("mix", Some(0.25), None),
                ("low_pass", Some(20_000.0), None),
                ("high_pass", Some(20.0), None),
                ("time_crossfade", Some(0.01), None),
                ("mode", None, Some("stereo")),
            ]
        );
        assert!(
            inspected
                .settings
                .iter()
                .all(|setting| !setting.authored && !setting.parameter_target)
        );
        let program = ProcessorProgram::compile(
            &delay,
            &controls,
            project.sample_rate,
            project.song.tempo_bpm,
        )
        .expect("Delay DSP");
        assert_eq!(
            authored_processor_estimate(&delay, project.sample_rate, project.song.tempo_bpm,),
            (program.state_bytes(), program.work_units())
        );
        let reverb = &project.song.returns[0].channel.inserts[0];
        let program = ProcessorProgram::compile(
            reverb,
            &controls,
            project.sample_rate,
            project.song.tempo_bpm,
        )
        .expect("Reverb DSP");
        assert_eq!(
            authored_processor_estimate(reverb, project.sample_rate, project.song.tempo_bpm),
            (program.state_bytes(), program.work_units())
        );
    }

    #[test]
    fn tempo_synced_ping_pong_delay_resolves_time_and_exact_state() {
        let source = include_str!("../../tests/fixtures/production_mix.crump").replace(
            "    master level -1db {",
            "    master level -1db {\n      processor synced_delay delay {\n        setting tempo_sync true\n        setting division 1/8d\n        setting mode ping_pong\n        setting feedback 0.4\n        setting mix 0.5\n        setting time_crossfade 0.02s\n      }",
        );
        let parsed = parse_project(Path::new("synced-delay.crump"), &source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("Project");
        let diagnostics = validate_project(&project);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let graph = CompiledMixGraph::compile(&project, &controls).expect("mix graph");
        let delay = graph
            .inspection
            .processors
            .iter()
            .find(|processor| processor.id == "synced_delay")
            .expect("synced Delay");
        let setting = |key: &str| {
            delay
                .settings
                .iter()
                .find(|setting| setting.key == key)
                .unwrap_or_else(|| panic!("missing `{key}`"))
        };
        assert_eq!(setting("time").value, Some(0.375));
        assert!(!setting("time").authored);
        assert!(!setting("time").parameter_target);
        assert_eq!(setting("tempo_sync").choice.as_deref(), Some("true"));
        assert_eq!(setting("division").choice.as_deref(), Some("1/8d"));
        assert_eq!(setting("mode").choice.as_deref(), Some("ping_pong"));
        assert_eq!(setting("time_crossfade").value, Some(0.02));
        assert_eq!(
            delay.state_bytes,
            std::mem::size_of::<ProcessorRuntime>() as u64
                + (18_000_u64 + 2) * std::mem::size_of::<[f32; 2]>() as u64
                + 2 * std::mem::size_of::<[f32; 2]>() as u64
        );

        let conflicting = source.replace(
            "        setting division 1/8d",
            "        setting division 1/8d\n        setting time 0.2s",
        );
        let parsed = parse_project(Path::new("conflicting-delay.crump"), &conflicting);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("Project");
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let error = CompiledMixGraph::compile(&project, &controls)
            .expect_err("compiled Delay must reject free time while synchronized");
        assert!(error.to_string().contains("cannot author `time`"));

        let orphaned_division = source.replace(
            "        setting tempo_sync true",
            "        setting tempo_sync false",
        );
        let parsed = parse_project(Path::new("orphaned-division.crump"), &orphaned_division);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("Project");
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let error = CompiledMixGraph::compile(&project, &controls)
            .expect_err("compiled Delay must reject a division while tempo sync is disabled");
        assert!(error.to_string().contains("unless tempo sync is enabled"));
    }

    #[test]
    fn return_reverb_uses_one_shared_state_and_preserves_its_tail() {
        let project = project(
            "    track a \"A\" instrument drums level 0db pan 0 {\n      send a_room target return/room tap pre level 0db\n      mute true\n      output master\n      clip ca pattern beat start 1.1 duration 1bar mode once\n    }\n    track b \"B\" instrument drums level 0db pan 0 {\n      send b_room target return/room tap pre level 0db\n      mute true\n      output master\n      clip cb pattern beat start 1.1 duration 1bar mode once\n    }\n    return room \"Room\" level 0db {\n      processor shared_room reverb {\n        setting pre_delay 0s\n        setting diffusion 0.7\n        setting decay 0.85\n        setting damping 0.2\n        setting modulation 0.1\n        setting width 1\n        setting mix 1\n      }\n      output master\n    }\n    master level 0db {\n    }",
        );
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let graph = CompiledMixGraph::compile(&project, &controls).expect("mix graph");
        let room_index = graph
            .program
            .channels
            .iter()
            .position(|channel| channel.identity == "return/room")
            .expect("Room Return");
        assert_eq!(graph.program.channels[room_index].inserts.len(), 1);
        assert_eq!(
            graph
                .program
                .routes
                .iter()
                .filter(|route| {
                    route.role == RouteRole::Send
                        && route.destination == RouteDestination::Bus(room_index)
                })
                .count(),
            2
        );

        let control_runtime = controls.runtime();
        let mut runtime = graph.runtime();
        let mut first_wet = None;
        let mut late_maximum = 0.0_f32;
        for frame in 0..96_000 {
            let sources = if frame == 0 {
                [[1.0, 1.0], [0.5, 0.5]]
            } else {
                [[0.0, 0.0]; 2]
            };
            let output = runtime.process(&sources, control_runtime.values(), false);
            let peak = output[0].abs().max(output[1].abs());
            if first_wet.is_none() && peak > 1.0e-7 {
                first_wet = Some(frame);
            }
            if frame >= 48_000 {
                late_maximum = late_maximum.max(peak);
            }
        }
        assert!(first_wet.is_some_and(|frame| frame < 6_000));
        assert!(late_maximum > 1.0e-7);
    }

    #[test]
    fn eq_inspection_exposes_every_active_band_value_and_safe_frequency_domain() {
        let source = include_str!("../../tests/fixtures/production_mix.crump").replace(
            "        setting mid1_q 1",
            "        setting mid1_q 1\n        setting low_frequency 120hz\n        setting low_gain 2db\n        setting mid2_frequency 2.5khz\n        setting mid2_gain -1db\n        setting mid2_q 1.5\n        setting high_frequency 8khz\n        setting high_gain 1db\n        setting low_pass 20khz",
        );
        let parsed = parse_project(Path::new("all-eq.crump"), &source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("Project");
        let diagnostics = validate_project(&project);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let graph = CompiledMixGraph::compile(&project, &controls).expect("mix");
        let equalizer = graph
            .inspection
            .processors
            .iter()
            .find(|processor| processor.id == "bus_eq")
            .expect("EQ inspection");
        assert_eq!(
            equalizer
                .settings
                .iter()
                .map(|setting| setting.key.as_str())
                .collect::<Vec<_>>(),
            vec![
                "high_pass",
                "low_frequency",
                "low_gain",
                "mid1_frequency",
                "mid1_gain",
                "mid1_q",
                "mid2_frequency",
                "mid2_gain",
                "mid2_q",
                "high_frequency",
                "high_gain",
                "low_pass",
            ]
        );
        assert_eq!(
            equalizer
                .settings
                .iter()
                .map(|setting| (setting.key.as_str(), setting.value))
                .collect::<Vec<_>>(),
            vec![
                ("high_pass", Some(20.0)),
                ("low_frequency", Some(120.0)),
                ("low_gain", Some(2.0)),
                ("mid1_frequency", Some(350.0)),
                ("mid1_gain", Some(-1.0)),
                ("mid1_q", Some(1.0)),
                ("mid2_frequency", Some(2_500.0)),
                ("mid2_gain", Some(-1.0)),
                ("mid2_q", Some(1.5)),
                ("high_frequency", Some(8_000.0)),
                ("high_gain", Some(1.0)),
                ("low_pass", Some(20_000.0)),
            ]
        );
        assert!(
            equalizer
                .settings
                .iter()
                .all(|setting| setting.authored && setting.parameter_target)
        );
        for setting in equalizer.settings.iter().filter(|setting| {
            setting.key.contains("frequency")
                || setting.key == "high_pass"
                || setting.key == "low_pass"
        }) {
            assert_eq!(setting.unit, Some(Unit::Hertz));
            assert_eq!(setting.minimum, Some(10.0));
            assert_eq!(setting.maximum, Some(23_520.0));
        }
    }

    #[test]
    fn latency_compensation_aligns_every_convergence_at_supported_quanta() {
        fn production_project() -> Project {
            let parsed = parse_project(
                Path::new("production_mix.crump"),
                include_str!("../../tests/fixtures/production_mix.crump"),
            );
            assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
            parsed.project.expect("Project")
        }

        fn neutralize(project: &mut Project) -> (Panner, crate::model::Processor) {
            let balance = project.song.tracks[0].channel.panner.clone();
            let latency = project.song.tracks[0].channel.inserts[0].clone();
            for instrument in &mut project.song.instruments {
                instrument.post_processors.clear();
            }
            for track in &mut project.song.tracks {
                track.channel.level_db = 0.0;
                track.channel.panner = balance.clone();
                track.channel.mute = false;
                track.channel.inserts.clear();
                track.channel.sends.clear();
            }
            for group in &mut project.song.groups {
                group.channel.level_db = 0.0;
                group.channel.panner = balance.clone();
                group.channel.mute = false;
                group.channel.inserts.clear();
                group.channel.sends.clear();
                group.channel.output = OutputTarget::Master;
            }
            for channel in &mut project.song.returns {
                channel.channel.level_db = 0.0;
                channel.channel.panner = balance.clone();
                channel.channel.mute = false;
                channel.channel.inserts.clear();
                channel.channel.sends.clear();
                channel.channel.output = OutputTarget::Master;
            }
            let master = project.song.master.as_mut().expect("Master");
            master.level_db = 0.0;
            master.inserts.clear();
            project.song.sidechains.clear();
            (balance, latency)
        }

        fn assert_aligned(project: &Project, expected_frame: usize) {
            let diagnostics = validate_project(project);
            assert!(!diagnostics.has_errors(), "{diagnostics:?}");
            let controls = CompiledControlProgram::compile(project).expect("controls");
            let graph = CompiledMixGraph::compile(project, &controls).expect("mix graph");
            for quantum in SUPPORTED_QUANTA {
                let mut runtime = graph.runtime();
                let mut control_runtime = controls.runtime();
                let mut active_frames = Vec::new();
                for block_start in (0..32).step_by(quantum) {
                    for frame in block_start..(block_start + quantum).min(32) {
                        control_runtime.evaluate_frame(frame as u64);
                        let mut sources = [[0.0_f32; 2]; MAX_RUNTIME_TRACKS];
                        if frame == 0 {
                            for source in sources.iter_mut().take(project.song.tracks.len()) {
                                *source = [1.0, 1.0];
                            }
                        }
                        let output = runtime.process(
                            &sources[..project.song.tracks.len()],
                            control_runtime.values(),
                            false,
                        );
                        if output[0].abs().max(output[1].abs()) > 1.0e-5 {
                            active_frames.push(frame);
                        }
                    }
                }
                assert_eq!(active_frames, [expected_frame], "quantum {quantum}");
            }
        }

        // Two main routes with unequal insert latency converge at one Group.
        let mut main = production_project();
        let (_, mut latency) = neutralize(&mut main);
        main.song.returns.clear();
        for track in &mut main.song.tracks {
            track.channel.output = OutputTarget::Group("mix_bus".to_owned());
        }
        latency.id = "main_latency".to_owned();
        main.song.tracks[0].channel.inserts.push(latency);
        assert_aligned(&main, 5);

        // A direct main route and a parallel Send/Return route converge at Master.
        let mut parallel = production_project();
        let (_, mut latency) = neutralize(&mut parallel);
        parallel.song.tracks.retain(|track| track.id == "drums");
        parallel.song.groups.clear();
        parallel.song.returns.truncate(1);
        parallel.song.tracks[0].channel.output = OutputTarget::Master;
        let mut send = production_project().song.tracks[0].channel.sends[0].clone();
        send.level_db = 0.0;
        send.tap = TapPoint::PostFader;
        parallel.song.tracks[0].channel.sends.push(send);
        latency.id = "return_latency".to_owned();
        parallel.song.returns[0].channel.inserts.push(latency);
        assert_aligned(&parallel, 5);

        // A delayed pre-fader sidechain and direct target source meet at the
        // compressor input; the muted key channel remains absent from Master.
        let mut sidechain = production_project();
        let (_, mut latency) = neutralize(&mut sidechain);
        sidechain.song.groups.clear();
        sidechain.song.returns.clear();
        for track in &mut sidechain.song.tracks {
            track.channel.output = OutputTarget::Master;
        }
        let drums = sidechain
            .song
            .tracks
            .iter_mut()
            .find(|track| track.id == "drums")
            .expect("drums");
        latency.id = "key_latency".to_owned();
        drums.channel.inserts.push(latency);
        drums.channel.mute = true;
        let compressor = production_project().song.tracks[1].channel.inserts[0].clone();
        sidechain
            .song
            .tracks
            .iter_mut()
            .find(|track| track.id == "bass")
            .expect("bass")
            .channel
            .inserts
            .push(compressor);
        let mut route = production_project().song.sidechains[0].clone();
        route.tap = TapPoint::PreFader;
        sidechain.song.sidechains.push(route);
        assert_aligned(&sidechain, 5);
    }

    #[test]
    fn invariant_fader_and_panner_coefficients_are_cached() {
        let mut gain = GainRuntime::new(MixScalar::Constant(-6.0));
        for _ in 0..128 {
            let _ = gain.coefficient(&[]);
        }
        assert_eq!(gain.coefficient_updates, 0);

        let panner = CompiledPanner::Balance(MixScalar::Dynamic {
            index: ControlIndex(0),
            initial: 0.25,
        });
        let mut runtime = panner.runtime();
        for _ in 0..128 {
            let _ = runtime.process([1.0, 1.0], &[0.25]);
        }
        assert_eq!(runtime.coefficient_updates, 0);
        let _ = runtime.process([1.0, 1.0], &[0.5]);
        assert_eq!(runtime.coefficient_updates, 1);
    }

    #[test]
    fn panner_modes_have_explicit_distinct_behavior() {
        let controls = [];
        let mono =
            CompiledPanner::MonoPan(MixScalar::Constant(0.0)).process([1.0, -1.0], &controls);
        assert_eq!(mono, [0.0, 0.0]);
        let balance =
            CompiledPanner::Balance(MixScalar::Constant(1.0)).process([1.0, 1.0], &controls);
        assert!(balance[0].abs() < 1.0e-6 && (balance[1] - 1.0).abs() < 1.0e-6);
        let stereo = CompiledPanner::StereoPan {
            left: MixScalar::Constant(-1.0),
            right: MixScalar::Constant(1.0),
        }
        .process([1.0, 1.0], &controls);
        assert!((stereo[0] - 1.0).abs() < 1.0e-6);
        assert!((stereo[1] - 1.0).abs() < 1.0e-6);
    }
}
