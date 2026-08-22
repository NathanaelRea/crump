//! Deterministic audio compilation and runtime shared by playback and rendering.

mod analysis;
pub mod audition;
pub mod bounce;
pub mod control;
mod drums;
mod follower;
pub(crate) mod generated_audio;
mod graph;
pub mod mix_graph;
mod oversampling;
mod processors;
pub(crate) mod qualification;
mod render;
pub mod sample;
pub mod sample_transform;
mod sampler;
pub(crate) mod schedule;

use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};
#[cfg(any(feature = "pipewire", test))]
use sha2::{Digest, Sha256};

#[cfg(any(feature = "pipewire", test))]
use crate::model::Instrument;
use crate::{
    diagnostic::Diagnostic,
    media::AssetInventory,
    model::{DrumVoice, InstrumentKind, Project, TapPoint},
    validate::{validate_project_assets, validate_project_with_topology},
};

use self::{
    control::{CompiledControlProgram, ControlIndex, ControlRuntime, ControlValueSnapshot},
    drums::DrumSynth,
    follower::{
        CompiledEnvelopeFollowerProgram, EnvelopeFollowerInspection, EnvelopeFollowerMeter,
        EnvelopeFollowerRuntime, MAX_ENVELOPE_FOLLOWERS,
    },
    graph::{CompiledGraph, GraphVoice},
    mix_graph::{
        ChannelMeter, CompiledMixGraph, MAX_MIX_CHANNELS, MAX_PROCESSOR_INSTANCES, MixGraphRuntime,
        ProcessorMeter,
    },
    sample::{CompiledSampleLibrary, CompiledSampleSource},
    sampler::{CompiledSamplerProgram, SamplerRuntime},
    schedule::{ClipEnvelope, MAX_RUNTIME_TRACKS, RuntimeEvent, TrackSchedule, compile_schedules},
};

pub use analysis::FinalizedAudioAnalysis;
pub(crate) use generated_audio::GeneratedAudioArtifact;
pub use qualification::{
    QualificationHost, QualificationOptions, QualificationReport, qualify_project,
};
pub(crate) use render::inspect_render_destination;
pub use render::{
    RenderManifest, RenderOptions, RenderResult, RenderStemManifest, RenderStemPlan,
    RenderStemProcessingPolicy, RenderStemRequest, plan_render_stems, render_project,
};

pub(crate) const PUBLICATION_TRANSITION_MILLISECONDS: u64 = 10;
/// One sounding graph voice plus one preallocated short replacement tail.
pub(crate) const GRAPH_VOICE_STORAGE_MULTIPLIER: u64 = 2;

/// Immutable, fully allocated-away-from-callback audio program.
#[derive(Debug, Clone)]
pub struct CompiledProject {
    project_id: String,
    sample_rate: u32,
    arrangement_frames: u64,
    total_frames: u64,
    controls: CompiledControlProgram,
    tracks: Vec<CompiledTrack>,
    mix_graph: CompiledMixGraph,
    envelope_followers: Arc<CompiledEnvelopeFollowerProgram>,
}

impl CompiledProject {
    /// Validates and compiles a media-free Project for playback or rendering.
    ///
    /// Projects containing sampler mappings must use [`Self::compile_with_assets`]
    /// so exact verified media bytes are available during publication.
    pub fn compile(project: &Project) -> Result<Self, EngineError> {
        Self::compile_with_assets(project, &AssetInventory::default())
    }

    /// Validates and compiles one Project plus its verified exact-byte media.
    pub fn compile_with_assets(
        project: &Project,
        inventory: &AssetInventory,
    ) -> Result<Self, EngineError> {
        let (mut diagnostics, topology) = validate_project_with_topology(project);
        diagnostics.extend(validate_project_assets(project, inventory));
        if diagnostics.has_errors() {
            return Err(EngineError::Validation(diagnostics.into_vec()));
        }
        let schedules = compile_schedules(project)?;
        let controls = CompiledControlProgram::compile(project)?;
        let sample_library = CompiledSampleLibrary::compile(inventory, project.sample_rate)
            .map_err(|error| EngineError::Compile(error.to_string()))?;
        let instruments: BTreeMap<_, _> = project
            .song
            .instruments
            .iter()
            .map(|instrument| (instrument.id.as_str(), instrument))
            .collect();
        let mut tracks = Vec::with_capacity(schedules.len());
        for schedule in schedules {
            let instrument = instruments
                .get(schedule.instrument_id.as_str())
                .ok_or_else(|| {
                    EngineError::Compile(format!("missing instrument `{}`", schedule.instrument_id))
                })?;
            let program = match instrument.kind {
                InstrumentKind::Graph => InstrumentProgram::Graph {
                    graph: Arc::new(CompiledGraph::compile(
                        instrument,
                        project.sample_rate,
                        &controls,
                    )?),
                    polyphony: usize::from(instrument.polyphony),
                },
                InstrumentKind::StockDrums => InstrumentProgram::Drums {
                    instrument: Box::new((*instrument).clone()),
                    tone: controls.index(&crate::model::ParameterTarget::Instrument {
                        instrument: instrument.id.clone(),
                        parameter: "tone".to_owned(),
                    }),
                    punch: controls.index(&crate::model::ParameterTarget::Instrument {
                        instrument: instrument.id.clone(),
                        parameter: "punch".to_owned(),
                    }),
                },
                InstrumentKind::Sampler => {
                    InstrumentProgram::Sampler(Arc::new(CompiledSamplerProgram::compile(
                        instrument,
                        &sample_library,
                        &controls,
                        project.sample_rate,
                    )?))
                }
            };
            #[cfg(any(feature = "pipewire", test))]
            let semantic_hash = track_semantic_hash(&schedule, instrument, project)?;
            tracks.push(CompiledTrack {
                schedule: Arc::new(schedule),
                program,
                #[cfg(any(feature = "pipewire", test))]
                semantic_hash,
            });
        }
        let mut mix_graph =
            CompiledMixGraph::compile_with_topology(project, &controls, &topology, true)?;
        let envelope_followers = Arc::new(CompiledEnvelopeFollowerProgram::compile(
            project,
            &controls,
            &mix_graph,
            project.sample_rate,
        )?);
        let schedule_program_bytes = tracks
            .iter()
            .map(|track| track.schedule.allocation_bytes())
            .fold(0_u64, u64::saturating_add);
        let instrument_program_bytes = tracks
            .iter()
            .map(|track| match &track.program {
                InstrumentProgram::Graph { graph, .. } => graph.allocation_bytes(),
                InstrumentProgram::Drums { .. } => 0,
                InstrumentProgram::Sampler(program) => program.allocation_bytes(),
            })
            .fold(sample_library.allocation_bytes(), u64::saturating_add);
        let song_runtime_structure_bytes = (std::mem::size_of::<SongRuntime>() as u64)
            .saturating_add(project.id.len() as u64)
            .saturating_add(
                (tracks.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<TrackRuntime>() as u64),
            )
            .saturating_add(controls.runtime_state_bytes())
            .saturating_add(envelope_followers.runtime_state_bytes());
        mix_graph.include_song_runtime_state(
            controls
                .allocation_bytes()
                .saturating_add(envelope_followers.allocation_bytes()),
            schedule_program_bytes,
            instrument_program_bytes,
            song_runtime_structure_bytes,
        )?;
        mix_graph.include_additional_callback_work(envelope_followers.work_units_per_frame())?;
        let arrangement_frames = project
            .song
            .end
            .to_frames(project.song.tempo_bpm, project.sample_rate)?;
        let tail_frames = seconds_to_frames(project.render_tail_seconds, project.sample_rate)?;
        let total_frames = arrangement_frames
            .checked_add(tail_frames)
            .ok_or_else(|| EngineError::Compile("render range exceeds frame limits".to_owned()))?;
        Ok(Self {
            project_id: project.id.clone(),
            sample_rate: project.sample_rate,
            arrangement_frames,
            total_frames,
            controls,
            tracks,
            mix_graph,
            envelope_followers,
        })
    }

    /// Creates a fresh, preallocated runtime at Song time zero.
    #[must_use]
    pub fn runtime(&self) -> SongRuntime {
        SongRuntime {
            project_id: self.project_id.clone(),
            sample_rate: self.sample_rate,
            arrangement_frames: self.arrangement_frames,
            total_frames: self.total_frames,
            frame: 0,
            audition_range: None,
            audition_loop: None,
            audition_output: AuditionMonitorOutput::Master,
            raw_audition: None,
            mapping_audition: None,
            #[cfg(any(feature = "pipewire", test))]
            audition_seek_requested: false,
            controls: self.controls.runtime(),
            envelope_followers: self.envelope_followers.runtime(),
            #[cfg(any(feature = "pipewire", test))]
            scalar_control_handoff: false,
            #[cfg(any(feature = "pipewire", test))]
            publication_pristine: true,
            tracks: self
                .tracks
                .iter()
                .enumerate()
                .map(|(index, track)| TrackRuntime::new(track, self.sample_rate, index as u64))
                .collect(),
            mix: self.mix_graph.runtime(),
            track_sources: [[0.0; 2]; MAX_RUNTIME_TRACKS],
            telemetry: RuntimeTelemetry::default(),
        }
    }

    /// Sample rate fixed by the Project.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Exact arrangement frame count before the explicit Render Tail.
    #[must_use]
    pub const fn arrangement_frames(&self) -> u64 {
        self.arrangement_frames
    }

    /// Exact complete Render Range frame count.
    #[must_use]
    pub const fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Evaluates inspectable effective-value provenance off the audio callback.
    #[must_use]
    pub fn control_snapshot_at_frame(&self, frame: u64) -> Vec<ControlValueSnapshot> {
        self.controls.snapshot_at(frame)
    }

    /// Inspectable processing paths, latency, and resource admission facts.
    #[must_use]
    pub fn mix_graph_inspection(&self) -> &mix_graph::MixGraphInspection {
        self.mix_graph.inspection()
    }

    /// Static Audio-derived control routes aligned to runtime follower telemetry indexes.
    #[must_use]
    pub fn envelope_follower_inspection(&self) -> Vec<EnvelopeFollowerInspection> {
        self.envelope_followers.inspection()
    }
}

#[derive(Debug, Clone)]
struct CompiledTrack {
    schedule: Arc<TrackSchedule>,
    program: InstrumentProgram,
    #[cfg(any(feature = "pipewire", test))]
    semantic_hash: [u8; 32],
}

#[derive(Debug, Clone)]
enum InstrumentProgram {
    Graph {
        graph: Arc<CompiledGraph>,
        polyphony: usize,
    },
    Drums {
        instrument: Box<crate::model::Instrument>,
        tone: Option<ControlIndex>,
        punch: Option<ControlIndex>,
    },
    Sampler(Arc<CompiledSamplerProgram>),
}

