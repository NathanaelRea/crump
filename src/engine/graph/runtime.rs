//! Graph-program compilation, per-voice lifecycle, and deterministic execution.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Graph, Instrument, NodeKind};

use super::super::{
    control::{CompiledControlProgram, CompiledPerVoiceRoute},
    EngineError,
};
use super::nodes::{
    authored_node_heap_bytes, authored_node_work_units, NodeInputs, NodeProgram, NodeState, Signal,
    MAX_RELEASE_SECONDS,
};
#[cfg(test)]
use super::nodes::{slew_frequency, FilterState};

const QUIET_SECONDS: f64 = 0.1;

/// Computes bounded node-equivalent callback work for one voice of an authored graph.
pub(crate) fn authored_graph_work_units(instrument: &Instrument) -> u64 {
    instrument.graph.as_ref().map_or(0, |graph| {
        graph.nodes.iter().fold(0_u64, |work, node| {
            work.saturating_add(authored_node_work_units(instrument, node))
        })
    })
}

/// Computes the mutable allocation made for one voice of an authored graph.
pub(crate) fn authored_graph_voice_state_bytes(
    instrument: &Instrument,
    sample_rate: u32,
    control_targets: usize,
) -> u64 {
    let Some(graph) = &instrument.graph else {
        return 0;
    };
    let node_count = graph.nodes.len() as u64;
    let fixed_and_vectors = (std::mem::size_of::<GraphVoice>() as u64)
        .saturating_add(node_count.saturating_mul(std::mem::size_of::<NodeState>() as u64))
        .saturating_add(node_count.saturating_mul(std::mem::size_of::<Signal>() as u64))
        .saturating_add(
            (control_targets as u64)
                .saturating_mul(2)
                .saturating_mul(std::mem::size_of::<f64>() as u64),
        );
    graph.nodes.iter().fold(fixed_and_vectors, |bytes, node| {
        bytes.saturating_add(authored_node_heap_bytes(instrument, node, sample_rate))
    })
}

/// Immutable graph program shared by preallocated voices.
#[derive(Debug, Clone)]
pub(crate) struct CompiledGraph {
    nodes: Vec<CompiledNode>,
    output_index: usize,
    sample_rate: f64,
    has_envelope: bool,
    per_voice_routes: Vec<CompiledPerVoiceRoute>,
    control_count: usize,
}

