//! Compilation and deterministic execution of validated Composable DSP Graphs.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{PI, TAU};

use crate::model::{Graph, GraphNode, Instrument, NodeKind, Parameter, SettingValue, Unit};

use super::EngineError;

const MAX_DELAY_SECONDS: f64 = 2.0;
const MAX_RELEASE_SECONDS: f64 = 8.0;
const QUIET_SECONDS: f64 = 0.1;

/// Immutable graph program shared by preallocated voices.
#[derive(Debug, Clone)]
pub(crate) struct CompiledGraph {
    nodes: Vec<CompiledNode>,
    output_index: usize,
    sample_rate: f64,
    has_envelope: bool,
}

impl CompiledGraph {
    /// Compiles a graph after whole-Project validation.
    pub(crate) fn compile(instrument: &Instrument, sample_rate: u32) -> Result<Self, EngineError> {
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
            let config = compile_config(authored, &parameters, sample_rate)?;
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
        })
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
    pub(crate) fn process(&self, voice: &mut GraphVoice) -> [f32; 2] {
        if !voice.active {
            return [0.0, 0.0];
        }
        for (index, node) in self.nodes.iter().enumerate() {
            // Resolve copyable inputs before mutably borrowing this node's state.
            // A topological program guarantees every referenced signal was written.
            let pitch = resolve_input(node, "pitch", voice);
            let gate = resolve_input(node, "gate", voice);
            let audio = resolve_input(node, "audio", voice);
            let gain = resolve_input(node, "gain", voice);
            let modulation = resolve_input(node, "mod", voice);
            let input_a = resolve_input(node, "a", voice);
            let input_b = resolve_input(node, "b", voice);
            let output = match (node.config, &mut voice.states[index]) {
                (NodeConfig::Oscillator(config), NodeState::Oscillator(state)) => {
                    let pitch = pitch.control().max(1.0) as f64;
                    let frequency = (pitch * 2.0_f64.powf(config.detune / 12.0))
                        .clamp(1.0, self.sample_rate * 0.45);
                    let increment = frequency / self.sample_rate;
                    let sample =
                        oscillator_sample(config.waveform, state.phase, increment) * config.level;
                    state.phase = (state.phase + increment).fract();
                    Signal::Mono(sample as f32)
                }
                (NodeConfig::Noise(config), NodeState::Noise(state)) => {
                    state.rng = xorshift(state.rng);
                    let sample =
                        ((state.rng >> 40) as i32 as f32 / i32::MAX as f32) * config.level as f32;
                    Signal::Mono(sample)
                }
                (NodeConfig::Envelope(config), NodeState::Envelope(state)) => Signal::Control(
                    state.process(gate.control() >= 0.5, &config, self.sample_rate) as f32,
                ),
                (NodeConfig::Mixer(config), NodeState::Stateless) => Signal::Mono(
                    input_a.mono() * config.a_level as f32 + input_b.mono() * config.b_level as f32,
                ),
                (NodeConfig::Lowpass(config), NodeState::Lowpass(state)) => {
                    let cutoff = (config.cutoff + modulation.control() as f64 * config.mod_depth)
                        .clamp(10.0, self.sample_rate * 0.45);
                    Signal::Mono(state.process(
                        audio.mono() as f64,
                        cutoff,
                        config.resonance,
                        self.sample_rate,
                    ) as f32)
                }
                (NodeConfig::Amplifier(config), NodeState::Stateless) => {
                    Signal::Mono(audio.mono() * gain.control() * config.level as f32)
                }
                (NodeConfig::Saturation(config), NodeState::Stateless) => {
                    let input = audio.mono();
                    let drive = config.drive.clamp(0.0, 8.0) as f32;
                    if drive == 0.0 {
                        Signal::Mono(input)
                    } else {
                        let normalizer = drive.tanh().max(0.000_001);
                        Signal::Mono((input * drive).tanh() / normalizer)
                    }
                }
                (NodeConfig::Pan(config), NodeState::Stateless) => {
                    let input = audio.mono();
                    let angle = (config.position.clamp(-1.0, 1.0) + 1.0) * PI * 0.25;
                    Signal::Stereo([
                        (input as f64 * angle.cos()) as f32,
                        (input as f64 * angle.sin()) as f32,
                    ])
                }
                (NodeConfig::Delay(config), NodeState::Delay(state)) => {
                    Signal::Stereo(state.process(audio.stereo(), &config))
                }
                (NodeConfig::Reverb(config), NodeState::Reverb(state)) => {
                    Signal::Stereo(state.process(audio.stereo(), &config))
                }
                (NodeConfig::Output, NodeState::Stateless) => Signal::Stereo(audio.stereo()),
                (NodeConfig::Lfo(config), NodeState::Lfo(state)) => {
                    let value = match config.waveform {
                        LfoWaveform::Sine => (state.phase * TAU).sin(),
                        LfoWaveform::Triangle => 1.0 - 4.0 * (state.phase - 0.5).abs(),
                    } * config.depth;
                    state.phase = (state.phase + config.rate / self.sample_rate).fract();
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
enum NodeConfig {
    Oscillator(OscillatorConfig),
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
    detune: f64,
    level: f64,
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
    level: f64,
}

#[derive(Debug, Clone, Copy)]
struct EnvelopeConfig {
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
}

#[derive(Debug, Clone, Copy)]
struct MixerConfig {
    a_level: f64,
    b_level: f64,
}

#[derive(Debug, Clone, Copy)]
struct LowpassConfig {
    cutoff: f64,
    resonance: f64,
    mod_depth: f64,
}

#[derive(Debug, Clone, Copy)]
struct AmplifierConfig {
    level: f64,
}

#[derive(Debug, Clone, Copy)]
struct SaturationConfig {
    drive: f64,
}

#[derive(Debug, Clone, Copy)]
struct PanConfig {
    position: f64,
}

#[derive(Debug, Clone, Copy)]
struct DelayConfig {
    delay_frames: usize,
    feedback: f32,
    mix: f32,
}

#[derive(Debug, Clone, Copy)]
struct ReverbConfig {
    feedback: f32,
    damping: f32,
    mix: f32,
}

#[derive(Debug, Clone, Copy)]
struct LfoConfig {
    waveform: LfoWaveform,
    rate: f64,
    depth: f64,
}

#[derive(Debug, Clone, Copy)]
enum LfoWaveform {
    Sine,
    Triangle,
}

#[derive(Debug, Clone)]
enum NodeState {
    Oscillator(OscillatorState),
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
            }),
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
    fn process(&mut self, gate: bool, config: &EnvelopeConfig, sample_rate: f64) -> f64 {
        if gate && !self.last_gate {
            self.stage = EnvelopeStage::Attack;
        } else if !gate && self.last_gate && self.stage != EnvelopeStage::Idle {
            self.stage = EnvelopeStage::Release;
            self.release_step = self.level / (config.release * sample_rate).max(1.0);
        }
        self.last_gate = gate;
        match self.stage {
            EnvelopeStage::Idle => self.level = 0.0,
            EnvelopeStage::Attack => {
                self.level += 1.0 / (config.attack * sample_rate).max(1.0);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvelopeStage::Decay;
                }
            }
            EnvelopeStage::Decay => {
                self.level -= (1.0 - config.sustain) / (config.decay * sample_rate).max(1.0);
                if self.level <= config.sustain {
                    self.level = config.sustain;
                    self.stage = EnvelopeStage::Sustain;
                }
            }
            EnvelopeStage::Sustain => self.level = config.sustain,
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

impl LowpassState {
    fn process(&mut self, input: f64, cutoff: f64, resonance: f64, sample_rate: f64) -> f64 {
        let g = (PI * cutoff / sample_rate).tan().min(10.0);
        let damping = 2.0 - 1.9 * resonance.clamp(0.0, 1.0);
        let denominator = 1.0 + g * (g + damping);
        let v1 = (self.integrator_one + g * (input - self.integrator_two)) / denominator;
        let v2 = self.integrator_two + g * v1;
        self.integrator_one = 2.0 * v1 - self.integrator_one;
        self.integrator_two = 2.0 * v2 - self.integrator_two;
        v2
    }
}

#[derive(Debug, Clone)]
struct DelayState {
    buffer: Vec<[f32; 2]>,
    index: usize,
}

impl DelayState {
    fn process(&mut self, input: [f32; 2], config: &DelayConfig) -> [f32; 2] {
        let delayed = self.buffer[self.index];
        self.buffer[self.index] = [
            input[0] + delayed[0] * config.feedback,
            input[1] + delayed[1] * config.feedback,
        ];
        self.index += 1;
        if self.index == self.buffer.len() {
            self.index = 0;
        }
        let dry = 1.0 - config.mix;
        [
            input[0] * dry + delayed[0] * config.mix,
            input[1] * dry + delayed[1] * config.mix,
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

    fn process(&mut self, input: [f32; 2], config: &ReverbConfig) -> [f32; 2] {
        let wet_left =
            self.left
                .process(input[0] + input[1] * 0.15, config.feedback, config.damping);
        let wet_right =
            self.right
                .process(input[1] + input[0] * 0.15, config.feedback, config.damping);
        let dry = 1.0 - config.mix;
        [
            input[0] * dry + wet_left * config.mix,
            input[1] * dry + wet_right * config.mix,
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
    sample_rate: u32,
) -> Result<NodeConfig, EngineError> {
    let number =
        |key: &str, unit: Unit, default: f64| resolve_number(node, key, unit, default, parameters);
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
            level: number("level", Unit::Ratio, 1.0)?,
        }),
        NodeKind::Noise => NodeConfig::Noise(NoiseConfig {
            level: number("level", Unit::Ratio, 1.0)?,
        }),
        NodeKind::Envelope => NodeConfig::Envelope(EnvelopeConfig {
            attack: number("attack", Unit::Seconds, 0.01)?.max(0.0),
            decay: number("decay", Unit::Seconds, 0.1)?.max(0.0),
            sustain: number("sustain", Unit::Ratio, 0.7)?.clamp(0.0, 1.0),
            release: number("release", Unit::Seconds, 0.3)?.clamp(0.0, MAX_RELEASE_SECONDS),
        }),
        NodeKind::Mixer => NodeConfig::Mixer(MixerConfig {
            a_level: number("a_level", Unit::Ratio, 1.0)?,
            b_level: number("b_level", Unit::Ratio, 1.0)?,
        }),
        NodeKind::Lowpass => NodeConfig::Lowpass(LowpassConfig {
            cutoff: number("cutoff", Unit::Hertz, 8_000.0)?,
            resonance: number("resonance", Unit::Ratio, 0.2)?.clamp(0.0, 1.0),
            mod_depth: number("mod_depth", Unit::Hertz, 0.0)?,
        }),
        NodeKind::Amplifier => NodeConfig::Amplifier(AmplifierConfig {
            level: number("level", Unit::Ratio, 1.0)?,
        }),
        NodeKind::Saturation => NodeConfig::Saturation(SaturationConfig {
            drive: number("drive", Unit::Ratio, 1.0)?.clamp(0.0, 8.0),
        }),
        NodeKind::Pan => NodeConfig::Pan(PanConfig {
            position: number("position", Unit::Pan, 0.0)?.clamp(-1.0, 1.0),
        }),
        NodeKind::Delay => {
            let seconds = number("time", Unit::Seconds, 0.25)?;
            if !(0.001..=MAX_DELAY_SECONDS).contains(&seconds) {
                return Err(EngineError::Compile(format!(
                    "delay time must be between 1ms and {MAX_DELAY_SECONDS}s"
                )));
            }
            NodeConfig::Delay(DelayConfig {
                delay_frames: (seconds * f64::from(sample_rate)).round() as usize,
                feedback: number("feedback", Unit::Ratio, 0.25)?.clamp(0.0, 0.95) as f32,
                mix: number("mix", Unit::Ratio, 0.15)?.clamp(0.0, 1.0) as f32,
            })
        }
        NodeKind::Reverb => NodeConfig::Reverb(ReverbConfig {
            feedback: (0.68 + number("room", Unit::Ratio, 0.5)?.clamp(0.0, 1.0) * 0.28) as f32,
            damping: number("damping", Unit::Ratio, 0.4)?.clamp(0.0, 0.95) as f32,
            mix: number("mix", Unit::Ratio, 0.15)?.clamp(0.0, 1.0) as f32,
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
            rate: number("rate", Unit::Hertz, 1.0)?.clamp(0.01, 50.0),
            depth: number("depth", Unit::Ratio, 1.0)?.clamp(0.0, 1.0),
        }),
    })
}

fn resolve_number(
    node: &GraphNode,
    key: &str,
    unit: Unit,
    default: f64,
    parameters: &BTreeMap<&str, &Parameter>,
) -> Result<f64, EngineError> {
    match node.settings.get(key) {
        None => Ok(default),
        Some(SettingValue::Number(value)) if value.is_finite() => Ok(*value),
        Some(SettingValue::Number(_)) => Err(EngineError::Compile(format!(
            "setting `{key}` is non-finite"
        ))),
        Some(SettingValue::Text(value)) => unit
            .parse_value(value)
            .map_err(|error| EngineError::Compile(format!("setting `{key}`: {error}"))),
        Some(SettingValue::Parameter(parameter)) => parameters
            .get(parameter.as_str())
            .map(|parameter| parameter.value)
            .ok_or_else(|| EngineError::Compile(format!("missing Parameter `{parameter}`"))),
    }
}

fn text<'a>(node: &'a GraphNode, key: &str, default: &'a str) -> &'a str {
    match node.settings.get(key) {
        Some(SettingValue::Text(value)) => value,
        _ => default,
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
      parameter tone "Tone" hz 100hz 10000hz 3000hz 3000hz
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
  }
}
"#;

    #[test]
    fn graph_voice_is_deterministic_and_releases() {
        let document = parse_project(Path::new("project.crump"), GRAPH_SOURCE);
        let project = document.project.unwrap();
        assert!(!validate_project(&project).has_errors());
        let graph = CompiledGraph::compile(&project.song.instruments[0], 48_000).unwrap();
        let mut first = graph.create_voice(1);
        let mut second = graph.create_voice(1);
        first.activate(9, 60, 0.8, 7);
        second.activate(9, 60, 0.8, 7);
        for _ in 0..256 {
            assert_eq!(graph.process(&mut first), graph.process(&mut second));
        }
        first.release(9);
        for _ in 0..48_000 {
            graph.process(&mut first);
            if !first.active() {
                break;
            }
        }
        assert!(!first.active());
    }
}
