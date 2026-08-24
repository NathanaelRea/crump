//! Closed native graph-node programs and preallocated state.

use std::{collections::BTreeMap, f64::consts::PI};

use crate::model::{GraphNode, Instrument, NodeKind, Parameter, SettingValue, Unit};

use super::super::{
    control::{CompiledControlProgram, ControlIndex},
    EngineError,
};

mod effects;
mod envelope;
mod filter;
mod oscillator;

use effects::MAX_DELAY_SECONDS;
#[cfg(test)]
pub(super) use filter::FilterState;
#[cfg(test)]
pub(super) use oscillator::slew_frequency;

pub(super) const MAX_RELEASE_SECONDS: f64 = envelope::MAX_RELEASE_SECONDS;

/// Returns the callback work represented by one authored node.
pub(super) fn authored_node_work_units(instrument: &Instrument, node: &GraphNode) -> u64 {
    if node.kind == NodeKind::Unison {
        authored_node_number(instrument, node, "voices", 4.0)
            .round()
            .clamp(1.0, oscillator::MAX_UNISON_VOICES as f64) as u64
    } else {
        1
    }
}

/// Returns heap storage allocated by one authored node state.
pub(super) fn authored_node_heap_bytes(
    instrument: &Instrument,
    node: &GraphNode,
    sample_rate: u32,
) -> u64 {
    match node.kind {
        NodeKind::Delay => {
            let seconds = authored_node_number(instrument, node, "time", 0.25);
            let frames = (seconds * f64::from(sample_rate)).round().max(1.0) as u64;
            frames.saturating_mul(std::mem::size_of::<[f32; 2]>() as u64)
        }
        NodeKind::Reverb => effects::graph_reverb_heap_bytes(sample_rate),
        NodeKind::Unison => std::mem::size_of::<oscillator::UnisonState>() as u64,
        _ => 0,
    }
}