impl CompiledGraph {
    /// Compiles a graph after whole-Project validation.
    pub(crate) fn compile(
        instrument: &Instrument,
        sample_rate: u32,
        controls: &CompiledControlProgram,
    ) -> Result<Self, EngineError> {
        let graph = instrument.graph.as_ref().ok_or_else(|| {
            EngineError::Compile(format!("instrument `{}` has no graph", instrument.id))
        })?;
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
            let mut inputs = BTreeMap::new();
            for connection in graph
                .connections
                .iter()
                .filter(|connection| connection.to.node == authored.id)
            {
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
                inputs.insert(connection.to.port.clone(), source);
            }
            let program =
                NodeProgram::compile(authored, &parameters, &instrument.id, sample_rate, controls)?;
            has_envelope |= authored.kind == NodeKind::Envelope;
            if authored.kind == NodeKind::Output {
                output_index = Some(nodes.len());
            }
            nodes.push(CompiledNode {
                has_key: inputs.contains_key("key"),
                has_velocity: inputs.contains_key("velocity"),
                has_gain: inputs.contains_key("gain"),
                has_modulation: inputs.contains_key("mod"),
                inputs,
                program,
            });
        }
        Ok(Self {
            nodes,
            output_index: output_index
                .ok_or_else(|| EngineError::Compile("graph has no output".to_owned()))?,
            sample_rate: f64::from(sample_rate),
            has_envelope,
            per_voice_routes: controls.per_voice_routes(&instrument.id),
            control_count: controls.target_count(),
        })
    }

    /// Owned immutable allocation retained by one Arc-shared graph program.
    pub(crate) fn allocation_bytes(&self) -> u64 {
        let word = std::mem::size_of::<usize>() as u64;
        let nodes = (self.nodes.capacity() as u64)
            .saturating_mul(std::mem::size_of::<CompiledNode>() as u64)
            .saturating_add(
                self.nodes
                    .iter()
                    .map(|node| {
                        (node.inputs.len() as u64)
                            .saturating_mul(
                                (std::mem::size_of::<(String, SignalSource)>() as u64)
                                    .saturating_add(word.saturating_mul(4)),
                            )
                            .saturating_add(
                                node.inputs
                                    .keys()
                                    .map(|port| port.capacity() as u64)
                                    .fold(0_u64, u64::saturating_add),
                            )
                    })
                    .fold(0_u64, u64::saturating_add),
            );
        let routes = (self.per_voice_routes.capacity() as u64)
            .saturating_mul(std::mem::size_of::<CompiledPerVoiceRoute>() as u64)
            .saturating_add(
                self.per_voice_routes
                    .iter()
                    .map(CompiledPerVoiceRoute::allocation_bytes)
                    .fold(0_u64, u64::saturating_add),
            );
        (std::mem::size_of::<Self>() as u64)
            .saturating_add(word.saturating_mul(2))
            .saturating_add(nodes)
            .saturating_add(routes)
    }

    /// Allocates all voice state away from the audio callback.
    pub(crate) fn create_voice(&self, slot_seed: u64) -> GraphVoice {
        let states = self
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                node.program
                    .create_state(self.sample_rate as u32, slot_seed ^ index as u64)
            })
            .collect();
        GraphVoice {
            states,
            signals: vec![Signal::None; self.nodes.len()],
            effective_controls: vec![0.0; self.control_count],
            previous_controls: vec![0.0; self.control_count],
            pitch_hz: 440.0,
            velocity: 0.0,
            gate: false,
            note_id: 0,
            active: false,
            releasing: false,
            age_frames: 0,
            quiet_frames: 0,
            seed: slot_seed,
        }
    }

    /// Processes one sample for one active voice.
    pub(crate) fn process(
        &self,
        voice: &mut GraphVoice,
        control_values: &[f64],
        unconstrained_values: &[f64],
    ) -> [f32; 2] {
        if !voice.active {
            return [0.0, 0.0];
        }
        voice
            .previous_controls
            .copy_from_slice(&voice.effective_controls);
        voice.effective_controls.copy_from_slice(control_values);
        let mut route_index = 0;
        while route_index < self.per_voice_routes.len() {
            let target = self.per_voice_routes[route_index].target();
            let mut value = unconstrained_values[target.0];
            let previous = if voice.age_frames == 0 {
                control_values[target.0]
            } else {
                voice.previous_controls[target.0]
            };
            while route_index < self.per_voice_routes.len()
                && self.per_voice_routes[route_index].target() == target
            {
                value += self.per_voice_routes[route_index].contribution_at(voice.age_frames);
                route_index += 1;
            }
            voice.effective_controls[target.0] =
                self.per_voice_routes[route_index - 1].constrain(value, previous);
        }
        let control_values = voice.effective_controls.as_slice();
        for (index, node) in self.nodes.iter().enumerate() {
            // Resolve copyable inputs before mutably borrowing this node's state.
            // A topological program guarantees every referenced signal was written.
            let pitch = resolve_input(node, "pitch", voice);
            let key = resolve_input(node, "key", voice);
            let gate = resolve_input(node, "gate", voice);
            let velocity = resolve_input(node, "velocity", voice);
            let audio = resolve_input(node, "audio", voice);
            let gain = resolve_input(node, "gain", voice);
            let modulation = resolve_input(node, "mod", voice);
            let input_a = resolve_input(node, "a", voice);
            let input_b = resolve_input(node, "b", voice);
            let inputs = NodeInputs {
                pitch,
                key,
                gate,
                velocity,
                audio,
                gain,
                modulation,
                input_a,
                input_b,
                has_key: node.has_key,
                has_velocity: node.has_velocity,
                has_gain: node.has_gain,
                has_modulation: node.has_modulation,
            };
            let output = node.program.process(
                &mut voice.states[index],
                inputs,
                control_values,
                self.sample_rate,
            );
            voice.signals[index] = output;
        }
        voice.age_frames = voice.age_frames.saturating_add(1);
        let output = voice.signals[self.output_index].stereo();
        if voice.releasing {
            let amplitude = output[0].abs().max(output[1].abs());
            if amplitude < 0.000_01 {
                voice.quiet_frames = voice.quiet_frames.saturating_add(1);
            } else {
                voice.quiet_frames = 0;
            }
            let quiet_limit = (self.sample_rate * QUIET_SECONDS) as u64;
            let release_limit = (self.sample_rate * MAX_RELEASE_SECONDS) as u64;
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
}

/// Preallocated mutable graph voice.
#[derive(Debug, Clone)]
pub(crate) struct GraphVoice {
    states: Vec<NodeState>,
    signals: Vec<Signal>,
    effective_controls: Vec<f64>,
    previous_controls: Vec<f64>,
    pitch_hz: f32,
    velocity: f32,
    gate: bool,
    note_id: u64,
    active: bool,
    releasing: bool,
    age_frames: u64,
    quiet_frames: u64,
    seed: u64,
}

impl GraphVoice {
    pub(crate) fn activate(&mut self, note_id: u64, pitch: u8, velocity: f32, seed: u64) {
        self.note_id = note_id;
        self.pitch_hz = midi_frequency(pitch);
        self.velocity = velocity.clamp(0.0, 1.0);
        self.gate = true;
        self.active = true;
        self.releasing = false;
        self.age_frames = 0;
        self.quiet_frames = 0;
        self.seed = seed;
        for (index, state) in self.states.iter_mut().enumerate() {
            state.retrigger(seed ^ index as u64);
        }
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

    pub(crate) fn stop(&mut self) {
        self.active = false;
        self.gate = false;
        self.releasing = false;
    }
}

#[derive(Debug, Clone)]
struct CompiledNode {
    inputs: BTreeMap<String, SignalSource>,
    program: NodeProgram,
    has_key: bool,
    has_velocity: bool,
    has_gain: bool,
    has_modulation: bool,
}

#[derive(Debug, Clone, Copy)]
enum SignalSource {
    Pitch,
    Gate,
    Velocity,
    Node(usize),
}

fn resolve_input(node: &CompiledNode, port: &str, voice: &GraphVoice) -> Signal {
    match node.inputs.get(port) {
        Some(SignalSource::Pitch) => Signal::Control(voice.pitch_hz),
        Some(SignalSource::Gate) => Signal::Control(f32::from(voice.gate)),
        Some(SignalSource::Velocity) => Signal::Control(voice.velocity),
        Some(SignalSource::Node(index)) => voice.signals[*index],
        None => Signal::None,
    }
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
        if connection.from.node == "note" {
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
        let touch_graph = CompiledGraph::compile(touch, 48_000, &controls).expect("touch graph");
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
        for _ in 0..4_096 {
            let soft_sample = touch_graph.process(
                &mut soft,
                control_runtime.values(),
                control_runtime.unconstrained_values(),
            );
            let loud_sample = touch_graph.process(
                &mut loud,
                control_runtime.values(),
                control_runtime.unconstrained_values(),
            );
            soft_energy += f64::from(soft_sample[0]).powi(2);
            loud_energy += f64::from(loud_sample[0]).powi(2);
        }
        assert!(loud_energy > soft_energy * 2.0);

        let cloud = &project.song.instruments[1];
        let cloud_graph = CompiledGraph::compile(cloud, 48_000, &controls).expect("cloud graph");
        assert!(
            authored_graph_work_units(cloud) > cloud.graph.as_ref().unwrap().nodes.len() as u64
        );
        let mut first = cloud_graph.create_voice(19);
        let mut second = cloud_graph.create_voice(19);
        first.activate(2, 60, 0.8, 123);
        second.activate(2, 60, 0.8, 123);
        let mut stereo_difference = 0.0_f64;
        for _ in 0..2_048 {
            let left = cloud_graph.process(
                &mut first,
                control_runtime.values(),
                control_runtime.unconstrained_values(),
            );
            let right = cloud_graph.process(
                &mut second,
                control_runtime.values(),
                control_runtime.unconstrained_values(),
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
            let fixed = std::mem::size_of::<GraphVoice>() as u64
                + voice.states.capacity() as u64 * std::mem::size_of::<NodeState>() as u64
                + voice.signals.capacity() as u64 * std::mem::size_of::<Signal>() as u64
                + (voice.effective_controls.capacity() + voice.previous_controls.capacity()) as u64
                    * std::mem::size_of::<f64>() as u64;
            voice
                .states
                .iter()
                .map(NodeState::heap_bytes)
                .fold(fixed, u64::saturating_add)
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
            let graph = CompiledGraph::compile(instrument, sample_rate, &controls).expect("graph");
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
        let graph = CompiledGraph::compile(instrument, 48_000, &controls).expect("unison graph");
        let voice = graph.create_voice(1);
        assert_eq!(
            authored_graph_voice_state_bytes(instrument, 48_000, controls.target_count()),
            allocated_voice_bytes(&voice),
            "unison boxed state"
        );
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
            let graph =
                CompiledGraph::compile(&project.song.instruments[instrument], 48_000, &controls)
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
                let sample =
                    graph.process(voice, controls.values(), controls.unconstrained_values());
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
        assert_eq!(
            bits_at(
                &graph,
                &controls,
                &mut voice,
                &[0, 1, 2, 7, 31, 127, 255, 511, 1023, 2047]
            ),
            vec![
                [1_026_515_386, 1_041_572_989],
                [1_027_012_346, 1_041_900_811],
                [1_026_449_584, 1_042_284_298],
                [1_018_901_480, 1_044_656_134],
                [1_043_328_761, 1_039_796_577],
                [3_172_328_535, 3_173_955_572],
                [3_182_247_488, 3_174_222_396],
                [3_163_633_100, 3_156_803_948],
                [3_184_078_985, 1_033_917_467],
                [1_043_785_122, 1_038_520_412],
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
                [1_034_930_134, 1_016_899_890],
                [1_036_462_561, 1_022_826_144],
                [1_038_385_530, 1_025_789_228],
                [1_045_621_262, 1_032_104_176],
                [3_155_606_392, 1_029_000_207],
                [1_049_975_312, 3_156_720_984],
                [1_046_039_332, 3_183_683_842],
                [1_046_836_382, 3_180_139_445],
                [1_038_095_838, 1_034_335_123],
                [3_186_132_116, 3_176_633_422],
                [1_029_445_642, 1_019_575_244],
            ]
        );

        // This graph covers the remaining dispatch variants plus release and
        // reuse of the same preallocated voice state.
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
                [3_183_228_578, 3_178_988_854],
                [1_033_873_582, 1_029_004_416],
                [1_027_768_114, 1_023_538_538],
                [1_021_237_990, 1_016_391_729],
                [1_014_466_502, 1_009_083_646],
                [1_008_078_229, 1_002_997_317],
                [1_007_768_689, 1_002_583_661],
                [1_020_800_953, 1_016_099_710],
                [3_143_638_558, 3_138_764_841],
                [3_157_850_060, 3_153_538_794],
                [1_020_446_415, 1_015_862_816],
                [3_156_336_776, 3_151_516_508],
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
                [3_157_090_158, 3_152_523_294],
                [3_157_290_214, 3_152_790_640],
                [3_157_321_301, 3_152_832_185],
                [3_156_847_986, 3_152_199_666],
                [3_173_748_348, 3_169_141_449],
                [1_001_827_690, 997_465_933],
                [3_127_017_331, 3_122_091_853],
                [668_808_787, 663_998_252],
            ]
        );

        voice.activate(2, 67, 0.4, 101);
        assert_eq!(
            bits_at(&graph, &controls, &mut voice, &[0, 1, 2, 3, 7, 15, 31]),
            vec![
                [3_183_228_578, 3_178_988_854],
                [1_034_214_086, 1_029_459_452],
                [1_028_772_544, 1_024_209_676],
                [1_023_480_346, 1_017_936_910],
                [1_012_866_051, 1_008_014_259],
                [1_024_001_682, 1_018_633_600],
                [3_156_082_337, 3_151_176_485],
            ]
        );
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
            CompiledGraph::compile(&project.song.instruments[0], 48_000, &controls).unwrap();
        let mut first = graph.create_voice(1);
        let mut second = graph.create_voice(1);
        let mut quarter_cycle = graph.create_voice(2);
        quarter_cycle.activate(10, 60, 0.8, 8);
        quarter_cycle.age_frames = 12_000;
        let _ = graph.process(
            &mut quarter_cycle,
            control_runtime.values(),
            control_runtime.unconstrained_values(),
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
        for _ in 0..256 {
            assert_eq!(
                graph.process(
                    &mut first,
                    control_runtime.values(),
                    control_runtime.unconstrained_values(),
                ),
                graph.process(
                    &mut second,
                    control_runtime.values(),
                    control_runtime.unconstrained_values(),
                )
            );
        }
        first.release(9);
        for _ in 0..48_000 {
            graph.process(
                &mut first,
                control_runtime.values(),
                control_runtime.unconstrained_values(),
            );
            if !first.active() {
                break;
            }
        }
        assert!(!first.active());
    }
}
