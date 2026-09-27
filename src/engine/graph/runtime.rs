//! Graph-program compilation, per-voice lifecycle, and deterministic execution.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    model::{
        Graph, GraphDomain, Instrument, InstrumentModulationScope, ModulationScope, NodeKind,
        ParameterTarget, Project, SettingValue,
    },
    validate::{MAX_STEREO_SUM_INPUTS, validate_graph_for_compile},
};

use super::super::{
    EngineError,
    control::{
        CompiledControlProgram, CompiledInstrumentSource, CompiledInstrumentSourceKind,
        CompiledPerVoiceRoute, InstrumentSourceContext, mix_modulation_identity,
        stable_modulation_identity,
    },
    sample::CompiledSampleLibrary,
    spectral::CompiledSpectralLibrary,
    wavetable::CompiledWavetableLibrary,
};
#[cfg(test)]
use super::nodes::{FilterState, slew_frequency};
use super::nodes::{
    MAX_RELEASE_SECONDS, NodeCompileContext, NodeInputs, NodeProgram, NodeState, Signal,
    authored_node_heap_bytes, authored_node_work_units,
};

const QUIET_SECONDS: f64 = 0.1;

/// Computes bounded node-equivalent callback work for one voice of an authored graph.
pub(crate) fn authored_graph_work_units(instrument: &Instrument, project: &Project) -> u64 {
    instrument.graph.as_ref().map_or(0, |graph| {
        authored_graph_domain_work_units(instrument, project, graph)
    })
}

/// Computes bounded callback work for one Track-owned shared graph instance.
pub(crate) fn authored_shared_graph_work_units(instrument: &Instrument, project: &Project) -> u64 {
    instrument.shared_graph.as_ref().map_or(0, |graph| {
        authored_graph_domain_work_units(instrument, project, graph)
    })
}

fn authored_graph_domain_work_units(
    instrument: &Instrument,
    project: &Project,
    graph: &Graph,
) -> u64 {
    let node_work = graph.nodes.iter().fold(0_u64, |work, node| {
        let node_work = if node.kind == NodeKind::StereoSum {
            graph
                .connections
                .iter()
                .filter(|connection| {
                    connection.to.node == node.id && connection.to.port == "inputs"
                })
                .count()
                .max(1) as u64
        } else {
            authored_node_work_units(instrument, node)
        };
        work.saturating_add(node_work)
    });
    let sources = instrument
        .modulation_sources
        .iter()
        .map(|source| (source.id.as_str(), source.scope))
        .collect::<BTreeMap<_, _>>();
    let route_work = instrument
        .modulation_routes
        .iter()
        .filter(|route| {
            graph.domain == GraphDomain::Voice
                || sources.get(route.source.as_str()) != Some(&InstrumentModulationScope::PerVoice)
        })
        .fold(0_u64, |work, route| {
            let route_work =
                if sources.get(route.source.as_str()) == Some(&InstrumentModulationScope::Global) {
                    1
                } else {
                    2_u64
                        .saturating_add(route.operators.len() as u64)
                        .saturating_add(u64::from(route.auxiliary.is_some()))
                };
            work.saturating_add(route_work)
        });
    let song_tier_work = instrument.parameters.iter().fold(0_u64, |work, parameter| {
        let target = ParameterTarget::Instrument {
            instrument: instrument.id.clone(),
            parameter: parameter.key.clone(),
        };
        let has_song_tier = project.song.modulations.iter().any(|route| {
            route.target == target
                && !matches!(&route.scope, ModulationScope::PerVoice(owner) if owner == &instrument.id)
        }) || project
            .song
            .envelope_followers
            .iter()
            .any(|route| route.target == target);
        work.saturating_add(u64::from(has_song_tier))
    });
    let song_per_voice_work = if graph.domain == GraphDomain::Voice {
        {
            project
                .song
                .modulations
                .iter()
                .filter(|route| {
                    matches!(
                        (&route.target, &route.scope),
                        (
                            ParameterTarget::Instrument { instrument: target, .. },
                            ModulationScope::PerVoice(owner)
                        ) if target == &instrument.id && owner == &instrument.id
                    )
                })
                .fold(0_u64, |work, route| {
                    work.saturating_add(2)
                        .saturating_add(route.operators.len() as u64)
                })
        }
    } else {
        0
    };
    node_work
        .saturating_add(route_work)
        .saturating_add(song_tier_work)
        .saturating_add(song_per_voice_work)
}

/// Computes the mutable allocation made for one voice of an authored graph.
pub(crate) fn authored_graph_voice_state_bytes(
    instrument: &Instrument,
    sample_rate: u32,
    control_targets: usize,
) -> u64 {
    instrument.graph.as_ref().map_or(0, |graph| {
        authored_graph_state_bytes(
            instrument,
            graph,
            sample_rate,
            control_targets,
            std::mem::size_of::<GraphVoice>() as u64,
            5,
        )
    })
}

/// Computes one Track-owned shared graph runtime allocation.
pub(crate) fn authored_shared_graph_state_bytes(
    instrument: &Instrument,
    sample_rate: u32,
    control_targets: usize,
) -> u64 {
    instrument.shared_graph.as_ref().map_or(0, |graph| {
        authored_graph_state_bytes(instrument, graph, sample_rate, control_targets, 0, 4)
    })
}

fn authored_graph_state_bytes(
    instrument: &Instrument,
    graph: &Graph,
    sample_rate: u32,
    control_targets: usize,
    fixed_bytes: u64,
    control_arrays: u64,
) -> u64 {
    let node_count = graph.nodes.len() as u64;
    let fixed_and_vectors = fixed_bytes
        .saturating_add(node_count.saturating_mul(std::mem::size_of::<NodeState>() as u64))
        .saturating_add(node_count.saturating_mul(std::mem::size_of::<Signal>() as u64))
        .saturating_add(
            (control_targets as u64)
                .saturating_mul(control_arrays)
                .saturating_mul(std::mem::size_of::<f64>() as u64),
        )
        .saturating_add(
            (control_targets.div_ceil(usize::BITS as usize) as u64)
                .saturating_mul(std::mem::size_of::<usize>() as u64),
        );
    graph.nodes.iter().fold(fixed_and_vectors, |bytes, node| {
        bytes.saturating_add(authored_node_heap_bytes(instrument, node, sample_rate))
    })
}

/// Absolute identities required by one graph-processing frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GraphProcessIdentity {
    pub(crate) song_frame: u64,
    pub(crate) instrument_identity: u64,
    voice_sum: [f32; 2],
}

impl GraphProcessIdentity {
    pub(crate) const fn new(song_frame: u64, instrument_identity: u64) -> Self {
        Self {
            song_frame,
            instrument_identity,
            voice_sum: [0.0, 0.0],
        }
    }

    pub(crate) const fn shared(
        song_frame: u64,
        instrument_identity: u64,
        voice_sum: [f32; 2],
    ) -> Self {
        Self {
            song_frame,
            instrument_identity,
            voice_sum,
        }
    }
}

/// One preallocated post-voice graph state owned by a Track runtime instance.
#[derive(Debug, Clone)]
pub(crate) struct SharedGraphRuntime {
    states: Vec<NodeState>,
    signals: Vec<Signal>,
    effective_controls: Vec<f64>,
    route_controls: Vec<f64>,
    previous_controls: Vec<f64>,
    transition_origin: Vec<f64>,
    transition_changed: Vec<bool>,
    frames_processed: u64,
    transition_position: u64,
    transition_frames: u64,
}

/// Immutable graph program shared by preallocated voices.
#[derive(Debug, Clone)]
pub(crate) struct CompiledGraph {
    nodes: Vec<CompiledNode>,
    output_index: usize,
    sample_rate: f64,
    has_envelope: bool,
    release_limit_seconds: f64,
    per_voice_routes: Vec<CompiledPerVoiceRoute>,
    route_graph_sources: Vec<(Option<usize>, Option<usize>)>,
    route_global_values: Vec<Option<usize>>,
    operations: Vec<VoiceOperation>,
    control_count: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct GraphAssetLibraries<'a> {
    samples: Option<&'a CompiledSampleLibrary>,
    spectral: Option<&'a CompiledSpectralLibrary>,
    wavetables: Option<&'a CompiledWavetableLibrary>,
}

impl CompiledGraph {
    /// Compiles a graph after whole-Project validation.
    pub(crate) fn compile(
        instrument: &Instrument,
        project: &Project,
        sample_rate: u32,
        controls: &CompiledControlProgram,
    ) -> Result<Self, EngineError> {
        let graph = instrument.graph.as_ref().ok_or_else(|| {
            EngineError::Compile(format!("instrument `{}` has no graph", instrument.id))
        })?;
        if graph.domain != GraphDomain::Voice {
            return Err(EngineError::Compile(
                "the primary Instrument graph must use the voice domain".to_owned(),
            ));
        }
        Self::compile_graph(
            instrument,
            project,
            graph,
            sample_rate,
            controls,
            GraphAssetLibraries::default(),
        )
    }

    /// Compiles a voice graph against publication-owned immutable asset derivatives.
    pub(crate) fn compile_with_assets(
        instrument: &Instrument,
        project: &Project,
        sample_rate: u32,
        controls: &CompiledControlProgram,
        samples: &CompiledSampleLibrary,
        spectral: &CompiledSpectralLibrary,
        wavetables: &CompiledWavetableLibrary,
    ) -> Result<Self, EngineError> {
        let graph = instrument.graph.as_ref().ok_or_else(|| {
            EngineError::Compile(format!("instrument `{}` has no graph", instrument.id))
        })?;
        if graph.domain != GraphDomain::Voice {
            return Err(EngineError::Compile(
                "the primary Instrument graph must use the voice domain".to_owned(),
            ));
        }
        Self::compile_graph(
            instrument,
            project,
            graph,
            sample_rate,
            controls,
            GraphAssetLibraries {
                samples: Some(samples),
                spectral: Some(spectral),
                wavetables: Some(wavetables),
            },
        )
    }

    pub(crate) fn compile_shared(
        instrument: &Instrument,
        project: &Project,
        sample_rate: u32,
        controls: &CompiledControlProgram,
    ) -> Result<Option<Self>, EngineError> {
        let Some(graph) = instrument.shared_graph.as_ref() else {
            return Ok(None);
        };
        if graph.domain != GraphDomain::Shared {
            return Err(EngineError::Compile(
                "the shared Instrument graph must use the shared domain".to_owned(),
            ));
        }
        Self::compile_graph(
            instrument,
            project,
            graph,
            sample_rate,
            controls,
            GraphAssetLibraries::default(),
        )
        .map(Some)
    }

    /// Compiles a shared graph against the same immutable publication asset libraries.
    pub(crate) fn compile_shared_with_assets(
        instrument: &Instrument,
        project: &Project,
        sample_rate: u32,
        controls: &CompiledControlProgram,
        samples: &CompiledSampleLibrary,
        spectral: &CompiledSpectralLibrary,
        wavetables: &CompiledWavetableLibrary,
    ) -> Result<Option<Self>, EngineError> {
        let Some(graph) = instrument.shared_graph.as_ref() else {
            return Ok(None);
        };
        if graph.domain != GraphDomain::Shared {
            return Err(EngineError::Compile(
                "the shared Instrument graph must use the shared domain".to_owned(),
            ));
        }
        Self::compile_graph(
            instrument,
            project,
            graph,
            sample_rate,
            controls,
            GraphAssetLibraries {
                samples: Some(samples),
                spectral: Some(spectral),
                wavetables: Some(wavetables),
            },
        )
        .map(Some)
    }

