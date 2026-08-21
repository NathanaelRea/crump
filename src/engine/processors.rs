//! Native shared stereo processors used by instrument, channel, and Master chains.
//!
//! Programs are compiled away from the callback. Mutable runtimes own every
//! delay line and detector buffer up front; one-sample processing performs no
//! allocation, locking, graph construction, or source interpretation.

use std::f64::consts::PI;

use crate::{
    model::{ParameterTarget, Processor, ProcessorKind},
    processor_semantics::{resolve_settings, ResolvedProcessorSettings},
};

use super::{
    control::{CompiledControlProgram, ControlIndex},
    oversampling::{
        FilteredOversampler, OversamplingInterpolator, OVERSAMPLING_FACTOR,
        OVERSAMPLING_FILTER_TAPS, OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES,
        OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES,
    },
    EngineError,
};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Scalar {
    Constant(f64),
    Control(ControlIndex),
}

impl Scalar {
    fn value(self, controls: &[f64]) -> f64 {
        match self {
            Self::Constant(value) => value,
            Self::Control(index) => controls.get(index.0).copied().unwrap_or(0.0),
        }
    }
}

/// One immutable compiled native processor.
#[derive(Debug, Clone)]
pub(crate) struct ProcessorProgram {
    pub(crate) id: String,
    pub(crate) kind: ProcessorKind,
    pub(crate) sidechain_inputs: Vec<String>,
    authored_latency_frames: u32,
    declared_latency_frames: u32,
    config: ProcessorConfig,
    sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq)]
