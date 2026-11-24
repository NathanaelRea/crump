//! Deterministic audio compilation and runtime shared by playback and rendering.

mod analysis;
pub mod audition;
pub mod bounce;
pub mod control;
mod drums;
mod fft;
mod follower;
mod freeze;
pub(crate) mod generated_audio;
mod graph;
mod lfo;
pub mod mix_graph;
mod oversampling;
mod processors;
pub(crate) use processors::{ProcessorAssets, ir_frames};
pub(crate) mod qualification;
mod render;
pub mod sample;
pub mod sample_transform;
mod sampler;
pub(crate) mod schedule;
pub(crate) mod spectral;
mod wavetable;

pub use sample::SampleSourceInspection;
pub use spectral::SpectralAnalysisInspection;
pub use wavetable::WavetableInspection;

use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::Instrument;
use crate::{
    diagnostic::Diagnostic,
    media::AssetInventory,
    model::{
        DrumVoice, GlideMode, InstrumentKind, NotePriority, Project, RetriggerPolicy,
        StealPriority, TapPoint, Voicing, VoicingMode,
    },
    validate::{MAX_CONTROL_TARGETS, validate_project_with_topology},
};

use self::{
    control::{
        CompiledControlProgram, ControlIndex, ControlRuntime, ControlValueSnapshot,
        TrackParameterLockRuntime, mix_modulation_identity, stable_modulation_identity,
    },
    drums::DrumSynth,
    follower::{
        CompiledEnvelopeFollowerProgram, EnvelopeFollowerInspection, EnvelopeFollowerMeter,
        EnvelopeFollowerRuntime, MAX_ENVELOPE_FOLLOWERS,
    },
    graph::{CompiledGraph, GraphProcessIdentity, GraphVoice, SharedGraphRuntime},
    mix_graph::{
        ChannelMeter, CompiledMixGraph, MAX_MIX_CHANNELS, MAX_PROCESSOR_INSTANCES, MixGraphRuntime,
        ProcessorMeter,
    },
    sample::{CompiledSampleLibrary, CompiledSampleSource},
    sampler::{CompiledSamplerProgram, SamplerRuntime},
    schedule::{ClipEnvelope, MAX_RUNTIME_TRACKS, RuntimeEvent, TrackSchedule, compile_schedules},
    spectral::CompiledSpectralLibrary,
};

pub use analysis::FinalizedAudioAnalysis;
pub(crate) use generated_audio::GeneratedAudioArtifact;
pub use mix_graph::AdmissionProfile;
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
    samples: Vec<SampleSourceInspection>,
    spectral: Vec<SpectralAnalysisInspection>,
    wavetables: Vec<WavetableInspection>,
}

impl CompiledProject {
    /// Validates and compiles a media-free Project for live playback.
    ///
    /// Projects containing sampler mappings must use [`Self::compile_with_assets`]
    /// so exact verified media bytes are available during publication.
    pub fn compile(project: &Project) -> Result<Self, EngineError> {
        Self::compile_with_assets(project, &AssetInventory::default())
    }

    /// Validates and compiles one Project plus its verified exact-byte media for live playback.
    pub fn compile_with_assets(
        project: &Project,
        inventory: &AssetInventory,
    ) -> Result<Self, EngineError> {
        Self::compile_for(project, inventory, AdmissionProfile::Live)
    }

    /// Validates and compiles one Project for the runtime named by `profile`.
    ///
    /// Offline render and bounce use [`AdmissionProfile::Offline`], which has no per-frame work
    /// budget; live playback uses [`AdmissionProfile::Live`].
    pub fn compile_for(
        project: &Project,
        inventory: &AssetInventory,
        profile: AdmissionProfile,
    ) -> Result<Self, EngineError> {
        Self::compile_with_cache(project, inventory, profile, None)
    }

    fn compile_with_cache(
        project: &Project,
        inventory: &AssetInventory,
        profile: AdmissionProfile,
        cache_directory: Option<&std::path::Path>,
    ) -> Result<Self, EngineError> {
        let (diagnostics, topology) = validate_project_with_topology(project, Some(inventory));
        if diagnostics.has_errors() {
            return Err(EngineError::Validation(diagnostics.into_vec()));
        }
        if profile == AdmissionProfile::Live
            && let Some(diagnostic) = diagnostics
                .as_slice()
                .iter()
                .find(|diagnostic| diagnostic.code == "CRV093")
        {
            return Err(EngineError::Compile(diagnostic.message.clone()));
        }
        let mut frozen = if profile == AdmissionProfile::Live {
            freeze::prepare(project, inventory, &topology, cache_directory)?
        } else {
            BTreeMap::new()
        };
        let schedules = compile_schedules(project)?;
        let controls = CompiledControlProgram::compile(project)?;
        let sample_library =
            CompiledSampleLibrary::compile_for_project(project, inventory, project.sample_rate)
                .map_err(|error| EngineError::Compile(error.to_string()))?;
        let sample_inspection = sample_library.inspection();
        let spectral_library =
            CompiledSpectralLibrary::compile_for_project(project, &sample_library)
                .map_err(|error| EngineError::Compile(error.to_string()))?;
        let spectral_inspection = spectral_library.inspection();
        let wavetable_library =
            wavetable::CompiledWavetableLibrary::compile(project, inventory, project.sample_rate)
                .map_err(|error| EngineError::Compile(error.to_string()))?;
        let wavetable_inspection = wavetable_library.inspection();
        let instruments: BTreeMap<_, _> = project
            .song
            .instruments
            .iter()
            .map(|instrument| (instrument.id.as_str(), instrument))
            .collect();
        let mut tracks = Vec::with_capacity(schedules.len());
        for mut schedule in schedules {
            let instrument = instruments
                .get(schedule.instrument_id.as_str())
                .ok_or_else(|| {
                    EngineError::Compile(format!("missing instrument `{}`", schedule.instrument_id))
                })?;
            let mut semantic_hash = track_semantic_hash(&schedule, instrument, project)?;
            let program = if let Some((key, frames)) = frozen.remove(&schedule.track_id) {
                semantic_hash = key;
                schedule.events = Vec::new();
                InstrumentProgram::Frozen(frames)
            } else {
                match instrument.kind {
                    InstrumentKind::Graph => InstrumentProgram::Graph {
                        // Voice and shared programs bind the same publication-owned
                        // exact-asset derivative library before any callback exists.
                        graph: Arc::new(CompiledGraph::compile_with_assets(
                            instrument,
                            project,
                            project.sample_rate,
                            &controls,
                            &sample_library,
                            &spectral_library,
                            &wavetable_library,
                        )?),
                        shared_graph: CompiledGraph::compile_shared_with_assets(
                            instrument,
                            project,
                            project.sample_rate,
                            &controls,
                            &sample_library,
                            &spectral_library,
                            &wavetable_library,
                        )?
                        .map(Arc::new),
                        polyphony: usize::from(instrument.polyphony),
                        voicing: CompiledVoicing::from_authored(&instrument.voicing),
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
                }
            };
            tracks.push(CompiledTrack {
                schedule: Arc::new(schedule),
                program,
                semantic_hash,
            });
        }
        let mut mix_graph = CompiledMixGraph::compile_with_topology(
            project,
            &controls,
            &topology,
            Some(profile),
            processors::ProcessorAssets::Samples(&sample_library),
        )?;
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
                InstrumentProgram::Graph {
                    graph,
                    shared_graph,
                    ..
                } => graph.allocation_bytes().saturating_add(
                    shared_graph
                        .as_ref()
                        .map_or(0, |graph| graph.allocation_bytes()),
                ),
                InstrumentProgram::Drums { .. } | InstrumentProgram::Frozen(_) => 0,
                InstrumentProgram::Sampler(program) => program.allocation_bytes(),
            })
            .fold(
                sample_library
                    .allocation_bytes()
                    .saturating_add(spectral_library.allocation_bytes()),
                u64::saturating_add,
            );
        let voicing_runtime_state_bytes = tracks
            .iter()
            .filter(|track| matches!(track.program, InstrumentProgram::Graph { .. }))
            .count() as u64
            * std::mem::size_of::<GraphVoicingRuntime>() as u64;
        let song_runtime_structure_bytes = (std::mem::size_of::<SongRuntime>() as u64)
            .saturating_add(project.id.len() as u64)
            .saturating_add(voicing_runtime_state_bytes)
            .saturating_add(
                (tracks.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<TrackRuntime>() as u64),
            )
            .saturating_add(controls.runtime_state_bytes())
            .saturating_add(controls.track_parameter_lock_runtime_state_bytes())
            .saturating_add(envelope_followers.runtime_state_bytes());
        mix_graph.include_song_runtime_state(
            controls
                .allocation_bytes()
                .saturating_add(envelope_followers.allocation_bytes()),
            schedule_program_bytes,
            instrument_program_bytes,
            song_runtime_structure_bytes,
        )?;
        let voicing_work_units_per_frame = tracks
            .iter()
            .map(|track| match &track.program {
                InstrumentProgram::Graph {
                    polyphony, voicing, ..
                } if voicing.glide_mode != GlideMode::Off => *polyphony as u64,
                _ => 0,
            })
            .fold(0_u64, u64::saturating_add);
        mix_graph.include_additional_callback_work(
            controls
                .instrument_work_units_per_frame()
                .saturating_add(envelope_followers.work_units_per_frame())
                .saturating_add(voicing_work_units_per_frame),
        )?;
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
            samples: sample_inspection,
            spectral: spectral_inspection,
            wavetables: wavetable_inspection,
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
            audition_seek_requested: false,
            controls: self.controls.runtime(),
            envelope_followers: self.envelope_followers.runtime(),
            scalar_control_handoff: false,
            publication_pristine: true,
            tracks: self
                .tracks
                .iter()
                .enumerate()
                .map(|(index, track)| {
                    TrackRuntime::new(
                        track,
                        self.sample_rate,
                        index as u64,
                        if matches!(track.program, InstrumentProgram::Frozen(_)) {
                            None
                        } else {
                            self.controls
                                .track_parameter_lock_runtime(&track.schedule.track_id)
                        },
                    )
                })
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

    /// Compiled control program aligned with this exact Project compilation.
    #[must_use]
    pub(crate) const fn control_program(&self) -> &CompiledControlProgram {
        &self.controls
    }

    /// Inspectable processing paths, latency, and resource admission facts.
    #[must_use]
    pub fn mix_graph_inspection(&self) -> &mix_graph::MixGraphInspection {
        self.mix_graph.inspection()
    }

    /// Derived exact-asset sample decode/resampling identities and shapes.
    #[must_use]
    pub fn sample_source_inspection(&self) -> &[SampleSourceInspection] {
        &self.samples
    }

    /// Derived exact-asset spectral analysis identities and bounded shapes.
    #[must_use]
    pub fn spectral_analysis_inspection(&self) -> &[SpectralAnalysisInspection] {
        &self.spectral
    }

    /// Derived exact-asset table identities, shapes, and mip bounds.
    #[must_use]
    pub fn wavetable_inspection(&self) -> &[WavetableInspection] {
        &self.wavetables
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
    semantic_hash: [u8; 32],
}

#[derive(Debug, Clone, Copy)]
struct CompiledVoicing {
    mode: VoicingMode,
    retrigger: RetriggerPolicy,
    glide_seconds: f64,
    glide_mode: GlideMode,
    note_priority: NotePriority,
    steal_priority: StealPriority,
    seed: u64,
}

impl CompiledVoicing {
    fn from_authored(voicing: &Voicing) -> Self {
        Self {
            mode: voicing.mode,
            retrigger: voicing.retrigger,
            glide_seconds: voicing.glide_seconds,
            glide_mode: voicing.glide_mode,
            note_priority: voicing.note_priority,
            steal_priority: voicing.steal_priority,
            seed: voicing.seed,
        }
    }