#[cfg(any(feature = "pipewire", test))]
fn track_semantic_hash(
    schedule: &TrackSchedule,
    instrument: &Instrument,
    project: &Project,
) -> Result<[u8; 32], EngineError> {
    let mut hasher = Sha256::new();
    hash_bytes(&mut hasher, schedule.track_id.as_bytes());
    hash_bytes(&mut hasher, schedule.instrument_id.as_bytes());
    hasher.update(project.sample_rate.to_le_bytes());
    // Track level/balance and Parameter base values are scalar controls. They
    // intentionally do not invalidate callback-safe voice/effect state.
    hasher.update((schedule.events.len() as u64).to_le_bytes());
    for event in &schedule.events {
        hasher.update(event.frame.to_le_bytes());
        match event.kind {
            RuntimeEvent::NoteOn {
                note_id,
                pitch,
                velocity,
                seed,
                clip_envelope,
            } => {
                hasher.update([0]);
                hasher.update(note_id.to_le_bytes());
                hasher.update([pitch]);
                hasher.update(velocity.to_bits().to_le_bytes());
                hasher.update(seed.to_le_bytes());
                hash_clip_envelope(&mut hasher, clip_envelope);
            }
            RuntimeEvent::NoteOff { note_id } => {
                hasher.update([1]);
                hasher.update(note_id.to_le_bytes());
            }
            RuntimeEvent::Hit {
                voice,
                velocity,
                seed,
                clip_envelope,
            } => {
                hasher.update([2, voice as u8]);
                hasher.update(velocity.to_bits().to_le_bytes());
                hasher.update(seed.to_le_bytes());
                hash_clip_envelope(&mut hasher, clip_envelope);
            }
            RuntimeEvent::SampleOn {
                note_id,
                mapping_index,
                start_frame,
                end_frame,
                gain_db,
                pan,
                fade_in_frames,
                fade_out_frames,
                direction,
                loop_region,
                velocity,
                clip_envelope,
            } => {
                hasher.update([3]);
                hasher.update(note_id.to_le_bytes());
                hasher.update(mapping_index.to_le_bytes());
                hasher.update(start_frame.to_le_bytes());
                hasher.update(end_frame.to_le_bytes());
                hasher.update(gain_db.to_bits().to_le_bytes());
                hasher.update(pan.to_bits().to_le_bytes());
                hasher.update(fade_in_frames.to_le_bytes());
                hasher.update(fade_out_frames.to_le_bytes());
                hasher.update([direction.map_or(0, |direction| direction as u8 + 1)]);
                if let Some(loop_region) = loop_region {
                    hasher.update([1]);
                    hasher.update(loop_region.start_frame.to_le_bytes());
                    hasher.update(loop_region.end_frame.to_le_bytes());
                    hasher.update(loop_region.crossfade_frames.to_le_bytes());
                } else {
                    hasher.update([0]);
                }
                hasher.update(velocity.to_bits().to_le_bytes());
                hash_clip_envelope(&mut hasher, clip_envelope);
            }
            RuntimeEvent::SampleOff { note_id } => {
                hasher.update([4]);
                hasher.update(note_id.to_le_bytes());
            }
        }
    }
    let parameters = instrument
        .parameters
        .iter()
        .map(|parameter| {
            let allocation_value = instrument.graph.as_ref().and_then(|graph| {
                graph
                    .nodes
                    .iter()
                    .any(|node| {
                        node.kind == crate::model::NodeKind::Delay
                            && matches!(
                                node.settings.get("time"),
                                Some(crate::model::SettingValue::Parameter(bound))
                                    if bound == &parameter.key
                            )
                    })
                    .then_some(parameter.value)
            });
            (&parameter.key, allocation_value)
        })
        .collect::<Vec<_>>();
    let sample_assets = instrument
        .sampler
        .as_ref()
        .map(|sampler| {
            sampler
                .mappings
                .iter()
                .filter_map(|mapping| {
                    project
                        .assets
                        .iter()
                        .find(|asset| asset.id == mapping.asset)
                        .map(|asset| (&asset.id, &asset.sha256))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut audio_program = serde_json::json!({
        "kind": instrument.kind,
        "polyphony": instrument.polyphony,
        "parameters": parameters,
        "graph": instrument.graph,
        "sampler": instrument.sampler,
        "sample_assets": sample_assets,
    });
    strip_source_metadata(&mut audio_program);
    hash_bytes(&mut hasher, &serde_json::to_vec(&audio_program)?);
    Ok(hasher.finalize().into())
}

#[cfg(any(feature = "pipewire", test))]
fn hash_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

#[cfg(any(feature = "pipewire", test))]
fn hash_clip_envelope(hasher: &mut Sha256, envelope: ClipEnvelope) {
    hasher.update(envelope.gain.to_bits().to_le_bytes());
    hasher.update(envelope.position_frames.to_le_bytes());
    hasher.update(envelope.duration_frames.to_le_bytes());
    hasher.update(envelope.fade_in_frames.to_le_bytes());
    hasher.update(envelope.fade_out_frames.to_le_bytes());
}

#[cfg(any(feature = "pipewire", test))]
fn strip_source_metadata(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.retain(|key, _| key != "location" && key != "site" && !key.ends_with("_site"));
            for child in object.values_mut() {
                strip_source_metadata(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                strip_source_metadata(child);
            }
        }
        _ => {}
    }
}

#[cfg(any(feature = "pipewire", test))]
#[derive(Default)]
struct TransitionFrame {
    common_sources: [[f32; 2]; MAX_RUNTIME_TRACKS],
    affected_sources: [[f32; 2]; MAX_RUNTIME_TRACKS],
    common_voices: usize,
    affected_voices: usize,
    frame: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuditionMonitorOutput {
    Master,
    Channel { index: usize, tap: TapPoint },
    Source { index: usize },
    Raw,
}

#[derive(Debug, Clone)]
struct RawAuditionRuntime {
    source: CompiledSampleSource,
    start: usize,
    end: usize,
    cursor: usize,
}

impl RawAuditionRuntime {
    fn process(&mut self) -> [f32; 2] {
        let output = self.source.frame(self.cursor).unwrap_or([0.0, 0.0]);
        self.cursor = self.cursor.saturating_add(1).min(self.end);
        output
    }

    fn rewind(&mut self) {
        self.cursor = self.start;
    }

    fn realign(&mut self, range_offset: u64) {
        let offset = usize::try_from(range_offset).unwrap_or(usize::MAX);
        self.cursor = self.start.saturating_add(offset).min(self.end);
    }
}

#[derive(Debug, Clone, Copy)]
struct MappingAuditionTrigger {
    track_index: usize,
    mapping_index: u16,
    note_id: u64,
    velocity: f32,
}

/// Mutable audio runtime. All vectors and DSP buffers are allocated at creation.
pub struct SongRuntime {
    project_id: String,
    sample_rate: u32,
    arrangement_frames: u64,
    total_frames: u64,
    frame: u64,
    audition_range: Option<(u64, u64)>,
    audition_loop: Option<(u64, u64)>,
    audition_output: AuditionMonitorOutput,
    raw_audition: Option<RawAuditionRuntime>,
    mapping_audition: Option<MappingAuditionTrigger>,
    #[cfg(any(feature = "pipewire", test))]
    audition_seek_requested: bool,
    controls: ControlRuntime,
    envelope_followers: EnvelopeFollowerRuntime,
    #[cfg(any(feature = "pipewire", test))]
    scalar_control_handoff: bool,
    #[cfg(any(feature = "pipewire", test))]
    publication_pristine: bool,
    tracks: Vec<TrackRuntime>,
    mix: MixGraphRuntime,
    track_sources: [[f32; 2]; MAX_RUNTIME_TRACKS],
    telemetry: RuntimeTelemetry,
}

impl SongRuntime {
    /// Processes one stereo frame.
    pub fn process_frame(&mut self, mode: ProcessingMode) -> Result<[f32; 2], EngineError> {
        #[cfg(any(feature = "pipewire", test))]
        {
            self.publication_pristine = false;
        }
        if let Some((start, end)) = self.audition_loop
            && self.frame >= end
        {
            self.rewind_audition(start);
        }
        if self.frame >= self.playback_end() {
            self.telemetry.current_peak = 0.0;
            self.telemetry.follower_count = self.envelope_followers.route_count();
            self.telemetry
                .follower_activity
                .fill(EnvelopeFollowerMeter::default());
            self.telemetry.active_voices = 0;
            self.mix.clear_current_activity();
            let (channel_activity, channel_count) = self.mix.channel_activity();
            self.telemetry.channel_activity = channel_activity;
            self.telemetry.channel_count = channel_count;
            self.telemetry.processor_count = self
                .mix
                .copy_processor_activity(&mut self.telemetry.processor_activity);
            self.telemetry.frame = self.frame;
            return Ok([0.0, 0.0]);
        }
        self.controls.evaluate_frame(self.frame);
        let control_values = self.controls.values();
        let unconstrained_values = self.controls.unconstrained_values();
        let mut active_voices = 0;
        for (index, track) in self.tracks.iter_mut().enumerate() {
            track.dispatch(self.frame);
            let (sample, voices) = track.process(control_values, unconstrained_values);
            self.track_sources[index] = sample;
            active_voices += voices;
        }
        let routed = self.mix.process(
            &self.track_sources[..self.tracks.len()],
            control_values,
            mode == ProcessingMode::Monitoring,
        );
        let mut master = if mode == ProcessingMode::Monitoring {
            self.monitor_output(routed)
        } else {
            routed
        };
        if !master.into_iter().all(f32::is_finite) {
            let frame = self.frame;
            if mode == ProcessingMode::Monitoring {
                // The realtime caller reports the error and emits silence. Move
                // past the poisoned frame so one bad state cannot stall transport.
                self.telemetry.current_peak = 0.0;
                self.telemetry.active_voices = 0;
                self.telemetry.frame = frame;
                self.frame = self.frame.saturating_add(1);
            }
            return Err(EngineError::NonFinite { frame });
        }
        self.telemetry.follower_count = self.envelope_followers.process(
            &self.mix,
            &mut self.controls,
            &mut self.telemetry.follower_activity,
        );
        if mode == ProcessingMode::Monitoring {
            for channel in &mut master {
                *channel = monitor_sample(*channel);
            }
        }
        let peak = master[0].abs().max(master[1].abs());
        self.telemetry.peak = self.telemetry.peak.max(peak);
        self.telemetry.current_peak = peak;
        self.telemetry.active_voices = active_voices;
        let (channel_activity, channel_count) = self.mix.channel_activity();
        self.telemetry.channel_activity = channel_activity;
        self.telemetry.channel_count = channel_count;
        self.telemetry.processor_count = self
            .mix
            .copy_processor_activity(&mut self.telemetry.processor_activity);
        self.telemetry.frame = self.frame;
        self.frame += 1;
        Ok(master)
    }

    pub(crate) fn channel_tap_sample(&self, index: usize, tap: TapPoint) -> Option<[f32; 2]> {
        self.mix.channel_tap(index, tap)
    }

    /// Stops future source dispatch while retaining bounded voice, processor, and route tails.
    pub(crate) fn begin_isolated_tail(&mut self, tail_frames: u64) {
        for track in &mut self.tracks {
            track.cursor = track.schedule.events.len();
            track.instrument.all_notes_off();
        }
        self.total_frames = self.frame.saturating_add(tail_frames);
        self.audition_range = None;
        self.audition_loop = None;
    }

    fn playback_end(&self) -> u64 {
        self.audition_range
            .map_or(self.total_frames, |(_, end)| end.min(self.total_frames))
    }

    fn monitor_output(&mut self, master: [f32; 2]) -> [f32; 2] {
        match self.audition_output {
            AuditionMonitorOutput::Master => master,
            AuditionMonitorOutput::Channel { index, tap } => {
                self.mix.channel_tap(index, tap).unwrap_or([0.0, 0.0])
            }
            AuditionMonitorOutput::Source { index } => {
                self.mix.source_tap(index).unwrap_or([0.0, 0.0])
            }
            AuditionMonitorOutput::Raw => self
                .raw_audition
                .as_mut()
                .map_or([0.0, 0.0], RawAuditionRuntime::process),
        }
    }

    fn rewind_audition(&mut self, start: u64) {
        self.frame = start;
        self.mix.set_transport_frame(start);
        for track in &mut self.tracks {
            track.rewind_loop();
        }
        if let Some(raw) = &mut self.raw_audition {
            raw.rewind();
        }
        self.envelope_followers.reset();
        self.controls.clear_future_modulation();
        if let Some(trigger) = self.mapping_audition {
            self.fire_mapping_audition(trigger);
        }
    }

    fn fire_mapping_audition(&mut self, trigger: MappingAuditionTrigger) -> bool {
        self.tracks
            .get_mut(trigger.track_index)
            .is_some_and(|track| {
                track.instrument.sampler_mapping_on(
                    trigger.note_id,
                    trigger.mapping_index,
                    trigger.velocity,
                )
            })
    }

    /// Advances deterministically without retaining output.
    ///
    /// Publication preparation uses this off the audio thread so a replacement
    /// runtime can enter at the current transport frame without callback work.
    pub fn advance_to(&mut self, target_frame: u64) -> Result<(), EngineError> {
        let target = target_frame.min(self.total_frames);
        while self.frame < target {
            let _ = self.process_frame(ProcessingMode::Offline)?;
        }
        Ok(())
    }

    /// Positions and clears a runtime without rendering its prior timeline.
    ///
    /// This may touch every preallocated DSP buffer and therefore runs on the
    /// controller thread before publication, never in the audio callback.
    pub(crate) fn seek_transport(&mut self, target_frame: u64) {
        self.frame = target_frame.min(self.total_frames);
        self.telemetry = RuntimeTelemetry {
            frame: self.frame,
            ..RuntimeTelemetry::default()
        };
        for track in &mut self.tracks {
            track.seek_transport(self.frame);
        }
        self.mix.reset();
        self.mix.set_transport_frame(self.frame);
        self.envelope_followers.reset();
        self.controls.clear_future_modulation();
        #[cfg(any(feature = "pipewire", test))]
        {
            self.publication_pristine = true;
        }
    }

    /// Clears a replacement on the controller thread before queue ownership moves.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn prime_for_publication(&mut self) {
        if !self.audition_seek_requested {
            self.seek_transport(self.frame);
        }
    }

    /// Realigns an already-cleared replacement with bounded cursor-only callback work.
    #[cfg(any(feature = "pipewire", test))]
    fn realign_pristine_publication(&mut self, target_frame: u64) {
        debug_assert!(self.publication_pristine);
        self.frame = target_frame.min(self.total_frames);
        self.telemetry.frame = self.frame;
        self.mix.set_transport_frame(self.frame);
        for track in &mut self.tracks {
            track.realign_pristine_transport(self.frame);
        }
        if let (Some((start, _)), Some(raw)) = (self.audition_range, &mut self.raw_audition) {
            raw.realign(self.frame.saturating_sub(start));
        }
    }

    /// Marks Tracks whose schedule, topology, and Instrument semantics are unchanged.
    ///
    /// A wholly topology-compatible publication also adopts the replacement control
    /// program and begins a bounded monitoring-only scalar ramp. Called on the PipeWire
    /// thread at a block boundary; it performs only bounded comparisons and copies over
    /// preallocated arrays and does not allocate.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn prepare_transition(&mut self, next: &mut Self) {
        self.scalar_control_handoff = false;
        for track in &mut self.tracks {
            track.transition_unchanged = false;
        }
        for track in &mut next.tracks {
            track.transition_unchanged = false;
        }

        // Explicit musician seeks retain their controller-prepared frame. Ordinary
        // publication resolves transport from the callback's current frame so slow
        // compilation cannot make UI telemetry authoritative. If valid range bounds
        // moved, preserve the exact old range offset (or loop phase) in the new range.
        let explicit_transport_jump = next.audition_seek_requested;
        let target_frame = if explicit_transport_jump {
            next.frame
        } else if self.audition_range != next.audition_range {
            match (self.audition_range, next.audition_range) {
                (Some((old_start, old_end)), Some((new_start, new_end))) => {
                    let old_length = old_end.saturating_sub(old_start).max(1);
                    let new_length = new_end.saturating_sub(new_start).max(1);
                    let old_offset = if self.audition_loop.is_some() {
                        self.frame.saturating_sub(old_start) % old_length
                    } else {
                        self.frame
                            .clamp(old_start, old_end.saturating_sub(1))
                            .saturating_sub(old_start)
                    };
                    new_start.saturating_add(old_offset.min(new_length.saturating_sub(1)))
                }
                _ => self.frame,
            }
        } else {
            self.frame
        };
        if !explicit_transport_jump {
            next.realign_pristine_publication(target_frame);
            if let Some(trigger) = next.mapping_audition {
                next.fire_mapping_audition(trigger);
            }
            // A mapped frame is a transport jump even though the callback, rather
            // than the controller, resolved it. Do not overwrite its prepared mix
            // state with state from a different absolute point at handoff.
            next.audition_seek_requested = target_frame != self.frame;
        }

        if explicit_transport_jump
            || self.sample_rate != next.sample_rate
            || target_frame != self.frame
        {
            return;
        }
        next.envelope_followers
            .adopt_compatible_state(&mut self.envelope_followers);
        for current in &mut self.tracks {
            if let Some(replacement) = next
                .tracks
                .iter_mut()
                .find(|replacement| current.same_semantics(replacement))
            {
                current.transition_unchanged = true;
                replacement.transition_unchanged = true;
            }
        }
        let wholly_compatible = self.tracks.len() == next.tracks.len()
            && self.tracks.iter().all(|track| track.transition_unchanged)
            && next.tracks.iter().all(|track| track.transition_unchanged)
            && self.mix.topology_compatible(&next.mix);
        if wholly_compatible {
            let frames = u64::from(self.sample_rate)
                .saturating_mul(PUBLICATION_TRANSITION_MILLISECONDS)
                / 1_000;
            self.scalar_control_handoff = self
                .controls
                .begin_publication_handoff(&mut next.controls, frames);
        }
    }

    /// Processes one publication frame without running shared Track voices twice.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn process_transition_frame(
        &mut self,
        next: &mut Self,
        old_gain: f32,
        new_gain: f32,
    ) -> Result<[f32; 2], EngineError> {
        let old = self.process_transition_sources(true);
        let new = next.process_transition_sources(false);
        let all_tracks_shared = self.tracks.len() == next.tracks.len()
            && self.tracks.iter().all(|track| track.transition_unchanged)
            && next.tracks.iter().all(|track| track.transition_unchanged);
        let mut old_sources = old.common_sources;
        for (source, affected) in old_sources.iter_mut().zip(old.affected_sources) {
            source[0] += affected[0];
            source[1] += affected[1];
        }
        let raw_output = matches!(self.audition_output, AuditionMonitorOutput::Raw)
            || matches!(next.audition_output, AuditionMonitorOutput::Raw);
        let shared_mix =
            all_tracks_shared && self.mix.topology_compatible(&next.mix) && !raw_output;
        let mut master = if shared_mix {
            let routed = self.mix.process(
                &old_sources[..self.tracks.len()],
                self.controls.values(),
                true,
            );
            self.telemetry.follower_count = self.envelope_followers.process(
                &self.mix,
                &mut self.controls,
                &mut self.telemetry.follower_activity,
            );
            next.telemetry.follower_count = next.envelope_followers.process(
                &self.mix,
                &mut next.controls,
                &mut next.telemetry.follower_activity,
            );
            self.monitor_output(routed)
        } else {
            let mut next_sources = new.affected_sources;
            for (current_index, current) in self.tracks.iter().enumerate() {
                if !current.transition_unchanged {
                    continue;
                }
                if let Some(replacement_index) = next
                    .tracks
                    .iter()
                    .position(|replacement| current.same_semantics(replacement))
                {
                    next_sources[replacement_index] = old.common_sources[current_index];
                }
            }
            let old_routed = self.mix.process(
                &old_sources[..self.tracks.len()],
                self.controls.values(),
                true,
            );
            let new_routed = next.mix.process(
                &next_sources[..next.tracks.len()],
                next.controls.values(),
                true,
            );
            self.telemetry.follower_count = self.envelope_followers.process(
                &self.mix,
                &mut self.controls,
                &mut self.telemetry.follower_activity,
            );
            next.telemetry.follower_count = next.envelope_followers.process(
                &next.mix,
                &mut next.controls,
                &mut next.telemetry.follower_activity,
            );
            let old_output = self.monitor_output(old_routed);
            let new_output = next.monitor_output(new_routed);
            [
                old_output[0] * old_gain + new_output[0] * new_gain,
                old_output[1] * old_gain + new_output[1] * new_gain,
            ]
        };
        let frame = old.frame;
        if !master.into_iter().all(f32::is_finite) {
            self.record_transition_output(frame, [0.0, 0.0], 0);
            next.record_transition_output(new.frame, [0.0, 0.0], 0);
            return Err(EngineError::NonFinite { frame });
        }
        for channel in &mut master {
            *channel = monitor_sample(*channel);
        }
        let active_voices = old
            .common_voices
            .saturating_add(old.affected_voices)
            .saturating_add(new.affected_voices);
        self.record_transition_output(frame, master, active_voices);
        next.record_transition_output(new.frame, master, active_voices);
        if shared_mix {
            next.telemetry.channel_activity = self.telemetry.channel_activity;
            next.telemetry.channel_count = self.telemetry.channel_count;
            next.telemetry.processor_activity = self.telemetry.processor_activity;
            next.telemetry.processor_count = self.telemetry.processor_count;
        }
        Ok(master)
    }

    /// Moves live state for unchanged Tracks into the accepted replacement runtime.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn complete_transition(&mut self, next: &mut Self) {
        for current in &mut self.tracks {
            if !current.transition_unchanged {
                continue;
            }
            if let Some(replacement) = next
                .tracks
                .iter_mut()
                .find(|replacement| current.same_semantics(replacement))
            {
                replacement.adopt_state_from(current);
            }
        }
        if self.scalar_control_handoff {
            std::mem::swap(&mut self.controls, &mut next.controls);
            self.scalar_control_handoff = false;
        }
        if !next.audition_seek_requested {
            next.mix.adopt_state_from(&mut self.mix);
        }
        next.audition_seek_requested = false;
        for track in &mut self.tracks {
            track.transition_unchanged = false;
        }
        for track in &mut next.tracks {
            track.transition_unchanged = false;
        }
        if let Some((start, _)) = next.audition_loop {
            for track in &mut next.tracks {
                track.prepare_loop(start);
            }
        }
        next.telemetry.active_voices = next
            .tracks
            .iter()
            .map(TrackRuntime::active_voice_count)
            .fold(0_usize, usize::saturating_add);
    }

    #[cfg(any(feature = "pipewire", test))]
    fn process_transition_sources(&mut self, process_unchanged: bool) -> TransitionFrame {
        self.publication_pristine = false;
        if let Some((start, end)) = self.audition_loop
            && self.frame >= end
        {
            self.rewind_audition(start);
        }
        let frame = self.frame;
        if frame >= self.playback_end() {
            return TransitionFrame {
                frame,
                ..TransitionFrame::default()
            };
        }
        self.controls.evaluate_frame(frame);
        let control_values = self.controls.values();
        let unconstrained_values = self.controls.unconstrained_values();
        let mut output = TransitionFrame {
            frame,
            ..TransitionFrame::default()
        };
        for (index, track) in self.tracks.iter_mut().enumerate() {
            if track.transition_unchanged && !process_unchanged {
                continue;
            }
            track.dispatch(frame);
            let (sample, voices) = track.process(control_values, unconstrained_values);
            if track.transition_unchanged {
                output.common_sources[index] = sample;
                output.common_voices = output.common_voices.saturating_add(voices);
            } else {
                output.affected_sources[index] = sample;
                output.affected_voices = output.affected_voices.saturating_add(voices);
            }
        }
        self.frame = self.frame.saturating_add(1);
        output
    }

    #[cfg(any(feature = "pipewire", test))]
    fn record_transition_output(&mut self, frame: u64, output: [f32; 2], active_voices: usize) {
        let peak = output[0].abs().max(output[1].abs());
        self.telemetry.peak = self.telemetry.peak.max(peak);
        self.telemetry.current_peak = peak;
        self.telemetry.active_voices = active_voices;
        let (channel_activity, channel_count) = self.mix.channel_activity();
        self.telemetry.channel_activity = channel_activity;
        self.telemetry.channel_count = channel_count;
        self.telemetry.processor_count = self
            .mix
            .copy_processor_activity(&mut self.telemetry.processor_activity);
        self.telemetry.frame = frame;
    }

    pub(crate) fn set_audition_range(&mut self, start: u64, end: u64) -> Result<(), EngineError> {
        if start >= end || end > self.arrangement_frames || self.frame < start || self.frame >= end
        {
            return Err(EngineError::Compile(format!(
                "audition range [{start}, {end}) does not contain runtime frame {} within arrangement {}",
                self.frame, self.arrangement_frames
            )));
        }
        self.audition_range = Some((start, end));
        Ok(())
    }

    pub(crate) fn set_audition_master_output(&mut self) {
        self.audition_output = AuditionMonitorOutput::Master;
        self.raw_audition = None;
    }

    pub(crate) fn set_audition_channel_output(&mut self, identity: &str, tap: TapPoint) -> bool {
        let Some(index) = self.mix.channel_index(identity) else {
            return false;
        };
        self.audition_output = AuditionMonitorOutput::Channel { index, tap };
        self.raw_audition = None;
        true
    }

    pub(crate) fn set_audition_source_output(&mut self, identity: &str) -> bool {
        let Some(index) = self.mix.channel_index(identity) else {
            return false;
        };
        self.audition_output = AuditionMonitorOutput::Source { index };
        self.raw_audition = None;
        true
    }

    pub(crate) fn set_raw_audition(
        &mut self,
        source: CompiledSampleSource,
        start: usize,
        end: usize,
        cursor: usize,
        audition_end: u64,
    ) -> Result<(), EngineError> {
        if start >= end || end > source.len() || cursor < start || cursor > end {
            return Err(EngineError::Compile(
                "raw audition region is outside the compiled sample source".to_owned(),
            ));
        }
        self.raw_audition = Some(RawAuditionRuntime {
            source,
            start,
            end,
            cursor,
        });
        self.audition_output = AuditionMonitorOutput::Raw;
        self.arrangement_frames = self.arrangement_frames.max(audition_end);
        self.total_frames = self.total_frames.max(audition_end);
        Ok(())
    }

    pub(crate) fn audition_mapping_on_track(
        &mut self,
        track_index: usize,
        mapping_id: &str,
        velocity: f32,
    ) -> bool {
        let Some(mapping_index) = self
            .tracks
            .get(track_index)
            .and_then(|track| track.instrument.sampler_mapping_index(mapping_id))
        else {
            return false;
        };
        let trigger = MappingAuditionTrigger {
            track_index,
            mapping_index,
            note_id: 0xa409_3822_299f_31d0,
            velocity,
        };
        self.mapping_audition = Some(trigger);
        self.fire_mapping_audition(trigger)
    }

    pub(crate) fn advance_audition_to(&mut self, target_frame: u64) -> Result<(), EngineError> {
        let target = target_frame.min(self.playback_end());
        while self.frame < target {
            let _ = self.process_frame(ProcessingMode::Monitoring)?;
        }
        Ok(())
    }

    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn request_audition_seek(&mut self) {
        self.audition_seek_requested = true;
    }

    /// Sets or clears a monitoring-only override by stable processor ID.
    pub fn set_processor_bypass(&mut self, processor_id: &str, bypass: Option<bool>) -> usize {
        self.mix
            .set_processor_bypass(processor_id, bypass, self.controls.values())
    }

    /// Enables a non-authoritative transport loop over an arrangement frame range.
    ///
    /// Call this only after advancing a fresh runtime into the requested range.
    /// The loop rewinds schedule cursors and releases active notes without
    /// allocating or clearing effect buffers in the audio callback.
    pub fn enable_audition_loop(&mut self, start: u64, end: u64) -> Result<(), EngineError> {
        if start >= end || end > self.arrangement_frames || self.frame < start || self.frame >= end
        {
            return Err(EngineError::Compile(format!(
                "audition loop [{start}, {end}) does not contain runtime frame {} within arrangement {}",
                self.frame, self.arrangement_frames
            )));
        }
        for track in &mut self.tracks {
            track.prepare_loop(start);
        }
        self.audition_loop = Some((start, end));
        Ok(())
    }

    /// Disables a previously configured non-authoritative audition loop.
    pub fn disable_audition_loop(&mut self) {
        self.audition_loop = None;
    }

    /// Enables or disables transient solo monitoring for one compiled channel identity.
    pub fn set_channel_solo(&mut self, identity: &str, enabled: bool) -> bool {
        self.mix.set_solo(identity, enabled)
    }

    /// Selects a transient pre/post-fader cue tap, or clears cue with `None`.
    pub fn set_channel_cue(&mut self, identity: Option<&str>, tap: TapPoint) -> bool {
        self.mix.set_cue(identity, tap)
    }

    /// Starts a transient audition note on the first Track using an Instrument.
    /// This does not mutate authoritative source or Project History.
    pub fn audition_note_on(
        &mut self,
        instrument_id: &str,
        note_id: u64,
        pitch: u8,
        velocity: f32,
    ) -> bool {
        let Some(track) = self
            .tracks
            .iter_mut()
            .find(|track| track.schedule.instrument_id == instrument_id)
        else {
            return false;
        };
        let seed = note_id ^ (u64::from(pitch) << 32) ^ 0x6a09_e667_f3bc_c909;
        track.instrument.dispatch(RuntimeEvent::NoteOn {
            note_id,
            pitch,
            velocity,
            seed,
            clip_envelope: ClipEnvelope::TRANSPARENT,
        });
        true
    }

    /// Starts a transient audition note on one zero-based Timeline Track.
    pub fn audition_note_on_track(
        &mut self,
        track_index: usize,
        note_id: u64,
        pitch: u8,
        velocity: f32,
    ) -> bool {
        let Some(track) = self.tracks.get_mut(track_index) else {
            return false;
        };
        let seed = note_id ^ (u64::from(pitch) << 32) ^ 0xbb67_ae85_84ca_a73b;
        track.instrument.dispatch(RuntimeEvent::NoteOn {
            note_id,
            pitch,
            velocity,
            seed,
            clip_envelope: ClipEnvelope::TRANSPARENT,
        });
        true
    }

    /// Releases one transient audition note on one zero-based Timeline Track.
    pub fn audition_note_off_track(&mut self, track_index: usize, note_id: u64) -> bool {
        let Some(track) = self.tracks.get_mut(track_index) else {
            return false;
        };
        track.instrument.dispatch(RuntimeEvent::NoteOff { note_id });
        true
    }

    /// Releases one transient audition note on the selected Instrument.
    pub fn audition_note_off(&mut self, instrument_id: &str, note_id: u64) -> bool {
        let Some(track) = self
            .tracks
            .iter_mut()
            .find(|track| track.schedule.instrument_id == instrument_id)
        else {
            return false;
        };
        track.instrument.dispatch(RuntimeEvent::NoteOff { note_id });
        true
    }

    /// Releases every graph voice, used on MIDI disconnect and shutdown.
    pub fn all_notes_off(&mut self) {
        for track in &mut self.tracks {
            track.instrument.all_notes_off();
        }
    }

    /// Fills a preallocated interleaved-neutral stereo block.
    pub fn process_block(
        &mut self,
        output: &mut [[f32; 2]],
        mode: ProcessingMode,
    ) -> Result<usize, EngineError> {
        let available = self.playback_end().saturating_sub(self.frame);
        let count = if self.audition_loop.is_some() {
            output.len()
        } else {
            usize::try_from(available.min(output.len() as u64)).unwrap_or(output.len())
        };
        for frame in &mut output[..count] {
            *frame = self.process_frame(mode)?;
        }
        output[count..].fill([0.0, 0.0]);
        Ok(count)
    }

    /// True after arrangement plus authored tail has been processed.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.audition_loop.is_none() && self.frame >= self.playback_end()
    }

    /// Current absolute frame.
    #[must_use]
    pub const fn frame(&self) -> u64 {
        self.frame
    }

    /// Current engine telemetry.
    #[must_use]
    pub const fn telemetry(&self) -> RuntimeTelemetry {
        self.telemetry
    }

    /// Project identity for runtime reporting.
    #[must_use]
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Sample rate.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Arrangement frame count.
    #[must_use]
    pub const fn arrangement_frames(&self) -> u64 {
        self.arrangement_frames
    }
}

