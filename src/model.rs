//! Immutable authored model produced by parsing and consumed by validation.

use std::{collections::BTreeMap, fmt, path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    diagnostic::SourceLocation,
    time::{Meter, MusicalTime},
};

/// The supported Crump source-language major version.
pub const SOURCE_VERSION: u32 = 1;

/// A fully parsed Project candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Source-language major version.
    pub source_version: u32,
    /// Stable Project identity.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Stable seed used by deterministic generators and synthesized noise.
    pub seed: u64,
    /// Playback and render sample rate.
    pub sample_rate: u32,
    /// Explicit effect/release tail after the arrangement end.
    pub render_tail_seconds: f64,
    /// Portable exact-byte WAV declarations owned by this Project.
    pub assets: Vec<MediaAsset>,
    /// The Project's one Song.
    pub song: Song,
    /// Imported source paths, in declaration order after canonicalization.
    pub imports: Vec<PathBuf>,
}

/// One portable exact-byte media declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAsset {
    /// Durable Project-local identity.
    pub id: String,
    /// Human-facing name.
    pub name: String,
    /// Normalized Project-relative WAV path.
    pub path: PathBuf,
    /// Required lowercase SHA-256 of exact file bytes.
    pub sha256: String,
    /// Path literal used by checked source edits.
    pub path_site: ValueSite,
    /// Hash literal used by checked source edits.
    pub sha256_site: ValueSite,
    /// Declaration location.
    pub location: SourceLocation,
}

/// One finite linear arrangement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Song {
    /// Fixed first-release tempo.
    pub tempo_bpm: f64,
    /// Fixed first-release meter.
    pub meter: Meter,
    /// Positive finite arrangement end.
    pub end: MusicalTime,
    /// Authored arrangement-end literal.
    pub end_site: ValueSite,
    /// Native instruments addressable by Tracks.
    pub instruments: Vec<Instrument>,
    /// Reusable finite musical material.
    pub patterns: Vec<Pattern>,
    /// Reusable deterministic timing and velocity templates.
    pub grooves: Vec<GrooveTemplate>,
    /// Presentation-ordered timeline Tracks.
    pub tracks: Vec<Track>,
    /// Routing-only hierarchical Group channels.
    pub groups: Vec<Group>,
    /// Routing-only parallel Return channels.
    pub returns: Vec<Return>,
    /// Explicit singleton Master, or the backward-compatible implicit Master.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master: Option<Master>,
    /// Named auxiliary Audio routes into processor sidechain ports.
    pub sidechains: Vec<SidechainRoute>,
    /// Named arrangement positions.
    pub locators: Vec<Locator>,
    /// Song-scoped absolute point Automation lanes.
    pub automations: Vec<AutomationLane>,
    /// Authored normalized LFO routes, ordered at runtime by durable route ID.
    pub modulations: Vec<ModulationRoute>,
    /// Declaration location, used by checked lane insertion.
    pub location: SourceLocation,
}

/// A native instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instrument {
    /// Durable identity within the Project.
    pub id: String,
    /// Human-facing name.
    pub name: String,
    /// Native implementation kind.
    pub kind: InstrumentKind,
    /// Maximum simultaneous voices.
    pub polyphony: u16,
    /// Physical-unit Parameters.
    pub parameters: Vec<Parameter>,
    /// Shallow progressive controls.
    pub faceplate: Vec<FaceplateControl>,
    /// Declarative graph for a graph-authored synth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<Graph>,
    /// Portable zone map for a sample-backed Instrument.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampler: Option<SamplerDefinition>,
    /// Ordered shared stereo processors after the per-voice sum.
    pub post_processors: Vec<Processor>,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Native instrument implementation kind.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentKind {
    /// A pitched polyphonic Composable DSP Graph.
    Graph,
    /// Crump's stock no-sample percussion instrument.
    StockDrums,
    /// A portable one-shot or key-mapped WAV Instrument.
    Sampler,
}

impl fmt::Display for InstrumentKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Graph => "graph",
            Self::StockDrums => "stock_drums",
            Self::Sampler => "sampler",
        })
    }
}

/// Immutable authored sample map compiled away from the audio callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplerDefinition {
    /// Durably identified zones. Their key ranges must not overlap.
    pub mappings: Vec<SampleMapping>,
    /// Declaration location used by inspection and diagnostics.
    pub location: SourceLocation,
}

/// One portable source region mapped to an inclusive MIDI key range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleMapping {
    /// Durable identity within the Instrument.
    pub id: String,
    /// Project media asset identity.
    pub asset: String,
    /// Inclusive lowest mapped MIDI pitch.
    pub low_key: u8,
    /// Inclusive highest mapped MIDI pitch.
    pub high_key: u8,
    /// MIDI pitch that plays the source at its recorded rate.
    pub root_key: u8,
    /// Inclusive region start in original source frames.
    pub start_frame: u64,
    /// Exclusive region end in original source frames.
    pub end_frame: u64,
    /// Whether Note Off releases playback or the complete region always plays.
    pub playback: SamplePlayback,
    /// Mapping trim in decibels.
    pub gain_db: f64,
    /// Mapping pan from left (-1) to right (+1).
    pub pan: f64,
    /// Linear attack time in seconds.
    pub attack_seconds: f64,
    /// Linear release time in seconds.
    pub release_seconds: f64,
    /// Click-prevention fade at the source-region start, in original source frames.
    pub fade_in_frames: u32,
    /// Click-prevention fade at the source-region end, in original source frames.
    pub fade_out_frames: u32,
    /// Optional durable choke group. Starting one zone releases peers in the group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choke_group: Option<String>,
    /// Complete mapping block location.
    pub location: SourceLocation,
    /// Region-start literal for checked source edits.
    pub start_site: ValueSite,
    /// Region-end literal for checked source edits.
    pub end_site: ValueSite,
    /// Gain literal for checked source edits.
    pub gain_site: ValueSite,
    /// Pan literal for checked source edits.
    pub pan_site: ValueSite,
    /// Fade-in literal for checked source edits.
    pub fade_in_site: ValueSite,
    /// Fade-out literal for checked source edits.
    pub fade_out_site: ValueSite,
}

/// Gate behavior for one mapped sample zone.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SamplePlayback {
    /// Play the complete selected region regardless of Note Off.
    OneShot,
    /// Enter the authored release stage on Note Off.
    Gated,
}

impl FromStr for SamplePlayback {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "one_shot" | "oneshot" => Ok(Self::OneShot),
            "gated" | "gate" => Ok(Self::Gated),
            _ => Err(ValueError::UnknownSamplePlayback(value.to_owned())),
        }
    }
}

/// A physical-unit instrument Parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    /// Immutable owner-local key.
    pub key: String,
    /// Human-facing exact-parameter label.
    pub name: String,
    /// Canonical physical unit.
    pub unit: Unit,
    /// Inclusive legal lower bound.
    pub minimum: f64,
    /// Inclusive legal upper bound.
    pub maximum: f64,
    /// Device default in physical units.
    pub default: f64,
    /// Authored base value in physical units.
    pub value: f64,
    /// Whether Song Automation may replace this Parameter's base value.
    pub automatable: bool,
    /// Whether a graph Control signal may modulate this Parameter.
    pub modulatable: bool,
    /// Literal token used by checked Structured Source Edits.
    pub value_site: ValueSite,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Canonical first-release Parameter units.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Unitless ratio or normalized amount.
    Ratio,
    /// Frequency in hertz.
    Hertz,
    /// Duration in seconds.
    Seconds,
    /// Gain in decibels.
    Decibels,
    /// Pitch in semitones.
    Semitones,
    /// Stereo position from -1 to 1.
    Pan,
    /// Discrete authored false/true state represented internally as 0/1.
    Boolean,
}

