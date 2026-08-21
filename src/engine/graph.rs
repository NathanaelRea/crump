//! Compilation and deterministic execution of validated Composable DSP Graphs.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{PI, TAU};

use crate::model::{Graph, GraphNode, Instrument, NodeKind, Parameter, SettingValue, Unit};

use super::{
    EngineError,
    control::{CompiledControlProgram, CompiledPerVoiceRoute, ControlIndex},
};

const MAX_DELAY_SECONDS: f64 = 2.0;
const MAX_RELEASE_SECONDS: f64 = 8.0;
const MAX_UNISON_VOICES: usize = 8;
const QUIET_SECONDS: f64 = 0.1;

/// Computes bounded node-equivalent callback work for one voice of an authored graph.
pub(crate) fn authored_graph_work_units(instrument: &Instrument) -> u64 {
    instrument.graph.as_ref().map_or(0, |graph| {
        graph.nodes.iter().fold(0_u64, |work, node| {
            let node_work = if node.kind == NodeKind::Unison {
                authored_graph_number(instrument, node, "voices", 4.0)
                    .round()
                    .clamp(1.0, MAX_UNISON_VOICES as f64) as u64
            } else {
                1
            };
            work.saturating_add(node_work)
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
        let heap = match node.kind {
            NodeKind::Delay => {
                let seconds = authored_graph_number(instrument, node, "time", 0.25);
                let frames = (seconds * f64::from(sample_rate)).round().max(1.0) as u64;
                frames.saturating_mul(std::mem::size_of::<[f32; 2]>() as u64)
            }
            NodeKind::Reverb => graph_reverb_heap_bytes(sample_rate),
            NodeKind::Unison => std::mem::size_of::<UnisonState>() as u64,
            _ => 0,
        };
        bytes.saturating_add(heap)
    })
}

fn authored_graph_number(
    instrument: &Instrument,
    node: &GraphNode,
    key: &str,
    default: f64,
) -> f64 {
    match node.settings.get(key) {
        Some(SettingValue::Number(value)) => *value,
        Some(SettingValue::Text(value)) => Unit::Seconds.parse_value(value).unwrap_or(default),
        Some(SettingValue::Parameter(parameter)) => instrument
            .parameters
            .iter()
            .find(|candidate| candidate.key == *parameter)
            .map_or(default, |parameter| parameter.value),
        None => default,
    }
}

fn graph_reverb_heap_bytes(sample_rate: u32) -> u64 {
    let scale = f64::from(sample_rate) / 48_000.0;
    let mut samples = 0_u64;
    for spread in [0_usize, 23] {
        for length in [1557_usize, 1617, 1491, 1422] {
            samples =
                samples.saturating_add(
                    (((length + spread) as f64 * scale).round() as usize).max(1) as u64,
                );
        }
        for length in [225_usize, 556] {
            samples =
                samples.saturating_add(
                    (((length + spread) as f64 * scale).round() as usize).max(1) as u64,
                );
        }
    }
    samples
        .saturating_mul(std::mem::size_of::<f32>() as u64)
        .saturating_add(8_u64.saturating_mul(std::mem::size_of::<CombFilter>() as u64))
        .saturating_add(4_u64.saturating_mul(std::mem::size_of::<AllpassFilter>() as u64))
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
            let config =
                compile_config(authored, &parameters, &instrument.id, sample_rate, controls)?;
            has_envelope |= authored.kind == NodeKind::Envelope;
            if authored.kind == NodeKind::Output {
                output_index = Some(nodes.len());
            }
            nodes.push(CompiledNode { inputs, config });
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
                NodeState::new(
                    &node.config,
                    self.sample_rate as u32,
                    slot_seed ^ index as u64,
                )
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
            let output = match (node.config, &mut voice.states[index]) {
                (NodeConfig::Oscillator(config), NodeState::Oscillator(state)) => {
                    let pitch = pitch.control().max(1.0) as f64;
                    let detune = config.detune.value(control_values);
                    let target =
                        (pitch * 2.0_f64.powf(detune / 12.0)).clamp(1.0, self.sample_rate * 0.45);
                    state.frequency = slew_frequency(
                        state.frequency,
                        target,
                        config.glide.value(control_values),
                        self.sample_rate,
                    );
                    let increment = state.frequency / self.sample_rate;
                    let sample = oscillator_sample(config.waveform, state.phase, increment)
                        * config.level.value(control_values);
                    state.phase = (state.phase + increment).fract();
                    Signal::Mono(sample as f32)
                }
                (NodeConfig::Unison(config), NodeState::Unison(state)) => {
                    let pitch = pitch.control().max(1.0) as f64;
                    let modulation = if node.inputs.contains_key("mod") {
                        modulation.control() as f64
                    } else {
                        0.0
                    };
                    let target = (pitch
                        * 2.0_f64.powf(modulation * config.mod_depth.value(control_values) / 12.0))
                    .clamp(1.0, self.sample_rate * 0.45);
                    state.frequency = slew_frequency(
                        state.frequency,
                        target,
                        config.glide.value(control_values),
                        self.sample_rate,
                    );
                    let gain = if node.inputs.contains_key("gain") {
                        gain.control() as f64
                    } else {
                        1.0
                    };
                    let velocity = if node.inputs.contains_key("velocity") {
                        velocity.control() as f64
                    } else {
                        1.0
                    };
                    let velocity_amount =
                        config.velocity_amount.value(control_values).clamp(0.0, 1.0);
                    let amplitude = gain
                        * ((1.0 - velocity_amount) + velocity_amount * velocity.clamp(0.0, 1.0))
                        * config.level.value(control_values);
                    let detune = config.detune.value(control_values);
                    let drift = config.drift.value(control_values);
                    let width = config.width.value(control_values).clamp(0.0, 1.0);
                    let normalization = 1.0 / (config.voices as f64).sqrt();
                    let mut output = [0.0_f64, 0.0_f64];
                    for subvoice in 0..config.voices {
                        let position = if config.voices == 1 {
                            0.0
                        } else {
                            subvoice as f64 * 2.0 / (config.voices - 1) as f64 - 1.0
                        };
                        let drift_value = (state.drift_phases[subvoice] * TAU).sin() * drift;
                        let frequency = (state.frequency
                            * 2.0_f64.powf((position * detune + drift_value) / 12.0))
                        .clamp(1.0, self.sample_rate * 0.45);
                        let increment = frequency / self.sample_rate;
                        let sample =
                            oscillator_sample(config.waveform, state.phases[subvoice], increment)
                                * amplitude
                                * normalization;
                        state.phases[subvoice] = (state.phases[subvoice] + increment).fract();
                        state.drift_phases[subvoice] = (state.drift_phases[subvoice]
                            + state.drift_rates[subvoice] / self.sample_rate)
                            .fract();
                        let angle = (position * width + 1.0) * PI * 0.25;
                        output[0] += sample * angle.cos();
                        output[1] += sample * angle.sin();
                    }
                    Signal::Stereo([output[0] as f32, output[1] as f32])
                }
                (NodeConfig::Noise(config), NodeState::Noise(state)) => {
                    state.rng = xorshift(state.rng);
                    let sample = ((state.rng >> 40) as i32 as f32 / i32::MAX as f32)
                        * config.level.value(control_values) as f32;
                    Signal::Mono(sample)
                }
                (NodeConfig::Envelope(config), NodeState::Envelope(state)) => Signal::Control(
                    state.process(
                        gate.control() >= 0.5,
                        config.attack.value(control_values).max(0.0),
                        config.decay.value(control_values).max(0.0),
                        config.sustain.value(control_values).clamp(0.0, 1.0),
                        config
                            .release
                            .value(control_values)
                            .clamp(0.0, MAX_RELEASE_SECONDS),
                        self.sample_rate,
                    ) as f32,
                ),
                (NodeConfig::Mixer(config), NodeState::Stateless) => Signal::Mono(
                    input_a.mono() * config.a_level.value(control_values) as f32
                        + input_b.mono() * config.b_level.value(control_values) as f32,
                ),
                (NodeConfig::Lowpass(config), NodeState::Lowpass(state)) => {
                    let key_multiplier = if node.inputs.contains_key("key") {
                        let key = key.control() as f64;
                        let reference = config.key_reference.value(control_values);
                        if key > 0.0 && reference > 0.0 {
                            2.0_f64.powf(
                                (key / reference).log2() * config.key_track.value(control_values),
                            )
                        } else {
                            1.0
                        }
                    } else {
                        1.0
                    };
                    let modulation = if node.inputs.contains_key("mod") {
                        modulation.control() as f64
                    } else {
                        0.0
                    };
                    let velocity = if node.inputs.contains_key("velocity") {
                        velocity.control() as f64
                    } else {
                        0.0
                    };
                    let cutoff = (config.cutoff.value(control_values) * key_multiplier
                        + modulation * config.mod_depth.value(control_values)
                        + velocity * config.velocity_depth.value(control_values))
                    .clamp(10.0, self.sample_rate * 0.45);
                    let outputs = state.process(
                        audio.mono() as f64,
                        cutoff,
                        config.resonance.value(control_values),
                        self.sample_rate,
                    );
                    let output = match config.mode {
                        FilterMode::Lowpass => outputs.lowpass,
                        FilterMode::Highpass => outputs.highpass,
                        FilterMode::Bandpass => outputs.bandpass,
                        FilterMode::Notch => outputs.notch,
                    };
                    Signal::Mono(output as f32)
                }
                (NodeConfig::Amplifier(config), NodeState::Stateless) => Signal::Mono(
                    audio.mono() * gain.control() * config.level.value(control_values) as f32,
                ),
                (NodeConfig::Saturation(config), NodeState::Stateless) => {
                    let input = audio.mono();
                    let drive = config.drive.value(control_values).clamp(0.0, 8.0) as f32;
                    if drive == 0.0 {
                        Signal::Mono(input)
                    } else {
                        let normalizer = drive.tanh().max(0.000_001);
                        Signal::Mono((input * drive).tanh() / normalizer)
                    }
                }
                (NodeConfig::Pan(config), NodeState::Stateless) => {
                    let input = audio.mono();
                    let angle =
                        (config.position.value(control_values).clamp(-1.0, 1.0) + 1.0) * PI * 0.25;
                    Signal::Stereo([
                        (input as f64 * angle.cos()) as f32,
                        (input as f64 * angle.sin()) as f32,
                    ])
                }
                (NodeConfig::Delay(config), NodeState::Delay(state)) => {
                    Signal::Stereo(state.process(
                        audio.stereo(),
                        config.feedback.value(control_values).clamp(0.0, 0.95) as f32,
                        config.mix.value(control_values).clamp(0.0, 1.0) as f32,
                    ))
                }
                (NodeConfig::Reverb(config), NodeState::Reverb(state)) => {
                    let room = config.room.value(control_values).clamp(0.0, 1.0);
                    Signal::Stereo(state.process(
                        audio.stereo(),
                        (0.68 + room * 0.28) as f32,
                        config.damping.value(control_values).clamp(0.0, 0.95) as f32,
                        config.mix.value(control_values).clamp(0.0, 1.0) as f32,
                    ))
                }
                (NodeConfig::Output, NodeState::Stateless) => Signal::Stereo(audio.stereo()),
                (NodeConfig::Lfo(config), NodeState::Lfo(state)) => {
                    let value = match config.waveform {
                        LfoWaveform::Sine => (state.phase * TAU).sin(),
                        LfoWaveform::Triangle => 1.0 - 4.0 * (state.phase - 0.5).abs(),
                    } * config.depth.value(control_values);
                    state.phase = (state.phase
                        + config.rate.value(control_values).clamp(0.01, 50.0) / self.sample_rate)
                        .fract();
                    Signal::Control(value as f32)
                }
                _ => Signal::None,
            };
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
            let envelopes_idle = !self.has_envelope
                || voice.states.iter().all(|state| {
                    !matches!(state, NodeState::Envelope(envelope) if envelope.stage != EnvelopeStage::Idle)
                });
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
    config: NodeConfig,
}

#[derive(Debug, Clone, Copy)]
enum SignalSource {
    Pitch,
    Gate,
    Velocity,
    Node(usize),
}

#[derive(Debug, Clone, Copy)]
enum Signal {
    None,
    Mono(f32),
    Stereo([f32; 2]),
    Control(f32),
}

impl Signal {
    const fn mono(self) -> f32 {
        match self {
            Self::Mono(value) => value,
            _ => 0.0,
        }
    }

    const fn stereo(self) -> [f32; 2] {
        match self {
            Self::Stereo(value) => value,
            _ => [0.0, 0.0],
        }
    }

    const fn control(self) -> f32 {
        match self {
            Self::Control(value) => value,
            _ => 0.0,
        }
    }
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

#[derive(Debug, Clone, Copy)]
struct Scalar {
    constant: f64,
    control: Option<ControlIndex>,
}

impl Scalar {
    const fn constant(value: f64) -> Self {
        Self {
            constant: value,
            control: None,
        }
    }

    const fn controlled(value: f64, control: ControlIndex) -> Self {
        Self {
            constant: value,
            control: Some(control),
        }
    }

    fn value(self, controls: &[f64]) -> f64 {
        self.control
            .and_then(|control| controls.get(control.0).copied())
            .unwrap_or(self.constant)
    }
}

#[derive(Debug, Clone, Copy)]
enum NodeConfig {
    Oscillator(OscillatorConfig),
    Unison(UnisonConfig),
    Noise(NoiseConfig),
    Envelope(EnvelopeConfig),
    Mixer(MixerConfig),
    Lowpass(LowpassConfig),
    Amplifier(AmplifierConfig),
    Saturation(SaturationConfig),
    Pan(PanConfig),
    Delay(DelayConfig),
    Reverb(ReverbConfig),
    Output,
    Lfo(LfoConfig),
}

#[derive(Debug, Clone, Copy)]
struct OscillatorConfig {
    waveform: OscillatorWaveform,
    detune: Scalar,
    glide: Scalar,
    level: Scalar,
}

#[derive(Debug, Clone, Copy)]
struct UnisonConfig {
    waveform: OscillatorWaveform,
    voices: usize,
    detune: Scalar,
    width: Scalar,
    drift: Scalar,
    glide: Scalar,
    mod_depth: Scalar,
    velocity_amount: Scalar,
    level: Scalar,
}

#[derive(Debug, Clone, Copy)]
enum OscillatorWaveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

#[derive(Debug, Clone, Copy)]
struct NoiseConfig {
    level: Scalar,
}

#[derive(Debug, Clone, Copy)]
struct EnvelopeConfig {
    attack: Scalar,
    decay: Scalar,
    sustain: Scalar,
    release: Scalar,
}

#[derive(Debug, Clone, Copy)]
struct MixerConfig {
    a_level: Scalar,
    b_level: Scalar,
}

#[derive(Debug, Clone, Copy)]
struct LowpassConfig {
    mode: FilterMode,
    cutoff: Scalar,
    resonance: Scalar,
    mod_depth: Scalar,
    velocity_depth: Scalar,
    key_track: Scalar,
    key_reference: Scalar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterMode {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
}

#[derive(Debug, Clone, Copy)]
struct AmplifierConfig {
    level: Scalar,
}

#[derive(Debug, Clone, Copy)]
struct SaturationConfig {
    drive: Scalar,
}

#[derive(Debug, Clone, Copy)]
struct PanConfig {
    position: Scalar,
}

#[derive(Debug, Clone, Copy)]
struct DelayConfig {
    delay_frames: usize,
    feedback: Scalar,
    mix: Scalar,
}

#[derive(Debug, Clone, Copy)]
struct ReverbConfig {
    room: Scalar,
    damping: Scalar,
    mix: Scalar,
}

#[derive(Debug, Clone, Copy)]
struct LfoConfig {
    waveform: LfoWaveform,
    rate: Scalar,
    depth: Scalar,
}

#[derive(Debug, Clone, Copy)]
enum LfoWaveform {
    Sine,
    Triangle,
}

#[derive(Debug, Clone)]
enum NodeState {
    Oscillator(OscillatorState),
    Unison(Box<UnisonState>),
    Noise(NoiseState),
    Envelope(EnvelopeState),
    Lowpass(LowpassState),
    Delay(DelayState),
    Reverb(ReverbState),
    Lfo(LfoState),
    Stateless,
}

impl NodeState {
    fn new(config: &NodeConfig, sample_rate: u32, seed: u64) -> Self {
        match config {
            NodeConfig::Oscillator(_) => Self::Oscillator(OscillatorState {
                phase: seed_phase(seed),
                frequency: 0.0,
            }),
            NodeConfig::Unison(_) => Self::Unison(Box::new(UnisonState::new(seed))),
            NodeConfig::Noise(_) => Self::Noise(NoiseState {
                rng: nonzero_seed(seed),
            }),
            NodeConfig::Envelope(_) => Self::Envelope(EnvelopeState::default()),
            NodeConfig::Lowpass(_) => Self::Lowpass(LowpassState::default()),
            NodeConfig::Delay(config) => Self::Delay(DelayState {
                buffer: vec![[0.0, 0.0]; config.delay_frames.max(1)],
                index: 0,
            }),
            NodeConfig::Reverb(_) => Self::Reverb(ReverbState::new(sample_rate)),
            NodeConfig::Lfo(_) => Self::Lfo(LfoState {
                phase: seed_phase(seed),
            }),
            NodeConfig::Mixer(_)
            | NodeConfig::Amplifier(_)
            | NodeConfig::Saturation(_)
            | NodeConfig::Pan(_)
            | NodeConfig::Output => Self::Stateless,
        }
    }

    fn retrigger(&mut self, seed: u64) {
        match self {
            Self::Oscillator(state) => state.phase = seed_phase(seed),
            Self::Unison(state) => state.retrigger(seed),
            Self::Noise(state) => state.rng = nonzero_seed(seed),
            Self::Envelope(state) => *state = EnvelopeState::default(),
            Self::Lowpass(state) => *state = LowpassState::default(),
            Self::Lfo(state) => state.phase = seed_phase(seed),
            Self::Delay(_) | Self::Reverb(_) | Self::Stateless => {}
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct OscillatorState {
    phase: f64,
    frequency: f64,
}

#[derive(Debug, Clone)]
struct UnisonState {
    phases: [f64; MAX_UNISON_VOICES],
    drift_phases: [f64; MAX_UNISON_VOICES],
    drift_rates: [f64; MAX_UNISON_VOICES],
    frequency: f64,
}

impl UnisonState {
    fn new(seed: u64) -> Self {
        let mut state = Self {
            phases: [0.0; MAX_UNISON_VOICES],
            drift_phases: [0.0; MAX_UNISON_VOICES],
            drift_rates: [0.0; MAX_UNISON_VOICES],
            frequency: 0.0,
        };
        state.retrigger(seed);
        state
    }

    fn retrigger(&mut self, seed: u64) {
        for index in 0..MAX_UNISON_VOICES {
            let voice_seed = xorshift(nonzero_seed(seed ^ index as u64));
            self.phases[index] = seed_phase(voice_seed);
            self.drift_phases[index] = seed_phase(xorshift(voice_seed));
            self.drift_rates[index] = 0.05 + seed_phase(xorshift(xorshift(voice_seed))) * 0.1;
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct NoiseState {
    rng: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum EnvelopeStage {
    #[default]
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy, Default)]
struct EnvelopeState {
    stage: EnvelopeStage,
    level: f64,
    last_gate: bool,
    release_step: f64,
}

impl EnvelopeState {
    fn process(
        &mut self,
        gate: bool,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
        sample_rate: f64,
    ) -> f64 {
        if gate && !self.last_gate {
            self.stage = EnvelopeStage::Attack;
        } else if !gate && self.last_gate && self.stage != EnvelopeStage::Idle {
            self.stage = EnvelopeStage::Release;
            self.release_step = self.level / (release * sample_rate).max(1.0);
        }
        self.last_gate = gate;
        match self.stage {
            EnvelopeStage::Idle => self.level = 0.0,
            EnvelopeStage::Attack => {
                self.level += 1.0 / (attack * sample_rate).max(1.0);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvelopeStage::Decay;
                }
            }
            EnvelopeStage::Decay => {
                self.level -= (1.0 - sustain) / (decay * sample_rate).max(1.0);
                if self.level <= sustain {
                    self.level = sustain;
                    self.stage = EnvelopeStage::Sustain;
                }
            }
            EnvelopeStage::Sustain => self.level = sustain,
            EnvelopeStage::Release => {
                self.level -= self.release_step.max(0.000_000_001);
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = EnvelopeStage::Idle;
                }
            }
        }
        self.level
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct LowpassState {
    integrator_one: f64,
    integrator_two: f64,
}

#[derive(Debug, Clone, Copy)]
struct FilterOutputs {
    lowpass: f64,
    highpass: f64,
    bandpass: f64,
    notch: f64,
}

impl LowpassState {
    fn process(
        &mut self,
        input: f64,
        cutoff: f64,
        resonance: f64,
        sample_rate: f64,
    ) -> FilterOutputs {
        let g = (PI * cutoff / sample_rate).tan().min(10.0);
        let damping = 2.0 - 1.9 * resonance.clamp(0.0, 1.0);
        let denominator = 1.0 + g * (g + damping);
        let bandpass = (self.integrator_one + g * (input - self.integrator_two)) / denominator;
        let lowpass = self.integrator_two + g * bandpass;
        let highpass = input - damping * bandpass - lowpass;
        self.integrator_one = 2.0 * bandpass - self.integrator_one;
        self.integrator_two = 2.0 * lowpass - self.integrator_two;
        FilterOutputs {
            lowpass,
            highpass,
            bandpass,
            notch: lowpass + highpass,
        }
    }
}

#[derive(Debug, Clone)]
struct DelayState {
    buffer: Vec<[f32; 2]>,
    index: usize,
}

impl DelayState {
    fn process(&mut self, input: [f32; 2], feedback: f32, mix: f32) -> [f32; 2] {
        let delayed = self.buffer[self.index];
        self.buffer[self.index] = [
            input[0] + delayed[0] * feedback,
            input[1] + delayed[1] * feedback,
        ];
        self.index += 1;
        if self.index == self.buffer.len() {
            self.index = 0;
        }
        let dry = 1.0 - mix;
        [
            input[0] * dry + delayed[0] * mix,
            input[1] * dry + delayed[1] * mix,
        ]
    }
}

#[derive(Debug, Clone)]
struct ReverbState {
    left: ReverbChannel,
    right: ReverbChannel,
}

impl ReverbState {
    fn new(sample_rate: u32) -> Self {
        Self {
            left: ReverbChannel::new(sample_rate, 0),
            right: ReverbChannel::new(sample_rate, 23),
        }
    }

    fn process(&mut self, input: [f32; 2], feedback: f32, damping: f32, mix: f32) -> [f32; 2] {
        let wet_left = self
            .left
            .process(input[0] + input[1] * 0.15, feedback, damping);
        let wet_right = self
            .right
            .process(input[1] + input[0] * 0.15, feedback, damping);
        let dry = 1.0 - mix;
        [
            input[0] * dry + wet_left * mix,
            input[1] * dry + wet_right * mix,
        ]
    }
}

#[derive(Debug, Clone)]
struct ReverbChannel {
    combs: Vec<CombFilter>,
    allpasses: Vec<AllpassFilter>,
}

impl ReverbChannel {
    fn new(sample_rate: u32, spread: usize) -> Self {
        let scale = sample_rate as f64 / 48_000.0;
        let combs = [1557, 1617, 1491, 1422]
            .into_iter()
            .map(|length| CombFilter::new(((length + spread) as f64 * scale).round() as usize))
            .collect();
        let allpasses = [225, 556]
            .into_iter()
            .map(|length| AllpassFilter::new(((length + spread) as f64 * scale).round() as usize))
            .collect();
        Self { combs, allpasses }
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let mut output = self
            .combs
            .iter_mut()
            .map(|comb| comb.process(input, feedback, damping))
            .sum::<f32>()
            * 0.25;
        for allpass in &mut self.allpasses {
            output = allpass.process(output);
        }
        output
    }
}

#[derive(Debug, Clone)]
struct CombFilter {
    buffer: Vec<f32>,
    index: usize,
    filtered: f32,
}

impl CombFilter {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(1)],
            index: 0,
            filtered: 0.0,
        }
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let output = self.buffer[self.index];
        self.filtered = output * (1.0 - damping) + self.filtered * damping;
        self.buffer[self.index] = input + self.filtered * feedback;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }
}

#[derive(Debug, Clone)]
struct AllpassFilter {
    buffer: Vec<f32>,
    index: usize,
}

impl AllpassFilter {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(1)],
            index: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.index];
        let output = -input + buffered;
        self.buffer[self.index] = input + buffered * 0.5;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }
}

#[derive(Debug, Clone, Copy)]
struct LfoState {
    phase: f64,
}

fn compile_config(
    node: &GraphNode,
    parameters: &BTreeMap<&str, &Parameter>,
    instrument_id: &str,
    sample_rate: u32,
    controls: &CompiledControlProgram,
) -> Result<NodeConfig, EngineError> {
    let number = |key: &str, unit: Unit, default: f64| {
        resolve_number(
            node,
            key,
            unit,
            default,
            parameters,
            instrument_id,
            controls,
        )
    };
    Ok(match node.kind {
        NodeKind::Oscillator => NodeConfig::Oscillator(OscillatorConfig {
            waveform: match text(node, "waveform", "saw") {
                "sine" => OscillatorWaveform::Sine,
                "saw" => OscillatorWaveform::Saw,
                "square" => OscillatorWaveform::Square,
                "triangle" => OscillatorWaveform::Triangle,
                value => {
                    return Err(EngineError::Compile(format!(
                        "unsupported oscillator waveform `{value}`"
                    )));
                }
            },
            detune: number("detune", Unit::Semitones, 0.0)?,
            glide: number("glide", Unit::Seconds, 0.0)?,
            level: number("level", Unit::Ratio, 1.0)?,
        }),
        NodeKind::Unison => NodeConfig::Unison(UnisonConfig {
            waveform: match text(node, "waveform", "saw") {
                "sine" => OscillatorWaveform::Sine,
                "saw" => OscillatorWaveform::Saw,
                "square" => OscillatorWaveform::Square,
                "triangle" => OscillatorWaveform::Triangle,
                value => {
                    return Err(EngineError::Compile(format!(
                        "unsupported unison waveform `{value}`"
                    )));
                }
            },
            voices: resolve_integer(node, "voices", 4, 1, MAX_UNISON_VOICES)?,
            detune: number("detune", Unit::Semitones, 0.15)?,
            width: number("width", Unit::Ratio, 0.75)?,
            drift: number("drift", Unit::Semitones, 0.02)?,
            glide: number("glide", Unit::Seconds, 0.0)?,
            mod_depth: number("mod_depth", Unit::Semitones, 0.0)?,
            velocity_amount: number("velocity_amount", Unit::Ratio, 0.0)?,
            level: number("level", Unit::Ratio, 1.0)?,
        }),
        NodeKind::Noise => NodeConfig::Noise(NoiseConfig {
            level: number("level", Unit::Ratio, 1.0)?,
        }),
        NodeKind::Envelope => NodeConfig::Envelope(EnvelopeConfig {
            attack: number("attack", Unit::Seconds, 0.01)?,
            decay: number("decay", Unit::Seconds, 0.1)?,
            sustain: number("sustain", Unit::Ratio, 0.7)?,
            release: number("release", Unit::Seconds, 0.3)?,
        }),
        NodeKind::Mixer => NodeConfig::Mixer(MixerConfig {
            a_level: number("a_level", Unit::Ratio, 1.0)?,
            b_level: number("b_level", Unit::Ratio, 1.0)?,
        }),
        NodeKind::Lowpass => NodeConfig::Lowpass(LowpassConfig {
            mode: match text(node, "mode", "lowpass") {
                "lowpass" => FilterMode::Lowpass,
                "highpass" => FilterMode::Highpass,
                "bandpass" => FilterMode::Bandpass,
                "notch" => FilterMode::Notch,
                value => {
                    return Err(EngineError::Compile(format!(
                        "unsupported filter mode `{value}`"
                    )));
                }
            },
            cutoff: number("cutoff", Unit::Hertz, 8_000.0)?,
            resonance: number("resonance", Unit::Ratio, 0.2)?,
            mod_depth: number("mod_depth", Unit::Hertz, 0.0)?,
            velocity_depth: number("velocity_depth", Unit::Hertz, 0.0)?,
            key_track: number("key_track", Unit::Ratio, 0.0)?,
            key_reference: number("key_reference", Unit::Hertz, 261.625_565)?,
        }),
        NodeKind::Amplifier => NodeConfig::Amplifier(AmplifierConfig {
            level: number("level", Unit::Ratio, 1.0)?,
        }),
        NodeKind::Saturation => NodeConfig::Saturation(SaturationConfig {
            drive: number("drive", Unit::Ratio, 1.0)?,
        }),
        NodeKind::Pan => NodeConfig::Pan(PanConfig {
            position: number("position", Unit::Pan, 0.0)?,
        }),
        NodeKind::Delay => {
            let seconds = number("time", Unit::Seconds, 0.25)?.constant;
            if !(0.001..=MAX_DELAY_SECONDS).contains(&seconds) {
                return Err(EngineError::Compile(format!(
                    "delay time must be between 1ms and {MAX_DELAY_SECONDS}s"
                )));
            }
            NodeConfig::Delay(DelayConfig {
                delay_frames: (seconds * f64::from(sample_rate)).round() as usize,
                feedback: number("feedback", Unit::Ratio, 0.25)?,
                mix: number("mix", Unit::Ratio, 0.15)?,
            })
        }
        NodeKind::Reverb => NodeConfig::Reverb(ReverbConfig {
            room: number("room", Unit::Ratio, 0.5)?,
            damping: number("damping", Unit::Ratio, 0.4)?,
            mix: number("mix", Unit::Ratio, 0.15)?,
        }),
        NodeKind::Output => NodeConfig::Output,
        NodeKind::Lfo => NodeConfig::Lfo(LfoConfig {
            waveform: match text(node, "waveform", "sine") {
                "sine" => LfoWaveform::Sine,
                "triangle" => LfoWaveform::Triangle,
                value => {
                    return Err(EngineError::Compile(format!(
                        "unsupported LFO waveform `{value}`"
                    )));
                }
            },
            rate: number("rate", Unit::Hertz, 1.0)?,
            depth: number("depth", Unit::Ratio, 1.0)?,
        }),
    })
}

fn resolve_number(
    node: &GraphNode,
    key: &str,
    unit: Unit,
    default: f64,
    parameters: &BTreeMap<&str, &Parameter>,
    instrument_id: &str,
    controls: &CompiledControlProgram,
) -> Result<Scalar, EngineError> {
    match node.settings.get(key) {
        None => Ok(Scalar::constant(default)),
        Some(SettingValue::Number(value)) if value.is_finite() => Ok(Scalar::constant(*value)),
        Some(SettingValue::Number(_)) => Err(EngineError::Compile(format!(
            "setting `{key}` is non-finite"
        ))),
        Some(SettingValue::Text(value)) => unit
            .parse_value(value)
            .map(Scalar::constant)
            .map_err(|error| EngineError::Compile(format!("setting `{key}`: {error}"))),
        Some(SettingValue::Parameter(parameter_key)) => {
            let parameter = parameters.get(parameter_key.as_str()).ok_or_else(|| {
                EngineError::Compile(format!("missing Parameter `{parameter_key}`"))
            })?;
            let target = crate::model::ParameterTarget::Instrument {
                instrument: instrument_id.to_owned(),
                parameter: parameter_key.clone(),
            };
            let control = controls.index(&target).ok_or_else(|| {
                EngineError::Compile(format!("missing compiled Parameter target `{target}`"))
            })?;
            Ok(Scalar::controlled(parameter.value, control))
        }
    }
}

fn text<'a>(node: &'a GraphNode, key: &str, default: &'a str) -> &'a str {
    match node.settings.get(key) {
        Some(SettingValue::Text(value)) => value,
        _ => default,
    }
}

fn resolve_integer(
    node: &GraphNode,
    key: &str,
    default: usize,
    minimum: usize,
    maximum: usize,
) -> Result<usize, EngineError> {
    match node.settings.get(key) {
        None => Ok(default),
        Some(SettingValue::Number(value))
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= minimum as f64
                && *value <= maximum as f64 =>
        {
            Ok(*value as usize)
        }
        Some(_) => Err(EngineError::Compile(format!(
            "setting `{key}` must be an integer from {minimum} to {maximum}"
        ))),
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

fn oscillator_sample(waveform: OscillatorWaveform, phase: f64, increment: f64) -> f64 {
    match waveform {
        OscillatorWaveform::Sine => (phase * TAU).sin(),
        OscillatorWaveform::Saw => 2.0 * phase - 1.0 - poly_blep(phase, increment),
        OscillatorWaveform::Square => {
            let naive = if phase < 0.5 { 1.0 } else { -1.0 };
            naive + poly_blep(phase, increment) - poly_blep((phase + 0.5).fract(), increment)
        }
        OscillatorWaveform::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
    }
}

fn poly_blep(phase: f64, increment: f64) -> f64 {
    if increment <= 0.0 {
        return 0.0;
    }
    if phase < increment {
        let normalized = phase / increment;
        return normalized + normalized - normalized * normalized - 1.0;
    }
    if phase > 1.0 - increment {
        let normalized = (phase - 1.0) / increment;
        return normalized * normalized + normalized + normalized + 1.0;
    }
    0.0
}

fn slew_frequency(current: f64, target: f64, glide_seconds: f64, sample_rate: f64) -> f64 {
    if current <= 0.0 || glide_seconds <= 0.0 {
        return target;
    }
    let coefficient = 1.0 - (-1.0 / (glide_seconds * sample_rate).max(1.0)).exp();
    let current_octaves = current.log2();
    let target_octaves = target.log2();
    2.0_f64.powf(current_octaves + (target_octaves - current_octaves) * coefficient)
}

fn midi_frequency(pitch: u8) -> f32 {
    (440.0 * 2.0_f64.powf((f64::from(pitch) - 69.0) / 12.0)) as f32
}

const fn xorshift(mut value: u64) -> u64 {
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    value
}

const fn nonzero_seed(seed: u64) -> u64 {
    if seed == 0 {
        0x9e37_79b9_7f4a_7c15
    } else {
        seed
    }
}

fn seed_phase(seed: u64) -> f64 {
    (nonzero_seed(seed) >> 11) as f64 / (1_u64 << 53) as f64
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
            .find(|node| matches!(node.config, NodeConfig::Lowpass(_)))
            .expect("filter node");
        assert!(filter.inputs.contains_key("key"));
        assert!(filter.inputs.contains_key("velocity"));
        assert!(filter.inputs.contains_key("mod"));
        assert!(matches!(
            filter.config,
            NodeConfig::Lowpass(LowpassConfig {
                mode: FilterMode::Bandpass,
                ..
            })
        ));

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
        let mut state = LowpassState::default();
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
    fn authored_voice_state_estimate_matches_delay_and_reverb_allocations() {
        fn reverb_channel_heap_bytes(channel: &ReverbChannel) -> u64 {
            let filter_objects = channel
                .combs
                .capacity()
                .saturating_mul(std::mem::size_of::<CombFilter>())
                .saturating_add(
                    channel
                        .allpasses
                        .capacity()
                        .saturating_mul(std::mem::size_of::<AllpassFilter>()),
                ) as u64;
            channel
                .combs
                .iter()
                .map(|filter| filter.buffer.capacity() as u64 * std::mem::size_of::<f32>() as u64)
                .chain(channel.allpasses.iter().map(|filter| {
                    filter.buffer.capacity() as u64 * std::mem::size_of::<f32>() as u64
                }))
                .fold(filter_objects, u64::saturating_add)
        }

        fn allocated_voice_bytes(voice: &GraphVoice) -> u64 {
            let fixed = std::mem::size_of::<GraphVoice>() as u64
                + voice.states.capacity() as u64 * std::mem::size_of::<NodeState>() as u64
                + voice.signals.capacity() as u64 * std::mem::size_of::<Signal>() as u64
                + (voice.effective_controls.capacity() + voice.previous_controls.capacity()) as u64
                    * std::mem::size_of::<f64>() as u64;
            voice.states.iter().fold(fixed, |bytes, state| {
                let heap = match state {
                    NodeState::Delay(delay) => {
                        delay.buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64
                    }
                    NodeState::Reverb(reverb) => reverb_channel_heap_bytes(&reverb.left)
                        .saturating_add(reverb_channel_heap_bytes(&reverb.right)),
                    NodeState::Unison(_) => std::mem::size_of::<UnisonState>() as u64,
                    _ => 0,
                };
                bytes.saturating_add(heap)
            })
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
        let graph = CompiledGraph::compile(instrument, 48_000, &controls).expect("graph");
        let voice = graph.create_voice(1);
        assert_eq!(
            authored_graph_voice_state_bytes(instrument, 48_000, controls.target_count()),
            allocated_voice_bytes(&voice)
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