struct TrackRuntime {
    schedule: Arc<TrackSchedule>,
    #[cfg(any(feature = "pipewire", test))]
    semantic_hash: [u8; 32],
    cursor: usize,
    loop_cursor: usize,
    #[cfg(any(feature = "pipewire", test))]
    transition_unchanged: bool,
    instrument: InstrumentRuntime,
}

impl TrackRuntime {
    fn new(track: &CompiledTrack, sample_rate: u32, seed: u64) -> Self {
        let instrument = match &track.program {
            InstrumentProgram::Graph { graph, polyphony } => InstrumentRuntime::Graph {
                graph: graph.clone(),
                voices: (0..*polyphony)
                    .map(|slot| GraphVoiceSlot::new(graph, seed ^ slot as u64))
                    .collect(),
            },
            InstrumentProgram::Drums {
                instrument,
                tone,
                punch,
            } => InstrumentRuntime::Drums {
                synth: DrumSynth::new(instrument, sample_rate),
                tone: *tone,
                punch: *punch,
            },
            InstrumentProgram::Sampler(program) => InstrumentRuntime::Sampler(program.runtime()),
        };
        Self {
            schedule: track.schedule.clone(),
            #[cfg(any(feature = "pipewire", test))]
            semantic_hash: track.semantic_hash,
            cursor: 0,
            loop_cursor: 0,
            #[cfg(any(feature = "pipewire", test))]
            transition_unchanged: false,
            instrument,
        }
    }