enum ProcessorConfig {
    Utility {
        gain: Scalar,
        width: Scalar,
        invert: bool,
        mono: bool,
    },
    Eq {
        bands: Vec<EqBandProgram>,
    },
    Compressor {
        threshold: Scalar,
        ratio: Scalar,
        attack: Scalar,
        release: Scalar,
        knee: Scalar,
        makeup: Scalar,
        mix: Scalar,
        rms: bool,
    },
    Delay {
        time: Scalar,
        feedback: Scalar,
        mix: Scalar,
        low_pass: Scalar,
        high_pass: Scalar,
        time_crossfade: Scalar,
        mode: DelayMode,
        maximum_frames: usize,
    },
    Reverb {
        pre_delay: Scalar,
        diffusion: Scalar,
        decay: Scalar,
        damping: Scalar,
        modulation: Scalar,
        width: Scalar,
        mix: Scalar,
    },
    Saturation {
        drive: Scalar,
        mix: Scalar,
        output: Scalar,
        mode: SaturationMode,
    },
    Limiter {
        ceiling: Scalar,
        release: Scalar,
        lookahead_frames: usize,
    },
    Latency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EqBandKind {
    HighPass,
    LowShelf,
    Peak,
    HighShelf,
    LowPass,
}

#[derive(Debug, Clone, PartialEq)]
struct EqBandProgram {
    kind: EqBandKind,
    frequency: Scalar,
    gain: Scalar,
    q: Scalar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaturationMode {
    SoftClip,
    Tanh,
    Tape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DelayMode {
    Stereo,
    PingPong,
}

const REVERB_COMB_LENGTHS_48K: [usize; 4] = [1557, 1617, 1491, 1422];
const REVERB_DIFFUSER_LENGTHS_48K: [usize; 4] = [225, 556, 441, 341];
const REVERB_MODULATION_DEPTH_FRAMES_48K: f64 = 8.0;
const REVERB_COMB_INTERPOLATION_PADDING: usize = 10;
const REVERB_INPUT_GAIN: f32 = 0.05;

fn scaled_stereo_reverb_lengths<const N: usize>(
    lengths_48k: [usize; N],
    sample_rate: u32,
) -> ([usize; N], [usize; N]) {
    let scale = f64::from(sample_rate) / 48_000.0;
    let left = lengths_48k.map(|frames| (frames as f64 * scale).round().max(1.0) as usize);
    let right = lengths_48k.map(|frames| ((frames + 23) as f64 * scale).round().max(1.0) as usize);
    (left, right)
}

fn reverb_comb_lengths(sample_rate: u32) -> ([usize; 4], [usize; 4]) {
    scaled_stereo_reverb_lengths(REVERB_COMB_LENGTHS_48K, sample_rate)
}

fn reverb_diffuser_lengths(sample_rate: u32) -> ([usize; 4], [usize; 4]) {
    scaled_stereo_reverb_lengths(REVERB_DIFFUSER_LENGTHS_48K, sample_rate)
}

fn reverb_heap_bytes(sample_rate: u32) -> u64 {
    let (left_combs, right_combs) = reverb_comb_lengths(sample_rate);
    let (left_diffusers, right_diffusers) = reverb_diffuser_lengths(sample_rate);
    let pre_delay_bytes = u64::from(sample_rate)
        .saturating_add(2)
        .saturating_mul(std::mem::size_of::<[f32; 2]>() as u64);
    let delay_struct_bytes = (left_combs.len() + right_combs.len()) as u64
        * std::mem::size_of::<MonoDelay>() as u64
        + (left_diffusers.len() + right_diffusers.len()) as u64
            * std::mem::size_of::<AllpassDelay>() as u64;
    let delay_samples = left_combs
        .into_iter()
        .chain(right_combs)
        .map(|frames| frames.saturating_add(REVERB_COMB_INTERPOLATION_PADDING) as u64)
        .chain(
            left_diffusers
                .into_iter()
                .chain(right_diffusers)
                .map(|frames| frames as u64),
        )
        .fold(0_u64, u64::saturating_add);
    pre_delay_bytes
        .saturating_add(delay_struct_bytes)
        .saturating_add(delay_samples.saturating_mul(std::mem::size_of::<f32>() as u64))
}

fn stereo_delay_heap_bytes(delay_frames: u32) -> u64 {
    u64::from(delay_frames.max(1)).saturating_mul(std::mem::size_of::<[f32; 2]>() as u64)
}

/// Fixed detector headroom used to cover reconstruction error during gain movement.
pub(crate) const LIMITER_TRUE_PEAK_SAFETY_DB: f64 = 0.1;
const LIMITER_TRUE_PEAK_SAFETY_RATIO: f32 = 0.988_553_1;

fn limiter_heap_bytes(lookahead_frames: usize) -> u64 {
    let audio_frames = lookahead_frames
        .saturating_add(OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES)
        .max(1) as u64;
    let peak_frames = lookahead_frames.saturating_add(1).max(1) as u64;
    audio_frames
        .saturating_mul(std::mem::size_of::<[f32; 2]>() as u64)
        .saturating_add(peak_frames.saturating_mul(std::mem::size_of::<f32>() as u64))
}

const fn saturation_work_units() -> u64 {
    (OVERSAMPLING_FILTER_TAPS as u64)
        .saturating_mul(4)
        .saturating_add(32)
}

fn limiter_work_units(lookahead_frames: usize) -> u64 {
    (OVERSAMPLING_FILTER_TAPS as u64)
        .saturating_mul(2)
        .saturating_add((lookahead_frames.saturating_add(1) as u64).saturating_mul(4))
        .saturating_add(24)
}

impl ProcessorProgram {
    pub(crate) fn compile(
        processor: &Processor,
        controls: &CompiledControlProgram,
        sample_rate: u32,
        tempo_bpm: f64,
    ) -> Result<Self, EngineError> {
        let settings = resolve_settings(processor, tempo_bpm)
            .map_err(|error| EngineError::Compile(error.compile_message(processor)))?;
        if processor.kind != ProcessorKind::Compressor && !processor.sidechain_inputs.is_empty() {
            return Err(EngineError::Compile(format!(
                "processor `{}` kind {:?} does not consume sidechain Audio",
                processor.id, processor.kind
            )));
        }
        if processor.kind == ProcessorKind::Compressor && processor.sidechain_inputs.len() > 1 {
            return Err(EngineError::Compile(format!(
                "compressor `{}` accepts at most one named sidechain input",
                processor.id
            )));
        }
        let scalar = |key: &str| processor_scalar(processor, controls, &settings, key);
        let config = match processor.kind {
            ProcessorKind::Utility => ProcessorConfig::Utility {
                gain: scalar("gain")?,
                width: scalar("width")?,
                invert: settings.choice("polarity") == Some("invert"),
                mono: settings.choice("mono") == Some("true"),
            },
            ProcessorKind::Eq => {
                let mut bands = Vec::with_capacity(6);
                if settings.contains("high_pass") {
                    bands.push(EqBandProgram {
                        kind: EqBandKind::HighPass,
                        frequency: scalar("high_pass")?,
                        gain: Scalar::Constant(0.0),
                        q: Scalar::Constant(std::f64::consts::FRAC_1_SQRT_2),
                    });
                }
                if settings.contains("low_frequency") {
                    bands.push(EqBandProgram {
                        kind: EqBandKind::LowShelf,
                        frequency: scalar("low_frequency")?,
                        gain: scalar("low_gain")?,
                        q: Scalar::Constant(std::f64::consts::FRAC_1_SQRT_2),
                    });
                }
                if settings.contains("mid1_frequency") {
                    bands.push(EqBandProgram {
                        kind: EqBandKind::Peak,
                        frequency: scalar("mid1_frequency")?,
                        gain: scalar("mid1_gain")?,
                        q: scalar("mid1_q")?,
                    });
                }
                if settings.contains("mid2_frequency") {
                    bands.push(EqBandProgram {
                        kind: EqBandKind::Peak,
                        frequency: scalar("mid2_frequency")?,
                        gain: scalar("mid2_gain")?,
                        q: scalar("mid2_q")?,
                    });
                }
                if settings.contains("high_frequency") {
                    bands.push(EqBandProgram {
                        kind: EqBandKind::HighShelf,
                        frequency: scalar("high_frequency")?,
                        gain: scalar("high_gain")?,
                        q: Scalar::Constant(std::f64::consts::FRAC_1_SQRT_2),
                    });
                }
                if settings.contains("low_pass") {
                    bands.push(EqBandProgram {
                        kind: EqBandKind::LowPass,
                        frequency: scalar("low_pass")?,
                        gain: Scalar::Constant(0.0),
                        q: Scalar::Constant(std::f64::consts::FRAC_1_SQRT_2),
                    });
                }
                ProcessorConfig::Eq { bands }
            }
            ProcessorKind::Compressor => ProcessorConfig::Compressor {
                threshold: scalar("threshold")?,
                ratio: scalar("ratio")?,
                attack: scalar("attack")?,
                release: scalar("release")?,
                knee: scalar("knee")?,
                makeup: scalar("makeup")?,
                mix: scalar("mix")?,
                rms: settings.choice("detector") == Some("rms"),
            },
            ProcessorKind::Delay => ProcessorConfig::Delay {
                time: scalar("time")?,
                feedback: scalar("feedback")?,
                mix: scalar("mix")?,
                low_pass: scalar("low_pass")?,
                high_pass: scalar("high_pass")?,
                time_crossfade: scalar("time_crossfade")?,
                mode: if settings.choice("mode") == Some("ping_pong") {
                    DelayMode::PingPong
                } else {
                    DelayMode::Stereo
                },
                maximum_frames: delay_maximum_frames(&settings, sample_rate) as usize,
            },
            ProcessorKind::Reverb => ProcessorConfig::Reverb {
                pre_delay: scalar("pre_delay")?,
                diffusion: scalar("diffusion")?,
                decay: scalar("decay")?,
                damping: scalar("damping")?,
                modulation: scalar("modulation")?,
                width: scalar("width")?,
                mix: scalar("mix")?,
            },
            ProcessorKind::Saturation => ProcessorConfig::Saturation {
                drive: scalar("drive")?,
                mix: scalar("mix")?,
                output: scalar("output")?,
                mode: match settings.choice("mode") {
                    Some("tape") => SaturationMode::Tape,
                    Some("tanh") => SaturationMode::Tanh,
                    Some("soft_clip") | None => SaturationMode::SoftClip,
                    Some(_) => SaturationMode::Tanh,
                },
            },
            ProcessorKind::Limiter => {
                let lookahead_seconds = settings.numeric("lookahead").ok_or_else(|| {
                    EngineError::Compile(format!(
                        "processor `{}` is missing resolved `lookahead`",
                        processor.id
                    ))
                })?;
                ProcessorConfig::Limiter {
                    ceiling: scalar("ceiling")?,
                    release: scalar("release")?,
                    lookahead_frames: (lookahead_seconds * f64::from(sample_rate)).round() as usize,
                }
            }
            ProcessorKind::Latency => ProcessorConfig::Latency,
        };
        let configured_latency = if processor.kind == ProcessorKind::Latency {
            settings.unsigned("frames").unwrap_or(0)
        } else {
            0
        };
        Ok(Self {
            id: processor.id.clone(),
            kind: processor.kind,
            sidechain_inputs: processor.sidechain_inputs.clone(),
            authored_latency_frames: processor.latency_frames,
            declared_latency_frames: processor.latency_frames.saturating_add(configured_latency),
            config,
            sample_rate,
        })
    }

    pub(crate) fn output_latency_frames(&self) -> u32 {
        let algorithmic = match self.config {
            ProcessorConfig::Saturation { .. } => {
                u32::try_from(OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES).unwrap_or(u32::MAX)
            }
            ProcessorConfig::Limiter {
                lookahead_frames, ..
            } => u32::try_from(
                lookahead_frames.saturating_add(OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES),
            )
            .unwrap_or(u32::MAX),
            _ => 0,
        };
        self.declared_latency_frames.saturating_add(algorithmic)
    }

    pub(crate) const fn authored_latency_frames(&self) -> u32 {
        self.authored_latency_frames
    }

    pub(crate) fn intrinsic_latency_frames(&self) -> u32 {
        self.output_latency_frames()
            .saturating_sub(self.authored_latency_frames)
    }

    pub(crate) const fn nonlinear_substeps(&self) -> u8 {
        match self.config {
            ProcessorConfig::Saturation { .. } => OVERSAMPLING_FACTOR,
            _ => 1,
        }
    }

    pub(crate) const fn oversampling_factor(&self) -> u8 {
        match self.config {
            ProcessorConfig::Saturation { .. } | ProcessorConfig::Limiter { .. } => {
                OVERSAMPLING_FACTOR
            }
            _ => 1,
        }
    }

    pub(crate) const fn oversampling_filter_taps(&self) -> u16 {
        match self.config {
            ProcessorConfig::Saturation { .. } | ProcessorConfig::Limiter { .. } => {
                OVERSAMPLING_FILTER_TAPS
            }
            _ => 0,
        }
    }

    pub(crate) const fn true_peak_safety_margin_db(&self) -> f64 {
        match self.config {
            ProcessorConfig::Limiter { .. } => LIMITER_TRUE_PEAK_SAFETY_DB,
            _ => 0.0,
        }
    }

    pub(crate) fn heap_bytes(&self) -> u64 {
        let sidechains = (self.sidechain_inputs.capacity() as u64)
            .saturating_mul(std::mem::size_of::<String>() as u64)
            .saturating_add(
                self.sidechain_inputs
                    .iter()
                    .map(|input| input.capacity() as u64)
                    .fold(0_u64, u64::saturating_add),
            );
        let configuration = match &self.config {
            ProcessorConfig::Eq { bands } => (bands.capacity() as u64)
                .saturating_mul(std::mem::size_of::<EqBandProgram>() as u64),
            _ => 0,
        };
        (self.id.capacity() as u64)
            .saturating_add(sidechains)
            .saturating_add(configuration)
    }

    pub(crate) fn state_bytes(&self) -> u64 {
        let algorithm_heap = match &self.config {
            ProcessorConfig::Delay { maximum_frames, .. } => {
                (*maximum_frames as u64).saturating_mul(std::mem::size_of::<[f32; 2]>() as u64)
            }
            ProcessorConfig::Reverb { .. } => reverb_heap_bytes(self.sample_rate),
            ProcessorConfig::Limiter {
                lookahead_frames, ..
            } => limiter_heap_bytes(*lookahead_frames),
            ProcessorConfig::Eq { bands } => {
                (bands.len() as u64).saturating_mul(std::mem::size_of::<StereoBiquad>() as u64)
            }
            ProcessorConfig::Saturation { .. } => stereo_delay_heap_bytes(
                u32::try_from(OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES).unwrap_or(u32::MAX),
            ),
            ProcessorConfig::Compressor { .. }
            | ProcessorConfig::Utility { .. }
            | ProcessorConfig::Latency => 0,
        };
        (std::mem::size_of::<ProcessorRuntime>() as u64)
            .saturating_add(algorithm_heap)
            .saturating_add(stereo_delay_heap_bytes(self.declared_latency_frames))
    }

    pub(crate) fn work_units(&self) -> u64 {
        match &self.config {
            ProcessorConfig::Utility { .. } => 8,
            ProcessorConfig::Eq { bands } => 24_u64.saturating_mul(bands.len() as u64),
            ProcessorConfig::Compressor { .. } => 28,
            ProcessorConfig::Delay { .. } => 32,
            ProcessorConfig::Reverb { .. } => 160,
            ProcessorConfig::Saturation { .. } => saturation_work_units(),
            ProcessorConfig::Limiter {
                lookahead_frames, ..
            } => limiter_work_units(*lookahead_frames),
            ProcessorConfig::Latency => 2,
        }
    }

    pub(crate) fn runtime(&self) -> ProcessorRuntime {
        ProcessorRuntime::new(self.clone())
    }

    pub(crate) fn topology_compatible(&self, other: &Self) -> bool {
        self.id == other.id
            && self.kind == other.kind
            && self.output_latency_frames() == other.output_latency_frames()
            && self.sidechain_inputs == other.sidechain_inputs
            && self.config == other.config
    }
}

fn processor_scalar(
    processor: &Processor,
    controls: &CompiledControlProgram,
    settings: &ResolvedProcessorSettings,
    key: &str,
) -> Result<Scalar, EngineError> {
    let setting = settings.get(key).ok_or_else(|| {
        EngineError::Compile(format!(
            "processor `{}` is missing resolved `{key}`",
            processor.id
        ))
    })?;
    let value = settings.numeric(key).ok_or_else(|| {
        EngineError::Compile(format!(
            "processor `{}` resolved `{key}` as a non-numeric setting",
            processor.id
        ))
    })?;
    if setting.parameter_target() {
        let target = ParameterTarget::Processor {
            processor: processor.id.clone(),
            parameter: key.to_owned(),
        };
        if let Some(index) = controls.index(&target) {
            return Ok(Scalar::Control(index));
        }
    }
    Ok(Scalar::Constant(value))
}

fn eq_band_count(settings: &ResolvedProcessorSettings) -> usize {
    [
        "high_pass",
        "low_frequency",
        "mid1_frequency",
        "mid2_frequency",
        "high_frequency",
        "low_pass",
    ]
    .into_iter()
    .filter(|key| settings.contains(key))
    .count()
}

pub(crate) fn authored_processor_estimate(
    processor: &Processor,
    sample_rate: u32,
    tempo_bpm: f64,
) -> (u64, u64) {
    let settings = resolve_settings(processor, tempo_bpm).ok();
    let configured_latency = settings
        .as_ref()
        .and_then(|settings| settings.unsigned("frames"))
        .unwrap_or(0);
    let declared_latency = processor.latency_frames.saturating_add(configured_latency);
    let fixed_and_declared = (std::mem::size_of::<ProcessorRuntime>() as u64)
        .saturating_add(stereo_delay_heap_bytes(declared_latency));
    let (algorithm_heap, work) = match processor.kind {
        ProcessorKind::Delay => (
            settings
                .as_ref()
                .map_or_else(
                    || u64::from(sample_rate).saturating_mul(4).saturating_add(2),
                    |settings| delay_maximum_frames(settings, sample_rate),
                )
                .saturating_mul(std::mem::size_of::<[f32; 2]>() as u64),
            32,
        ),
        ProcessorKind::Reverb => (reverb_heap_bytes(sample_rate), 160),
        ProcessorKind::Limiter => {
            let lookahead = settings
                .as_ref()
                .and_then(|settings| settings.numeric("lookahead"))
                .unwrap_or(0.005);
            let frames = (lookahead.max(0.0) * f64::from(sample_rate)).round() as usize;
            (limiter_heap_bytes(frames), limiter_work_units(frames))
        }
        ProcessorKind::Eq => {
            let bands = settings.as_ref().map_or(0, eq_band_count) as u64;
            (
                bands.saturating_mul(std::mem::size_of::<StereoBiquad>() as u64),
                bands.saturating_mul(24),
            )
        }
        ProcessorKind::Compressor => (0, 28),
        ProcessorKind::Saturation => (
            stereo_delay_heap_bytes(
                u32::try_from(OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES).unwrap_or(u32::MAX),
            ),
            saturation_work_units(),
        ),
        ProcessorKind::Utility => (0, 8),
        ProcessorKind::Latency => (0, 2),
    };
    (fixed_and_declared.saturating_add(algorithm_heap), work)
}

fn delay_maximum_frames(settings: &ResolvedProcessorSettings, sample_rate: u32) -> u64 {
    if settings.choice("tempo_sync") == Some("true") {
        let seconds = settings.numeric("time").unwrap_or(4.0);
        (seconds * f64::from(sample_rate)).ceil() as u64 + 2
    } else {
        u64::from(sample_rate).saturating_mul(4).saturating_add(2)
    }
}

/// Mutable preallocated state for one shared stereo processor instance.
#[derive(Debug, Clone)]
pub(crate) struct ProcessorRuntime {
    program: ProcessorProgram,
    state: ProcessorState,
    declared_delay: StereoDelay,
}

// Large fixed FIR histories stay inline so callback state has no hidden heap owner.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
enum ProcessorState {
    Utility,
    Eq(Vec<StereoBiquad>),
    Compressor {
        envelope: f64,
        gain: f64,
        gain_reduction_db: f32,
    },
    Delay(DelayState),
    Reverb(ReverbState),
    Saturation(SaturationState),
    Limiter(LimiterState),
    Latency,
}

impl ProcessorRuntime {
    fn new(program: ProcessorProgram) -> Self {
        let state = match &program.config {
            ProcessorConfig::Utility { .. } => ProcessorState::Utility,
            ProcessorConfig::Eq { bands } => {
                ProcessorState::Eq(vec![StereoBiquad::default(); bands.len()])
            }
            ProcessorConfig::Compressor { .. } => ProcessorState::Compressor {
                envelope: 0.0,
                gain: 1.0,
                gain_reduction_db: 0.0,
            },
            ProcessorConfig::Delay { maximum_frames, .. } => {
                ProcessorState::Delay(DelayState::new(*maximum_frames))
            }
            ProcessorConfig::Reverb { .. } => {
                ProcessorState::Reverb(ReverbState::new(program.sample_rate))
            }
            ProcessorConfig::Saturation { .. } => {
                ProcessorState::Saturation(SaturationState::new())
            }
            ProcessorConfig::Limiter {
                lookahead_frames, ..
            } => ProcessorState::Limiter(LimiterState::new(*lookahead_frames)),
            ProcessorConfig::Latency => ProcessorState::Latency,
        };
        let declared_delay = StereoDelay::new(program.declared_latency_frames as usize);
        Self {
            program,
            state,
            declared_delay,
        }
    }

    pub(crate) fn process(
        &mut self,
        sample: [f32; 2],
        sidechain: Option<[f32; 2]>,
        controls: &[f64],
    ) -> [f32; 2] {
        let processed = match (&self.program.config, &mut self.state) {
            (
                ProcessorConfig::Utility {
                    gain,
                    width,
                    invert,
                    mono,
                },
                ProcessorState::Utility,
            ) => {
                let mut output = sample;
                if *mono {
                    let mono = (output[0] + output[1]) * 0.5;
                    output = [mono, mono];
                }
                let mid = (output[0] + output[1]) * 0.5;
                let side = (output[0] - output[1]) * 0.5 * width.value(controls) as f32;
                let polarity = if *invert { -1.0 } else { 1.0 };
                let gain = db_gain(gain.value(controls)) * polarity;
                [(mid + side) * gain, (mid - side) * gain]
            }
            (ProcessorConfig::Eq { bands }, ProcessorState::Eq(states)) => {
                let mut output = sample;
                for (band, state) in bands.iter().zip(states) {
                    let coefficients = BiquadCoefficients::for_band(
                        band.kind,
                        band.frequency.value(controls),
                        band.gain.value(controls),
                        band.q.value(controls),
                        self.program.sample_rate,
                    );
                    output = state.process(output, coefficients);
                }
                output
            }
            (
                ProcessorConfig::Compressor {
                    threshold,
                    ratio,
                    attack,
                    release,
                    knee,
                    makeup,
                    mix,
                    rms,
                },
                ProcessorState::Compressor {
                    envelope,
                    gain,
                    gain_reduction_db,
                },
            ) => {
                let detector_sample = sidechain.unwrap_or(sample);
                let detector = if *rms {
                    ((f64::from(detector_sample[0]).powi(2)
                        + f64::from(detector_sample[1]).powi(2))
                        * 0.5)
                        .sqrt()
                } else {
                    f64::from(detector_sample[0].abs().max(detector_sample[1].abs()))
                };
                let attack_coefficient =
                    time_coefficient(attack.value(controls), self.program.sample_rate);
                let release_coefficient =
                    time_coefficient(release.value(controls), self.program.sample_rate);
                let coefficient = if detector > *envelope {
                    attack_coefficient
                } else {
                    release_coefficient
                };
                *envelope = detector + coefficient * (*envelope - detector);
                let level_db = 20.0 * envelope.max(1.0e-12).log10();
                let threshold = threshold.value(controls);
                let compressed_over = compressor_curve_reduction_db(
                    level_db,
                    threshold,
                    ratio.value(controls),
                    knee.value(controls),
                );
                *gain = f64::from(db_gain(-compressed_over));
                *gain_reduction_db = compressed_over as f32;
                let mix = mix.value(controls).clamp(0.0, 1.0) as f32;
                let wet_gain = *gain as f32 * db_gain(makeup.value(controls));
                [
                    sample[0] + (sample[0] * wet_gain - sample[0]) * mix,
                    sample[1] + (sample[1] * wet_gain - sample[1]) * mix,
                ]
            }
            (
                ProcessorConfig::Delay {
                    time,
                    feedback,
                    mix,
                    low_pass,
                    high_pass,
                    time_crossfade,
                    mode,
                    maximum_frames,
                },
                ProcessorState::Delay(state),
            ) => state.process(
                sample,
                time.value(controls),
                feedback.value(controls),
                mix.value(controls),
                low_pass.value(controls),
                high_pass.value(controls),
                time_crossfade.value(controls),
                *mode,
                *maximum_frames,
                self.program.sample_rate,
            ),
            (
                ProcessorConfig::Reverb {
                    pre_delay,
                    diffusion,
                    decay,
                    damping,
                    modulation,
                    width,
                    mix,
                },
                ProcessorState::Reverb(state),
            ) => state.process(
                sample,
                pre_delay.value(controls),
                diffusion.value(controls),
                decay.value(controls),
                damping.value(controls),
                modulation.value(controls),
                width.value(controls),
                mix.value(controls),
                self.program.sample_rate,
            ),
            (
                ProcessorConfig::Saturation {
                    drive,
                    mix,
                    output,
                    mode,
                },
                ProcessorState::Saturation(state),
            ) => state.process(
                sample,
                drive.value(controls),
                mix.value(controls),
                output.value(controls),
                *mode,
            ),
            (
                ProcessorConfig::Limiter {
                    ceiling, release, ..
                },
                ProcessorState::Limiter(state),
            ) => state.process(
                sample,
                db_gain(ceiling.value(controls)).max(1.0e-6),
                release.value(controls),
                self.program.sample_rate,
            ),
            (ProcessorConfig::Latency, ProcessorState::Latency) => sample,
            _ => sample,
        };
        self.declared_delay.process(processed)
    }

    pub(crate) fn reset(&mut self) {
        match &mut self.state {
            ProcessorState::Utility | ProcessorState::Latency => {}
            ProcessorState::Eq(states) => states.fill(StereoBiquad::default()),
            ProcessorState::Compressor {
                envelope,
                gain,
                gain_reduction_db,
            } => {
                *envelope = 0.0;
                *gain = 1.0;
                *gain_reduction_db = 0.0;
            }
            ProcessorState::Delay(state) => state.reset(),
            ProcessorState::Reverb(state) => state.reset(),
            ProcessorState::Saturation(state) => state.reset(),
            ProcessorState::Limiter(state) => state.reset(),
        }
        self.declared_delay.reset();
    }

    #[cfg(test)]
    pub(crate) fn heap_storage_fingerprint(&self) -> u64 {
        fn include(seed: u64, pointer: *const (), capacity: usize) -> u64 {
            seed.rotate_left(7) ^ (pointer as usize as u64) ^ (capacity as u64).rotate_left(31)
        }
        let mut fingerprint = include(
            0xCBF2_9CE4_8422_2325,
            self.declared_delay.buffer.as_ptr().cast(),
            self.declared_delay.buffer.capacity(),
        );
        match &self.state {
            ProcessorState::Eq(states) => {
                fingerprint = include(fingerprint, states.as_ptr().cast(), states.capacity());
            }
            ProcessorState::Delay(state) => {
                fingerprint = include(
                    fingerprint,
                    state.buffer.as_ptr().cast(),
                    state.buffer.capacity(),
                );
            }
            ProcessorState::Reverb(state) => {
                fingerprint = include(
                    fingerprint,
                    state.pre_delay.buffer.as_ptr().cast(),
                    state.pre_delay.buffer.capacity(),
                );
                for delay in state.left_combs.iter().chain(&state.right_combs) {
                    fingerprint = include(
                        fingerprint,
                        delay.buffer.as_ptr().cast(),
                        delay.buffer.capacity(),
                    );
                }
                for delay in state.left_diffusers.iter().chain(&state.right_diffusers) {
                    fingerprint = include(
                        fingerprint,
                        delay.buffer.as_ptr().cast(),
                        delay.buffer.capacity(),
                    );
                }
            }
            ProcessorState::Saturation(state) => {
                fingerprint = include(
                    fingerprint,
                    state.dry_delay.buffer.as_ptr().cast(),
                    state.dry_delay.buffer.capacity(),
                );
            }
            ProcessorState::Limiter(state) => {
                fingerprint = include(
                    fingerprint,
                    state.audio_buffer.as_ptr().cast(),
                    state.audio_buffer.capacity(),
                );
                fingerprint = include(
                    fingerprint,
                    state.peak_buffer.as_ptr().cast(),
                    state.peak_buffer.capacity(),
                );
            }
            ProcessorState::Utility
            | ProcessorState::Compressor { .. }
            | ProcessorState::Latency => {}
        }
        fingerprint
    }

    pub(crate) fn gain_reduction_db(&self) -> Option<f32> {
        match self.state {
            ProcessorState::Compressor {
                gain_reduction_db, ..
            } => Some(gain_reduction_db),
            _ => None,
        }
    }
}

fn db_gain(decibels: f64) -> f32 {
    10.0_f64.powf(decibels.clamp(-192.0, 96.0) / 20.0) as f32
}

fn time_coefficient(seconds: f64, sample_rate: u32) -> f64 {
    (-1.0 / (seconds.max(1.0e-6) * f64::from(sample_rate))).exp()
}

fn compressor_curve_reduction_db(level_db: f64, threshold: f64, ratio: f64, knee: f64) -> f64 {
    let ratio = ratio.max(1.0);
    let knee = knee.max(0.0);
    let slope = 1.0 - 1.0 / ratio;
    let over = level_db - threshold;
    if knee == 0.0 || over >= knee * 0.5 {
        over.max(0.0) * slope
    } else if over <= -knee * 0.5 {
        0.0
    } else {
        let transition = over + knee * 0.5;
        transition * transition * slope / (2.0 * knee)
    }
}

#[derive(Debug, Clone)]
struct StereoDelay {
    buffer: Vec<[f32; 2]>,
    cursor: usize,
    delay_frames: usize,
}

impl StereoDelay {
    fn new(delay_frames: usize) -> Self {
        Self {
            buffer: vec![[0.0, 0.0]; delay_frames.max(1)],
            cursor: 0,
            delay_frames,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill([0.0; 2]);
        self.cursor = 0;
    }

    fn process(&mut self, sample: [f32; 2]) -> [f32; 2] {
        if self.delay_frames == 0 {
            return sample;
        }
        let output = self.buffer[self.cursor];
        self.buffer[self.cursor] = sample;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        output
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct StereoBiquad {
    z1: [f64; 2],
    z2: [f64; 2],
}

impl StereoBiquad {
    fn process(&mut self, sample: [f32; 2], coefficients: BiquadCoefficients) -> [f32; 2] {
        let mut output = [0.0_f32; 2];
        for channel in 0..2 {
            let input = f64::from(sample[channel]);
            let value = coefficients.b0 * input + self.z1[channel];
            self.z1[channel] = coefficients.b1 * input - coefficients.a1 * value + self.z2[channel];
            self.z2[channel] = coefficients.b2 * input - coefficients.a2 * value;
            output[channel] = value as f32;
        }
        output
    }
}

#[derive(Debug, Clone, Copy)]
struct BiquadCoefficients {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl BiquadCoefficients {
    fn for_band(kind: EqBandKind, frequency: f64, gain_db: f64, q: f64, sample_rate: u32) -> Self {
        let frequency = frequency.clamp(10.0, f64::from(sample_rate) * 0.49);
        let q = q.clamp(0.1, 20.0);
        let omega = 2.0 * PI * frequency / f64::from(sample_rate);
        let cosine = omega.cos();
        let sine = omega.sin();
        let alpha = sine / (2.0 * q);
        let amplitude = 10.0_f64.powf(gain_db.clamp(-36.0, 36.0) / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match kind {
            EqBandKind::HighPass => (
                (1.0 + cosine) * 0.5,
                -(1.0 + cosine),
                (1.0 + cosine) * 0.5,
                1.0 + alpha,
                -2.0 * cosine,
                1.0 - alpha,
            ),
            EqBandKind::LowPass => (
                (1.0 - cosine) * 0.5,
                1.0 - cosine,
                (1.0 - cosine) * 0.5,
                1.0 + alpha,
                -2.0 * cosine,
                1.0 - alpha,
            ),
            EqBandKind::Peak => (
                1.0 + alpha * amplitude,
                -2.0 * cosine,
                1.0 - alpha * amplitude,
                1.0 + alpha / amplitude,
                -2.0 * cosine,
                1.0 - alpha / amplitude,
            ),
            EqBandKind::LowShelf => {
                let root = amplitude.sqrt();
                let alpha = sine * std::f64::consts::FRAC_1_SQRT_2;
                (
                    amplitude
                        * ((amplitude + 1.0) - (amplitude - 1.0) * cosine + 2.0 * root * alpha),
                    2.0 * amplitude * ((amplitude - 1.0) - (amplitude + 1.0) * cosine),
                    amplitude
                        * ((amplitude + 1.0) - (amplitude - 1.0) * cosine - 2.0 * root * alpha),
                    (amplitude + 1.0) + (amplitude - 1.0) * cosine + 2.0 * root * alpha,
                    -2.0 * ((amplitude - 1.0) + (amplitude + 1.0) * cosine),
                    (amplitude + 1.0) + (amplitude - 1.0) * cosine - 2.0 * root * alpha,
                )
            }
            EqBandKind::HighShelf => {
                let root = amplitude.sqrt();
                let alpha = sine * std::f64::consts::FRAC_1_SQRT_2;
                (
                    amplitude
                        * ((amplitude + 1.0) + (amplitude - 1.0) * cosine + 2.0 * root * alpha),
                    -2.0 * amplitude * ((amplitude - 1.0) + (amplitude + 1.0) * cosine),
                    amplitude
                        * ((amplitude + 1.0) + (amplitude - 1.0) * cosine - 2.0 * root * alpha),
                    (amplitude + 1.0) - (amplitude - 1.0) * cosine + 2.0 * root * alpha,
                    2.0 * ((amplitude - 1.0) - (amplitude + 1.0) * cosine),
                    (amplitude + 1.0) - (amplitude - 1.0) * cosine - 2.0 * root * alpha,
                )
            }
        };
        let inverse = 1.0 / a0.max(1.0e-12);
        Self {
            b0: b0 * inverse,
            b1: b1 * inverse,
            b2: b2 * inverse,
            a1: a1 * inverse,
            a2: a2 * inverse,
        }
    }
}

#[derive(Debug, Clone)]
struct DelayState {
    buffer: Vec<[f32; 2]>,
    cursor: usize,
    low_pass_state: [f32; 2],
    high_pass_state: [f32; 2],
    high_pass_input: [f32; 2],
    current_delay_frames: f64,
    transition_from_frames: f64,
    transition_to_frames: f64,
    transition_position: usize,
    transition_frames: usize,
}

impl DelayState {
    fn new(maximum_frames: usize) -> Self {
        Self {
            buffer: vec![[0.0, 0.0]; maximum_frames.max(3)],
            cursor: 0,
            low_pass_state: [0.0, 0.0],
            high_pass_state: [0.0, 0.0],
            high_pass_input: [0.0, 0.0],
            current_delay_frames: 0.0,
            transition_from_frames: 0.0,
            transition_to_frames: 0.0,
            transition_position: 0,
            transition_frames: 0,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill([0.0; 2]);
        self.cursor = 0;
        self.low_pass_state = [0.0; 2];
        self.high_pass_state = [0.0; 2];
        self.high_pass_input = [0.0; 2];
        self.current_delay_frames = 0.0;
        self.transition_from_frames = 0.0;
        self.transition_to_frames = 0.0;
        self.transition_position = 0;
        self.transition_frames = 0;
    }

    fn read(&self, delay_frames: f64) -> [f32; 2] {
        let integer = delay_frames.floor() as usize;
        let fraction = (delay_frames - integer as f64) as f32;
        let first =
            (self.cursor + self.buffer.len() - integer % self.buffer.len()) % self.buffer.len();
        let second = (first + self.buffer.len() - 1) % self.buffer.len();
        [
            self.buffer[first][0] * (1.0 - fraction) + self.buffer[second][0] * fraction,
            self.buffer[first][1] * (1.0 - fraction) + self.buffer[second][1] * fraction,
        ]
    }

    #[allow(clippy::too_many_arguments)]
    fn process(
        &mut self,
        sample: [f32; 2],
        time: f64,
        feedback: f64,
        mix: f64,
        low_pass: f64,
        high_pass: f64,
        time_crossfade: f64,
        mode: DelayMode,
        maximum_frames: usize,
        sample_rate: u32,
    ) -> [f32; 2] {
        let target_delay = (time.max(1.0 / f64::from(sample_rate)) * f64::from(sample_rate))
            .clamp(1.0, maximum_frames.saturating_sub(2) as f64);
        if self.current_delay_frames == 0.0 {
            self.current_delay_frames = target_delay;
        } else if self.transition_frames == 0
            && (target_delay - self.current_delay_frames).abs() > 1.0e-9
        {
            self.transition_from_frames = self.current_delay_frames;
            self.transition_to_frames = target_delay;
            self.transition_position = 0;
            self.transition_frames = (time_crossfade.clamp(0.001, 0.1) * f64::from(sample_rate))
                .round()
                .max(2.0) as usize;
        }
        let mut delayed = if self.transition_frames == 0 {
            self.read(self.current_delay_frames)
        } else {
            let denominator = self.transition_frames.saturating_sub(1).max(1) as f32;
            let blend = self.transition_position as f32 / denominator;
            let from = self.read(self.transition_from_frames);
            let to = self.read(self.transition_to_frames);
            [
                from[0] + (to[0] - from[0]) * blend,
                from[1] + (to[1] - from[1]) * blend,
            ]
        };
        if self.transition_frames > 0 {
            self.transition_position = self.transition_position.saturating_add(1);
            if self.transition_position >= self.transition_frames {
                self.current_delay_frames = self.transition_to_frames;
                self.transition_position = 0;
                self.transition_frames = 0;
            }
        }

        let low_alpha = (-2.0 * PI * low_pass.clamp(10.0, f64::from(sample_rate) * 0.49)
            / f64::from(sample_rate))
        .exp() as f32;
        let high_alpha = (-2.0 * PI * high_pass.clamp(10.0, f64::from(sample_rate) * 0.49)
            / f64::from(sample_rate))
        .exp() as f32;
        for (channel, delayed_sample) in delayed.iter_mut().enumerate() {
            self.low_pass_state[channel] =
                *delayed_sample * (1.0 - low_alpha) + self.low_pass_state[channel] * low_alpha;
            self.high_pass_state[channel] = high_alpha
                * (self.high_pass_state[channel] + self.low_pass_state[channel]
                    - self.high_pass_input[channel]);
            self.high_pass_input[channel] = self.low_pass_state[channel];
            *delayed_sample = self.high_pass_state[channel];
        }
        let feedback = feedback.clamp(0.0, 0.999) as f32;
        let feedback_signal = match mode {
            DelayMode::Stereo => delayed,
            DelayMode::PingPong => [delayed[1], delayed[0]],
        };
        self.buffer[self.cursor] = [
            sample[0] + feedback_signal[0] * feedback,
            sample[1] + feedback_signal[1] * feedback,
        ];
        self.cursor = (self.cursor + 1) % self.buffer.len();
        let mix = mix.clamp(0.0, 1.0) as f32;
        [
            sample[0] + (delayed[0] - sample[0]) * mix,
            sample[1] + (delayed[1] - sample[1]) * mix,
        ]
    }
}

#[derive(Debug, Clone)]
struct MonoDelay {
    buffer: Vec<f32>,
    cursor: usize,
    base_frames: f64,
    damping_state: f32,
}

impl MonoDelay {
    fn new(frames: usize) -> Self {
        Self {
            buffer: vec![0.0; frames.saturating_add(REVERB_COMB_INTERPOLATION_PADDING)],
            cursor: 0,
            base_frames: frames.max(1) as f64,
            damping_state: 0.0,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.cursor = 0;
        self.damping_state = 0.0;
    }

    fn read(&self, delay_frames: f64) -> f32 {
        let delay_frames = delay_frames.clamp(1.0, self.buffer.len().saturating_sub(2) as f64);
        let position = (self.cursor as f64 - delay_frames).rem_euclid(self.buffer.len() as f64);
        let earlier = position.floor() as usize % self.buffer.len();
        let later = (earlier + 1) % self.buffer.len();
        let fraction = (position - position.floor()) as f32;
        self.buffer[earlier] + (self.buffer[later] - self.buffer[earlier]) * fraction
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32, modulation_frames: f64) -> f32 {
        let output = self.read(self.base_frames + modulation_frames);
        self.damping_state += (output - self.damping_state) * (1.0 - damping);
        self.buffer[self.cursor] = input + self.damping_state * feedback;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        output
    }
}

#[derive(Debug, Clone)]
struct AllpassDelay {
    buffer: Vec<f32>,
    cursor: usize,
}

impl AllpassDelay {
    fn new(frames: usize) -> Self {
        Self {
            buffer: vec![0.0; frames.max(1)],
            cursor: 0,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.cursor = 0;
    }

    fn process(&mut self, input: f32, coefficient: f32) -> f32 {
        let delayed = self.buffer[self.cursor];
        let output = delayed - coefficient * input;
        self.buffer[self.cursor] = input + coefficient * output;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        output
    }
}

#[derive(Debug, Clone)]
struct ReverbPreDelay {
    buffer: Vec<[f32; 2]>,
    cursor: usize,
    current_frames: f64,
    transition_from_frames: f64,
    transition_to_frames: f64,
    transition_position: usize,
    transition_frames: usize,
}

impl ReverbPreDelay {
    fn new(sample_rate: u32) -> Self {
        Self {
            buffer: vec![[0.0, 0.0]; sample_rate as usize + 2],
            cursor: 0,
            current_frames: -1.0,
            transition_from_frames: 0.0,
            transition_to_frames: 0.0,
            transition_position: 0,
            transition_frames: 0,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill([0.0; 2]);
        self.cursor = 0;
        self.current_frames = -1.0;
        self.transition_from_frames = 0.0;
        self.transition_to_frames = 0.0;
        self.transition_position = 0;
        self.transition_frames = 0;
    }

    fn read(&self, sample: [f32; 2], delay_frames: f64) -> [f32; 2] {
        if delay_frames <= 0.0 {
            return sample;
        }
        let position = (self.cursor as f64 - delay_frames).rem_euclid(self.buffer.len() as f64);
        let earlier = position.floor() as usize % self.buffer.len();
        let later = (earlier + 1) % self.buffer.len();
        let fraction = (position - position.floor()) as f32;
        [
            self.buffer[earlier][0] + (self.buffer[later][0] - self.buffer[earlier][0]) * fraction,
            self.buffer[earlier][1] + (self.buffer[later][1] - self.buffer[earlier][1]) * fraction,
        ]
    }

    fn process(&mut self, sample: [f32; 2], seconds: f64, sample_rate: u32) -> [f32; 2] {
        let target =
            (seconds.clamp(0.0, 1.0) * f64::from(sample_rate)).clamp(0.0, f64::from(sample_rate));
        if self.current_frames < 0.0 {
            self.current_frames = target;
        } else if self.transition_frames == 0 && (target - self.current_frames).abs() > 1.0e-9 {
            self.transition_from_frames = self.current_frames;
            self.transition_to_frames = target;
            self.transition_position = 0;
            self.transition_frames = (0.01 * f64::from(sample_rate)).round().max(2.0) as usize;
        }
        let delayed = if self.transition_frames == 0 {
            self.read(sample, self.current_frames)
        } else {
            let denominator = self.transition_frames.saturating_sub(1).max(1) as f32;
            let blend = self.transition_position as f32 / denominator;
            let from = self.read(sample, self.transition_from_frames);
            let to = self.read(sample, self.transition_to_frames);
            [
                from[0] + (to[0] - from[0]) * blend,
                from[1] + (to[1] - from[1]) * blend,
            ]
        };
        self.buffer[self.cursor] = sample;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        if self.transition_frames > 0 {
            self.transition_position = self.transition_position.saturating_add(1);
            if self.transition_position >= self.transition_frames {
                self.current_frames = self.transition_to_frames;
                self.transition_position = 0;
                self.transition_frames = 0;
            }
        }
        delayed
    }
}

#[derive(Debug, Clone)]
struct ReverbState {
    pre_delay: ReverbPreDelay,
    left_combs: Vec<MonoDelay>,
    right_combs: Vec<MonoDelay>,
    left_diffusers: Vec<AllpassDelay>,
    right_diffusers: Vec<AllpassDelay>,
    phase: f64,
}

impl ReverbState {
    fn new(sample_rate: u32) -> Self {
        let (left_combs, right_combs) = reverb_comb_lengths(sample_rate);
        let (left_diffusers, right_diffusers) = reverb_diffuser_lengths(sample_rate);
        Self {
            pre_delay: ReverbPreDelay::new(sample_rate),
            left_combs: left_combs.into_iter().map(MonoDelay::new).collect(),
            right_combs: right_combs.into_iter().map(MonoDelay::new).collect(),
            left_diffusers: left_diffusers.into_iter().map(AllpassDelay::new).collect(),
            right_diffusers: right_diffusers.into_iter().map(AllpassDelay::new).collect(),
            phase: 0.0,
        }
    }

    fn reset(&mut self) {
        self.pre_delay.reset();
        for delay in self.left_combs.iter_mut().chain(&mut self.right_combs) {
            delay.reset();
        }
        for diffuser in self
            .left_diffusers
            .iter_mut()
            .chain(&mut self.right_diffusers)
        {
            diffuser.reset();
        }
        self.phase = 0.0;
    }

    #[allow(clippy::too_many_arguments)]
    fn process(
        &mut self,
        sample: [f32; 2],
        pre_delay: f64,
        diffusion: f64,
        decay: f64,
        damping: f64,
        modulation: f64,
        width: f64,
        mix: f64,
        sample_rate: u32,
    ) -> [f32; 2] {
        let delayed = self.pre_delay.process(sample, pre_delay, sample_rate);

        let network_input = [
            (delayed[0] * 0.75 + delayed[1] * 0.25) * REVERB_INPUT_GAIN,
            (delayed[1] * 0.75 + delayed[0] * 0.25) * REVERB_INPUT_GAIN,
        ];
        self.phase = (self.phase + 0.17 / f64::from(sample_rate)) % 1.0;
        let feedback = (0.15 + decay.clamp(0.0, 1.0) * 0.835).clamp(0.0, 0.985) as f32;
        let damping = damping.clamp(0.0, 1.0) as f32;
        let modulation_depth = modulation.clamp(0.0, 1.0)
            * REVERB_MODULATION_DEPTH_FRAMES_48K
            * f64::from(sample_rate)
            / 48_000.0;
        let mut wet = [0.0_f32; 2];
        for (index, delay) in self.left_combs.iter_mut().enumerate() {
            let phase = (self.phase + index as f64 * 0.173).fract() * 2.0 * PI;
            wet[0] += delay.process(
                network_input[0],
                feedback,
                damping,
                phase.sin() * modulation_depth,
            );
        }
        for (index, delay) in self.right_combs.iter_mut().enumerate() {
            let phase = (self.phase + 0.25 + index as f64 * 0.173).fract() * 2.0 * PI;
            wet[1] += delay.process(
                network_input[1],
                feedback,
                damping,
                phase.sin() * modulation_depth,
            );
        }
        wet[0] *= 0.25;
        wet[1] *= 0.25;

        let diffusion = diffusion.clamp(0.0, 1.0) as f32 * 0.75;
        for diffuser in &mut self.left_diffusers {
            wet[0] = diffuser.process(wet[0], diffusion);
        }
        for diffuser in &mut self.right_diffusers {
            wet[1] = diffuser.process(wet[1], diffusion);
        }

        let mid = (wet[0] + wet[1]) * 0.5;
        let side = (wet[0] - wet[1]) * 0.5 * width.clamp(0.0, 1.0) as f32;
        wet = [mid + side, mid - side];
        let mix = mix.clamp(0.0, 1.0) as f32;
        [
            sample[0] + (wet[0] - sample[0]) * mix,
            sample[1] + (wet[1] - sample[1]) * mix,
        ]
    }
}

fn saturate(sample: f32, mode: SaturationMode) -> f32 {
    match mode {
        SaturationMode::SoftClip => sample / (1.0 + sample.abs()),
        SaturationMode::Tanh => sample.tanh(),
        SaturationMode::Tape => {
            let biased = sample + 0.05;
            biased.tanh() - 0.05_f32.tanh()
        }
    }
}

#[derive(Debug, Clone)]
struct SaturationState {
    oversampler: FilteredOversampler,
    dry_delay: StereoDelay,
}

impl SaturationState {
    fn new() -> Self {
        Self {
            oversampler: FilteredOversampler::default(),
            dry_delay: StereoDelay::new(OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES),
        }
    }

    fn reset(&mut self) {
        self.oversampler.reset();
        self.dry_delay.reset();
    }

    fn process(
        &mut self,
        sample: [f32; 2],
        drive: f64,
        mix: f64,
        output: f64,
        mode: SaturationMode,
    ) -> [f32; 2] {
        let drive = drive.max(0.0) as f32;
        let wet = self.oversampler.process(sample, |sample| {
            [
                saturate(sample[0] * drive, mode),
                saturate(sample[1] * drive, mode),
            ]
        });
        let dry = self.dry_delay.process(sample);
        let mix = mix.clamp(0.0, 1.0) as f32;
        let output_gain = db_gain(output);
        [
            (dry[0] + (wet[0] - dry[0]) * mix) * output_gain,
            (dry[1] + (wet[1] - dry[1]) * mix) * output_gain,
        ]
    }
}

#[derive(Debug, Clone)]
struct LimiterState {
    audio_buffer: Vec<[f32; 2]>,
    audio_cursor: usize,
    detector: OversamplingInterpolator,
    peak_buffer: Vec<f32>,
    peak_cursor: usize,
    gain: f32,
}

impl LimiterState {
    fn new(lookahead_frames: usize) -> Self {
        let latency_frames = lookahead_frames
            .saturating_add(OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES)
            .max(1);
        Self {
            audio_buffer: vec![[0.0, 0.0]; latency_frames],
            audio_cursor: 0,
            detector: OversamplingInterpolator::default(),
            peak_buffer: vec![0.0; lookahead_frames.saturating_add(1).max(1)],
            peak_cursor: 0,
            gain: 1.0,
        }
    }

    fn reset(&mut self) {
        self.audio_buffer.fill([0.0; 2]);
        self.audio_cursor = 0;
        self.detector.reset();
        self.peak_buffer.fill(0.0);
        self.peak_cursor = 0;
        self.gain = 1.0;
    }

    fn process(
        &mut self,
        sample: [f32; 2],
        ceiling: f32,
        release: f64,
        sample_rate: u32,
    ) -> [f32; 2] {
        let true_peak = self
            .detector
            .process(sample)
            .into_iter()
            .flatten()
            .map(f32::abs)
            .fold(1.0e-12_f32, f32::max);
        self.peak_buffer[self.peak_cursor] = true_peak;
        self.peak_cursor = (self.peak_cursor + 1) % self.peak_buffer.len();

        let output = self.audio_buffer[self.audio_cursor];
        self.audio_buffer[self.audio_cursor] = sample;
        self.audio_cursor = (self.audio_cursor + 1) % self.audio_buffer.len();

        let window_peak = self.peak_buffer.iter().copied().fold(1.0e-12_f32, f32::max);
        let target = (ceiling * LIMITER_TRUE_PEAK_SAFETY_RATIO / window_peak).min(1.0);
        if target < self.gain {
            self.gain = target;
        } else {
            let coefficient = time_coefficient(release, sample_rate) as f32;
            self.gain = target + coefficient * (self.gain - target);
        }
        [output[0] * self.gain, output[1] * self.gain]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_program(kind: ProcessorKind, config: ProcessorConfig) -> ProcessorProgram {
        ProcessorProgram {
            id: "test".to_owned(),
            kind,
            sidechain_inputs: Vec::new(),
            authored_latency_frames: 0,
            declared_latency_frames: 0,
            config,
            sample_rate: 48_000,
        }
    }

    fn eq_band(kind: EqBandKind, frequency: f64, gain: f64, q: f64) -> EqBandProgram {
        EqBandProgram {
            kind,
            frequency: Scalar::Constant(frequency),
            gain: Scalar::Constant(gain),
            q: Scalar::Constant(q),
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct TestCompressor {
        threshold: f64,
        ratio: f64,
        attack: f64,
        release: f64,
        knee: f64,
        makeup: f64,
        mix: f64,
        rms: bool,
    }

    impl Default for TestCompressor {
        fn default() -> Self {
            Self {
                threshold: -24.0,
                ratio: 4.0,
                attack: 0.001,
                release: 0.1,
                knee: 0.0,
                makeup: 0.0,
                mix: 1.0,
                rms: false,
            }
        }
    }

    impl TestCompressor {
        fn runtime(self) -> ProcessorRuntime {
            test_program(
                ProcessorKind::Compressor,
                ProcessorConfig::Compressor {
                    threshold: Scalar::Constant(self.threshold),
                    ratio: Scalar::Constant(self.ratio),
                    attack: Scalar::Constant(self.attack),
                    release: Scalar::Constant(self.release),
                    knee: Scalar::Constant(self.knee),
                    makeup: Scalar::Constant(self.makeup),
                    mix: Scalar::Constant(self.mix),
                    rms: self.rms,
                },
            )
            .runtime()
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct TestReverb {
        pre_delay: f64,
        diffusion: f64,
        decay: f64,
        damping: f64,
        modulation: f64,
        width: f64,
        mix: f64,
    }

    impl Default for TestReverb {
        fn default() -> Self {
            Self {
                pre_delay: 0.0,
                diffusion: 0.7,
                decay: 0.65,
                damping: 0.35,
                modulation: 0.1,
                width: 1.0,
                mix: 1.0,
            }
        }
    }

    impl TestReverb {
        fn runtime(self) -> ProcessorRuntime {
            test_program(
                ProcessorKind::Reverb,
                ProcessorConfig::Reverb {
                    pre_delay: Scalar::Constant(self.pre_delay),
                    diffusion: Scalar::Constant(self.diffusion),
                    decay: Scalar::Constant(self.decay),
                    damping: Scalar::Constant(self.damping),
                    modulation: Scalar::Constant(self.modulation),
                    width: Scalar::Constant(self.width),
                    mix: Scalar::Constant(self.mix),
                },
            )
            .runtime()
        }

        fn impulse_from(self, initial: [f32; 2], frames: usize) -> Vec<[f32; 2]> {
            let mut runtime = self.runtime();
            (0..frames)
                .map(|frame| {
                    let input = if frame == 0 { initial } else { [0.0, 0.0] };
                    runtime.process(input, None, &[])
                })
                .collect()
        }

        fn impulse(self, frames: usize) -> Vec<[f32; 2]> {
            self.impulse_from([1.0, 1.0], frames)
        }
    }

    fn tone_amplitude(samples: &[f32], frequency: f64) -> f64 {
        let (sine, cosine) = samples.iter().enumerate().fold(
            (0.0_f64, 0.0_f64),
            |(sine, cosine), (frame, sample)| {
                let phase = 2.0 * PI * frequency * frame as f64 / 48_000.0;
                (
                    sine + f64::from(*sample) * phase.sin(),
                    cosine + f64::from(*sample) * phase.cos(),
                )
            },
        );
        2.0 * sine.hypot(cosine) / samples.len() as f64
    }

    fn reconstructed_true_peak(samples: &[f32]) -> f64 {
        let mut interpolator = OversamplingInterpolator::default();
        samples
            .iter()
            .flat_map(|sample| interpolator.process([*sample, *sample]))
            .skip(usize::from(OVERSAMPLING_FILTER_TAPS) * usize::from(OVERSAMPLING_FACTOR))
            .flatten()
            .map(|sample| f64::from(sample.abs()))
            .fold(0.0_f64, f64::max)
    }

    fn measured_eq_gain(band: EqBandProgram, test_frequency: f64) -> f64 {
        let mut runtime =
            test_program(ProcessorKind::Eq, ProcessorConfig::Eq { bands: vec![band] }).runtime();
        let mut input_energy = 0.0_f64;
        let mut output_energy = 0.0_f64;
        for frame in 0..48_000 {
            let phase = 2.0 * PI * test_frequency * frame as f64 / 48_000.0;
            let input = (phase.sin() * 0.1) as f32;
            let output = runtime.process([input, input], None, &[])[0];
            if frame >= 12_000 {
                input_energy += f64::from(input).powi(2);
                output_energy += f64::from(output).powi(2);
            }
        }
        (output_energy / input_energy).sqrt()
    }

    fn stereo_correlation(samples: &[[f32; 2]]) -> f64 {
        let cross = samples
            .iter()
            .map(|sample| f64::from(sample[0]) * f64::from(sample[1]))
            .sum::<f64>();
        let left = samples
            .iter()
            .map(|sample| f64::from(sample[0]).powi(2))
            .sum::<f64>();
        let right = samples
            .iter()
            .map(|sample| f64::from(sample[1]).powi(2))
            .sum::<f64>();
        cross / (left * right).sqrt()
    }

    #[test]
    fn utility_gain_polarity_mono_and_width_have_explicit_safe_semantics() {
        let mut narrowing = test_program(
            ProcessorKind::Utility,
            ProcessorConfig::Utility {
                gain: Scalar::Constant(0.0),
                width: Scalar::Constant(0.25),
                invert: false,
                mono: false,
            },
        )
        .runtime();
        let input = [[1.0, -0.5], [0.25, 0.75], [-0.8, -0.2], [0.1, -0.9]];
        let output = input.map(|sample| narrowing.process(sample, None, &[]));
        for (input, output) in input.into_iter().zip(output) {
            let input_mid = (input[0] + input[1]) * 0.5;
            let input_side = (input[0] - input[1]) * 0.5;
            let output_mid = (output[0] + output[1]) * 0.5;
            let output_side = (output[0] - output[1]) * 0.5;
            assert!((output_mid - input_mid).abs() < 1.0e-6);
            assert!((output_side - input_side * 0.25).abs() < 1.0e-6);
        }
        assert!(stereo_correlation(&output) >= stereo_correlation(&input));

        let mut mono_inverted = test_program(
            ProcessorKind::Utility,
            ProcessorConfig::Utility {
                gain: Scalar::Constant(6.0),
                width: Scalar::Constant(1.0),
                invert: true,
                mono: true,
            },
        )
        .runtime();
        let processed = mono_inverted.process([1.0, -0.5], None, &[]);
        let expected = -0.25 * db_gain(6.0);
        assert!((processed[0] - expected).abs() < 1.0e-6);
        assert!((processed[1] - expected).abs() < 1.0e-6);
    }

    #[test]
    fn every_eq_band_has_the_expected_frequency_response() {
        let high_pass_low = measured_eq_gain(
            eq_band(
                EqBandKind::HighPass,
                500.0,
                0.0,
                std::f64::consts::FRAC_1_SQRT_2,
            ),
            50.0,
        );
        let high_pass_high = measured_eq_gain(
            eq_band(
                EqBandKind::HighPass,
                500.0,
                0.0,
                std::f64::consts::FRAC_1_SQRT_2,
            ),
            5_000.0,
        );
        assert!(high_pass_low < 0.02 && high_pass_high > 0.99);

        let low_pass_low = measured_eq_gain(
            eq_band(
                EqBandKind::LowPass,
                2_000.0,
                0.0,
                std::f64::consts::FRAC_1_SQRT_2,
            ),
            200.0,
        );
        let low_pass_high = measured_eq_gain(
            eq_band(
                EqBandKind::LowPass,
                2_000.0,
                0.0,
                std::f64::consts::FRAC_1_SQRT_2,
            ),
            15_000.0,
        );
        assert!(low_pass_low > 0.99 && low_pass_high < 0.02);

        let low_shelf_low = measured_eq_gain(eq_band(EqBandKind::LowShelf, 500.0, 6.0, 1.0), 50.0);
        let low_shelf_high =
            measured_eq_gain(eq_band(EqBandKind::LowShelf, 500.0, 6.0, 1.0), 5_000.0);
        assert!(low_shelf_low > 1.9 && (0.95..1.05).contains(&low_shelf_high));

        let high_shelf_low =
            measured_eq_gain(eq_band(EqBandKind::HighShelf, 5_000.0, 6.0, 1.0), 500.0);
        let high_shelf_high =
            measured_eq_gain(eq_band(EqBandKind::HighShelf, 5_000.0, 6.0, 1.0), 15_000.0);
        assert!((0.95..1.05).contains(&high_shelf_low) && high_shelf_high > 1.9);

        let peak_center = measured_eq_gain(eq_band(EqBandKind::Peak, 1_000.0, 6.0, 1.0), 1_000.0);
        let peak_far = measured_eq_gain(eq_band(EqBandKind::Peak, 1_000.0, 6.0, 1.0), 100.0);
        assert!(peak_center > 1.9 && (0.95..1.05).contains(&peak_far));
        let wide_flank = measured_eq_gain(eq_band(EqBandKind::Peak, 1_000.0, 6.0, 0.5), 1_500.0);
        let narrow_flank = measured_eq_gain(eq_band(EqBandKind::Peak, 1_000.0, 6.0, 5.0), 1_500.0);
        assert!(wide_flank > narrow_flank + 0.2);
    }

    #[test]
    fn every_eq_band_has_a_finite_decaying_impulse_response() {
        for band in [
            eq_band(EqBandKind::HighPass, 1_000.0, 0.0, 0.707),
            eq_band(EqBandKind::LowShelf, 500.0, 6.0, 1.0),
            eq_band(EqBandKind::Peak, 1_000.0, -6.0, 2.0),
            eq_band(EqBandKind::HighShelf, 5_000.0, 6.0, 1.0),
            eq_band(EqBandKind::LowPass, 4_000.0, 0.0, 0.707),
        ] {
            let mut runtime =
                test_program(ProcessorKind::Eq, ProcessorConfig::Eq { bands: vec![band] })
                    .runtime();
            let mut last = 0.0_f32;
            for frame in 0..16_384 {
                let input = if frame == 0 { 1.0 } else { 0.0 };
                last = runtime.process([input, input], None, &[])[0];
                assert!(last.is_finite());
            }
            assert!(last.abs() < 1.0e-6);
        }
    }

    #[test]
    fn eq_contract_boundaries_remain_finite_and_bounded() {
        let mut bands = Vec::new();
        for frequency in [10.0, 23_520.0] {
            bands.push(eq_band(EqBandKind::HighPass, frequency, 0.0, 0.707));
            bands.push(eq_band(EqBandKind::LowPass, frequency, 0.0, 0.707));
            for gain in [-24.0, 24.0] {
                bands.push(eq_band(EqBandKind::LowShelf, frequency, gain, 1.0));
                bands.push(eq_band(EqBandKind::HighShelf, frequency, gain, 1.0));
                for q in [0.1, 20.0] {
                    bands.push(eq_band(EqBandKind::Peak, frequency, gain, q));
                }
            }
        }
        for band in bands {
            let coefficients = BiquadCoefficients::for_band(
                band.kind,
                band.frequency.value(&[]),
                band.gain.value(&[]),
                band.q.value(&[]),
                48_000,
            );
            for value in [
                coefficients.b0,
                coefficients.b1,
                coefficients.b2,
                coefficients.a1,
                coefficients.a2,
            ] {
                assert!(value.is_finite());
            }
            let mut runtime =
                test_program(ProcessorKind::Eq, ProcessorConfig::Eq { bands: vec![band] })
                    .runtime();
            for frame in 0..48_000 {
                let input = if frame == 0 { 1.0 } else { 0.0 };
                let output = runtime.process([input, -input], None, &[]);
                assert!(output.into_iter().all(|sample| sample.is_finite()));
                assert!(output.into_iter().all(|sample| sample.abs() < 1.0e6));
            }
        }
    }

    #[test]
    fn eq_output_is_independent_of_processing_quantum() {
        fn render(quantum: usize) -> Vec<[f32; 2]> {
            let bands = vec![
                eq_band(EqBandKind::HighPass, 30.0, 0.0, 0.707),
                eq_band(EqBandKind::LowShelf, 120.0, 2.0, 1.0),
                eq_band(EqBandKind::Peak, 500.0, -2.0, 0.8),
                eq_band(EqBandKind::Peak, 2_500.0, 1.5, 1.2),
                eq_band(EqBandKind::HighShelf, 8_000.0, -1.0, 1.0),
                eq_band(EqBandKind::LowPass, 20_000.0, 0.0, 0.707),
            ];
            let mut runtime =
                test_program(ProcessorKind::Eq, ProcessorConfig::Eq { bands }).runtime();
            let input = (0..8_192)
                .map(|frame| {
                    let phase = frame as f64 / 48_000.0;
                    [
                        (0.3 * (2.0 * PI * 110.0 * phase).sin()
                            + 0.1 * (2.0 * PI * 3_100.0 * phase).sin())
                            as f32,
                        (0.2 * (2.0 * PI * 220.0 * phase).sin()
                            - 0.1 * (2.0 * PI * 7_000.0 * phase).sin())
                            as f32,
                    ]
                })
                .collect::<Vec<_>>();
            let mut output = Vec::with_capacity(input.len());
            for block in input.chunks(quantum) {
                output.extend(
                    block
                        .iter()
                        .map(|sample| runtime.process(*sample, None, &[])),
                );
            }
            output
        }

        let baseline = render(64);
        assert_eq!(render(128), baseline);
        assert_eq!(render(256), baseline);
    }

    #[test]
    fn delay_read_interpolates_fractional_frames() {
        let mut delay = DelayState::new(8);
        delay.cursor = 4;
        delay.buffer[3] = [1.0, 0.0];
        delay.buffer[2] = [0.0, 2.0];
        assert_eq!(delay.read(1.25), [0.75, 0.5]);
    }

    #[test]
    fn delay_ping_pong_crossfeeds_feedback_while_stereo_does_not() {
        fn echoes(mode: DelayMode) -> [[f32; 2]; 3] {
            let mut delay = DelayState::new(16);
            let mut output = [[0.0, 0.0]; 3];
            for (frame, sample) in [[1.0, 0.0], [0.0, 0.0], [0.0, 0.0]].into_iter().enumerate() {
                output[frame] = delay.process(
                    sample,
                    1.0 / 48_000.0,
                    0.5,
                    1.0,
                    23_520.0,
                    10.0,
                    0.001,
                    mode,
                    16,
                    48_000,
                );
            }
            output
        }

        let stereo = echoes(DelayMode::Stereo);
        let ping_pong = echoes(DelayMode::PingPong);
        assert!(stereo[1][0] > 0.9 && ping_pong[1][0] > 0.9);
        assert!(stereo[2][0].abs() > stereo[2][1].abs() * 5.0);
        assert!(ping_pong[2][1].abs() > ping_pong[2][0].abs() * 5.0);
    }

    #[test]
    fn delay_feedback_filters_have_expected_frequency_selectivity() {
        fn measured_gain(frequency: f64, low_pass: f64, high_pass: f64) -> f64 {
            let mut delay = DelayState::new(16);
            let mut input_energy = 0.0_f64;
            let mut output_energy = 0.0_f64;
            for frame in 0..48_000 {
                let input = (2.0 * PI * frequency * frame as f64 / 48_000.0).sin() as f32;
                let output = delay.process(
                    [input, input],
                    1.0 / 48_000.0,
                    0.0,
                    1.0,
                    low_pass,
                    high_pass,
                    0.001,
                    DelayMode::Stereo,
                    16,
                    48_000,
                )[0];
                if frame >= 4_800 {
                    input_energy += f64::from(input).powi(2);
                    output_energy += f64::from(output).powi(2);
                }
            }
            (output_energy / input_energy).sqrt()
        }

        fn repeat_ratio(frequency: f64) -> f64 {
            const BURST_FRAMES: usize = 1_024;
            const DELAY_FRAMES: usize = 2_048;
            let mut delay = DelayState::new(DELAY_FRAMES + 2);
            let mut first_energy = 0.0_f64;
            let mut second_energy = 0.0_f64;
            for frame in 0..(DELAY_FRAMES * 2 + BURST_FRAMES) {
                let input = if frame < BURST_FRAMES {
                    let window = (PI * frame as f64 / BURST_FRAMES as f64).sin().powi(2);
                    (window * (2.0 * PI * frequency * frame as f64 / 48_000.0).sin()) as f32
                } else {
                    0.0
                };
                let output = delay.process(
                    [input, input],
                    DELAY_FRAMES as f64 / 48_000.0,
                    0.8,
                    1.0,
                    1_000.0,
                    10.0,
                    0.001,
                    DelayMode::Stereo,
                    DELAY_FRAMES + 2,
                    48_000,
                )[0];
                if (DELAY_FRAMES..DELAY_FRAMES + BURST_FRAMES).contains(&frame) {
                    first_energy += f64::from(output).powi(2);
                }
                if (DELAY_FRAMES * 2..DELAY_FRAMES * 2 + BURST_FRAMES).contains(&frame) {
                    second_energy += f64::from(output).powi(2);
                }
            }
            (second_energy / first_energy).sqrt()
        }

        let low_pass_low = measured_gain(200.0, 1_000.0, 10.0);
        let low_pass_high = measured_gain(10_000.0, 1_000.0, 10.0);
        assert!(low_pass_low > low_pass_high * 5.0);
        let high_pass_low = measured_gain(200.0, 23_520.0, 1_000.0);
        let high_pass_high = measured_gain(10_000.0, 23_520.0, 1_000.0);
        assert!(high_pass_high > high_pass_low * 4.0);
        assert!(repeat_ratio(200.0) > repeat_ratio(10_000.0) * 5.0);
    }

    #[test]
    fn delay_time_change_crossfade_avoids_an_abrupt_read_head_jump() {
        let mut delay = DelayState::new(64);
        let old_time = 10.0 / 48_000.0;
        let new_time = 34.0 / 48_000.0;
        let mut frame = 0_usize;
        let previous = loop {
            let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
            let output = delay.process(
                [input, input],
                old_time,
                0.0,
                1.0,
                23_520.0,
                10.0,
                0.001,
                DelayMode::Stereo,
                64,
                48_000,
            )[0];
            frame += 1;
            if frame > 128 && output > 0.85 {
                break output;
            }
        };
        let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
        let mut abrupt = delay.clone();
        abrupt.current_delay_frames = 34.0;
        let abrupt_output = abrupt.process(
            [input, input],
            new_time,
            0.0,
            1.0,
            23_520.0,
            10.0,
            0.001,
            DelayMode::Stereo,
            64,
            48_000,
        )[0];
        let safe_output = delay.process(
            [input, input],
            new_time,
            0.0,
            1.0,
            23_520.0,
            10.0,
            0.001,
            DelayMode::Stereo,
            64,
            48_000,
        )[0];
        assert!((safe_output - previous).abs() < 0.2);
        assert!((abrupt_output - previous).abs() > 1.5);
        for _ in 1..48 {
            frame += 1;
            let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
            let output = delay.process(
                [input, input],
                new_time,
                0.0,
                1.0,
                23_520.0,
                10.0,
                0.001,
                DelayMode::Stereo,
                64,
                48_000,
            );
            assert!(output.into_iter().all(f32::is_finite));
        }
        assert_eq!(delay.transition_frames, 0);
        assert!((delay.current_delay_frames - 34.0).abs() < 1.0e-9);
    }

    #[test]
    fn delay_remains_finite_at_maximum_feedback_during_repeated_time_changes() {
        let mut delay = DelayState::new(258);
        let mut maximum = 0.0_f32;
        for frame in 0..200_000 {
            let delay_frames = [1.25, 17.5, 127.75][(frame / 997) % 3];
            let input = if frame == 0 { [1.0, -1.0] } else { [0.0, 0.0] };
            let output = delay.process(
                input,
                delay_frames / 48_000.0,
                0.999,
                1.0,
                23_520.0,
                10.0,
                0.001,
                DelayMode::PingPong,
                258,
                48_000,
            );
            assert!(output.into_iter().all(f32::is_finite));
            maximum = maximum.max(output[0].abs()).max(output[1].abs());
        }
        assert!(maximum <= 1.0);
    }

    #[test]
    fn declared_delay_is_exact() {
        let mut delay = StereoDelay::new(3);
        assert_eq!(delay.process([1.0, -1.0]), [0.0, 0.0]);
        assert_eq!(delay.process([0.0, 0.0]), [0.0, 0.0]);
        assert_eq!(delay.process([0.0, 0.0]), [0.0, 0.0]);
        assert_eq!(delay.process([0.0, 0.0]), [1.0, -1.0]);
    }

    #[test]
    fn reverb_diffuser_is_a_gain_neutral_allpass() {
        let mut dc = AllpassDelay::new(17);
        let mut output = 0.0;
        for _ in 0..20_000 {
            output = dc.process(1.0, 0.75);
        }
        assert!((output - 1.0).abs() < 1.0e-5);

        let mut impulse = AllpassDelay::new(17);
        let energy = (0..100_000)
            .map(|frame| {
                let output = impulse.process(if frame == 0 { 1.0 } else { 0.0 }, 0.75);
                f64::from(output).powi(2)
            })
            .sum::<f64>();
        assert!((energy - 1.0).abs() < 1.0e-5);

        for frequency in [100.0, 5_000.0, 20_000.0] {
            let mut allpass = AllpassDelay::new(17);
            let mut input_energy = 0.0_f64;
            let mut output_energy = 0.0_f64;
            for frame in 0..96_000 {
                let input = (2.0 * PI * frequency * frame as f64 / 48_000.0).sin() as f32;
                let output = allpass.process(input, 0.75);
                if frame >= 48_000 {
                    input_energy += f64::from(input).powi(2);
                    output_energy += f64::from(output).powi(2);
                }
            }
            let gain = (output_energy / input_energy).sqrt();
            assert!((gain - 1.0).abs() < 1.0e-4, "{frequency} Hz: {gain}");
        }
    }

    #[test]
    fn reverb_zero_pre_delay_reaches_the_network_without_a_ring_wrap() {
        let mut reverb = ReverbState::new(48_000);
        let allocated_bytes = reverb.pre_delay.buffer.capacity() as u64
            * std::mem::size_of::<[f32; 2]>() as u64
            + (reverb.left_combs.capacity() + reverb.right_combs.capacity()) as u64
                * std::mem::size_of::<MonoDelay>() as u64
            + (reverb.left_diffusers.capacity() + reverb.right_diffusers.capacity()) as u64
                * std::mem::size_of::<AllpassDelay>() as u64
            + reverb
                .left_combs
                .iter()
                .chain(&reverb.right_combs)
                .map(|delay| delay.buffer.capacity() as u64 * std::mem::size_of::<f32>() as u64)
                .sum::<u64>()
            + reverb
                .left_diffusers
                .iter()
                .chain(&reverb.right_diffusers)
                .map(|delay| delay.buffer.capacity() as u64 * std::mem::size_of::<f32>() as u64)
                .sum::<u64>();
        assert_eq!(reverb_heap_bytes(48_000), allocated_bytes);
        let program = test_program(
            ProcessorKind::Reverb,
            ProcessorConfig::Reverb {
                pre_delay: Scalar::Constant(0.0),
                diffusion: Scalar::Constant(0.7),
                decay: Scalar::Constant(0.65),
                damping: Scalar::Constant(0.35),
                modulation: Scalar::Constant(0.1),
                width: Scalar::Constant(1.0),
                mix: Scalar::Constant(0.25),
            },
        );
        assert_eq!(
            program.state_bytes(),
            std::mem::size_of::<ProcessorRuntime>() as u64
                + allocated_bytes
                + std::mem::size_of::<[f32; 2]>() as u64
        );
        assert_eq!(program.work_units(), 160);

        let first_wet_frame = (0..5_000).find(|frame| {
            let input = if *frame == 0 { [1.0, 1.0] } else { [0.0, 0.0] };
            let output = reverb.process(input, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 48_000);
            output[0].abs().max(output[1].abs()) > 1.0e-6
        });
        assert!(
            first_wet_frame.is_some_and(|frame| frame < 5_000),
            "zero pre-delay must not wait for the one-second pre-delay ring to wrap"
        );
    }

    #[test]
    fn reverb_pre_delay_shifts_the_wet_onset_by_the_authored_frames() {
        let first_wet_frame = |response: &[[f32; 2]]| {
            response
                .iter()
                .position(|sample| sample[0].abs().max(sample[1].abs()) > 1.0e-7)
                .expect("wet onset")
        };
        let immediate = TestReverb {
            pre_delay: 0.0,
            ..TestReverb::default()
        }
        .impulse(8_000);
        let delayed = TestReverb {
            pre_delay: 0.01,
            ..TestReverb::default()
        }
        .impulse(8_480);
        let maximum = TestReverb {
            pre_delay: 1.0,
            ..TestReverb::default()
        }
        .impulse(56_000);
        assert_eq!(first_wet_frame(&delayed) - first_wet_frame(&immediate), 480);
        assert_eq!(
            first_wet_frame(&maximum) - first_wet_frame(&immediate),
            48_000
        );
    }

    #[test]
    fn reverb_pre_delay_automation_crossfades_between_fractional_taps() {
        let mut delay = ReverbPreDelay::new(48_000);
        let old_time = 10.0 / 48_000.0;
        let new_time = 34.5 / 48_000.0;
        let mut frame = 0_usize;
        let previous = loop {
            let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
            let output = delay.process([input, input], old_time, 48_000)[0];
            frame += 1;
            if frame > 128 && output > 0.85 {
                break output;
            }
        };
        let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
        let mut abrupt = delay.clone();
        abrupt.current_frames = 34.5;
        let abrupt_output = abrupt.process([input, input], new_time, 48_000)[0];
        let safe_output = delay.process([input, input], new_time, 48_000)[0];
        assert!((safe_output - previous).abs() < 0.2);
        assert!((abrupt_output - previous).abs() > 1.5);
        for _ in 1..480 {
            frame += 1;
            let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
            let output = delay.process([input, input], new_time, 48_000);
            assert!(output.into_iter().all(f32::is_finite));
        }
        assert_eq!(delay.transition_frames, 0);
        assert!((delay.current_frames - 34.5).abs() < 1.0e-9);

        let mut fractional = ReverbPreDelay::new(48_000);
        let response = (0..40)
            .map(|frame| {
                fractional.process(
                    if frame == 0 { [1.0, -1.0] } else { [0.0, 0.0] },
                    34.5 / 48_000.0,
                    48_000,
                )
            })
            .collect::<Vec<_>>();
        assert!((response[34][0] - 0.5).abs() < 1.0e-7);
        assert!((response[35][0] - 0.5).abs() < 1.0e-7);
        assert!((response[34][1] + 0.5).abs() < 1.0e-7);
        assert!((response[35][1] + 0.5).abs() < 1.0e-7);
    }

    #[test]
    fn reverb_comb_modulation_moves_a_fractional_delay_tap() {
        let onset = |offset: f64| {
            let mut delay = MonoDelay::new(100);
            (0..120)
                .find(|frame| {
                    let input = if *frame == 0 { 1.0 } else { 0.0 };
                    delay.process(input, 0.0, 0.0, offset).abs() > 1.0e-7
                })
                .expect("modulated comb onset")
        };
        assert_eq!(onset(0.0), 100);
        assert_eq!(onset(8.0), 108);

        let mut fractional = MonoDelay::new(100);
        let response = (0..103)
            .map(|frame| fractional.process(if frame == 0 { 1.0 } else { 0.0 }, 0.0, 0.0, 0.5))
            .collect::<Vec<_>>();
        assert!((response[100] - 0.5).abs() < 1.0e-7);
        assert!((response[101] - 0.5).abs() < 1.0e-7);
    }

    #[test]
    fn reverb_diffusion_increases_echo_density_without_changing_topology() {
        let sparse = TestReverb {
            diffusion: 0.0,
            decay: 0.4,
            damping: 0.0,
            modulation: 0.0,
            ..TestReverb::default()
        }
        .impulse(48_000);
        let dense = TestReverb {
            diffusion: 1.0,
            decay: 0.4,
            damping: 0.0,
            modulation: 0.0,
            ..TestReverb::default()
        }
        .impulse(48_000);
        let active_frames = |response: &[[f32; 2]]| {
            response
                .iter()
                .skip(2_000)
                .filter(|sample| sample[0].abs().max(sample[1].abs()) > 1.0e-7)
                .count()
        };
        assert!(active_frames(&dense) > active_frames(&sparse) * 4);
    }

    #[test]
    fn reverb_decay_and_damping_shape_the_tail() {
        let short = TestReverb {
            decay: 0.1,
            damping: 0.2,
            modulation: 0.0,
            ..TestReverb::default()
        }
        .impulse(192_000);
        let long = TestReverb {
            decay: 0.9,
            damping: 0.2,
            modulation: 0.0,
            ..TestReverb::default()
        }
        .impulse(192_000);
        let energy = |response: &[[f32; 2]], start: usize| {
            response[start..]
                .iter()
                .map(|sample| f64::from(sample[0]).powi(2) + f64::from(sample[1]).powi(2))
                .sum::<f64>()
        };
        let short_tail = energy(&short, 96_000);
        let long_tail = energy(&long, 96_000);
        assert!(
            long_tail > short_tail * 100.0,
            "short={short_tail:e}, long={long_tail:e}"
        );
        assert!(long_tail > 1.0e-12, "long={long_tail:e}");

        let bright = TestReverb {
            decay: 0.85,
            damping: 0.0,
            modulation: 0.0,
            ..TestReverb::default()
        }
        .impulse(96_000);
        let dark = TestReverb {
            decay: 0.85,
            damping: 0.85,
            modulation: 0.0,
            ..TestReverb::default()
        }
        .impulse(96_000);
        let roughness = |response: &[[f32; 2]]| {
            let samples = &response[4_800..];
            let signal = samples
                .iter()
                .map(|sample| f64::from(sample[0]).powi(2) + f64::from(sample[1]).powi(2))
                .sum::<f64>();
            let derivative = samples
                .windows(2)
                .map(|pair| {
                    f64::from(pair[1][0] - pair[0][0]).powi(2)
                        + f64::from(pair[1][1] - pair[0][1]).powi(2)
                })
                .sum::<f64>();
            derivative / signal
        };
        assert!(roughness(&dark) < roughness(&bright));
    }

    #[test]
    fn reverb_modulation_is_deterministic_and_bounded() {
        let unmodulated = TestReverb {
            decay: 0.9,
            modulation: 0.0,
            ..TestReverb::default()
        }
        .impulse(192_000);
        let config = TestReverb {
            decay: 0.9,
            modulation: 1.0,
            ..TestReverb::default()
        };
        let modulated = config.impulse(192_000);
        assert_eq!(modulated, config.impulse(192_000));
        let difference = modulated
            .iter()
            .zip(&unmodulated)
            .map(|(left, right)| {
                f64::from(left[0] - right[0]).powi(2) + f64::from(left[1] - right[1]).powi(2)
            })
            .sum::<f64>();
        assert!(difference > 1.0e-8);
        assert!(modulated.iter().flatten().all(|sample| sample.is_finite()));
        assert!(modulated.iter().flatten().all(|sample| sample.abs() <= 1.0));
    }

    #[test]
    fn reverb_width_preserves_mono_fold_down_and_zero_width_is_mono() {
        let mono = TestReverb {
            width: 0.0,
            modulation: 0.0,
            ..TestReverb::default()
        }
        .impulse(96_000);
        let wide = TestReverb {
            width: 1.0,
            modulation: 0.0,
            ..TestReverb::default()
        }
        .impulse(96_000);
        assert!(mono.iter().all(|sample| sample[0] == sample[1]));
        let side_energy = wide
            .iter()
            .map(|sample| f64::from(sample[0] - sample[1]).powi(2))
            .sum::<f64>();
        assert!(side_energy > 1.0e-8);
        let side_only = TestReverb {
            width: 1.0,
            modulation: 0.0,
            ..TestReverb::default()
        }
        .impulse_from([1.0, -1.0], 96_000);
        let side_only_energy = side_only
            .iter()
            .map(|sample| f64::from(sample[0]).powi(2) + f64::from(sample[1]).powi(2))
            .sum::<f64>();
        assert!(side_only_energy > 1.0e-8);
        for (mono, wide) in mono.iter().zip(&wide) {
            let folded = (wide[0] + wide[1]) * 0.5;
            assert!((mono[0] - folded).abs() < 1.0e-7);
        }
    }

    #[test]
    fn reverb_maximum_controls_remain_finite_with_a_long_tail() {
        let mut runtime = TestReverb {
            diffusion: 1.0,
            decay: 1.0,
            damping: 0.0,
            modulation: 1.0,
            width: 1.0,
            mix: 1.0,
            ..TestReverb::default()
        }
        .runtime();
        let mut maximum = 0.0_f32;
        let mut late_maximum = 0.0_f32;
        for frame in 0..576_000 {
            let input = if frame == 0 { [1.0, 1.0] } else { [0.0, 0.0] };
            let output = runtime.process(input, None, &[]);
            assert!(output.into_iter().all(f32::is_finite));
            maximum = maximum.max(output[0].abs()).max(output[1].abs());
            if frame >= 480_000 {
                late_maximum = late_maximum.max(output[0].abs()).max(output[1].abs());
            }
        }
        assert!(maximum <= 1.0);
        assert!(late_maximum > 1.0e-7);
    }

    #[test]
    fn reverb_is_bounded_for_sustained_dc_and_low_frequency_input() {
        let mut runtime = TestReverb {
            diffusion: 1.0,
            decay: 1.0,
            damping: 0.0,
            modulation: 1.0,
            width: 1.0,
            mix: 1.0,
            ..TestReverb::default()
        }
        .runtime();
        let mut maximum = 0.0_f32;
        for frame in 0..480_000 {
            let low = (2.0 * PI * 20.0 * frame as f64 / 48_000.0).sin() as f32 * 0.2;
            let output = runtime.process([0.3 + low, 0.3 - low], None, &[]);
            assert!(output.into_iter().all(f32::is_finite));
            maximum = maximum.max(output[0].abs()).max(output[1].abs());
        }
        assert!(maximum <= 2.0);
        for _ in 0..480_000 {
            let output = runtime.process([0.0, 0.0], None, &[]);
            assert!(output.into_iter().all(f32::is_finite));
            maximum = maximum.max(output[0].abs()).max(output[1].abs());
        }
        assert!(maximum <= 2.0);
    }

    #[test]
    fn state_bytes_match_delay_runtime_allocations() {
        let program = ProcessorProgram {
            id: "delay".to_owned(),
            kind: ProcessorKind::Delay,
            sidechain_inputs: Vec::new(),
            authored_latency_frames: 3,
            declared_latency_frames: 3,
            config: ProcessorConfig::Delay {
                time: Scalar::Constant(0.25),
                feedback: Scalar::Constant(0.25),
                mix: Scalar::Constant(0.25),
                low_pass: Scalar::Constant(20_000.0),
                high_pass: Scalar::Constant(20.0),
                time_crossfade: Scalar::Constant(0.01),
                mode: DelayMode::Stereo,
                maximum_frames: 17,
            },
            sample_rate: 48_000,
        };
        let expected = program.state_bytes();
        let runtime = program.runtime();
        let ProcessorState::Delay(delay) = &runtime.state else {
            panic!("Delay state");
        };
        let allocated = std::mem::size_of::<ProcessorRuntime>() as u64
            + runtime.declared_delay.buffer.capacity() as u64
                * std::mem::size_of::<[f32; 2]>() as u64
            + delay.buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64;
        assert_eq!(expected, allocated);
    }

    #[test]
    fn compressor_curve_honors_threshold_ratio_and_soft_knee() {
        assert_eq!(compressor_curve_reduction_db(-24.0, -24.0, 4.0, 0.0), 0.0);
        assert_eq!(compressor_curve_reduction_db(-12.0, -24.0, 4.0, 0.0), 9.0);
        assert_eq!(compressor_curve_reduction_db(0.0, -24.0, 1.0, 0.0), 0.0);
        assert_eq!(compressor_curve_reduction_db(-28.0, -24.0, 4.0, 8.0), 0.0);
        assert_eq!(compressor_curve_reduction_db(-20.0, -24.0, 4.0, 8.0), 3.0);
        assert_eq!(compressor_curve_reduction_db(-24.0, -24.0, 4.0, 8.0), 0.75);
        let below_center = compressor_curve_reduction_db(-24.001, -24.0, 4.0, 8.0);
        let above_center = compressor_curve_reduction_db(-23.999, -24.0, 4.0, 8.0);
        assert!(below_center < 0.75 && above_center > 0.75);
    }

    #[test]
    fn compressor_peak_and_rms_detectors_have_distinct_linked_stereo_levels() {
        fn settled_reduction(rms: bool) -> f32 {
            let mut runtime = TestCompressor {
                rms,
                ..TestCompressor::default()
            }
            .runtime();
            for _ in 0..96_000 {
                runtime.process([1.0, 0.0], None, &[]);
            }
            runtime.gain_reduction_db().expect("compressor telemetry")
        }

        let peak = settled_reduction(false);
        let rms = settled_reduction(true);
        assert!((peak - 18.0).abs() < 0.01);
        assert!((rms - 15.742).abs() < 0.01);
        assert!(peak > rms + 2.2);
    }

    #[test]
    fn compressor_envelope_matches_attack_and_release_vectors() {
        let mut runtime = TestCompressor::default().runtime();
        let mut attack_vector = Vec::new();
        for frame in 0..480 {
            runtime.process([0.25, -0.25], Some([1.0, 1.0]), &[]);
            if matches!(frame, 0 | 47 | 479) {
                attack_vector.push(runtime.gain_reduction_db().expect("attack telemetry"));
            }
        }
        for (actual, expected) in attack_vector.into_iter().zip([0.0, 15.012, 18.0]) {
            assert!((actual - expected).abs() < 0.02, "{actual} != {expected}");
        }

        let mut release_vector = Vec::new();
        for frame in 0..14_400 {
            runtime.process([0.25, -0.25], Some([0.0, 0.0]), &[]);
            if matches!(frame, 0 | 479 | 4_799 | 14_399) {
                release_vector.push(runtime.gain_reduction_db().expect("release telemetry"));
            }
        }
        for (actual, expected) in release_vector
            .into_iter()
            .zip([17.998, 17.348, 11.485, 0.0])
        {
            assert!((actual - expected).abs() < 0.02, "{actual} != {expected}");
        }
    }

    #[test]
    fn compressor_wet_dry_mix_is_parallel_and_exact_at_endpoints() {
        fn settled_output(mix: f64) -> [f32; 2] {
            let mut runtime = TestCompressor {
                mix,
                ..TestCompressor::default()
            }
            .runtime();
            let mut output = [0.0, 0.0];
            for _ in 0..48_000 {
                output = runtime.process([0.25, -0.25], Some([1.0, 1.0]), &[]);
            }
            output
        }

        let dry = settled_output(0.0);
        let half = settled_output(0.5);
        let wet = settled_output(1.0);
        assert_eq!(dry, [0.25, -0.25]);
        assert!(wet[0] < 0.04 && wet[1] > -0.04);
        for channel in 0..2 {
            assert!((half[channel] - (dry[channel] + wet[channel]) * 0.5).abs() < 1.0e-6);
        }
    }

    #[test]
    fn compressor_uses_external_sidechain_instead_of_program_audio() {
        fn settled(sidechain: Option<[f32; 2]>) -> ([f32; 2], f32) {
            let mut runtime = TestCompressor::default().runtime();
            let mut output = [0.0, 0.0];
            for _ in 0..48_000 {
                output = runtime.process([0.05, -0.05], sidechain, &[]);
            }
            (
                output,
                runtime.gain_reduction_db().expect("compressor telemetry"),
            )
        }

        let (program_detected, program_reduction) = settled(None);
        let (sidechain_detected, sidechain_reduction) = settled(Some([1.0, 1.0]));
        assert!(program_reduction < 1.0e-4);
        assert!(sidechain_reduction > 17.9);
        assert!(sidechain_detected[0] < program_detected[0] * 0.14);
        assert!(sidechain_detected[1] > program_detected[1] * 0.14);
    }

    #[test]
    fn compressor_reduction_telemetry_excludes_makeup_gain() {
        fn settle(makeup: f64) -> ([f32; 2], f32) {
            let program = ProcessorProgram {
                id: "compressor".to_owned(),
                kind: ProcessorKind::Compressor,
                sidechain_inputs: Vec::new(),
                authored_latency_frames: 0,
                declared_latency_frames: 0,
                config: ProcessorConfig::Compressor {
                    threshold: Scalar::Constant(-24.0),
                    ratio: Scalar::Constant(4.0),
                    attack: Scalar::Constant(0.001),
                    release: Scalar::Constant(0.1),
                    knee: Scalar::Constant(0.0),
                    makeup: Scalar::Constant(makeup),
                    mix: Scalar::Constant(1.0),
                    rms: false,
                },
                sample_rate: 48_000,
            };
            let mut runtime = program.runtime();
            let mut output = [0.0, 0.0];
            for _ in 0..48_000 {
                output = runtime.process([1.0, -1.0], None, &[]);
            }
            (
                output,
                runtime.gain_reduction_db().expect("compressor telemetry"),
            )
        }

        let (without_makeup, reduction_without_makeup) = settle(0.0);
        let (with_makeup, reduction_with_makeup) = settle(12.0);
        assert!(reduction_without_makeup > 17.9);
        assert!((reduction_without_makeup - reduction_with_makeup).abs() < 1.0e-6);
        assert!(with_makeup[0] > without_makeup[0] * 3.9);
        assert!(with_makeup[1] < without_makeup[1] * 3.9);
    }

    #[test]
    fn saturation_filtered_oversampling_suppresses_aliasing_and_preserves_thd() {
        let input = (0..96_000)
            .map(|frame| (2.0 * PI * 10_000.0 * frame as f64 / 48_000.0).sin() as f32 * 0.8)
            .collect::<Vec<_>>();
        for mode in [
            SaturationMode::SoftClip,
            SaturationMode::Tanh,
            SaturationMode::Tape,
        ] {
            let base_rate = input[48_000..]
                .iter()
                .map(|sample| saturate(*sample * 6.0, mode))
                .collect::<Vec<_>>();
            let mut runtime = test_program(
                ProcessorKind::Saturation,
                ProcessorConfig::Saturation {
                    drive: Scalar::Constant(6.0),
                    mix: Scalar::Constant(1.0),
                    output: Scalar::Constant(0.0),
                    mode,
                },
            )
            .runtime();
            let filtered = input
                .iter()
                .map(|sample| runtime.process([*sample, *sample], None, &[])[0])
                .skip(48_000)
                .collect::<Vec<_>>();
            let base_alias = tone_amplitude(&base_rate, 18_000.0);
            let filtered_alias = tone_amplitude(&filtered, 18_000.0);
            let filtered_fundamental = tone_amplitude(&filtered, 10_000.0);
            assert!(base_alias > 0.01, "{mode:?}: base alias={base_alias:e}");
            assert!(
                filtered_alias < base_alias * 0.03,
                "{mode:?}: base={base_alias:e}, filtered={filtered_alias:e}"
            );
            assert!(filtered_fundamental > 0.2, "{mode:?}");
        }

        let mut signatures = Vec::new();
        for mode in [
            SaturationMode::SoftClip,
            SaturationMode::Tanh,
            SaturationMode::Tape,
        ] {
            let mut runtime = test_program(
                ProcessorKind::Saturation,
                ProcessorConfig::Saturation {
                    drive: Scalar::Constant(4.0),
                    mix: Scalar::Constant(1.0),
                    output: Scalar::Constant(0.0),
                    mode,
                },
            )
            .runtime();
            let response = (0..96_000)
                .map(|frame| {
                    let sample = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32 * 0.25;
                    runtime.process([sample, sample], None, &[])[0]
                })
                .skip(48_000)
                .collect::<Vec<_>>();
            assert!(response.iter().all(|sample| sample.is_finite()));
            let fundamental = tone_amplitude(&response, 1_000.0);
            let harmonic_energy = (2..=10)
                .map(|harmonic| tone_amplitude(&response, f64::from(harmonic) * 1_000.0).powi(2))
                .sum::<f64>();
            let thd = harmonic_energy.sqrt() / fundamental;
            assert!(thd > 0.01, "{mode:?}: THD={thd:e}");
            signatures.push(thd);
        }
        assert!(signatures
            .windows(2)
            .all(|pair| (pair[0] - pair[1]).abs() > 1.0e-4));
    }

    #[test]
    fn saturation_aligns_dry_mix_and_reports_exact_state() {
        let program = test_program(
            ProcessorKind::Saturation,
            ProcessorConfig::Saturation {
                drive: Scalar::Constant(10.0),
                mix: Scalar::Constant(0.0),
                output: Scalar::Constant(0.0),
                mode: SaturationMode::SoftClip,
            },
        );
        assert_eq!(
            program.output_latency_frames(),
            OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES as u32
        );
        let expected = program.state_bytes();
        let mut runtime = program.runtime();
        let allocated = std::mem::size_of::<ProcessorRuntime>() as u64
            + runtime.declared_delay.buffer.capacity() as u64
                * std::mem::size_of::<[f32; 2]>() as u64
            + match &runtime.state {
                ProcessorState::Saturation(state) => {
                    state.dry_delay.buffer.capacity() as u64
                        * std::mem::size_of::<[f32; 2]>() as u64
                }
                _ => unreachable!("Saturation state"),
            };
        assert_eq!(expected, allocated);
        for frame in 0..=OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES {
            let input = if frame == 0 { [1.0, -1.0] } else { [0.0, 0.0] };
            let output = runtime.process(input, None, &[]);
            if frame < OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES {
                assert_eq!(output, [0.0, 0.0]);
            } else {
                assert_eq!(output, [1.0, -1.0]);
            }
        }
    }

    #[test]
    fn nonlinear_processors_are_exact_across_block_grouping_and_finite_at_bounds() {
        let input = (0..16_384)
            .map(|frame| {
                let tone = (2.0 * PI * 11_731.0 * frame as f64 / 48_000.0).sin() as f32;
                let impulse = if frame % 997 == 0 { 8.0 } else { 0.0 };
                [tone * 4.0 + impulse, tone * -3.0 - impulse]
            })
            .collect::<Vec<_>>();
        let programs = [
            test_program(
                ProcessorKind::Saturation,
                ProcessorConfig::Saturation {
                    drive: Scalar::Constant(10.0),
                    mix: Scalar::Constant(1.0),
                    output: Scalar::Constant(24.0),
                    mode: SaturationMode::Tape,
                },
            ),
            test_program(
                ProcessorKind::Limiter,
                ProcessorConfig::Limiter {
                    ceiling: Scalar::Constant(-24.0),
                    release: Scalar::Constant(2.0),
                    lookahead_frames: 960,
                },
            ),
        ];
        let process = |program: &ProcessorProgram, quantum: usize| {
            let mut runtime = program.runtime();
            let mut output = Vec::with_capacity(input.len());
            for block in input.chunks(quantum) {
                output.extend(
                    block
                        .iter()
                        .map(|sample| runtime.process(*sample, None, &[])),
                );
            }
            output
        };
        for program in &programs {
            let reference = process(program, 64);
            assert!(reference.iter().flatten().all(|sample| sample.is_finite()));
            assert_eq!(reference, process(program, 128));
            assert_eq!(reference, process(program, 256));
        }
    }

    #[test]
    fn limiter_latency_includes_detector_filter_and_authored_lookahead() {
        let mut limiter = LimiterState::new(1);
        let latency = OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES + 1;
        for frame in 0..=latency {
            let input = if frame == 0 { [1.0, -1.0] } else { [0.0, 0.0] };
            let output = limiter.process(input, 0.8, 0.1, 48_000);
            if frame < latency {
                assert_eq!(output, [0.0, 0.0]);
            } else {
                let expected = 0.8 * LIMITER_TRUE_PEAK_SAFETY_RATIO;
                assert!((output[0] - expected).abs() < 1.0e-4, "{output:?}");
                assert!((output[1] + expected).abs() < 1.0e-4, "{output:?}");
            }
        }
    }

    #[test]
    fn limiter_controls_an_intersample_peak_above_its_sample_peak() {
        let ceiling = 0.9_f32;
        let lookahead = 32;
        let mut limiter = LimiterState::new(lookahead);
        let mut output = Vec::with_capacity(96_000);
        let mut input_sample_peak = 0.0_f32;
        for frame in 0..96_000 {
            let sample = (2.0 * PI * 12_000.0 * frame as f64 / 48_000.0
                + std::f64::consts::FRAC_PI_4)
                .sin() as f32
                * 1.2;
            input_sample_peak = input_sample_peak.max(sample.abs());
            output.push(limiter.process([sample, sample], ceiling, 0.08, 48_000)[0]);
        }
        assert!(input_sample_peak < ceiling);
        let steady = &output[48_000..];
        let output_true_peak = reconstructed_true_peak(steady);
        assert!(
            output_true_peak <= f64::from(ceiling) * 1.001,
            "true peak={output_true_peak:e}"
        );
        assert!(output_true_peak > 0.8);
    }

    #[test]
    fn limiter_controls_reconstructed_output_while_gain_changes() {
        let ceiling = 0.8_f32;
        let mut limiter = LimiterState::new(64);
        let output = (0..96_000)
            .map(|frame| {
                let amplitude = match (frame / 257) % 4 {
                    0 => 0.1,
                    1 => 2.0,
                    2 => 0.35,
                    _ => 1.4,
                };
                let carrier = (2.0 * PI * 11_731.0 * frame as f64 / 48_000.0 + 0.37).sin() as f32;
                let impulse = if frame % 997 == 0 { 2.5 } else { 0.0 };
                limiter.process(
                    [carrier * amplitude + impulse, carrier * amplitude - impulse],
                    ceiling,
                    0.02,
                    48_000,
                )[0]
            })
            .collect::<Vec<_>>();
        let true_peak = reconstructed_true_peak(&output);
        assert!(
            true_peak <= f64::from(ceiling) * 1.001,
            "gain-changing reconstructed peak={true_peak:e}"
        );
    }

    #[test]
    fn limiter_release_recovers_without_changing_declared_latency_or_state() {
        let recovered_gain = |release: f64| {
            let mut limiter = LimiterState::new(32);
            for _ in 0..2_000 {
                let _ = limiter.process([2.0, -2.0], 0.8, release, 48_000);
            }
            assert!(limiter.gain < 0.41);
            for _ in 0..4_800 {
                let _ = limiter.process([0.1, -0.1], 0.8, release, 48_000);
            }
            limiter.gain
        };
        let fast = recovered_gain(0.01);
        let slow = recovered_gain(0.5);
        assert!(fast > 0.99, "fast={fast}");
        assert!(slow < 0.6, "slow={slow}");
        assert!(fast > slow);

        let program = test_program(
            ProcessorKind::Limiter,
            ProcessorConfig::Limiter {
                ceiling: Scalar::Constant(-1.0),
                release: Scalar::Constant(0.08),
                lookahead_frames: 32,
            },
        );
        assert_eq!(
            program.output_latency_frames(),
            (32 + OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES) as u32
        );
        let expected = program.state_bytes();
        let runtime = program.runtime();
        let ProcessorState::Limiter(state) = &runtime.state else {
            unreachable!("Limiter state")
        };
        let allocated = std::mem::size_of::<ProcessorRuntime>() as u64
            + runtime.declared_delay.buffer.capacity() as u64
                * std::mem::size_of::<[f32; 2]>() as u64
            + state.audio_buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64
            + state.peak_buffer.capacity() as u64 * std::mem::size_of::<f32>() as u64;
        assert_eq!(expected, allocated);
    }

    #[test]
    fn limiter_never_exceeds_sample_ceiling() {
        let mut limiter = LimiterState::new(4);
        let latency = 4 + OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES;
        let mut outputs = Vec::new();
        for frame in 0..(latency + 8) {
            let sample = if frame == 0 { [2.0, -2.0] } else { [0.0, 0.0] };
            outputs.push(limiter.process(sample, 0.8, 0.1, 48_000));
        }
        assert!(outputs
            .iter()
            .flatten()
            .all(|sample| sample.abs() <= 0.800_1));
        assert!(outputs.iter().flatten().any(|sample| sample.abs() > 0.78));
    }
}
