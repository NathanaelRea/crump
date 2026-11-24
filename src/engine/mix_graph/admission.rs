use super::*;

/// Supported callback quantum sizes used for admission estimates and fixtures.
pub const SUPPORTED_QUANTA: [usize; 3] = [64, 128, 256];
/// Tracks, Groups, Returns, and the singleton Master admitted by the runtime.
pub const MAX_MIX_CHANNELS: usize = MAX_RUNTIME_TRACKS * 3 + 1;
/// Named maximum runtime bytes, including every admitted resident publication runtime.
pub const MAX_PUBLICATION_RUNTIME_BYTES: u64 = 1024 * 1024 * 1024;
/// One Active runtime plus at most one guarded replacement/retired runtime.
pub const MAX_RESIDENT_PUBLICATION_RUNTIMES: u64 = 2;
/// Named abstract work-unit budget per rendered sample.
pub const MAX_WORK_UNITS_PER_FRAME: u64 = 65_536;
/// Maximum shared processor instances with fixed-capacity callback telemetry.
pub const MAX_PROCESSOR_INSTANCES: usize = 512;

/// Which runtime a Project is being admitted for.
///
/// Live playback runs inside a realtime callback and overlaps an old and a replacement runtime
/// during publication, so it is held to the per-frame work budget and to two resident runtimes.
/// Offline processing (render and bounce) never swaps runtimes and has no deadline: it keeps every
/// structural capacity and the memory budget for one runtime, but no per-frame work budget.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionProfile {
    /// Realtime playback with guarded publication transitions.
    Live,
    /// Offline render and bounce.
    Offline,
}

impl AdmissionProfile {
    /// Human-readable name used in diagnostics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Live => "live playback",
            Self::Offline => "offline render",
        }
    }
}

/// One named runtime limit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionLimit {
    /// Timeline Tracks.
    Tracks,
    /// Tracks, Groups, Returns, and Master.
    Channels,
    /// Shared processor instances.
    Processors,
    /// Graph voice-pool instances.
    GraphVoices,
    /// Dense effective Parameter targets.
    ControlTargets,
    /// Resident runtime bytes.
    RuntimeBytes,
    /// Abstract callback work per frame.
    WorkUnitsPerFrame,
}

impl AdmissionLimit {
    /// Stable diagnostic name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tracks => "tracks",
            Self::Channels => "mix channels",
            Self::Processors => "processors",
            Self::GraphVoices => "graph voices",
            Self::ControlTargets => "control targets",
            Self::RuntimeBytes => "runtime bytes",
            Self::WorkUnitsPerFrame => "work units/frame",
        }
    }

    /// What an author can change to fit this limit.
    #[must_use]
    pub const fn help(self) -> &'static str {
        match self {
            Self::Tracks | Self::Channels => "reduce Tracks, Groups, or Returns",
            Self::Processors => "reduce processors or Instrument post-chains used by many Tracks",
            Self::GraphVoices => "reduce Instrument polyphony or the Tracks that use it",
            Self::ControlTargets => "reduce exposed Parameters, Automation, or Modulation targets",
            Self::RuntimeBytes => {
                "reduce referenced sample duration, polyphony, delay or reverb state, or Track count"
            }
            Self::WorkUnitsPerFrame => {
                "lower polyphony on lines that need few voices, simplify graphs, or render offline"
            }
        }
    }
}

/// One limit a Project exceeds under an [`AdmissionProfile`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdmissionFailure {
    /// Exceeded limit.
    pub limit: AdmissionLimit,
    /// Estimated value compared against the budget.
    pub actual: u64,
    /// Named budget.
    pub budget: u64,
}

impl std::fmt::Display for AdmissionFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} {} (budget {})",
            self.actual,
            self.limit.label(),
            self.budget
        )
    }
}

