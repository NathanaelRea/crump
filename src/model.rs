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
    /// The Project's one Song.
    pub song: Song,
    /// Imported source paths, in declaration order after canonicalization.
    pub imports: Vec<PathBuf>,
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
    /// Native instruments addressable by Tracks.
    pub instruments: Vec<Instrument>,
    /// Reusable finite musical material.
    pub patterns: Vec<Pattern>,
    /// Presentation-ordered timeline Tracks.
    pub tracks: Vec<Track>,
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
}

impl fmt::Display for InstrumentKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Graph => "graph",
            Self::StockDrums => "stock_drums",
        })
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
}

impl Unit {
    /// Parses a physical literal and rejects mismatched suffixes.
    pub fn parse_value(self, value: &str) -> Result<f64, ValueError> {
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
        let number = format_number(value);
        match self {
            Self::Ratio | Self::Semitones | Self::Pan => number,
            Self::Hertz => format!("{number}hz"),
            Self::Seconds => format!("{number}s"),
            Self::Decibels => format!("{number}db"),
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
            _ => Err(ValueError::UnknownUnit(value.to_owned())),
        }
    }
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
    /// Band-limited-enough-for-first-slice oscillator.
    Oscillator,
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

/// Reusable finite musical material.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    /// Stable declaration key.
    pub id: String,
    /// Positive finite span.
    pub span: MusicalTime,
    /// Ordered source events; runtime ordering is normalized deterministically.
    pub events: Vec<PatternEvent>,
    /// Declaration location.
    pub location: SourceLocation,
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
}

impl PatternEvent {
    /// Returns the event onset.
    #[must_use]
    pub const fn at(&self) -> MusicalTime {
        match self {
            Self::Note { at, .. } | Self::Hit { at, .. } => *at,
        }
    }

    /// Returns the event's whole-line source site.
    #[must_use]
    pub const fn site(&self) -> &ValueSite {
        match self {
            Self::Note { site, .. } | Self::Hit { site, .. } => site,
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
    /// Source-backed level in decibels.
    pub level_db: f64,
    /// Source-backed equal-power stereo position.
    pub pan: f64,
    /// Timeline Clips.
    pub clips: Vec<Clip>,
    /// Level literal for checked exact-value edits.
    pub level_site: ValueSite,
    /// Pan literal for checked exact-value edits.
    pub pan_site: ValueSite,
    /// Declaration location.
    pub location: SourceLocation,
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
    /// Endpoint syntax was not `node.port`.
    #[error("invalid graph endpoint `{0}`; expected node.port")]
    InvalidEndpoint(String),
    /// Stock percussion voice is unknown.
    #[error("unknown stock percussion voice `{0}`")]
    UnknownDrumVoice(String),
    /// Clip mode is unknown.
    #[error("unknown clip mode `{0}`; expected once or loop")]
    UnknownClipMode(String),
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
        return "0".to_owned();
    }
    let mut formatted = format!("{value:.6}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
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
    }
}