impl Unit {
    /// Parses a physical literal and rejects mismatched suffixes.
    pub fn parse_value(self, value: &str) -> Result<f64, ValueError> {
        if self == Self::Boolean {
            return match value {
                "false" => Ok(0.0),
                "true" => Ok(1.0),
                _ => Err(ValueError::InvalidBoolean(value.to_owned())),
            };
        }
        let (number, suffix) = split_number_suffix(value)?;
        let parsed = number
            .parse::<f64>()
            .map_err(|_| ValueError::InvalidNumber)?;
        if !parsed.is_finite() {
            return Err(ValueError::NonFinite);
        }
        let multiplier = match (self, suffix) {
            (Self::Ratio | Self::Pan | Self::Semitones, "") => 1.0,
            (Self::Ratio, "%") => 0.01,
            (Self::Hertz, "hz" | "Hz") => 1.0,
            (Self::Hertz, "khz" | "kHz") => 1_000.0,
            (Self::Seconds, "s") => 1.0,
            (Self::Seconds, "ms") => 0.001,
            (Self::Decibels, "db" | "dB") => 1.0,
            (Self::Semitones, "st") => 1.0,
            (Self::Pan, "pan") => 1.0,
            (Self::Boolean, _) => unreachable!("Boolean literals return before numeric parsing"),
            _ => {
                return Err(ValueError::WrongUnit {
                    expected: self,
                    suffix: suffix.to_owned(),
                });
            }
        };
        Ok(parsed * multiplier)
    }

    /// Formats a physical value for source replacement.
    #[must_use]
    pub fn format_value(self, value: f64) -> String {
        if self == Self::Boolean {
            return if value >= 0.5 { "true" } else { "false" }.to_owned();
        }
        let number = format_number(value);
        match self {
            Self::Ratio | Self::Semitones | Self::Pan => number,
            Self::Hertz => format!("{number}hz"),
            Self::Seconds => format!("{number}s"),
            Self::Decibels => format!("{number}db"),
            Self::Boolean => unreachable!("Boolean values return before numeric formatting"),
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Ratio => "ratio",
            Self::Hertz => "hz",
            Self::Seconds => "seconds",
            Self::Decibels => "db",
            Self::Semitones => "semitones",
            Self::Pan => "pan",
            Self::Boolean => "boolean",
        })
    }
}

impl FromStr for Unit {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ratio" => Ok(Self::Ratio),
            "hz" | "hertz" => Ok(Self::Hertz),
            "seconds" | "second" | "s" => Ok(Self::Seconds),
            "db" | "decibels" => Ok(Self::Decibels),
            "semitones" | "st" => Ok(Self::Semitones),
            "pan" => Ok(Self::Pan),
            "boolean" | "bool" => Ok(Self::Boolean),
            _ => Err(ValueError::UnknownUnit(value.to_owned())),
        }
    }
}

/// Explicit panner behavior for a stereo channel strip.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Panner {
    /// Equal-power placement after collapsing a stereo input to mono.
    MonoPan {
        /// Position from left (-1) to right (+1).
        position: f64,
        /// Source literal for checked edits.
        site: ValueSite,
    },
    /// Attenuating stereo balance without crossfeed.
    Balance {
        /// Balance from left (-1) to right (+1).
        position: f64,
        /// Source literal for checked edits.
        site: ValueSite,
    },
    /// Independent equal-power positions for the two input sides.
    StereoPan {
        /// Position of the input left side.
        left: f64,
        /// Position of the input right side.
        right: f64,
        /// Left position source literal.
        left_site: ValueSite,
        /// Right position source literal.
        right_site: ValueSite,
    },
}

impl Panner {
    /// Returns the legacy primary position used by shallow Track editing.
    #[must_use]
    pub const fn primary_position(&self) -> f64 {
        match self {
            Self::MonoPan { position, .. } | Self::Balance { position, .. } => *position,
            Self::StereoPan { left, right, .. } => (*left + *right) * 0.5,
        }
    }

    /// Returns the first editable source position.
    #[must_use]
    pub const fn primary_site(&self) -> &ValueSite {
        match self {
            Self::MonoPan { site, .. } | Self::Balance { site, .. } => site,
            Self::StereoPan { left_site, .. } => left_site,
        }
    }

    /// Stable position Parameters exposed by this explicit panner mode.
    #[must_use]
    pub const fn parameters(&self) -> &'static [TrackParameter] {
        match self {
            Self::MonoPan { .. } => &[TrackParameter::Pan],
            Self::Balance { .. } => &[TrackParameter::Balance],
            Self::StereoPan { .. } => &[TrackParameter::LeftPan, TrackParameter::RightPan],
        }
    }

    /// Resolves one mode-specific panner Parameter to its authored value and source site.
    #[must_use]
    pub const fn parameter_value_site(
        &self,
        parameter: TrackParameter,
    ) -> Option<(f64, &ValueSite)> {
        match (self, parameter) {
            (Self::MonoPan { position, site }, TrackParameter::Pan)
            | (Self::Balance { position, site }, TrackParameter::Balance) => {
                Some((*position, site))
            }
            (
                Self::StereoPan {
                    left, left_site, ..
                },
                TrackParameter::LeftPan,
            ) => Some((*left, left_site)),
            (
                Self::StereoPan {
                    right, right_site, ..
                },
                TrackParameter::RightPan,
            ) => Some((*right, right_site)),
            _ => None,
        }
    }
}

/// One concrete native channel or Master processor kind.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorKind {
    /// Gain, polarity, mono conversion, and correlation-preserving width narrowing.
    Utility,
    /// Multi-band channel equalizer.
    Eq,
    /// Feed-forward compressor with an optional external sidechain.
    Compressor,
    /// Fractional stereo delay.
    Delay,
    /// Shared stereo algorithmic reverberator.
    Reverb,
    /// Multi-phase soft saturation/clipper.
    Saturation,
    /// Lookahead inter-sample-peak-aware Master limiter.
    Limiter,
    /// Explicit deterministic latency used for routing and compensation tests.
    Latency,
}

impl FromStr for ProcessorKind {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "utility" => Ok(Self::Utility),
            "eq" | "equalizer" => Ok(Self::Eq),
            "compressor" | "comp" => Ok(Self::Compressor),
            "delay" => Ok(Self::Delay),
            "reverb" => Ok(Self::Reverb),
            "saturation" | "saturator" => Ok(Self::Saturation),
            "limiter" => Ok(Self::Limiter),
            "latency" => Ok(Self::Latency),
            _ => Err(ValueError::UnknownProcessor(value.to_owned())),
        }
    }
}

/// One source-backed processor setting literal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessorSetting {
    /// Immutable setting key defined by the processor kind.
    pub key: String,
    /// Exact authored literal interpreted by the processor schema.
    pub value: String,
    /// Literal token used by checked edits.
    pub value_site: ValueSite,
    /// Declaration location.
    pub location: SourceLocation,
}

/// One durably identified processor instance in an ordered chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Processor {
    /// Song-wide durable device identity.
    pub id: String,
    /// Native processor implementation.
    pub kind: ProcessorKind,
    /// Kind-defined settings in declaration order.
    pub settings: Vec<ProcessorSetting>,
    /// Named auxiliary stereo Audio input ports.
    pub sidechain_inputs: Vec<String>,
    /// Declared deterministic output latency for the active configuration.
    pub latency_frames: u32,
    /// Explicit latency literal when one was authored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_site: Option<ValueSite>,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Stable authored channel identity used by Audio routing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(tag = "role", content = "id", rename_all = "snake_case")]
pub enum ChannelRef {
    /// Timeline Track channel.
    Track(String),
    /// Hierarchical Group channel.
    Group(String),
    /// Parallel Return channel.
    Return(String),
}

impl fmt::Display for ChannelRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Track(id) => write!(formatter, "track/{id}"),
            Self::Group(id) => write!(formatter, "group/{id}"),
            Self::Return(id) => write!(formatter, "return/{id}"),
        }
    }
}