/// Formats failures as one sentence fragment, for example `140000 work units/frame (budget 65536)`.
#[must_use]
pub fn describe_admission_failures(failures: &[AdmissionFailure]) -> String {
    failures
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn publication_transition_work_units(
    steady_state: u64,
    tracks: usize,
    channels: usize,
    control_targets: usize,
) -> u64 {
    let bookkeeping = (tracks as u64)
        .saturating_mul(4)
        .saturating_add((channels as u64).saturating_mul(8))
        .saturating_add((control_targets as u64).saturating_mul(2));
    steady_state
        .saturating_mul(MAX_RESIDENT_PUBLICATION_RUNTIMES)
        .saturating_add(bookkeeping)
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
    /// Track-owned shared post-voice graph runtime instances.
    pub shared_graphs: u64,
    /// Shared-graph work evaluated once per Track and output frame.
    pub shared_graph_work_units_per_frame: u64,
    /// Dense effective Parameter targets.
    pub control_targets: usize,
    /// Abstract bounded work performed per output sample in steady state.
    pub work_units_per_frame: u64,
    /// Worst-case callback work while old and replacement runtimes overlap.
    pub publication_transition_work_units_per_frame: u64,
    /// Preallocated channel/bus block-buffer bytes at the largest quantum.
    pub bus_buffer_bytes: u64,
    /// Preallocated channel meter state bytes.
    pub meter_bytes: u64,
    /// Preallocated fixed-capacity processor telemetry state bytes.
    pub processor_meter_bytes: u64,
    /// Mutable preallocated state for every graph voice, including Node buffers.
    pub graph_voice_state_bytes: u64,
    /// Mutable state for one shared post-voice graph per referencing Track.
    pub shared_graph_state_bytes: u64,
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
    /// Whether every live-playback structural, memory, and work budget admits the Project.
    pub admitted: bool,
    /// Whether every offline-render structural and memory budget admits the Project.
    #[serde(default)]
    pub offline_admitted: bool,
}

impl RuntimeResourceEstimate {
    /// Produces a conservative estimate without allocating runtime DSP state.
    pub fn for_project(project: &Project, control_targets: usize) -> Result<Self, EngineError> {
        let mut graph_voices = 0_u64;
        let mut graph_node_instances = 0_u64;
        let mut shared_graphs = 0_u64;
        let mut shared_graph_work_units_per_frame = 0_u64;
        let mut graph_voice_state_bytes = 0_u64;
        let mut shared_graph_state_bytes = 0_u64;
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
                    stored_voices.saturating_mul(authored_graph_work_units(instrument, project)),
                );
                graph_voice_state_bytes = graph_voice_state_bytes
                    .saturating_add(
                        stored_voices.saturating_mul(authored_graph_voice_state_bytes(
                            instrument,
                            project.sample_rate,
                            control_targets,
                        )),
                    )
                    .saturating_add(voices.saturating_mul(graph_voice_slot_overhead_bytes()));
            }
            if instrument.shared_graph.is_some() {
                let shared_work = authored_shared_graph_work_units(instrument, project);
                shared_graphs = shared_graphs.saturating_add(1);
                shared_graph_work_units_per_frame =
                    shared_graph_work_units_per_frame.saturating_add(shared_work);
                graph_node_instances = graph_node_instances.saturating_add(shared_work);
                shared_graph_state_bytes =
                    shared_graph_state_bytes.saturating_add(authored_shared_graph_state_bytes(
                        instrument,
                        project.sample_rate,
                        control_targets,
                    ));
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
            .saturating_add(shared_graph_state_bytes)
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
        let publication_transition_work_units_per_frame = publication_transition_work_units(
            work_units_per_frame,
            tracks,
            channels,
            control_targets,
        );
        let mut estimate = Self {
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
            shared_graphs,
            shared_graph_work_units_per_frame,
            control_targets,
            work_units_per_frame,
            publication_transition_work_units_per_frame,
            bus_buffer_bytes,
            meter_bytes,
            processor_meter_bytes,
            graph_voice_state_bytes,
            shared_graph_state_bytes,
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
            admitted: false,
            offline_admitted: false,
        };
        estimate.refresh_admission();
        Ok(estimate)
    }

    /// Every limit this estimate exceeds under `profile`, in a stable order.
    #[must_use]
    pub fn admission_failures(&self, profile: AdmissionProfile) -> Vec<AdmissionFailure> {
        let mut failures = Vec::new();
        let mut check = |limit, actual: u64, budget: u64| {
            if actual > budget {
                failures.push(AdmissionFailure {
                    limit,
                    actual,
                    budget,
                });
            }
        };
        check(
            AdmissionLimit::Tracks,
            self.tracks as u64,
            MAX_RUNTIME_TRACKS as u64,
        );
        check(
            AdmissionLimit::Channels,
            self.channels as u64,
            MAX_MIX_CHANNELS as u64,
        );
        check(
            AdmissionLimit::Processors,
            self.processors as u64,
            MAX_PROCESSOR_INSTANCES as u64,
        );
        check(
            AdmissionLimit::GraphVoices,
            self.graph_voices,
            u64::from(MAX_GRAPH_VOICES),
        );
        check(
            AdmissionLimit::ControlTargets,
            self.control_targets as u64,
            MAX_CONTROL_TARGETS as u64,
        );
        match profile {
            AdmissionProfile::Live => {
                check(
                    AdmissionLimit::RuntimeBytes,
                    self.publication_peak_bytes,
                    self.publication_budget_bytes,
                );
                check(
                    AdmissionLimit::WorkUnitsPerFrame,
                    self.publication_transition_work_units_per_frame,
                    self.work_budget_per_frame,
                );
            }
            AdmissionProfile::Offline => check(
                AdmissionLimit::RuntimeBytes,
                self.runtime_bytes,
                self.publication_budget_bytes,
            ),
        }
        failures
    }

    /// Whether every limit for `profile` admits the Project.
    #[must_use]
    pub fn admits(&self, profile: AdmissionProfile) -> bool {
        match profile {
            AdmissionProfile::Live => self.admitted,
            AdmissionProfile::Offline => self.offline_admitted,
        }
    }

    fn refresh_admission(&mut self) {
        self.admitted = self.admission_failures(AdmissionProfile::Live).is_empty();
        self.offline_admitted = self
            .admission_failures(AdmissionProfile::Offline)
            .is_empty();
    }

    pub(super) fn include_compiled_state(&mut self, program: &CompiledMixProgram) {
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

    pub(super) fn include_additional_callback_work(&mut self, work_units_per_frame: u64) {
        self.work_units_per_frame = self
            .work_units_per_frame
            .saturating_add(work_units_per_frame);
        self.recalculate_memory_and_admission();
    }

    pub(super) fn include_song_runtime_state(
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

    pub(crate) fn include_additional_instrument_program_bytes(&mut self, bytes: u64) {
        self.instrument_program_bytes = self.instrument_program_bytes.saturating_add(bytes);
        self.recalculate_memory_and_admission();
    }

    pub(super) fn recalculate_memory_and_admission(&mut self) {
        self.runtime_bytes = self
            .bus_buffer_bytes
            .saturating_add(self.meter_bytes)
            .saturating_add(self.processor_meter_bytes)
            .saturating_add(self.graph_voice_state_bytes)
            .saturating_add(self.shared_graph_state_bytes)
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
        self.publication_transition_work_units_per_frame = publication_transition_work_units(
            self.work_units_per_frame,
            self.tracks,
            self.channels,
            self.control_targets,
        );
        self.refresh_admission();
    }
}

pub(super) fn channel_and_master_processors(
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
