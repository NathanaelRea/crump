//! One definition per Processing Node kind.
//!
//! Each kind's source name, typed Ports, settings and their legal domains are declared once
//! here. Graph Validation, the runtime's compile-time checks, admission and the workspace read
//! these definitions instead of restating them.

use crate::{
    model::{GraphNode, MediaAssetUsage, NodeKind, SettingValue, Unit},
    validate::MAX_STEREO_SUM_INPUTS,
};

/// Signal carried by a Port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignalType {
    AudioMono,
    AudioStereo,
    Hertz,
    Gate,
    Unipolar,
    Control,
}

impl SignalType {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::AudioMono => "mono Audio",
            Self::AudioStereo => "stereo Audio",
            Self::Hertz => "hertz Control",
            Self::Gate => "gate Control",
            Self::Unipolar => "unipolar Control",
            Self::Control => "uni- or bipolar Control",
        }
    }

    /// Whether a Bus carrying `self` may feed an input of type `input`.
    ///
    /// Unipolar Control is a subset of generic Control; every other pair needs an explicit
    /// conversion Node.
    pub(crate) const fn feeds(self, input: Self) -> bool {
        self as u8 == input as u8 || matches!((self, input), (Self::Unipolar, Self::Control))
    }
}

/// How many Buses an input Port accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cardinality {
    /// Exactly one Bus.
    Required,
    /// At most one Bus.
    Optional,
    /// One or more Buses, bounded by the Node's `max_inputs`, folded in Bus-ID order.
    Summing,
}

/// A named, typed connection point.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PortSpec {
    pub(crate) name: &'static str,
    pub(crate) signal: SignalType,
    pub(crate) cardinality: Cardinality,
}

/// A closed numeric range in a setting's canonical physical unit.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NumericDomain {
    pub(crate) minimum: f64,
    pub(crate) maximum: f64,
}

impl NumericDomain {
    pub(crate) const fn bounded(minimum: f64, maximum: f64) -> Self {
        Self { minimum, maximum }
    }

    pub(crate) fn contains(self, value: f64) -> bool {
        value >= self.minimum && value <= self.maximum
    }

    pub(crate) fn clamp(self, value: f64) -> f64 {
        value.clamp(self.minimum, self.maximum)
    }

    pub(crate) fn label(self) -> String {
        format!("{}..={}", self.minimum, self.maximum)
    }
}

/// The value a setting accepts.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SettingKind {
    Number(Unit),
    Duration,
    LfoRate,
    Identifier,
    Pitch,
    Integer { minimum: u16, maximum: u16 },
    Text(&'static [&'static str]),
}

/// One authored setting key.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SettingSpec {
    pub(crate) key: &'static str,
    pub(crate) kind: SettingKind,
    /// Legal resolved numeric values, including the domain of a referenced Parameter.
    pub(crate) domain: Option<NumericDomain>,
}

/// Everything a caller needs to know about one Processing Node kind.
#[derive(Debug)]
pub(crate) struct NodeDefinition {
    /// Canonical source name, as accepted by `NodeKind::from_str`.
    pub(crate) name: &'static str,
    pub(crate) inputs: &'static [PortSpec],
    pub(crate) outputs: &'static [PortSpec],
    pub(crate) settings: &'static [SettingSpec],
    /// The output depends only on earlier frames, so a Bus into this Node may close a
    /// Feedback Loop.
    pub(crate) breaks_feedback: bool,
    /// Role of the Project-owned Media Asset named by the `asset` setting.
    pub(crate) asset: Option<MediaAssetUsage>,
    /// First words of the structured directives a Node block accepts besides `key value`
    /// settings. A keyword that is also a setting key is a directive only when it is not
    /// written as `key value`.
    pub(crate) directives: &'static [&'static str],
}