    fn structurally_compatible(self, replacement: Self) -> bool {
        self.mode == replacement.mode
            && self.retrigger == replacement.retrigger
            && self.glide_mode == replacement.glide_mode
            && self.note_priority == replacement.note_priority
            && self.steal_priority == replacement.steal_priority
            && self.seed == replacement.seed
    }
}

#[derive(Debug, Clone)]
enum InstrumentProgram {
    Frozen(Arc<[[f32; 2]]>),
    Graph {
        graph: Arc<CompiledGraph>,
        shared_graph: Option<Arc<CompiledGraph>>,
        polyphony: usize,
        voicing: CompiledVoicing,
    },
    Drums {
        instrument: Box<crate::model::Instrument>,
        tone: Option<ControlIndex>,
        punch: Option<ControlIndex>,
    },
    Sampler(Arc<CompiledSamplerProgram>),
}

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
    let mut parameters = instrument
        .parameters
        .iter()
        .map(|parameter| {
            let allocation_value = instrument
                .graph
                .iter()
                .chain(instrument.shared_graph.iter())
                .any(|graph| {
                    graph.nodes.iter().any(|node| {
                        node.kind == crate::model::NodeKind::Delay
                            && matches!(
                                node.setting("time"),
                                Some(crate::model::SettingValue::Parameter(bound))
                                    if bound == &parameter.key
                            )
                    })
                })
                .then_some(parameter.value);
            (&parameter.key, allocation_value)
        })
        .collect::<Vec<_>>();
    parameters.sort_by_key(|(left, _)| *left);
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
    let graph_sample_asset_ids = instrument
        .graph
        .iter()
        .chain(instrument.shared_graph.iter())
        .flat_map(|graph| &graph.nodes)
        .flat_map(|node| match node.kind {
            crate::model::NodeKind::SampleSource
            | crate::model::NodeKind::Granular
            | crate::model::NodeKind::Spectral => node
                .setting("asset")
                .and_then(|value| match value {
                    crate::model::SettingValue::Text(asset_id) => Some(vec![asset_id.as_str()]),
                    _ => None,
                })
                .unwrap_or_default(),
            crate::model::NodeKind::Multisample => node
                .directives
                .iter()
                .filter_map(|directive| {
                    directive
                        .zone_clauses()
                        .map(|zone| zone.asset.value.as_str())
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect::<std::collections::BTreeSet<_>>();
    let graph_sample_assets = graph_sample_asset_ids
        .into_iter()
        .filter_map(|asset_id| {
            project
                .assets
                .iter()
                .find(|asset| asset.id == asset_id)
                .map(|asset| (&asset.id, &asset.sha256))
        })
        .collect::<Vec<_>>();
    let wavetable_asset_ids = instrument
        .graph
        .iter()
        .chain(instrument.shared_graph.iter())
        .flat_map(|graph| &graph.nodes)
        .filter(|node| node.kind == crate::model::NodeKind::Wavetable)
        .filter_map(|node| match node.setting("asset") {
            Some(crate::model::SettingValue::Text(asset_id)) => Some(asset_id.as_str()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let wavetable_assets = wavetable_asset_ids
        .into_iter()
        .filter_map(|asset_id| {
            project
                .assets
                .iter()
                .find(|asset| asset.id == asset_id)
                .map(|asset| (&asset.id, &asset.sha256, &asset.wavetable))
        })
        .collect::<Vec<_>>();
    let canonical_graph = instrument
        .graph
        .as_ref()
        .map(canonical_graph_for_semantic_hash);
    let canonical_shared_graph = instrument
        .shared_graph
        .as_ref()
        .map(canonical_graph_for_semantic_hash);
    let mut modulation_sources = instrument.modulation_sources.iter().collect::<Vec<_>>();
    modulation_sources.sort_by(|left, right| left.id.cmp(&right.id));
    let mut modulation_routes = instrument.modulation_routes.iter().collect::<Vec<_>>();
    modulation_routes.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.id.cmp(&right.id))
    });
    let modulation_route_topology = modulation_routes
        .into_iter()
        .map(|route| {
            serde_json::json!({
                "id": route.id,
                "source": route.source,
                "target": route.target,
                "polarity": route.polarity,
                "curve": route.curve,
                "order": route.order,
                "auxiliary": route.auxiliary,
                "operators": route.operators,
            })
        })
        .collect::<Vec<_>>();
    let mut song_modulation_topology = project
        .song
        .modulations
        .iter()
        .filter(|route| {
            matches!(
                &route.target,
                crate::model::ParameterTarget::Instrument { instrument: owner, .. }
                    if owner == &instrument.id
            )
        })
        .collect::<Vec<_>>();
    song_modulation_topology.sort_by(|left, right| left.id.cmp(&right.id));
    let mut follower_topology = project
        .song
        .envelope_followers
        .iter()
        .filter(|route| {
            matches!(
                &route.target,
                crate::model::ParameterTarget::Instrument { instrument: owner, .. }
                    if owner == &instrument.id
            )
        })
        .collect::<Vec<_>>();
    follower_topology.sort_by(|left, right| left.id.cmp(&right.id));
    let mut audio_program = serde_json::json!({
        "kind": instrument.kind,
        "polyphony": instrument.polyphony,
        "voicing": {
            "mode": instrument.voicing.mode,
            "retrigger": instrument.voicing.retrigger,
            "glide_mode": instrument.voicing.glide_mode,
            "note_priority": instrument.voicing.note_priority,
            "steal_priority": instrument.voicing.steal_priority,
            "seed": instrument.voicing.seed,
        },
        "parameters": parameters,
        "graph": canonical_graph,
        "shared_graph": canonical_shared_graph,
        "modulation_sources": modulation_sources,
        "modulation_route_topology": modulation_route_topology,
        "song_modulation_topology": song_modulation_topology,
        "follower_topology": follower_topology,
        "sampler": instrument.sampler,
        "sample_assets": sample_assets,
        "graph_sample_assets": graph_sample_assets,
        "wavetable_assets": wavetable_assets,
    });
    strip_source_metadata(&mut audio_program);
    hash_bytes(&mut hasher, &serde_json::to_vec(&audio_program)?);
    Ok(hasher.finalize().into())
}

fn canonical_graph_for_semantic_hash(graph: &crate::model::Graph) -> crate::model::Graph {
    let mut graph = graph.clone();
    let summing_nodes = graph
        .nodes
        .iter()
        .filter(|node| node.kind.is_summing())
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    graph.nodes.sort_by(|left, right| left.id.cmp(&right.id));
    for node in &mut graph.nodes {
        node.settings
            .sort_by(|left, right| left.key.cmp(&right.key));
    }
    for connection in &mut graph.connections {
        if connection.id_site.is_none() && !summing_nodes.contains(&connection.to.node) {
            connection.id = format!(
                "@canonical-connect:{}:{}:{}:{}",
                connection.from.node, connection.from.port, connection.to.node, connection.to.port
            );
        }
    }
    graph.connections.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.from.cmp(&right.from))
            .then_with(|| left.to.cmp(&right.to))
    });
    graph
}

fn hash_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn hash_clip_envelope(hasher: &mut Sha256, envelope: ClipEnvelope) {
    hasher.update(envelope.gain.to_bits().to_le_bytes());
    hasher.update(envelope.position_frames.to_le_bytes());
    hasher.update(envelope.duration_frames.to_le_bytes());
    hasher.update(envelope.fade_in_frames.to_le_bytes());
    hasher.update(envelope.fade_out_frames.to_le_bytes());
}

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

/// Voices sounding after one source step, split by publication compatibility.
#[derive(Debug, Default, Clone, Copy)]
struct TrackVoices {
    unchanged: usize,
    affected: usize,
}

impl TrackVoices {
    fn total(self) -> usize {
        self.unchanged.saturating_add(self.affected)
    }
}

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
    audition_seek_requested: bool,
    controls: ControlRuntime,
    envelope_followers: EnvelopeFollowerRuntime,
    scalar_control_handoff: bool,
    publication_pristine: bool,
    tracks: Vec<TrackRuntime>,
    mix: MixGraphRuntime,
    track_sources: [[f32; 2]; MAX_RUNTIME_TRACKS],
    telemetry: RuntimeTelemetry,
}

impl SongRuntime {
    /// Processes one stereo frame.
    pub fn process_frame(&mut self, mode: ProcessingMode) -> Result<[f32; 2], EngineError> {
        if !self.begin_frame() {
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
        let voices = self.process_track_sources(false);
        let routed = self.mix.process(
            &self.track_sources[..self.tracks.len()],
            self.controls.values(),
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
        self.record_output(self.frame, master, voices.total());
        self.frame += 1;
        Ok(master)
    }

    /// Starts one transport frame for playback and publication alike.
    ///
    /// Wraps an expired audition loop and reports whether authored playback is
    /// still running at the resulting frame.
    fn begin_frame(&mut self) -> bool {
        {
            self.publication_pristine = false;
        }
        if let Some((start, end)) = self.audition_loop
            && self.frame >= end
        {
            self.rewind_audition(start);
        }
        self.frame < self.playback_end()
    }

    /// Evaluates controls and runs Track sources for the current frame.
    ///
    /// This is the one per-frame source step shared by playback and publication.
    /// Every processed Track writes its `track_sources` entry and drains its
    /// feedback-guard mutes into telemetry; a skipped Track contributes silence.
    /// `skip_unchanged` omits publication-unchanged Tracks whose voices the
    /// outgoing runtime already runs.
    fn process_track_sources(&mut self, skip_unchanged: bool) -> TrackVoices {
        self.controls.evaluate_frame(self.frame);
        let mut voices = TrackVoices::default();
        for (index, track) in self.tracks.iter_mut().enumerate() {
            if skip_unchanged && track.transition_unchanged {
                self.track_sources[index] = [0.0, 0.0];
                continue;
            }
            track.dispatch(self.frame);
            let (sample, count) = track.process(&self.controls, self.frame);
            self.track_sources[index] = sample;
            if track.transition_unchanged {
                voices.unchanged = voices.unchanged.saturating_add(count);
            } else {
                voices.affected = voices.affected.saturating_add(count);
            }
            self.telemetry.feedback_mutes = self
                .telemetry
                .feedback_mutes
                .saturating_add(track.take_feedback_mutes());
        }
        voices
    }

    /// Records one emitted frame's output, voices, and mix meters.
    fn record_output(&mut self, frame: u64, output: [f32; 2], active_voices: usize) {
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
        {
            self.publication_pristine = true;
        }
    }

    /// Clears a replacement on the controller thread before queue ownership moves.
    pub(crate) fn prime_for_publication(&mut self) {
        if !self.audition_seek_requested {
            self.seek_transport(self.frame);
        }
    }

    /// Realigns an already-cleared replacement with bounded cursor-only callback work.
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
    pub(crate) fn prepare_transition(&mut self, next: &mut Self) {
        self.scalar_control_handoff = false;
        self.controls.reset_transition_changes();
        next.controls.reset_transition_changes();
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
                    let new_length = new_end.saturating_sub(new_start).max(1);
                    let old_offset = audition::range_offset(
                        self.frame,
                        old_start,
                        old_end,
                        self.audition_loop.is_some(),
                    );
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
        let frames =
            u64::from(self.sample_rate).saturating_mul(PUBLICATION_TRANSITION_MILLISECONDS) / 1_000;
        if wholly_compatible {
            self.scalar_control_handoff = self
                .controls
                .begin_publication_handoff(&mut next.controls, frames);
        }
        if !self.scalar_control_handoff {
            for current in &mut self.tracks {
                if current.transition_unchanged
                    && self.controls.track_parameter_locks_differ(
                        &next.controls,
                        current.schedule.track_id.as_str(),
                    )
                    && let Some(replacement) = next
                        .tracks
                        .iter_mut()
                        .find(|replacement| current.same_semantics(replacement))
                {
                    current.transition_unchanged = false;
                    replacement.transition_unchanged = false;
                }
            }
        }
        let upstream_changed = self.controls.transition_changed();
        let control_values = self.controls.values();
        let pre_instrument_controls = self.controls.pre_instrument_unconstrained_values();
        for current in &mut self.tracks {
            if !current.transition_unchanged {
                continue;
            }
            if let Some(replacement) = next
                .tracks
                .iter_mut()
                .find(|replacement| current.same_semantics(replacement))
            {
                if self.scalar_control_handoff {
                    current.begin_parameter_lock_handoff(
                        replacement,
                        control_values,
                        pre_instrument_controls,
                        frames,
                    );
                }
                current.instrument.begin_compatible_handoff(
                    &replacement.instrument,
                    upstream_changed,
                    frames,
                );
            }
        }
    }

    /// Processes one publication frame without running shared Track voices twice.
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
            let old_output = self.monitor_output(old_routed);
            let new_output = next.monitor_output(new_routed);
            [
                old_output[0] * old_gain + new_output[0] * new_gain,
                old_output[1] * old_gain + new_output[1] * new_gain,
            ]
        };
        let frame = old.frame;
        if !master.into_iter().all(f32::is_finite) {
            self.record_output(frame, [0.0, 0.0], 0);
            next.record_output(new.frame, [0.0, 0.0], 0);
            return Err(EngineError::NonFinite { frame });
        }
        // As in `process_frame`, followers observe only a finite mix. A shared mix
        // feeds both runtimes' followers because only the outgoing mix ran.
        self.telemetry.follower_count = self.envelope_followers.process(
            &self.mix,
            &mut self.controls,
            &mut self.telemetry.follower_activity,
        );
        let next_mix = if shared_mix { &self.mix } else { &next.mix };
        next.telemetry.follower_count = next.envelope_followers.process(
            next_mix,
            &mut next.controls,
            &mut next.telemetry.follower_activity,
        );
        for channel in &mut master {
            *channel = monitor_sample(*channel);
        }
        let active_voices = old
            .common_voices
            .saturating_add(old.affected_voices)
            .saturating_add(new.affected_voices);
        self.record_output(frame, master, active_voices);
        next.record_output(new.frame, master, active_voices);
        if shared_mix {
            next.telemetry.channel_activity = self.telemetry.channel_activity;
            next.telemetry.channel_count = self.telemetry.channel_count;
            next.telemetry.processor_activity = self.telemetry.processor_activity;
            next.telemetry.processor_count = self.telemetry.processor_count;
        }
        Ok(master)
    }

