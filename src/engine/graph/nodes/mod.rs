//! Closed native graph-node programs and preallocated state.

use std::{
    collections::{BTreeMap, BTreeSet},
    f64::consts::PI,
    sync::Arc,
};

use crate::{
    model::{
        ControlPolarity, GraphNode, Instrument, LfoDirection, LfoMode, LfoWaveform, NodeKind,
        Parameter, Project, SettingValue, Unit, parse_pitch,
    },
    time::{Meter, parse_musical_time},
    validate::MAX_STEREO_SUM_INPUTS,
};

use super::super::{
    EngineError,
    control::{
        CompiledControlProgram, ControlIndex, mix_modulation_identity, stable_modulation_identity,
    },
    sample::{CompiledSampleLibrary, CompiledSampleSource},
    spectral::{
        CompiledSpectralAnalysis, CompiledSpectralLibrary, MAX_SPECTRAL_PEAKS,
        SPECTRAL_ANALYSIS_ALGORITHM,
    },
    wavetable::{CompiledWavetable, CompiledWavetableLibrary},
};

mod effects;
mod envelope;
mod filter;
mod oscillator;
mod sample_source;
mod spectral;
mod wavetable;

use effects::MAX_DELAY_SECONDS;
#[cfg(test)]
pub(super) use filter::FilterState;
#[cfg(test)]
pub(super) use oscillator::slew_frequency;

pub(super) const MAX_RELEASE_SECONDS: f64 = envelope::MAX_RELEASE_SECONDS;

/// Returns the callback work represented by one authored node.
pub(super) fn authored_node_work_units(instrument: &Instrument, node: &GraphNode) -> u64 {
    match node.kind {
        NodeKind::Unison => authored_node_number(instrument, node, "voices", 4.0)
            .round()
            .clamp(1.0, oscillator::MAX_UNISON_VOICES as f64) as u64,
        NodeKind::AnalogOscillator | NodeKind::Wavetable => {
            let voices_key = if node.setting("unison").is_some() {
                "unison"
            } else {
                "voices"
            };
            let voices = authored_node_number(instrument, node, voices_key, 1.0)
                .round()
                .clamp(1.0, oscillator::MAX_UNISON_VOICES as f64) as u64;
            let flat_warp = u64::from(match node.setting("warp") {
                None => false,
                Some(SettingValue::Text(value)) => value != "none",
                Some(_) => true,
            });
            let structured_warps = node
                .directives
                .iter()
                .filter(|directive| {
                    directive
                        .tokens
                        .first()
                        .is_some_and(|token| token.value == "warp")
                })
                .count() as u64;
            let flat_distortion = match node.setting("distortion") {
                None => false,
                Some(SettingValue::Text(value)) => value != "none",
                Some(_) => true,
            };
            let distortion = u64::from(
                node.kind == NodeKind::AnalogOscillator
                    && (flat_distortion
                        || node.directives.iter().any(|directive| {
                            directive
                                .tokens
                                .first()
                                .is_some_and(|token| token.value == "distortion")
                        })),
            );
            let connected_audio_rate_ports = ["fm", "pm", "sync", "am"]
                .into_iter()
                .filter(|port| {
                    instrument
                        .graph
                        .iter()
                        .chain(instrument.shared_graph.iter())
                        .flat_map(|graph| &graph.connections)
                        .any(|connection| {
                            connection.to.node == node.id && connection.to.port == *port
                        })
                })
                .count() as u64;
            // Charge every enabled audio-rate operation per subvoice. FM is currently
            // shared before the subvoice loop, but this conservative law also covers
            // edge-triggered sync resetting every phase on its worst callback frame.
            voices.saturating_mul(
                // Charge the maximum 13-level mip scan plus four Catmull–Rom
                // frame reads even when this asset selects the cheaper linear law.
                (if node.kind == NodeKind::Wavetable {
                    40_u64
                } else {
                    1_u64
                })
                .saturating_add(flat_warp)
                .saturating_add(structured_warps)
                .saturating_add(distortion)
                .saturating_add(connected_audio_rate_ports),
            )
        }
        NodeKind::SampleSource => 8,
        NodeKind::Granular => {
            authored_node_number(instrument, node, "max_grains", 16.0)
                .round()
                .clamp(1.0, 64.0) as u64
                * 8
        }
        NodeKind::Spectral => {
            authored_node_number(instrument, node, "max_peaks", MAX_SPECTRAL_PEAKS as f64)
                .round()
                .clamp(1.0, MAX_SPECTRAL_PEAKS as f64) as u64
                * 8
        }
        NodeKind::Multisample => (node
            .directives
            .iter()
            .filter(|directive| {
                directive
                    .tokens
                    .first()
                    .is_some_and(|token| token.value == "zone")
            })
            .count() as u64)
            .max(1)
            .saturating_mul(8),
        NodeKind::StereoFilter => match node.setting("model") {
            Some(SettingValue::Text(model)) if model == "ladder_drive" => 16,
            _ => 2,
        },
        NodeKind::Lfo if matches!(node.setting("waveform"), Some(SettingValue::Text(value)) if value == "custom") => {
            1_u64.saturating_add(node.directives.len() as u64)
        }
        _ => 1,
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
        NodeKind::AnalogOscillator => {
            std::mem::size_of::<oscillator::AnalogOscillatorState>() as u64
        }
        NodeKind::Wavetable => std::mem::size_of::<wavetable::WavetableState>() as u64,
        NodeKind::SampleSource => sample_source::sample_source_state_bytes(),
        NodeKind::Multisample => sample_source::multisample_state_bytes(node.directives.len()),
        NodeKind::Granular => sample_source::granular_state_bytes(
            authored_node_number(instrument, node, "max_grains", 16.0)
                .round()
                .clamp(1.0, 64.0) as usize,
        ),
        NodeKind::Spectral => spectral::spectral_state_bytes(
            authored_node_number(instrument, node, "max_peaks", MAX_SPECTRAL_PEAKS as f64)
                .round()
                .clamp(1.0, MAX_SPECTRAL_PEAKS as f64) as usize,
        ),
        NodeKind::StereoFilter => std::mem::size_of::<filter::StereoFilterState>() as u64,
        _ => 0,
    }
}