    fn compile_graph(
        instrument: &Instrument,
        project: &Project,
        graph: &Graph,
        sample_rate: u32,
        controls: &CompiledControlProgram,
        assets: GraphAssetLibraries<'_>,
    ) -> Result<Self, EngineError> {
        let diagnostics = validate_graph_for_compile(instrument, graph);
        if let Some(error) = diagnostics
            .as_slice()
            .iter()
            .find(|diagnostic| diagnostic.severity == crate::diagnostic::Severity::Error)
        {
            return Err(EngineError::Compile(format!(
                "graph validation failed: {}",
                error.message
            )));
        }
        validate_runtime_graph(graph)?;
        let order = topological_order(graph)?;
        let ordered_indexes: BTreeMap<_, _> = order
            .iter()
            .enumerate()
            .map(|(index, id)| (id.as_str(), index))
            .collect();
        let authored_nodes: BTreeMap<_, _> = graph
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect();
        let parameters: BTreeMap<_, _> = instrument
            .parameters
            .iter()
            .map(|parameter| (parameter.key.as_str(), parameter))
            .collect();
        let mut nodes = Vec::with_capacity(order.len());
        let mut output_index = None;
        let mut has_envelope = false;
        for id in &order {
            let authored = authored_nodes
                .get(id.as_str())
                .ok_or_else(|| EngineError::Compile(format!("missing graph node `{id}`")))?;
            let mut inputs: BTreeMap<String, Vec<SignalSource>> = BTreeMap::new();
            let mut authored_inputs = graph
                .connections
                .iter()
                .filter(|connection| connection.to.node == authored.id)
                .collect::<Vec<_>>();
            authored_inputs.sort_by(|left, right| left.id.cmp(&right.id));
            for connection in authored_inputs {
                let source = if connection.from.node == "note" {
                    match connection.from.port.as_str() {
                        "pitch" => SignalSource::Pitch,
                        "gate" => SignalSource::Gate,
                        "velocity" => SignalSource::Velocity,
                        port => {
                            return Err(EngineError::Compile(format!(
                                "unknown note source `{port}`"
                            )));
                        }
                    }
                } else if connection.from.node == "voice_sum" {
                    if connection.from.port != "audio" {
                        return Err(EngineError::Compile(format!(
                            "unknown voice_sum source `{}`",
                            connection.from.port
                        )));
                    }
                    SignalSource::VoiceSum
                } else {
                    SignalSource::Node(
                        *ordered_indexes
                            .get(connection.from.node.as_str())
                            .ok_or_else(|| {
                                EngineError::Compile(format!(
                                    "connection source `{}` was not compiled",
                                    connection.from.node
                                ))
                            })?,
                    )
                };
                inputs
                    .entry(connection.to.port.clone())
                    .or_default()
                    .push(source);
            }
            let program = NodeProgram::compile(
                authored,
                &parameters,
                &instrument.id,
                NodeCompileContext {
                    sample_rate,
                    tempo_bpm: project.song.tempo_bpm,
                    meter: project.song.meter,
                    // Asset declarations and compiled exact-byte derivatives are
                    // compile-time inputs only; neither lookup reaches the callback.
                    project,
                    samples: assets.samples,
                    spectral: assets.spectral,
                    wavetables: assets.wavetables,
                },
                controls,
            )?;
            has_envelope |= matches!(authored.kind, NodeKind::Envelope | NodeKind::Exciter);
            if authored.kind == NodeKind::Output {
                output_index = Some(nodes.len());
            }
            nodes.push(CompiledNode {
                transition_identity: stable_modulation_identity(&authored.id),
                has_key: inputs.contains_key("key"),
                has_velocity: inputs.contains_key("velocity"),
                has_gain: inputs.contains_key("gain"),
                has_modulation: inputs.contains_key("mod"),
                has_frequency_modulation: inputs.contains_key("fm"),
                has_phase_modulation: inputs.contains_key("pm"),
                has_sync: inputs.contains_key("sync"),
                has_amplitude_modulation: inputs.contains_key("am"),
                inputs,
                program,
            });
        }
        let per_voice_routes = controls.graph_routes(instrument, project, graph.domain)?;
        let route_graph_sources = per_voice_routes
            .iter()
            .map(|route| {
                let primary = route
                    .instrument()
                    .and_then(|route| graph_source_index(route.source(), &ordered_indexes));
                let auxiliary = route
                    .instrument()
                    .and_then(|route| route.auxiliary())
                    .and_then(|source| graph_source_index(source, &ordered_indexes));
                (primary, auxiliary)
            })
            .collect::<Vec<_>>();
        let route_global_values = per_voice_routes
            .iter()
            .map(|route| {
                route
                    .instrument()
                    .and_then(|route| controls.global_instrument_route_index(route))
            })
            .collect::<Vec<_>>();
        let operations = compile_voice_operations(
            graph,
            &ordered_indexes,
            &per_voice_routes,
            &route_graph_sources,
            instrument,
            controls,
        )?;
        Ok(Self {
            nodes,
            output_index: output_index
                .ok_or_else(|| EngineError::Compile("graph has no output".to_owned()))?,
            sample_rate: f64::from(sample_rate),
            has_envelope,
            release_limit_seconds: if graph.nodes.iter().any(|node| {
                node.kind == NodeKind::ModalBank
                    || (matches!(node.kind, NodeKind::SampleSource | NodeKind::Multisample)
                        && node.settings.iter().any(|setting| {
                            setting.key == "playback"
                                && matches!(&setting.value, SettingValue::Text(value) if value == "one_shot")
                        }))
            }) {
                30.0
            } else {
                MAX_RELEASE_SECONDS
            },
            per_voice_routes,
            route_graph_sources,
            route_global_values,
            operations,
            control_count: controls.target_count(),
        })
    }

    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn transition_compatible(&self, replacement: &Self) -> bool {
        self.control_count == replacement.control_count
            && self.sample_rate.to_bits() == replacement.sample_rate.to_bits()
            && self.operations == replacement.operations
            && self.nodes.len() == replacement.nodes.len()
            && self
                .nodes
                .iter()
                .zip(replacement.nodes.iter())
                .all(|(current, replacement)| current.transition_compatible(replacement))
            && self.route_graph_sources == replacement.route_graph_sources
            && self.per_voice_routes.len() == replacement.per_voice_routes.len()
            && self
                .per_voice_routes
                .iter()
                .zip(replacement.per_voice_routes.iter())
                .all(|(current, replacement)| current.transition_target(replacement).is_ok())
    }

    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn compatible_transition_targets(
        &self,
        replacement: &Self,
        upstream_changed: &[bool],
        changed: &mut [bool],
    ) -> bool {
        if !self.transition_compatible(replacement)
            || upstream_changed.len() != self.control_count
            || changed.len() != self.control_count
        {
            return false;
        }
        changed.copy_from_slice(upstream_changed);
        for (current, replacement) in self
            .per_voice_routes
            .iter()
            .zip(replacement.per_voice_routes.iter())
        {
            let Ok(target) = current.transition_target(replacement) else {
                return false;
            };
            if let Some(target) = target {
                changed[target.0] = true;
            }
        }
        loop {
            let mut propagated = false;
            for route in &replacement.per_voice_routes {
                if route
                    .parameter_dependencies()
                    .any(|parameter| changed[parameter.0])
                {
                    let target = route.target();
                    if !changed[target.0] {
                        changed[target.0] = true;
                        propagated = true;
                    }
                }
            }
            if !propagated {
                break;
            }
        }
        true
    }

    pub(crate) const fn control_count(&self) -> usize {
        self.control_count
    }

    /// Owned immutable allocation retained by one Arc-shared graph program.
    pub(crate) fn allocation_bytes(&self) -> u64 {
        let word = std::mem::size_of::<usize>() as u64;
        let nodes =
            (self.nodes.capacity() as u64)
                .saturating_mul(std::mem::size_of::<CompiledNode>() as u64)
                .saturating_add(
                    self.nodes
                        .iter()
                        .map(|node| {
                            node.inputs
                                .iter()
                                .fold(0_u64, |bytes, (port, sources)| {
                                    bytes
                                        .saturating_add(port.capacity() as u64)
                                        .saturating_add(word.saturating_mul(4))
                                        .saturating_add((sources.capacity() as u64).saturating_mul(
                                            std::mem::size_of::<SignalSource>() as u64,
                                        ))
                                })
                                .saturating_add(node.program.heap_bytes())
                        })
                        .fold(0_u64, u64::saturating_add),
                );
        let routes =
            (self.per_voice_routes.capacity() as u64)
                .saturating_mul(std::mem::size_of::<CompiledPerVoiceRoute>() as u64)
                .saturating_add(
                    self.per_voice_routes
                        .iter()
                        .map(CompiledPerVoiceRoute::allocation_bytes)
                        .fold(0_u64, u64::saturating_add),
                )
                .saturating_add(
                    (self.route_graph_sources.capacity() as u64).saturating_mul(
                        std::mem::size_of::<(Option<usize>, Option<usize>)>() as u64,
                    ),
                )
                .saturating_add(
                    (self.route_global_values.capacity() as u64)
                        .saturating_mul(std::mem::size_of::<Option<usize>>() as u64),
                )
                .saturating_add(
                    (self.operations.capacity() as u64)
                        .saturating_mul(std::mem::size_of::<VoiceOperation>() as u64),
                );
        (std::mem::size_of::<Self>() as u64)
            .saturating_add(word.saturating_mul(2))
            .saturating_add(nodes)
            .saturating_add(routes)
    }

    /// Allocates all voice state away from the audio callback.
    pub(crate) fn create_voice(&self, slot_seed: u64) -> GraphVoice {
        let states = self.create_node_states(slot_seed);
        GraphVoice {
            states,
            signals: vec![Signal::None; self.nodes.len()],
            effective_controls: vec![0.0; self.control_count],
            route_controls: vec![0.0; self.control_count],
            instrument_controls: vec![0.0; self.control_count],
            previous_controls: vec![0.0; self.control_count],
            transition_origin: vec![0.0; self.control_count],
            transition_changed: vec![false; self.control_count],
            pitch_hz: 440.0,
            pitch_log2: 440.0_f32.log2(),
            glide_target_log2: 440.0_f32.log2(),
            glide_step_log2: 0.0,
            glide_frames_remaining: 0,
            pitch: 69,
            velocity: 0.0,
            gate: false,
            note_id: 0,
            active: false,
            releasing: false,
            age_frames: 0,
            modulation_age_frames: 0,
            quiet_frames: 0,
            seed: slot_seed,
            transition_position: 0,
            transition_frames: 0,
        }
    }

    pub(crate) fn create_shared_runtime(&self, track_seed: u64) -> SharedGraphRuntime {
        SharedGraphRuntime {
            states: self.create_node_states(track_seed),
            signals: vec![Signal::None; self.nodes.len()],
            effective_controls: vec![0.0; self.control_count],
            route_controls: vec![0.0; self.control_count],
            previous_controls: vec![0.0; self.control_count],
            transition_origin: vec![0.0; self.control_count],
            transition_changed: vec![false; self.control_count],
            frames_processed: 0,
            transition_position: 0,
            transition_frames: 0,
        }
    }

    fn create_node_states(&self, seed: u64) -> Vec<NodeState> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                node.program
                    .create_state(self.sample_rate as u32, seed, index as u64)
            })
            .collect()
    }

    /// Processes one sample for one active voice.
    pub(crate) fn process(
        &self,
        voice: &mut GraphVoice,
        control_values: &[f64],
        pre_instrument_unconstrained_values: &[f64],
        song_modulation: &[f64],
        global_instrument_route_values: &[crate::engine::control::RuntimeInstrumentRouteValue],
        identity: GraphProcessIdentity,
    ) -> [f32; 2] {
        if !voice.active {
            return [0.0, 0.0];
        }
        voice.advance_glide();
        let transition_progress = if voice.transition_position < voice.transition_frames {
            voice.transition_position = voice.transition_position.saturating_add(1);
            Some(voice.transition_position as f64 / voice.transition_frames as f64)
        } else {
            None
        };
        voice
            .previous_controls
            .copy_from_slice(&voice.effective_controls);
        voice.effective_controls.copy_from_slice(control_values);
        voice
            .route_controls
            .copy_from_slice(pre_instrument_unconstrained_values);
        voice
            .instrument_controls
            .copy_from_slice(pre_instrument_unconstrained_values);
        for operation in &self.operations {
            let VoiceOperation::Node(index) = *operation else {
                let VoiceOperation::Route(route_index) = *operation else {
                    unreachable!();
                };
                let route = &self.per_voice_routes[route_index];
                let target = route.target();
                let previous = if voice.modulation_age_frames == 0 {
                    control_values[target.0]
                } else {
                    voice.previous_controls[target.0]
                };
                let contribution = if let Some(instrument_route) = route.instrument() {
                    if let Some(global_value) = self.route_global_values[route_index] {
                        global_instrument_route_values[global_value].contribution()
                    } else {
                        let (primary_node, auxiliary_node) = self.route_graph_sources[route_index];
                        let primary_graph =
                            primary_node.map(|node| voice.signals[node].control() as f64);
                        let auxiliary_graph =
                            auxiliary_node.map(|node| voice.signals[node].control() as f64);
                        let primary = instrument_source_value(
                            instrument_route.source(),
                            voice,
                            &voice.route_controls,
                            primary_graph,
                            identity.song_frame,
                            identity.instrument_identity,
                        );
                        let auxiliary = instrument_route.auxiliary().and_then(|source| {
                            instrument_source_value(
                                source,
                                voice,
                                &voice.route_controls,
                                auxiliary_graph,
                                identity.song_frame,
                                identity.instrument_identity,
                            )
                        });
                        primary.map_or(0.0, |primary| {
                            instrument_route.contribution(primary, auxiliary)
                        })
                    }
                } else {
                    route.contribution_at(voice.modulation_age_frames, song_modulation)
                };
                voice.route_controls[target.0] += contribution;
                if route.instrument().is_some_and(|route| {
                    route.source().scope() != InstrumentModulationScope::PerVoice
                }) {
                    voice.instrument_controls[target.0] += contribution;
                }
                let constrained = route.constrain(voice.route_controls[target.0], previous);
                voice.effective_controls[target.0] = if let Some(progress) = transition_progress
                    && voice.transition_changed[target.0]
                {
                    voice.transition_origin[target.0]
                        + (constrained - voice.transition_origin[target.0]) * progress
                } else {
                    constrained
                };
                continue;
            };
            process_compiled_node(
                &self.nodes[index],
                index,
                &mut voice.states,
                &mut voice.signals,
                &voice.effective_controls,
                self.sample_rate,
                VoiceNodeContext {
                    song_frame: identity.song_frame,
                    pitch_hz: voice.pitch_hz,
                    gate: voice.gate,
                    velocity: voice.velocity,
                    voice_sum: [0.0, 0.0],
                },
            );
        }
        voice.age_frames = voice.age_frames.saturating_add(1);
        voice.modulation_age_frames = voice.modulation_age_frames.saturating_add(1);
        let mut output = voice.signals[self.output_index].stereo();
        if voice.releasing {
            let amplitude = output[0].abs().max(output[1].abs());
            if amplitude < 0.000_01 {
                voice.quiet_frames = voice.quiet_frames.saturating_add(1);
            } else {
                voice.quiet_frames = 0;
            }
            let quiet_limit = (self.sample_rate * QUIET_SECONDS) as u64;
            let release_limit = (self.sample_rate * self.release_limit_seconds) as u64;
            // Acoustic bodies and one-shot samples ring independently of note-off.
            // Their bounded release budget ends with a fade; gated graphs retain
            // the envelope release limit.
            if self.release_limit_seconds > MAX_RELEASE_SECONDS {
                let fade_frames = (self.sample_rate * 0.05) as u64;
                let gain = (release_limit.saturating_sub(voice.age_frames) as f32
                    / fade_frames as f32)
                    .min(1.0);
                output[0] *= gain;
                output[1] *= gain;
            }
            let envelopes_idle =
                !self.has_envelope || voice.states.iter().all(NodeState::envelope_idle);
            if (envelopes_idle && voice.quiet_frames >= quiet_limit)
                || voice.age_frames >= release_limit
            {
                voice.active = false;
            }
        }
        output
    }

    /// Processes one Instrument-owned post-voice graph frame for one Track instance.
    pub(crate) fn process_shared(
        &self,
        runtime: &mut SharedGraphRuntime,
        control_values: &[f64],
        pre_instrument_unconstrained_values: &[f64],
        song_modulation: &[f64],
        global_instrument_route_values: &[crate::engine::control::RuntimeInstrumentRouteValue],
        identity: GraphProcessIdentity,
    ) -> [f32; 2] {
        let transition_progress = if runtime.transition_position < runtime.transition_frames {
            runtime.transition_position = runtime.transition_position.saturating_add(1);
            Some(runtime.transition_position as f64 / runtime.transition_frames as f64)
        } else {
            None
        };
        runtime
            .previous_controls
            .copy_from_slice(&runtime.effective_controls);
        runtime.effective_controls.copy_from_slice(control_values);
        runtime
            .route_controls
            .copy_from_slice(pre_instrument_unconstrained_values);

        for operation in &self.operations {
            match *operation {
                VoiceOperation::Route(route_index) => {
                    let route = &self.per_voice_routes[route_index];
                    let target = route.target();
                    let previous = if runtime.frames_processed == 0 {
                        control_values[target.0]
                    } else {
                        runtime.previous_controls[target.0]
                    };
                    let contribution = if let Some(instrument_route) = route.instrument() {
                        if let Some(global_value) = self.route_global_values[route_index] {
                            global_instrument_route_values[global_value].contribution()
                        } else {
                            let primary = shared_instrument_source_value(
                                instrument_route.source(),
                                &runtime.route_controls,
                                identity.song_frame,
                                identity.instrument_identity,
                            );
                            let auxiliary = instrument_route.auxiliary().and_then(|source| {
                                shared_instrument_source_value(
                                    source,
                                    &runtime.route_controls,
                                    identity.song_frame,
                                    identity.instrument_identity,
                                )
                            });
                            primary.map_or(0.0, |primary| {
                                instrument_route.contribution(primary, auxiliary)
                            })
                        }
                    } else {
                        route.contribution_at(identity.song_frame, song_modulation)
                    };
                    runtime.route_controls[target.0] += contribution;
                    let constrained = route.constrain(runtime.route_controls[target.0], previous);
                    runtime.effective_controls[target.0] = if let Some(progress) =
                        transition_progress
                        && runtime.transition_changed[target.0]
                    {
                        runtime.transition_origin[target.0]
                            + (constrained - runtime.transition_origin[target.0]) * progress
                    } else {
                        constrained
                    };
                }
                VoiceOperation::Node(index) => process_compiled_node(
                    &self.nodes[index],
                    index,
                    &mut runtime.states,
                    &mut runtime.signals,
                    &runtime.effective_controls,
                    self.sample_rate,
                    VoiceNodeContext {
                        song_frame: identity.song_frame,
                        pitch_hz: 440.0,
                        gate: false,
                        velocity: 0.0,
                        voice_sum: identity.voice_sum,
                    },
                ),
            }
        }
        runtime.frames_processed = runtime.frames_processed.saturating_add(1);
        runtime.signals[self.output_index].stereo()
    }
}