    #[cfg(any(feature = "pipewire", test))]
    fn same_semantics(&self, other: &Self) -> bool {
        self.schedule.track_id == other.schedule.track_id
            && self.semantic_hash == other.semantic_hash
    }

    #[cfg(any(feature = "pipewire", test))]
    fn adopt_state_from(&mut self, current: &mut Self) {
        self.cursor = current.cursor;
        self.loop_cursor = current.loop_cursor;
        self.instrument.adopt_state_from(&mut current.instrument);
    }

    fn seek_transport(&mut self, frame: u64) {
        self.realign_pristine_transport(frame);
        self.instrument.all_notes_off();
    }

    fn realign_pristine_transport(&mut self, frame: u64) {
        self.cursor = self
            .schedule
            .events
            .partition_point(|event| event.frame < frame);
    }

    fn prepare_loop(&mut self, start: u64) {
        self.loop_cursor = self
            .schedule
            .events
            .partition_point(|event| event.frame < start);
    }

    fn rewind_loop(&mut self) {
        self.cursor = self.loop_cursor;
        self.instrument.all_notes_off();
    }

    fn dispatch(&mut self, frame: u64) {
        while self
            .schedule
            .events
            .get(self.cursor)
            .is_some_and(|event| event.frame == frame)
        {
            let event = self.schedule.events[self.cursor].kind.clone();
            self.cursor += 1;
            self.instrument.dispatch(event);
        }
    }

    #[cfg(any(feature = "pipewire", test))]
    fn active_voice_count(&self) -> usize {
        self.instrument.active_voice_count()
    }

    fn process(&mut self, controls: &[f64], unconstrained_controls: &[f64]) -> ([f32; 2], usize) {
        self.instrument.process(controls, unconstrained_controls)
    }
}

const GRAPH_STEAL_CROSSFADE_FRAMES: u32 = 64;

struct GraphVoiceSlot {
    voice: GraphVoice,
    replacement_tail: GraphVoice,
    envelope: ClipEnvelope,
    replacement_tail_envelope: ClipEnvelope,
    crossfade_remaining: u32,
}