impl NodeDefinition {
    /// Defaults for the optional facts; each kind states its name, Ports and settings.
    const PLAIN: Self = Self {
        name: "",
        inputs: &[],
        outputs: &[],
        settings: &[],
        breaks_feedback: false,
        asset: None,
        directives: &[],
    };

    pub(crate) fn input(&self, port: &str) -> Option<&'static PortSpec> {
        self.inputs.iter().find(|spec| spec.name == port)
    }

    pub(crate) fn output(&self, port: &str) -> Option<SignalType> {
        self.outputs
            .iter()
            .find(|spec| spec.name == port)
            .map(|spec| spec.signal)
    }

    /// Inputs that must receive at least one Bus.
    pub(crate) fn required_inputs(&self) -> impl Iterator<Item = &'static str> + use<> {
        let inputs = self.inputs;
        inputs
            .iter()
            .filter(|spec| spec.cardinality != Cardinality::Optional)
            .map(|spec| spec.name)
    }

    pub(crate) fn setting(&self, key: &str) -> Option<&'static SettingSpec> {
        let settings = self.settings;
        settings.iter().find(|spec| spec.key == key)
    }

    /// Declared numeric domain for `key`.
    pub(crate) fn domain(&self, key: &str) -> Option<NumericDomain> {
        self.setting(key).and_then(|spec| spec.domain)
    }
}

/// Maximum number of Buses `node` admits on input `port`.
pub(crate) fn input_bus_limit(node: &GraphNode, port: &str) -> usize {
    match node
        .kind
        .definition()
        .input(port)
        .map(|spec| spec.cardinality)
    {
        Some(Cardinality::Summing) => match node.setting("max_inputs") {
            Some(SettingValue::Number(value)) if value.is_finite() => {
                (*value as usize).clamp(1, MAX_STEREO_SUM_INPUTS)
            }
            _ => MAX_STEREO_SUM_INPUTS,
        },
        _ => 1,
    }
}

const fn required(name: &'static str, signal: SignalType) -> PortSpec {
    PortSpec {
        name,
        signal,
        cardinality: Cardinality::Required,
    }
}

const fn optional(name: &'static str, signal: SignalType) -> PortSpec {
    PortSpec {
        name,
        signal,
        cardinality: Cardinality::Optional,
    }
}

const fn summing(name: &'static str, signal: SignalType) -> PortSpec {
    PortSpec {
        name,
        signal,
        cardinality: Cardinality::Summing,
    }
}

const fn output(name: &'static str, signal: SignalType) -> PortSpec {
    PortSpec {
        name,
        signal,
        cardinality: Cardinality::Optional,
    }
}

const fn setting(key: &'static str, kind: SettingKind) -> SettingSpec {
    SettingSpec {
        key,
        kind,
        domain: None,
    }
}

const fn bounded(key: &'static str, kind: SettingKind, minimum: f64, maximum: f64) -> SettingSpec {
    SettingSpec {
        key,
        kind,
        domain: Some(NumericDomain::bounded(minimum, maximum)),
    }
}