/// Preallocated mutable graph voice.
#[derive(Debug, Clone)]
pub(crate) struct GraphVoice {
    states: Vec<NodeState>,
    signals: Vec<Signal>,
    effective_controls: Vec<f64>,
    route_controls: Vec<f64>,
    instrument_controls: Vec<f64>,
    previous_controls: Vec<f64>,
    transition_origin: Vec<f64>,
    transition_changed: Vec<bool>,
    pitch_hz: f32,
    pitch_log2: f32,
    glide_target_log2: f32,
    glide_step_log2: f32,
    glide_frames_remaining: u64,
    pitch: u8,
    velocity: f32,
    gate: bool,
    note_id: u64,
    active: bool,
    releasing: bool,
    age_frames: u64,
    modulation_age_frames: u64,
    quiet_frames: u64,
    seed: u64,
    transition_position: u64,
    transition_frames: u64,
}

impl GraphVoice {
    #[cfg(test)]
    pub(crate) fn allocation_bytes(&self) -> u64 {
        let fixed = std::mem::size_of::<Self>() as u64
            + self.states.capacity() as u64 * std::mem::size_of::<NodeState>() as u64
            + self.signals.capacity() as u64 * std::mem::size_of::<Signal>() as u64
            + (self.effective_controls.capacity()
                + self.route_controls.capacity()
                + self.instrument_controls.capacity()
                + self.previous_controls.capacity()
                + self.transition_origin.capacity()) as u64
                * std::mem::size_of::<f64>() as u64
            + packed_bool_allocation_bytes(self.transition_changed.capacity());
        self.states
            .iter()
            .map(NodeState::heap_bytes)
            .fold(fixed, u64::saturating_add)
    }

