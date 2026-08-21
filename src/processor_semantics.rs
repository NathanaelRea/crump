//! Native processor setting contracts, validation, defaults, and effective resolution.

use std::fmt;

use crate::model::{Processor, ProcessorKind, Unit, ValueError};

/// The complete value contract for one native processor setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SettingContract {
    /// A source-backed continuous Parameter.
    Continuous {
        unit: Unit,
        minimum: f64,
        maximum: f64,
        default: f64,
    },
    /// A numeric setting that changes compiled DSP topology.
    StructuralNumeric {
        unit: Unit,
        minimum: f64,
        maximum: f64,
        default: f64,
    },
    /// One value from a fixed set of source spellings.
    Choice {
        choices: &'static [&'static str],
        default: &'static str,
    },
    /// A structural frame count represented as an unsigned integer.
    UnsignedFrames { default: u32 },
    /// A tempo-relative Delay division.
    DelayDivision { default: &'static str },
}

impl SettingContract {
    pub(crate) const fn continuous(self) -> Option<(Unit, f64, f64, f64)> {
        match self {
            Self::Continuous {
                unit,
                minimum,
                maximum,
                default,
            } => Some((unit, minimum, maximum, default)),
            _ => None,
        }
    }

    pub(crate) const fn numeric(self) -> Option<(Unit, f64, f64, f64)> {
        match self {
            Self::Continuous {
                unit,
                minimum,
                maximum,
                default,
            }
            | Self::StructuralNumeric {
                unit,
                minimum,
                maximum,
                default,
            } => Some((unit, minimum, maximum, default)),
            _ => None,
        }
    }
}

/// One setting key and its complete authored value contract.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SettingDescriptor {
    pub(crate) key: &'static str,
    pub(crate) contract: SettingContract,
}

const fn continuous(
    key: &'static str,
    unit: Unit,
    minimum: f64,
    maximum: f64,
    default: f64,
) -> SettingDescriptor {
    SettingDescriptor {
        key,
        contract: SettingContract::Continuous {
            unit,
            minimum,
            maximum,
            default,
        },
    }
}

const fn structural_numeric(
    key: &'static str,
    unit: Unit,
    minimum: f64,
    maximum: f64,
    default: f64,
) -> SettingDescriptor {
    SettingDescriptor {
        key,
        contract: SettingContract::StructuralNumeric {
            unit,
            minimum,
            maximum,
            default,
        },
    }
}

const fn choice(
    key: &'static str,
    choices: &'static [&'static str],
    default: &'static str,
) -> SettingDescriptor {
    SettingDescriptor {
        key,
        contract: SettingContract::Choice { choices, default },
    }
}

const BOOLEAN_CHOICES: &[&str] = &["false", "true"];
const UTILITY_POLARITY_CHOICES: &[&str] = &["normal", "invert"];
const COMPRESSOR_DETECTOR_CHOICES: &[&str] = &["peak", "rms"];
const DELAY_MODE_CHOICES: &[&str] = &["stereo", "ping_pong"];
const SATURATION_MODE_CHOICES: &[&str] = &["soft_clip", "tanh", "tape"];

const UTILITY_SETTINGS: &[SettingDescriptor] = &[
    continuous("gain", Unit::Decibels, -96.0, 24.0, 0.0),
    continuous("width", Unit::Ratio, 0.0, 1.0, 1.0),
    choice("polarity", UTILITY_POLARITY_CHOICES, "normal"),
    choice("mono", BOOLEAN_CHOICES, "false"),
];