    /// Moves live state for unchanged Tracks into the accepted replacement runtime.
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

    fn process_transition_sources(&mut self, process_unchanged: bool) -> TransitionFrame {
        let running = self.begin_frame();
        let mut output = TransitionFrame {
            frame: self.frame,
            ..TransitionFrame::default()
        };
        if !running {
            return output;
        }
        let voices = self.process_track_sources(!process_unchanged);
        for (index, track) in self.tracks.iter().enumerate() {
            if track.transition_unchanged {
                output.common_sources[index] = self.track_sources[index];
            } else {
                output.affected_sources[index] = self.track_sources[index];
            }
        }
        output.common_voices = voices.unchanged;
        output.affected_voices = voices.affected;
        self.frame = self.frame.saturating_add(1);
        output
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

    /// Moves an audition runtime to `target_frame` inside its range by a cleared seek, keeping
    /// a raw source cursor aligned with the range offset.
    pub(crate) fn locate_audition(&mut self, target_frame: u64) {
        let target = target_frame.min(self.playback_end());
        self.seek_transport(target);
        if let (Some((start, _)), Some(raw)) = (self.audition_range, &mut self.raw_audition) {
            raw.realign(self.frame.saturating_sub(start));
        }
    }

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
        let Some(track) = self.tracks.iter_mut().find(|track| {
            track.schedule.instrument_id == instrument_id
                && !matches!(track.instrument, InstrumentRuntime::Frozen(_))
        }) else {
            return false;
        };
        let seed = note_id ^ (u64::from(pitch) << 32) ^ 0x6a09_e667_f3bc_c909;
        track.instrument.dispatch(
            RuntimeEvent::NoteOn {
                note_id,
                pitch,
                velocity,
                seed,
                clip_envelope: ClipEnvelope::TRANSPARENT,
            },
            self.frame,
        );
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
        if matches!(track.instrument, InstrumentRuntime::Frozen(_)) {
            return false;
        }
        let seed = note_id ^ (u64::from(pitch) << 32) ^ 0xbb67_ae85_84ca_a73b;
        track.instrument.dispatch(
            RuntimeEvent::NoteOn {
                note_id,
                pitch,
                velocity,
                seed,
                clip_envelope: ClipEnvelope::TRANSPARENT,
            },
            self.frame,
        );
        true
    }

    /// Releases one transient audition note on one zero-based Timeline Track.
    pub fn audition_note_off_track(&mut self, track_index: usize, note_id: u64) -> bool {
        let Some(track) = self.tracks.get_mut(track_index) else {
            return false;
        };
        track
            .instrument
            .dispatch(RuntimeEvent::NoteOff { note_id }, self.frame);
        true
    }

    /// Releases one transient audition note on the selected Instrument.
    pub fn audition_note_off(&mut self, instrument_id: &str, note_id: u64) -> bool {
        let Some(track) = self.tracks.iter_mut().find(|track| {
            track.schedule.instrument_id == instrument_id
                && !matches!(track.instrument, InstrumentRuntime::Frozen(_))
        }) else {
            return false;
        };
        track
            .instrument
            .dispatch(RuntimeEvent::NoteOff { note_id }, self.frame);
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
    semantic_hash: [u8; 32],
    cursor: usize,
    loop_cursor: usize,
    transition_unchanged: bool,
    modulation_identity: u64,
    parameter_locks: Option<TrackParameterLockRuntime>,
    parameter_locks_handoff: bool,
    instrument: InstrumentRuntime,
    /// Feedback-guard mutes not yet folded into Song telemetry.
    feedback_mutes: u64,
}

impl TrackRuntime {
    fn new(
        track: &CompiledTrack,
        sample_rate: u32,
        seed: u64,
        parameter_locks: Option<TrackParameterLockRuntime>,
    ) -> Self {
        let modulation_identity = stable_modulation_identity(&track.schedule.track_id);
        let instrument = match &track.program {
            InstrumentProgram::Graph {
                graph,
                shared_graph,
                polyphony,
                voicing,
            } => InstrumentRuntime::Graph {
                graph: graph.clone(),
                voices: (0..*polyphony)
                    .map(|slot| GraphVoiceSlot::new(graph, seed ^ slot as u64))
                    .collect(),
                shared: shared_graph.as_ref().map(|graph| SharedGraphInstance {
                    graph: graph.clone(),
                    runtime: graph
                        .create_shared_runtime(modulation_identity ^ 0x510e_527f_ade6_82d1),
                }),
                voicing: Box::new(GraphVoicingRuntime::new(*voicing, sample_rate)),
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
            InstrumentProgram::Frozen(frames) => InstrumentRuntime::Frozen(frames.clone()),
        };
        Self {
            schedule: track.schedule.clone(),
            semantic_hash: track.semantic_hash,
            cursor: 0,
            loop_cursor: 0,
            transition_unchanged: false,
            modulation_identity,
            parameter_locks,
            parameter_locks_handoff: false,
            instrument,
            feedback_mutes: 0,
        }
    }

    /// Returns and clears the feedback-guard mutes recorded since the previous call.
    fn take_feedback_mutes(&mut self) -> u64 {
        std::mem::take(&mut self.feedback_mutes)
    }

    fn same_semantics(&self, other: &Self) -> bool {
        self.schedule.track_id == other.schedule.track_id
            && self.semantic_hash == other.semantic_hash
            && self.instrument.transition_compatible(&other.instrument)
    }

    fn begin_parameter_lock_handoff(
        &mut self,
        replacement: &mut Self,
        fallback_effective: &[f64],
        fallback_pre_instrument: &[f64],
        frames: u64,
    ) {
        let began = match (&mut self.parameter_locks, &mut replacement.parameter_locks) {
            (Some(current), Some(replacement)) => current.begin_handoff(
                Some(replacement.lanes_handle()),
                fallback_effective,
                fallback_pre_instrument,
                false,
                frames,
            ),
            (None, Some(_)) => {
                std::mem::swap(&mut self.parameter_locks, &mut replacement.parameter_locks);
                let current = self
                    .parameter_locks
                    .as_mut()
                    .expect("replacement lock runtime was moved into current Track");
                current.begin_handoff(
                    Some(current.lanes_handle()),
                    fallback_effective,
                    fallback_pre_instrument,
                    true,
                    frames,
                )
            }
            (Some(current), None) => current.begin_handoff(
                None,
                fallback_effective,
                fallback_pre_instrument,
                false,
                frames,
            ),
            (None, None) => false,
        };
        self.parameter_locks_handoff = began;
    }

    fn adopt_state_from(&mut self, current: &mut Self) {
        self.cursor = current.cursor;
        self.loop_cursor = current.loop_cursor;
        if current.parameter_locks_handoff {
            let keep_runtime = current
                .parameter_locks
                .as_mut()
                .is_some_and(crate::engine::control::TrackParameterLockRuntime::finish_handoff);
            if keep_runtime {
                if let (Some(replacement), Some(current)) =
                    (&mut self.parameter_locks, &mut current.parameter_locks)
                {
                    replacement.adopt_values_from(current);
                } else {
                    self.parameter_locks = current.parameter_locks.take();
                }
            } else {
                self.parameter_locks = None;
            }
            current.parameter_locks_handoff = false;
        }
        self.instrument.adopt_state_from(&mut current.instrument);
    }

    fn seek_transport(&mut self, frame: u64) {
        self.realign_pristine_transport(frame);
        self.instrument.all_notes_off();
        self.instrument
            .reset_shared_state(self.modulation_identity ^ 0x510e_527f_ade6_82d1);
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
            self.instrument.dispatch(event, frame);
        }
    }

    fn active_voice_count(&self) -> usize {
        self.instrument.active_voice_count()
    }