    pub(crate) fn activate(&mut self, note_id: u64, pitch: u8, velocity: f32, seed: u64) {
        self.note_id = note_id;
        self.set_pitch_immediately(pitch);
        self.pitch = pitch;
        self.velocity = velocity.clamp(0.0, 1.0);
        self.gate = true;
        self.active = true;
        self.releasing = false;
        self.age_frames = 0;
        self.modulation_age_frames = 0;
        self.quiet_frames = 0;
        self.seed = seed;
        self.transition_position = self.transition_frames;
        for state in &mut self.states {
            state.retrigger(seed);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retarget_legato(
        &mut self,
        note_id: u64,
        pitch: u8,
        velocity: f32,
        seed: u64,
        glide_start_hz: Option<f32>,
        glide_seconds: f64,
        sample_rate: u32,
    ) {
        self.note_id = note_id;
        self.pitch = pitch;
        self.velocity = velocity.clamp(0.0, 1.0);
        self.seed = seed;
        self.gate = true;
        self.active = true;
        self.releasing = false;
        self.quiet_frames = 0;
        for state in &mut self.states {
            state.retarget_note(seed);
        }
        self.configure_glide(glide_start_hz, pitch, glide_seconds, sample_rate);
    }

    pub(crate) fn configure_glide(
        &mut self,
        start_hz: Option<f32>,
        target_pitch: u8,
        seconds: f64,
        sample_rate: u32,
    ) {
        let target_hz = midi_frequency(target_pitch);
        let frames = (seconds.max(0.0) * f64::from(sample_rate)).round() as u64;
        let Some(start_hz) = start_hz.filter(|value| value.is_finite() && *value > 0.0) else {
            self.set_pitch_immediately(target_pitch);
            return;
        };
        if frames == 0 {
            self.set_pitch_immediately(target_pitch);
            return;
        }
        self.pitch_hz = start_hz;
        self.pitch_log2 = start_hz.log2();
        self.glide_target_log2 = target_hz.log2();
        self.glide_step_log2 = (self.glide_target_log2 - self.pitch_log2) / frames as f32;
        self.glide_frames_remaining = frames;
    }

    fn set_pitch_immediately(&mut self, pitch: u8) {
        self.pitch_hz = midi_frequency(pitch);
        self.pitch_log2 = self.pitch_hz.log2();
        self.glide_target_log2 = self.pitch_log2;
        self.glide_step_log2 = 0.0;
        self.glide_frames_remaining = 0;
    }

    fn advance_glide(&mut self) {
        if self.glide_frames_remaining == 0 {
            return;
        }
        self.glide_frames_remaining -= 1;
        if self.glide_frames_remaining == 0 {
            self.pitch_log2 = self.glide_target_log2;
        } else {
            self.pitch_log2 += self.glide_step_log2;
        }
        self.pitch_hz = self.pitch_log2.exp2();
    }

    pub(crate) const fn pitch_hz(&self) -> f32 {
        self.pitch_hz
    }

    pub(crate) const fn gate_active(&self) -> bool {
        self.active && self.gate
    }

    #[cfg(test)]
    pub(crate) const fn test_note_id(&self) -> u64 {
        self.note_id
    }

    #[cfg(test)]
    pub(crate) const fn test_modulation_age_frames(&self) -> u64 {
        self.modulation_age_frames
    }

    #[cfg(test)]
    pub(crate) fn test_set_modulation_age_frames(&mut self, frames: u64) {
        self.modulation_age_frames = frames;
    }

    #[cfg(test)]
    pub(crate) const fn test_seed(&self) -> u64 {
        self.seed
    }

    #[cfg(test)]
    pub(crate) const fn test_glide_frames_remaining(&self) -> u64 {
        self.glide_frames_remaining
    }

    #[cfg(test)]
    pub(crate) fn test_advance_glide(&mut self) {
        self.advance_glide();
    }

    #[cfg(test)]
    pub(crate) fn test_set_age_frames(&mut self, frames: u64) {
        self.age_frames = frames;
    }

    pub(crate) fn release(&mut self, note_id: u64) {
        if self.active && self.note_id == note_id {
            self.release_all();
        }
    }

    pub(crate) fn release_all(&mut self) {
        if self.active {
            self.gate = false;
            self.releasing = true;
            self.age_frames = 0;
            self.quiet_frames = 0;
        }
    }

    pub(crate) const fn active(&self) -> bool {
        self.active
    }

    pub(crate) const fn releasing(&self) -> bool {
        self.releasing
    }

    pub(crate) const fn age_frames(&self) -> u64 {
        self.age_frames
    }

    #[cfg(test)]
    pub(crate) fn effective_control(&self, index: super::super::control::ControlIndex) -> f64 {
        self.effective_controls[index.0]
    }

    #[cfg(test)]
    pub(crate) fn transition_target_changed(
        &self,
        index: super::super::control::ControlIndex,
    ) -> bool {
        self.transition_changed[index.0]
    }

    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn begin_route_transition(&mut self, changed: &[bool], frames: u64) {
        self.transition_origin
            .copy_from_slice(&self.effective_controls);
        self.transition_changed.copy_from_slice(changed);
        self.transition_position = 0;
        self.transition_frames = frames.max(1);
    }

    pub(crate) fn stop(&mut self) {
        self.active = false;
        self.gate = false;
        self.releasing = false;
    }
}

impl SharedGraphRuntime {
    pub(crate) fn reset(&mut self, seed: u64) {
        for state in &mut self.states {
            state.reset(seed);
        }
        self.signals.fill(Signal::None);
        self.effective_controls.fill(0.0);
        self.route_controls.fill(0.0);
        self.previous_controls.fill(0.0);
        self.transition_origin.fill(0.0);
        self.transition_changed.fill(false);
        self.frames_processed = 0;
        self.transition_position = 0;
        self.transition_frames = 0;
    }

    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn begin_route_transition(&mut self, changed: &[bool], frames: u64) {
        self.transition_origin
            .copy_from_slice(&self.effective_controls);
        self.transition_changed.copy_from_slice(changed);
        self.transition_position = 0;
        self.transition_frames = frames.max(1);
    }

    #[cfg(test)]
    pub(crate) fn allocation_bytes(&self) -> u64 {
        let fixed = self.states.capacity() as u64 * std::mem::size_of::<NodeState>() as u64
            + self.signals.capacity() as u64 * std::mem::size_of::<Signal>() as u64
            + (self.effective_controls.capacity()
                + self.route_controls.capacity()
                + self.previous_controls.capacity()
                + self.transition_origin.capacity()) as u64
                * std::mem::size_of::<f64>() as u64
            + packed_bool_allocation_bytes(self.transition_changed.capacity());
        self.states
            .iter()
            .map(NodeState::heap_bytes)
            .fold(fixed, u64::saturating_add)
    }

    #[cfg(test)]
    pub(crate) fn effective_control(&self, index: super::super::control::ControlIndex) -> f64 {
        self.effective_controls[index.0]
    }
}

#[cfg(test)]
fn packed_bool_allocation_bytes(capacity_bits: usize) -> u64 {
    (capacity_bits.div_ceil(usize::BITS as usize) as u64)
        .saturating_mul(std::mem::size_of::<usize>() as u64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VoiceOperation {
    Node(usize),
    Route(usize),
}

fn graph_source_index(
    source: &CompiledInstrumentSource,
    node_indexes: &BTreeMap<&str, usize>,
) -> Option<usize> {
    match source.kind() {
        CompiledInstrumentSourceKind::GraphPort { node, .. } => {
            node_indexes.get(node.as_str()).copied()
        }
        _ => None,
    }
}

fn parameter_source(
    source: &CompiledInstrumentSource,
) -> Option<super::super::control::ControlIndex> {
    match source.kind() {
        CompiledInstrumentSourceKind::Parameter { parameter } => Some(*parameter),
        _ => None,
    }
}

fn shared_instrument_source_value(
    source: &CompiledInstrumentSource,
    controls: &[f64],
    song_frame: u64,
    instrument_identity: u64,
) -> Option<f64> {
    let random_identity = match source.scope() {
        InstrumentModulationScope::Global => 0,
        InstrumentModulationScope::Instrument => instrument_identity,
        InstrumentModulationScope::PerVoice => return None,
    };
    source.value(InstrumentSourceContext {
        frame: song_frame,
        pitch: 0,
        velocity: 0.0,
        gate: false,
        controls,
        graph_control: None,
        random_identity,
    })
}

fn instrument_source_value(
    source: &CompiledInstrumentSource,
    voice: &GraphVoice,
    global_controls: &[f64],
    graph_control: Option<f64>,
    song_frame: u64,
    instrument_identity: u64,
) -> Option<f64> {
    let (frame, controls, random_identity) = match source.scope() {
        InstrumentModulationScope::Global => (song_frame, global_controls, 0),
        InstrumentModulationScope::Instrument => (
            song_frame,
            voice.instrument_controls.as_slice(),
            instrument_identity,
        ),
        InstrumentModulationScope::PerVoice => (
            voice.modulation_age_frames,
            voice.route_controls.as_slice(),
            mix_modulation_identity(voice.seed, voice.note_id),
        ),
    };
    source.value(InstrumentSourceContext {
        frame,
        pitch: voice.pitch,
        velocity: f64::from(voice.velocity),
        gate: voice.gate,
        controls,
        graph_control,
        random_identity,
    })
}

fn compile_voice_operations(
    graph: &Graph,
    node_indexes: &BTreeMap<&str, usize>,
    routes: &[CompiledPerVoiceRoute],
    graph_sources: &[(Option<usize>, Option<usize>)],
    instrument: &Instrument,
    controls: &CompiledControlProgram,
) -> Result<Vec<VoiceOperation>, EngineError> {
    let node_count = graph.nodes.len();
    let operation_count = node_count.saturating_add(routes.len());
    let mut outgoing = vec![Vec::<usize>::new(); operation_count];
    let mut incoming = vec![0_usize; operation_count];
    let mut add_edge = |from: usize, to: usize| {
        if !outgoing[from].contains(&to) {
            outgoing[from].push(to);
            incoming[to] += 1;
        }
    };

    for connection in &graph.connections {
        if !matches!(connection.from.node.as_str(), "note" | "voice_sum") {
            add_edge(
                node_indexes[connection.from.node.as_str()],
                node_indexes[connection.to.node.as_str()],
            );
        }
    }
    for (route_index, route) in routes.iter().enumerate() {
        let operation = node_count + route_index;
        if let Some(node) = graph_sources[route_index].0 {
            add_edge(node, operation);
        }
        if let Some(node) = graph_sources[route_index].1 {
            add_edge(node, operation);
        }
        let target = route.target();
        for node in &graph.nodes {
            if instrument.parameters.iter().any(|parameter| {
                node.references_parameter(&parameter.key)
                    && controls.index(&crate::model::ParameterTarget::Instrument {
                        instrument: instrument.id.clone(),
                        parameter: parameter.key.clone(),
                    }) == Some(target)
            }) {
                add_edge(operation, node_indexes[node.id.as_str()]);
            }
        }
    }
    for (producer_index, producer) in routes.iter().enumerate() {
        for (consumer_index, consumer) in routes.iter().enumerate() {
            if producer_index == consumer_index {
                continue;
            }
            let consumes_target = producer.instrument().is_some()
                && consumer.instrument().is_some_and(|route| {
                    parameter_source(route.source()) == Some(producer.target())
                        || route.auxiliary().and_then(parameter_source) == Some(producer.target())
                });
            if consumes_target {
                add_edge(node_count + producer_index, node_count + consumer_index);
            }
        }
    }
    for left in 0..routes.len() {
        for right in left + 1..routes.len() {
            if routes[left].target() == routes[right].target() {
                add_edge(node_count + left, node_count + right);
            }
        }
    }
    // Ownership is a real tier, not merely a label: every Instrument operation precedes every
    // Song-owned operation. If a Song target feeds a graph Node required by an Instrument route,
    // these edges expose the impossible same-frame dependency as a compile-time cycle.
    for (instrument_index, instrument_route) in routes.iter().enumerate() {
        if instrument_route.instrument().is_none() {
            continue;
        }
        for (song_index, song_route) in routes.iter().enumerate() {
            if song_route.instrument().is_none() {
                add_edge(node_count + instrument_index, node_count + song_index);
            }
        }
    }

    let mut ready = incoming
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<BTreeSet<_>>();
    let mut operations = Vec::with_capacity(operation_count);
    while let Some(operation) = ready.pop_first() {
        operations.push(if operation < node_count {
            VoiceOperation::Node(operation)
        } else {
            VoiceOperation::Route(operation - node_count)
        });
        for destination in &outgoing[operation] {
            incoming[*destination] -= 1;
            if incoming[*destination] == 0 {
                ready.insert(*destination);
            }
        }
    }
    if operations.len() != operation_count {
        return Err(EngineError::Compile(
            "graph Control and Instrument Modulation dependencies contain a cycle".to_owned(),
        ));
    }
    Ok(operations)
}

#[derive(Debug, Clone)]
struct CompiledNode {
    transition_identity: u64,
    inputs: BTreeMap<String, Vec<SignalSource>>,
    program: NodeProgram,
    has_key: bool,
    has_velocity: bool,
    has_gain: bool,
    has_modulation: bool,
    has_frequency_modulation: bool,
    has_phase_modulation: bool,
    has_sync: bool,
    has_amplitude_modulation: bool,
}

impl CompiledNode {
    fn transition_compatible(&self, replacement: &Self) -> bool {
        self.transition_identity == replacement.transition_identity
            && self.inputs == replacement.inputs
            && self.program.transition_compatible(&replacement.program)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignalSource {
    Pitch,
    Gate,
    Velocity,
    VoiceSum,
    Node(usize),
}

#[derive(Debug, Clone, Copy)]
struct VoiceNodeContext {
    song_frame: u64,
    pitch_hz: f32,
    gate: bool,
    velocity: f32,
    voice_sum: [f32; 2],
}

fn process_compiled_node(
    node: &CompiledNode,
    index: usize,
    states: &mut [NodeState],
    signals: &mut [Signal],
    controls: &[f64],
    sample_rate: f64,
    context: VoiceNodeContext,
) {
    // A topological program guarantees every referenced signal was written before this Node.
    let input = |port| {
        resolve_input_values(
            node,
            port,
            signals,
            context.pitch_hz,
            context.gate,
            context.velocity,
            context.voice_sum,
        )
    };
    let inputs = NodeInputs {
        song_frame: context.song_frame,
        pitch: input("pitch"),
        key: input("key"),
        gate: input("gate"),
        velocity: input("velocity"),
        audio: input("audio"),
        gain: input("gain"),
        modulation: input("mod"),
        frequency_modulation: input("fm"),
        phase_modulation: input("pm"),
        sync: input("sync"),
        amplitude_modulation: input("am"),
        input_a: input("a"),
        input_b: input("b"),
        summed_audio: resolve_stereo_sum_values(
            node,
            signals,
            context.pitch_hz,
            context.gate,
            context.velocity,
            context.voice_sum,
        ),
        has_key: node.has_key,
        has_velocity: node.has_velocity,
        has_gain: node.has_gain,
        has_modulation: node.has_modulation,
        has_frequency_modulation: node.has_frequency_modulation,
        has_phase_modulation: node.has_phase_modulation,
        has_sync: node.has_sync,
        has_amplitude_modulation: node.has_amplitude_modulation,
    };
    signals[index] = node
        .program
        .process(&mut states[index], inputs, controls, sample_rate);
}

fn resolve_input_values(
    node: &CompiledNode,
    port: &str,
    signals: &[Signal],
    pitch_hz: f32,
    gate: bool,
    velocity: f32,
    voice_sum: [f32; 2],
) -> Signal {
    node.inputs
        .get(port)
        .and_then(|sources| sources.first())
        .map_or(Signal::None, |source| {
            resolve_source_values(*source, signals, pitch_hz, gate, velocity, voice_sum)
        })
}

#[cfg(test)]
fn resolve_stereo_sum(node: &CompiledNode, voice: &GraphVoice) -> Signal {
    resolve_stereo_sum_values(
        node,
        &voice.signals,
        voice.pitch_hz,
        voice.gate,
        voice.velocity,
        [0.0, 0.0],
    )
}

fn resolve_stereo_sum_values(
    node: &CompiledNode,
    signals: &[Signal],
    pitch_hz: f32,
    gate: bool,
    velocity: f32,
    voice_sum: [f32; 2],
) -> Signal {
    let Some(sources) = node.inputs.get("inputs") else {
        return Signal::None;
    };
    let mut sum = [0.0_f32; 2];
    for source in sources {
        let value =
            resolve_source_values(*source, signals, pitch_hz, gate, velocity, voice_sum).stereo();
        sum[0] += value[0];
        sum[1] += value[1];
    }
    Signal::Stereo(sum)
}

fn resolve_source_values(
    source: SignalSource,
    signals: &[Signal],
    pitch_hz: f32,
    gate: bool,
    velocity: f32,
    voice_sum: [f32; 2],
) -> Signal {
    match source {
        SignalSource::Pitch => Signal::Control(pitch_hz),
        SignalSource::Gate => Signal::Control(f32::from(gate)),
        SignalSource::Velocity => Signal::Control(velocity),
        SignalSource::VoiceSum => Signal::Stereo(voice_sum),
        SignalSource::Node(index) => signals[index],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeSignalType {
    AudioMono,
    AudioStereo,
    Hertz,
    Gate,
    Unipolar,
    Control,
}

fn validate_runtime_graph(graph: &Graph) -> Result<(), EngineError> {
    let nodes: BTreeMap<_, _> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut bus_ids = BTreeSet::new();
    let mut cardinality: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    let mut voice_sum_connections = 0_usize;
    for connection in &graph.connections {
        if !bus_ids.insert(connection.id.as_str()) {
            return Err(EngineError::Compile(format!(
                "duplicate graph Bus id `{}`",
                connection.id
            )));
        }
        let from = if connection.from.node == "note" {
            if graph.domain != GraphDomain::Voice {
                return Err(EngineError::Compile(
                    "shared graph cannot read the per-voice note source".to_owned(),
                ));
            }
            match connection.from.port.as_str() {
                "pitch" => Some(RuntimeSignalType::Hertz),
                "gate" => Some(RuntimeSignalType::Gate),
                "velocity" => Some(RuntimeSignalType::Unipolar),
                _ => None,
            }
        } else if connection.from.node == "voice_sum" {
            if graph.domain != GraphDomain::Shared || connection.from.port != "audio" {
                return Err(EngineError::Compile(
                    "voice_sum.audio is available only in the shared graph".to_owned(),
                ));
            }
            voice_sum_connections = voice_sum_connections.saturating_add(1);
            Some(RuntimeSignalType::AudioStereo)
        } else {
            nodes
                .get(connection.from.node.as_str())
                .and_then(|node| runtime_node_output(node.kind, &connection.from.port))
        }
        .ok_or_else(|| EngineError::Compile(format!("invalid Bus source `{}`", connection.from)))?;
        let target_node = nodes
            .get(connection.to.node.as_str())
            .copied()
            .ok_or_else(|| {
                EngineError::Compile(format!("missing node `{}`", connection.to.node))
            })?;
        let to = runtime_node_input(target_node.kind, &connection.to.port).ok_or_else(|| {
            EngineError::Compile(format!("invalid Bus target `{}`", connection.to))
        })?;
        if !runtime_signal_compatible(from, to) {
            return Err(EngineError::Compile(format!(
                "Bus `{}` requires an explicit signal conversion",
                connection.id
            )));
        }
        let count = cardinality
            .entry((connection.to.node.as_str(), connection.to.port.as_str()))
            .or_default();
        *count += 1;
        let summing_limit = (target_node.kind == NodeKind::StereoSum
            && connection.to.port == "inputs")
            .then(|| match target_node.setting("max_inputs") {
                Some(crate::model::SettingValue::Number(value)) if value.is_finite() => {
                    (*value as usize).clamp(1, MAX_STEREO_SUM_INPUTS)
                }
                _ => MAX_STEREO_SUM_INPUTS,
            });
        if (summing_limit.is_none() && *count > 1)
            || summing_limit.is_some_and(|limit| *count > limit)
        {
            return Err(EngineError::Compile(format!(
                "input {} exceeds its bounded Bus cardinality",
                connection.to
            )));
        }
    }
    if graph.domain == GraphDomain::Shared && voice_sum_connections != 1 {
        return Err(EngineError::Compile(format!(
            "shared graph requires exactly one voice_sum.audio Bus; found {voice_sum_connections}"
        )));
    }
    for node in &graph.nodes {
        for required in runtime_required_inputs(node.kind) {
            if cardinality
                .get(&(node.id.as_str(), *required))
                .copied()
                .unwrap_or(0)
                == 0
            {
                return Err(EngineError::Compile(format!(
                    "node `{}` requires input `{required}`",
                    node.id
                )));
            }
        }
    }
    Ok(())
}

fn runtime_node_output(kind: NodeKind, port: &str) -> Option<RuntimeSignalType> {
    match (kind, port) {
        (NodeKind::Exciter | NodeKind::ModalBank, "audio") => Some(RuntimeSignalType::AudioMono),
        (
            NodeKind::Oscillator
            | NodeKind::Noise
            | NodeKind::Mixer
            | NodeKind::Lowpass
            | NodeKind::Amplifier
            | NodeKind::Saturation
            | NodeKind::StereoToMono,
            "audio",
        ) => Some(RuntimeSignalType::AudioMono),
        (
            NodeKind::Unison
            | NodeKind::AnalogOscillator
            | NodeKind::Wavetable
            | NodeKind::SampleSource
            | NodeKind::Multisample
            | NodeKind::Granular
            | NodeKind::Spectral
            | NodeKind::StereoFilter
            | NodeKind::Pan
            | NodeKind::Delay
            | NodeKind::Reverb
            | NodeKind::MonoToStereo
            | NodeKind::StereoMixer
            | NodeKind::StereoSum,
            "audio",
        ) => Some(RuntimeSignalType::AudioStereo),
        (NodeKind::Envelope, "control") => Some(RuntimeSignalType::Unipolar),
        (NodeKind::Lfo, "control") => Some(RuntimeSignalType::Control),
        _ => None,
    }
}

fn runtime_node_input(kind: NodeKind, port: &str) -> Option<RuntimeSignalType> {
    match (kind, port) {
        (NodeKind::Exciter | NodeKind::ModalBank, "gate") => Some(RuntimeSignalType::Gate),
        (NodeKind::Exciter, "velocity") => Some(RuntimeSignalType::Unipolar),
        (NodeKind::ModalBank, "pitch") => Some(RuntimeSignalType::Hertz),
        (NodeKind::ModalBank, "audio") => Some(RuntimeSignalType::AudioMono),
        (
            NodeKind::Oscillator
            | NodeKind::Unison
            | NodeKind::AnalogOscillator
            | NodeKind::Wavetable
            | NodeKind::SampleSource
            | NodeKind::Multisample
            | NodeKind::Granular
            | NodeKind::Spectral,
            "pitch",
        ) => Some(RuntimeSignalType::Hertz),
        (
            NodeKind::Unison
            | NodeKind::AnalogOscillator
            | NodeKind::Wavetable
            | NodeKind::SampleSource
            | NodeKind::Multisample,
            "gain" | "velocity",
        ) => Some(RuntimeSignalType::Unipolar),
        (NodeKind::Unison, "mod") => Some(RuntimeSignalType::Control),
        (NodeKind::AnalogOscillator, "fm" | "pm" | "sync" | "am")
        | (NodeKind::Wavetable, "fm" | "pm" | "sync") => Some(RuntimeSignalType::AudioMono),
        (
            NodeKind::Envelope
            | NodeKind::SampleSource
            | NodeKind::Multisample
            | NodeKind::Granular,
            "gate",
        ) => Some(RuntimeSignalType::Gate),
        (NodeKind::Mixer, "a" | "b") => Some(RuntimeSignalType::AudioMono),
        (NodeKind::StereoMixer, "a" | "b") => Some(RuntimeSignalType::AudioStereo),
        (NodeKind::StereoSum, "inputs") => Some(RuntimeSignalType::AudioStereo),
        (
            NodeKind::Lowpass
            | NodeKind::Amplifier
            | NodeKind::Saturation
            | NodeKind::Pan
            | NodeKind::MonoToStereo,
            "audio",
        ) => Some(RuntimeSignalType::AudioMono),
        (
            NodeKind::Delay
            | NodeKind::Reverb
            | NodeKind::Output
            | NodeKind::StereoToMono
            | NodeKind::StereoFilter,
            "audio",
        ) => Some(RuntimeSignalType::AudioStereo),
        (NodeKind::Lowpass | NodeKind::StereoFilter, "key") => Some(RuntimeSignalType::Hertz),
        (NodeKind::Lowpass | NodeKind::StereoFilter, "mod") => Some(RuntimeSignalType::Control),
        (NodeKind::Lowpass | NodeKind::StereoFilter, "velocity") => {
            Some(RuntimeSignalType::Unipolar)
        }
        (NodeKind::Amplifier, "gain") => Some(RuntimeSignalType::Unipolar),
        _ => None,
    }
}

const fn runtime_required_inputs(kind: NodeKind) -> &'static [&'static str] {
    match kind {
        NodeKind::Exciter => &["gate", "velocity"],
        NodeKind::ModalBank => &["gate", "pitch", "audio"],
        NodeKind::Oscillator
        | NodeKind::Unison
        | NodeKind::AnalogOscillator
        | NodeKind::Wavetable => &["pitch"],
        NodeKind::SampleSource => &["pitch", "gate"],
        NodeKind::Multisample => &["pitch", "gate", "velocity"],
        NodeKind::Granular => &["pitch", "gate"],
        NodeKind::Spectral => &["pitch"],
        NodeKind::Noise | NodeKind::Lfo => &[],
        NodeKind::Envelope => &["gate"],
        NodeKind::Mixer | NodeKind::StereoMixer => &["a"],
        NodeKind::StereoSum => &["inputs"],
        NodeKind::Lowpass
        | NodeKind::Saturation
        | NodeKind::Pan
        | NodeKind::MonoToStereo
        | NodeKind::StereoToMono => &["audio"],
        NodeKind::Amplifier => &["audio", "gain"],
        NodeKind::Delay | NodeKind::Reverb | NodeKind::Output | NodeKind::StereoFilter => {
            &["audio"]
        }
    }
}

const fn runtime_signal_compatible(from: RuntimeSignalType, to: RuntimeSignalType) -> bool {
    from as u8 == to as u8
        || matches!(
            (from, to),
            (RuntimeSignalType::Unipolar, RuntimeSignalType::Control)
        )
}

fn topological_order(graph: &Graph) -> Result<Vec<String>, EngineError> {
    let mut incoming: BTreeMap<&str, usize> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), 0))
        .collect();
    let mut outgoing: BTreeMap<&str, Vec<&str>> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), Vec::new()))
        .collect();
    for connection in &graph.connections {
        if matches!(connection.from.node.as_str(), "note" | "voice_sum") {
            continue;
        }
        *incoming
            .get_mut(connection.to.node.as_str())
            .ok_or_else(|| {
                EngineError::Compile(format!("missing node `{}`", connection.to.node))
            })? += 1;
        outgoing
            .get_mut(connection.from.node.as_str())
            .ok_or_else(|| {
                EngineError::Compile(format!("missing node `{}`", connection.from.node))
            })?
            .push(connection.to.node.as_str());
    }
    let mut ready: BTreeSet<_> = incoming
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(*id))
        .collect();
    let mut order = Vec::with_capacity(graph.nodes.len());
    while let Some(id) = ready.pop_first() {
        order.push(id.to_owned());
        if let Some(destinations) = outgoing.get(id) {
            for destination in destinations {
                let count = incoming
                    .get_mut(destination)
                    .ok_or_else(|| EngineError::Compile(format!("missing node `{destination}`")))?;
                *count -= 1;
                if *count == 0 {
                    ready.insert(destination);
                }
            }
        }
    }
    if order.len() != graph.nodes.len() {
        return Err(EngineError::Compile("graph contains a cycle".to_owned()));
    }
    Ok(order)
}