impl FromStr for ChannelRef {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.split_once('/') {
            Some(("track", id)) if !id.is_empty() => Ok(Self::Track(id.to_owned())),
            Some(("group", id)) if !id.is_empty() => Ok(Self::Group(id.to_owned())),
            Some(("return", id)) if !id.is_empty() => Ok(Self::Return(id.to_owned())),
            _ => Err(ValueError::InvalidChannelRef(value.to_owned())),
        }
    }
}

/// Main-output destination of one Track, Group, or Return.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(tag = "role", content = "id", rename_all = "snake_case")]
pub enum OutputTarget {
    /// Song singleton Master.
    Master,
    /// Parent Group.
    Group(String),
    /// Later Return, legal only for Return outputs.
    Return(String),
}

impl fmt::Display for OutputTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Master => formatter.write_str("master"),
            Self::Group(id) => write!(formatter, "group/{id}"),
            Self::Return(id) => write!(formatter, "return/{id}"),
        }
    }
}

impl FromStr for OutputTarget {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "master" {
            return Ok(Self::Master);
        }
        match value.split_once('/') {
            Some(("group", id)) if !id.is_empty() => Ok(Self::Group(id.to_owned())),
            Some(("return", id)) if !id.is_empty() => Ok(Self::Return(id.to_owned())),
            _ => Err(ValueError::InvalidOutputTarget(value.to_owned())),
        }
    }
}

/// Fixed source point for a parallel Send or sidechain route.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TapPoint {
    /// After inserts and before channel gain/pan/mute.
    PreFader,
    /// After channel gain/pan/mute.
    PostFader,
}

impl FromStr for TapPoint {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pre" | "pre_fader" => Ok(Self::PreFader),
            "post" | "post_fader" => Ok(Self::PostFader),
            _ => Err(ValueError::UnknownTapPoint(value.to_owned())),
        }
    }
}

/// One durably identified parallel branch to a Return.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Send {
    /// Song-wide durable identity.
    pub id: String,
    /// Destination Return ID.
    pub target: String,
    /// Explicit pre/post channel tap.
    pub tap: TapPoint,
    /// Source-backed branch gain in decibels.
    pub level_db: f64,
    /// Gain literal for checked edits.
    pub level_site: ValueSite,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Shared fixed channel-strip state for Tracks, Groups, and Returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStrip {
    /// Ordered insert chain.
    pub inserts: Vec<Processor>,
    /// Explicit pre/post Sends.
    pub sends: Vec<Send>,
    /// Source-backed channel fader in decibels.
    pub level_db: f64,
    /// Explicit panner mode and authored positions.
    pub panner: Panner,
    /// Source-backed mute state.
    pub mute: bool,
    /// Main output target.
    pub output: OutputTarget,
    /// Fader literal used by checked edits.
    pub level_site: ValueSite,
    /// Mute literal used by checked edits, when explicitly authored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute_site: Option<ValueSite>,
}

/// Routing-only hierarchical summing channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// Durable Group identity.
    pub id: String,
    /// Human-facing name.
    pub name: String,
    /// Fixed channel strip.
    pub channel: ChannelStrip,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Routing-only parallel processing channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Return {
    /// Durable Return identity.
    pub id: String,
    /// Human-facing name.
    pub name: String,
    /// Fixed channel strip.
    pub channel: ChannelStrip,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Song singleton final processing channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Master {
    /// Ordered authored Master chain.
    pub inserts: Vec<Processor>,
    /// Source-backed final fader in decibels.
    pub level_db: f64,
    /// Final fader literal used by checked edits.
    pub level_site: ValueSite,
    /// Declaration location.
    pub location: SourceLocation,
}

/// One auxiliary Audio Bus from a channel tap to a named processor port.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidechainRoute {
    /// Durable route identity.
    pub id: String,
    /// Source channel.
    pub from: ChannelRef,
    /// Source channel tap.
    pub tap: TapPoint,
    /// Destination processor identity.
    pub processor: String,
    /// Destination named auxiliary Audio port.
    pub input: String,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Checked source ranges for one reusable Groove Template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrooveSites {
    /// Grid literal.
    pub grid: ValueSite,
    /// Swing-ratio literal.
    pub swing: ValueSite,
    /// Timing-humanization literal.
    pub timing: ValueSite,
    /// Velocity-humanization literal.
    pub velocity: ValueSite,
    /// Event-probability literal.
    pub probability: ValueSite,
    /// Deterministic seed literal.
    pub seed: ValueSite,
}

/// One reusable deterministic timing, velocity, and probability template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrooveTemplate {
    /// Durable Song-wide identity.
    pub id: String,
    /// Exact subdivision grid.
    pub grid: MusicalTime,
    /// Delayed odd-subdivision ratio, where 0.5 is straight.
    pub swing: f64,
    /// Maximum deterministic signed timing displacement.
    pub timing: MusicalTime,
    /// Maximum deterministic signed normalized velocity displacement.
    pub velocity: f64,
    /// Independent deterministic event inclusion probability.
    pub probability: f64,
    /// Authored deterministic random seed.
    pub seed: u64,
    /// Checked value ranges.
    pub sites: GrooveSites,
    /// Declaration location.
    pub location: SourceLocation,
}

/// One named Song position used for navigation and ranges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Locator {
    /// Durable locator identity.
    pub id: String,
    /// Human-facing name.
    pub name: String,
    /// Exact Song time.
    pub at: MusicalTime,
    /// Authored time literal.
    pub at_site: ValueSite,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Stable identity of an addressable Parameter owner and immutable key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(tag = "owner", rename_all = "snake_case")]
pub enum ParameterTarget {
    /// A Parameter declared by a durable Instrument definition.
    Instrument {
        /// Durable Instrument ID.
        instrument: String,
        /// Immutable owner-local Parameter key.
        parameter: String,
    },
    /// A built-in Track channel Parameter.
    Track {
        /// Durable Track ID.
        track: String,
        /// Immutable built-in channel key.
        parameter: TrackParameter,
    },
    /// A built-in Group channel Parameter.
    Group {
        /// Durable Group ID.
        group: String,
        /// Immutable built-in channel key.
        parameter: TrackParameter,
    },
    /// A built-in Return channel Parameter.
    Return {
        /// Durable Return ID.
        r#return: String,
        /// Immutable built-in channel key.
        parameter: TrackParameter,
    },
    /// A durably identified Send gain.
    Send {
        /// Durable Send ID.
        send: String,
    },
    /// A kind-defined native processor scalar.
    Processor {
        /// Durable processor instance ID.
        processor: String,
        /// Immutable kind-defined setting key.
        parameter: String,
    },
    /// The intrinsic Master fader.
    Master,
}

impl fmt::Display for ParameterTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Instrument {
                instrument,
                parameter,
            } => {
                write!(formatter, "instrument/{instrument}/{parameter}")
            }
            Self::Track { track, parameter } => write!(formatter, "track/{track}/{parameter}"),
            Self::Group { group, parameter } => write!(formatter, "group/{group}/{parameter}"),
            Self::Return {
                r#return,
                parameter,
            } => {
                write!(
                    formatter,
                    "return/{return}/{parameter}",
                    r#return = r#return
                )
            }
            Self::Send { send } => write!(formatter, "send/{send}/level"),
            Self::Processor {
                processor,
                parameter,
            } => {
                write!(formatter, "processor/{processor}/{parameter}")
            }
            Self::Master => formatter.write_str("master/level"),
        }
    }
}