const EQ_SETTINGS: &[SettingDescriptor] = &[
    continuous("high_pass", Unit::Hertz, 10.0, 23_520.0, 20.0),
    continuous("low_frequency", Unit::Hertz, 10.0, 23_520.0, 120.0),
    continuous("low_gain", Unit::Decibels, -24.0, 24.0, 0.0),
    continuous("mid1_frequency", Unit::Hertz, 10.0, 23_520.0, 500.0),
    continuous("mid1_gain", Unit::Decibels, -24.0, 24.0, 0.0),
    continuous("mid1_q", Unit::Ratio, 0.1, 20.0, 1.0),
    continuous("mid2_frequency", Unit::Hertz, 10.0, 23_520.0, 2_500.0),
    continuous("mid2_gain", Unit::Decibels, -24.0, 24.0, 0.0),
    continuous("mid2_q", Unit::Ratio, 0.1, 20.0, 1.0),
    continuous("high_frequency", Unit::Hertz, 10.0, 23_520.0, 8_000.0),
    continuous("high_gain", Unit::Decibels, -24.0, 24.0, 0.0),
    continuous("low_pass", Unit::Hertz, 10.0, 23_520.0, 20_000.0),
];

const COMPRESSOR_SETTINGS: &[SettingDescriptor] = &[
    continuous("threshold", Unit::Decibels, -96.0, 0.0, -18.0),
    continuous("ratio", Unit::Ratio, 1.0, 100.0, 4.0),
    continuous("attack", Unit::Seconds, 0.000_01, 10.0, 0.01),
    continuous("release", Unit::Seconds, 0.000_01, 10.0, 0.1),
    continuous("knee", Unit::Decibels, 0.0, 36.0, 6.0),
    continuous("makeup", Unit::Decibels, 0.0, 36.0, 0.0),
    continuous("mix", Unit::Ratio, 0.0, 1.0, 1.0),
    choice("detector", COMPRESSOR_DETECTOR_CHOICES, "peak"),
];

const DELAY_SETTINGS: &[SettingDescriptor] = &[
    continuous("time", Unit::Seconds, 1.0 / 48_000.0, 4.0, 0.25),
    choice("tempo_sync", BOOLEAN_CHOICES, "false"),
    SettingDescriptor {
        key: "division",
        contract: SettingContract::DelayDivision { default: "1/4" },
    },
    continuous("feedback", Unit::Ratio, 0.0, 0.999, 0.25),
    continuous("mix", Unit::Ratio, 0.0, 0.999, 0.25),
    continuous("low_pass", Unit::Hertz, 10.0, 23_520.0, 20_000.0),
    continuous("high_pass", Unit::Hertz, 10.0, 23_520.0, 20.0),
    continuous("time_crossfade", Unit::Seconds, 0.001, 0.1, 0.01),
    choice("mode", DELAY_MODE_CHOICES, "stereo"),
];

const REVERB_SETTINGS: &[SettingDescriptor] = &[
    continuous("pre_delay", Unit::Seconds, 0.0, 1.0, 0.0),
    continuous("diffusion", Unit::Ratio, 0.0, 1.0, 0.7),
    continuous("decay", Unit::Ratio, 0.0, 1.0, 0.65),
    continuous("damping", Unit::Ratio, 0.0, 1.0, 0.35),
    continuous("modulation", Unit::Ratio, 0.0, 1.0, 0.1),
    continuous("width", Unit::Ratio, 0.0, 1.0, 1.0),
    continuous("mix", Unit::Ratio, 0.0, 1.0, 0.25),
];

const SATURATION_SETTINGS: &[SettingDescriptor] = &[
    continuous("drive", Unit::Ratio, 0.0, 10.0, 1.0),
    continuous("mix", Unit::Ratio, 0.0, 1.0, 1.0),
    continuous("output", Unit::Decibels, -24.0, 24.0, 0.0),
    choice("mode", SATURATION_MODE_CHOICES, "soft_clip"),
];

const LIMITER_SETTINGS: &[SettingDescriptor] = &[
    continuous("ceiling", Unit::Decibels, -24.0, 0.0, -1.0),
    continuous("release", Unit::Seconds, 0.0, 2.0, 0.08),
    structural_numeric("lookahead", Unit::Seconds, 0.0, 0.02, 0.005),
];

const LATENCY_SETTINGS: &[SettingDescriptor] = &[SettingDescriptor {
    key: "frames",
    contract: SettingContract::UnsignedFrames { default: 0 },
}];