fn midi_frequency(pitch: u8) -> f32 {
    (440.0 * 2.0_f64.powf((f64::from(pitch) - 69.0) / 12.0)) as f32
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{dsl::parse_project, validate::validate_project};

    use super::*;

    const GRAPH_SOURCE: &str = r#"crump 1
project "Graph" {
  id graph
  seed 1
  sample_rate 48000
  render_tail 1s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument lead "Lead" graph {
      polyphony 4
      parameter tone "Tone" hz 100hz 10000hz 3000hz 3000hz modulatable
      faceplate {
        control tone "Tone" "Brightness" maps tone
      }
      graph 8 {
        node osc oscillator waveform saw
        node env envelope attack 5ms decay 80ms sustain 0.6 release 200ms
        node filter lowpass cutoff $tone resonance 0.2
        node amp amplifier
        node pan pan position 0
        node out output
        connect note.pitch -> osc.pitch
        connect note.gate -> env.gate
        connect osc.audio -> filter.audio
        connect filter.audio -> amp.audio
        connect env.control -> amp.gain
        connect amp.audio -> pan.audio
        connect pan.audio -> out.audio
      }
    }
    pattern notes 1bar {
      note 0beat C4 1beat 0.8
    }
    track lead "Lead" instrument lead level -6db pan 0 {
      clip one pattern notes start 0bar duration 1bar mode once
    }
    modulation voice-tone lfo per_voice/lead bipolar sine free 1hz target instrument/lead/tone depth 1000hz {
      scale 1
    }
  }
}
"#;

    const EXPRESSIVE_SOURCE: &str = r#"crump 1
project "Expressive graph" {
  id expressive-graph
  seed 77
  sample_rate 48000
  render_tail 1s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument touch "Touch" graph {
      polyphony 2
      parameter cutoff "Cutoff" hz 100hz 10000hz 900hz 900hz modulatable
      parameter env_depth "Envelope" hz -10000hz 10000hz 3200hz 3200hz modulatable
      parameter velocity_depth "Velocity timbre" hz -10000hz 10000hz 1800hz 1800hz modulatable
      faceplate {
        control cutoff "Cutoff" "Base multimode-filter cutoff" maps cutoff
        control env_depth "Envelope" "Timbre-envelope depth" maps env_depth
      }
      graph 12 {
        node osc oscillator waveform saw glide 20ms
        node amp_env envelope attack 5ms decay 80ms sustain 0.7 release 150ms
        node timbre_env envelope attack 2ms decay 120ms sustain 0.2 release 100ms
        node filter filter mode bandpass cutoff $cutoff resonance 0.25 mod_depth $env_depth velocity_depth $velocity_depth key_track 1 key_reference 261.625565hz
        node amp amplifier
        node touch amplifier
        node pan pan position 0
        node out output
        connect note.pitch -> osc.pitch
        connect note.gate -> amp_env.gate
        connect note.gate -> timbre_env.gate
        connect note.pitch -> filter.key
        connect note.velocity -> filter.velocity
        connect timbre_env.control -> filter.mod
        connect osc.audio -> filter.audio
        connect filter.audio -> amp.audio
        connect amp_env.control -> amp.gain
        connect amp.audio -> touch.audio
        connect note.velocity -> touch.gain
        connect touch.audio -> pan.audio
        connect pan.audio -> out.audio
      }
    }
    instrument cloud "Cloud" graph {
      polyphony 2
      parameter width "Width" ratio 0 1 0.8 0.8 modulatable
      parameter motion "Motion" st -1st 1st 0.1st 0.1st modulatable
      faceplate {
        control width "Width" "Mono-compatible unison spread" maps width
        control motion "Motion" "Bounded pitch movement" maps motion
      }
      graph 6 {
        node env envelope attack 10ms decay 100ms sustain 0.8 release 300ms
        node motion lfo waveform triangle rate 0.3hz depth 1
        node cloud unison waveform saw voices 6 detune 0.18st width $width drift 0.03st glide 30ms mod_depth $motion velocity_amount 0.5 level 0.35
        node out output
        connect note.gate -> env.gate
        connect note.pitch -> cloud.pitch
        connect note.velocity -> cloud.velocity
        connect env.control -> cloud.gain
        connect motion.control -> cloud.mod
        connect cloud.audio -> out.audio
      }
    }
    pattern notes 1bar {
      note 0beat C4 1beat 0.25
      note 2beat C5 1beat 0.9
    }
    track touch "Touch" instrument touch level -12db pan 0 {
      clip touch-clip pattern notes start 0bar duration 1bar mode once
    }
    track cloud "Cloud" instrument cloud level -12db pan 0 {
      clip cloud-clip pattern notes start 0bar duration 1bar mode once
    }
    modulation cloud-width lfo global bipolar sine sync 1bar target instrument/cloud/width depth 0.1 {
      scale 1
    }
    modulation cloud-motion lfo per_voice/cloud bipolar triangle sync 1beat target instrument/cloud/motion depth 0.05st {
      scale 1
    }
  }
}
"#;

    const EXTENDED_GOLDEN_SOURCE: &str = r#"crump 1