impl FromStr for ParameterTarget {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parts = value.split('/').collect::<Vec<_>>();
        match parts.as_slice() {
            ["instrument", instrument, parameter]
                if !instrument.is_empty() && !parameter.is_empty() =>
            {
                Ok(Self::Instrument {
                    instrument: (*instrument).to_owned(),
                    parameter: (*parameter).to_owned(),
                })
            }
            ["track", track, parameter] if !track.is_empty() => Ok(Self::Track {
                track: (*track).to_owned(),
                parameter: TrackParameter::from_str(parameter)?,
            }),
            ["group", group, parameter] if !group.is_empty() => Ok(Self::Group {
                group: (*group).to_owned(),
                parameter: TrackParameter::from_str(parameter)?,
            }),
            ["return", r#return, parameter] if !r#return.is_empty() => Ok(Self::Return {
                r#return: (*r#return).to_owned(),
                parameter: TrackParameter::from_str(parameter)?,
            }),
            ["send", send, "level"] if !send.is_empty() => Ok(Self::Send {
                send: (*send).to_owned(),
            }),
            ["processor", processor, parameter]
                if !processor.is_empty() && !parameter.is_empty() =>
            {
                Ok(Self::Processor {
                    processor: (*processor).to_owned(),
                    parameter: (*parameter).to_owned(),
                })
            }
            ["master", "level"] => Ok(Self::Master),
            _ => Err(ValueError::InvalidParameterTarget(value.to_owned())),
        }
    }
}

/// Immutable built-in channel Parameter key.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TrackParameter {
    /// Channel fader in decibels.
    Level,
    /// Equal-power mono panner position from -1 to 1.
    Pan,
    /// Stereo balance from -1 to 1.
    Balance,
    /// Independent stereo-panner position for the left input.
    LeftPan,
    /// Independent stereo-panner position for the right input.
    RightPan,
    /// Source-backed step-only channel mute.
    Mute,
}

impl fmt::Display for TrackParameter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Level => "level",
            Self::Pan => "pan",
            Self::Balance => "balance",
            Self::LeftPan => "left_pan",
            Self::RightPan => "right_pan",
            Self::Mute => "mute",
        })
    }
}

impl FromStr for TrackParameter {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "level" => Ok(Self::Level),
            "pan" => Ok(Self::Pan),
            "balance" => Ok(Self::Balance),
            "left_pan" => Ok(Self::LeftPan),
            "right_pan" => Ok(Self::RightPan),
            "mute" => Ok(Self::Mute),
            _ => Err(ValueError::UnknownTrackParameter(value.to_owned())),
        }
    }
}

/// Resolved physical domain and base value for one Parameter target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParameterSpec {
    /// Canonical physical unit.
    pub unit: Unit,
    /// Inclusive legal lower bound.
    pub minimum: f64,
    /// Inclusive legal upper bound.
    pub maximum: f64,
    /// Authored base value before Automation.
    pub base: f64,
    /// Whether Song Automation is legal for this target.
    pub automatable: bool,
    /// Whether normalized routed Modulation is legal for this target.
    pub modulatable: bool,
}

impl Project {
    /// Resolves a stable target through the authoritative Parameter inventory.
    #[must_use]
    pub fn parameter_spec(&self, target: &ParameterTarget) -> Option<ParameterSpec> {
        self.parameter_specs()
            .into_iter()
            .find_map(|(candidate, spec)| (candidate == *target).then_some(spec))
    }

    /// Enumerates every authored Instrument, mixer, routing, and processor Parameter.
    #[must_use]
    pub fn parameter_specs(&self) -> Vec<(ParameterTarget, ParameterSpec)> {
        let mut targets = Vec::new();
        for instrument in &self.song.instruments {
            for parameter in &instrument.parameters {
                targets.push((
                    ParameterTarget::Instrument {
                        instrument: instrument.id.clone(),
                        parameter: parameter.key.clone(),
                    },
                    ParameterSpec {
                        unit: parameter.unit,
                        minimum: parameter.minimum,
                        maximum: parameter.maximum,
                        base: parameter.value,
                        automatable: parameter.automatable,
                        modulatable: parameter.modulatable,
                    },
                ));
            }
        }
        for track in &self.song.tracks {
            extend_channel_parameter_specs(&mut targets, &track.channel, |parameter| {
                ParameterTarget::Track {
                    track: track.id.clone(),
                    parameter,
                }
            });
        }
        for group in &self.song.groups {
            extend_channel_parameter_specs(&mut targets, &group.channel, |parameter| {
                ParameterTarget::Group {
                    group: group.id.clone(),
                    parameter,
                }
            });
        }
        for channel in &self.song.returns {
            extend_channel_parameter_specs(&mut targets, &channel.channel, |parameter| {
                ParameterTarget::Return {
                    r#return: channel.id.clone(),
                    parameter,
                }
            });
        }
        for channel in self
            .song
            .tracks
            .iter()
            .map(|track| &track.channel)
            .chain(self.song.groups.iter().map(|group| &group.channel))
            .chain(self.song.returns.iter().map(|channel| &channel.channel))
        {
            for send in &channel.sends {
                targets.push((
                    ParameterTarget::Send {
                        send: send.id.clone(),
                    },
                    ParameterSpec {
                        unit: Unit::Decibels,
                        minimum: -96.0,
                        maximum: 12.0,
                        base: send.level_db,
                        automatable: true,
                        modulatable: true,
                    },
                ));
            }
        }
        for processor in all_processors(&self.song) {
            for setting in &processor.settings {
                if let Some(spec) =
                    processor_setting_spec(processor.kind, &setting.key, &setting.value)
                {
                    targets.push((
                        ParameterTarget::Processor {
                            processor: processor.id.clone(),
                            parameter: setting.key.clone(),
                        },
                        spec,
                    ));
                }
            }
        }
        if let Some(master) = &self.song.master {
            targets.push((
                ParameterTarget::Master,
                ParameterSpec {
                    unit: Unit::Decibels,
                    minimum: -96.0,
                    maximum: 12.0,
                    base: master.level_db,
                    automatable: true,
                    modulatable: true,
                },
            ));
        }
        targets.sort_by(|(left, _), (right, _)| left.cmp(right));
        targets
    }

    /// Enumerates continuous routing, mixer, and shared-processor Parameters.
    #[must_use]
    pub fn mix_parameter_specs(&self) -> Vec<(ParameterTarget, ParameterSpec)> {
        self.parameter_specs()
            .into_iter()
            .filter(|(target, _)| !matches!(target, ParameterTarget::Instrument { .. }))
            .collect()
    }
}

fn extend_channel_parameter_specs(
    targets: &mut Vec<(ParameterTarget, ParameterSpec)>,
    channel: &ChannelStrip,
    target: impl Fn(TrackParameter) -> ParameterTarget,
) {
    for parameter in [TrackParameter::Level, TrackParameter::Mute]
        .into_iter()
        .chain(channel.panner.parameters().iter().copied())
    {
        if let Some(spec) = channel_parameter_spec(channel, parameter) {
            targets.push((target(parameter), spec));
        }
    }
}

fn channel_parameter_spec(
    channel: &ChannelStrip,
    parameter: TrackParameter,
) -> Option<ParameterSpec> {
    Some(match parameter {
        TrackParameter::Level => ParameterSpec {
            unit: Unit::Decibels,
            minimum: -96.0,
            maximum: 12.0,
            base: channel.level_db,
            automatable: true,
            modulatable: true,
        },
        TrackParameter::Mute => ParameterSpec {
            unit: Unit::Boolean,
            minimum: 0.0,
            maximum: 1.0,
            base: f64::from(channel.mute),
            automatable: true,
            modulatable: false,
        },
        parameter @ (TrackParameter::Pan
        | TrackParameter::Balance
        | TrackParameter::LeftPan
        | TrackParameter::RightPan) => {
            let (base, _) = channel.panner.parameter_value_site(parameter)?;
            ParameterSpec {
                unit: Unit::Pan,
                minimum: -1.0,
                maximum: 1.0,
                base,
                automatable: true,
                modulatable: true,
            }
        }
    })
}