/// Returns the canonical setting order and contracts for one processor kind.
pub(crate) const fn descriptors(kind: ProcessorKind) -> &'static [SettingDescriptor] {
    match kind {
        ProcessorKind::Utility => UTILITY_SETTINGS,
        ProcessorKind::Eq => EQ_SETTINGS,
        ProcessorKind::Compressor => COMPRESSOR_SETTINGS,
        ProcessorKind::Delay => DELAY_SETTINGS,
        ProcessorKind::Reverb => REVERB_SETTINGS,
        ProcessorKind::Saturation => SATURATION_SETTINGS,
        ProcessorKind::Limiter => LIMITER_SETTINGS,
        ProcessorKind::Latency => LATENCY_SETTINGS,
    }
}

/// Looks up one setting contract by kind and immutable key.
pub(crate) fn descriptor(kind: ProcessorKind, key: &str) -> Option<&'static SettingDescriptor> {
    descriptors(kind)
        .iter()
        .find(|descriptor| descriptor.key == key)
}

/// Parsed or effective value of one processor setting.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ResolvedSettingValue {
    Numeric(f64),
    Choice(String),
    Unsigned(u32),
}

/// Why an authored setting literal cannot participate in processor semantics.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProcessorSettingError {
    Unsupported {
        key: String,
        literal: String,
    },
    InvalidPhysical {
        key: String,
        source: ValueError,
    },
    OutOfRange {
        key: String,
        unit: Unit,
        minimum: f64,
        maximum: f64,
    },
    Conflict(ProcessorSettingConflict),
    InvalidTempoDivision {
        literal: String,
    },
}

impl ProcessorSettingError {
    /// Formats the defensive runtime-compilation failure with processor identity.
    pub(crate) fn compile_message(&self, processor: &Processor) -> String {
        match self {
            Self::Unsupported { key, literal } => format!(
                "processor `{}` kind {:?} does not implement `{key} {literal}`",
                processor.id, processor.kind
            ),
            Self::InvalidPhysical { key, source } => {
                format!("processor `{}` `{key}`: {source}", processor.id)
            }
            Self::OutOfRange {
                key,
                unit,
                minimum,
                maximum,
            } => format!(
                "processor `{}` `{key}` must be within {minimum}..={maximum} {unit:?}",
                processor.id
            ),
            Self::Conflict(ProcessorSettingConflict::DelayTimeWithTempoSync) => format!(
                "delay `{}` cannot author `time` while tempo sync is enabled",
                processor.id
            ),
            Self::Conflict(ProcessorSettingConflict::DelayDivisionWithoutTempoSync) => format!(
                "delay `{}` cannot author `division` unless tempo sync is enabled",
                processor.id
            ),
            Self::InvalidTempoDivision { literal } => format!(
                "delay `{}` has invalid tempo division `{literal}`",
                processor.id
            ),
        }
    }
}

impl fmt::Display for ProcessorSettingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { key, literal } => {
                write!(formatter, "unsupported setting `{key} {literal}`")
            }
            Self::InvalidPhysical { key, source } => write!(formatter, "`{key}`: {source}"),
            Self::OutOfRange {
                key,
                unit,
                minimum,
                maximum,
            } => write!(
                formatter,
                "`{key}` must be within {minimum}..={maximum} {unit:?}"
            ),
            Self::Conflict(conflict) => conflict.fmt(formatter),
            Self::InvalidTempoDivision { literal } => {
                write!(formatter, "invalid tempo division `{literal}`")
            }
        }
    }
}

impl std::error::Error for ProcessorSettingError {}

/// A relationship violation between otherwise valid authored settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessorSettingConflict {
    DelayTimeWithTempoSync,
    DelayDivisionWithoutTempoSync,
}

impl ProcessorSettingConflict {
    /// The setting whose source value receives the diagnostic.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::DelayTimeWithTempoSync => "time",
            Self::DelayDivisionWithoutTempoSync => "division",
        }
    }
}