project "Extended vectors" {
  id graph-vectors
  seed 1
  sample_rate 48000
  render_tail 1s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument all "All nodes" graph {
      polyphony 1
      parameter tone "Tone" hz 100hz 10000hz 900hz 900hz modulatable
      faceplate {
        control tone "Tone" "Filter tone" maps tone
      }
      graph 16 {
        node sine oscillator waveform sine level 0.2
        node square oscillator waveform square level 0.15
        node triangle oscillator waveform triangle level 0.1
        node noise noise level 0.03
        node mix_a mixer a_level 1 b_level 1
        node mix_b mixer a_level 1 b_level 1
        node mix_all mixer a_level 1 b_level 1
        node env envelope attack 0ms decay 0ms sustain 0.6 release 5ms
        node amp amplifier level 0.8
        node saturate saturation drive 1.7
        node motion lfo waveform sine rate 2hz depth 1
        node high filter mode highpass cutoff $tone resonance 0.2 mod_depth 200hz
        node notch filter mode notch cutoff 2200hz resonance 0.35
        node position pan position -0.25
        node out output
        connect note.pitch -> sine.pitch
        connect note.pitch -> square.pitch
        connect note.pitch -> triangle.pitch
        connect sine.audio -> mix_a.a
        connect square.audio -> mix_a.b
        connect triangle.audio -> mix_b.a
        connect noise.audio -> mix_b.b
        connect mix_a.audio -> mix_all.a
        connect mix_b.audio -> mix_all.b
        connect note.gate -> env.gate
        connect mix_all.audio -> amp.audio
        connect env.control -> amp.gain
        connect amp.audio -> saturate.audio
        connect motion.control -> high.mod
        connect saturate.audio -> high.audio
        connect high.audio -> notch.audio
        connect notch.audio -> position.audio
        connect position.audio -> out.audio
      }
    }
    pattern notes 1bar {
      note 0beat C4 1beat 0.8
    }
    track all "All" instrument all level -6db pan 0 {
      clip one pattern notes start 0bar duration 1bar mode once
    }
  }
}
"#;

    #[test]
    fn expressive_graph_combines_velocity_filter_envelopes_key_tracking_and_unison() {
        let document = parse_project(Path::new("expressive.crump"), EXPRESSIVE_SOURCE);
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");
        let diagnostics = validate_project(&project);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let mut control_runtime = controls.runtime();
        control_runtime.evaluate_frame(0);

        let touch = &project.song.instruments[0];
        let touch_graph =
            CompiledGraph::compile(touch, &project, 48_000, &controls).expect("touch graph");
        let filter = touch_graph
            .nodes
            .iter()
            .find(|node| node.program.is_bandpass_filter())
            .expect("filter node");
        assert!(filter.inputs.contains_key("key"));
        assert!(filter.inputs.contains_key("velocity"));
        assert!(filter.inputs.contains_key("mod"));
        assert!(filter.program.is_bandpass_filter());

        let mut soft = touch_graph.create_voice(11);
        let mut loud = touch_graph.create_voice(11);
        soft.activate(1, 60, 0.2, 91);
        loud.activate(1, 60, 0.9, 91);
        let mut soft_energy = 0.0_f64;
        let mut loud_energy = 0.0_f64;
        for frame in 0..4_096 {
            let soft_sample = touch_graph.process(
                &mut soft,
                control_runtime.values(),
                control_runtime.pre_instrument_unconstrained_values(),
                control_runtime.song_modulation_values(),
                control_runtime.instrument_route_values(),
                GraphProcessIdentity::new(frame, 11),
            );
            let loud_sample = touch_graph.process(
                &mut loud,
                control_runtime.values(),
                control_runtime.pre_instrument_unconstrained_values(),
                control_runtime.song_modulation_values(),
                control_runtime.instrument_route_values(),
                GraphProcessIdentity::new(frame, 11),
            );
            soft_energy += f64::from(soft_sample[0]).powi(2);
            loud_energy += f64::from(loud_sample[0]).powi(2);
        }
        assert!(loud_energy > soft_energy * 2.0);

        let cloud = &project.song.instruments[1];
        let cloud_graph =
            CompiledGraph::compile(cloud, &project, 48_000, &controls).expect("cloud graph");
        assert!(
            authored_graph_work_units(cloud, &project)
                > cloud.graph.as_ref().unwrap().nodes.len() as u64
        );
        let mut first = cloud_graph.create_voice(19);
        let mut second = cloud_graph.create_voice(19);
        first.activate(2, 60, 0.8, 123);
        second.activate(2, 60, 0.8, 123);
        let mut stereo_difference = 0.0_f64;
        for frame in 0..2_048 {
            let left = cloud_graph.process(
                &mut first,
                control_runtime.values(),
                control_runtime.pre_instrument_unconstrained_values(),
                control_runtime.song_modulation_values(),
                control_runtime.instrument_route_values(),
                GraphProcessIdentity::new(frame, 19),
            );
            let right = cloud_graph.process(
                &mut second,
                control_runtime.values(),
                control_runtime.pre_instrument_unconstrained_values(),
                control_runtime.song_modulation_values(),
                control_runtime.instrument_route_values(),
                GraphProcessIdentity::new(frame, 19),
            );
            assert_eq!(left, right, "drift and phase must be deterministic");
            assert!(left.into_iter().all(f32::is_finite));
            stereo_difference += f64::from((left[0] - left[1]).abs());
        }
        assert!(
            stereo_difference > 0.01,
            "unison width must produce stereo movement"
        );
    }

    #[test]
    fn multimode_filter_and_glide_vectors_are_finite_and_distinct() {
        let mut state = FilterState::default();
        let mut energy = [0.0_f64; 4];
        for frame in 0..4_096 {
            let outputs =
                state.process(if frame == 0 { 1.0 } else { 0.0 }, 1_200.0, 0.35, 48_000.0);
            let values = [
                outputs.lowpass,
                outputs.highpass,
                outputs.bandpass,
                outputs.notch,
            ];
            for (total, value) in energy.iter_mut().zip(values) {
                assert!(value.is_finite());
                *total += value * value;
            }
        }
        assert!(energy.into_iter().all(|value| value > 0.0));
        assert_ne!(energy[0].to_bits(), energy[1].to_bits());
        assert_ne!(energy[1].to_bits(), energy[2].to_bits());

        assert_eq!(slew_frequency(220.0, 440.0, 0.0, 48_000.0), 440.0);
        let first = slew_frequency(220.0, 440.0, 0.1, 48_000.0);
        assert!(first > 220.0 && first < 440.0);
        let mut grouped = 220.0;
        for _ in 0..4_800 {
            grouped = slew_frequency(grouped, 440.0, 0.1, 48_000.0);
        }
        let mut split = 220.0;
        for _ in 0..2_400 {
            split = slew_frequency(split, 440.0, 0.1, 48_000.0);
        }
        for _ in 0..2_400 {
            split = slew_frequency(split, 440.0, 0.1, 48_000.0);
        }
        assert_eq!(grouped.to_bits(), split.to_bits());
        assert!(grouped > 300.0 && grouped < 440.0);
    }

    #[test]
    fn authored_voice_state_estimate_matches_dynamic_node_allocations() {
        fn allocated_voice_bytes(voice: &GraphVoice) -> u64 {
            voice.allocation_bytes()
        }

        let source = GRAPH_SOURCE
            .replace(
                "        node out output",
                "        node echo delay time 2s feedback 0.2 mix 0.5\n        node room reverb room 0.7 damping 0.3 mix 0.5\n        node out output",
            )
            .replace(
                "        connect pan.audio -> out.audio",
                "        connect pan.audio -> echo.audio\n        connect echo.audio -> room.audio\n        connect room.audio -> out.audio",
            );
        let document = parse_project(Path::new("voice-state.crump"), &source);
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");
        assert!(!validate_project(&project).has_errors());
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let instrument = &project.song.instruments[0];
        for sample_rate in [44_100, 48_000, 96_000] {
            let graph = CompiledGraph::compile(instrument, &project, sample_rate, &controls)
                .expect("graph");
            let voice = graph.create_voice(1);
            assert_eq!(
                authored_graph_voice_state_bytes(instrument, sample_rate, controls.target_count()),
                allocated_voice_bytes(&voice),
                "delay/reverb state at {sample_rate} Hz"
            );
        }

        let document = parse_project(Path::new("unison-state.crump"), EXPRESSIVE_SOURCE);
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");
        assert!(!validate_project(&project).has_errors());
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let instrument = &project.song.instruments[1];
        let graph =
            CompiledGraph::compile(instrument, &project, 48_000, &controls).expect("unison graph");
        let voice = graph.create_voice(1);
        assert_eq!(
            authored_graph_voice_state_bytes(instrument, 48_000, controls.target_count()),
            allocated_voice_bytes(&voice),
            "unison boxed state"
        );

        for (name, source) in [
            (
                "sample-source-state.crump",
                include_str!("../../../fixtures/synth-alpha/minimal-sample-source.crump"),
            ),
            (
                "multisample-state.crump",
                include_str!("../../../fixtures/synth-alpha/minimal-multisample.crump"),
            ),
            (
                "granular-state.crump",
                include_str!("../../../fixtures/synth-alpha/minimal-granular.crump"),
            ),
            (
                "spectral-state.crump",
                include_str!("../../../fixtures/synth-alpha/minimal-spectral.crump"),
            ),
        ] {
            let document = parse_project(Path::new(name), source);
            assert!(
                document.diagnostics.is_empty(),
                "{:?}",
                document.diagnostics
            );
            let project = document.project.expect("sample Project");
            assert!(!validate_project(&project).has_errors());
            let controls = CompiledControlProgram::compile(&project).expect("controls");
            let instrument = &project.song.instruments[0];
            let graph = CompiledGraph::compile(instrument, &project, 48_000, &controls)
                .expect("sample graph");
            let voice = graph.create_voice(1);
            assert_eq!(
                authored_graph_voice_state_bytes(instrument, 48_000, controls.target_count()),
                allocated_voice_bytes(&voice),
                "sample source state for {name}"
            );
        }
    }

    #[test]
    fn pre_refactor_graph_node_vectors_are_bit_exact() {
        // Captured from the pre-split graph runtime at source SHA-256
        // 8a80e4186afb27582ea495f30a85872e9eb1ed8929eb16e8355fe3659f901ee2.
        // These constants intentionally are not generated by the implementation under test.
        fn compile(
            source: &str,
            instrument: usize,
        ) -> (CompiledGraph, crate::engine::control::ControlRuntime) {
            let document = parse_project(Path::new("golden.crump"), source);
            assert!(
                document.diagnostics.is_empty(),
                "{:?}",
                document.diagnostics
            );
            let project = document.project.expect("Project");
            assert!(!validate_project(&project).has_errors());
            let controls = CompiledControlProgram::compile(&project).expect("controls");
            let mut runtime = controls.runtime();
            runtime.evaluate_frame(0);
            let graph = CompiledGraph::compile(
                &project.song.instruments[instrument],
                &project,
                48_000,
                &controls,
            )
            .expect("graph");
            (graph, runtime)
        }

        fn bits_at(
            graph: &CompiledGraph,
            controls: &crate::engine::control::ControlRuntime,
            voice: &mut GraphVoice,
            selected_frames: &[usize],
        ) -> Vec<[u32; 2]> {
            let mut bits = Vec::with_capacity(selected_frames.len());
            let last = selected_frames.last().copied().unwrap_or(0);
            for frame in 0..=last {
                let sample = graph.process(
                    voice,
                    controls.values(),
                    controls.pre_instrument_unconstrained_values(),
                    controls.song_modulation_values(),
                    controls.instrument_route_values(),
                    GraphProcessIdentity::new(frame as u64, 0),
                );
                if selected_frames.binary_search(&frame).is_ok() {
                    bits.push([sample[0].to_bits(), sample[1].to_bits()]);
                }
            }
            bits
        }

        let selected = [0, 1, 2, 3, 4, 5, 6, 7, 15, 31, 63, 127, 191, 255];
        let (graph, controls) = compile(GRAPH_SOURCE, 0);
        let mut voice = graph.create_voice(1);
        voice.activate(9, 60, 0.8, 7);
        assert_eq!(
            bits_at(&graph, &controls, &mut voice, &selected),
            vec![
                [0, 0],
                [3_107_034_005, 3_107_034_005],
                [3_130_162_376, 3_130_162_376],
                [3_142_967_149, 3_142_967_149],
                [3_151_145_772, 3_151_145_772],
                [3_156_753_946, 3_156_753_946],
                [3_161_215_303, 3_161_215_303],
                [3_164_102_225, 3_164_102_225],
                [3_173_822_010, 3_173_822_010],
                [3_179_831_500, 3_179_831_500],
                [3_179_941_648, 3_179_941_648],
                [1_040_410_978, 1_040_410_978],
                [3_202_043_131, 3_202_043_131],
                [3_191_796_292, 3_191_796_292],
            ]
        );

        let (graph, controls) = compile(EXPRESSIVE_SOURCE, 1);
        let mut voice = graph.create_voice(19);
        voice.activate(2, 60, 0.8, 123);
        assert_eq!(
            bits_at(&graph, &controls, &mut voice, &selected),
            vec![
                [2_973_421_526, 2_973_476_225],
                [3_137_742_602, 3_137_742_030],
                [3_141_994_187, 3_141_992_472],
                [3_145_938_779, 3_145_937_064],
                [3_147_968_356, 3_147_965_498],
                [3_149_949_825, 3_149_945_539],
                [3_151_883_187, 3_151_877_186],
                [3_153_768_441, 3_153_760_441],
                [3_160_617_602, 3_160_600_458],
                [3_165_927_315, 3_165_891_885],
                [3_165_422_452, 3_165_278_443],
                [1_028_346_822, 1_028_637_138],
                [3_191_712_929, 3_191_549_191],
                [3_179_410_905, 3_178_373_442],
            ]
        );

        let source = GRAPH_SOURCE
            .replace(
                "        node out output",
                "        node echo delay time 1ms feedback 0.2 mix 0.5\n        node room reverb room 0.7 damping 0.3 mix 0.5\n        node out output",
            )
            .replace(
                "        connect pan.audio -> out.audio",
                "        connect pan.audio -> echo.audio\n        connect echo.audio -> room.audio\n        connect room.audio -> out.audio",
            );
        let effect_frames = [
            0, 1, 2, 3, 4, 5, 6, 7, 15, 31, 47, 48, 49, 50, 63, 95, 127, 255, 511, 1023, 1556,
            1557, 1799,
        ];
        let (graph, controls) = compile(&source, 0);
        let mut voice = graph.create_voice(1);
        voice.activate(9, 60, 0.8, 7);
        assert_eq!(
            bits_at(&graph, &controls, &mut voice, &effect_frames),
            vec![
                [0, 0],
                [3_090_256_789, 3_090_256_789],
                [3_113_385_160, 3_113_385_160],
                [3_126_189_933, 3_126_189_933],
                [3_134_368_556, 3_134_368_556],
                [3_139_976_730, 3_139_976_730],
                [3_144_438_087, 3_144_438_087],
                [3_147_325_009, 3_147_325_009],
                [3_157_044_794, 3_157_044_794],
                [3_163_054_284, 3_163_054_284],
                [3_164_213_739, 3_164_213_739],
                [3_164_212_827, 3_164_212_827],
                [3_164_226_022, 3_164_226_022],
                [3_164_335_141, 3_164_335_141],
                [3_168_822_829, 3_168_822_829],
                [3_164_540_458, 3_164_540_458],
                [1_016_278_791, 1_016_278_791],
                [3_189_391_216, 3_189_391_216],
                [1_034_166_272, 1_034_166_272],
                [3_181_149_180, 3_181_149_180],
                [3_180_947_470, 3_181_989_237],
                [3_180_371_188, 3_181_440_908],
                [1_025_745_300, 1_041_293_928],
            ]
        );

        voice.release(9);
        // Note-relative Modulation retains its note-on clock through release instead of
        // retriggering at note-off; graph Node state remains in the same preallocated voice.
        assert_eq!(
            bits_at(
                &graph,
                &controls,
                &mut voice,
                &[0, 1, 2, 7, 31, 127, 255, 511, 1023, 2047]
            ),
            vec![
                [1_026_530_984, 1_041_576_888],
                [1_027_057_548, 1_041_912_112],
                [1_026_520_320, 1_042_301_982],
                [1_019_159_880, 1_044_688_434],
                [1_043_361_276, 1_039_861_607],
                [3_172_046_095, 3_173_673_132],
                [3_182_106_910, 3_173_941_240],
                [3_163_064_420, 3_155_666_588],
                [3_183_952_992, 1_034_043_460],
                [1_043_908_956, 1_038_354_814],
            ]
        );

        // Delay and Reverb intentionally retain their preallocated tail state
        // when the voice is reused for a new note.
        voice.activate(10, 67, 0.5, 17);
        assert_eq!(
            bits_at(
                &graph,
                &controls,
                &mut voice,
                &[0, 1, 2, 7, 31, 47, 48, 49, 95, 255, 511]
            ),
            vec![
                [1_035_128_466, 1_017_164_788],
                [1_036_739_060, 1_023_784_037],
                [1_038_884_673, 1_026_816_323],
                [1_046_003_143, 1_032_404_372],
                [3_159_454_152, 1_028_871_983],
                [1_050_086_278, 3_158_408_624],
                [1_046_209_322, 3_183_546_830],
                [1_046_988_693, 3_179_579_703],
                [1_038_156_121, 1_034_172_538],
                [3_186_095_464, 3_176_502_482],
                [1_028_326_498, 1_017_192_228],
            ]
        );

        // This graph covers the remaining dispatch variants plus release and
        // reuse of the same preallocated voice state. These noise-dependent
        // vectors include the intentional correction from tiny unipolar noise
        // to centered unit-range noise; the other goldens remain unchanged.
        let (graph, controls) = compile(EXTENDED_GOLDEN_SOURCE, 0);
        let mut voice = graph.create_voice(31);
        voice.activate(1, 60, 0.8, 99);
        assert_eq!(
            bits_at(
                &graph,
                &controls,
                &mut voice,
                &[0, 1, 2, 3, 4, 5, 7, 15, 31, 63, 127, 255]
            ),
            vec![
                [3_186_861_714, 3_181_563_227],
                [1_033_710_224, 1_028_786_113],
                [1_029_129_991, 1_024_448_515],
                [1_022_456_440, 1_017_205_872],
                [1_018_821_630, 1_014_532_770],
                [1_015_874_991, 1_010_595_007],
                [1_015_092_215, 1_009_548_940],
                [1_026_422_104, 1_021_868_149],
                [1_004_185_075, 999_430_297],
                [3_156_052_715, 3_151_136_900],
                [1_021_328_610, 1_016_452_279],
                [973_239_096, 967_726_066],
            ]
        );

        voice.release(1);
        assert_eq!(
            bits_at(
                &graph,
                &controls,
                &mut voice,
                &[0, 1, 2, 7, 31, 127, 255, 511]
            ),
            vec![
                [3_166_236_878, 3_161_925_011],
                [3_154_229_078, 3_148_699_870],
                [3_153_990_487, 3_148_465_298],
                [3_157_447_211, 3_153_000_445],
                [3_171_432_592, 3_166_046_772],
                [998_721_673, 993_315_185],
                [3_127_301_038, 3_122_281_421],
                [668_394_627, 663_721_519],
            ]
        );

        voice.activate(2, 67, 0.4, 101);
        assert_eq!(
            bits_at(&graph, &controls, &mut voice, &[0, 1, 2, 3, 7, 15, 31]),
            vec![
                [3_186_861_714, 3_181_563_227],
                [1_035_717_632, 1_031_468_726],
                [1_030_496_960, 1_025_361_895],
                [1_026_981_231, 1_022_615_342],
                [1_013_907_276, 1_008_709_983],
                [1_027_244_584, 1_022_967_276],
                [1_001_712_304, 997_311_736],
            ]
        );
    }

    #[test]
    fn version_two_stereo_sum_compiles_in_bus_id_order_and_stays_finite() {
        let source = include_str!("../../../tests/fixtures/stereo_foundation.crump");
        let document = parse_project(Path::new("stereo_foundation.crump"), source);
        let project = document.project.expect("version 2 Project");
        assert!(!validate_project(&project).has_errors());
        let controls = CompiledControlProgram::compile(&project).unwrap();
        let mut control_runtime = controls.runtime();
        control_runtime.evaluate_frame(0);
        let instrument = &project.song.instruments[0];
        assert_eq!(authored_graph_work_units(instrument, &project), 13);
        let graph = CompiledGraph::compile(instrument, &project, 48_000, &controls).unwrap();
        assert!(authored_graph_voice_state_bytes(instrument, 48_000, controls.target_count()) > 0);
        let mut with_shared = instrument.clone();
        let mut shared = instrument.graph.clone().unwrap();
        shared.domain = GraphDomain::Shared;
        with_shared.shared_graph = Some(shared);
        assert!(
            CompiledGraph::compile_shared(&with_shared, &project, 48_000, &controls).is_err(),
            "a voice graph relabeled as shared must fail its note-source and voice-sum contracts"
        );
        let sum = graph
            .nodes
            .iter()
            .find(|node| node.program.is_stereo_sum())
            .expect("compiled stereo sum");
        assert_eq!(
            sum.inputs.get("inputs").expect("sum inputs"),
            &[
                SignalSource::Node(6),
                SignalSource::Node(7),
                SignalSource::Node(8)
            ],
            "a-large, m-small, then z-cancel must be the effective fold order"
        );
        let mut order_probe = graph.create_voice(18);
        order_probe.signals[6] = Signal::Stereo([1.0e20, 1.0e20]);
        order_probe.signals[7] = Signal::Stereo([1.0, 1.0]);
        order_probe.signals[8] = Signal::Stereo([-1.0e20, -1.0e20]);
        assert_eq!(resolve_stereo_sum(sum, &order_probe).stereo(), [0.0, 0.0]);
        assert_eq!((-1.0e20_f32 + 1.0e20) + 1.0, 1.0);

        let mut voice = graph.create_voice(17);
        voice.activate(1, 60, 0.8, 19);
        for song_frame in 0..4_096 {
            let output = graph.process(
                &mut voice,
                control_runtime.values(),
                control_runtime.pre_instrument_unconstrained_values(),
                control_runtime.song_modulation_values(),
                control_runtime.instrument_route_values(),
                GraphProcessIdentity::new(song_frame, 17),
            );
            assert!(output.into_iter().all(f32::is_finite));
            assert_eq!(output[0].to_bits(), output[1].to_bits());
        }

        let mut invalid = project.clone();
        let authored = invalid.song.instruments[0].graph.as_mut().unwrap();
        let mut extra = authored
            .connections
            .iter()
            .find(|bus| bus.id == "a-large")
            .unwrap()
            .clone();
        extra.id = "third-output".to_owned();
        extra.to.node = "out".to_owned();
        extra.to.port = "audio".to_owned();
        authored.connections.push(extra);
        let controls = CompiledControlProgram::compile(&invalid).unwrap();
        assert!(
            CompiledGraph::compile(&invalid.song.instruments[0], &invalid, 48_000, &controls)
                .unwrap_err()
                .to_string()
                .contains("cardinality")
        );
    }

    #[test]
    fn graph_transition_compatibility_rejects_structural_node_program_changes() {
        let document = parse_project(
            Path::new("tests/fixtures/virtual_analog_foundation.crump"),
            include_str!("../../../tests/fixtures/virtual_analog_foundation.crump"),
        );
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");
        assert!(!validate_project(&project).has_errors());
        let compile = |project: &Project| {
            let controls = CompiledControlProgram::compile(project).expect("controls");
            CompiledGraph::compile(
                &project.song.instruments[0],
                project,
                project.sample_rate,
                &controls,
            )
            .expect("graph")
        };
        let current = compile(&project);

        let mut scalar = project.clone();
        scalar.song.instruments[0]
            .parameters
            .iter_mut()
            .find(|parameter| parameter.key == "oscillator-level")
            .expect("oscillator level Parameter")
            .value = 0.37;
        assert!(current.transition_compatible(&compile(&scalar)));

        let mut waveform = project.clone();
        let carrier = waveform.song.instruments[0]
            .graph
            .as_mut()
            .expect("voice graph")
            .nodes
            .iter_mut()
            .find(|node| node.id == "carrier")
            .expect("carrier");
        carrier
            .settings
            .iter_mut()
            .find(|setting| setting.key == "waveform")
            .expect("waveform setting")
            .value = crate::model::SettingValue::Text("square".to_owned());
        assert!(!current.transition_compatible(&compile(&waveform)));

        let mut allocation = project.clone();
        let carrier = allocation.song.instruments[0]
            .graph
            .as_mut()
            .expect("voice graph")
            .nodes
            .iter_mut()
            .find(|node| node.id == "carrier")
            .expect("carrier");
        carrier
            .settings
            .iter_mut()
            .find(|setting| setting.key == "unison")
            .expect("unison setting")
            .value = crate::model::SettingValue::Number(5.0);
        assert!(!current.transition_compatible(&compile(&allocation)));
    }

    #[test]
    fn wavetable_transition_compatibility_tracks_cache_allocation_and_stage_structure() {
        let document = parse_project(
            Path::new("fixtures/synth-alpha/wavetable-demo.crump"),
            include_str!("../../../fixtures/synth-alpha/wavetable-demo.crump"),
        );
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");
        assert!(!validate_project(&project).has_errors());
        let compile = |project: &Project| {
            let controls = CompiledControlProgram::compile(project).expect("controls");
            CompiledGraph::compile(
                &project.song.instruments[0],
                project,
                project.sample_rate,
                &controls,
            )
            .expect("wavetable graph")
        };
        let current = compile(&project);

        let mut scalar = project.clone();
        scalar.song.instruments[0]
            .parameters
            .iter_mut()
            .find(|parameter| parameter.key == "table-position")
            .expect("table position Parameter")
            .value = 0.73;
        assert!(current.transition_compatible(&compile(&scalar)));

        let mut cache_structure = project.clone();
        cache_structure.assets[0]
            .wavetable
            .as_mut()
            .expect("wavetable definition")
            .morph = crate::model::WavetableMorph::Linear;
        assert!(!current.transition_compatible(&compile(&cache_structure)));

        let mut allocation = project.clone();
        let source = allocation.song.instruments[0]
            .graph
            .as_mut()
            .expect("voice graph")
            .nodes
            .iter_mut()
            .find(|node| node.id == "source")
            .expect("wavetable source");
        source
            .settings
            .iter_mut()
            .find(|setting| setting.key == "unison")
            .expect("unison setting")
            .value = crate::model::SettingValue::Number(4.0);
        assert!(!current.transition_compatible(&compile(&allocation)));

        let mut stage_structure = project.clone();
        let source = stage_structure.song.instruments[0]
            .graph
            .as_mut()
            .expect("voice graph")
            .nodes
            .iter_mut()
            .find(|node| node.id == "source")
            .expect("wavetable source");
        source
            .directives
            .iter_mut()
            .find(|directive| {
                directive
                    .tokens
                    .get(1)
                    .is_some_and(|token| token.value == "bend-stage")
            })
            .and_then(|directive| directive.tokens.get_mut(2))
            .expect("bend warp kind")
            .value = "pulse_width".to_owned();
        assert!(!current.transition_compatible(&compile(&stage_structure)));
    }

    #[test]
    fn sample_transition_compatibility_retains_scalars_and_restarts_structural_state() {
        let document = parse_project(
            Path::new("fixtures/synth-alpha/minimal-sample-source.crump"),
            include_str!("../../../fixtures/synth-alpha/minimal-sample-source.crump"),
        );
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");
        assert!(!validate_project(&project).has_errors());
        let compile = |project: &Project| {
            let controls = CompiledControlProgram::compile(project).expect("controls");
            CompiledGraph::compile(
                &project.song.instruments[0],
                project,
                project.sample_rate,
                &controls,
            )
            .expect("sample graph")
        };
        let current = compile(&project);

        let mut scalar = project.clone();
        scalar.song.instruments[0]
            .parameters
            .iter_mut()
            .find(|parameter| parameter.key == "start")
            .expect("start Parameter")
            .value = 0.25;
        assert!(current.transition_compatible(&compile(&scalar)));

        let mut direction = project.clone();
        let source = direction.song.instruments[0]
            .graph
            .as_mut()
            .expect("voice graph")
            .nodes
            .iter_mut()
            .find(|node| node.id == "source")
            .expect("sample source");
        source
            .settings
            .iter_mut()
            .find(|setting| setting.key == "direction")
            .expect("direction")
            .value = crate::model::SettingValue::Text("reverse".to_owned());
        assert!(!current.transition_compatible(&compile(&direction)));

        let mut exact_asset = project;
        exact_asset.assets[0].sha256 = "1".repeat(64);
        assert!(!current.transition_compatible(&compile(&exact_asset)));
    }

    #[test]
    fn granular_transition_compatibility_retains_controls_and_restarts_structure() {
        let document = parse_project(
            Path::new("fixtures/synth-alpha/minimal-granular.crump"),
            include_str!("../../../fixtures/synth-alpha/minimal-granular.crump"),
        );
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");
        assert!(!validate_project(&project).has_errors());
        let compile = |project: &Project| {
            let controls = CompiledControlProgram::compile(project).expect("controls");
            CompiledGraph::compile(
                &project.song.instruments[0],
                project,
                project.sample_rate,
                &controls,
            )
            .expect("granular graph")
        };
        let current = compile(&project);
        let mut scalar = project.clone();
        scalar.song.instruments[0]
            .parameters
            .iter_mut()
            .find(|parameter| parameter.key == "position")
            .expect("position")
            .value = 0.7;
        assert!(current.transition_compatible(&compile(&scalar)));

        for (key, value) in [
            (
                "window",
                crate::model::SettingValue::Text("triangle".to_owned()),
            ),
            ("edge", crate::model::SettingValue::Text("wrap".to_owned())),
            ("max_grains", crate::model::SettingValue::Number(32.0)),
            ("seed", crate::model::SettingValue::Number(72.0)),
        ] {
            let mut structure = project.clone();
            structure.song.instruments[0]
                .graph
                .as_mut()
                .expect("voice graph")
                .nodes
                .iter_mut()
                .find(|node| node.id == "source")
                .expect("granular source")
                .settings
                .iter_mut()
                .find(|setting| setting.key == key)
                .unwrap_or_else(|| panic!("missing {key}"))
                .value = value;
            assert!(
                !current.transition_compatible(&compile(&structure)),
                "{key} is structural"
            );
        }
        let mut exact_asset = project;
        exact_asset.assets[0].sha256 = "1".repeat(64);
        assert!(!current.transition_compatible(&compile(&exact_asset)));
    }

    #[test]
    fn spectral_transition_compatibility_retains_controls_and_restarts_structure() {
        let document = parse_project(
            Path::new("fixtures/synth-alpha/minimal-spectral.crump"),
            include_str!("../../../fixtures/synth-alpha/minimal-spectral.crump"),
        );
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");
        assert!(!validate_project(&project).has_errors());
        let compile = |project: &Project| {
            let controls = CompiledControlProgram::compile(project).expect("controls");
            CompiledGraph::compile(
                &project.song.instruments[0],
                project,
                project.sample_rate,
                &controls,
            )
            .expect("spectral graph")
        };
        let current = compile(&project);
        let mut scalar = project.clone();
        scalar.song.instruments[0]
            .parameters
            .iter_mut()
            .find(|parameter| parameter.key == "scan")
            .expect("scan")
            .value = 0.7;
        assert!(current.transition_compatible(&compile(&scalar)));

        for (key, value) in [
            ("max_peaks", crate::model::SettingValue::Number(64.0)),
            (
                "transients",
                crate::model::SettingValue::Text("preserve".to_owned()),
            ),
            ("seed", crate::model::SettingValue::Number(80.0)),
        ] {
            let mut structure = project.clone();
            structure.song.instruments[0]
                .graph
                .as_mut()
                .expect("voice graph")
                .nodes
                .iter_mut()
                .find(|node| node.id == "source")
                .expect("spectral source")
                .settings
                .iter_mut()
                .find(|setting| setting.key == key)
                .unwrap_or_else(|| panic!("missing {key}"))
                .value = value;
            assert!(
                !current.transition_compatible(&compile(&structure)),
                "{key} is structural"
            );
        }
        let mut exact_asset = project;
        exact_asset.assets[0].sha256 = "1".repeat(64);
        assert!(!current.transition_compatible(&compile(&exact_asset)));
    }

    #[test]
    fn defensive_compile_rejects_malformed_in_memory_graph_facts() {
        let source = include_str!("../../../tests/fixtures/stereo_foundation.crump");
        let project = parse_project(Path::new("stereo_foundation.crump"), source)
            .project
            .unwrap();
        let controls = CompiledControlProgram::compile(&project).unwrap();
        let instrument = &project.song.instruments[0];
        let rejects = |candidate: &Instrument| {
            assert!(CompiledGraph::compile(candidate, &project, 48_000, &controls).is_err());
        };

        let mut bounded = instrument.clone();
        bounded.graph.as_mut().unwrap().max_nodes = 1;
        rejects(&bounded);

        let mut wrong_type = instrument.clone();
        wrong_type.graph.as_mut().unwrap().nodes[0]
            .settings
            .iter_mut()
            .find(|setting| setting.key == "waveform")
            .unwrap()
            .value = crate::model::SettingValue::Number(1.0);
        rejects(&wrong_type);

        let mut duplicate = instrument.clone();
        let setting = duplicate.graph.as_ref().unwrap().nodes[0].settings[0].clone();
        duplicate.graph.as_mut().unwrap().nodes[0]
            .settings
            .push(setting);
        rejects(&duplicate);

        let mut unknown = instrument.clone();
        let mut setting = unknown.graph.as_ref().unwrap().nodes[0].settings[0].clone();
        setting.key = "unknown".to_owned();
        unknown.graph.as_mut().unwrap().nodes[0]
            .settings
            .push(setting);
        rejects(&unknown);

        let mut unreachable = instrument.clone();
        let mut orphan = unreachable.graph.as_ref().unwrap().nodes[0].clone();
        orphan.id = "orphan".to_owned();
        orphan.kind = NodeKind::Noise;
        orphan.settings.clear();
        unreachable.graph.as_mut().unwrap().nodes.push(orphan);
        rejects(&unreachable);

        let mut no_output = instrument.clone();
        no_output
            .graph
            .as_mut()
            .unwrap()
            .nodes
            .retain(|node| node.kind != NodeKind::Output);
        no_output
            .graph
            .as_mut()
            .unwrap()
            .connections
            .retain(|connection| connection.to.node != "out");
        rejects(&no_output);
    }

    #[test]
    fn shared_graph_uses_track_projection_and_retains_post_voice_tail_state() {
        let source = include_str!("../../../tests/fixtures/shared_graph_foundation.crump");
        let project = parse_project(Path::new("shared-graph.crump"), source)
            .project
            .expect("Project");
        assert!(!validate_project(&project).has_errors());
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let graph = CompiledGraph::compile_shared(
            &project.song.instruments[0],
            &project,
            48_000,
            &controls,
        )
        .expect("compile shared")
        .expect("shared graph");
        let mut runtime = graph.create_shared_runtime(91);
        assert_eq!(
            runtime.allocation_bytes(),
            authored_shared_graph_state_bytes(
                &project.song.instruments[0],
                project.sample_rate,
                controls.target_count(),
            )
        );
        let fx = controls
            .index(&ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "fx".to_owned(),
            })
            .expect("fx");
        let mut control_runtime = controls.runtime();
        control_runtime.evaluate_frame(0);
        graph.process_shared(
            &mut runtime,
            control_runtime.values(),
            control_runtime.pre_instrument_unconstrained_values(),
            control_runtime.song_modulation_values(),
            control_runtime.instrument_route_values(),
            GraphProcessIdentity::shared(0, 17, [1.0, -1.0]),
        );
        assert!(
            (runtime.effective_control(fx) - 0.5).abs() < 1.0e-12,
            "shared projection includes the Instrument route but omits velocity per-voice depth"
        );

        let mut delayed_peak = 0.0_f32;
        for frame in 1..300 {
            control_runtime.evaluate_frame(frame);
            let output = graph.process_shared(
                &mut runtime,
                control_runtime.values(),
                control_runtime.pre_instrument_unconstrained_values(),
                control_runtime.song_modulation_values(),
                control_runtime.instrument_route_values(),
                GraphProcessIdentity::shared(frame, 17, [0.0, 0.0]),
            );
            if frame >= 240 {
                delayed_peak = delayed_peak.max(output[0].abs()).max(output[1].abs());
            }
        }
        assert!(
            delayed_peak > 0.1,
            "shared Delay must keep processing after the voice sum becomes silent"
        );
    }

    #[test]
    fn mixed_scope_instrument_routes_fold_in_signed_order_then_id() {
        let source = include_str!("../../../tests/fixtures/instrument_modulation.crump");
        let mut project = parse_project(Path::new("mixed-order.crump"), source)
            .project
            .expect("Project");
        project.song.modulations.clear();
        let instrument = &mut project.song.instruments[0];
        for route in &mut instrument.modulation_routes {
            route.bypass = !matches!(route.id.as_str(), "macro-tone" | "motion-tone");
        }
        let macro_route = instrument
            .modulation_routes
            .iter_mut()
            .find(|route| route.id == "macro-tone")
            .expect("macro route");
        macro_route.depth = "2e20".to_owned();
        macro_route.order = -20;
        let mut cancellation = macro_route.clone();
        cancellation.id = "macro-cancel".to_owned();
        cancellation.depth = "-2e20".to_owned();
        cancellation.order = 0;
        instrument.modulation_routes.push(cancellation);
        let motion = instrument
            .modulation_routes
            .iter_mut()
            .find(|route| route.id == "motion-tone")
            .expect("Instrument route");
        motion.depth = "1".to_owned();
        motion.order = -10;

        assert!(!validate_project(&project).has_errors());
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let mut control_runtime = controls.runtime();
        control_runtime.evaluate_frame(0);
        let graph =
            CompiledGraph::compile(&project.song.instruments[0], &project, 48_000, &controls)
                .expect("graph");
        let mut voice = graph.create_voice(1);
        voice.activate(1, 60, 0.8, 5);
        graph.process(
            &mut voice,
            control_runtime.values(),
            control_runtime.pre_instrument_unconstrained_values(),
            control_runtime.song_modulation_values(),
            control_runtime.instrument_route_values(),
            GraphProcessIdentity::new(0, 1),
        );
        let tone = controls
            .index(&ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "tone".to_owned(),
            })
            .expect("tone");
        assert_eq!(
            voice.effective_controls[tone.0].to_bits(),
            0.0_f64.to_bits(),
            "global +1e20, Instrument +1, then global -1e20 must follow authored order"
        );
    }

    #[test]
    fn chained_macro_routes_feed_downstream_routes_in_the_same_frame() {
        let source = include_str!("../../../tests/fixtures/instrument_modulation.crump");
        let mut project = parse_project(Path::new("macro-chain.crump"), source)
            .project
            .expect("Project");
        project.song.modulations.clear();
        let instrument = &mut project.song.instruments[0];
        instrument
            .modulation_sources
            .iter_mut()
            .find(|source| source.id == "macro-source")
            .expect("macro source")
            .scope = InstrumentModulationScope::PerVoice;
        for route in &mut instrument.modulation_routes {
            route.bypass = !matches!(route.id.as_str(), "macro-tone" | "velocity-tone");
        }
        let macro_tone = instrument
            .modulation_routes
            .iter_mut()
            .find(|route| route.id == "macro-tone")
            .expect("macro consumer");
        macro_tone.depth = "1".to_owned();
        macro_tone.order = -20;
        let producer = instrument
            .modulation_routes
            .iter_mut()
            .find(|route| route.id == "velocity-tone")
            .expect("macro producer");
        producer.id = "velocity-macro".to_owned();
        producer.target = "macro".to_owned();
        producer.depth = "0.25".to_owned();
        producer.order = -30;

        assert!(!validate_project(&project).has_errors());
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let mut control_runtime = controls.runtime();
        control_runtime.evaluate_frame(0);
        let graph =
            CompiledGraph::compile(&project.song.instruments[0], &project, 48_000, &controls)
                .expect("graph");
        let mut voice = graph.create_voice(1);
        voice.activate(1, 60, 0.8, 5);
        graph.process(
            &mut voice,
            control_runtime.values(),
            control_runtime.pre_instrument_unconstrained_values(),
            control_runtime.song_modulation_values(),
            control_runtime.instrument_route_values(),
            GraphProcessIdentity::new(0, 1),
        );
        let tone = controls
            .index(&ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "tone".to_owned(),
            })
            .expect("tone");
        let actual = voice.effective_controls[tone.0];
        assert!(
            (actual - 0.9).abs() < 1.0e-7,
            "velocity must update macro from 0.5 to 0.7 before macro feeds tone; got {actual}"
        );
    }

    #[test]
    fn instrument_routes_fold_before_song_routes_with_one_final_clamp() {
        let source = include_str!("../../../tests/fixtures/instrument_modulation.crump");
        let mut project = parse_project(Path::new("tier-order.crump"), source)
            .project
            .expect("Project");
        let instrument = &mut project.song.instruments[0];
        for route in &mut instrument.modulation_routes {
            route.bypass = route.id != "motion-tone";
        }
        instrument
            .modulation_routes
            .iter_mut()
            .find(|route| route.id == "motion-tone")
            .expect("Instrument route")
            .depth = "1".to_owned();
        project.song.modulations[0].depth = "0.5".to_owned();

        assert!(!validate_project(&project).has_errors());
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let mut control_runtime = controls.runtime();
        control_runtime.evaluate_frame(0);
        let graph =
            CompiledGraph::compile(&project.song.instruments[0], &project, 48_000, &controls)
                .expect("graph");
        let mut voice = graph.create_voice(1);
        voice.activate(1, 60, 0.8, 5);
        graph.process(
            &mut voice,
            control_runtime.values(),
            control_runtime.pre_instrument_unconstrained_values(),
            control_runtime.song_modulation_values(),
            control_runtime.instrument_route_values(),
            GraphProcessIdentity::new(0, 1),
        );
        let tone = controls
            .index(&ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "tone".to_owned(),
            })
            .expect("tone");
        assert!(
            (voice.effective_controls[tone.0] - 0.7).abs() < 1.0e-12,
            "base 0.2 + Instrument 1.0 + Song -0.5 must clamp once to 0.7"
        );
    }

    #[test]
    fn scoped_random_identity_distinguishes_tracks_but_not_global_state() {
        fn tone_after_one_frame(source: &str, instrument_identity: u64) -> f64 {
            let project = parse_project(Path::new("scoped-random.crump"), source)
                .project
                .expect("Project");
            assert!(!validate_project(&project).has_errors());
            let controls = CompiledControlProgram::compile(&project).expect("controls");
            let mut control_runtime = controls.runtime();
            control_runtime.evaluate_frame(100);
            let graph =
                CompiledGraph::compile(&project.song.instruments[0], &project, 48_000, &controls)
                    .expect("graph");
            let mut voice = graph.create_voice(1);
            voice.activate(7, 60, 0.8, 11);
            graph.process(
                &mut voice,
                control_runtime.values(),
                control_runtime.pre_instrument_unconstrained_values(),
                control_runtime.song_modulation_values(),
                control_runtime.instrument_route_values(),
                GraphProcessIdentity::new(100, instrument_identity),
            );
            let tone = controls
                .index(&ParameterTarget::Instrument {
                    instrument: "lead".to_owned(),
                    parameter: "tone".to_owned(),
                })
                .expect("tone");
            voice.effective_controls[tone.0]
        }

        let source = include_str!("../../../tests/fixtures/instrument_modulation.crump");
        let instrument_scoped = source.replace(
            "source random random_hold per_voice bipolar rate free 4hz seed 91",
            "source random random_hold instrument bipolar rate free 4hz seed 91",
        );
        assert_eq!(
            tone_after_one_frame(&instrument_scoped, 31).to_bits(),
            tone_after_one_frame(&instrument_scoped, 31).to_bits()
        );
        assert_ne!(
            tone_after_one_frame(&instrument_scoped, 31).to_bits(),
            tone_after_one_frame(&instrument_scoped, 47).to_bits(),
            "Instrument scope must include durable Track identity"
        );

        let global_scoped = instrument_scoped.replace(
            "source random random_hold instrument bipolar rate free 4hz seed 91",
            "source random random_hold global bipolar rate free 4hz seed 91",
        );
        assert_eq!(
            tone_after_one_frame(&global_scoped, 31).to_bits(),
            tone_after_one_frame(&global_scoped, 47).to_bits(),
            "global scope must ignore Track identity"
        );

        let pitch = 60_u8;
        let first_note = 10_u64;
        let second_note = 11_u64;
        let first_seed = first_note ^ (u64::from(pitch) << 32) ^ 0x6a09_e667_f3bc_c909;
        let second_seed = second_note ^ (u64::from(pitch) << 32) ^ 0x6a09_e667_f3bc_c909;
        assert_ne!(
            mix_modulation_identity(first_seed, first_note),
            mix_modulation_identity(second_seed, second_note),
            "same-pitch audition note IDs must not cancel out of random identity"
        );
    }

    #[test]
    fn note_relative_modulation_age_continues_through_release() {
        let source = include_str!("../../../tests/fixtures/instrument_modulation.crump");
        let project = parse_project(Path::new("release-age.crump"), source)
            .project
            .expect("Project");
        assert!(!validate_project(&project).has_errors());
        let controls = CompiledControlProgram::compile(&project).expect("controls");
        let mut control_runtime = controls.runtime();
        control_runtime.evaluate_frame(0);
        let graph =
            CompiledGraph::compile(&project.song.instruments[0], &project, 48_000, &controls)
                .expect("graph");
        let mut voice = graph.create_voice(2);
        voice.activate(9, 60, 0.8, 7);
        for song_frame in 0..128 {
            graph.process(
                &mut voice,
                control_runtime.values(),
                control_runtime.pre_instrument_unconstrained_values(),
                control_runtime.song_modulation_values(),
                control_runtime.instrument_route_values(),
                GraphProcessIdentity::new(song_frame, 2),
            );
        }
        assert_eq!(voice.modulation_age_frames, 128);
        voice.release(9);
        assert_eq!(
            voice.age_frames, 0,
            "release lifetime retains its own clock"
        );
        assert_eq!(voice.modulation_age_frames, 128);
        graph.process(
            &mut voice,
            control_runtime.values(),
            control_runtime.pre_instrument_unconstrained_values(),
            control_runtime.song_modulation_values(),
            control_runtime.instrument_route_values(),
            GraphProcessIdentity::new(128, 2),
        );
        assert_eq!(voice.modulation_age_frames, 129);
    }

    #[test]
    fn graph_voice_is_deterministic_and_releases() {
        let document = parse_project(Path::new("project.crump"), GRAPH_SOURCE);
        let project = document.project.unwrap();
        assert!(!validate_project(&project).has_errors());
        let controls = CompiledControlProgram::compile(&project).unwrap();
        let mut control_runtime = controls.runtime();
        control_runtime.evaluate_frame(0);
        let graph =
            CompiledGraph::compile(&project.song.instruments[0], &project, 48_000, &controls)
                .unwrap();
        let mut first = graph.create_voice(1);
        let mut second = graph.create_voice(1);
        let mut quarter_cycle = graph.create_voice(2);
        quarter_cycle.activate(10, 60, 0.8, 8);
        quarter_cycle.age_frames = 12_000;
        quarter_cycle.modulation_age_frames = 12_000;
        let _ = graph.process(
            &mut quarter_cycle,
            control_runtime.values(),
            control_runtime.pre_instrument_unconstrained_values(),
            control_runtime.song_modulation_values(),
            control_runtime.instrument_route_values(),
            GraphProcessIdentity::new(12_000, 2),
        );
        let tone = controls
            .index(&crate::model::ParameterTarget::Instrument {
                instrument: "lead".to_owned(),
                parameter: "tone".to_owned(),
            })
            .expect("tone control");
        assert!((quarter_cycle.effective_controls[tone.0] - 4_000.0).abs() < 1.0e-9);
        first.activate(9, 60, 0.8, 7);
        second.activate(9, 60, 0.8, 7);
        for song_frame in 0..256 {
            assert_eq!(
                graph.process(
                    &mut first,
                    control_runtime.values(),
                    control_runtime.pre_instrument_unconstrained_values(),
                    control_runtime.song_modulation_values(),
                    control_runtime.instrument_route_values(),
                    GraphProcessIdentity::new(song_frame, 1),
                ),
                graph.process(
                    &mut second,
                    control_runtime.values(),
                    control_runtime.pre_instrument_unconstrained_values(),
                    control_runtime.song_modulation_values(),
                    control_runtime.instrument_route_values(),
                    GraphProcessIdentity::new(song_frame, 1),
                )
            );
        }
        first.release(9);
        for song_frame in 256..48_256 {
            graph.process(
                &mut first,
                control_runtime.values(),
                control_runtime.pre_instrument_unconstrained_values(),
                control_runtime.song_modulation_values(),
                control_runtime.instrument_route_values(),
                GraphProcessIdentity::new(song_frame, 1),
            );
            if !first.active() {
                break;
            }
        }
        assert!(!first.active());
    }
}