fn all_processors(song: &Song) -> impl Iterator<Item = &Processor> {
    song.instruments
        .iter()
        .flat_map(|instrument| &instrument.post_processors)
        .chain(song.tracks.iter().flat_map(|track| &track.channel.inserts))
        .chain(song.groups.iter().flat_map(|group| &group.channel.inserts))
        .chain(
            song.returns
                .iter()
                .flat_map(|channel| &channel.channel.inserts),
        )
        .chain(song.master.iter().flat_map(|master| master.inserts.iter()))
}

/// Resolves one processor setting literal to its continuous control contract.
#[must_use]
pub fn processor_setting_spec(
    kind: ProcessorKind,
    key: &str,
    literal: &str,
) -> Option<ParameterSpec> {
    let (unit, minimum, maximum) = processor_setting_contract(kind, key)?;
    let base = unit.parse_value(literal).ok()?;
    Some(ParameterSpec {
        unit,
        minimum,
        maximum,
        base,
        automatable: true,
        modulatable: true,
    })
}

/// Returns the unit and legal inclusive domain of a continuous processor setting.
#[must_use]
pub fn processor_setting_contract(kind: ProcessorKind, key: &str) -> Option<(Unit, f64, f64)> {
    let (unit, minimum, maximum, _) = crate::processor_semantics::descriptor(kind, key)?
        .contract
        .continuous()?;
    Some((unit, minimum, maximum))
}

/// Resolves a tempo-synchronized Delay division to quarter-note beats.
#[must_use]
pub fn delay_division_beats(literal: &str) -> Option<f64> {
    crate::processor_semantics::delay_division_beats(literal)
}

/// Resolves a tempo-synchronized Delay division to seconds.
#[must_use]
pub fn delay_division_seconds(literal: &str, tempo_bpm: f64) -> Option<f64> {
    crate::processor_semantics::delay_division_seconds(literal, tempo_bpm)
}

/// One Song-scoped absolute point Automation lane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationLane {
    /// Durable lane identity.
    pub id: String,
    /// Stable Parameter target.
    pub target: ParameterTarget,
    /// Strictly time-ordered point data beginning at Song time zero.
    pub points: Vec<AutomationPoint>,
    /// Whole lane declaration location.
    pub location: SourceLocation,
}

/// One editable point and the interpolation leaving it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationPoint {
    /// Durable identity within the lane.
    pub id: String,
    /// Exact Song time.
    pub at: MusicalTime,
    /// Authored physical literal, resolved against the target unit after imports load.
    pub value: String,
    /// Curve from this point to the following point.
    pub interpolation: AutomationInterpolation,
    /// Whole point directive used by checked deletion.
    pub site: ValueSite,
    /// Exact time literal used by checked movement.
    pub at_site: ValueSite,
    /// Exact value literal used by checked value editing.
    pub value_site: ValueSite,
    /// Exact interpolation literal used by checked curve editing.
    pub interpolation_site: ValueSite,
    /// Source declaration location.
    pub location: SourceLocation,
}

/// Deterministic point interpolation to the following Automation point.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "exponent", rename_all = "snake_case")]
pub enum AutomationInterpolation {
    /// Hold the current point until the following point.
    Step,
    /// Interpolate with normalized linear progress.
    Linear,
    /// Raise normalized progress to a finite positive exponent.
    Power(f64),
}

impl fmt::Display for AutomationInterpolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Step => formatter.write_str("step"),
            Self::Linear => formatter.write_str("linear"),
            Self::Power(exponent) => write!(formatter, "power({})", format_number(*exponent)),
        }
    }
}

impl FromStr for AutomationInterpolation {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "step" => Ok(Self::Step),
            "linear" => Ok(Self::Linear),
            _ => {
                let exponent = value
                    .strip_prefix("power(")
                    .and_then(|value| value.strip_suffix(')'))
                    .ok_or_else(|| ValueError::UnknownInterpolation(value.to_owned()))?
                    .parse::<f64>()
                    .map_err(|_| ValueError::UnknownInterpolation(value.to_owned()))?;
                if exponent.is_finite() && exponent > 0.0 {
                    Ok(Self::Power(exponent))
                } else {
                    Err(ValueError::InvalidPowerExponent)
                }
            }
        }
    }
}

/// One authored normalized LFO route into a physical Parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModulationRoute {
    /// Durable Song-wide route identity and deterministic fold key.
    pub id: String,
    /// LFO execution scope and owner, when applicable.
    pub scope: ModulationScope,
    /// Normalized source polarity.
    pub polarity: ControlPolarity,
    /// LFO waveform.
    pub waveform: LfoWaveform,
    /// Free-running or tempo-synchronized period.
    pub rate: ModulationRate,
    /// Stable destination Parameter.
    pub target: ParameterTarget,
    /// Route depth in the destination's canonical physical unit.
    pub depth: String,
    /// Authored route-local normalized arithmetic, in declaration order.
    pub operators: Vec<ModulationOperator>,
    /// Source declaration location.
    pub location: SourceLocation,
}

/// Runtime ownership and phase semantics of one Modulation source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "scope", content = "instrument", rename_all = "snake_case")]
pub enum ModulationScope {
    /// One Song-wide phase derived from absolute transport time.
    Global,
    /// One instrument-instance phase derived from absolute transport time.
    Instrument(String),
    /// One phase per voice, reset at note activation.
    PerVoice(String),
}

/// Normalized Control signal domain.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlPolarity {
    /// Values in `[0, 1]`.
    Unipolar,
    /// Values in `[-1, 1]`.
    Bipolar,
}

/// Supported deterministic LFO shapes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LfoWaveform {
    /// Sinusoidal cycle.
    Sine,
    /// Symmetric triangle cycle.
    Triangle,
}

/// LFO timing mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum ModulationRate {
    /// Cycles per second.
    FreeHertz(f64),
    /// Exact musical duration of one cycle.
    Synchronized(MusicalTime),
}

/// One route-local normalized arithmetic instruction.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(tag = "operator", rename_all = "snake_case")]
pub enum ModulationOperator {
    /// Add a normalized scalar.
    Add {
        /// Scalar operand.
        value: f64,
    },
    /// Multiply by a normalized scalar.
    Multiply {
        /// Scalar operand.
        value: f64,
    },
    /// Scale by a normalized scalar (an explicit semantic alias for multiply).
    Scale {
        /// Scalar operand.
        value: f64,
    },
    /// Bias by a normalized scalar (an explicit semantic alias for add).
    Bias {
        /// Scalar operand.
        value: f64,
    },
    /// Constrain the route-local normalized signal.
    Clamp {
        /// Inclusive lower bound.
        minimum: f64,
        /// Inclusive upper bound.
        maximum: f64,
    },
}

/// One shallow Instrument Faceplate control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceplateControl {
    /// Stable control key.
    pub key: String,
    /// Human-facing label.
    pub label: String,
    /// Short contextual explanation.
    pub explanation: String,
    /// Target Parameter key.
    pub parameter: String,
    /// Declaration location.
    pub location: SourceLocation,
}

/// One hierarchical graph definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Graph {
    /// Declared resource ceiling for runtime nodes per voice.
    pub max_nodes: u16,
    /// Definition-local processing nodes.
    pub nodes: Vec<GraphNode>,
    /// Typed directed connections.
    pub connections: Vec<Connection>,
    /// Declaration location.
    pub location: SourceLocation,
}