impl fmt::Display for ProcessorSettingConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DelayTimeWithTempoSync => "cannot author `time` while tempo sync is enabled",
            Self::DelayDivisionWithoutTempoSync => {
                "cannot author `division` unless tempo sync is enabled"
            }
        })
    }
}

/// Validates and parses one authored setting through its canonical contract.
pub(crate) fn validate_authored_setting(
    kind: ProcessorKind,
    key: &str,
    literal: &str,
) -> Result<ResolvedSettingValue, ProcessorSettingError> {
    let Some(descriptor) = descriptor(kind, key) else {
        return Err(ProcessorSettingError::Unsupported {
            key: key.to_owned(),
            literal: literal.to_owned(),
        });
    };
    match descriptor.contract {
        SettingContract::Continuous {
            unit,
            minimum,
            maximum,
            ..
        }
        | SettingContract::StructuralNumeric {
            unit,
            minimum,
            maximum,
            ..
        } => {
            let value = unit.parse_value(literal).map_err(|source| {
                ProcessorSettingError::InvalidPhysical {
                    key: key.to_owned(),
                    source,
                }
            })?;
            if !(minimum..=maximum).contains(&value) {
                return Err(ProcessorSettingError::OutOfRange {
                    key: key.to_owned(),
                    unit,
                    minimum,
                    maximum,
                });
            }
            Ok(ResolvedSettingValue::Numeric(value))
        }
        SettingContract::Choice { choices, .. } if choices.contains(&literal) => {
            Ok(ResolvedSettingValue::Choice(literal.to_owned()))
        }
        SettingContract::DelayDivision { .. } if delay_division_beats(literal).is_some() => {
            Ok(ResolvedSettingValue::Choice(literal.to_owned()))
        }
        SettingContract::UnsignedFrames { .. } => literal
            .parse::<u32>()
            .map(ResolvedSettingValue::Unsigned)
            .map_err(|_| ProcessorSettingError::Unsupported {
                key: key.to_owned(),
                literal: literal.to_owned(),
            }),
        SettingContract::Choice { .. } | SettingContract::DelayDivision { .. } => {
            Err(ProcessorSettingError::Unsupported {
                key: key.to_owned(),
                literal: literal.to_owned(),
            })
        }
    }
}

/// Finds source text for the first authored instance of a setting key.
pub(crate) fn setting_literal<'a>(processor: &'a Processor, key: &str) -> Option<&'a str> {
    processor
        .settings
        .iter()
        .find(|setting| setting.key == key)
        .map(|setting| setting.value.as_str())
}

/// Reports whether a setting key was authored on this processor instance.
pub(crate) fn has_setting(processor: &Processor, key: &str) -> bool {
    setting_literal(processor, key).is_some()
}

/// Returns every relationship violation in deterministic diagnostic order.
pub(crate) fn validate_relationships(processor: &Processor) -> Vec<ProcessorSettingConflict> {
    if processor.kind != ProcessorKind::Delay {
        return Vec::new();
    }
    let tempo_sync = setting_literal(processor, "tempo_sync") == Some("true");
    let mut conflicts = Vec::new();
    if tempo_sync && has_setting(processor, "time") {
        conflicts.push(ProcessorSettingConflict::DelayTimeWithTempoSync);
    }
    if !tempo_sync && has_setting(processor, "division") {
        conflicts.push(ProcessorSettingConflict::DelayDivisionWithoutTempoSync);
    }
    conflicts
}

/// One effective setting after defaults and conditional meaning are resolved.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedProcessorSetting {
    pub(crate) descriptor: &'static SettingDescriptor,
    pub(crate) value: ResolvedSettingValue,
    pub(crate) authored: bool,
    pub(crate) derived: bool,
}

impl ResolvedProcessorSetting {
    /// Whether this exact setting is an authored continuous Parameter target.
    pub(crate) fn parameter_target(&self) -> bool {
        self.authored
            && !self.derived
            && matches!(self.descriptor.contract, SettingContract::Continuous { .. })
    }
}

/// Canonically ordered effective settings for one processor instance.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedProcessorSettings(Vec<ResolvedProcessorSetting>);