impl GraphVoiceSlot {
    fn new(graph: &CompiledGraph, seed: u64) -> Self {
        Self {
            voice: graph.create_voice(seed),
            replacement_tail: graph.create_voice(seed ^ 0xa5a5_a5a5_a5a5_a5a5),
            envelope: ClipEnvelope::TRANSPARENT,
            replacement_tail_envelope: ClipEnvelope::TRANSPARENT,
            crossfade_remaining: 0,
        }
    }

    fn activate(
        &mut self,
        note_id: u64,
        pitch: u8,
        velocity: f32,
        seed: u64,
        envelope: ClipEnvelope,
    ) {
        if self.voice.active() {
            self.voice.release_all();
            std::mem::swap(&mut self.voice, &mut self.replacement_tail);
            std::mem::swap(&mut self.envelope, &mut self.replacement_tail_envelope);
            self.crossfade_remaining = GRAPH_STEAL_CROSSFADE_FRAMES;
        }
        self.voice.activate(note_id, pitch, velocity, seed);
        self.envelope = envelope;
    }

    fn release(&mut self, note_id: u64) {
        self.voice.release(note_id);
        self.replacement_tail.release(note_id);
    }

    fn release_all(&mut self) {
        self.voice.release_all();
        self.replacement_tail.release_all();
    }

    const fn active(&self) -> bool {
        self.voice.active() || (self.crossfade_remaining > 0 && self.replacement_tail.active())
    }

    const fn primary_active(&self) -> bool {
        self.voice.active()
    }

    const fn releasing(&self) -> bool {
        self.voice.releasing()
    }

    const fn age_frames(&self) -> u64 {
        self.voice.age_frames()
    }

    fn process(
        &mut self,
        graph: &CompiledGraph,
        controls: &[f64],
        unconstrained_controls: &[f64],
    ) -> [f32; 2] {
        let current = if self.voice.active() {
            let gain = self.envelope.gain_at(self.voice.age_frames());
            let sample = graph.process(&mut self.voice, controls, unconstrained_controls);
            [sample[0] * gain, sample[1] * gain]
        } else {
            [0.0, 0.0]
        };
        if self.crossfade_remaining == 0 {
            return current;
        }
        let tail = if self.replacement_tail.active() {
            let gain = self
                .replacement_tail_envelope
                .gain_at(self.replacement_tail.age_frames());
            let sample =
                graph.process(&mut self.replacement_tail, controls, unconstrained_controls);
            [sample[0] * gain, sample[1] * gain]
        } else {
            [0.0, 0.0]
        };
        let progress = (GRAPH_STEAL_CROSSFADE_FRAMES - self.crossfade_remaining + 1) as f32
            / GRAPH_STEAL_CROSSFADE_FRAMES as f32;
        self.crossfade_remaining -= 1;
        if self.crossfade_remaining == 0 {
            self.replacement_tail.stop();
        }
        [
            tail[0] * (1.0 - progress) + current[0] * progress,
            tail[1] * (1.0 - progress) + current[1] * progress,
        ]
    }
}

enum InstrumentRuntime {
    Graph {
        graph: Arc<CompiledGraph>,
        voices: Vec<GraphVoiceSlot>,
    },
    Drums {
        synth: DrumSynth,
        tone: Option<ControlIndex>,
        punch: Option<ControlIndex>,
    },
    Sampler(SamplerRuntime),
}

impl InstrumentRuntime {
    #[cfg(any(feature = "pipewire", test))]
    fn adopt_state_from(&mut self, current: &mut Self) {
        match (self, current) {
            (
                Self::Graph {
                    voices: replacement,
                    ..
                },
                Self::Graph {
                    voices: current, ..
                },
            ) => std::mem::swap(replacement, current),
            (
                Self::Drums {
                    synth: replacement, ..
                },
                Self::Drums { synth: current, .. },
            ) => std::mem::swap(replacement, current),
            (Self::Sampler(replacement), Self::Sampler(current)) => {
                std::mem::swap(replacement, current);
            }
            _ => {}
        }
    }

    fn sampler_mapping_index(&self, mapping_id: &str) -> Option<u16> {
        match self {
            Self::Sampler(sampler) => sampler.mapping_index(mapping_id),
            Self::Graph { .. } | Self::Drums { .. } => None,
        }
    }

    fn sampler_mapping_on(&mut self, note_id: u64, mapping_index: u16, velocity: f32) -> bool {
        match self {
            Self::Sampler(sampler) => sampler.mapping_on(note_id, mapping_index, velocity),
            Self::Graph { .. } | Self::Drums { .. } => false,
        }
    }

    fn dispatch(&mut self, event: RuntimeEvent) {
        match (self, event) {
            (
                Self::Graph { voices, .. },
                RuntimeEvent::NoteOn {
                    note_id,
                    pitch,
                    velocity,
                    seed,
                    clip_envelope,
                },
            ) => {
                let slot = select_voice(voices);
                voices[slot].activate(note_id, pitch, velocity, seed, clip_envelope);
            }
            (Self::Graph { voices, .. }, RuntimeEvent::NoteOff { note_id }) => {
                for voice in voices {
                    voice.release(note_id);
                }
            }
            (
                Self::Drums { synth: drums, .. },
                RuntimeEvent::Hit {
                    voice,
                    velocity,
                    seed,
                    clip_envelope,
                },
            ) => {
                drums.trigger(voice, velocity, seed, clip_envelope);
            }
            (
                Self::Drums { synth: drums, .. },
                RuntimeEvent::NoteOn {
                    pitch,
                    velocity,
                    seed,
                    clip_envelope,
                    ..
                },
            ) => {
                if let Some(voice) = midi_drum_voice(pitch) {
                    drums.trigger(voice, velocity, seed, clip_envelope);
                }
            }
            (
                Self::Sampler(sampler),
                RuntimeEvent::NoteOn {
                    note_id,
                    pitch,
                    velocity,
                    clip_envelope,
                    ..
                },
            ) => sampler.note_on(note_id, pitch, velocity, clip_envelope),
            (Self::Sampler(sampler), RuntimeEvent::NoteOff { note_id }) => {
                sampler.note_off(note_id);
            }
            (
                Self::Sampler(sampler),
                RuntimeEvent::SampleOn {
                    note_id,
                    mapping_index,
                    start_frame,
                    end_frame,
                    gain_db,
                    pan,
                    fade_in_frames,
                    fade_out_frames,
                    direction,
                    loop_region,
                    velocity,
                    clip_envelope,
                },
            ) => sampler.sample_on(
                note_id,
                mapping_index,
                start_frame,
                end_frame,
                gain_db,
                pan,
                fade_in_frames,
                fade_out_frames,
                direction,
                loop_region,
                velocity,
                clip_envelope,
            ),
            (Self::Sampler(sampler), RuntimeEvent::SampleOff { note_id }) => {
                sampler.sample_off(note_id);
            }
            _ => {}
        }
    }

    fn all_notes_off(&mut self) {
        match self {
            Self::Graph { voices, .. } => {
                for voice in voices {
                    voice.release_all();
                }
            }
            Self::Sampler(sampler) => sampler.all_notes_off(),
            Self::Drums { .. } => {}
        }
    }

    #[cfg(any(feature = "pipewire", test))]
    fn active_voice_count(&self) -> usize {
        match self {
            Self::Graph { voices, .. } => voices.iter().filter(|voice| voice.active()).count(),
            Self::Drums { synth, .. } => synth.active_voice_count(),
            Self::Sampler(sampler) => sampler.active_voice_count(),
        }
    }

    fn process(&mut self, controls: &[f64], unconstrained_controls: &[f64]) -> ([f32; 2], usize) {
        match self {
            Self::Graph { graph, voices } => {
                let mut output = [0.0_f32, 0.0_f32];
                let mut active = 0;
                for voice in voices {
                    if voice.active() {
                        active += 1;
                        let sample = voice.process(graph, controls, unconstrained_controls);
                        output[0] += sample[0];
                        output[1] += sample[1];
                    }
                }
                (output, active)
            }
            Self::Drums { synth, tone, punch } => {
                let tone = tone.map_or(0.55, |index| controls[index.0]);
                let punch = punch.map_or(0.65, |index| controls[index.0]);
                (synth.process(tone, punch), synth.active_voice_count())
            }
            Self::Sampler(sampler) => {
                let output = sampler.process(controls);
                (output, sampler.active_voice_count())
            }
        }
    }
}

fn midi_drum_voice(pitch: u8) -> Option<DrumVoice> {
    match pitch {
        35 | 36 => Some(DrumVoice::Kick),
        37 | 38 | 40 => Some(DrumVoice::Snare),
        39 => Some(DrumVoice::Clap),
        42 | 44 | 46 => Some(DrumVoice::Hat),
        _ => None,
    }
}

fn select_voice(voices: &[GraphVoiceSlot]) -> usize {
    voices
        .iter()
        .position(|voice| !voice.primary_active())
        .or_else(|| {
            voices
                .iter()
                .enumerate()
                .filter(|(_, voice)| voice.releasing())
                .max_by_key(|(_, voice)| voice.age_frames())
                .map(|(index, _)| index)
        })
        .or_else(|| {
            voices
                .iter()
                .enumerate()
                .max_by_key(|(_, voice)| voice.age_frames())
                .map(|(index, _)| index)
        })
        .unwrap_or(0)
}

fn monitor_sample(sample: f32) -> f32 {
    if !sample.is_finite() {
        return 0.0;
    }
    let absolute = sample.abs();
    if absolute <= 0.9 {
        sample
    } else {
        sample.signum() * (0.9 + (absolute - 0.9).tanh() * 0.099)
    }
}

fn seconds_to_frames(seconds: f64, sample_rate: u32) -> Result<u64, EngineError> {
    let frames = seconds * f64::from(sample_rate);
    if !frames.is_finite() || frames < 0.0 || frames > u64::MAX as f64 {
        return Err(EngineError::Compile(
            "render tail exceeds frame limits".to_owned(),
        ));
    }
    Ok((frames + 0.5).floor() as u64)
}

/// Whether output passes through the non-authoritative monitor safety stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingMode {
    /// Bit-stable authored output; non-finite samples fail.
    Offline,
    /// Sanitized and emergency-limited local monitoring output.
    Monitoring,
}

/// Low-cost runtime status suitable for terminal visualization.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RuntimeTelemetry {
    /// Last processed frame.
    pub frame: u64,
    /// Current absolute stereo peak.
    pub current_peak: f32,
    /// Maximum peak observed since runtime creation.
    pub peak: f32,
    /// Currently active graph and percussion voices.
    pub active_voices: usize,
    /// Number of valid compiled channel entries in `channel_activity`.
    pub channel_count: usize,
    /// Fixed-capacity per-channel peak and average activity in compiled execution order.
    /// Identities and roles are published by `MixGraphInspection::channels` at the same indexes.
    #[serde(with = "channel_meter_array_serde")]
    pub channel_activity: [ChannelMeter; MAX_MIX_CHANNELS],
    /// Number of valid compiled Envelope Follower entries in `follower_activity`.
    pub follower_count: usize,
    /// Fixed-capacity Audio-derived control telemetry aligned to follower inspection indexes.
    #[serde(with = "follower_meter_array_serde")]
    pub follower_activity: [EnvelopeFollowerMeter; MAX_ENVELOPE_FOLLOWERS],
    /// Number of valid compiled processor entries in `processor_activity`.
    pub processor_count: usize,
    /// Fixed-capacity processor telemetry aligned to `CompiledProcessorInspection::index`.
    #[serde(with = "processor_meter_array_serde")]
    pub processor_activity: [ProcessorMeter; MAX_PROCESSOR_INSTANCES],
}

impl Default for RuntimeTelemetry {
    fn default() -> Self {
        Self {
            frame: 0,
            current_peak: 0.0,
            peak: 0.0,
            active_voices: 0,
            channel_count: 0,
            channel_activity: [ChannelMeter::default(); MAX_MIX_CHANNELS],
            follower_count: 0,
            follower_activity: [EnvelopeFollowerMeter::default(); MAX_ENVELOPE_FOLLOWERS],
            processor_count: 0,
            processor_activity: [ProcessorMeter::default(); MAX_PROCESSOR_INSTANCES],
        }
    }
}