/// A graph Processing Node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Definition-local Durable Domain ID.
    pub id: String,
    /// Native operation.
    pub kind: NodeKind,
    /// Named deterministic settings.
    pub settings: BTreeMap<String, SettingValue>,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Native first-release graph operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Band-limited-enough-for-first-slice mono oscillator.
    Oscillator,
    /// Bounded deterministic stereo/unison oscillator.
    Unison,
    /// Deterministic white-noise source.
    Noise,
    /// ADSR envelope.
    Envelope,
    /// Two-input mono sum.
    Mixer,
    /// Resonant low-pass filter.
    Lowpass,
    /// Audio multiplied by a control signal.
    Amplifier,
    /// Soft nonlinear waveshaping.
    Saturation,
    /// Equal-power mono-to-stereo placement.
    Pan,
    /// Bounded stereo feedback delay.
    Delay,
    /// Bounded deterministic stereo reverberator.
    Reverb,
    /// Graph's single stereo output sink.
    Output,
    /// Unipolar low-frequency oscillator.
    Lfo,
}

impl FromStr for NodeKind {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "oscillator" | "osc" => Ok(Self::Oscillator),
            "unison" | "unison_oscillator" => Ok(Self::Unison),
            "noise" => Ok(Self::Noise),
            "envelope" | "adsr" => Ok(Self::Envelope),
            "mixer" | "mix" => Ok(Self::Mixer),
            "lowpass" | "filter" => Ok(Self::Lowpass),
            "amplifier" | "amp" => Ok(Self::Amplifier),
            "saturation" | "saturator" => Ok(Self::Saturation),
            "pan" | "panner" => Ok(Self::Pan),
            "delay" => Ok(Self::Delay),
            "reverb" => Ok(Self::Reverb),
            "output" => Ok(Self::Output),
            "lfo" => Ok(Self::Lfo),
            _ => Err(ValueError::UnknownNode(value.to_owned())),
        }
    }
}

/// A node setting that is either literal, Parameter-backed, or categorical.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SettingValue {
    /// Finite scalar literal.
    Number(f64),
    /// Reference to a physical-unit Parameter key.
    Parameter(String),
    /// Categorical setting such as an oscillator waveform.
    Text(String),
}

/// A typed graph endpoint spelled `node.port` in source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Endpoint {
    /// Node ID, or the reserved `note` input source.
    pub node: String,
    /// Owner-local port name.
    pub port: String,
}

impl fmt::Display for Endpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.node, self.port)
    }
}

impl FromStr for Endpoint {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (node, port) = value
            .split_once('.')
            .ok_or_else(|| ValueError::InvalidEndpoint(value.to_owned()))?;
        if node.is_empty() || port.is_empty() || port.contains('.') {
            return Err(ValueError::InvalidEndpoint(value.to_owned()));
        }
        Ok(Self {
            node: node.to_owned(),
            port: port.to_owned(),
        })
    }
}

/// One graph Bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    /// Output endpoint.
    pub from: Endpoint,
    /// Input endpoint.
    pub to: Endpoint,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Checked source ranges for one durable Pattern Parameter lock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterLockSites {
    /// Lock onset literal.
    pub at: ValueSite,
    /// Instrument Parameter key.
    pub parameter: ValueSite,
    /// Physical value literal.
    pub value: ValueSite,
}

/// A durable step override scoped to one Pattern traversal inside a Clip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterLock {
    /// Durable identity within the Pattern.
    pub id: String,
    /// Pattern-relative onset.
    pub at: MusicalTime,
    /// Instrument Parameter key resolved for each consuming Track.
    pub parameter: String,
    /// Authored physical value.
    pub value: String,
    /// Checked source ranges.
    pub sites: ParameterLockSites,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Checked source ranges for one procedural Pattern Variation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternVariationSites {
    /// Referenced source Pattern identity.
    pub source: ValueSite,
    /// Rotation literal.
    pub rotate: ValueSite,
    /// Transposition literal.
    pub transpose: ValueSite,
    /// Velocity-scale literal.
    pub velocity: ValueSite,
    /// Reverse boolean literal.
    pub reverse: ValueSite,
    /// Deterministic event inclusion probability.
    pub probability: ValueSite,
    /// Deterministic seed literal.
    pub seed: ValueSite,
}

/// Procedural transformations materialized off the realtime callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternVariation {
    /// Referenced Pattern lineage.
    pub source: String,
    /// Exact cyclic onset rotation.
    pub rotate: MusicalTime,
    /// Whole-semitone note transposition.
    pub transpose_semitones: i16,
    /// Normalized velocity multiplier.
    pub velocity_scale: f64,
    /// Whether onset order is mirrored within the Pattern span.
    pub reverse: bool,
    /// Independent deterministic event inclusion probability.
    pub probability: f64,
    /// Authored deterministic seed.
    pub seed: u64,
    /// Checked source ranges.
    pub sites: PatternVariationSites,
}

/// Reusable finite musical material.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    /// Stable declaration key.
    pub id: String,
    /// Positive finite span.
    pub span: MusicalTime,
    /// Authored span literal used by checked arrangement edits.
    pub span_site: ValueSite,
    /// Ordered source events; runtime ordering is normalized deterministically.
    pub events: Vec<PatternEvent>,
    /// Durable Pattern-relative Parameter locks.
    #[serde(default)]
    pub locks: Vec<ParameterLock>,
    /// Optional procedural lineage instead of flattened Events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variation: Option<PatternVariation>,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Checked source ranges for one durable sample occurrence.
///
/// These ranges are boxed as one off-realtime edit record so ordinary Note and Hit Events do not
/// inherit the much larger inline size of a sample occurrence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleEventSites {
    /// Whole-line range for checked create/delete/duplicate/chop operations.
    pub site: ValueSite,
    /// Onset literal range for checked movement.
    pub at: ValueSite,
    /// Duration literal range for checked resizing/chopping.
    pub duration: ValueSite,
    /// Region-start literal range for checked trim/slip operations.
    pub start: ValueSite,
    /// Region-end literal range for checked trim/slip operations.
    pub end: ValueSite,
    /// Gain literal range.
    pub gain: ValueSite,
    /// Pan literal range.
    pub pan: ValueSite,
    /// Fade-in literal range.
    pub fade_in: ValueSite,
    /// Fade-out literal range.
    pub fade_out: ValueSite,
    /// Velocity literal range.
    pub velocity: ValueSite,
}

/// A concrete first-release Event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatternEvent {
    /// A pitched note with positive duration.
    Note {
        /// Relative onset.
        at: MusicalTime,
        /// MIDI-compatible pitch number.
        pitch: u8,
        /// Positive note duration.
        duration: MusicalTime,
        /// Normalized note velocity.
        velocity: f64,
        /// Whole-line range for checked Event Grid edits.
        site: ValueSite,
        /// Onset literal range for checked Event Grid edits.
        at_site: ValueSite,
        /// Pitch literal range for checked Event Grid edits.
        pitch_site: ValueSite,
        /// Duration literal range for checked Event Grid edits.
        duration_site: ValueSite,
        /// Velocity literal range for checked Event Grid edits.
        velocity_site: ValueSite,
    },
    /// A synthesized percussion trigger.
    Hit {
        /// Relative onset.
        at: MusicalTime,
        /// Stock percussion voice.
        voice: DrumVoice,
        /// Normalized trigger velocity.
        velocity: f64,
        /// Whole-line range for checked Event Grid edits.
        site: ValueSite,
        /// Onset literal range for checked Event Grid edits.
        at_site: ValueSite,
        /// Voice literal range for checked Event Grid edits.
        voice_site: ValueSite,
        /// Velocity literal range for checked Event Grid edits.
        velocity_site: ValueSite,
    },
    /// A durably identified occurrence of one sampler mapping and source region.
    Sample {
        /// Durable identity within the Pattern.
        id: String,
        /// Relative onset.
        at: MusicalTime,
        /// Mapping identity on the Track's sampler Instrument.
        mapping: String,
        /// Positive gate/crop duration in musical time.
        duration: MusicalTime,
        /// Inclusive source start in original asset frames.
        start_frame: u64,
        /// Exclusive source end in original asset frames.
        end_frame: u64,
        /// Occurrence gain in decibels.
        gain_db: f64,
        /// Occurrence pan from -1 to 1.
        pan: f64,
        /// Click-prevention fade-in in original source frames.
        fade_in_frames: u32,
        /// Click-prevention fade-out in original source frames.
        fade_out_frames: u32,
        /// Normalized trigger velocity.
        velocity: f64,
        /// Checked source ranges, boxed because they are needed only for off-realtime edits.
        #[serde(flatten)]
        sites: Box<SampleEventSites>,
    },
}