impl ResolvedProcessorSettings {
    pub(crate) fn iter(&self) -> impl Iterator<Item = &ResolvedProcessorSetting> {
        self.0.iter()
    }

    pub(crate) fn get(&self, key: &str) -> Option<&ResolvedProcessorSetting> {
        self.0.iter().find(|setting| setting.descriptor.key == key)
    }

    pub(crate) fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub(crate) fn numeric(&self, key: &str) -> Option<f64> {
        match &self.get(key)?.value {
            ResolvedSettingValue::Numeric(value) => Some(*value),
            ResolvedSettingValue::Choice(_) | ResolvedSettingValue::Unsigned(_) => None,
        }
    }

    pub(crate) fn choice(&self, key: &str) -> Option<&str> {
        match &self.get(key)?.value {
            ResolvedSettingValue::Choice(value) => Some(value),
            ResolvedSettingValue::Numeric(_) | ResolvedSettingValue::Unsigned(_) => None,
        }
    }

    pub(crate) fn unsigned(&self, key: &str) -> Option<u32> {
        match &self.get(key)?.value {
            ResolvedSettingValue::Unsigned(value) => Some(*value),
            ResolvedSettingValue::Numeric(_) | ResolvedSettingValue::Choice(_) => None,
        }
    }
}

fn default_value(contract: SettingContract) -> ResolvedSettingValue {
    match contract {
        SettingContract::Continuous { default, .. }
        | SettingContract::StructuralNumeric { default, .. } => {
            ResolvedSettingValue::Numeric(default)
        }
        SettingContract::Choice { default, .. } | SettingContract::DelayDivision { default } => {
            ResolvedSettingValue::Choice(default.to_owned())
        }
        SettingContract::UnsignedFrames { default } => ResolvedSettingValue::Unsigned(default),
    }
}

fn resolve_descriptor(
    processor: &Processor,
    key: &str,
) -> Result<ResolvedProcessorSetting, ProcessorSettingError> {
    let descriptor = descriptor(processor.kind, key).expect("known processor setting descriptor");
    let literal = setting_literal(processor, key);
    let value = literal.map_or_else(
        || Ok(default_value(descriptor.contract)),
        |literal| validate_authored_setting(processor.kind, key, literal),
    )?;
    Ok(ResolvedProcessorSetting {
        descriptor,
        value,
        authored: literal.is_some(),
        derived: false,
    })
}

fn push_descriptor(
    resolved: &mut Vec<ResolvedProcessorSetting>,
    processor: &Processor,
    key: &str,
) -> Result<(), ProcessorSettingError> {
    resolved.push(resolve_descriptor(processor, key)?);
    Ok(())
}