fn authored_node_number(instrument: &Instrument, node: &GraphNode, key: &str, default: f64) -> f64 {
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

use effects::{DelayConfig, DelayState, ReverbConfig, ReverbState};
use envelope::{EnvelopeConfig, EnvelopeState};
use filter::{FilterConfig, FilterMode, FilterState as RuntimeFilterState};
use oscillator::{
    LfoConfig, LfoState, LfoWaveform, NoiseConfig, NoiseState, OscillatorConfig, OscillatorState,
    OscillatorWaveform, UnisonConfig, UnisonState,
};

#[derive(Debug, Clone, Copy)]
pub(super) enum Signal {
    None,
    Mono(f32),
    Stereo([f32; 2]),
    Control(f32),
}

impl Signal {
    pub(super) const fn mono(self) -> f32 {
        match self {
            Self::Mono(value) => value,
            _ => 0.0,
        }
    }

    pub(super) const fn stereo(self) -> [f32; 2] {
        match self {
            Self::Stereo(value) => value,
            _ => [0.0, 0.0],
        }
    }

    pub(super) const fn control(self) -> f32 {
        match self {
            Self::Control(value) => value,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Scalar {
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

    pub(super) fn value(self, controls: &[f64]) -> f64 {
        self.control
            .and_then(|control| controls.get(control.0).copied())
            .unwrap_or(self.constant)
    }
}

#[derive(Debug, Clone, Copy)]
struct MixerConfig {
    a_level: Scalar,
    b_level: Scalar,
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

/// Immutable closed program for one authored graph Node.
#[derive(Debug, Clone, Copy)]
pub(super) struct NodeProgram(NodeProgramKind);

#[derive(Debug, Clone, Copy)]
enum NodeProgramKind {
    Oscillator(OscillatorConfig),
    Unison(UnisonConfig),
    Noise(NoiseConfig),
    Envelope(EnvelopeConfig),
    Mixer(MixerConfig),
    Filter(FilterConfig),
    Amplifier(AmplifierConfig),
    Saturation(SaturationConfig),
    Pan(PanConfig),
    Delay(DelayConfig),
    Reverb(ReverbConfig),
    Output,
    Lfo(LfoConfig),
}

/// Mutable state paired with one [`NodeProgram`] variant at voice creation.
#[derive(Debug, Clone)]
pub(super) struct NodeState(NodeStateKind);

#[derive(Debug, Clone)]
enum NodeStateKind {
    Oscillator(OscillatorState),
    Unison(Box<UnisonState>),
    Noise(NoiseState),
    Envelope(EnvelopeState),
    Filter(RuntimeFilterState),
    Delay(DelayState),
    Reverb(ReverbState),
    Lfo(LfoState),
    Stateless,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NodeInputs {
    pub(super) pitch: Signal,
    pub(super) key: Signal,
    pub(super) gate: Signal,
    pub(super) velocity: Signal,
    pub(super) audio: Signal,
    pub(super) gain: Signal,
    pub(super) modulation: Signal,
    pub(super) input_a: Signal,
    pub(super) input_b: Signal,
    pub(super) has_key: bool,
    pub(super) has_velocity: bool,
    pub(super) has_gain: bool,
    pub(super) has_modulation: bool,
}

impl NodeProgram {
    pub(super) fn compile(
        node: &GraphNode,
        parameters: &BTreeMap<&str, &Parameter>,
        instrument_id: &str,
        sample_rate: u32,
        controls: &CompiledControlProgram,
    ) -> Result<Self, EngineError> {
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
        Ok(Self(match node.kind {
            NodeKind::Oscillator => NodeProgramKind::Oscillator(OscillatorConfig {
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
            NodeKind::Unison => NodeProgramKind::Unison(UnisonConfig {
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
                voices: resolve_integer(node, "voices", 4, 1, oscillator::MAX_UNISON_VOICES)?,
                detune: number("detune", Unit::Semitones, 0.15)?,
                width: number("width", Unit::Ratio, 0.75)?,
                drift: number("drift", Unit::Semitones, 0.02)?,
                glide: number("glide", Unit::Seconds, 0.0)?,
                mod_depth: number("mod_depth", Unit::Semitones, 0.0)?,
                velocity_amount: number("velocity_amount", Unit::Ratio, 0.0)?,
                level: number("level", Unit::Ratio, 1.0)?,
            }),
            NodeKind::Noise => NodeProgramKind::Noise(NoiseConfig {
                level: number("level", Unit::Ratio, 1.0)?,
            }),
            NodeKind::Envelope => NodeProgramKind::Envelope(EnvelopeConfig {
                attack: number("attack", Unit::Seconds, 0.01)?,
                decay: number("decay", Unit::Seconds, 0.1)?,
                sustain: number("sustain", Unit::Ratio, 0.7)?,
                release: number("release", Unit::Seconds, 0.3)?,
            }),
            NodeKind::Mixer => NodeProgramKind::Mixer(MixerConfig {
                a_level: number("a_level", Unit::Ratio, 1.0)?,
                b_level: number("b_level", Unit::Ratio, 1.0)?,
            }),
            NodeKind::Lowpass => NodeProgramKind::Filter(FilterConfig {
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
            NodeKind::Amplifier => NodeProgramKind::Amplifier(AmplifierConfig {
                level: number("level", Unit::Ratio, 1.0)?,
            }),
            NodeKind::Saturation => NodeProgramKind::Saturation(SaturationConfig {
                drive: number("drive", Unit::Ratio, 1.0)?,
            }),
            NodeKind::Pan => NodeProgramKind::Pan(PanConfig {
                position: number("position", Unit::Pan, 0.0)?,
            }),
            NodeKind::Delay => {
                let seconds = number("time", Unit::Seconds, 0.25)?.constant;
                if !(0.001..=MAX_DELAY_SECONDS).contains(&seconds) {
                    return Err(EngineError::Compile(format!(
                        "delay time must be between 1ms and {MAX_DELAY_SECONDS}s"
                    )));
                }
                NodeProgramKind::Delay(DelayConfig {
                    delay_frames: (seconds * f64::from(sample_rate)).round() as usize,
                    feedback: number("feedback", Unit::Ratio, 0.25)?,
                    mix: number("mix", Unit::Ratio, 0.15)?,
                })
            }
            NodeKind::Reverb => NodeProgramKind::Reverb(ReverbConfig {
                room: number("room", Unit::Ratio, 0.5)?,
                damping: number("damping", Unit::Ratio, 0.4)?,
                mix: number("mix", Unit::Ratio, 0.15)?,
            }),
            NodeKind::Output => NodeProgramKind::Output,
            NodeKind::Lfo => NodeProgramKind::Lfo(LfoConfig {
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
        }))
    }

    pub(super) fn create_state(self, sample_rate: u32, seed: u64) -> NodeState {
        NodeState(match self.0 {
            NodeProgramKind::Oscillator(_) => NodeStateKind::Oscillator(OscillatorState::new(seed)),
            NodeProgramKind::Unison(_) => NodeStateKind::Unison(Box::new(UnisonState::new(seed))),
            NodeProgramKind::Noise(_) => NodeStateKind::Noise(NoiseState::new(seed)),
            NodeProgramKind::Envelope(_) => NodeStateKind::Envelope(EnvelopeState::default()),
            NodeProgramKind::Filter(_) => NodeStateKind::Filter(RuntimeFilterState::default()),
            NodeProgramKind::Delay(config) => {
                NodeStateKind::Delay(DelayState::new(config.delay_frames))
            }
            NodeProgramKind::Reverb(_) => NodeStateKind::Reverb(ReverbState::new(sample_rate)),
            NodeProgramKind::Lfo(_) => NodeStateKind::Lfo(LfoState::new(seed)),
            NodeProgramKind::Mixer(_)
            | NodeProgramKind::Amplifier(_)
            | NodeProgramKind::Saturation(_)
            | NodeProgramKind::Pan(_)
            | NodeProgramKind::Output => NodeStateKind::Stateless,
        })
    }

    /// Executes one Node; construction normally pairs each program with its state.
    pub(super) fn process(
        self,
        state: &mut NodeState,
        inputs: NodeInputs,
        controls: &[f64],
        sample_rate: f64,
    ) -> Signal {
        match (self.0, &mut state.0) {
            (NodeProgramKind::Oscillator(config), NodeStateKind::Oscillator(state)) => {
                Signal::Mono(config.process(state, inputs.pitch.control(), controls, sample_rate))
            }
            (NodeProgramKind::Unison(config), NodeStateKind::Unison(state)) => {
                Signal::Stereo(config.process(
                    state,
                    inputs.pitch.control(),
                    inputs.modulation.control(),
                    inputs.gain.control(),
                    inputs.velocity.control(),
                    inputs.has_modulation,
                    inputs.has_gain,
                    inputs.has_velocity,
                    controls,
                    sample_rate,
                ))
            }
            (NodeProgramKind::Noise(config), NodeStateKind::Noise(state)) => {
                Signal::Mono(config.process(state, controls))
            }
            (NodeProgramKind::Envelope(config), NodeStateKind::Envelope(state)) => {
                Signal::Control(config.process(state, inputs.gate.control(), controls, sample_rate))
            }
            (NodeProgramKind::Mixer(config), NodeStateKind::Stateless) => Signal::Mono(
                inputs.input_a.mono() * config.a_level.value(controls) as f32
                    + inputs.input_b.mono() * config.b_level.value(controls) as f32,
            ),
            (NodeProgramKind::Filter(config), NodeStateKind::Filter(state)) => {
                Signal::Mono(config.process(
                    state,
                    inputs.audio.mono(),
                    inputs.key.control(),
                    inputs.modulation.control(),
                    inputs.velocity.control(),
                    inputs.has_key,
                    inputs.has_modulation,
                    inputs.has_velocity,
                    controls,
                    sample_rate,
                ))
            }
            (NodeProgramKind::Amplifier(config), NodeStateKind::Stateless) => Signal::Mono(
                inputs.audio.mono() * inputs.gain.control() * config.level.value(controls) as f32,
            ),
            (NodeProgramKind::Saturation(config), NodeStateKind::Stateless) => {
                let input = inputs.audio.mono();
                let drive = config.drive.value(controls).clamp(0.0, 8.0) as f32;
                if drive == 0.0 {
                    Signal::Mono(input)
                } else {
                    let normalizer = drive.tanh().max(0.000_001);
                    Signal::Mono((input * drive).tanh() / normalizer)
                }
            }
            (NodeProgramKind::Pan(config), NodeStateKind::Stateless) => {
                let input = inputs.audio.mono();
                let angle = (config.position.value(controls).clamp(-1.0, 1.0) + 1.0) * PI * 0.25;
                Signal::Stereo([
                    (input as f64 * angle.cos()) as f32,
                    (input as f64 * angle.sin()) as f32,
                ])
            }
            (NodeProgramKind::Delay(config), NodeStateKind::Delay(state)) => {
                Signal::Stereo(config.process(state, inputs.audio.stereo(), controls))
            }
            (NodeProgramKind::Reverb(config), NodeStateKind::Reverb(state)) => {
                Signal::Stereo(config.process(state, inputs.audio.stereo(), controls))
            }
            (NodeProgramKind::Output, NodeStateKind::Stateless) => {
                Signal::Stereo(inputs.audio.stereo())
            }
            (NodeProgramKind::Lfo(config), NodeStateKind::Lfo(state)) => {
                Signal::Control(config.process(state, controls, sample_rate))
            }
            // Preserve callback-safe legacy behavior if an internal caller violates
            // the program/state pairing invariant.
            _ => Signal::None,
        }
    }

    #[cfg(test)]
    pub(super) const fn is_bandpass_filter(self) -> bool {
        matches!(
            self.0,
            NodeProgramKind::Filter(FilterConfig {
                mode: FilterMode::Bandpass,
                ..
            })
        )
    }
}

impl NodeState {
    pub(super) fn retrigger(&mut self, seed: u64) {
        match &mut self.0 {
            NodeStateKind::Oscillator(state) => state.retrigger(seed),
            NodeStateKind::Unison(state) => state.retrigger(seed),
            NodeStateKind::Noise(state) => state.retrigger(seed),
            NodeStateKind::Envelope(state) => *state = EnvelopeState::default(),
            NodeStateKind::Filter(state) => *state = RuntimeFilterState::default(),
            NodeStateKind::Lfo(state) => state.retrigger(seed),
            NodeStateKind::Delay(_) | NodeStateKind::Reverb(_) | NodeStateKind::Stateless => {}
        }
    }

    pub(super) fn envelope_idle(&self) -> bool {
        !matches!(&self.0, NodeStateKind::Envelope(state) if !state.idle())
    }

    #[cfg(test)]
    pub(super) fn heap_bytes(&self) -> u64 {
        match &self.0 {
            NodeStateKind::Delay(state) => state.heap_bytes(),
            NodeStateKind::Reverb(state) => state.heap_bytes(),
            NodeStateKind::Unison(_) => std::mem::size_of::<UnisonState>() as u64,
            _ => 0,
        }
    }
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