impl NodeKind {
    /// The single definition of this Processing Node kind.
    #[must_use]
    pub(crate) const fn definition(self) -> &'static NodeDefinition {
        use SettingKind::{Duration, Identifier, Integer, LfoRate, Number, Pitch, Text};
        use SignalType::{AudioMono, AudioStereo, Control, Gate, Hertz, Unipolar};
        match self {
            NodeKind::Exciter => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "exciter",
                    inputs: &[required("gate", Gate), required("velocity", Unipolar)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[
                        setting("kind", Text(&["impact", "collisions", "texture"])),
                        bounded("contact", Number(Unit::Seconds), 2e-5, 0.02),
                        bounded("attack", Number(Unit::Seconds), 0.0, 1.0),
                        bounded("hardness", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("rate", Number(Unit::Hertz), 1.0, 10000.0),
                        bounded("duration", Number(Unit::Seconds), 0.005, 2.0),
                        bounded("level", Number(Unit::Ratio), 0.0, 4.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::ModalBank => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "modal_bank",
                    inputs: &[
                        required("audio", AudioMono),
                        required("pitch", Hertz),
                        required("gate", Gate),
                    ],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[
                        bounded("rise", Number(Unit::Seconds), 0.0, 1.0),
                        setting("profile", Identifier),
                        setting(
                            "max_modes",
                            Integer {
                                minimum: 1,
                                maximum: 256,
                            },
                        ),
                        bounded("decay_scale", Number(Unit::Ratio), 0.1, 4.0),
                        bounded("brightness", Number(Unit::Ratio), -2.0, 2.0),
                        bounded("key_track", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("level", Number(Unit::Ratio), 0.0, 4.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Oscillator => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "oscillator",
                    inputs: &[required("pitch", Hertz)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[
                        setting("waveform", Text(&["sine", "saw", "square", "triangle"])),
                        bounded("detune", Number(Unit::Semitones), -96.0, 96.0),
                        bounded("glide", Number(Unit::Seconds), 0.0, 5.0),
                        bounded("level", Number(Unit::Ratio), 0.0, 4.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Unison => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "unison",
                    inputs: &[
                        required("pitch", Hertz),
                        optional("velocity", Unipolar),
                        optional("gain", Unipolar),
                        optional("mod", Control),
                    ],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        setting("waveform", Text(&["sine", "saw", "square", "triangle"])),
                        setting(
                            "voices",
                            Integer {
                                minimum: 1,
                                maximum: 8,
                            },
                        ),
                        bounded("detune", Number(Unit::Semitones), 0.0, 2.0),
                        bounded("width", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("drift", Number(Unit::Semitones), 0.0, 0.5),
                        bounded("glide", Number(Unit::Seconds), 0.0, 5.0),
                        bounded("mod_depth", Number(Unit::Semitones), -24.0, 24.0),
                        bounded("velocity_amount", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("level", Number(Unit::Ratio), 0.0, 4.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Noise => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "noise",
                    inputs: &[],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[bounded("level", Number(Unit::Ratio), 0.0, 4.0)],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Envelope => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "envelope",
                    inputs: &[required("gate", Gate)],
                    outputs: &[output("control", Unipolar)],
                    settings: &[
                        setting("mode", Text(&["adsr", "ahdsr"])),
                        bounded("attack", Duration, 0.0, 8.0),
                        bounded("hold", Duration, 0.0, 8.0),
                        bounded("decay", Duration, 0.0, 8.0),
                        bounded("sustain", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("release", Duration, 0.0, 8.0),
                        bounded("attack_curve", Number(Unit::Ratio), -1.0, 1.0),
                        bounded("decay_curve", Number(Unit::Ratio), -1.0, 1.0),
                        bounded("release_curve", Number(Unit::Ratio), -1.0, 1.0),
                        setting("retrigger", Text(&["always", "legato"])),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Mixer => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "mixer",
                    inputs: &[required("a", AudioMono), optional("b", AudioMono)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[
                        bounded("a_level", Number(Unit::Ratio), 0.0, 4.0),
                        bounded("b_level", Number(Unit::Ratio), 0.0, 4.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Lowpass => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "lowpass",
                    inputs: &[
                        required("audio", AudioMono),
                        optional("velocity", Unipolar),
                        optional("key", Hertz),
                        optional("mod", Control),
                    ],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[
                        setting("mode", Text(&["lowpass", "highpass", "bandpass", "notch"])),
                        bounded("cutoff", Number(Unit::Hertz), 10.0, 21600.0),
                        bounded("resonance", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("mod_depth", Number(Unit::Hertz), -21600.0, 21600.0),
                        bounded("velocity_depth", Number(Unit::Hertz), -21600.0, 21600.0),
                        bounded("key_track", Number(Unit::Ratio), 0.0, 2.0),
                        bounded("key_reference", Number(Unit::Hertz), 10.0, 21600.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::StereoFilter => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "stereo_filter",
                    inputs: &[
                        required("audio", AudioStereo),
                        optional("velocity", Unipolar),
                        optional("key", Hertz),
                        optional("mod", Control),
                    ],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        setting("model", Text(&["svf_clean", "ladder_drive"])),
                        setting("mode", Text(&["lowpass", "highpass", "bandpass", "notch"])),
                        bounded("cutoff", Number(Unit::Hertz), 10.0, 21600.0),
                        bounded("resonance", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("drive", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("mix", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("pan", Number(Unit::Pan), -1.0, 1.0),
                        bounded("mod_depth", Number(Unit::Hertz), -21600.0, 21600.0),
                        bounded("velocity_depth", Number(Unit::Hertz), -21600.0, 21600.0),
                        bounded("key_track", Number(Unit::Ratio), 0.0, 2.0),
                        bounded("key_reference", Number(Unit::Hertz), 10.0, 21600.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Amplifier => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "amplifier",
                    inputs: &[required("audio", AudioMono), required("gain", Unipolar)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[bounded("level", Number(Unit::Ratio), 0.0, 4.0)],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Saturation => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "saturation",
                    inputs: &[required("audio", AudioMono)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[bounded("drive", Number(Unit::Ratio), 0.0, 8.0)],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Pan => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "pan",
                    inputs: &[required("audio", AudioMono)],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[bounded("position", Number(Unit::Pan), -1.0, 1.0)],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Delay => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "delay",
                    inputs: &[required("audio", AudioStereo)],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        bounded("time", Number(Unit::Seconds), 0.001, 2.0),
                        bounded("feedback", Number(Unit::Ratio), 0.0, 0.95),
                        bounded("mix", Number(Unit::Ratio), 0.0, 1.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Reverb => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "reverb",
                    inputs: &[required("audio", AudioStereo)],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        bounded("room", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("damping", Number(Unit::Ratio), 0.0, 0.95),
                        bounded("mix", Number(Unit::Ratio), 0.0, 1.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Output => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "output",
                    inputs: &[required("audio", AudioStereo)],
                    outputs: &[],
                    settings: &[],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Lfo => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "lfo",
                    inputs: &[],
                    outputs: &[output("control", Control)],
                    settings: &[
                        bounded("rate", LfoRate, 0.01, 50.0),
                        bounded("depth", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("phase", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("random_phase", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("swing", Number(Unit::Ratio), -1.0, 1.0),
                        setting(
                            "waveform",
                            Text(&["sine", "triangle", "saw", "square", "sample_hold", "custom"]),
                        ),
                        setting("mode", Text(&["loop", "one_shot"])),
                        setting("direction", Text(&["forward", "reverse", "ping_pong"])),
                        setting("polarity", Text(&["unipolar", "bipolar"])),
                        setting("retrigger", Text(&["note", "transport"])),
                    ],
                    directives: &["point"],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::MonoToStereo => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "mono_to_stereo",
                    inputs: &[required("audio", AudioMono)],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::StereoToMono => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "stereo_to_mono",
                    inputs: &[required("audio", AudioStereo)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::StereoMixer => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "stereo_mixer",
                    inputs: &[required("a", AudioStereo), optional("b", AudioStereo)],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        bounded("a_level", Number(Unit::Ratio), 0.0, 4.0),
                        bounded("b_level", Number(Unit::Ratio), 0.0, 4.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::StereoSum => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "stereo_sum",
                    inputs: &[summing("inputs", AudioStereo)],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[setting(
                        "max_inputs",
                        Integer {
                            minimum: 1,
                            maximum: 64,
                        },
                    )],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Sum => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "sum",
                    inputs: &[summing("inputs", AudioMono)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[setting(
                        "max_inputs",
                        Integer {
                            minimum: 1,
                            maximum: 64,
                        },
                    )],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::AnalogOscillator => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "analog_oscillator",
                    inputs: &[
                        required("pitch", Hertz),
                        optional("velocity", Unipolar),
                        optional("gain", Unipolar),
                        optional("fm", AudioMono),
                        optional("pm", AudioMono),
                        optional("sync", AudioMono),
                        optional("am", AudioMono),
                    ],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        setting(
                            "waveform",
                            Text(&["sine", "saw", "square", "pulse", "triangle"]),
                        ),
                        setting(
                            "voices",
                            Integer {
                                minimum: 1,
                                maximum: 8,
                            },
                        ),
                        setting(
                            "unison",
                            Integer {
                                minimum: 1,
                                maximum: 8,
                            },
                        ),
                        bounded("coarse", Number(Unit::Semitones), -48.0, 48.0),
                        bounded("fine", Number(Unit::Semitones), -1.0, 1.0),
                        bounded("detune", Number(Unit::Semitones), 0.0, 2.0),
                        bounded("blend", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("width", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("drift", Number(Unit::Semitones), 0.0, 0.5),
                        bounded("glide", Number(Unit::Seconds), 0.0, 5.0),
                        bounded("fm_depth", Number(Unit::Semitones), -48.0, 48.0),
                        bounded("pm_depth", Number(Unit::Ratio), 0.0, 1.0),
                        setting("am_mode", Text(&["am", "ring"])),
                        bounded("am_depth", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("phase", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("random_phase", Number(Unit::Ratio), 0.0, 1.0),
                        setting("warp", Text(&["none", "pulse_width", "bend", "remap"])),
                        bounded("warp_amount", Number(Unit::Ratio), -1.0, 1.0),
                        setting("distortion", Text(&["none", "soft_clip", "tanh"])),
                        bounded("drive", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("velocity_amount", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("level", Number(Unit::Ratio), 0.0, 4.0),
                        bounded("pan", Number(Unit::Ratio), -1.0, 1.0),
                    ],
                    directives: &["warp", "distortion"],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Wavetable => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "wavetable",
                    inputs: &[
                        required("pitch", Hertz),
                        optional("velocity", Unipolar),
                        optional("gain", Unipolar),
                        optional("fm", AudioMono),
                        optional("pm", AudioMono),
                        optional("sync", AudioMono),
                    ],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        setting("asset", Identifier),
                        setting(
                            "voices",
                            Integer {
                                minimum: 1,
                                maximum: 8,
                            },
                        ),
                        setting(
                            "unison",
                            Integer {
                                minimum: 1,
                                maximum: 8,
                            },
                        ),
                        bounded("coarse", Number(Unit::Semitones), -48.0, 48.0),
                        bounded("fine", Number(Unit::Semitones), -1.0, 1.0),
                        bounded("table_position", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("phase", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("random_phase", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("detune", Number(Unit::Semitones), 0.0, 2.0),
                        bounded("blend", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("width", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("drift", Number(Unit::Semitones), 0.0, 0.5),
                        bounded("glide", Number(Unit::Seconds), 0.0, 5.0),
                        bounded("fm_depth", Number(Unit::Semitones), -48.0, 48.0),
                        bounded("pm_depth", Number(Unit::Ratio), 0.0, 1.0),
                        setting("warp", Text(&["none", "pulse_width", "bend", "remap"])),
                        bounded("warp_amount", Number(Unit::Ratio), -1.0, 1.0),
                        bounded("velocity_amount", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("level", Number(Unit::Ratio), 0.0, 4.0),
                        bounded("pan", Number(Unit::Ratio), -1.0, 1.0),
                    ],
                    asset: Some(MediaAssetUsage::Wavetable),
                    directives: &["warp"],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::SampleSource => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "sample_source",
                    inputs: &[
                        required("pitch", Hertz),
                        required("gate", Gate),
                        optional("velocity", Unipolar),
                        optional("gain", Unipolar),
                    ],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        setting("asset", Identifier),
                        setting("selected_slice", Identifier),
                        bounded("start", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("end", Number(Unit::Ratio), 0.0, 1.0),
                        setting("loop_mode", Text(&["off", "forward", "ping_pong"])),
                        bounded("loop_start", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("loop_end", Number(Unit::Ratio), 0.0, 1.0),
                        setting(
                            "loop_crossfade",
                            Integer {
                                minimum: 0,
                                maximum: 8192,
                            },
                        ),
                        setting("direction", Text(&["forward", "reverse"])),
                        setting("playback", Text(&["gated", "one_shot"])),
                        setting("root", Pitch),
                        bounded("tune", Number(Unit::Semitones), -48.0, 48.0),
                        bounded("rate", Number(Unit::Ratio), 0.125, 8.0),
                        bounded("velocity_amount", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("level", Number(Unit::Ratio), 0.0, 4.0),
                        bounded("pan", Number(Unit::Pan), -1.0, 1.0),
                    ],
                    asset: Some(MediaAssetUsage::Sample),
                    directives: &["slice"],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Multisample => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "multisample",
                    inputs: &[
                        required("pitch", Hertz),
                        required("gate", Gate),
                        required("velocity", Unipolar),
                        optional("gain", Unipolar),
                    ],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        setting("playback", Text(&["gated", "one_shot"])),
                        bounded("tune", Number(Unit::Semitones), -48.0, 48.0),
                        bounded("rate", Number(Unit::Ratio), 0.125, 8.0),
                        bounded("velocity_amount", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("level", Number(Unit::Ratio), 0.0, 4.0),
                        bounded("pan", Number(Unit::Pan), -1.0, 1.0),
                    ],
                    directives: &["zone"],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Granular => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "granular",
                    inputs: &[required("pitch", Hertz), required("gate", Gate)],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        setting("asset", Identifier),
                        setting(
                            "max_grains",
                            Integer {
                                minimum: 1,
                                maximum: 64,
                            },
                        ),
                        bounded("position", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("scan", Number(Unit::Ratio), -2.0, 2.0),
                        bounded("size", Number(Unit::Seconds), 0.005, 1.0),
                        bounded("density", Number(Unit::Hertz), 0.1, 1000.0),
                        setting("root", Pitch),
                        bounded("tune", Number(Unit::Semitones), -48.0, 48.0),
                        bounded("spray", Number(Unit::Ratio), 0.0, 1.0),
                        setting("window", Text(&["hann", "triangle", "tukey"])),
                        setting("edge", Text(&["wrap", "reflect", "clamp"])),
                        bounded("direction_probability", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("stereo_spread", Number(Unit::Ratio), 0.0, 1.0),
                        setting("freeze", Number(Unit::Boolean)),
                        setting(
                            "seed",
                            Integer {
                                minimum: 0,
                                maximum: 65535,
                            },
                        ),
                        bounded("level", Number(Unit::Ratio), 0.0, 4.0),
                    ],
                    asset: Some(MediaAssetUsage::Granular),
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Spectral => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "spectral",
                    inputs: &[required("pitch", Hertz)],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        setting("asset", Identifier),
                        setting("algorithm", Text(&["crump-spectral-peaks-v1"])),
                        setting(
                            "max_peaks",
                            Integer {
                                minimum: 1,
                                maximum: 128,
                            },
                        ),
                        bounded("scan", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("rate", Number(Unit::Ratio), -4.0, 4.0),
                        setting("root", Pitch),
                        bounded("tune", Number(Unit::Semitones), -48.0, 48.0),
                        bounded("low", Number(Unit::Hertz), 20.0, 21600.0),
                        bounded("high", Number(Unit::Hertz), 20.0, 21600.0),
                        setting("transients", Text(&["preserve", "reset", "suppress"])),
                        bounded("phase_randomization", Number(Unit::Ratio), 0.0, 1.0),
                        setting("freeze", Number(Unit::Boolean)),
                        setting(
                            "seed",
                            Integer {
                                minimum: 0,
                                maximum: 65535,
                            },
                        ),
                        bounded("level", Number(Unit::Ratio), 0.0, 4.0),
                    ],
                    asset: Some(MediaAssetUsage::Spectral),
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::DelayLine => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "delay_line",
                    inputs: &[
                        required("audio", AudioMono),
                        optional("pitch", Hertz),
                        optional("time_mod", AudioMono),
                    ],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[
                        setting("mode", Text(&["time", "pitch"])),
                        bounded("time", Number(Unit::Seconds), 0.0, 2.0),
                        bounded("ratio", Number(Unit::Ratio), 0.01, 16.0),
                        bounded("max_time", Number(Unit::Seconds), 0.001, 2.0),
                        bounded("mod_depth", Number(Unit::Ratio), 0.0, 1.0),
                        setting("interpolation", Text(&["linear", "lagrange3", "allpass"])),
                        bounded("tune_compensation", Number(Unit::Ratio), 0.0, 16.0),
                    ],
                    breaks_feedback: true,
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Math => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "math",
                    inputs: &[required("a", AudioMono), optional("b", AudioMono)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[
                        setting(
                            "operation",
                            Text(&[
                                "add", "subtract", "multiply", "min", "max", "abs", "negate",
                                "offset", "scale",
                            ]),
                        ),
                        bounded("amount", Number(Unit::Ratio), -64.0, 64.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::StereoMath => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "stereo_math",
                    inputs: &[required("a", AudioStereo), optional("b", AudioStereo)],
                    outputs: &[output("audio", AudioStereo)],
                    settings: &[
                        setting(
                            "operation",
                            Text(&[
                                "add", "subtract", "multiply", "min", "max", "abs", "negate",
                                "offset", "scale",
                            ]),
                        ),
                        bounded("amount", Number(Unit::Ratio), -64.0, 64.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Shaper => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "shaper",
                    inputs: &[required("audio", AudioMono)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[
                        setting(
                            "curve",
                            Text(&[
                                "tanh",
                                "hard_clip",
                                "soft_clip",
                                "one_sided",
                                "asymmetric",
                                "table",
                            ]),
                        ),
                        bounded("drive", Number(Unit::Ratio), 0.0, 64.0),
                        bounded("bias", Number(Unit::Ratio), -16.0, 16.0),
                        bounded("asymmetry", Number(Unit::Ratio), 0.0, 1.0),
                    ],
                    directives: &["point"],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::ToAudio => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "to_audio",
                    inputs: &[required("control", Control)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::ToControl => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "to_control",
                    inputs: &[required("audio", AudioMono)],
                    outputs: &[output("control", Control)],
                    settings: &[],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::DcBlock => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "dc_block",
                    inputs: &[required("audio", AudioMono)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[bounded("cutoff", Number(Unit::Hertz), 1.0, 500.0)],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::LinearPhaseLowpass => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "linear_phase_lowpass",
                    inputs: &[required("audio", AudioMono)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[bounded("amount", Number(Unit::Ratio), 0.0, 1.0)],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Biquad => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "biquad",
                    inputs: &[required("audio", AudioMono), optional("key", Hertz)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[
                        setting(
                            "mode",
                            Text(&[
                                "lowpass",
                                "highpass",
                                "bandpass",
                                "notch",
                                "peak",
                                "lowshelf",
                                "highshelf",
                                "allpass",
                            ]),
                        ),
                        bounded("frequency", Number(Unit::Hertz), 1.0, 24000.0),
                        bounded("q", Number(Unit::Ratio), 0.05, 64.0),
                        bounded("gain", Number(Unit::Decibels), -48.0, 48.0),
                        bounded("key_track", Number(Unit::Ratio), 0.0, 1.0),
                        bounded("key_reference", Number(Unit::Hertz), 1.0, 24000.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::OnePole => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "onepole",
                    inputs: &[required("audio", AudioMono)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[
                        setting("mode", Text(&["lowpass", "highpass", "leak"])),
                        bounded("cutoff", Number(Unit::Hertz), 1.0, 24000.0),
                        bounded("time", Number(Unit::Seconds), 1e-5, 60.0),
                    ],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::BiquadChain => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "biquad_chain",
                    inputs: &[required("audio", AudioMono), optional("key", Hertz)],
                    outputs: &[output("audio", AudioMono)],
                    settings: &[],
                    directives: &["band"],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
            NodeKind::Constant => {
                const DEFINITION: NodeDefinition = NodeDefinition {
                    name: "constant",
                    inputs: &[],
                    outputs: &[output("pitch", Hertz)],
                    settings: &[bounded("frequency", Number(Unit::Hertz), 1.0, 24000.0)],
                    ..NodeDefinition::PLAIN
                };
                &DEFINITION
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::str::FromStr;

    use super::*;

    pub(crate) const ALL: [NodeKind; 39] = [
        NodeKind::Exciter,
        NodeKind::ModalBank,
        NodeKind::Oscillator,
        NodeKind::Unison,
        NodeKind::Noise,
        NodeKind::Envelope,
        NodeKind::Mixer,
        NodeKind::Lowpass,
        NodeKind::StereoFilter,
        NodeKind::Amplifier,
        NodeKind::Saturation,
        NodeKind::Pan,
        NodeKind::Delay,
        NodeKind::Reverb,
        NodeKind::Output,
        NodeKind::Lfo,
        NodeKind::MonoToStereo,
        NodeKind::StereoToMono,
        NodeKind::StereoMixer,
        NodeKind::StereoSum,
        NodeKind::Sum,
        NodeKind::AnalogOscillator,
        NodeKind::Wavetable,
        NodeKind::SampleSource,
        NodeKind::Multisample,
        NodeKind::Granular,
        NodeKind::Spectral,
        NodeKind::DelayLine,
        NodeKind::Math,
        NodeKind::StereoMath,
        NodeKind::Shaper,
        NodeKind::ToAudio,
        NodeKind::ToControl,
        NodeKind::DcBlock,
        NodeKind::LinearPhaseLowpass,
        NodeKind::Biquad,
        NodeKind::OnePole,
        NodeKind::BiquadChain,
        NodeKind::Constant,
    ];

    #[test]
    fn every_definition_is_self_consistent() {
        for kind in ALL {
            let definition = kind.definition();
            assert_eq!(NodeKind::from_str(definition.name).ok(), Some(kind));
            for (index, port) in definition.inputs.iter().enumerate() {
                assert!(
                    definition.inputs[..index]
                        .iter()
                        .all(|other| other.name != port.name),
                    "{} repeats input `{}`",
                    definition.name,
                    port.name
                );
            }
            for (index, spec) in definition.settings.iter().enumerate() {
                assert!(
                    definition.settings[..index]
                        .iter()
                        .all(|other| other.key != spec.key),
                    "{} repeats setting `{}`",
                    definition.name,
                    spec.key
                );
                if let Some(domain) = spec.domain {
                    assert!(
                        domain.minimum <= domain.maximum,
                        "{} {}",
                        definition.name,
                        spec.key
                    );
                }
            }
            assert_eq!(
                definition.asset.is_some(),
                definition.setting("asset").is_some(),
                "{} asset role must match its `asset` setting",
                definition.name
            );
            assert!(!definition.outputs.is_empty() || kind == NodeKind::Output);
        }
    }

    #[test]
    fn summing_inputs_are_bounded_by_max_inputs() {
        for kind in ALL {
            let definition = kind.definition();
            let summing = definition
                .inputs
                .iter()
                .any(|port| port.cardinality == Cardinality::Summing);
            assert_eq!(summing, definition.setting("max_inputs").is_some());
            assert_eq!(summing, kind.is_summing());
        }
    }
}