/// Resolves defaults, conditional EQ settings, and tempo-derived Delay time once.
pub(crate) fn resolve_settings(
    processor: &Processor,
    tempo_bpm: f64,
) -> Result<ResolvedProcessorSettings, ProcessorSettingError> {
    for authored in &processor.settings {
        validate_authored_setting(processor.kind, &authored.key, &authored.value)?;
    }
    if let Some(conflict) = validate_relationships(processor).into_iter().next() {
        return Err(ProcessorSettingError::Conflict(conflict));
    }

    let mut resolved = Vec::new();
    match processor.kind {
        ProcessorKind::Utility => {
            for key in ["gain", "width", "polarity", "mono"] {
                push_descriptor(&mut resolved, processor, key)?;
            }
        }
        ProcessorKind::Eq => {
            if has_setting(processor, "high_pass") {
                push_descriptor(&mut resolved, processor, "high_pass")?;
            }
            if has_setting(processor, "low_gain") || has_setting(processor, "low_frequency") {
                push_descriptor(&mut resolved, processor, "low_frequency")?;
                push_descriptor(&mut resolved, processor, "low_gain")?;
            }
            if ["mid1_gain", "mid1_frequency", "mid1_q"]
                .into_iter()
                .any(|key| has_setting(processor, key))
            {
                for key in ["mid1_frequency", "mid1_gain", "mid1_q"] {
                    push_descriptor(&mut resolved, processor, key)?;
                }
            }
            if ["mid2_gain", "mid2_frequency", "mid2_q"]
                .into_iter()
                .any(|key| has_setting(processor, key))
            {
                for key in ["mid2_frequency", "mid2_gain", "mid2_q"] {
                    push_descriptor(&mut resolved, processor, key)?;
                }
            }
            if has_setting(processor, "high_gain") || has_setting(processor, "high_frequency") {
                push_descriptor(&mut resolved, processor, "high_frequency")?;
                push_descriptor(&mut resolved, processor, "high_gain")?;
            }
            if has_setting(processor, "low_pass") {
                push_descriptor(&mut resolved, processor, "low_pass")?;
            }
        }
        ProcessorKind::Compressor => {
            for key in [
                "threshold",
                "ratio",
                "attack",
                "release",
                "knee",
                "makeup",
                "mix",
                "detector",
            ] {
                push_descriptor(&mut resolved, processor, key)?;
            }
        }
        ProcessorKind::Delay => {
            let tempo_sync = setting_literal(processor, "tempo_sync") == Some("true");
            if tempo_sync {
                let division = setting_literal(processor, "division").unwrap_or("1/4");
                let seconds = delay_division_seconds(division, tempo_bpm).ok_or_else(|| {
                    ProcessorSettingError::InvalidTempoDivision {
                        literal: division.to_owned(),
                    }
                })?;
                resolved.push(ResolvedProcessorSetting {
                    descriptor: descriptor(ProcessorKind::Delay, "time")
                        .expect("Delay time descriptor"),
                    value: ResolvedSettingValue::Numeric(seconds),
                    authored: false,
                    derived: true,
                });
            } else {
                push_descriptor(&mut resolved, processor, "time")?;
            }
            push_descriptor(&mut resolved, processor, "tempo_sync")?;
            if tempo_sync {
                push_descriptor(&mut resolved, processor, "division")?;
            }
            for key in [
                "feedback",
                "mix",
                "low_pass",
                "high_pass",
                "time_crossfade",
                "mode",
            ] {
                push_descriptor(&mut resolved, processor, key)?;
            }
        }
        ProcessorKind::Reverb => {
            for key in [
                "pre_delay",
                "diffusion",
                "decay",
                "damping",
                "modulation",
                "width",
                "mix",
            ] {
                push_descriptor(&mut resolved, processor, key)?;
            }
        }
        ProcessorKind::Saturation => {
            for key in ["drive", "mix", "output", "mode"] {
                push_descriptor(&mut resolved, processor, key)?;
            }
        }
        ProcessorKind::Limiter => {
            for key in ["ceiling", "release", "lookahead"] {
                push_descriptor(&mut resolved, processor, key)?;
            }
        }
        ProcessorKind::Latency => push_descriptor(&mut resolved, processor, "frames")?,
    }
    Ok(ResolvedProcessorSettings(resolved))
}

/// Resolves one tempo-synchronized Delay division to quarter-note beats.
pub(crate) fn delay_division_beats(literal: &str) -> Option<f64> {
    let (fraction, multiplier) = match literal.strip_suffix('d') {
        Some(fraction) => (fraction, 1.5),
        None => match literal.strip_suffix('t') {
            Some(fraction) => (fraction, 2.0 / 3.0),
            None => (literal, 1.0),
        },
    };
    let denominator = fraction.strip_prefix("1/")?.parse::<u32>().ok()?;
    if !matches!(denominator, 1 | 2 | 4 | 8 | 16 | 32) {
        return None;
    }
    Some(4.0 / f64::from(denominator) * multiplier)
}