    fn process(&mut self, control_runtime: &ControlRuntime, song_frame: u64) -> ([f32; 2], usize) {
        let (controls, pre_instrument_unconstrained_controls) =
            if let Some(parameter_locks) = &mut self.parameter_locks {
                parameter_locks.project(song_frame, control_runtime)
            } else {
                (
                    control_runtime.values(),
                    control_runtime.pre_instrument_unconstrained_values(),
                )
            };
        self.instrument.process(
            controls,
            pre_instrument_unconstrained_controls,
            control_runtime.song_modulation_values(),
            control_runtime.instrument_route_values(),
            song_frame,
            self.modulation_identity,
            &mut self.feedback_mutes,
        )
    }
}

const GRAPH_STEAL_CROSSFADE_FRAMES: u32 = 64;

pub(super) const fn graph_voice_slot_overhead_bytes() -> u64 {
    (std::mem::size_of::<GraphVoiceSlot>() - 2 * std::mem::size_of::<GraphVoice>()) as u64
}

struct GraphVoiceSlot {
    voice: GraphVoice,
    replacement_tail: GraphVoice,
    envelope: ClipEnvelope,
    replacement_tail_envelope: ClipEnvelope,
    crossfade_remaining: u32,
}

impl GraphVoiceSlot {
    #[cfg(test)]
    fn allocation_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>() as u64)
            .saturating_add(
                self.voice
                    .allocation_bytes()
                    .saturating_sub(std::mem::size_of::<GraphVoice>() as u64),
            )
            .saturating_add(
                self.replacement_tail
                    .allocation_bytes()
                    .saturating_sub(std::mem::size_of::<GraphVoice>() as u64),
            )
    }

    fn new(graph: &CompiledGraph, seed: u64) -> Self {
        Self {
            voice: graph.create_voice(seed),
            replacement_tail: graph.create_voice(seed ^ 0xa5a5_a5a5_a5a5_a5a5),
            envelope: ClipEnvelope::TRANSPARENT,
            replacement_tail_envelope: ClipEnvelope::TRANSPARENT,
            crossfade_remaining: 0,
        }
    }

    #[cfg(test)]
    fn activate(
        &mut self,
        note_id: u64,
        pitch: u8,
        velocity: f32,
        seed: u64,
        envelope: ClipEnvelope,
    ) {
        self.activate_with_glide(note_id, pitch, velocity, seed, envelope, None, 0.0, 1);
    }

    #[allow(clippy::too_many_arguments)]
    fn activate_with_glide(
        &mut self,
        note_id: u64,
        pitch: u8,
        velocity: f32,
        seed: u64,
        envelope: ClipEnvelope,
        glide_start_hz: Option<f32>,
        glide_seconds: f64,
        sample_rate: u32,
    ) {
        if self.voice.active() {
            self.voice.release_all();
            std::mem::swap(&mut self.voice, &mut self.replacement_tail);
            std::mem::swap(&mut self.envelope, &mut self.replacement_tail_envelope);
            self.crossfade_remaining = GRAPH_STEAL_CROSSFADE_FRAMES;
        }
        self.voice.activate(note_id, pitch, velocity, seed);
        self.voice
            .configure_glide(glide_start_hz, pitch, glide_seconds, sample_rate);
        self.envelope = envelope;
    }

    #[allow(clippy::too_many_arguments)]
    fn legato(
        &mut self,
        note_id: u64,
        pitch: u8,
        velocity: f32,
        seed: u64,
        envelope: ClipEnvelope,
        glide_start_hz: Option<f32>,
        glide_seconds: f64,
        sample_rate: u32,
    ) {
        self.voice.retarget_legato(
            note_id,
            pitch,
            velocity,
            seed,
            glide_start_hz,
            glide_seconds,
            sample_rate,
        );
        self.envelope = envelope;
    }

    const fn gate_active(&self) -> bool {
        self.voice.gate_active()
    }

    const fn pitch_hz(&self) -> f32 {
        self.voice.pitch_hz()
    }

    fn release(&mut self, note_id: u64) {
        self.voice.release(note_id);
        self.replacement_tail.release(note_id);
    }

    fn release_all(&mut self) {
        self.voice.release_all();
        self.replacement_tail.release_all();
    }

    fn begin_route_transition(&mut self, changed: &[bool], frames: u64) {
        self.voice.begin_route_transition(changed, frames);
        self.replacement_tail
            .begin_route_transition(changed, frames);
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

    fn take_feedback_mutes(&mut self) -> u64 {
        self.voice
            .take_feedback_mutes()
            .saturating_add(self.replacement_tail.take_feedback_mutes())
    }

    fn process(
        &mut self,
        graph: &CompiledGraph,
        controls: &[f64],
        pre_instrument_unconstrained_controls: &[f64],
        song_modulation: &[f64],
        instrument_route_values: &[control::RuntimeInstrumentRouteValue],
        identity: GraphProcessIdentity,
    ) -> [f32; 2] {
        let current = if self.voice.active() {
            let gain = self.envelope.gain_at(self.voice.age_frames());
            let sample = graph.process(
                &mut self.voice,
                controls,
                pre_instrument_unconstrained_controls,
                song_modulation,
                instrument_route_values,
                identity,
            );
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
            let sample = graph.process(
                &mut self.replacement_tail,
                controls,
                pre_instrument_unconstrained_controls,
                song_modulation,
                instrument_route_values,
                identity,
            );
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

const MAX_HELD_GRAPH_NOTES: usize = 64;

#[derive(Debug, Clone, Copy)]
struct HeldGraphNote {
    note_id: u64,
    pitch: u8,
    velocity: f32,
    seed: u64,
    clip_envelope: ClipEnvelope,
    started_frame: u64,
    sequence: u64,
}

impl HeldGraphNote {
    fn envelope_at(self, frame: u64) -> ClipEnvelope {
        let mut envelope = self.clip_envelope;
        envelope.position_frames = envelope
            .position_frames
            .saturating_add(frame.saturating_sub(self.started_frame));
        envelope
    }
}

struct GraphVoicingRuntime {
    config: CompiledVoicing,
    sample_rate: u32,
    held: [Option<HeldGraphNote>; MAX_HELD_GRAPH_NOTES],
    sequence: u64,
    current_note: Option<u64>,
    last_pitch_hz: Option<f32>,
}

impl GraphVoicingRuntime {
    fn new(config: CompiledVoicing, sample_rate: u32) -> Self {
        Self {
            config,
            sample_rate,
            held: [None; MAX_HELD_GRAPH_NOTES],
            sequence: 0,
            current_note: None,
            last_pitch_hz: None,
        }
    }

    fn reset(&mut self) {
        self.held.fill(None);
        self.sequence = 0;
        self.current_note = None;
        self.last_pitch_hz = None;
    }

    fn adopt_state_from(&mut self, current: &mut Self) {
        std::mem::swap(&mut self.held, &mut current.held);
        std::mem::swap(&mut self.sequence, &mut current.sequence);
        std::mem::swap(&mut self.current_note, &mut current.current_note);
        std::mem::swap(&mut self.last_pitch_hz, &mut current.last_pitch_hz);
    }

    fn note_on(&mut self, voices: &mut [GraphVoiceSlot], mut note: HeldGraphNote) {
        if self.config.seed != 0 {
            note.seed = mix_modulation_identity(note.seed, self.config.seed);
        }
        note.sequence = self.next_sequence();
        let frame = note.started_frame;
        match self.config.mode {
            VoicingMode::Poly => self.poly_note_on(voices, note),
            VoicingMode::Mono => {
                self.insert_held(note);
                self.apply_mono_selection(voices, frame, self.current_note.is_some());
            }
        }
    }

    fn note_off(&mut self, voices: &mut [GraphVoiceSlot], note_id: u64, frame: u64) {
        match self.config.mode {
            VoicingMode::Poly => {
                for voice in voices {
                    voice.release(note_id);
                }
            }
            VoicingMode::Mono => {
                self.remove_held(note_id);
                if self.current_note == Some(note_id) {
                    self.apply_mono_selection(voices, frame, true);
                }
            }
        }
    }

    fn next_sequence(&mut self) -> u64 {
        self.sequence = self.sequence.saturating_add(1);
        self.sequence
    }

    fn insert_held(&mut self, note: HeldGraphNote) {
        let slot = self
            .held
            .iter()
            .position(|held| held.is_some_and(|held| held.note_id == note.note_id))
            .or_else(|| self.held.iter().position(Option::is_none))
            .or_else(|| {
                self.held
                    .iter()
                    .enumerate()
                    .filter_map(|(index, held)| held.map(|held| (index, held.sequence)))
                    .min_by_key(|(_, sequence)| *sequence)
                    .map(|(index, _)| index)
            })
            .unwrap_or(0);
        self.held[slot] = Some(note);
    }

    fn remove_held(&mut self, note_id: u64) {
        for held in &mut self.held {
            if held.is_some_and(|held| held.note_id == note_id) {
                *held = None;
            }
        }
    }

    fn selected_held(&self) -> Option<HeldGraphNote> {
        let mut selected: Option<HeldGraphNote> = None;
        for candidate in self.held.iter().flatten().copied() {
            let prefer = selected.is_none_or(|current| match self.config.note_priority {
                NotePriority::Last => candidate.sequence > current.sequence,
                NotePriority::Low => {
                    candidate.pitch < current.pitch
                        || (candidate.pitch == current.pitch
                            && candidate.sequence > current.sequence)
                }
                NotePriority::High => {
                    candidate.pitch > current.pitch
                        || (candidate.pitch == current.pitch
                            && candidate.sequence > current.sequence)
                }
            });
            if prefer {
                selected = Some(candidate);
            }
        }
        selected
    }

    fn apply_mono_selection(&mut self, voices: &mut [GraphVoiceSlot], frame: u64, overlap: bool) {
        let Some(voice) = voices.first_mut() else {
            return;
        };
        let Some(selected) = self.selected_held() else {
            if let Some(note_id) = self.current_note.take() {
                voice.release(note_id);
            }
            return;
        };
        if self.current_note == Some(selected.note_id) {
            return;
        }
        let glide_start = self.glide_start(voice, overlap);
        let envelope = selected.envelope_at(frame);
        if overlap && voice.primary_active() && self.config.retrigger == RetriggerPolicy::Legato {
            voice.legato(
                selected.note_id,
                selected.pitch,
                selected.velocity,
                selected.seed,
                envelope,
                glide_start,
                self.config.glide_seconds,
                self.sample_rate,
            );
        } else {
            voice.activate_with_glide(
                selected.note_id,
                selected.pitch,
                selected.velocity,
                selected.seed,
                envelope,
                glide_start,
                self.config.glide_seconds,
                self.sample_rate,
            );
        }
        self.current_note = Some(selected.note_id);
        self.last_pitch_hz = Some(graph_note_frequency(selected.pitch));
    }

    fn poly_note_on(&mut self, voices: &mut [GraphVoiceSlot], note: HeldGraphNote) {
        let overlap = voices.iter().any(GraphVoiceSlot::gate_active);
        let slot = select_voice_with_priority(voices, self.config.steal_priority);
        let glide_start = {
            let voice = &voices[slot];
            self.glide_start(voice, overlap)
        };
        voices[slot].activate_with_glide(
            note.note_id,
            note.pitch,
            note.velocity,
            note.seed,
            note.clip_envelope,
            glide_start,
            self.config.glide_seconds,
            self.sample_rate,
        );
        self.last_pitch_hz = Some(graph_note_frequency(note.pitch));
    }

    fn glide_start(&self, voice: &GraphVoiceSlot, overlap: bool) -> Option<f32> {
        let enabled = match self.config.glide_mode {
            GlideMode::Off => false,
            GlideMode::Always => self.last_pitch_hz.is_some(),
            GlideMode::Legato => overlap,
        };
        enabled.then(|| {
            if overlap && voice.primary_active() {
                voice.pitch_hz()
            } else {
                self.last_pitch_hz.unwrap_or_else(|| voice.pitch_hz())
            }
        })
    }
}

fn graph_note_frequency(pitch: u8) -> f32 {
    440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0)
}

struct SharedGraphInstance {
    graph: Arc<CompiledGraph>,
    runtime: SharedGraphRuntime,
}

enum InstrumentRuntime {
    Frozen(Arc<[[f32; 2]]>),
    Graph {
        graph: Arc<CompiledGraph>,
        voices: Vec<GraphVoiceSlot>,
        shared: Option<SharedGraphInstance>,
        voicing: Box<GraphVoicingRuntime>,
    },
    Drums {
        synth: DrumSynth,
        tone: Option<ControlIndex>,
        punch: Option<ControlIndex>,
    },
    Sampler(SamplerRuntime),
}

impl InstrumentRuntime {
    fn transition_compatible(&self, replacement: &Self) -> bool {
        match (self, replacement) {
            (
                Self::Graph {
                    graph: current,
                    shared: current_shared,
                    voicing: current_voicing,
                    ..
                },
                Self::Graph {
                    graph: replacement,
                    shared: replacement_shared,
                    voicing: replacement_voicing,
                    ..
                },
            ) => {
                current_voicing
                    .config
                    .structurally_compatible(replacement_voicing.config)
                    && current.transition_compatible(replacement)
                    && match (current_shared, replacement_shared) {
                        (None, None) => true,
                        (Some(current), Some(replacement)) => {
                            current.graph.transition_compatible(&replacement.graph)
                        }
                        _ => false,
                    }
            }
            (Self::Drums { .. }, Self::Drums { .. }) | (Self::Sampler(_), Self::Sampler(_)) => true,
            _ => false,
        }
    }

    fn begin_compatible_handoff(
        &mut self,
        replacement: &Self,
        upstream_changed: &[bool],
        frames: u64,
    ) {
        let (
            Self::Graph {
                graph: current_graph,
                voices,
                shared: current_shared,
                voicing: current_voicing,
            },
            Self::Graph {
                graph: replacement_graph,
                shared: replacement_shared,
                voicing: replacement_voicing,
                ..
            },
        ) = (self, replacement)
        else {
            return;
        };
        let control_count = current_graph.control_count();
        let mut changed = [false; MAX_CONTROL_TARGETS];
        let changed = &mut changed[..control_count];
        debug_assert!(current_graph.compatible_transition_targets(
            replacement_graph,
            upstream_changed,
            changed,
        ));
        for voice in voices {
            voice.begin_route_transition(changed, frames);
        }
        *current_graph = replacement_graph.clone();
        if let (Some(current), Some(replacement)) =
            (current_shared.as_mut(), replacement_shared.as_ref())
        {
            debug_assert!(current.graph.compatible_transition_targets(
                &replacement.graph,
                upstream_changed,
                changed,
            ));
            current.runtime.begin_route_transition(changed, frames);
            current.graph = replacement.graph.clone();
        }
        current_voicing.config = replacement_voicing.config;
    }

    fn adopt_state_from(&mut self, current: &mut Self) {
        match (self, current) {
            (
                Self::Graph {
                    voices: replacement,
                    shared: replacement_shared,
                    voicing: replacement_voicing,
                    ..
                },
                Self::Graph {
                    voices: current,
                    shared: current_shared,
                    voicing: current_voicing,
                    ..
                },
            ) => {
                std::mem::swap(replacement, current);
                replacement_voicing.adopt_state_from(current_voicing);
                if let (Some(replacement), Some(current)) =
                    (replacement_shared.as_mut(), current_shared.as_mut())
                {
                    std::mem::swap(&mut replacement.runtime, &mut current.runtime);
                }
            }
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
            Self::Graph { .. } | Self::Drums { .. } | Self::Frozen(_) => None,
        }
    }

    fn sampler_mapping_on(&mut self, note_id: u64, mapping_index: u16, velocity: f32) -> bool {
        match self {
            Self::Sampler(sampler) => sampler.mapping_on(note_id, mapping_index, velocity),
            Self::Graph { .. } | Self::Drums { .. } | Self::Frozen(_) => false,
        }
    }

    fn dispatch(&mut self, event: RuntimeEvent, frame: u64) {
        match (self, event) {
            (
                Self::Graph {
                    voices, voicing, ..
                },
                RuntimeEvent::NoteOn {
                    note_id,
                    pitch,
                    velocity,
                    seed,
                    clip_envelope,
                },
            ) => voicing.note_on(
                voices,
                HeldGraphNote {
                    note_id,
                    pitch,
                    velocity,
                    seed,
                    clip_envelope,
                    started_frame: frame,
                    sequence: 0,
                },
            ),
            (
                Self::Graph {
                    voices, voicing, ..
                },
                RuntimeEvent::NoteOff { note_id },
            ) => voicing.note_off(voices, note_id, frame),
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

    fn reset_shared_state(&mut self, seed: u64) {
        if let Self::Graph {
            shared: Some(shared),
            ..
        } = self
        {
            shared.runtime.reset(seed);
        }
    }

    fn all_notes_off(&mut self) {
        match self {
            Self::Graph {
                voices, voicing, ..
            } => {
                for voice in voices {
                    voice.release_all();
                }
                voicing.reset();
            }
            Self::Sampler(sampler) => sampler.all_notes_off(),
            Self::Drums { .. } | Self::Frozen(_) => {}
        }
    }

    fn active_voice_count(&self) -> usize {
        match self {
            Self::Graph { voices, .. } => voices.iter().filter(|voice| voice.active()).count(),
            Self::Drums { synth, .. } => synth.active_voice_count(),
            Self::Sampler(sampler) => sampler.active_voice_count(),
            Self::Frozen(_) => 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process(
        &mut self,
        controls: &[f64],
        pre_instrument_unconstrained_controls: &[f64],
        song_modulation: &[f64],
        instrument_route_values: &[control::RuntimeInstrumentRouteValue],
        song_frame: u64,
        instrument_identity: u64,
        feedback_mutes: &mut u64,
    ) -> ([f32; 2], usize) {
        match self {
            Self::Graph {
                graph,
                voices,
                shared,
                ..
            } => {
                let mut output = [0.0_f32, 0.0_f32];
                let mut active = 0;
                for voice in voices {
                    if voice.active() {
                        active += 1;
                        let sample = voice.process(
                            graph,
                            controls,
                            pre_instrument_unconstrained_controls,
                            song_modulation,
                            instrument_route_values,
                            GraphProcessIdentity::new(song_frame, instrument_identity),
                        );
                        output[0] += sample[0];
                        output[1] += sample[1];
                        *feedback_mutes =
                            feedback_mutes.saturating_add(voice.take_feedback_mutes());
                    }
                }
                if let Some(shared) = shared {
                    output = shared.graph.process_shared(
                        &mut shared.runtime,
                        controls,
                        pre_instrument_unconstrained_controls,
                        song_modulation,
                        instrument_route_values,
                        GraphProcessIdentity::shared(song_frame, instrument_identity, output),
                    );
                    *feedback_mutes =
                        feedback_mutes.saturating_add(shared.runtime.take_feedback_mutes());
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
            Self::Frozen(frames) => (
                usize::try_from(song_frame)
                    .ok()
                    .and_then(|index| frames.get(index))
                    .copied()
                    .unwrap_or([0.0; 2]),
                0,
            ),
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

fn select_voice_with_priority(voices: &[GraphVoiceSlot], priority: StealPriority) -> usize {
    voices
        .iter()
        .position(|voice| !voice.primary_active())
        .or_else(|| match priority {
            StealPriority::ReleaseOldest => voices
                .iter()
                .enumerate()
                .filter(|(_, voice)| voice.releasing())
                .max_by_key(|(_, voice)| voice.age_frames())
                .map(|(index, _)| index),
            StealPriority::ActiveOldest => None,
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
    /// Graph voices and shared runtimes muted by the feedback guard since runtime creation.
    #[serde(default)]
    pub feedback_mutes: u64,
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
            feedback_mutes: 0,
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
    #[error("Project cannot compile because validation failed: {}", .0.iter().filter(|diagnostic| diagnostic.severity == crate::diagnostic::Severity::Error).map(|diagnostic| diagnostic.message.as_str()).collect::<Vec<_>>().join("; "))]
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
        ClipEnvelope, CompiledProject, GraphProcessIdentity, MAX_PROCESSOR_INSTANCES,
        ProcessingMode, RuntimeTelemetry,
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

    fn lead_graph() -> Arc<super::CompiledGraph> {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile");
        compiled
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
            .expect("lead graph")
    }

    fn held_note(note_id: u64, pitch: u8, frame: u64) -> super::HeldGraphNote {
        super::HeldGraphNote {
            note_id,
            pitch,
            velocity: 0.75,
            seed: note_id.wrapping_mul(17),
            clip_envelope: ClipEnvelope::TRANSPARENT,
            started_frame: frame,
            sequence: 0,
        }
    }

    #[test]
    fn mono_legato_priority_retains_voice_state_and_falls_back_through_held_notes() {
        let graph = lead_graph();
        let mut voices = vec![super::GraphVoiceSlot::new(&graph, 7)];
        let mut voicing = super::GraphVoicingRuntime::new(
            super::CompiledVoicing {
                mode: crate::model::VoicingMode::Mono,
                retrigger: crate::model::RetriggerPolicy::Legato,
                glide_seconds: 0.01,
                glide_mode: crate::model::GlideMode::Legato,
                note_priority: crate::model::NotePriority::Low,
                steal_priority: crate::model::StealPriority::ReleaseOldest,
                seed: 99,
            },
            48_000,
        );

        voicing.note_on(&mut voices, held_note(1, 60, 0));
        assert_eq!(voices[0].voice.test_note_id(), 1);
        voices[0].voice.test_set_modulation_age_frames(321);
        let first_seed = voices[0].voice.test_seed();

        voicing.note_on(&mut voices, held_note(2, 72, 10));
        assert_eq!(
            voices[0].voice.test_note_id(),
            1,
            "low priority must keep the lower held note"
        );
        voicing.note_on(&mut voices, held_note(3, 55, 20));
        assert_eq!(voices[0].voice.test_note_id(), 3);
        assert_eq!(voices[0].voice.test_modulation_age_frames(), 321);
        assert_ne!(voices[0].voice.test_seed(), first_seed);
        assert_eq!(voices[0].voice.test_glide_frames_remaining(), 480);
        assert_eq!(voices[0].crossfade_remaining, 0);

        voicing.note_off(&mut voices, 3, 30);
        assert_eq!(voices[0].voice.test_note_id(), 1);
        assert_eq!(voices[0].voice.test_modulation_age_frames(), 321);
        voicing.note_off(&mut voices, 1, 40);
        assert_eq!(voices[0].voice.test_note_id(), 2);
        voicing.note_off(&mut voices, 2, 50);
        assert!(voices[0].voice.releasing());
        assert!(voicing.current_note.is_none());
    }

    #[test]
    fn mono_held_note_saturation_evicts_the_oldest_identity_deterministically() {
        let graph = lead_graph();
        let mut voices = vec![super::GraphVoiceSlot::new(&graph, 13)];
        let mut voicing = super::GraphVoicingRuntime::new(
            super::CompiledVoicing {
                mode: crate::model::VoicingMode::Mono,
                retrigger: crate::model::RetriggerPolicy::Legato,
                glide_seconds: 0.0,
                glide_mode: crate::model::GlideMode::Off,
                note_priority: crate::model::NotePriority::Last,
                steal_priority: crate::model::StealPriority::ReleaseOldest,
                seed: 0,
            },
            48_000,
        );

        for note_id in 1..=super::MAX_HELD_GRAPH_NOTES as u64 + 1 {
            voicing.note_on(
                &mut voices,
                held_note(note_id, 48 + (note_id % 24) as u8, note_id),
            );
        }
        assert_eq!(
            voicing.held.iter().flatten().count(),
            super::MAX_HELD_GRAPH_NOTES
        );
        assert!(
            voicing.held.iter().flatten().all(|held| held.note_id != 1),
            "the oldest held identity must be evicted first"
        );

        for note_id in (2..=super::MAX_HELD_GRAPH_NOTES as u64 + 1).rev() {
            voicing.note_off(&mut voices, note_id, 100 + note_id);
        }
        assert!(voicing.current_note.is_none());
        voicing.note_off(&mut voices, 1, 1_000);
        assert!(voicing.current_note.is_none());
    }

    #[test]
    fn mono_always_retrigger_restarts_state_behind_the_preallocated_crossfade() {
        let graph = lead_graph();
        let mut voices = vec![super::GraphVoiceSlot::new(&graph, 11)];
        let mut voicing = super::GraphVoicingRuntime::new(
            super::CompiledVoicing {
                mode: crate::model::VoicingMode::Mono,
                retrigger: crate::model::RetriggerPolicy::Always,
                glide_seconds: 0.0,
                glide_mode: crate::model::GlideMode::Off,
                note_priority: crate::model::NotePriority::Last,
                steal_priority: crate::model::StealPriority::ReleaseOldest,
                seed: 0,
            },
            48_000,
        );
        voicing.note_on(&mut voices, held_note(1, 60, 0));
        voices[0].voice.test_set_modulation_age_frames(99);
        voicing.note_on(&mut voices, held_note(2, 64, 1));
        assert_eq!(voices[0].voice.test_note_id(), 2);
        assert_eq!(voices[0].voice.test_modulation_age_frames(), 0);
        assert_eq!(
            voices[0].crossfade_remaining,
            super::GRAPH_STEAL_CROSSFADE_FRAMES
        );
    }

    #[test]
    fn central_glide_is_linear_in_log_frequency_and_voice_stealing_is_explicit() {
        let graph = lead_graph();
        let mut voice = graph.create_voice(3);
        voice.activate(1, 60, 1.0, 5);
        let start = super::graph_note_frequency(60);
        let target = super::graph_note_frequency(72);
        voice.configure_glide(Some(start), 72, 0.01, 1_000);
        assert!((voice.pitch_hz() - start).abs() < 1.0e-4);
        for _ in 0..5 {
            voice.test_advance_glide();
        }
        let geometric_midpoint = (start * target).sqrt();
        assert!((voice.pitch_hz() - geometric_midpoint).abs() < 1.0e-3);
        for _ in 0..5 {
            voice.test_advance_glide();
        }
        assert!((voice.pitch_hz() - target).abs() < 1.0e-3);

        let mut slots = [
            super::GraphVoiceSlot::new(&graph, 21),
            super::GraphVoiceSlot::new(&graph, 22),
        ];
        slots[0].activate(1, 60, 1.0, 1, ClipEnvelope::TRANSPARENT);
        slots[1].activate(2, 64, 1.0, 2, ClipEnvelope::TRANSPARENT);
        slots[0].voice.test_set_age_frames(100);
        slots[1].voice.release_all();
        slots[1].voice.test_set_age_frames(10);
        assert_eq!(
            super::select_voice_with_priority(&slots, crate::model::StealPriority::ReleaseOldest),
            1
        );
        assert_eq!(
            super::select_voice_with_priority(&slots, crate::model::StealPriority::ActiveOldest),
            0
        );
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
        for song_frame in 0..2_000 {
            previous = slot.process(
                &graph,
                controls.values(),
                controls.pre_instrument_unconstrained_values(),
                controls.song_modulation_values(),
                controls.instrument_route_values(),
                GraphProcessIdentity::new(song_frame, 5),
            );
        }

        slot.activate(2, 72, 1.0, 202, ClipEnvelope::TRANSPARENT);
        assert_eq!(
            slot.crossfade_remaining,
            super::GRAPH_STEAL_CROSSFADE_FRAMES
        );
        let replacement = slot.process(
            &graph,
            controls.values(),
            controls.pre_instrument_unconstrained_values(),
            controls.song_modulation_values(),
            controls.instrument_route_values(),
            GraphProcessIdentity::new(2_000, 5),
        );
        assert!(
            (replacement[0] - previous[0]).abs() < 0.1
                && (replacement[1] - previous[1]).abs() < 0.1,
            "replacement discontinuity: {previous:?} -> {replacement:?}"
        );
        for offset in 1..super::GRAPH_STEAL_CROSSFADE_FRAMES {
            let sample = slot.process(
                &graph,
                controls.values(),
                controls.pre_instrument_unconstrained_values(),
                controls.song_modulation_values(),
                controls.instrument_route_values(),
                GraphProcessIdentity::new(2_000 + u64::from(offset), 5),
            );
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
            .saturating_add(resources.shared_graph_state_bytes)
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
                super::InstrumentRuntime::Graph { .. }
                | super::InstrumentRuntime::Sampler(_)
                | super::InstrumentRuntime::Frozen(_) => 0,
            })
            .fold(0_u64, u64::saturating_add);
        let allocated_sampler_state = first
            .tracks
            .iter()
            .map(|track| match &track.instrument {
                super::InstrumentRuntime::Sampler(sampler) => sampler.allocated_voice_state_bytes(),
                super::InstrumentRuntime::Graph { .. }
                | super::InstrumentRuntime::Drums { .. }
                | super::InstrumentRuntime::Frozen(_) => 0,
            })
            .fold(0_u64, u64::saturating_add);
        let allocated_graph_state = |runtime: &super::SongRuntime| {
            runtime
                .tracks
                .iter()
                .map(|track| match &track.instrument {
                    super::InstrumentRuntime::Graph { voices, .. } => {
                        assert_eq!(voices.len(), voices.capacity());
                        voices
                            .iter()
                            .map(super::GraphVoiceSlot::allocation_bytes)
                            .fold(0_u64, u64::saturating_add)
                    }
                    super::InstrumentRuntime::Sampler(_)
                    | super::InstrumentRuntime::Drums { .. }
                    | super::InstrumentRuntime::Frozen(_) => 0,
                })
                .fold(0_u64, u64::saturating_add)
        };
        assert_eq!(resources.drum_voice_state_bytes, allocated_drum_state);
        assert_eq!(resources.sampler_voice_state_bytes, allocated_sampler_state);
        assert_eq!(
            resources.graph_voice_state_bytes,
            allocated_graph_state(&first)
        );
        assert_eq!(
            resources.graph_voice_state_bytes,
            allocated_graph_state(&second)
        );
        assert_eq!(
            resources
                .graph_voice_state_bytes
                .saturating_mul(resources.publication_runtime_slots),
            allocated_graph_state(&first).saturating_add(allocated_graph_state(&second))
        );
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
    fn shared_state_can_independently_fail_the_named_admission_budget() {
        // Enough 2 s shared delays on every Track to cross the publication budget.
        let delays = 28;
        let nodes = (1..=delays)
            .map(|index| {
                format!("        node shared-delay-{index} delay time 2s feedback 0.5 mix $fx\n")
            })
            .collect::<String>();
        let chain = (2..=delays)
            .map(|index| {
                format!(
                    "        bus delay-chain-{index} shared-delay-{}.audio -> shared-delay-{index}.audio\n",
                    index - 1
                )
            })
            .collect::<String>();
        let source = include_str!("../../tests/fixtures/shared_graph_foundation.crump").replace(
            "      graph shared 2 {\n        node shared-delay delay time 5ms feedback 0.5 mix $fx\n        node shared-out output\n        bus voice-sum voice_sum.audio -> shared-delay.audio\n        bus shared-output shared-delay.audio -> shared-out.audio\n      }",
            &format!(
                "      graph shared {} {{\n{nodes}        node shared-out output\n        bus voice-sum voice_sum.audio -> shared-delay-1.audio\n{chain}        bus shared-output shared-delay-{delays}.audio -> shared-out.audio\n      }}",
                delays + 1
            ),
        );
        let mut heavy = fixture(&source);
        let template = heavy.song.tracks[0].clone();
        for index in 1..super::schedule::MAX_RUNTIME_TRACKS {
            let mut track = template.clone();
            track.id = format!("lead-track-{index}");
            track.name = format!("Lead {index}");
            track.clips[0].id = format!("notes-{index}");
            heavy.song.tracks.push(track);
        }

        let mut without_shared = heavy.clone();
        without_shared.song.instruments[0].shared_graph = None;
        without_shared.song.instruments[0]
            .faceplate
            .retain(|control| control.key != "fx");
        CompiledProject::compile(&without_shared)
            .expect("the same Project without shared state must fit the budget");

        let diagnostics = crate::validate::validate_project(&heavy);
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV093"),
            "shared Delay state alone should cross the named live publication budget: {diagnostics:?}"
        );
        assert!(CompiledProject::compile(&heavy).is_err());
    }

    #[test]
    fn shared_graph_admission_matches_allocated_track_state() {
        let project = fixture(include_str!(
            "../../tests/fixtures/shared_graph_foundation.crump"
        ));
        let compiled = CompiledProject::compile(&project).expect("compile shared graph");
        let resources = &compiled.mix_graph_inspection().resources;
        let allocated = |runtime: &super::SongRuntime| {
            runtime
                .tracks
                .iter()
                .map(|track| match &track.instrument {
                    super::InstrumentRuntime::Graph {
                        shared: Some(shared),
                        ..
                    } => shared.runtime.allocation_bytes(),
                    super::InstrumentRuntime::Graph { shared: None, .. }
                    | super::InstrumentRuntime::Sampler(_)
                    | super::InstrumentRuntime::Drums { .. }
                    | super::InstrumentRuntime::Frozen(_) => 0,
                })
                .fold(0_u64, u64::saturating_add)
        };
        let first = compiled.runtime();
        let second = compiled.runtime();
        assert_eq!(resources.shared_graphs, 1);
        assert!(resources.shared_graph_work_units_per_frame > 0);
        assert_eq!(resources.shared_graph_state_bytes, allocated(&first));
        assert_eq!(resources.shared_graph_state_bytes, allocated(&second));
        assert_eq!(
            resources
                .shared_graph_state_bytes
                .saturating_mul(resources.publication_runtime_slots),
            allocated(&first).saturating_add(allocated(&second))
        );
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

    /// Builds a Project with a runaway feedback Instrument and a plain tone.
    fn runaway_project(gain: &str) -> CompiledProject {
        let runaway = format!(
            r#"crump-module 2
instrument runaway "Runaway" graph {{
  polyphony 2
  parameter level "Level" ratio 0 1 0.5 0.5
  faceplate {{
    control level "Level" "Output level" maps level
  }}
  graph voice 40 {{
    node pluck exciter kind impact contact 0.02ms hardness 1 level $level
    node mix math operation add
    node previous delay_line mode time time 1ms interpolation linear max_time 2ms
    node grow math operation scale amount {gain}
    node stereo mono_to_stereo
    node out output
    bus gate note.gate -> pluck.gate
    bus velocity note.velocity -> pluck.velocity
    bus excite pluck.audio -> mix.a
    bus into mix.audio -> previous.audio
    bus held previous.audio -> grow.a
    bus back grow.audio -> mix.b
    bus tap mix.audio -> stereo.audio
    bus output stereo.audio -> out.audio
  }}
}}
"#
        );
        let tone = r#"crump-module 2
instrument tone "Tone" graph {
  polyphony 2
  parameter level "Level" ratio 0 1 0.5 0.5
  faceplate {
    control level "Level" "Output level" maps level
  }
  graph voice 4 {
    node osc oscillator waveform sine level $level
    node stereo mono_to_stereo
    node out output
    bus pitch note.pitch -> osc.pitch
    bus tap osc.audio -> stereo.audio
    bus output stereo.audio -> out.audio
  }
}
"#;
        let project = r#"crump 2
import "runaway.crump"
import "tone.crump"
project "Feedback" {
  id feedback
  seed 7
  sample_rate 48000
  render_tail 1s
  song {
    tempo 120bpm
    meter 4/4
    end 2bar
    pattern notes 2bar {
      note 0beat C4 1beat 0.8
    }
    track runaway "runaway" instrument runaway level 0db pan 0 {
      clip runaway-clip pattern notes start 0bar duration 2bar mode once
    }
    track tone "tone" instrument tone level 0db pan 0 {
      clip tone-clip pattern notes start 0bar duration 2bar mode once
    }
  }
}
"#;
        let bundle = crate::project::load_project_sources(
            std::path::PathBuf::from("/feedback-transition-test"),
            std::collections::BTreeMap::from([
                (
                    std::path::PathBuf::from("project.crump"),
                    project.to_owned(),
                ),
                (std::path::PathBuf::from("runaway.crump"), runaway),
                (std::path::PathBuf::from("tone.crump"), tone.to_owned()),
            ]),
        );
        assert!(
            !bundle.diagnostics.has_errors(),
            "{:?}",
            bundle.diagnostics.as_slice()
        );
        CompiledProject::compile(bundle.project.as_ref().expect("parsed Project")).expect("compile")
    }

    #[test]
    fn publication_crossfade_counts_feedback_mutes_like_playback() {
        const FRAMES: u64 = 4_800;
        let compiled = runaway_project("64");
        let mut baseline = compiled.runtime();
        for _ in 0..FRAMES {
            baseline
                .process_frame(ProcessingMode::Monitoring)
                .expect("baseline frame");
        }
        let expected_mutes = baseline.telemetry().feedback_mutes;
        assert!(expected_mutes > 0, "the runaway voice must trip the guard");

        // Unchanged Tracks run in the outgoing runtime and match plain playback.
        let mut reference = compiled.runtime();
        let mut current = compiled.runtime();
        let mut next = compiled.runtime();
        current.prepare_transition(&mut next);
        assert!(
            current
                .tracks
                .iter()
                .all(|track| track.transition_unchanged)
        );
        for _ in 0..FRAMES {
            let expected = reference
                .process_frame(ProcessingMode::Monitoring)
                .expect("reference frame");
            let actual = current
                .process_transition_frame(&mut next, 0.0, 1.0)
                .expect("transition frame");
            assert_eq!(actual, expected);
        }
        assert_eq!(current.telemetry().feedback_mutes, expected_mutes);

        // An affected runaway Track runs in the replacement, which reports its mutes.
        let replacement = runaway_project("63");
        let mut current = compiled.runtime();
        let mut next = replacement.runtime();
        current.prepare_transition(&mut next);
        assert!(next.tracks.iter().any(|track| !track.transition_unchanged));
        for _ in 0..FRAMES {
            current
                .process_transition_frame(&mut next, 0.0, 1.0)
                .expect("transition frame");
        }
        assert!(next.telemetry().feedback_mutes > 0);
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
    fn compatible_shared_graph_state_runs_once_and_survives_publication() {
        let source = include_str!("../../tests/fixtures/shared_graph_foundation.crump");
        let reordered = source
            .replace(
                "        node shared-delay delay time 5ms feedback 0.5 mix $fx\n        node shared-out output",
                "        node shared-out output\n        node shared-delay delay time 5ms feedback 0.5 mix $fx",
            )
            .replace(
                "        bus voice-sum voice_sum.audio -> shared-delay.audio\n        bus shared-output shared-delay.audio -> shared-out.audio",
                "        bus shared-output shared-delay.audio -> shared-out.audio\n        bus voice-sum voice_sum.audio -> shared-delay.audio",
            );
        let compiled = CompiledProject::compile(&fixture(source)).expect("compile shared graph");
        let shifted =
            CompiledProject::compile(&fixture(&reordered)).expect("compile reordered shared graph");
        let mut baseline = compiled.runtime();
        let mut current = compiled.runtime();
        let mut next = shifted.runtime();
        baseline.advance_to(10_000).expect("advance baseline");
        current.advance_to(10_000).expect("advance current");
        current.prepare_transition(&mut next);
        assert!(current.tracks[0].transition_unchanged);

        for position in 1..=480_u64 {
            let expected = baseline
                .process_frame(ProcessingMode::Monitoring)
                .expect("baseline shared frame");
            let phase = position as f64 / 480.0;
            let actual = current
                .process_transition_frame(
                    &mut next,
                    (phase * std::f64::consts::FRAC_PI_2).cos() as f32,
                    (phase * std::f64::consts::FRAC_PI_2).sin() as f32,
                )
                .expect("shared transition frame");
            assert_eq!(actual, expected, "shared frame {position}");
        }
        current.complete_transition(&mut next);
        for _ in 0..256 {
            assert_eq!(
                next.process_frame(ProcessingMode::Monitoring)
                    .expect("replacement shared frame"),
                baseline
                    .process_frame(ProcessingMode::Monitoring)
                    .expect("baseline shared continuation")
            );
        }
    }

    #[test]
    fn track_parameter_locks_do_not_leak_into_another_tracks_shared_tail() {
        let source = r#"crump 2
project "Track Lock Isolation" {
  id track-lock-isolation
  seed 91
  sample_rate 48000
  render_tail 20ms
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument lead "Lead" graph {
      polyphony 1
      parameter level "Level" ratio 0 1 0.4 0.4
      parameter fx "FX" ratio 0 1 0.25 0.25 automatable
      faceplate {
        control level "Level" "Voice level" maps level
        control fx "FX" "Shared delay mix" maps fx
      }
      graph voice 5 {
        node oscillator oscillator waveform saw level $level
        node envelope envelope attack 1ms decay 1ms sustain 1 release 1ms
        node amplifier amplifier level 1
        node widen mono_to_stereo
        node voice-out output
        bus pitch note.pitch -> oscillator.pitch
        bus gate note.gate -> envelope.gate
        bus gain envelope.control -> amplifier.gain
        bus mono oscillator.audio -> amplifier.audio
        bus widened amplifier.audio -> widen.audio
        bus stereo widen.audio -> voice-out.audio
      }
      graph shared 2 {
        node delay delay time 5ms feedback 0.6 mix $fx
        node shared-out output
        bus crossing voice_sum.audio -> delay.audio
        bus output delay.audio -> shared-out.audio
      }
    }
    pattern controller 1bar {
      lock controller-fx at 0beat parameter fx value 0.1
    }
    pattern audible 1bar {
      note 0beat C4 1/64beat 0.8
    }
    track controller "Controller" instrument lead level 0db pan 0 {
      clip controller-clip pattern controller start 1/16beat duration 1/16beat mode once
    }
    track audible "Audible" instrument lead level 0db pan 0 {
      clip audible-clip pattern audible start 0beat duration 1/16beat mode once
    }
  }
}
"#;
        let render = |source: &str| {
            let compiled =
                CompiledProject::compile(&fixture(source)).expect("compile lock fixture");
            let mut runtime = compiled.runtime();
            (0..4_096)
                .map(|_| {
                    runtime
                        .process_frame(ProcessingMode::Offline)
                        .expect("render lock isolation frame")
                })
                .collect::<Vec<_>>()
        };
        let low_lock = render(source);
        let high_lock = render(&source.replace("value 0.1", "value 0.9"));
        assert!(low_lock.iter().any(|sample| sample != &[0.0, 0.0]));
        assert_eq!(
            low_lock, high_lock,
            "a silent controller Track's lock must not alter another Track's shared tail"
        );
    }

    #[test]
    fn shared_graph_runs_after_voice_sum_and_before_instrument_processors() {
        let source = r#"crump 2
project "Shared Ordering" {
  id shared-ordering
  seed 17
  sample_rate 48000
  render_tail 20ms
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument lead "Lead" graph {
      polyphony 1
      parameter level "Level" ratio 0 1 0.8 0.8
      faceplate {
        control level "Level" "Voice level" maps level
      }
      processor mono-post utility {
        setting mono true
      }
      graph voice 3 {
        node oscillator oscillator waveform saw level $level
        node position pan position -0.75
        node voice-out output
        bus pitch note.pitch -> oscillator.pitch
        bus positioned oscillator.audio -> position.audio
        bus output position.audio -> voice-out.audio
      }
      graph shared 2 {
        node room reverb room 0.8 damping 0.2 mix 1
        node shared-out output
        bus crossing voice_sum.audio -> room.audio
        bus output room.audio -> shared-out.audio
      }
    }
    pattern notes 1bar {
      note 0beat C4 1beat 0.8
    }
    track lead "Lead" instrument lead level 0db pan 0 {
      clip notes pattern notes start 0bar duration 1bar mode once
    }
  }
}
"#;
        let without_post = source.replace(
            "      processor mono-post utility {\n        setting mono true\n      }\n",
            "",
        );
        let render = |source: &str| {
            let compiled =
                CompiledProject::compile(&fixture(source)).expect("compile ordering fixture");
            let mut runtime = compiled.runtime();
            (0..4_096)
                .map(|_| {
                    runtime
                        .process_frame(ProcessingMode::Offline)
                        .expect("render ordering frame")
                })
                .collect::<Vec<_>>()
        };
        let post_processed = render(source);
        let shared_only = render(&without_post);
        assert!(
            shared_only
                .iter()
                .any(|sample| sample[0].to_bits() != sample[1].to_bits()),
            "the asymmetric voice and shared reverb must exercise stereo ordering"
        );
        for (frame, (actual, shared)) in post_processed.iter().zip(&shared_only).enumerate() {
            let mono = (shared[0] + shared[1]) * 0.5;
            assert_eq!(
                *actual,
                [mono, mono],
                "Instrument processor did not receive shared output at frame {frame}"
            );
        }
    }

    #[test]
    fn shared_graph_callback_path_reuses_preallocated_state() {
        let compiled = CompiledProject::compile(&fixture(include_str!(
            "../../tests/fixtures/shared_graph_foundation.crump"
        )))
        .expect("compile shared allocation fixture");
        let mut runtime = compiled.runtime();
        let mut block = vec![[0.0_f32; 2]; 256];
        runtime
            .process_block(&mut block, ProcessingMode::Offline)
            .expect("warm shared callback");
        let shared_allocation = |runtime: &super::SongRuntime| match &runtime.tracks[0].instrument {
            super::InstrumentRuntime::Graph {
                shared: Some(shared),
                ..
            } => shared.runtime.allocation_bytes(),
            _ => panic!("shared graph runtime"),
        };
        let before = shared_allocation(&runtime);
        let block_capacity = block.capacity();
        let mut processed = 0;
        for _ in 0..128 {
            processed += runtime
                .process_block(&mut block, ProcessingMode::Offline)
                .expect("shared callback block");
        }
        assert_eq!(processed, 128 * block.len());
        assert_eq!(block.capacity(), block_capacity);
        assert_eq!(
            shared_allocation(&runtime),
            before,
            "shared Node, signal, control, transition, and tail buffers must never resize"
        );
    }

    #[test]
    fn seeking_resets_track_owned_shared_dsp_state() {
        let compiled = CompiledProject::compile(&fixture(include_str!(
            "../../tests/fixtures/shared_graph_foundation.crump"
        )))
        .expect("compile shared seek fixture");
        let mut used = compiled.runtime();
        for _ in 0..1_024 {
            used.process_frame(ProcessingMode::Offline)
                .expect("excite shared delay");
        }
        used.tracks[0].instrument.all_notes_off();
        for _ in 0..7_000 {
            used.process_frame(ProcessingMode::Offline)
                .expect("release voices while retaining shared tail");
        }
        assert_eq!(used.tracks[0].active_voice_count(), 0);
        used.seek_transport(0);
        let mut fresh = compiled.runtime();
        for frame in 0..1_024 {
            assert_eq!(
                used.process_frame(ProcessingMode::Offline)
                    .expect("post-seek frame"),
                fresh
                    .process_frame(ProcessingMode::Offline)
                    .expect("fresh frame"),
                "shared state leaked across seek at frame {frame}"
            );
        }
    }

    #[test]
    fn compatible_shared_scalar_edits_ramp_but_allocation_edits_restart() {
        fn shared_control(
            runtime: &super::SongRuntime,
            target: super::control::ControlIndex,
        ) -> f64 {
            let super::InstrumentRuntime::Graph {
                shared: Some(shared),
                ..
            } = &runtime.tracks[0].instrument
            else {
                panic!("shared graph runtime");
            };
            shared.runtime.effective_control(target)
        }

        let source = include_str!("../../tests/fixtures/shared_graph_foundation.crump");
        let edited = source.replace(
            "parameter fx \"Shared FX\" ratio 0 1 0.25 0.25 modulatable",
            "parameter fx \"Shared FX\" ratio 0 1 0.75 0.75 modulatable",
        );
        let compiled = CompiledProject::compile(&fixture(source)).expect("compile shared graph");
        let replacement =
            CompiledProject::compile(&fixture(&edited)).expect("compile shared scalar edit");
        let fx = compiled
            .controls
            .index(&crate::model::ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "fx".to_owned(),
            })
            .expect("fx target");
        let mut current = compiled.runtime();
        let mut next = replacement.runtime();
        current.advance_to(1_000).expect("advance current");
        let before = shared_control(&current, fx);
        current.prepare_transition(&mut next);
        assert!(current.tracks[0].transition_unchanged);
        for position in 1..=480_u64 {
            let phase = position as f64 / 480.0;
            current
                .process_transition_frame(
                    &mut next,
                    (phase * std::f64::consts::FRAC_PI_2).cos() as f32,
                    (phase * std::f64::consts::FRAC_PI_2).sin() as f32,
                )
                .expect("shared scalar transition");
        }
        let after = shared_control(&current, fx);
        assert!(after > before + 0.45, "{before} -> {after}");
        current.complete_transition(&mut next);
        assert_eq!(shared_control(&next, fx), after);

        let allocation_edit = source.replace("time 5ms", "time 10ms");
        let allocation = CompiledProject::compile(&fixture(&allocation_edit))
            .expect("compile shared allocation edit");
        let mut current = compiled.runtime();
        let mut next = allocation.runtime();
        current.advance_to(1_000).expect("advance current");
        current.prepare_transition(&mut next);
        assert!(!current.tracks[0].transition_unchanged);
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
    fn stable_graph_declaration_reordering_preserves_active_voice_state() {
        let project = fixture(include_str!("../../tests/fixtures/stereo_foundation.crump"));
        let original = CompiledProject::compile(&project).expect("compile original graph");
        let mut reordered = project.clone();
        let graph = reordered.song.instruments[0].graph.as_mut().unwrap();
        graph.nodes.reverse();
        for node in &mut graph.nodes {
            node.settings.reverse();
        }
        graph.connections.reverse();
        let reordered = CompiledProject::compile(&reordered).expect("compile reordered graph");

        let mut current = original.runtime();
        let mut next = reordered.runtime();
        current.advance_to(1_000).expect("activate graph voice");
        assert!(current.tracks[0].active_voice_count() > 0);
        current.prepare_transition(&mut next);
        assert!(current.tracks[0].transition_unchanged);
        for _ in 0..480 {
            current
                .process_transition_frame(&mut next, 1.0, 0.0)
                .expect("compatible transition frame");
        }
        current.complete_transition(&mut next);
        assert!(next.tracks[0].active_voice_count() > 0);
    }

    #[test]
    fn legacy_connection_source_reordering_preserves_active_voice_state() {
        let original_source = include_str!("../../tests/fixtures/minimal.crump");
        let reordered_source = original_source.replace(
            "        connect note.pitch -> oscillator.pitch\n        connect note.gate -> envelope.gate",
            "        connect note.gate -> envelope.gate\n        connect note.pitch -> oscillator.pitch",
        );
        assert_ne!(original_source, reordered_source);
        let original =
            CompiledProject::compile(&fixture(original_source)).expect("compile original");
        let reordered =
            CompiledProject::compile(&fixture(&reordered_source)).expect("compile reordered");

        let mut current = original.runtime();
        let mut next = reordered.runtime();
        current.advance_to(1_000).expect("activate graph voice");
        assert!(current.tracks[0].active_voice_count() > 0);
        current.prepare_transition(&mut next);
        assert!(current.tracks[0].transition_unchanged);
        for _ in 0..480 {
            current
                .process_transition_frame(&mut next, 1.0, 0.0)
                .expect("compatible transition frame");
        }
        current.complete_transition(&mut next);
        assert!(next.tracks[0].active_voice_count() > 0);
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
            .iter_mut()
            .find(|setting| setting.key == "waveform")
            .expect("waveform setting")
            .value = crate::model::SettingValue::Text("square".to_owned());
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
    fn modulation_declaration_reordering_preserves_active_voice_state() {
        let project = fixture(include_str!(
            "../../tests/fixtures/instrument_modulation.crump"
        ));
        let original = CompiledProject::compile(&project).expect("compile original order");
        let mut reordered = project;
        let instrument = &mut reordered.song.instruments[0];
        instrument.parameters.reverse();
        instrument.modulation_sources.reverse();
        instrument.modulation_routes.reverse();
        reordered.song.modulations.reverse();
        reordered.song.envelope_followers.reverse();
        let reordered = CompiledProject::compile(&reordered).expect("compile reordered program");

        let mut current = original.runtime();
        for _ in 0..64 {
            current
                .process_frame(ProcessingMode::Offline)
                .expect("warm voice");
        }
        let mut next = reordered.runtime();
        current.prepare_transition(&mut next);
        assert!(
            current
                .tracks
                .iter()
                .all(|track| track.transition_unchanged),
            "semantically unordered declarations must not restart Tracks"
        );
        current.complete_transition(&mut next);
        assert!(
            next.tracks
                .iter()
                .all(|track| track.active_voice_count() > 0)
        );
    }

    #[test]
    fn active_parameter_lock_edits_adopt_the_program_with_a_bounded_handoff() {
        let source = include_str!("../../tests/fixtures/minimal.crump")
            .replace(
                "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz",
                "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz automatable",
            )
            .replace(
                "      note 0beat C4 1beat 0.8",
                "      lock tone-lock at 0beat parameter tone value 3200hz\n      note 0beat C4 1beat 0.8",
            );
        let original_project = fixture(&source);
        let original = CompiledProject::compile(&original_project).expect("compile original lock");
        let changed_source = source.replace(
            "lock tone-lock at 0beat parameter tone value 3200hz",
            "lock tone-lock at 0beat parameter tone value 4000hz",
        );
        let changed =
            CompiledProject::compile(&fixture(&changed_source)).expect("compile changed lock");
        let tone = original
            .controls
            .index(&crate::model::ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "tone".to_owned(),
            })
            .expect("tone");
        let locked_value = |runtime: &super::SongRuntime| {
            runtime
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .and_then(|track| track.parameter_locks.as_ref())
                .map(|locks| locks.effective_value(tone))
                .expect("lead Track lock projection")
        };
        let mut current = original.runtime();
        for _ in 0..64 {
            current
                .process_frame(ProcessingMode::Offline)
                .expect("warm locked voice");
        }
        let mut next = changed.runtime();
        current.prepare_transition(&mut next);
        assert!(current.scalar_control_handoff);
        assert_eq!(locked_value(&current), 3_200.0);
        current
            .process_transition_frame(&mut next, 1.0, 0.0)
            .expect("first lock handoff frame");
        assert!(locked_value(&current) > 3_200.0);
        assert!(locked_value(&current) < 4_000.0);
        for _ in 1..480 {
            current
                .process_transition_frame(&mut next, 1.0, 0.0)
                .expect("lock handoff frame");
        }
        assert_eq!(locked_value(&current), 4_000.0);
        current.complete_transition(&mut next);
        assert_eq!(locked_value(&next), 4_000.0);
        assert!(
            next.tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .expect("lead Track")
                .active_voice_count()
                > 0
        );
    }

    #[test]
    fn parameter_lock_addition_and_removal_keep_track_projection_through_handoff() {
        let base_source = include_str!("../../tests/fixtures/minimal.crump").replace(
            "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz",
            "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz automatable",
        );
        let locked_source = base_source.replace(
            "      note 0beat C4 1beat 0.8",
            "      lock tone-lock at 0beat parameter tone value 1000hz\n      note 0beat C4 1beat 0.8",
        );
        let base = CompiledProject::compile(&fixture(&base_source)).expect("compile unlocked");
        let locked = CompiledProject::compile(&fixture(&locked_source)).expect("compile locked");
        let tone = base
            .controls
            .index(&crate::model::ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "tone".to_owned(),
            })
            .expect("tone");

        let mut current = base.runtime();
        for _ in 0..64 {
            current
                .process_frame(ProcessingMode::Offline)
                .expect("warm unlocked runtime");
        }
        let mut next = locked.runtime();
        current.prepare_transition(&mut next);
        assert!(current.scalar_control_handoff);
        for _ in 0..480 {
            current
                .process_transition_frame(&mut next, 1.0, 0.0)
                .expect("add-lock handoff");
        }
        let lock_value = |runtime: &super::SongRuntime| {
            runtime
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .and_then(|track| track.parameter_locks.as_ref())
                .map(|locks| locks.effective_value(tone))
        };
        assert_eq!(lock_value(&current), Some(1_000.0));
        current.complete_transition(&mut next);
        assert_eq!(lock_value(&next), Some(1_000.0));

        let mut current = locked.runtime();
        for _ in 0..64 {
            current
                .process_frame(ProcessingMode::Offline)
                .expect("warm locked runtime");
        }
        let mut next = base.runtime();
        current.prepare_transition(&mut next);
        assert!(current.scalar_control_handoff);
        assert!(
            current
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .is_some_and(|track| track.parameter_locks_handoff),
            "the departing Track must own the lock-removal ramp"
        );
        for _ in 0..480 {
            current
                .process_transition_frame(&mut next, 1.0, 0.0)
                .expect("remove-lock handoff");
        }
        assert_eq!(lock_value(&current), Some(3_200.0));
        current.complete_transition(&mut next);
        assert!(
            next.tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .is_some_and(|track| track.parameter_locks.is_none())
        );
        next.process_frame(ProcessingMode::Offline)
            .expect("post-removal frame");
        assert_eq!(next.controls.values()[tone.0], 3_200.0);
    }

    #[test]
    fn lock_edits_crossfade_the_owner_when_an_unrelated_track_is_incompatible() {
        let source = include_str!("../../tests/fixtures/minimal.crump")
            .replace(
                "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz",
                "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz automatable",
            )
            .replace(
                "      note 0beat C4 1beat 0.8",
                "      lock tone-lock at 0beat parameter tone value 3200hz\n      note 0beat C4 1beat 0.8",
            );
        let original = CompiledProject::compile(&fixture(&source)).expect("compile original");
        let replacement = source
            .replace(
                "lock tone-lock at 0beat parameter tone value 3200hz",
                "lock tone-lock at 0beat parameter tone value 4000hz",
            )
            .replace("hit 0beat kick 0.9", "hit 0beat kick 0.8");
        let changed =
            CompiledProject::compile(&fixture(&replacement)).expect("compile replacement");
        let tone = changed
            .controls
            .index(&crate::model::ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "tone".to_owned(),
            })
            .expect("tone");

        let mut current = original.runtime();
        for _ in 0..64 {
            current
                .process_frame(ProcessingMode::Offline)
                .expect("warm current runtime");
        }
        let mut next = changed.runtime();
        current.prepare_transition(&mut next);
        assert!(!current.scalar_control_handoff);
        assert!(
            !current
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .expect("current lead Track")
                .transition_unchanged,
            "the lock owner must be rendered on both sides of the publication fade"
        );
        assert!(
            !next
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .expect("replacement lead Track")
                .transition_unchanged
        );
        current
            .process_transition_frame(&mut next, 1.0, 0.0)
            .expect("first transition frame");
        assert_eq!(
            next.tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .and_then(|track| track.parameter_locks.as_ref())
                .map(|locks| locks.effective_value(tone)),
            Some(4_000.0)
        );
    }

    #[test]
    fn instrument_route_scalar_edits_crossfade_active_voices_and_macro_dependents() {
        fn active_voice_control(
            runtime: &super::SongRuntime,
            target: super::control::ControlIndex,
        ) -> f64 {
            let track = runtime
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead-track")
                .expect("lead Track");
            let super::InstrumentRuntime::Graph { voices, .. } = &track.instrument else {
                panic!("lead must use a graph Instrument");
            };
            voices
                .iter()
                .find(|slot| slot.voice.active())
                .expect("active graph voice")
                .voice
                .effective_control(target)
        }

        fn active_voice_target_changed(
            runtime: &super::SongRuntime,
            target: super::control::ControlIndex,
        ) -> bool {
            let track = runtime
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead-track")
                .expect("lead Track");
            let super::InstrumentRuntime::Graph { voices, .. } = &track.instrument else {
                panic!("lead must use a graph Instrument");
            };
            voices
                .iter()
                .find(|slot| slot.voice.active())
                .expect("active graph voice")
                .voice
                .transition_target_changed(target)
        }

        let mut project = fixture(include_str!(
            "../../tests/fixtures/instrument_modulation.crump"
        ));
        project.song.modulations.clear();
        for route in &mut project.song.instruments[0].modulation_routes {
            route.bypass = route.id != "velocity-tone";
        }
        let original = CompiledProject::compile(&project).expect("compile original routes");
        let tone = original
            .controls
            .index(&crate::model::ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "tone".to_owned(),
            })
            .expect("tone");
        let mut changed = project.clone();
        changed.song.instruments[0]
            .modulation_routes
            .iter_mut()
            .find(|route| route.id == "velocity-tone")
            .expect("velocity route")
            .depth = "0.4".to_owned();
        let changed = CompiledProject::compile(&changed).expect("compile changed route");

        let mut current = original.runtime();
        for _ in 0..64 {
            current
                .process_frame(ProcessingMode::Offline)
                .expect("warm active voice");
        }
        let before = active_voice_control(&current, tone);
        let mut next = changed.runtime();
        current.prepare_transition(&mut next);
        assert!(
            current
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead-track")
                .expect("lead Track")
                .transition_unchanged,
            "route depth is a compatible scalar edit"
        );
        assert!(active_voice_target_changed(&current, tone));
        current
            .process_transition_frame(&mut next, 1.0, 0.0)
            .expect("first route handoff frame");
        let first = active_voice_control(&current, tone);
        assert!(first > before);
        assert!(
            first - before < 0.01,
            "route target must ramp instead of stepping: {before} -> {first}"
        );
        for _ in 1..480 {
            current
                .process_transition_frame(&mut next, 1.0, 0.0)
                .expect("route handoff frame");
        }
        assert!(active_voice_control(&current, tone) > before + 0.2);
        current.complete_transition(&mut next);
        assert!(
            next.tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead-track")
                .expect("lead Track after handoff")
                .active_voice_count()
                > 0
        );

        let mut bypass_project = project.clone();
        bypass_project.song.instruments[0]
            .modulation_routes
            .iter_mut()
            .find(|route| route.id == "velocity-tone")
            .expect("velocity route")
            .bypass = true;
        let bypass_changed =
            CompiledProject::compile(&bypass_project).expect("compile bypass edit");
        let mut current = original.runtime();
        for _ in 0..64 {
            current
                .process_frame(ProcessingMode::Offline)
                .expect("warm bypass voice");
        }
        let mut next = bypass_changed.runtime();
        current.prepare_transition(&mut next);
        assert!(
            current
                .tracks
                .iter()
                .all(|track| track.transition_unchanged)
        );
        assert!(active_voice_target_changed(&current, tone));

        let mut topology_project = project.clone();
        topology_project.song.instruments[0]
            .modulation_routes
            .iter_mut()
            .find(|route| route.id == "velocity-tone")
            .expect("velocity route")
            .source = "key-position".to_owned();
        let topology_changed =
            CompiledProject::compile(&topology_project).expect("compile route topology edit");
        let mut current = original.runtime();
        let mut next = topology_changed.runtime();
        current.prepare_transition(&mut next);
        assert!(
            !current
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead-track")
                .expect("lead Track")
                .transition_unchanged,
            "route source topology changes must restart the affected Track"
        );

        let mut macro_project = project;
        macro_project.song.instruments[0]
            .modulation_sources
            .iter_mut()
            .find(|source| source.id == "macro-source")
            .expect("macro source")
            .scope = crate::model::InstrumentModulationScope::Instrument;
        for route in &mut macro_project.song.instruments[0].modulation_routes {
            route.bypass = route.id != "macro-tone";
        }
        let macro_original =
            CompiledProject::compile(&macro_project).expect("compile macro original");
        let mut macro_changed = macro_project;
        macro_changed.song.instruments[0]
            .parameters
            .iter_mut()
            .find(|parameter| parameter.key == "macro")
            .expect("macro Parameter")
            .value = 0.9;
        let macro_changed = CompiledProject::compile(&macro_changed).expect("compile macro edit");
        let mut current = macro_original.runtime();
        for _ in 0..64 {
            current
                .process_frame(ProcessingMode::Offline)
                .expect("warm macro voice");
        }
        let mut next = macro_changed.runtime();
        current.prepare_transition(&mut next);
        assert!(
            active_voice_target_changed(&current, tone),
            "macro dependency closure must include downstream route targets"
        );
    }

    #[test]
    fn two_tracks_share_global_modulation_but_isolate_instrument_state() {
        fn active_tones(
            runtime: &super::SongRuntime,
            tone: super::control::ControlIndex,
        ) -> Vec<(String, f64)> {
            runtime
                .tracks
                .iter()
                .map(|track| {
                    let super::InstrumentRuntime::Graph { voices, .. } = &track.instrument else {
                        panic!("test Tracks must use the graph Instrument");
                    };
                    let value = voices
                        .iter()
                        .find(|slot| slot.voice.active())
                        .expect("active voice")
                        .voice
                        .effective_control(tone);
                    (track.schedule.track_id.clone(), value)
                })
                .collect()
        }

        fn runtime_tones(
            mut project: crate::model::Project,
            scope: crate::model::InstrumentModulationScope,
        ) -> Vec<(String, f64)> {
            project.song.modulations.clear();
            let instrument = &mut project.song.instruments[0];
            instrument
                .modulation_sources
                .iter_mut()
                .find(|source| source.id == "random")
                .expect("random source")
                .scope = scope;
            for route in &mut instrument.modulation_routes {
                route.bypass = route.id != "random-tone";
            }
            let mut duplicate = project.song.tracks[0].clone();
            duplicate.id = "lead-copy".to_owned();
            duplicate.name = "Lead Copy".to_owned();
            duplicate.clips[0].id = "lead-copy-clip".to_owned();
            project.song.tracks.push(duplicate);

            let compiled = CompiledProject::compile(&project).expect("compile two Tracks");
            let tone = compiled
                .controls
                .index(&crate::model::ParameterTarget::Instrument {
                    instrument: "lead".to_owned(),
                    parameter: "tone".to_owned(),
                })
                .expect("tone");
            let mut runtime = compiled.runtime();
            runtime
                .process_frame(ProcessingMode::Offline)
                .expect("activate both Tracks");
            active_tones(&runtime, tone)
        }

        let project = fixture(include_str!(
            "../../tests/fixtures/instrument_modulation.crump"
        ));
        let instrument = runtime_tones(
            project.clone(),
            crate::model::InstrumentModulationScope::Instrument,
        );
        assert_eq!(instrument.len(), 2);
        assert_ne!(
            instrument[0].1.to_bits(),
            instrument[1].1.to_bits(),
            "Instrument random state must be isolated by durable Track identity"
        );

        let per_voice = runtime_tones(
            project.clone(),
            crate::model::InstrumentModulationScope::PerVoice,
        );
        assert_eq!(per_voice.len(), 2);
        assert_ne!(
            per_voice[0].1.to_bits(),
            per_voice[1].1.to_bits(),
            "per-voice random state must include each Track Event identity"
        );

        let global = runtime_tones(project, crate::model::InstrumentModulationScope::Global);
        assert_eq!(global.len(), 2);
        assert_eq!(
            global[0].1.to_bits(),
            global[1].1.to_bits(),
            "global modulation must be shared across Tracks using one Instrument"
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