impl PatternEvent {
    /// Returns the event onset.
    #[must_use]
    pub const fn at(&self) -> MusicalTime {
        match self {
            Self::Note { at, .. } | Self::Hit { at, .. } | Self::Sample { at, .. } => *at,
        }
    }

    /// Returns the event's whole-line source site.
    #[must_use]
    pub const fn site(&self) -> &ValueSite {
        match self {
            Self::Note { site, .. } | Self::Hit { site, .. } => site,
            Self::Sample { sites, .. } => &sites.site,
        }
    }
}

/// Stock synthesized-percussion voice.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DrumVoice {
    /// Sine-based low drum.
    Kick,
    /// Noise and body snare.
    Snare,
    /// Short high-passed noise hat.
    Hat,
    /// Layered noise clap.
    Clap,
}

impl FromStr for DrumVoice {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "kick" => Ok(Self::Kick),
            "snare" => Ok(Self::Snare),
            "hat" | "hihat" => Ok(Self::Hat),
            "clap" => Ok(Self::Clap),
            _ => Err(ValueError::UnknownDrumVoice(value.to_owned())),
        }
    }
}

/// A linear timeline Track.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    /// Durable Track identity.
    pub id: String,
    /// Human-facing name.
    pub name: String,
    /// Assigned Native Instrument ID.
    pub instrument: String,
    /// Fixed channel strip after the instrument post-sum chain.
    pub channel: ChannelStrip,
    /// Timeline Clips.
    pub clips: Vec<Clip>,
    /// Declaration location.
    pub location: SourceLocation,
}

/// Checked source ranges for one arranged Clip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipSites {
    /// Complete declaration range for checked deletion and duplication.
    pub site: ValueSite,
    /// Referenced Pattern literal.
    pub pattern: ValueSite,
    /// Timeline-start literal.
    pub start: ValueSite,
    /// Timeline-duration literal.
    pub duration: ValueSite,
    /// Traversal-mode literal.
    pub mode: ValueSite,
    /// Optional source-offset literal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_offset: Option<ValueSite>,
    /// Optional transpose literal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transpose: Option<ValueSite>,
    /// Optional Clip-gain literal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain: Option<ValueSite>,
    /// Optional fade-in literal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fade_in: Option<ValueSite>,
    /// Optional fade-out literal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fade_out: Option<ValueSite>,
    /// Optional reusable Groove Template identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groove: Option<ValueSite>,
}

/// An arranged occurrence of a Pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    /// Durable Clip identity.
    pub id: String,
    /// Referenced Pattern ID.
    pub pattern: String,
    /// Timeline start.
    pub start: MusicalTime,
    /// Positive timeline duration.
    pub duration: MusicalTime,
    /// Explicit traversal behavior.
    pub mode: ClipMode,
    /// Pattern-local phase at the Clip's left edge.
    pub source_offset: MusicalTime,
    /// Whole-semitone Pattern transposition applied before traversal.
    pub transpose_semitones: i16,
    /// Event-excitation gain applied before Instrument processing.
    pub gain_db: f64,
    /// Event-excitation fade at the Clip's left edge.
    pub fade_in: MusicalTime,
    /// Event-excitation fade at the Clip's right edge.
    pub fade_out: MusicalTime,
    /// Optional reusable Groove Template identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groove: Option<String>,
    /// Checked source ranges used by structured arrangement edits.
    pub sites: ClipSites,
    /// Declaration location.
    pub location: SourceLocation,
}

/// First-release Clip traversal mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipMode {
    /// Traverse the Pattern at most once.
    Once,
    /// Repeat the Pattern until the Clip ends.
    Loop,
}

impl FromStr for ClipMode {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "once" => Ok(Self::Once),
            "loop" => Ok(Self::Loop),
            _ => Err(ValueError::UnknownClipMode(value.to_owned())),
        }
    }
}

/// A source literal range used by a checked replacement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValueSite {
    /// Project-relative source file.
    pub file: PathBuf,
    /// Inclusive UTF-8 byte offset.
    pub start: usize,
    /// Exclusive UTF-8 byte offset.
    pub end: usize,
    /// Exact token text observed in the parsed revision.
    pub expected: String,
}

impl ValueSite {
    /// Creates a checked source site.
    #[must_use]
    pub fn new(
        file: impl Into<PathBuf>,
        start: usize,
        end: usize,
        expected: impl Into<String>,
    ) -> Self {
        Self {
            file: file.into(),
            start,
            end,
            expected: expected.into(),
        }
    }
}

/// Authored-value parsing errors.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum ValueError {
    /// A scalar was malformed.
    #[error("expected a finite number")]
    InvalidNumber,
    /// A scalar was NaN or infinite.
    #[error("value must be finite")]
    NonFinite,
    /// A discrete boolean literal was not canonical source text.
    #[error("invalid boolean `{0}`; expected true or false")]
    InvalidBoolean(String),
    /// The unit name is not part of the first-release grammar.
    #[error("unknown unit `{0}`")]
    UnknownUnit(String),
    /// A physical literal used the wrong suffix.
    #[error("expected {expected}, not suffix `{suffix}`")]
    WrongUnit {
        /// Required unit.
        expected: Unit,
        /// Observed suffix.
        suffix: String,
    },
    /// The node operation is unsupported.
    #[error("unknown graph node kind `{0}`")]
    UnknownNode(String),
    /// The channel processor operation is unsupported.
    #[error("unknown processor kind `{0}`")]
    UnknownProcessor(String),
    /// A stable channel reference was malformed.
    #[error("invalid channel reference `{0}`; expected track/ID, group/ID, or return/ID")]
    InvalidChannelRef(String),
    /// A main-output target was malformed.
    #[error("invalid output target `{0}`; expected master, group/ID, or return/ID")]
    InvalidOutputTarget(String),
    /// A pre/post tap token was unsupported.
    #[error("unknown tap point `{0}`; expected pre_fader or post_fader")]
    UnknownTapPoint(String),
    /// Endpoint syntax was not `node.port`.
    #[error("invalid graph endpoint `{0}`; expected node.port")]
    InvalidEndpoint(String),
    /// Stock percussion voice is unknown.
    #[error("unknown stock percussion voice `{0}`")]
    UnknownDrumVoice(String),
    /// Clip mode is unknown.
    #[error("unknown clip mode `{0}`; expected once or loop")]
    UnknownClipMode(String),
    /// Sampler gate behavior is unknown.
    #[error("unknown sample playback `{0}`; expected one_shot or gated")]
    UnknownSamplePlayback(String),
    /// A stable Parameter target path was malformed.
    #[error("invalid Parameter target `{0}`; expected OWNER/ID/PARAMETER or master/level")]
    InvalidParameterTarget(String),
    /// A Track Parameter key is unsupported.
    #[error(
        "unknown channel Parameter `{0}`; expected level, pan, balance, left_pan, right_pan, or mute"
    )]
    UnknownTrackParameter(String),
    /// An Automation interpolation token is unsupported.
    #[error("unknown Automation interpolation `{0}`; expected step, linear, or power(P)")]
    UnknownInterpolation(String),
    /// A power curve exponent was non-finite or non-positive.
    #[error("Automation power exponent must be finite and greater than zero")]
    InvalidPowerExponent,
}