fn authored_node_number(instrument: &Instrument, node: &GraphNode, key: &str, default: f64) -> f64 {
    match node.setting(key) {
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

#[derive(Debug)]
struct ParsedSampleSlice {
    id: String,
    start: u64,
    end: u64,
}

#[derive(Debug)]
struct ParsedMultisampleZone {
    id: String,
    asset: String,
    low_key: u8,
    high_key: u8,
    low_velocity: f64,
    high_velocity: f64,
    root_key: u8,
    layer: String,
    articulation: String,
    round_robin: String,
    round_robin_index: u16,
    region_start: u64,
    region_end: u64,
    gain_db: f64,
    pan: f64,
}

fn parse_sample_slices(node: &GraphNode) -> Result<Vec<ParsedSampleSlice>, EngineError> {
    node.directives
        .iter()
        .map(|directive| {
            if !directive.children.is_empty()
                || directive.tokens.len() != 4
                || directive.tokens[0].value != "slice"
            {
                return Err(EngineError::Compile(
                    "sample_source directives use `slice ID START_FRAME END_FRAME`".to_owned(),
                ));
            }
            let start = directive.tokens[2].value.parse::<u64>().map_err(|_| {
                EngineError::Compile("sample slice start must be an unsigned frame".to_owned())
            })?;
            let end = directive.tokens[3].value.parse::<u64>().map_err(|_| {
                EngineError::Compile("sample slice end must be an unsigned frame".to_owned())
            })?;
            if start >= end {
                return Err(EngineError::Compile(
                    "sample slice must be a positive half-open frame region".to_owned(),
                ));
            }
            Ok(ParsedSampleSlice {
                id: directive.tokens[1].value.clone(),
                start,
                end,
            })
        })
        .collect()
}

fn parse_multisample_zones(node: &GraphNode) -> Result<Vec<ParsedMultisampleZone>, EngineError> {
    node.directives
        .iter()
        .map(|directive| {
            let tokens = &directive.tokens;
            if !directive.children.is_empty()
                || tokens.len() != 26
                || tokens[0].value != "zone"
                || tokens[2].value != "asset"
                || tokens[4].value != "keys"
                || tokens[7].value != "velocity"
                || tokens[10].value != "root"
                || tokens[12].value != "layer"
                || tokens[14].value != "articulation"
                || tokens[16].value != "round_robin"
                || tokens[19].value != "region"
                || tokens[22].value != "gain"
                || tokens[24].value != "pan"
            {
                return Err(EngineError::Compile(
                    "multisample zone directive does not match the stable zone schema".to_owned(),
                ));
            }
            let parse_frame = |index: usize| {
                tokens[index].value.parse::<u64>().map_err(|_| {
                    EngineError::Compile("multisample region frames must be unsigned".to_owned())
                })
            };
            let parse_velocity = |index: usize| {
                Unit::Ratio
                    .parse_value(&tokens[index].value)
                    .map_err(|error| {
                        EngineError::Compile(format!("invalid zone velocity: {error}"))
                    })
            };
            let low_key = parse_pitch(&tokens[5].value)
                .map_err(|error| EngineError::Compile(error.to_string()))?;
            let high_key = parse_pitch(&tokens[6].value)
                .map_err(|error| EngineError::Compile(error.to_string()))?;
            let root_key = parse_pitch(&tokens[11].value)
                .map_err(|error| EngineError::Compile(error.to_string()))?;
            let low_velocity = parse_velocity(8)?;
            let high_velocity = parse_velocity(9)?;
            let round_robin_index = tokens[18].value.parse::<u16>().map_err(|_| {
                EngineError::Compile("round-robin index must be an unsigned integer".to_owned())
            })?;
            let region_start = parse_frame(20)?;
            let region_end = parse_frame(21)?;
            let gain_db = Unit::Decibels
                .parse_value(&tokens[23].value)
                .map_err(|error| EngineError::Compile(error.to_string()))?;
            let pan = Unit::Pan
                .parse_value(&tokens[25].value)
                .map_err(|error| EngineError::Compile(error.to_string()))?;
            if low_key > high_key
                || !(0.0..=1.0).contains(&low_velocity)
                || !(low_velocity..=1.0).contains(&high_velocity)
                || region_start >= region_end
            {
                return Err(EngineError::Compile(
                    "multisample zone has an invalid key, velocity, or region range".to_owned(),
                ));
            }
            Ok(ParsedMultisampleZone {
                id: tokens[1].value.clone(),
                asset: tokens[3].value.clone(),
                low_key,
                high_key,
                low_velocity,
                high_velocity,
                root_key,
                layer: tokens[13].value.clone(),
                articulation: tokens[15].value.clone(),
                round_robin: tokens[17].value.clone(),
                round_robin_index,
                region_start,
                region_end,
                gain_db,
                pan,
            })
        })
        .collect()
}

fn compiled_sample_source(
    context: NodeCompileContext<'_>,
    asset_id: &str,
    minimum_frames: usize,
) -> Result<CompiledSampleSource, EngineError> {
    if let Some(samples) = context.samples {
        return samples.get(asset_id).cloned().ok_or_else(|| {
            EngineError::Compile(format!("sample asset `{asset_id}` was not compiled"))
        });
    }
    let declaration = context
        .project
        .assets
        .iter()
        .find(|asset| asset.id == asset_id)
        .ok_or_else(|| EngineError::Compile(format!("missing sample asset `{asset_id}`")))?;
    CompiledSampleSource::placeholder(declaration, context.sample_rate, minimum_frames)
        .map_err(|error| EngineError::Compile(error.to_string()))
}

fn compiled_spectral_analysis(
    context: NodeCompileContext<'_>,
    asset_id: &str,
) -> Result<CompiledSpectralAnalysis, EngineError> {
    if let Some(spectral) = context.spectral {
        return spectral.get(asset_id).cloned().ok_or_else(|| {
            EngineError::Compile(format!("spectral asset `{asset_id}` was not analyzed"))
        });
    }
    let source = compiled_sample_source(context, asset_id, 1)?;
    CompiledSpectralAnalysis::placeholder(
        source.asset_id(),
        source.source_sha256(),
        source.cache_identity(),
        context.sample_rate,
    )
    .map_err(|error| EngineError::Compile(error.to_string()))
}

use effects::{DelayConfig, DelayState, ReverbConfig, ReverbState};
use envelope::{EnvelopeConfig, EnvelopeRetrigger, EnvelopeState};
use filter::{
    FilterConfig, FilterMode, FilterState as RuntimeFilterState, StereoFilterConfig,
    StereoFilterModel, StereoFilterState,
};
use oscillator::{
    AnalogAmplitudeMode, AnalogDistortion, AnalogOscillatorConfig, AnalogOscillatorState,
    AnalogPhaseWarp, AnalogWarpStage, GraphLfoRetrigger, LfoConfig, LfoState, NoiseConfig,
    NoiseState, OscillatorConfig, OscillatorState, OscillatorWaveform, UnisonConfig, UnisonState,
};
use sample_source::{
    GrainEdge, GrainWindow, GranularConfig, GranularState, MultisampleConfig,
    MultisampleRoundRobinGroup, MultisampleState, MultisampleZone,
    SampleDirection as GraphSampleDirection, SampleGateMode, SampleLoopMode, SampleSourceConfig,
    SampleSourceState,
};
use spectral::{SpectralConfig, SpectralState, SpectralTransientPolicy};
use wavetable::{WavetableConfig, WavetableState};

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
#[derive(Debug, Clone)]
pub(super) struct NodeProgram(NodeProgramKind);

#[derive(Debug, Clone)]
enum NodeProgramKind {
    Oscillator(OscillatorConfig),
    Unison(UnisonConfig),
    AnalogOscillator(Box<AnalogOscillatorConfig>),
    Wavetable(Box<WavetableConfig>),
    SampleSource(Box<SampleSourceConfig>),
    Multisample(Box<MultisampleConfig>),
    Granular(Box<GranularConfig>),
    Spectral(Box<SpectralConfig>),
    Noise(NoiseConfig),
    Envelope(EnvelopeConfig),
    Mixer(MixerConfig),
    MonoToStereo,
    StereoToMono,
    StereoMixer(MixerConfig),
    StereoSum,
    Filter(FilterConfig),
    StereoFilter(StereoFilterConfig),
    Amplifier(AmplifierConfig),
    Saturation(SaturationConfig),
    Pan(PanConfig),
    Delay(DelayConfig),
    Reverb(ReverbConfig),
    Output,
    Lfo(Box<LfoConfig>),
}

/// Mutable state paired with one [`NodeProgram`] variant at voice creation.
#[derive(Debug, Clone)]
pub(super) struct NodeState(NodeStateKind, NodeSeedIdentity);

#[derive(Debug, Clone, Copy)]
struct NodeSeedIdentity {
    legacy_index: u64,
    semantic_identity: Option<u64>,
}

impl NodeSeedIdentity {
    fn resolve(self, voice_seed: u64) -> u64 {
        self.semantic_identity
            .map_or(voice_seed ^ self.legacy_index, |identity| {
                mix_modulation_identity(voice_seed, identity)
            })
    }
}

#[derive(Debug, Clone)]
enum NodeStateKind {
    Oscillator(OscillatorState),
    Unison(Box<UnisonState>),
    AnalogOscillator(Box<AnalogOscillatorState>),
    Wavetable(Box<WavetableState>),
    SampleSource(SampleSourceState),
    Multisample(Box<MultisampleState>),
    Granular(Box<GranularState>),
    Spectral(Box<SpectralState>),
    Noise(NoiseState),
    Envelope(EnvelopeState),
    Filter(RuntimeFilterState),
    StereoFilter(Box<StereoFilterState>),
    Delay(DelayState),
    Reverb(ReverbState),
    Lfo(LfoState),
    Stateless,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NodeInputs {
    pub(super) song_frame: u64,
    pub(super) pitch: Signal,
    pub(super) key: Signal,
    pub(super) gate: Signal,
    pub(super) velocity: Signal,
    pub(super) audio: Signal,
    pub(super) gain: Signal,
    pub(super) modulation: Signal,
    pub(super) frequency_modulation: Signal,
    pub(super) phase_modulation: Signal,
    pub(super) sync: Signal,
    pub(super) amplitude_modulation: Signal,
    pub(super) input_a: Signal,
    pub(super) input_b: Signal,
    pub(super) summed_audio: Signal,
    pub(super) has_key: bool,
    pub(super) has_velocity: bool,
    pub(super) has_gain: bool,
    pub(super) has_modulation: bool,
    pub(super) has_frequency_modulation: bool,
    pub(super) has_phase_modulation: bool,
    pub(super) has_sync: bool,
    pub(super) has_amplitude_modulation: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NodeCompileContext<'a> {
    pub(super) sample_rate: u32,
    pub(super) tempo_bpm: f64,
    pub(super) meter: Meter,
    pub(super) project: &'a Project,
    pub(super) samples: Option<&'a CompiledSampleLibrary>,
    pub(super) spectral: Option<&'a CompiledSpectralLibrary>,
    pub(super) wavetables: Option<&'a CompiledWavetableLibrary>,
}

impl NodeProgram {
    pub(super) fn compile(
        node: &GraphNode,
        parameters: &BTreeMap<&str, &Parameter>,
        instrument_id: &str,
        context: NodeCompileContext<'_>,
        controls: &CompiledControlProgram,
    ) -> Result<Self, EngineError> {
        let sample_rate = context.sample_rate;
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
            NodeKind::AnalogOscillator => {
                let mut ordered_warps = Vec::with_capacity(3);
                let flat_warp = analog_warp(text(node, "warp", "none"))?;
                if flat_warp != AnalogPhaseWarp::None {
                    let site = node.setting_site("warp");
                    ordered_warps.push((
                        (
                            site.map_or_else(
                                || node.location.file.clone(),
                                |site| site.file.clone(),
                            ),
                            site.map_or(node.location.start, |site| site.start),
                        ),
                        AnalogWarpStage {
                            kind: flat_warp,
                            amount: number("warp_amount", Unit::Ratio, 0.0)?,
                        },
                    ));
                }
                let mut distortion = analog_distortion(text(node, "distortion", "none"))?;
                let mut drive = number("drive", Unit::Ratio, 0.0)?;
                for directive in &node.directives {
                    if !directive.children.is_empty() || directive.tokens.len() != 5 {
                        return Err(EngineError::Compile(
                            "analog oscillator directives require five tokens and no children"
                                .to_owned(),
                        ));
                    }
                    let tokens = &directive.tokens;
                    if tokens[3].value != "amount" {
                        return Err(EngineError::Compile(
                            "analog oscillator directive requires `amount VALUE`".to_owned(),
                        ));
                    }
                    match tokens[0].value.as_str() {
                        "warp" => {
                            ordered_warps.push((
                                (tokens[0].site.file.clone(), tokens[0].site.start),
                                AnalogWarpStage {
                                    kind: analog_warp(&tokens[2].value)?,
                                    amount: resolve_directive_number(
                                        &tokens[4].value,
                                        Unit::Ratio,
                                        parameters,
                                        instrument_id,
                                        controls,
                                    )?,
                                },
                            ));
                        }
                        "distortion" => {
                            if distortion != AnalogDistortion::None {
                                return Err(EngineError::Compile(
                                    "analog oscillator supports one post-read distortion"
                                        .to_owned(),
                                ));
                            }
                            distortion = analog_distortion(&tokens[2].value)?;
                            drive = resolve_directive_number(
                                &tokens[4].value,
                                Unit::Ratio,
                                parameters,
                                instrument_id,
                                controls,
                            )?;
                        }
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported analog oscillator directive `{value}`"
                            )));
                        }
                    }
                }
                ordered_warps.sort_by(|left, right| left.0.cmp(&right.0));
                if ordered_warps.len() > 2 {
                    return Err(EngineError::Compile(
                        "analog oscillator supports at most two ordered warps".to_owned(),
                    ));
                }
                let mut warps = [None, None];
                for (slot, (_, stage)) in warps.iter_mut().zip(ordered_warps) {
                    *slot = Some(stage);
                }
                let voices_key = if node.setting("unison").is_some() {
                    "unison"
                } else {
                    "voices"
                };
                NodeProgramKind::AnalogOscillator(Box::new(AnalogOscillatorConfig {
                    waveform: match text(node, "waveform", "saw") {
                        "sine" => OscillatorWaveform::Sine,
                        "saw" => OscillatorWaveform::Saw,
                        "square" | "pulse" => OscillatorWaveform::Square,
                        "triangle" => OscillatorWaveform::Triangle,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported analog oscillator waveform `{value}`"
                            )));
                        }
                    },
                    voices: resolve_integer(node, voices_key, 1, 1, oscillator::MAX_UNISON_VOICES)?,
                    coarse: number("coarse", Unit::Semitones, 0.0)?,
                    fine: number("fine", Unit::Semitones, 0.0)?,
                    detune: number("detune", Unit::Semitones, 0.15)?,
                    blend: number("blend", Unit::Ratio, 1.0)?,
                    width: number("width", Unit::Ratio, 0.75)?,
                    drift: number("drift", Unit::Semitones, 0.02)?,
                    glide: number("glide", Unit::Seconds, 0.0)?,
                    fm_depth: number("fm_depth", Unit::Semitones, 0.0)?,
                    pm_depth: number("pm_depth", Unit::Ratio, 0.0)?,
                    amplitude_mode: match text(node, "am_mode", "am") {
                        "am" => AnalogAmplitudeMode::Am,
                        "ring" => AnalogAmplitudeMode::Ring,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported analog oscillator amplitude mode `{value}`"
                            )));
                        }
                    },
                    amplitude_depth: number("am_depth", Unit::Ratio, 1.0)?,
                    phase: number("phase", Unit::Ratio, 0.0)?,
                    random_phase: number("random_phase", Unit::Ratio, 0.0)?,
                    warps,
                    distortion,
                    drive,
                    velocity_amount: number("velocity_amount", Unit::Ratio, 0.0)?,
                    level: number("level", Unit::Ratio, 1.0)?,
                    pan: number("pan", Unit::Ratio, 0.0)?,
                    node_identity: stable_modulation_identity(&node.id),
                }))
            }
            NodeKind::Wavetable => {
                let asset_id = text(node, "asset", "");
                let declaration = context
                    .project
                    .assets
                    .iter()
                    .find(|asset| asset.id == asset_id)
                    .ok_or_else(|| {
                        EngineError::Compile(format!(
                            "wavetable Node `{}` references missing asset `{asset_id}`",
                            node.id
                        ))
                    })?;
                let definition = declaration.wavetable.as_ref().ok_or_else(|| {
                    EngineError::Compile(format!("asset `{asset_id}` has no wavetable definition"))
                })?;
                let table = if let Some(library) = context.wavetables {
                    Arc::clone(library.get(asset_id).ok_or_else(|| {
                        EngineError::Compile(format!(
                            "wavetable asset `{asset_id}` was not compiled"
                        ))
                    })?)
                } else {
                    Arc::new(
                        CompiledWavetable::placeholder(declaration, definition, sample_rate)
                            .map_err(|error| EngineError::Compile(error.to_string()))?,
                    )
                };
                let mut ordered_warps = Vec::with_capacity(3);
                let flat_warp = analog_warp(text(node, "warp", "none"))?;
                if flat_warp != AnalogPhaseWarp::None {
                    let site = node.setting_site("warp");
                    ordered_warps.push((
                        (
                            site.map_or_else(
                                || node.location.file.clone(),
                                |site| site.file.clone(),
                            ),
                            site.map_or(node.location.start, |site| site.start),
                        ),
                        AnalogWarpStage {
                            kind: flat_warp,
                            amount: number("warp_amount", Unit::Ratio, 0.0)?,
                        },
                    ));
                }
                for directive in &node.directives {
                    let tokens = &directive.tokens;
                    if !directive.children.is_empty()
                        || tokens.len() != 5
                        || tokens[0].value != "warp"
                        || tokens[3].value != "amount"
                    {
                        return Err(EngineError::Compile(
                            "wavetable directives use `warp ID KIND amount VALUE`".to_owned(),
                        ));
                    }
                    ordered_warps.push((
                        (tokens[0].site.file.clone(), tokens[0].site.start),
                        AnalogWarpStage {
                            kind: analog_warp(&tokens[2].value)?,
                            amount: resolve_directive_number(
                                &tokens[4].value,
                                Unit::Ratio,
                                parameters,
                                instrument_id,
                                controls,
                            )?,
                        },
                    ));
                }
                ordered_warps.sort_by(|left, right| left.0.cmp(&right.0));
                if ordered_warps.len() > 2 {
                    return Err(EngineError::Compile(
                        "wavetable oscillator supports at most two ordered warps".to_owned(),
                    ));
                }
                let mut warps = [None, None];
                for (slot, (_, stage)) in warps.iter_mut().zip(ordered_warps) {
                    *slot = Some(stage);
                }
                let voices_key = if node.setting("unison").is_some() {
                    "unison"
                } else {
                    "voices"
                };
                NodeProgramKind::Wavetable(Box::new(WavetableConfig {
                    table,
                    voices: resolve_integer(node, voices_key, 1, 1, oscillator::MAX_UNISON_VOICES)?,
                    coarse: number("coarse", Unit::Semitones, 0.0)?,
                    fine: number("fine", Unit::Semitones, 0.0)?,
                    table_position: number("table_position", Unit::Ratio, 0.0)?,
                    detune: number("detune", Unit::Semitones, 0.15)?,
                    blend: number("blend", Unit::Ratio, 1.0)?,
                    width: number("width", Unit::Ratio, 0.75)?,
                    drift: number("drift", Unit::Semitones, 0.02)?,
                    glide: number("glide", Unit::Seconds, 0.0)?,
                    fm_depth: number("fm_depth", Unit::Semitones, 0.0)?,
                    pm_depth: number("pm_depth", Unit::Ratio, 0.0)?,
                    phase: number("phase", Unit::Ratio, 0.0)?,
                    random_phase: number("random_phase", Unit::Ratio, 0.0)?,
                    warps,
                    velocity_amount: number("velocity_amount", Unit::Ratio, 0.0)?,
                    level: number("level", Unit::Ratio, 1.0)?,
                    pan: number("pan", Unit::Ratio, 0.0)?,
                    node_identity: stable_modulation_identity(&node.id),
                }))
            }
            NodeKind::SampleSource => {
                let slices = parse_sample_slices(node)?;
                if slices.len() > sample_source::MAX_SAMPLE_SLICES {
                    return Err(EngineError::Compile(format!(
                        "sample_source supports at most {} slices",
                        sample_source::MAX_SAMPLE_SLICES
                    )));
                }
                let minimum_frames = slices.iter().map(|slice| slice.end).max().unwrap_or(1);
                let asset_id = text(node, "asset", "");
                let source = compiled_sample_source(
                    context,
                    asset_id,
                    usize::try_from(minimum_frames).map_err(|_| {
                        EngineError::Compile(
                            "sample slice frame exceeds addressable storage".to_owned(),
                        )
                    })?,
                )?;
                let selected_slice = text(node, "selected_slice", "");
                let selected = if selected_slice.is_empty() {
                    slices.first()
                } else {
                    slices.iter().find(|slice| slice.id == selected_slice)
                };
                if !selected_slice.is_empty() && selected.is_none() {
                    return Err(EngineError::Compile(format!(
                        "sample_source selected_slice `{selected_slice}` is not declared"
                    )));
                }
                let frame_scale =
                    f64::from(source.sample_rate()) / f64::from(source.source_sample_rate());
                let (slice_id, slice_start, slice_end) = selected.map_or_else(
                    || (None, 0.0, source.len() as f64),
                    |slice| {
                        (
                            Some(slice.id.clone()),
                            slice.start as f64 * frame_scale,
                            slice.end as f64 * frame_scale,
                        )
                    },
                );
                if slice_start >= slice_end || slice_end > source.len() as f64 + 1.0e-9 {
                    return Err(EngineError::Compile(format!(
                        "sample_source slice is outside compiled asset `{asset_id}`"
                    )));
                }
                let loop_mode = match text(node, "loop_mode", "off") {
                    "off" => SampleLoopMode::Off,
                    "forward" => SampleLoopMode::Forward,
                    "ping_pong" => SampleLoopMode::PingPong,
                    value => {
                        return Err(EngineError::Compile(format!(
                            "unsupported sample loop mode `{value}`"
                        )));
                    }
                };
                let gate_mode = match text(node, "playback", "gated") {
                    "gated" => SampleGateMode::Gated,
                    "one_shot" => SampleGateMode::OneShot,
                    value => {
                        return Err(EngineError::Compile(format!(
                            "unsupported sample playback mode `{value}`"
                        )));
                    }
                };
                if gate_mode == SampleGateMode::OneShot && loop_mode != SampleLoopMode::Off {
                    return Err(EngineError::Compile(
                        "one-shot sample_source playback cannot enable a loop".to_owned(),
                    ));
                }
                NodeProgramKind::SampleSource(Box::new(SampleSourceConfig {
                    source,
                    slice_id,
                    slice_start,
                    slice_end,
                    start: number("start", Unit::Ratio, 0.0)?,
                    end: number("end", Unit::Ratio, 1.0)?,
                    loop_mode,
                    loop_start: number("loop_start", Unit::Ratio, 0.0)?,
                    loop_end: number("loop_end", Unit::Ratio, 1.0)?,
                    loop_crossfade: resolve_integer(node, "loop_crossfade", 0, 0, 8_192)? as f64
                        * frame_scale,
                    direction: match text(node, "direction", "forward") {
                        "forward" => GraphSampleDirection::Forward,
                        "reverse" => GraphSampleDirection::Reverse,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported sample direction `{value}`"
                            )));
                        }
                    },
                    gate_mode,
                    root_key: parse_pitch(text(node, "root", "C4"))
                        .map_err(|error| EngineError::Compile(error.to_string()))?,
                    tune: number("tune", Unit::Semitones, 0.0)?,
                    rate: number("rate", Unit::Ratio, 1.0)?,
                    velocity_amount: number("velocity_amount", Unit::Ratio, 0.0)?,
                    level: number("level", Unit::Ratio, 1.0)?,
                    pan: number("pan", Unit::Pan, 0.0)?,
                }))
            }
            NodeKind::Multisample => {
                let parsed = parse_multisample_zones(node)?;
                if parsed.is_empty() || parsed.len() > sample_source::MAX_MULTISAMPLE_ZONES {
                    return Err(EngineError::Compile(format!(
                        "multisample requires 1 to {} zones",
                        sample_source::MAX_MULTISAMPLE_ZONES
                    )));
                }
                let mut zone_ids = BTreeSet::new();
                let mut zones = Vec::with_capacity(parsed.len());
                let mut grouped = BTreeMap::<(String, String), Vec<(u16, u16)>>::new();
                for authored in parsed {
                    if !zone_ids.insert(authored.id.clone()) {
                        return Err(EngineError::Compile(format!(
                            "duplicate multisample zone `{}`",
                            authored.id
                        )));
                    }
                    if authored.articulation != "default" {
                        return Err(EngineError::Compile(format!(
                            "multisample zone `{}` uses unsupported articulation `{}`",
                            authored.id, authored.articulation
                        )));
                    }
                    let minimum_frames = usize::try_from(authored.region_end).map_err(|_| {
                        EngineError::Compile("zone frame exceeds addressable storage".to_owned())
                    })?;
                    let source = compiled_sample_source(context, &authored.asset, minimum_frames)?;
                    let frame_scale =
                        f64::from(source.sample_rate()) / f64::from(source.source_sample_rate());
                    let region_start = authored.region_start as f64 * frame_scale;
                    let region_end = authored.region_end as f64 * frame_scale;
                    if region_start >= region_end || region_end > source.len() as f64 + 1.0e-9 {
                        return Err(EngineError::Compile(format!(
                            "multisample zone `{}` is outside asset `{}`",
                            authored.id, authored.asset
                        )));
                    }
                    let index = u16::try_from(zones.len()).map_err(|_| {
                        EngineError::Compile("multisample zone index exceeds u16".to_owned())
                    })?;
                    grouped
                        .entry((authored.layer.clone(), authored.round_robin.clone()))
                        .or_default()
                        .push((authored.round_robin_index, index));
                    zones.push(MultisampleZone {
                        id: authored.id,
                        source,
                        low_key: authored.low_key,
                        high_key: authored.high_key,
                        low_velocity: authored.low_velocity,
                        high_velocity: authored.high_velocity,
                        root_key: authored.root_key,
                        layer: authored.layer,
                        articulation: authored.articulation,
                        round_robin: authored.round_robin,
                        round_robin_index: authored.round_robin_index,
                        region_start,
                        region_end,
                        gain: 10.0_f64.powf(authored.gain_db / 20.0),
                        pan: authored.pan,
                    });
                }
                let mut groups = Vec::with_capacity(grouped.len());
                for ((layer, group), mut indexes) in grouped {
                    indexes.sort_unstable_by_key(|(round_robin, _)| *round_robin);
                    if indexes.windows(2).any(|pair| pair[0].0 == pair[1].0) {
                        return Err(EngineError::Compile(format!(
                            "multisample round-robin group `{group}` repeats an index"
                        )));
                    }
                    groups.push(MultisampleRoundRobinGroup {
                        identity: stable_modulation_identity(&format!(
                            "{}:{layer}:{group}",
                            node.id
                        )),
                        zone_indexes: indexes.into_iter().map(|(_, index)| index).collect(),
                    });
                }
                NodeProgramKind::Multisample(Box::new(MultisampleConfig {
                    zones,
                    groups,
                    gate_mode: match text(node, "playback", "gated") {
                        "gated" => SampleGateMode::Gated,
                        "one_shot" => SampleGateMode::OneShot,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported multisample playback mode `{value}`"
                            )));
                        }
                    },
                    tune: number("tune", Unit::Semitones, 0.0)?,
                    rate: number("rate", Unit::Ratio, 1.0)?,
                    velocity_amount: number("velocity_amount", Unit::Ratio, 0.0)?,
                    level: number("level", Unit::Ratio, 1.0)?,
                    pan: number("pan", Unit::Pan, 0.0)?,
                }))
            }
            NodeKind::Granular => {
                let asset_id = text(node, "asset", "");
                let source = compiled_sample_source(context, asset_id, 1)?;
                NodeProgramKind::Granular(Box::new(GranularConfig {
                    source,
                    max_grains: resolve_integer(node, "max_grains", 16, 1, 64)?,
                    position: number("position", Unit::Ratio, 0.5)?,
                    scan: number("scan", Unit::Ratio, 0.0)?,
                    size: number("size", Unit::Seconds, 0.1)?,
                    density: number("density", Unit::Hertz, 12.0)?,
                    root_key: parse_pitch(text(node, "root", "C4"))
                        .map_err(|error| EngineError::Compile(error.to_string()))?,
                    tune: number("tune", Unit::Semitones, 0.0)?,
                    spray: number("spray", Unit::Ratio, 0.0)?,
                    window: match text(node, "window", "hann") {
                        "hann" => GrainWindow::Hann,
                        "triangle" => GrainWindow::Triangle,
                        "tukey" => GrainWindow::Tukey,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported grain window `{value}`"
                            )));
                        }
                    },
                    edge: match text(node, "edge", "wrap") {
                        "wrap" => GrainEdge::Wrap,
                        "reflect" => GrainEdge::Reflect,
                        "clamp" => GrainEdge::Clamp,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported grain edge mode `{value}`"
                            )));
                        }
                    },
                    direction_probability: number("direction_probability", Unit::Ratio, 0.0)?,
                    stereo_spread: number("stereo_spread", Unit::Ratio, 0.5)?,
                    freeze: number("freeze", Unit::Boolean, 0.0)?,
                    level: number("level", Unit::Ratio, 1.0)?,
                    seed: resolve_integer(node, "seed", 0, 0, usize::from(u16::MAX))? as u64,
                    node_identity: stable_modulation_identity(&node.id),
                }))
            }
            NodeKind::Noise => NodeProgramKind::Noise(NoiseConfig {
                level: number("level", Unit::Ratio, 1.0)?,
            }),
            NodeKind::Envelope => {
                let hold = match text(node, "mode", "adsr") {
                    "adsr" => {
                        if node.setting("hold").is_some() {
                            return Err(EngineError::Compile(
                                "an adsr envelope cannot author a hold segment; use mode ahdsr"
                                    .to_owned(),
                            ));
                        }
                        Scalar::constant(0.0)
                    }
                    "ahdsr" => resolve_duration(
                        node,
                        "hold",
                        0.0,
                        parameters,
                        instrument_id,
                        controls,
                        context,
                    )?,
                    value => {
                        return Err(EngineError::Compile(format!(
                            "unsupported envelope mode `{value}`"
                        )));
                    }
                };
                NodeProgramKind::Envelope(EnvelopeConfig {
                    attack: resolve_duration(
                        node,
                        "attack",
                        0.01,
                        parameters,
                        instrument_id,
                        controls,
                        context,
                    )?,
                    hold,
                    decay: resolve_duration(
                        node,
                        "decay",
                        0.1,
                        parameters,
                        instrument_id,
                        controls,
                        context,
                    )?,
                    sustain: number("sustain", Unit::Ratio, 0.7)?,
                    release: resolve_duration(
                        node,
                        "release",
                        0.3,
                        parameters,
                        instrument_id,
                        controls,
                        context,
                    )?,
                    attack_curve: number("attack_curve", Unit::Ratio, 0.0)?,
                    decay_curve: number("decay_curve", Unit::Ratio, 0.0)?,
                    release_curve: number("release_curve", Unit::Ratio, 0.0)?,
                    retrigger: match text(node, "retrigger", "legato") {
                        "always" => EnvelopeRetrigger::Always,
                        "legato" => EnvelopeRetrigger::Legato,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported envelope retrigger policy `{value}`"
                            )));
                        }
                    },
                })
            }
            NodeKind::Mixer => NodeProgramKind::Mixer(MixerConfig {
                a_level: number("a_level", Unit::Ratio, 1.0)?,
                b_level: number("b_level", Unit::Ratio, 1.0)?,
            }),
            NodeKind::MonoToStereo => NodeProgramKind::MonoToStereo,
            NodeKind::StereoToMono => NodeProgramKind::StereoToMono,
            NodeKind::StereoMixer => NodeProgramKind::StereoMixer(MixerConfig {
                a_level: number("a_level", Unit::Ratio, 1.0)?,
                b_level: number("b_level", Unit::Ratio, 1.0)?,
            }),
            NodeKind::StereoSum => {
                resolve_integer(
                    node,
                    "max_inputs",
                    MAX_STEREO_SUM_INPUTS,
                    1,
                    MAX_STEREO_SUM_INPUTS,
                )?;
                NodeProgramKind::StereoSum
            }
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
            NodeKind::StereoFilter => {
                let model = match text(node, "model", "svf_clean") {
                    "svf_clean" => StereoFilterModel::SvfClean,
                    "ladder_drive" => StereoFilterModel::LadderDrive,
                    value => {
                        return Err(EngineError::Compile(format!(
                            "unsupported stereo filter model `{value}`"
                        )));
                    }
                };
                let mode = match text(node, "mode", "lowpass") {
                    "lowpass" => FilterMode::Lowpass,
                    "highpass" => FilterMode::Highpass,
                    "bandpass" => FilterMode::Bandpass,
                    "notch" => FilterMode::Notch,
                    value => {
                        return Err(EngineError::Compile(format!(
                            "unsupported stereo filter mode `{value}`"
                        )));
                    }
                };
                if model == StereoFilterModel::LadderDrive && mode != FilterMode::Lowpass {
                    return Err(EngineError::Compile(
                        "ladder_drive supports only lowpass mode; use svf_clean for other modes"
                            .to_owned(),
                    ));
                }
                NodeProgramKind::StereoFilter(StereoFilterConfig {
                    model,
                    mode,
                    cutoff: number("cutoff", Unit::Hertz, 8_000.0)?,
                    resonance: number("resonance", Unit::Ratio, 0.2)?,
                    drive: number("drive", Unit::Ratio, 0.0)?,
                    mix: number("mix", Unit::Ratio, 1.0)?,
                    pan: number("pan", Unit::Pan, 0.0)?,
                    mod_depth: number("mod_depth", Unit::Hertz, 0.0)?,
                    velocity_depth: number("velocity_depth", Unit::Hertz, 0.0)?,
                    key_track: number("key_track", Unit::Ratio, 0.0)?,
                    key_reference: number("key_reference", Unit::Hertz, 261.625_565)?,
                })
            }
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
            NodeKind::Lfo => {
                let (custom_points, custom_point_count) = compile_lfo_points(node)?;
                NodeProgramKind::Lfo(Box::new(LfoConfig {
                    waveform: match text(node, "waveform", "sine") {
                        "sine" => LfoWaveform::Sine,
                        "triangle" => LfoWaveform::Triangle,
                        "saw" => LfoWaveform::Saw,
                        "square" => LfoWaveform::Square,
                        "sample_hold" | "custom" => LfoWaveform::SampleHold,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported LFO waveform `{value}`"
                            )));
                        }
                    },
                    custom_points,
                    custom_point_count,
                    rate: resolve_lfo_rate(
                        node,
                        "rate",
                        1.0,
                        parameters,
                        instrument_id,
                        controls,
                        context,
                    )?,
                    depth: number("depth", Unit::Ratio, 1.0)?,
                    phase: number("phase", Unit::Ratio, 0.0)?,
                    random_phase: number("random_phase", Unit::Ratio, 1.0)?,
                    swing: number("swing", Unit::Ratio, 0.0)?,
                    mode: match text(node, "mode", "loop") {
                        "loop" => LfoMode::Loop,
                        "one_shot" => LfoMode::OneShot,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported LFO mode `{value}`"
                            )));
                        }
                    },
                    direction: match text(node, "direction", "forward") {
                        "forward" => LfoDirection::Forward,
                        "reverse" => LfoDirection::Reverse,
                        "ping_pong" => LfoDirection::PingPong,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported LFO direction `{value}`"
                            )));
                        }
                    },
                    polarity: match text(node, "polarity", "bipolar") {
                        "unipolar" => ControlPolarity::Unipolar,
                        "bipolar" => ControlPolarity::Bipolar,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported LFO polarity `{value}`"
                            )));
                        }
                    },
                    retrigger: match text(node, "retrigger", "note") {
                        "note" => GraphLfoRetrigger::Note,
                        "transport" => GraphLfoRetrigger::Transport,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported LFO retrigger policy `{value}`"
                            )));
                        }
                    },
                }))
            }
            NodeKind::Spectral => {
                let algorithm = text(node, "algorithm", "");
                if algorithm != SPECTRAL_ANALYSIS_ALGORITHM {
                    return Err(EngineError::Compile(format!(
                        "spectral source requires algorithm `{SPECTRAL_ANALYSIS_ALGORITHM}`"
                    )));
                }
                let asset_id = text(node, "asset", "");
                let analysis = compiled_spectral_analysis(context, asset_id)?;
                if analysis.sample_rate() != context.sample_rate {
                    return Err(EngineError::Compile(
                        "spectral analysis sample rate does not match the Project".to_owned(),
                    ));
                }
                NodeProgramKind::Spectral(Box::new(SpectralConfig {
                    analysis,
                    max_peaks: resolve_integer(
                        node,
                        "max_peaks",
                        MAX_SPECTRAL_PEAKS,
                        1,
                        MAX_SPECTRAL_PEAKS,
                    )?,
                    scan: number("scan", Unit::Ratio, 0.0)?,
                    rate: number("rate", Unit::Ratio, 1.0)?,
                    root_key: parse_pitch(text(node, "root", "C4"))
                        .map_err(|error| EngineError::Compile(error.to_string()))?,
                    tune: number("tune", Unit::Semitones, 0.0)?,
                    low: number("low", Unit::Hertz, 20.0)?,
                    high: number("high", Unit::Hertz, 20_000.0)?,
                    transients: match text(node, "transients", "preserve") {
                        "preserve" => SpectralTransientPolicy::Preserve,
                        "reset" => SpectralTransientPolicy::Reset,
                        "suppress" => SpectralTransientPolicy::Suppress,
                        value => {
                            return Err(EngineError::Compile(format!(
                                "unsupported spectral transient policy `{value}`"
                            )));
                        }
                    },
                    phase_randomization: number("phase_randomization", Unit::Ratio, 0.0)?,
                    freeze: number("freeze", Unit::Boolean, 0.0)?,
                    level: number("level", Unit::Ratio, 1.0)?,
                    seed: resolve_integer(node, "seed", 0, 0, usize::from(u16::MAX))? as u64,
                    node_identity: stable_modulation_identity(&node.id),
                }))
            }
        }))
    }

    pub(super) fn create_state(
        &self,
        sample_rate: u32,
        voice_seed: u64,
        legacy_index: u64,
    ) -> NodeState {
        let semantic_identity = match &self.0 {
            NodeProgramKind::Granular(config) => Some(config.node_identity),
            NodeProgramKind::Spectral(config) => Some(config.node_identity),
            _ => None,
        };
        let seed_identity = NodeSeedIdentity {
            legacy_index,
            semantic_identity,
        };
        let seed = seed_identity.resolve(voice_seed);
        NodeState(
            match &self.0 {
                NodeProgramKind::Oscillator(_) => {
                    NodeStateKind::Oscillator(OscillatorState::new(seed))
                }
                NodeProgramKind::Unison(_) => {
                    NodeStateKind::Unison(Box::new(UnisonState::new(seed)))
                }
                NodeProgramKind::AnalogOscillator(config) => NodeStateKind::AnalogOscillator(
                    Box::new(AnalogOscillatorState::new(seed, config.node_identity)),
                ),
                NodeProgramKind::Wavetable(config) => NodeStateKind::Wavetable(Box::new(
                    WavetableState::new(seed, config.node_identity),
                )),
                NodeProgramKind::SampleSource(_) => {
                    NodeStateKind::SampleSource(SampleSourceState::new())
                }
                NodeProgramKind::Multisample(config) => NodeStateKind::Multisample(Box::new(
                    MultisampleState::new(config.zones.len(), seed),
                )),
                NodeProgramKind::Granular(config) => {
                    NodeStateKind::Granular(Box::new(GranularState::new(config.max_grains, seed)))
                }
                NodeProgramKind::Spectral(config) => {
                    NodeStateKind::Spectral(Box::new(SpectralState::new(config.max_peaks, seed)))
                }
                NodeProgramKind::Noise(_) => NodeStateKind::Noise(NoiseState::new(seed)),
                NodeProgramKind::Envelope(_) => NodeStateKind::Envelope(EnvelopeState::default()),
                NodeProgramKind::Filter(_) => NodeStateKind::Filter(RuntimeFilterState::default()),
                NodeProgramKind::StereoFilter(_) => {
                    NodeStateKind::StereoFilter(Box::<StereoFilterState>::default())
                }
                NodeProgramKind::Delay(config) => {
                    NodeStateKind::Delay(DelayState::new(config.delay_frames))
                }
                NodeProgramKind::Reverb(_) => NodeStateKind::Reverb(ReverbState::new(sample_rate)),
                NodeProgramKind::Lfo(config) => {
                    NodeStateKind::Lfo(LfoState::new(seed, config.retrigger))
                }
                NodeProgramKind::Mixer(_)
                | NodeProgramKind::MonoToStereo
                | NodeProgramKind::StereoToMono
                | NodeProgramKind::StereoMixer(_)
                | NodeProgramKind::StereoSum
                | NodeProgramKind::Amplifier(_)
                | NodeProgramKind::Saturation(_)
                | NodeProgramKind::Pan(_)
                | NodeProgramKind::Output => NodeStateKind::Stateless,
            },
            seed_identity,
        )
    }

    /// Executes one Node; construction normally pairs each program with its state.
    pub(super) fn process(
        &self,
        state: &mut NodeState,
        inputs: NodeInputs,
        controls: &[f64],
        sample_rate: f64,
    ) -> Signal {
        match (&self.0, &mut state.0) {
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
            (NodeProgramKind::AnalogOscillator(config), NodeStateKind::AnalogOscillator(state)) => {
                Signal::Stereo(config.process(
                    state,
                    inputs.pitch.control(),
                    inputs.frequency_modulation.mono(),
                    inputs.phase_modulation.mono(),
                    inputs.sync.mono(),
                    inputs.amplitude_modulation.mono(),
                    inputs.gain.control(),
                    inputs.velocity.control(),
                    inputs.has_frequency_modulation,
                    inputs.has_phase_modulation,
                    inputs.has_sync,
                    inputs.has_amplitude_modulation,
                    inputs.has_gain,
                    inputs.has_velocity,
                    controls,
                    sample_rate,
                ))
            }
            (NodeProgramKind::Wavetable(config), NodeStateKind::Wavetable(state)) => {
                Signal::Stereo(config.process(
                    state,
                    inputs.pitch.control(),
                    inputs.frequency_modulation.mono(),
                    inputs.phase_modulation.mono(),
                    inputs.sync.mono(),
                    inputs.gain.control(),
                    inputs.velocity.control(),
                    inputs.has_frequency_modulation,
                    inputs.has_phase_modulation,
                    inputs.has_sync,
                    inputs.has_gain,
                    inputs.has_velocity,
                    controls,
                    sample_rate,
                ))
            }
            (NodeProgramKind::SampleSource(config), NodeStateKind::SampleSource(state)) => {
                Signal::Stereo(config.process(
                    state,
                    inputs.pitch.control(),
                    inputs.gate.control(),
                    inputs.gain.control(),
                    inputs.velocity.control(),
                    inputs.has_gain,
                    inputs.has_velocity,
                    controls,
                ))
            }
            (NodeProgramKind::Multisample(config), NodeStateKind::Multisample(state)) => {
                Signal::Stereo(config.process(
                    state,
                    inputs.pitch.control(),
                    inputs.gate.control(),
                    inputs.gain.control(),
                    inputs.velocity.control(),
                    inputs.has_gain,
                    controls,
                ))
            }
            (NodeProgramKind::Granular(config), NodeStateKind::Granular(state)) => {
                Signal::Stereo(config.process(
                    state,
                    inputs.pitch.control(),
                    inputs.gate.control(),
                    controls,
                    sample_rate,
                ))
            }
            (NodeProgramKind::Spectral(config), NodeStateKind::Spectral(state)) => {
                Signal::Stereo(config.process(state, inputs.pitch.control(), controls, sample_rate))
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
            (NodeProgramKind::MonoToStereo, NodeStateKind::Stateless) => {
                let input = inputs.audio.mono();
                Signal::Stereo([input, input])
            }
            (NodeProgramKind::StereoToMono, NodeStateKind::Stateless) => {
                let input = inputs.audio.stereo();
                Signal::Mono((input[0] + input[1]) * 0.5)
            }
            (NodeProgramKind::StereoMixer(config), NodeStateKind::Stateless) => {
                let a = inputs.input_a.stereo();
                let b = inputs.input_b.stereo();
                let a_level = config.a_level.value(controls) as f32;
                let b_level = config.b_level.value(controls) as f32;
                Signal::Stereo([
                    a[0] * a_level + b[0] * b_level,
                    a[1] * a_level + b[1] * b_level,
                ])
            }
            (NodeProgramKind::StereoSum, NodeStateKind::Stateless) => inputs.summed_audio,
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
            (NodeProgramKind::StereoFilter(config), NodeStateKind::StereoFilter(state)) => {
                Signal::Stereo(config.process(
                    state,
                    inputs.audio.stereo(),
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
                Signal::Control(config.process(state, controls, sample_rate, inputs.song_frame))
            }
            // Preserve callback-safe legacy behavior if an internal caller violates
            // the program/state pairing invariant.
            _ => Signal::None,
        }
    }

    /// Whether retained mutable state has the same structural program contract.
    pub(super) fn transition_compatible(&self, replacement: &Self) -> bool {
        match (&self.0, &replacement.0) {
            (NodeProgramKind::Oscillator(left), NodeProgramKind::Oscillator(right)) => {
                left.waveform == right.waveform
            }
            (NodeProgramKind::Unison(left), NodeProgramKind::Unison(right)) => {
                left.waveform == right.waveform && left.voices == right.voices
            }
            (NodeProgramKind::AnalogOscillator(left), NodeProgramKind::AnalogOscillator(right)) => {
                left.waveform == right.waveform
                    && left.voices == right.voices
                    && left.amplitude_mode == right.amplitude_mode
                    && left.distortion == right.distortion
                    && left.node_identity == right.node_identity
                    && left
                        .warps
                        .iter()
                        .zip(right.warps.iter())
                        .all(|(left, right)| match (left, right) {
                            (None, None) => true,
                            (Some(left), Some(right)) => left.kind == right.kind,
                            _ => false,
                        })
            }
            (NodeProgramKind::Wavetable(left), NodeProgramKind::Wavetable(right)) => {
                left.table.cache_identity() == right.table.cache_identity()
                    && left.voices == right.voices
                    && left.node_identity == right.node_identity
                    && left
                        .warps
                        .iter()
                        .zip(right.warps.iter())
                        .all(|(left, right)| match (left, right) {
                            (None, None) => true,
                            (Some(left), Some(right)) => left.kind == right.kind,
                            _ => false,
                        })
            }
            (NodeProgramKind::SampleSource(left), NodeProgramKind::SampleSource(right)) => {
                left.transition_compatible(right)
            }
            (NodeProgramKind::Multisample(left), NodeProgramKind::Multisample(right)) => {
                left.transition_compatible(right)
            }
            (NodeProgramKind::Granular(left), NodeProgramKind::Granular(right)) => {
                left.transition_compatible(right)
            }
            (NodeProgramKind::Spectral(left), NodeProgramKind::Spectral(right)) => {
                left.transition_compatible(right)
            }
            (NodeProgramKind::Envelope(left), NodeProgramKind::Envelope(right)) => {
                left.retrigger == right.retrigger
            }
            (NodeProgramKind::Filter(left), NodeProgramKind::Filter(right)) => {
                left.mode == right.mode
            }
            (NodeProgramKind::StereoFilter(left), NodeProgramKind::StereoFilter(right)) => {
                left.model == right.model && left.mode == right.mode
            }
            (NodeProgramKind::Delay(left), NodeProgramKind::Delay(right)) => {
                left.delay_frames == right.delay_frames
            }
            (NodeProgramKind::Lfo(left), NodeProgramKind::Lfo(right)) => {
                let count = usize::from(left.custom_point_count);
                left.waveform == right.waveform
                    && left.custom_point_count == right.custom_point_count
                    && left.mode == right.mode
                    && left.direction == right.direction
                    && left.polarity == right.polarity
                    && left.retrigger == right.retrigger
                    && left.custom_points[..count]
                        .iter()
                        .zip(right.custom_points[..count].iter())
                        .all(|(left, right)| {
                            left.phase.to_bits() == right.phase.to_bits()
                                && left.value.to_bits() == right.value.to_bits()
                        })
            }
            (NodeProgramKind::Noise(_), NodeProgramKind::Noise(_))
            | (NodeProgramKind::Mixer(_), NodeProgramKind::Mixer(_))
            | (NodeProgramKind::MonoToStereo, NodeProgramKind::MonoToStereo)
            | (NodeProgramKind::StereoToMono, NodeProgramKind::StereoToMono)
            | (NodeProgramKind::StereoMixer(_), NodeProgramKind::StereoMixer(_))
            | (NodeProgramKind::StereoSum, NodeProgramKind::StereoSum)
            | (NodeProgramKind::Amplifier(_), NodeProgramKind::Amplifier(_))
            | (NodeProgramKind::Saturation(_), NodeProgramKind::Saturation(_))
            | (NodeProgramKind::Pan(_), NodeProgramKind::Pan(_))
            | (NodeProgramKind::Reverb(_), NodeProgramKind::Reverb(_))
            | (NodeProgramKind::Output, NodeProgramKind::Output) => true,
            _ => false,
        }
    }

    pub(super) fn heap_bytes(&self) -> u64 {
        match &self.0 {
            NodeProgramKind::AnalogOscillator(_) => {
                std::mem::size_of::<AnalogOscillatorConfig>() as u64
            }
            NodeProgramKind::Wavetable(config) => config.allocation_bytes(),
            NodeProgramKind::SampleSource(config) => config.allocation_bytes(),
            NodeProgramKind::Multisample(config) => config.allocation_bytes(),
            NodeProgramKind::Granular(config) => config.allocation_bytes(),
            NodeProgramKind::Spectral(config) => config.allocation_bytes(),
            NodeProgramKind::Lfo(_) => std::mem::size_of::<LfoConfig>() as u64,
            _ => 0,
        }
    }

    #[cfg(test)]
    pub(super) fn is_stereo_sum(&self) -> bool {
        matches!(self.0, NodeProgramKind::StereoSum)
    }

    #[cfg(test)]
    pub(super) fn is_bandpass_filter(&self) -> bool {
        matches!(
            &self.0,
            NodeProgramKind::Filter(FilterConfig {
                mode: FilterMode::Bandpass,
                ..
            })
        )
    }
}

impl NodeState {
    pub(super) fn reset(&mut self, seed: u64) {
        let seed = self.1.resolve(seed);
        match &mut self.0 {
            NodeStateKind::Oscillator(state) => state.retrigger(seed),
            NodeStateKind::Unison(state) => state.retrigger(seed),
            NodeStateKind::AnalogOscillator(state) => state.retrigger(seed),
            NodeStateKind::Wavetable(state) => state.retrigger(seed),
            NodeStateKind::SampleSource(state) => state.retrigger(),
            NodeStateKind::Multisample(state) => state.retrigger(seed),
            NodeStateKind::Granular(state) => state.retrigger(seed),
            NodeStateKind::Spectral(state) => state.retrigger(seed),
            NodeStateKind::Noise(state) => state.retrigger(seed),
            NodeStateKind::Envelope(state) => *state = EnvelopeState::default(),
            NodeStateKind::Filter(state) => *state = RuntimeFilterState::default(),
            NodeStateKind::StereoFilter(state) => **state = StereoFilterState::default(),
            NodeStateKind::Lfo(state) => state.reset(seed),
            NodeStateKind::Delay(state) => state.reset(),
            NodeStateKind::Reverb(state) => state.reset(),
            NodeStateKind::Stateless => {}
        }
    }

    pub(super) fn retrigger(&mut self, seed: u64) {
        let seed = self.1.resolve(seed);
        match &mut self.0 {
            NodeStateKind::Oscillator(state) => state.retrigger(seed),
            NodeStateKind::Unison(state) => state.retrigger(seed),
            NodeStateKind::AnalogOscillator(state) => state.retrigger(seed),
            NodeStateKind::Wavetable(state) => state.retrigger(seed),
            NodeStateKind::SampleSource(state) => state.retrigger(),
            NodeStateKind::Multisample(state) => state.retrigger(seed),
            NodeStateKind::Granular(state) => state.retrigger(seed),
            NodeStateKind::Spectral(state) => state.retrigger(seed),
            NodeStateKind::Noise(state) => state.retrigger(seed),
            NodeStateKind::Envelope(state) => *state = EnvelopeState::default(),
            NodeStateKind::Filter(state) => *state = RuntimeFilterState::default(),
            NodeStateKind::StereoFilter(state) => **state = StereoFilterState::default(),
            NodeStateKind::Lfo(state) => state.retrigger(seed),
            NodeStateKind::Delay(_) | NodeStateKind::Reverb(_) | NodeStateKind::Stateless => {}
        }
    }

    /// Applies Node-local selected-note retrigger policy without resetting retained voice state.
    pub(super) fn retarget_note(&mut self, seed: u64) {
        let seed = self.1.resolve(seed);
        match &mut self.0 {
            NodeStateKind::Envelope(state) => state.note_retrigger(),
            NodeStateKind::Lfo(state) => state.retrigger(seed),
            NodeStateKind::Oscillator(_)
            | NodeStateKind::Unison(_)
            | NodeStateKind::AnalogOscillator(_)
            | NodeStateKind::Wavetable(_)
            | NodeStateKind::SampleSource(_)
            | NodeStateKind::Multisample(_)
            | NodeStateKind::Granular(_)
            | NodeStateKind::Spectral(_)
            | NodeStateKind::Noise(_)
            | NodeStateKind::Filter(_)
            | NodeStateKind::StereoFilter(_)
            | NodeStateKind::Delay(_)
            | NodeStateKind::Reverb(_)
            | NodeStateKind::Stateless => {}
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
            NodeStateKind::AnalogOscillator(_) => {
                std::mem::size_of::<AnalogOscillatorState>() as u64
            }
            NodeStateKind::Wavetable(_) => std::mem::size_of::<WavetableState>() as u64,
            NodeStateKind::SampleSource(_) => sample_source::sample_source_state_bytes(),
            NodeStateKind::Multisample(state) => state.allocation_bytes(),
            NodeStateKind::Granular(state) => state.allocation_bytes(),
            NodeStateKind::Spectral(state) => state.allocation_bytes(),
            NodeStateKind::StereoFilter(_) => std::mem::size_of::<StereoFilterState>() as u64,
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
    match node.setting(key) {
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
            if parameter.unit != unit {
                return Err(EngineError::Compile(format!(
                    "setting `{key}` expects {unit}, but Parameter `{parameter_key}` uses {}",
                    parameter.unit
                )));
            }
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

fn compile_lfo_points(
    node: &GraphNode,
) -> Result<
    (
        [crate::engine::lfo::LfoPoint; crate::engine::lfo::MAX_CUSTOM_LFO_POINTS],
        u8,
    ),
    EngineError,
> {
    let custom = text(node, "waveform", "sine") == "custom";
    if !custom && !node.directives.is_empty() {
        return Err(EngineError::Compile(
            "only a custom LFO waveform may declare point directives".to_owned(),
        ));
    }
    if !custom {
        return Ok((
            [crate::engine::lfo::LfoPoint::default(); crate::engine::lfo::MAX_CUSTOM_LFO_POINTS],
            0,
        ));
    }
    if !(2..=crate::engine::lfo::MAX_CUSTOM_LFO_POINTS).contains(&node.directives.len()) {
        return Err(EngineError::Compile(format!(
            "custom LFO requires 2..={} ordered points",
            crate::engine::lfo::MAX_CUSTOM_LFO_POINTS
        )));
    }
    let mut points =
        [crate::engine::lfo::LfoPoint::default(); crate::engine::lfo::MAX_CUSTOM_LFO_POINTS];
    let mut ids = std::collections::BTreeSet::new();
    let mut previous_phase = None;
    for (index, directive) in node.directives.iter().enumerate() {
        let tokens = &directive.tokens;
        if !directive.children.is_empty()
            || tokens.len() != 6
            || tokens[0].value != "point"
            || tokens[2].value != "phase"
            || tokens[4].value != "value"
            || tokens[1].value.is_empty()
            || !ids.insert(tokens[1].value.as_str())
        {
            return Err(EngineError::Compile(
                "custom LFO point syntax is `point ID phase VALUE value VALUE` with unique IDs"
                    .to_owned(),
            ));
        }
        let phase = tokens[3]
            .value
            .parse::<f64>()
            .map_err(|_| EngineError::Compile("custom LFO phase must be finite".to_owned()))?;
        let value = tokens[5]
            .value
            .parse::<f64>()
            .map_err(|_| EngineError::Compile("custom LFO value must be finite".to_owned()))?;
        if !phase.is_finite()
            || !value.is_finite()
            || !(0.0..=1.0).contains(&phase)
            || !(-1.0..=1.0).contains(&value)
            || previous_phase.is_some_and(|previous| phase <= previous)
        {
            return Err(EngineError::Compile(
                "custom LFO points require strictly increasing phase 0..=1 and value -1..=1"
                    .to_owned(),
            ));
        }
        points[index] = crate::engine::lfo::LfoPoint { phase, value };
        previous_phase = Some(phase);
    }
    if points[0].phase != 0.0 || points[node.directives.len() - 1].phase != 1.0 {
        return Err(EngineError::Compile(
            "custom LFO points must begin at phase 0 and end at phase 1".to_owned(),
        ));
    }
    Ok((points, node.directives.len() as u8))
}

fn resolve_lfo_rate(
    node: &GraphNode,
    key: &str,
    default: f64,
    parameters: &BTreeMap<&str, &Parameter>,
    instrument_id: &str,
    controls: &CompiledControlProgram,
    context: NodeCompileContext<'_>,
) -> Result<Scalar, EngineError> {
    if let Some(SettingValue::Text(value)) = node.setting(key)
        && Unit::Hertz.parse_value(value).is_err()
    {
        let period = parse_musical_time(value, context.meter)
            .map_err(|error| EngineError::Compile(format!("setting `{key}`: {error}")))?;
        let frames = period
            .to_frames(context.tempo_bpm, context.sample_rate)
            .map_err(|error| EngineError::Compile(format!("setting `{key}`: {error}")))?;
        if frames == 0 {
            return Err(EngineError::Compile(
                "synchronized LFO period resolves to zero frames".to_owned(),
            ));
        }
        return Ok(Scalar::constant(
            f64::from(context.sample_rate) / frames as f64,
        ));
    }
    resolve_number(
        node,
        key,
        Unit::Hertz,
        default,
        parameters,
        instrument_id,
        controls,
    )
}

fn resolve_duration(
    node: &GraphNode,
    key: &str,
    default: f64,
    parameters: &BTreeMap<&str, &Parameter>,
    instrument_id: &str,
    controls: &CompiledControlProgram,
    context: NodeCompileContext<'_>,
) -> Result<Scalar, EngineError> {
    if let Some(SettingValue::Text(value)) = node.setting(key)
        && Unit::Seconds.parse_value(value).is_err()
    {
        let period = parse_musical_time(value, context.meter)
            .map_err(|error| EngineError::Compile(format!("setting `{key}`: {error}")))?;
        let frames = period
            .to_frames(context.tempo_bpm, context.sample_rate)
            .map_err(|error| EngineError::Compile(format!("setting `{key}`: {error}")))?;
        let seconds = frames as f64 / f64::from(context.sample_rate);
        if !(0.0..=MAX_RELEASE_SECONDS).contains(&seconds) {
            return Err(EngineError::Compile(format!(
                "setting `{key}` resolves outside 0s..={MAX_RELEASE_SECONDS}s"
            )));
        }
        return Ok(Scalar::constant(seconds));
    }
    resolve_number(
        node,
        key,
        Unit::Seconds,
        default,
        parameters,
        instrument_id,
        controls,
    )
}

fn resolve_directive_number(
    value: &str,
    unit: Unit,
    parameters: &BTreeMap<&str, &Parameter>,
    instrument_id: &str,
    controls: &CompiledControlProgram,
) -> Result<Scalar, EngineError> {
    if let Some(parameter_key) = value.strip_prefix('$') {
        let parameter = parameters
            .get(parameter_key)
            .ok_or_else(|| EngineError::Compile(format!("missing Parameter `{parameter_key}`")))?;
        if parameter.unit != unit {
            return Err(EngineError::Compile(format!(
                "Parameter `{parameter_key}` has incompatible unit {}",
                parameter.unit
            )));
        }
        let target = crate::model::ParameterTarget::Instrument {
            instrument: instrument_id.to_owned(),
            parameter: parameter_key.to_owned(),
        };
        let control = controls.index(&target).ok_or_else(|| {
            EngineError::Compile(format!("missing compiled Parameter target `{target}`"))
        })?;
        Ok(Scalar::controlled(parameter.value, control))
    } else {
        unit.parse_value(value)
            .map(Scalar::constant)
            .map_err(|error| EngineError::Compile(format!("directive amount: {error}")))
    }
}

fn analog_warp(value: &str) -> Result<AnalogPhaseWarp, EngineError> {
    match value {
        "none" => Ok(AnalogPhaseWarp::None),
        "pulse_width" => Ok(AnalogPhaseWarp::PulseWidth),
        "bend" => Ok(AnalogPhaseWarp::Bend),
        "remap" | "phase_remap" => Ok(AnalogPhaseWarp::Remap),
        _ => Err(EngineError::Compile(format!(
            "unsupported analog oscillator warp `{value}`"
        ))),
    }
}

fn analog_distortion(value: &str) -> Result<AnalogDistortion, EngineError> {
    match value {
        "none" => Ok(AnalogDistortion::None),
        "soft_clip" => Ok(AnalogDistortion::SoftClip),
        "tanh" => Ok(AnalogDistortion::Tanh),
        _ => Err(EngineError::Compile(format!(
            "unsupported analog oscillator distortion `{value}`"
        ))),
    }
}

fn text<'a>(node: &'a GraphNode, key: &str, default: &'a str) -> &'a str {
    match node.setting(key) {
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
    match node.setting(key) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(audio: Signal, a: Signal, b: Signal, summed_audio: Signal) -> NodeInputs {
        NodeInputs {
            song_frame: 0,
            pitch: Signal::None,
            key: Signal::None,
            gate: Signal::None,
            velocity: Signal::None,
            audio,
            gain: Signal::None,
            modulation: Signal::None,
            frequency_modulation: Signal::None,
            phase_modulation: Signal::None,
            sync: Signal::None,
            amplitude_modulation: Signal::None,
            input_a: a,
            input_b: b,
            summed_audio,
            has_key: false,
            has_velocity: false,
            has_gain: false,
            has_modulation: false,
            has_frequency_modulation: false,
            has_phase_modulation: false,
            has_sync: false,
            has_amplitude_modulation: false,
        }
    }

    fn process(program: NodeProgram, inputs: NodeInputs) -> Signal {
        let mut state = NodeState(
            NodeStateKind::Stateless,
            NodeSeedIdentity {
                legacy_index: 0,
                semantic_identity: None,
            },
        );
        program.process(&mut state, inputs, &[], 48_000.0)
    }

    #[test]
    fn explicit_conversion_mix_and_sum_vectors_are_finite_and_exact() {
        assert_eq!(
            process(
                NodeProgram(NodeProgramKind::MonoToStereo),
                inputs(Signal::Mono(0.25), Signal::None, Signal::None, Signal::None),
            )
            .stereo(),
            [0.25, 0.25]
        );
        assert_eq!(
            process(
                NodeProgram(NodeProgramKind::StereoToMono),
                inputs(
                    Signal::Stereo([1.0, -0.5]),
                    Signal::None,
                    Signal::None,
                    Signal::None,
                ),
            )
            .mono(),
            0.25
        );
        assert_eq!(
            process(
                NodeProgram(NodeProgramKind::StereoMixer(MixerConfig {
                    a_level: Scalar::constant(0.5),
                    b_level: Scalar::constant(2.0),
                })),
                inputs(
                    Signal::None,
                    Signal::Stereo([0.5, -0.25]),
                    Signal::Stereo([0.125, 0.25]),
                    Signal::None,
                ),
            )
            .stereo(),
            [0.5, 0.375]
        );
        let sum = process(
            NodeProgram(NodeProgramKind::StereoSum),
            inputs(
                Signal::None,
                Signal::None,
                Signal::None,
                Signal::Stereo([0.75, -0.25]),
            ),
        )
        .stereo();
        assert_eq!(sum, [0.75, -0.25]);
        assert!(sum.into_iter().all(f32::is_finite));
    }
}