mod channel_meter_array_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    use super::{ChannelMeter, MAX_MIX_CHANNELS};

    pub(super) fn serialize<S>(
        value: &[ChannelMeter; MAX_MIX_CHANNELS],
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.as_slice().serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<[ChannelMeter; MAX_MIX_CHANNELS], D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<ChannelMeter>::deserialize(deserializer)?;
        if values.len() != MAX_MIX_CHANNELS {
            return Err(D::Error::invalid_length(
                values.len(),
                &"the fixed compiled mix-channel capacity",
            ));
        }
        let mut activity = [ChannelMeter::default(); MAX_MIX_CHANNELS];
        activity.copy_from_slice(&values);
        Ok(activity)
    }
}

mod follower_meter_array_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    use super::{EnvelopeFollowerMeter, MAX_ENVELOPE_FOLLOWERS};

    pub(super) fn serialize<S>(
        value: &[EnvelopeFollowerMeter; MAX_ENVELOPE_FOLLOWERS],
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.as_slice().serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<[EnvelopeFollowerMeter; MAX_ENVELOPE_FOLLOWERS], D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<EnvelopeFollowerMeter>::deserialize(deserializer)?;
        if values.len() != MAX_ENVELOPE_FOLLOWERS {
            return Err(D::Error::invalid_length(
                values.len(),
                &"the fixed Envelope Follower telemetry capacity",
            ));
        }
        let mut activity = [EnvelopeFollowerMeter::default(); MAX_ENVELOPE_FOLLOWERS];
        activity.copy_from_slice(&values);
        Ok(activity)
    }
}

mod processor_meter_array_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    use super::{MAX_PROCESSOR_INSTANCES, ProcessorMeter};

    pub(super) fn serialize<S>(
        value: &[ProcessorMeter; MAX_PROCESSOR_INSTANCES],
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.as_slice().serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<[ProcessorMeter; MAX_PROCESSOR_INSTANCES], D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<ProcessorMeter>::deserialize(deserializer)?;
        if values.len() != MAX_PROCESSOR_INSTANCES {
            return Err(D::Error::invalid_length(
                values.len(),
                &"the fixed compiled processor telemetry capacity",
            ));
        }
        let mut activity = [ProcessorMeter::default(); MAX_PROCESSOR_INSTANCES];
        activity.copy_from_slice(&values);
        Ok(activity)
    }
}