/// Resolves one tempo-synchronized Delay division to seconds.
pub(crate) fn delay_division_seconds(literal: &str, tempo_bpm: f64) -> Option<f64> {
    if !tempo_bpm.is_finite() || tempo_bpm <= 0.0 {
        return None;
    }
    Some(delay_division_beats(literal)? * 60.0 / tempo_bpm)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::diagnostic::SourceLocation;
    use crate::model::{ProcessorSetting, ValueSite};

    use super::*;

    fn processor(kind: ProcessorKind, settings: &[(&str, &str)]) -> Processor {
        Processor {
            id: "processor".to_owned(),
            kind,
            settings: settings
                .iter()
                .map(|(key, value)| ProcessorSetting {
                    key: (*key).to_owned(),
                    value: (*value).to_owned(),
                    value_site: ValueSite::new(PathBuf::from("project.crump"), 0, 0, *value),
                    location: SourceLocation::file("project.crump"),
                })
                .collect(),
            sidechain_inputs: Vec::new(),
            latency_frames: 0,
            latency_site: None,
            location: SourceLocation::file("project.crump"),
        }
    }

    #[test]
    fn every_descriptor_accepts_its_default_and_numeric_boundaries() {
        for kind in [
            ProcessorKind::Utility,
            ProcessorKind::Eq,
            ProcessorKind::Compressor,
            ProcessorKind::Delay,
            ProcessorKind::Reverb,
            ProcessorKind::Saturation,
            ProcessorKind::Limiter,
            ProcessorKind::Latency,
        ] {
            for descriptor in descriptors(kind) {
                match descriptor.contract {
                    SettingContract::Continuous {
                        unit,
                        minimum,
                        maximum,
                        default,
                    }
                    | SettingContract::StructuralNumeric {
                        unit,
                        minimum,
                        maximum,
                        default,
                    } => {
                        for value in [minimum, default, maximum] {
                            assert!(
                                validate_authored_setting(
                                    kind,
                                    descriptor.key,
                                    &unit.format_value(value)
                                )
                                .is_ok(),
                                "{kind:?}/{} value {value}",
                                descriptor.key
                            );
                        }
                    }
                    SettingContract::Choice { choices, default } => {
                        assert!(choices.contains(&default));
                        for choice in choices {
                            assert!(validate_authored_setting(kind, descriptor.key, choice).is_ok());
                        }
                    }
                    SettingContract::DelayDivision { default } => {
                        assert!(validate_authored_setting(kind, descriptor.key, default).is_ok())
                    }
                    SettingContract::UnsignedFrames { default } => assert!(
                        validate_authored_setting(kind, descriptor.key, &default.to_string())
                            .is_ok()
                    ),
                }
            }
        }
    }

    #[test]
    fn eq_presence_and_delay_derivation_share_one_effective_order() {
        let empty_eq = resolve_settings(&processor(ProcessorKind::Eq, &[]), 120.0).unwrap();
        assert_eq!(empty_eq.iter().count(), 0);

        let mid_eq =
            resolve_settings(&processor(ProcessorKind::Eq, &[("mid1_q", "2")]), 120.0).unwrap();
        assert_eq!(
            mid_eq
                .iter()
                .map(|setting| setting.descriptor.key)
                .collect::<Vec<_>>(),
            ["mid1_frequency", "mid1_gain", "mid1_q"]
        );

        let delay = resolve_settings(
            &processor(
                ProcessorKind::Delay,
                &[("tempo_sync", "true"), ("division", "1/8d")],
            ),
            120.0,
        )
        .unwrap();
        assert_eq!(delay.numeric("time"), Some(0.375));
        let time = delay.get("time").unwrap();
        assert!(time.derived);
        assert!(!time.parameter_target());
    }

    #[test]
    fn delay_relationships_reject_incompatible_authored_settings() {
        assert_eq!(
            validate_relationships(&processor(
                ProcessorKind::Delay,
                &[("tempo_sync", "true"), ("time", "1s")],
            )),
            [ProcessorSettingConflict::DelayTimeWithTempoSync]
        );
        assert_eq!(
            validate_relationships(&processor(ProcessorKind::Delay, &[("division", "1/8")])),
            [ProcessorSettingConflict::DelayDivisionWithoutTempoSync]
        );
    }
}