fn split_number_suffix(value: &str) -> Result<(&str, &str), ValueError> {
    let split = value
        .char_indices()
        .find(|(index, character)| {
            *index > 0
                && !character.is_ascii_digit()
                && *character != '.'
                && *character != '-'
                && *character != '+'
                && *character != 'e'
                && *character != 'E'
        })
        .map_or(value.len(), |(index, _)| index);
    let (number, suffix) = value.split_at(split);
    if number.is_empty() {
        return Err(ValueError::InvalidNumber);
    }
    Ok((number, suffix))
}

fn format_number(value: f64) -> String {
    if value == 0.0 {
        "0".to_owned()
    } else {
        // Rust's shortest Display representation round-trips to the same finite f64.
        value.to_string()
    }
}

/// Converts a note spelling such as `C4` or `F#3` to MIDI pitch.
pub fn parse_pitch(value: &str) -> Result<u8, PitchError> {
    let bytes = value.as_bytes();
    if bytes.len() < 2 {
        return Err(PitchError::Invalid(value.to_owned()));
    }
    let base = match bytes[0].to_ascii_uppercase() {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return Err(PitchError::Invalid(value.to_owned())),
    };
    let (accidental, octave_start) = match bytes.get(1) {
        Some(b'#') => (1, 2),
        Some(b'b') => (-1, 2),
        _ => (0, 1),
    };
    let octave: i16 = value[octave_start..]
        .parse()
        .map_err(|_| PitchError::Invalid(value.to_owned()))?;
    let pitch = (octave + 1) * 12 + base + accidental;
    u8::try_from(pitch)
        .ok()
        .filter(|pitch| *pitch <= 127)
        .ok_or_else(|| PitchError::OutOfRange(value.to_owned()))
}

/// Formats MIDI pitch with sharps and scientific pitch octave numbering.
#[must_use]
pub fn format_pitch(pitch: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let class = usize::from(pitch % 12);
    let octave = i16::from(pitch / 12) - 1;
    format!("{}{octave}", NAMES[class])
}

/// Pitch-spelling errors.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PitchError {
    /// Spelling was not a note name plus octave.
    #[error("invalid pitch `{0}`; expected a spelling such as C4 or F#3")]
    Invalid(String),
    /// Spelling fell outside MIDI-compatible 0..127.
    #[error("pitch `{0}` is outside the supported C-1..G9 range")]
    OutOfRange(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_pitch() {
        assert_eq!(parse_pitch("C4").unwrap(), 60);
        assert_eq!(parse_pitch("F#3").unwrap(), 54);
        assert_eq!(format_pitch(69), "A4");
    }

    #[test]
    fn parameter_units_are_physical() {
        assert_eq!(Unit::Hertz.parse_value("4.2khz").unwrap(), 4_200.0);
        assert_eq!(Unit::Seconds.parse_value("15ms").unwrap(), 0.015);
        assert!(Unit::Decibels.parse_value("2hz").is_err());
        assert_eq!(Unit::Boolean.parse_value("false").unwrap(), 0.0);
        assert_eq!(Unit::Boolean.parse_value("true").unwrap(), 1.0);
        assert_eq!(Unit::Boolean.format_value(0.0), "false");
        assert_eq!(Unit::Boolean.format_value(1.0), "true");
    }

    #[test]
    fn delay_divisions_resolve_straight_dotted_and_triplet_time() {
        assert_eq!(delay_division_beats("1/1"), Some(4.0));
        assert_eq!(delay_division_beats("1/4"), Some(1.0));
        assert_eq!(delay_division_beats("1/8d"), Some(0.75));
        assert_eq!(delay_division_beats("1/4t"), Some(2.0 / 3.0));
        assert_eq!(delay_division_beats("1/32"), Some(0.125));
        assert_eq!(delay_division_beats("1/32d"), Some(0.1875));
        assert_eq!(delay_division_beats("1/32t"), Some(1.0 / 12.0));
        assert_eq!(delay_division_seconds("1/8d", 120.0), Some(0.375));
        assert_eq!(delay_division_seconds("1/1d", 20.0), Some(18.0));
        assert_eq!(delay_division_beats("1/7"), None);
        assert_eq!(delay_division_beats("2/4"), None);
        assert_eq!(delay_division_seconds("1/4", 0.0), None);
        assert_eq!(
            processor_setting_contract(ProcessorKind::Delay, "time"),
            Some((Unit::Seconds, 1.0 / 48_000.0, 4.0))
        );
    }

    #[test]
    fn production_parameter_targets_round_trip_and_resolve_contracts() {
        let source = include_str!("../tests/fixtures/production_mix.crump");
        let parsed =
            crate::dsl::parse_project(std::path::Path::new("production_mix.crump"), source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("production Project");
        let targets = [
            "instrument/drums/tone",
            "track/drums/level",
            "track/drums/mute",
            "track/bass/left_pan",
            "track/bass/right_pan",
            "group/mix_bus/pan",
            "group/mix_bus/level",
            "return/room/balance",
            "send/drum_room/level",
            "processor/bass_comp/threshold",
            "master/level",
        ];
        for path in targets {
            let target = path.parse::<ParameterTarget>().expect("Parameter target");
            assert_eq!(target.to_string(), path);
            let spec = project.parameter_spec(&target).expect("Parameter contract");
            assert!(
                spec.minimum <= spec.base && spec.base <= spec.maximum,
                "{path}"
            );
        }
        let inventory = project.parameter_specs();
        assert!(inventory.iter().any(|(target, _)| matches!(
            target,
            ParameterTarget::Instrument { instrument, parameter }
                if instrument == "drums" && parameter == "tone"
        )));
        let mix_inventory = project.mix_parameter_specs();
        assert!(mix_inventory
            .iter()
            .all(|(target, _)| !matches!(target, ParameterTarget::Instrument { .. })));
        assert_eq!(
            inventory
                .iter()
                .filter(|(target, _)| !matches!(target, ParameterTarget::Instrument { .. }))
                .count(),
            mix_inventory.len()
        );

        assert!("send/drum_room/gain".parse::<ParameterTarget>().is_err());
        assert!(project
            .parameter_spec(&"group/missing/level".parse().expect("target"))
            .is_none());
        let limiter_lookahead = "processor/master_limiter/lookahead"
            .parse()
            .expect("structural processor path");
        assert!(project.parameter_spec(&limiter_lookahead).is_none());
        assert!(matches!(
            crate::processor_semantics::descriptor(ProcessorKind::Limiter, "lookahead")
                .map(|descriptor| descriptor.contract),
            Some(
                crate::processor_semantics::SettingContract::StructuralNumeric {
                    unit: Unit::Seconds,
                    minimum: 0.0,
                    maximum: 0.02,
                    ..
                }
            )
        ));

        for processor in all_processors(&project.song) {
            for setting in &processor.settings {
                let target = ParameterTarget::Processor {
                    processor: processor.id.clone(),
                    parameter: setting.key.clone(),
                };
                assert_eq!(
                    project.parameter_spec(&target).is_some(),
                    processor_setting_contract(processor.kind, &setting.key).is_some(),
                    "processor/{}/{}",
                    processor.id,
                    setting.key
                );
            }
        }
    }

    #[test]
    fn physical_values_and_power_curves_format_without_precision_loss() {
        for unit in [
            Unit::Ratio,
            Unit::Hertz,
            Unit::Seconds,
            Unit::Decibels,
            Unit::Semitones,
            Unit::Pan,
        ] {
            for value in [0.123_456_789, 0.000_000_4, -0.987_654_321] {
                let formatted = unit.format_value(value);
                assert_eq!(unit.parse_value(&formatted).unwrap(), value, "{formatted}");
            }
        }

        let interpolation = AutomationInterpolation::Power(0.000_000_4);
        let formatted = interpolation.to_string();
        assert_ne!(formatted, "power(0)");
        assert_eq!(formatted.parse(), Ok(interpolation));
    }
}