/// Audio compilation and processing failures.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// Project did not pass whole-project validation.
    #[error("Project cannot compile because validation failed")]
    Validation(Vec<Diagnostic>),
    /// Validated model could not compile to bounded runtime state.
    #[error("audio compilation failed: {0}")]
    Compile(String),
    /// Exact musical-time arithmetic failed.
    #[error(transparent)]
    Time(#[from] crate::time::TimeError),
    /// Offline authored output became non-finite.
    #[error("offline render produced a non-finite sample at frame {frame}")]
    NonFinite {
        /// First failing absolute frame.
        frame: u64,
    },
    /// WAV or publication I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// WAV encoding failed.
    #[error(transparent)]
    Wav(#[from] hound::Error),
    /// Manifest serialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use crate::{dsl::parse_project, model::TapPoint};

    use super::{
        ClipEnvelope, CompiledProject, MAX_PROCESSOR_INSTANCES, ProcessingMode, RuntimeTelemetry,
    };

    fn fixture(source: &str) -> crate::model::Project {
        let document = parse_project(Path::new("project.crump"), source);
        assert!(
            !document.diagnostics.has_errors(),
            "{:?}",
            document.diagnostics.as_slice()
        );
        document.project.expect("parsed fixture")
    }

    #[test]
    fn runtime_telemetry_processor_array_round_trips_at_fixed_capacity() {
        let mut telemetry = RuntimeTelemetry {
            processor_count: 2,
            ..RuntimeTelemetry::default()
        };
        telemetry.processor_activity[0].gain_reduction_db = 3.5;
        telemetry.processor_activity[MAX_PROCESSOR_INSTANCES - 1].maximum_gain_reduction_db = 9.0;
        let encoded = serde_json::to_value(telemetry).expect("serialize telemetry");
        assert_eq!(
            encoded["processor_activity"]
                .as_array()
                .expect("processor array")
                .len(),
            MAX_PROCESSOR_INSTANCES
        );
        let decoded: RuntimeTelemetry =
            serde_json::from_value(encoded.clone()).expect("deserialize telemetry");
        assert_eq!(decoded.processor_count, 2);
        assert_eq!(decoded.processor_activity[0].gain_reduction_db, 3.5);
        assert_eq!(
            decoded.processor_activity[MAX_PROCESSOR_INSTANCES - 1].maximum_gain_reduction_db,
            9.0
        );

        let mut malformed = encoded;
        malformed["processor_activity"]
            .as_array_mut()
            .expect("processor array")
            .pop();
        let error = serde_json::from_value::<RuntimeTelemetry>(malformed)
            .expect_err("short fixed-capacity processor array must fail");
        assert!(error.to_string().contains("fixed compiled processor"));
    }

    #[test]
    fn monitoring_stage_is_transparent_below_threshold() {
        assert_eq!(super::monitor_sample(0.5), 0.5);
        assert!(super::monitor_sample(100.0) < 1.0);
        assert_eq!(super::monitor_sample(f32::NAN), 0.0);
    }

    #[test]
    fn graph_voice_replacement_uses_preallocated_short_crossfade() {
        let source = include_str!("../../tests/fixtures/minimal.crump").replacen(
            "      polyphony 4",
            "      polyphony 1",
            1,
        );
        let project = fixture(&source);
        let compiled = CompiledProject::compile(&project).expect("compile");
        let graph = compiled
            .tracks
            .iter()
            .find_map(|track| match &track.program {
                super::InstrumentProgram::Graph { graph, .. }
                    if track.schedule.instrument_id == "lead" =>
                {
                    Some(graph.clone())
                }
                _ => None,
            })
            .expect("lead graph");
        let mut controls = compiled.controls.runtime();
        controls.evaluate_frame(0);
        let mut slot = super::GraphVoiceSlot::new(&graph, 5);
        slot.activate(1, 60, 1.0, 101, ClipEnvelope::TRANSPARENT);
        let mut previous = [0.0, 0.0];
        for _ in 0..2_000 {
            previous = slot.process(&graph, controls.values(), controls.unconstrained_values());
        }

        slot.activate(2, 72, 1.0, 202, ClipEnvelope::TRANSPARENT);
        assert_eq!(
            slot.crossfade_remaining,
            super::GRAPH_STEAL_CROSSFADE_FRAMES
        );
        let replacement = slot.process(&graph, controls.values(), controls.unconstrained_values());
        assert!(
            (replacement[0] - previous[0]).abs() < 0.1
                && (replacement[1] - previous[1]).abs() < 0.1,
            "replacement discontinuity: {previous:?} -> {replacement:?}"
        );
        for _ in 1..super::GRAPH_STEAL_CROSSFADE_FRAMES {
            let sample = slot.process(&graph, controls.values(), controls.unconstrained_values());
            assert!(sample.into_iter().all(f32::is_finite));
        }
        assert_eq!(slot.crossfade_remaining, 0);
        assert!(!slot.replacement_tail.active());

        let resources = &compiled.mix_graph_inspection().resources;
        let instrument = project
            .song
            .instruments
            .iter()
            .find(|instrument| instrument.id == "lead")
            .expect("lead Instrument");
        let one_voice = super::graph::authored_graph_voice_state_bytes(
            instrument,
            project.sample_rate,
            compiled.controls.target_count(),
        );
        assert!(
            resources.graph_voice_state_bytes >= one_voice * super::GRAPH_VOICE_STORAGE_MULTIPLIER
        );
    }

    #[test]
    fn monitoring_reports_non_finite_output_and_keeps_transport_moving() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile");
        let mut runtime = compiled.runtime();
        runtime.mix.poison_track_level_for_test(0);

        let error = runtime
            .process_frame(ProcessingMode::Monitoring)
            .expect_err("non-finite monitoring output");
        assert!(matches!(error, super::EngineError::NonFinite { frame: 0 }));
        assert_eq!(runtime.frame(), 1);
        assert_eq!(runtime.telemetry().current_peak, 0.0);
    }

    #[test]
    fn publication_seek_reuses_every_preallocated_mix_buffer() {
        let project = fixture(include_str!("../../tests/fixtures/production_mix.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile production mix");
        let mut runtime = compiled.runtime();
        for _ in 0..2_000 {
            runtime
                .process_frame(ProcessingMode::Monitoring)
                .expect("dirty processor state");
        }
        let storage = runtime.mix.heap_storage_fingerprint();
        runtime.seek_transport(1_000);
        assert_eq!(runtime.mix.heap_storage_fingerprint(), storage);
        runtime.seek_transport(2_000);
        assert_eq!(runtime.mix.heap_storage_fingerprint(), storage);
    }

    #[test]
    fn callback_transition_realign_does_not_reset_dsp_storage() {
        let project = fixture(include_str!("../../tests/fixtures/production_mix.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile production mix");
        let mut current = compiled.runtime();
        let mut next = compiled.runtime();
        current.advance_to(2_000).expect("advance Active runtime");
        next.seek_transport(1_500);
        let reset_calls = next.mix.reset_calls();
        let storage = next.mix.heap_storage_fingerprint();

        current.prepare_transition(&mut next);

        assert_eq!(next.frame(), current.frame());
        assert_eq!(next.mix.reset_calls(), reset_calls);
        assert_eq!(next.mix.heap_storage_fingerprint(), storage);
    }

    #[test]
    fn complete_runtime_admission_counts_shared_programs_and_two_resident_slots() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile graph fixture");
        let resources = &compiled.mix_graph_inspection().resources;
        let topology = crate::routing::AuthoredTopology::compile(&project);
        let validation_estimate =
            super::mix_graph::CompiledMixGraph::resource_estimate_for_validation(
                &project, &topology,
            )
            .expect("validation estimate");
        assert_eq!(resources, &validation_estimate);
        assert!(resources.control_program_bytes > 0);
        assert!(resources.schedule_program_bytes > 0);
        assert!(resources.instrument_program_bytes > 0);
        assert!(resources.drum_voice_state_bytes > 0);
        assert!(resources.song_runtime_structure_bytes > 0);
        let accounted = resources
            .bus_buffer_bytes
            .saturating_add(resources.meter_bytes)
            .saturating_add(resources.processor_meter_bytes)
            .saturating_add(resources.graph_voice_state_bytes)
            .saturating_add(resources.drum_voice_state_bytes)
            .saturating_add(resources.sampler_voice_state_bytes)
            .saturating_add(resources.processor_state_bytes)
            .saturating_add(resources.compensation_state_bytes)
            .saturating_add(resources.compiled_program_bytes)
            .saturating_add(resources.control_program_bytes)
            .saturating_add(resources.schedule_program_bytes)
            .saturating_add(resources.instrument_program_bytes)
            .saturating_add(resources.runtime_structure_bytes)
            .saturating_add(resources.song_runtime_structure_bytes);
        assert_eq!(resources.runtime_bytes, accounted);
        assert_eq!(
            resources.publication_runtime_slots,
            super::mix_graph::MAX_RESIDENT_PUBLICATION_RUNTIMES
        );
        assert_eq!(
            resources.publication_peak_bytes,
            resources
                .runtime_bytes
                .saturating_mul(resources.publication_runtime_slots)
        );

        let first = compiled.runtime();
        let second = compiled.runtime();
        let allocated_drum_state = first
            .tracks
            .iter()
            .map(|track| match &track.instrument {
                super::InstrumentRuntime::Drums { synth, .. } => {
                    synth.allocated_voice_state_bytes()
                }
                super::InstrumentRuntime::Graph { .. } | super::InstrumentRuntime::Sampler(_) => 0,
            })
            .fold(0_u64, u64::saturating_add);
        let allocated_sampler_state = first
            .tracks
            .iter()
            .map(|track| match &track.instrument {
                super::InstrumentRuntime::Sampler(sampler) => sampler.allocated_voice_state_bytes(),
                super::InstrumentRuntime::Graph { .. } | super::InstrumentRuntime::Drums { .. } => {
                    0
                }
            })
            .fold(0_u64, u64::saturating_add);
        assert_eq!(resources.drum_voice_state_bytes, allocated_drum_state);
        assert_eq!(resources.sampler_voice_state_bytes, allocated_sampler_state);
        for (first, second) in first.tracks.iter().zip(&second.tracks) {
            assert!(Arc::ptr_eq(&first.schedule, &second.schedule));
            if let (
                super::InstrumentRuntime::Graph {
                    graph: first_graph, ..
                },
                super::InstrumentRuntime::Graph {
                    graph: second_graph,
                    ..
                },
            ) = (&first.instrument, &second.instrument)
            {
                assert!(Arc::ptr_eq(first_graph, second_graph));
            }
        }
    }

    #[test]
    fn finished_runtime_clears_current_activity_telemetry() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile");
        let mut runtime = compiled.runtime();
        runtime
            .advance_to(compiled.total_frames())
            .expect("advance to end");
        runtime
            .process_frame(ProcessingMode::Monitoring)
            .expect("finished frame");
        assert_eq!(runtime.telemetry().current_peak, 0.0);
        assert_eq!(runtime.telemetry().active_voices, 0);
        assert_eq!(runtime.telemetry().frame, compiled.total_frames());
    }

    #[test]
    fn unchanged_tracks_continue_exactly_across_transition() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile");
        let mut baseline = compiled.runtime();
        let mut current = compiled.runtime();
        let mut next = compiled.runtime();
        baseline.advance_to(1_000).expect("advance baseline");
        current.advance_to(1_000).expect("advance current");
        current.prepare_transition(&mut next);
        assert_eq!(next.frame(), 1_000);
        assert!(
            current
                .tracks
                .iter()
                .all(|track| track.transition_unchanged)
        );

        for position in 1..=480_u64 {
            let expected = baseline
                .process_frame(ProcessingMode::Monitoring)
                .expect("baseline frame");
            let phase = position as f64 / 480.0;
            let old_gain = (phase * std::f64::consts::FRAC_PI_2).cos() as f32;
            let new_gain = (phase * std::f64::consts::FRAC_PI_2).sin() as f32;
            let actual = current
                .process_transition_frame(&mut next, old_gain, new_gain)
                .expect("transition frame");
            assert_eq!(actual, expected);
        }
        current.complete_transition(&mut next);
        assert!(next.tracks.iter().all(|track| !track.transition_unchanged));

        for _ in 0..128 {
            assert_eq!(
                next.process_frame(ProcessingMode::Monitoring)
                    .expect("replacement frame"),
                baseline
                    .process_frame(ProcessingMode::Monitoring)
                    .expect("baseline frame")
            );
        }
    }

    #[test]
    fn audition_loop_is_deterministic_across_supported_callback_grouping() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile loop fixture");
        let mut reference = None;
        for quantum in super::mix_graph::SUPPORTED_QUANTA {
            let mut runtime = compiled.runtime();
            runtime
                .enable_audition_loop(0, 512)
                .expect("enable exact audition loop");
            let mut rendered = Vec::with_capacity(4_096);
            for _ in 0..(4_096 / quantum) {
                let mut block = vec![[0.0_f32; 2]; quantum];
                assert_eq!(
                    runtime
                        .process_block(&mut block, ProcessingMode::Monitoring)
                        .expect("loop callback block"),
                    quantum
                );
                rendered.extend(block);
            }
            assert_eq!(runtime.frame(), 512);
            if let Some(expected) = &reference {
                assert_eq!(&rendered, expected, "callback quantum {quantum}");
            } else {
                reference = Some(rendered);
            }
        }
    }

    #[test]
    fn production_routing_monitoring_matches_offline_at_supported_quanta() {
        let project = fixture(include_str!("../../tests/fixtures/production_mix.crump"));
        let compiled = CompiledProject::compile(&project).expect("compiled Project");
        let target_frames = 70_000_usize;
        let mut reference = None;
        for quantum in crate::engine::mix_graph::SUPPORTED_QUANTA {
            let mut offline = compiled.runtime();
            let mut monitoring = compiled.runtime();
            let mut heard_audio = false;
            let mut rendered = Vec::with_capacity(target_frames);
            for _ in 0..target_frames.div_ceil(quantum) {
                let mut expected = vec![[0.0_f32; 2]; quantum];
                let mut actual = vec![[0.0_f32; 2]; quantum];
                offline
                    .process_block(&mut expected, ProcessingMode::Offline)
                    .expect("offline routing");
                monitoring
                    .process_block(&mut actual, ProcessingMode::Monitoring)
                    .expect("monitoring routing");
                heard_audio |= expected.iter().flatten().any(|sample| *sample != 0.0);
                assert_eq!(actual, expected, "quantum {quantum}");
                let remaining = target_frames.saturating_sub(rendered.len());
                rendered.extend_from_slice(&expected[..remaining.min(quantum)]);
            }
            assert!(heard_audio, "fixture was silent at quantum {quantum}");
            if let Some(reference) = &reference {
                assert_eq!(&rendered, reference, "offline quantum {quantum}");
            } else {
                reference = Some(rendered);
            }
            let telemetry = monitoring.telemetry();
            let processors = &compiled.mix_graph_inspection().processors;
            assert_eq!(telemetry.processor_count, processors.len());
            let compressor = processors
                .iter()
                .find(|processor| processor.id == "bass_comp")
                .expect("compressor inspection");
            assert!(
                telemetry.processor_activity[compressor.index].maximum_gain_reduction_db > 0.0,
                "sidechain compressor telemetry must reach the runtime status seam"
            );
            let stutter = processors
                .iter()
                .find(|processor| processor.id == "bus_stutter")
                .expect("stutter inspection");
            assert!(
                telemetry.processor_activity[stutter.index].stutter_ignored_triggers > 0,
                "warm-up and active-repeat trigger policy must be observable"
            );
        }
    }

    #[test]
    fn offline_processing_ignores_transient_solo_and_cue() {
        let project = fixture(include_str!("../../tests/fixtures/production_mix.crump"));
        let compiled = CompiledProject::compile(&project).expect("compiled Project");
        let mut baseline = compiled.runtime();
        let mut auditioned = compiled.runtime();
        assert!(auditioned.set_channel_solo("return/room", true));
        assert!(auditioned.set_channel_cue(Some("track/drums"), TapPoint::PreFader));
        for _ in 0..1_024 {
            assert_eq!(
                baseline.process_frame(ProcessingMode::Offline).unwrap(),
                auditioned.process_frame(ProcessingMode::Offline).unwrap()
            );
        }

        let minimal = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&minimal).expect("compiled Project");
        let mut baseline = compiled.runtime();
        let mut auditioned = compiled.runtime();
        assert!(auditioned.set_channel_cue(Some("track/lead"), TapPoint::PreFader));
        assert!((0..1_024).any(|_| {
            baseline.process_frame(ProcessingMode::Monitoring).unwrap()
                != auditioned
                    .process_frame(ProcessingMode::Monitoring)
                    .unwrap()
        }));
    }

    #[test]
    fn integrated_production_fixture_renders_a_decaying_post_arrangement_tail() {
        let project = fixture(include_str!("../../tests/fixtures/production_mix.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile production mix");
        let mut runtime = compiled.runtime();
        runtime
            .advance_to(compiled.arrangement_frames())
            .expect("advance to render tail");
        let tail_frames = compiled
            .total_frames()
            .saturating_sub(compiled.arrangement_frames());
        assert_eq!(tail_frames, 96_000);
        let window = 24_000_u64;
        let mut early_energy = 0.0_f64;
        let mut late_energy = 0.0_f64;
        for offset in 0..tail_frames {
            let sample = runtime
                .process_frame(ProcessingMode::Offline)
                .expect("tail frame");
            let energy = f64::from(sample[0]).powi(2) + f64::from(sample[1]).powi(2);
            if offset < window {
                early_energy += energy;
            }
            if offset >= tail_frames - window {
                late_energy += energy;
            }
        }
        assert!(
            early_energy > 1.0e-12,
            "tail must contain routed effects: early={early_energy:e}, late={late_energy:e}"
        );
        assert!(
            late_energy < early_energy,
            "tail must decay: early={early_energy:e}, late={late_energy:e}"
        );
    }

    #[test]
    fn publication_transition_refreshes_processor_telemetry_for_both_runtimes() {
        let project = fixture(include_str!("../../tests/fixtures/production_mix.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile production mix");
        let expected_processors = compiled.mix_graph_inspection().processors.len();
        let mut current = compiled.runtime();
        let mut next = compiled.runtime();
        current.advance_to(5_000).expect("advance current");
        current.prepare_transition(&mut next);
        current
            .process_transition_frame(&mut next, 0.75, 0.25)
            .expect("publication frame");

        assert_eq!(current.telemetry.processor_count, expected_processors);
        assert_eq!(next.telemetry.processor_count, expected_processors);
        assert_eq!(
            next.telemetry.processor_activity,
            current.telemetry.processor_activity
        );
    }

    #[test]
    fn compatible_processor_graph_state_runs_once_and_survives_publication() {
        let source = include_str!("../../tests/fixtures/production_mix.crump");
        let compiled = CompiledProject::compile(&fixture(source)).expect("compile production mix");
        let shifted =
            CompiledProject::compile(&fixture(&format!("# display-only publication\n{source}")))
                .expect("compile shifted production mix");
        let mut baseline = compiled.runtime();
        let mut current = compiled.runtime();
        let mut next = shifted.runtime();
        baseline.advance_to(5_000).expect("advance baseline");
        current.advance_to(5_000).expect("advance current");
        current.prepare_transition(&mut next);
        assert!(current.mix.topology_compatible(&next.mix));

        for position in 1..=480_u64 {
            let expected = baseline
                .process_frame(ProcessingMode::Monitoring)
                .expect("baseline processor frame");
            let phase = position as f64 / 480.0;
            let actual = current
                .process_transition_frame(
                    &mut next,
                    (phase * std::f64::consts::FRAC_PI_2).cos() as f32,
                    (phase * std::f64::consts::FRAC_PI_2).sin() as f32,
                )
                .expect("processor transition frame");
            assert_eq!(actual, expected, "processor frame {position}");
        }
        current.complete_transition(&mut next);
        for _ in 0..256 {
            assert_eq!(
                next.process_frame(ProcessingMode::Monitoring)
                    .expect("replacement processor frame"),
                baseline
                    .process_frame(ProcessingMode::Monitoring)
                    .expect("baseline processor continuation")
            );
        }
    }

    #[test]
    fn unrelated_publication_preserves_exact_linear_and_step_automation() {
        let track = "    track lead \"Lead\" instrument lead level -9db pan -0.1 {";
        for (curve, start_frame) in [("linear", 10_000_u64), ("step", 23_900)] {
            let source = include_str!("../../tests/fixtures/minimal.crump").replace(
                track,
                &format!(
                    "    automation lead-level target track/lead/level {{\n      point start 0bar -18db {curve}\n      point lift 1beat -6db step\n    }}\n{track}"
                ),
            );
            let compiled = CompiledProject::compile(&fixture(&source)).expect("compile Automation");
            let shifted = CompiledProject::compile(&fixture(&format!(
                "# unrelated display-only publication\n{source}"
            )))
            .expect("compile shifted Automation");
            let mut baseline = compiled.runtime();
            let mut current = compiled.runtime();
            let mut next = shifted.runtime();
            baseline.advance_to(start_frame).expect("advance baseline");
            current.advance_to(start_frame).expect("advance current");
            current.prepare_transition(&mut next);
            assert!(!current.scalar_control_handoff);

            for position in 1..=480_u64 {
                let expected = baseline
                    .process_frame(ProcessingMode::Monitoring)
                    .expect("baseline Automation frame");
                let phase = position as f64 / 480.0;
                let actual = current
                    .process_transition_frame(
                        &mut next,
                        (phase * std::f64::consts::FRAC_PI_2).cos() as f32,
                        (phase * std::f64::consts::FRAC_PI_2).sin() as f32,
                    )
                    .expect("publication Automation frame");
                assert_eq!(actual, expected, "curve {curve} at offset {position}");
            }
        }
    }

    #[test]
    fn compatible_publication_retains_loop_and_relative_transport_exactly() {
        let source = include_str!("../../tests/fixtures/minimal.crump");
        let compiled = CompiledProject::compile(&fixture(source)).expect("compile loop fixture");
        let shifted =
            CompiledProject::compile(&fixture(&format!("# display-only change\n{source}")))
                .expect("compile replacement loop fixture");
        let mut baseline = compiled.runtime();
        let mut current = compiled.runtime();
        let mut next = shifted.runtime();
        for runtime in [&mut baseline, &mut current] {
            runtime.advance_to(100).expect("advance loop start");
            runtime
                .enable_audition_loop(100, 500)
                .expect("enable retained loop");
            for _ in 0..250 {
                runtime
                    .process_frame(ProcessingMode::Monitoring)
                    .expect("advance inside loop");
            }
        }
        next.seek_transport(300);
        next.enable_audition_loop(100, 500)
            .expect("enable replacement loop");
        next.prime_for_publication();
        current.prepare_transition(&mut next);
        assert_eq!(next.frame(), current.frame());

        for position in 1..=480_u64 {
            let expected = baseline
                .process_frame(ProcessingMode::Monitoring)
                .expect("baseline loop frame");
            let phase = position as f64 / 480.0;
            let actual = current
                .process_transition_frame(
                    &mut next,
                    (phase * std::f64::consts::FRAC_PI_2).cos() as f32,
                    (phase * std::f64::consts::FRAC_PI_2).sin() as f32,
                )
                .expect("loop publication frame");
            assert_eq!(actual, expected, "transition frame {position}");
        }
        current.complete_transition(&mut next);
        for _ in 0..256 {
            assert_eq!(
                next.process_frame(ProcessingMode::Monitoring)
                    .expect("replacement loop continuation"),
                baseline
                    .process_frame(ProcessingMode::Monitoring)
                    .expect("baseline loop continuation")
            );
        }
    }

    #[test]
    fn explicit_loop_enable_seeks_without_preserving_old_track_state() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile");
        let mut current = compiled.runtime();
        let mut next = compiled.runtime();
        current.advance_to(1_000).expect("advance current");
        next.seek_transport(100);
        next.enable_audition_loop(100, 500).expect("enable loop");
        next.request_audition_seek();

        current.prepare_transition(&mut next);

        assert_eq!(next.frame(), 100);
        assert!(
            current
                .tracks
                .iter()
                .all(|track| !track.transition_unchanged)
        );
    }

    #[test]
    fn source_location_changes_do_not_restart_tracks() {
        let source = include_str!("../../tests/fixtures/minimal.crump");
        let original = CompiledProject::compile(&fixture(source)).expect("compile original");
        let shifted =
            CompiledProject::compile(&fixture(&format!("# shifted source locations\n{source}")))
                .expect("compile shifted");
        let mut current = original.runtime();
        let mut next = shifted.runtime();
        current.prepare_transition(&mut next);
        assert!(
            current
                .tracks
                .iter()
                .all(|track| track.transition_unchanged)
        );
    }

    #[test]
    fn automation_audio_is_independent_of_runtime_block_size() {
        let source = include_str!("../../tests/fixtures/minimal.crump")
            .replace(
                "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz",
                "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz automatable",
            )
            .replace(
                "    track lead \"Lead\" instrument lead level -9db pan -0.1 {",
                r#"    automation lead-tone target instrument/lead/tone {
      point dark 0bar 800hz linear
      point open 2beat 8000hz power(2)
      point settle 3beat 2400hz step
    }
    automation lead-level target track/lead/level {
      point quiet 0bar -18db linear
      point present 2beat -6db step
    }
    track lead "Lead" instrument lead level -9db pan -0.1 {"#,
            );
        let compiled = CompiledProject::compile(&fixture(&source)).expect("compile Automation");
        let frame_count = 80_000;
        let mut scalar_runtime = compiled.runtime();
        let scalar = (0..frame_count)
            .map(|_| {
                scalar_runtime
                    .process_frame(ProcessingMode::Offline)
                    .expect("scalar frame")
            })
            .collect::<Vec<_>>();

        let mut block_runtime = compiled.runtime();
        let mut blocked = Vec::with_capacity(frame_count);
        let sizes = [1_usize, 7, 64, 257, 1_024];
        let mut block_index = 0;
        while blocked.len() < frame_count {
            let size = sizes[block_index % sizes.len()].min(frame_count - blocked.len());
            block_index += 1;
            let mut block = vec![[0.0_f32; 2]; size];
            let count = block_runtime
                .process_block(&mut block, ProcessingMode::Offline)
                .expect("block");
            blocked.extend_from_slice(&block[..count]);
        }
        assert_eq!(blocked, scalar);
    }

    #[test]
    fn allocation_changing_parameter_base_restarts_the_affected_track() {
        let source = include_str!("../../tests/fixtures/minimal.crump")
            .replace(
                "      parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz",
                "      parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz\n      parameter delay-time \"Delay Time\" seconds 10ms 1s 250ms 250ms",
            )
            .replace(
                "        node output output",
                "        node echo delay time $delay-time feedback 0.2 mix 0.2\n        node output output",
            )
            .replace(
                "        connect position.audio -> output.audio",
                "        connect position.audio -> echo.audio\n        connect echo.audio -> output.audio",
            );
        let project = fixture(&source);
        let original = CompiledProject::compile(&project).expect("compile delay graph");
        let mut changed = project;
        changed.song.instruments[0]
            .parameters
            .iter_mut()
            .find(|parameter| parameter.key == "delay-time")
            .expect("delay time Parameter")
            .value = 0.5;
        let changed = CompiledProject::compile(&changed).expect("compile changed delay graph");
        let mut current = original.runtime();
        let mut next = changed.runtime();
        current.prepare_transition(&mut next);
        assert!(
            !current
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .expect("lead Track")
                .transition_unchanged
        );
    }

    #[test]
    fn scalar_control_edits_preserve_state_but_topology_edits_do_not() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let original = CompiledProject::compile(&project).expect("compile original");

        let mut renamed = project.clone();
        renamed.song.instruments[0].name = "Renamed for display".to_owned();
        renamed.song.instruments[0].faceplate[0].explanation = "Different help text".to_owned();
        let display_only = CompiledProject::compile(&renamed).expect("compile display edit");
        let mut current = original.runtime();
        let mut next = display_only.runtime();
        current.prepare_transition(&mut next);
        assert!(
            current
                .tracks
                .iter()
                .all(|track| track.transition_unchanged)
        );

        let mut changed = project;
        changed.song.instruments[0].parameters[0].value = 4_000.0;
        let changed = CompiledProject::compile(&changed).expect("compile Parameter edit");
        let tone = original
            .controls
            .index(&crate::model::ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "tone".to_owned(),
            })
            .expect("lead tone control");
        let mut current = original.runtime();
        let mut next = changed.runtime();
        current.prepare_transition(&mut next);
        assert!(
            current
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "drums")
                .expect("drums Track")
                .transition_unchanged
        );
        assert!(
            current
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .expect("lead Track")
                .transition_unchanged
        );
        assert!(current.scalar_control_handoff);
        assert_eq!(current.controls.values()[tone.0], 3_200.0);
        current
            .process_transition_frame(&mut next, 1.0, 0.0)
            .expect("first scalar handoff frame");
        assert!(current.controls.values()[tone.0] > 3_200.0);
        assert!(current.controls.values()[tone.0] < 4_000.0);
        for _ in 1..480 {
            current
                .process_transition_frame(&mut next, 1.0, 0.0)
                .expect("scalar handoff frame");
        }
        assert_eq!(current.controls.values()[tone.0], 4_000.0);
        current.complete_transition(&mut next);
        assert_eq!(next.controls.values()[tone.0], 4_000.0);
        assert!(
            next.tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .expect("lead Track after handoff")
                .active_voice_count()
                > 0,
            "the active lead voice must survive scalar publication"
        );

        let mut topology_changed = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        topology_changed.song.instruments[0]
            .graph
            .as_mut()
            .expect("lead graph")
            .nodes[0]
            .settings
            .insert(
                "waveform".to_owned(),
                crate::model::SettingValue::Text("square".to_owned()),
            );
        let topology_changed =
            CompiledProject::compile(&topology_changed).expect("compile topology edit");
        let mut current = original.runtime();
        let mut next = topology_changed.runtime();
        current.prepare_transition(&mut next);
        assert!(
            !current
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .expect("lead Track")
                .transition_unchanged
        );
    }

    #[test]
    fn envelope_follower_has_one_sample_latency_provenance_and_stable_storage() {
        let source = include_str!("../../tests/fixtures/minimal.crump")
            .replace(
                "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz",
                "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz modulatable",
            )
            .replace(
                "    track lead \"Lead\"",
                "    follower lead_follow from track/lead tap post detector peak attack 1ms release 20ms floor -60db ceiling -6db target instrument/lead/tone depth 100hz {\n      clamp 0 1\n    }\n    track lead \"Lead\"",
            );
        let project = fixture(&source);
        let compiled = CompiledProject::compile(&project).expect("compile follower fixture");
        let inspection = compiled.envelope_follower_inspection();
        assert_eq!(inspection.len(), 1);
        assert_eq!(inspection[0].id, "lead_follow");
        assert_eq!(inspection[0].source, "track/lead");
        assert_eq!(inspection[0].control_latency_frames, 1);
        assert_eq!(inspection[0].unit, crate::model::Unit::Hertz);
        assert_eq!(inspection[0].depth, 100.0);

        let target = compiled
            .controls
            .index(&crate::model::ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "tone".to_owned(),
            })
            .expect("follower target");
        let mut runtime = compiled.runtime();
        let fingerprint = runtime.envelope_followers.storage_fingerprint();
        let mut observed = None;
        for _ in 0..2_000 {
            let value_before = runtime.controls.value(target);
            runtime
                .process_frame(ProcessingMode::Offline)
                .expect("follower source frame");
            let telemetry = runtime.telemetry();
            assert_eq!(telemetry.follower_count, 1);
            let contribution = f64::from(telemetry.follower_activity[0].contribution);
            if contribution > 0.0 {
                assert_eq!(value_before, 3_200.0);
                assert_eq!(runtime.controls.value(target), value_before);
                observed = Some(contribution);
                break;
            }
        }
        let contribution = observed.expect("audible source must drive follower");
        runtime
            .process_frame(ProcessingMode::Offline)
            .expect("follower contribution frame");
        assert!(
            (runtime.controls.value(target) - (3_200.0 + contribution)).abs() < 1.0e-4,
            "Audio from frame N must contribute exactly at N+1"
        );
        let frame_zero_value = 3_200.0;
        assert_eq!(
            runtime.envelope_followers.storage_fingerprint(),
            fingerprint
        );

        runtime.seek_transport(0);
        runtime
            .process_frame(ProcessingMode::Offline)
            .expect("reset follower source frame");
        assert_eq!(runtime.controls.value(target), frame_zero_value);
    }
}
