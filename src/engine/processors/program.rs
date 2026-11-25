use crate::{
    model::{ParameterTarget, Processor, ProcessorKind},
    processor_semantics::{
        ResolvedProcessorSettings, parse_generic_musical_duration, resolve_settings,
    },
};

use super::*;

use super::super::{
    EngineError,
    control::{CompiledControlProgram, ControlIndex},
    oversampling::{
        OVERSAMPLING_FACTOR, OVERSAMPLING_FILTER_TAPS, OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES,
        OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES,
    },
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Scalar {
    Constant(f64),
    Control(ControlIndex),
}

impl Scalar {
    pub(super) fn value(self, controls: &[f64]) -> f64 {
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
    pub(super) bypass: Scalar,
    pub(super) authored_bypass: bool,
    pub(super) authored_latency_frames: u32,
    pub(super) declared_latency_frames: u32,
    pub(super) config: ProcessorConfig,
    pub(super) sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ProcessorConfig {
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
    Gate {
        mode: GateMode,
        threshold: Scalar,
        ratio: Scalar,
        attack: Scalar,
        hold: Scalar,
        release: Scalar,
        range: Scalar,
        hysteresis: Scalar,
        rms: bool,
        mix: Scalar,
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
    Convolution(ConvolutionProgram),
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
    ModulationDelay {
        delay: Scalar,
        depth: Scalar,
        rate: Scalar,
        feedback: Scalar,
        mix: Scalar,
        width: Scalar,
        mode: ModulationDelayMode,
        maximum_frames: usize,
    },
    Crusher {
        bit_depth: Scalar,
        sample_rate: Scalar,
        reconstruction: Scalar,
        mix: Scalar,
        output: Scalar,
    },
    Stutter {
        trigger: Scalar,
        chance: Scalar,
        crossfade: Scalar,
        mix: Scalar,
        slice_frames: usize,
        repeats: u32,
        interval_frames: u64,
        seed: u64,
    },
    Limiter {
        ceiling: Scalar,
        release: Scalar,
        lookahead_frames: usize,
    },
    Latency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EqBandKind {
    HighPass,
    LowShelf,
    Peak,
    HighShelf,
    LowPass,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct EqBandProgram {
    pub(super) kind: EqBandKind,
    pub(super) frequency: Scalar,
    pub(super) gain: Scalar,
    pub(super) q: Scalar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GateMode {
    Gate,
    Expander,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SaturationMode {
    SoftClip,
    Tanh,
    Tape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DelayMode {
    Stereo,
    PingPong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModulationDelayMode {
    Chorus,
    Flanger,
}

pub(super) const MODULATION_DELAY_MAX_SECONDS: f64 = 0.07;
pub(super) const REVERB_COMB_LENGTHS_48K: [usize; 4] = [1557, 1617, 1491, 1422];
pub(super) const REVERB_DIFFUSER_LENGTHS_48K: [usize; 4] = [225, 556, 441, 341];
pub(super) const REVERB_MODULATION_DEPTH_FRAMES_48K: f64 = 8.0;
pub(super) const REVERB_COMB_INTERPOLATION_PADDING: usize = 10;
pub(super) const REVERB_INPUT_GAIN: f32 = 0.05;

pub(super) fn scaled_stereo_reverb_lengths<const N: usize>(
    lengths_48k: [usize; N],
    sample_rate: u32,
) -> ([usize; N], [usize; N]) {
    let scale = f64::from(sample_rate) / 48_000.0;
    let left = lengths_48k.map(|frames| (frames as f64 * scale).round().max(1.0) as usize);
    let right = lengths_48k.map(|frames| ((frames + 23) as f64 * scale).round().max(1.0) as usize);
    (left, right)
}

pub(super) fn reverb_comb_lengths(sample_rate: u32) -> ([usize; 4], [usize; 4]) {
    scaled_stereo_reverb_lengths(REVERB_COMB_LENGTHS_48K, sample_rate)
}

pub(super) fn reverb_diffuser_lengths(sample_rate: u32) -> ([usize; 4], [usize; 4]) {
    scaled_stereo_reverb_lengths(REVERB_DIFFUSER_LENGTHS_48K, sample_rate)
}

pub(super) fn reverb_heap_bytes(sample_rate: u32) -> u64 {
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

pub(super) fn stereo_delay_heap_bytes(delay_frames: u32) -> u64 {
    u64::from(delay_frames.max(1)).saturating_mul(std::mem::size_of::<[f32; 2]>() as u64)
}

/// Fixed detector headroom used to cover reconstruction error during gain movement.
pub(crate) const LIMITER_TRUE_PEAK_SAFETY_DB: f64 = 0.1;
pub(super) const LIMITER_TRUE_PEAK_SAFETY_RATIO: f32 = 0.988_553_1;
pub(super) const BYPASS_GUARD_FRAMES: f32 = 64.0;

pub(super) fn limiter_heap_bytes(lookahead_frames: usize) -> u64 {
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

pub(super) fn limiter_work_units(lookahead_frames: usize) -> u64 {
    (OVERSAMPLING_FILTER_TAPS as u64)
        .saturating_mul(2)
        .saturating_add((lookahead_frames.saturating_add(1) as u64).saturating_mul(4))
        .saturating_add(24)
}

#[derive(Clone, Copy, Default)]
pub(crate) enum ProcessorAssets<'a> {
    #[default]
    Unloaded,
    Metadata(&'a crate::media::AssetInventory),
    Samples(&'a crate::engine::sample::CompiledSampleLibrary),
}

impl ProcessorProgram {
    pub(crate) fn compile(
        processor: &Processor,
        controls: &CompiledControlProgram,
        sample_rate: u32,
        tempo_bpm: f64,
        assets: ProcessorAssets<'_>,
    ) -> Result<Self, EngineError> {
        let settings = resolve_settings(processor, tempo_bpm)
            .map_err(|error| EngineError::Compile(error.compile_message(processor)))?;
        if !matches!(
            processor.kind,
            ProcessorKind::Compressor | ProcessorKind::Gate
        ) && !processor.sidechain_inputs.is_empty()
        {
            return Err(EngineError::Compile(format!(
                "processor `{}` kind {:?} does not consume sidechain Audio",
                processor.id, processor.kind
            )));
        }
        if matches!(
            processor.kind,
            ProcessorKind::Compressor | ProcessorKind::Gate
        ) && processor.sidechain_inputs.len() > 1
        {
            return Err(EngineError::Compile(format!(
                "compressor `{}` accepts at most one named sidechain input",
                processor.id
            )));
        }
        let scalar = |key: &str| processor_scalar(processor, controls, &settings, key);
        let bypass = controls
            .index(&ParameterTarget::Processor {
                processor: processor.id.clone(),
                parameter: "bypass".to_owned(),
            })
            .map(Scalar::Control)
            .ok_or_else(|| {
                EngineError::Compile(format!(
                    "processor `{}` is missing its compiled bypass Parameter",
                    processor.id
                ))
            })?;
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
            ProcessorKind::Gate => ProcessorConfig::Gate {
                mode: if settings.choice("mode") == Some("expander") {
                    GateMode::Expander
                } else {
                    GateMode::Gate
                },
                threshold: scalar("threshold")?,
                ratio: scalar("ratio")?,
                attack: scalar("attack")?,
                hold: scalar("hold")?,
                release: scalar("release")?,
                range: scalar("range")?,
                hysteresis: scalar("hysteresis")?,
                rms: settings.choice("detector") == Some("rms"),
                mix: scalar("mix")?,
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
            ProcessorKind::Convolution => {
                let id = settings.choice("ir").expect("required IR setting");
                let cuts = [
                    settings.numeric("low_cut").unwrap_or_default(),
                    settings.numeric("high_cut").unwrap_or_default(),
                ];
                let (kernel, frames) = if let ProcessorAssets::Samples(samples) = assets {
                    let source = samples.get(id).ok_or_else(|| {
                        EngineError::Compile(format!(
                            "convolution `{}` has no loaded impulse response `{id}`",
                            processor.id
                        ))
                    })?;
                    let (kernel, frames) = Kernel::compile(source, sample_rate, cuts)?;
                    (Some(kernel), frames)
                } else {
                    let frames = if let ProcessorAssets::Metadata(inventory) = assets {
                        let asset = inventory.get(id).ok_or_else(|| {
                            EngineError::Compile(format!("missing impulse response `{id}`"))
                        })?;
                        ir_frames(&asset.metadata(), sample_rate).map_err(EngineError::Compile)?
                    } else {
                        // Source-only validation has no WAV metadata. Exact admission
                        // runs again with the inventory before preparing kernels.
                        1
                    };
                    (None, frames + filter_padding(cuts, sample_rate))
                };
                ProcessorConfig::Convolution(ConvolutionProgram {
                    mix: scalar("mix")?,
                    pre_delay: scalar("pre_delay")?,
                    level: scalar("level")?,
                    width: scalar("width")?,
                    frames,
                    kernel,
                })
            }
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
            ProcessorKind::ModulationDelay => ProcessorConfig::ModulationDelay {
                delay: scalar("delay")?,
                depth: scalar("depth")?,
                rate: scalar("rate")?,
                feedback: scalar("feedback")?,
                mix: scalar("mix")?,
                width: scalar("width")?,
                mode: if settings.choice("mode") == Some("flanger") {
                    ModulationDelayMode::Flanger
                } else {
                    ModulationDelayMode::Chorus
                },
                maximum_frames: modulation_delay_maximum_frames(sample_rate),
            },
            ProcessorKind::Crusher => ProcessorConfig::Crusher {
                bit_depth: scalar("bit_depth")?,
                sample_rate: scalar("sample_rate")?,
                reconstruction: scalar("reconstruction")?,
                mix: scalar("mix")?,
                output: scalar("output")?,
            },
            ProcessorKind::Stutter => {
                let slice_frames =
                    musical_setting_frames(processor, &settings, "slice", tempo_bpm, sample_rate)?;
                let interval_frames = musical_setting_frames(
                    processor,
                    &settings,
                    "interval",
                    tempo_bpm,
                    sample_rate,
                )? as u64;
                if interval_frames < slice_frames as u64 {
                    return Err(EngineError::Compile(format!(
                        "stutter `{}` interval must be at least its slice duration",
                        processor.id
                    )));
                }
                ProcessorConfig::Stutter {
                    trigger: scalar("trigger")?,
                    chance: scalar("chance")?,
                    crossfade: scalar("crossfade")?,
                    mix: scalar("mix")?,
                    slice_frames,
                    repeats: settings.unsigned("repeats").unwrap_or(4),
                    interval_frames,
                    seed: stable_stutter_seed(
                        settings.unsigned("seed").unwrap_or(0),
                        &processor.id,
                    ),
                }
            }
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
            bypass,
            authored_bypass: processor.bypass,
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

    pub(crate) const fn uses_project_render_tail(&self) -> bool {
        matches!(
            self.config,
            ProcessorConfig::Delay { .. }
                | ProcessorConfig::Reverb { .. }
                | ProcessorConfig::ModulationDelay { .. }
        )
    }

    pub(crate) fn tail_frames_upper_bound(&self) -> Option<u32> {
        match self.config {
            ProcessorConfig::Convolution(ref program) => {
                Some(tail_frames(program.frames, self.sample_rate))
            }
            ProcessorConfig::Crusher { .. } => Some(
                self.sample_rate
                    .saturating_add(99)
                    .saturating_div(100)
                    .saturating_mul(2),
            ),
            ProcessorConfig::Stutter {
                slice_frames,
                repeats,
                ..
            } => Some(
                u32::try_from((slice_frames as u64).saturating_mul(u64::from(repeats)))
                    .unwrap_or(u32::MAX),
            ),
            _ => None,
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
            ProcessorConfig::Convolution(program) => estimate(program.frames, self.sample_rate).0,
            ProcessorConfig::Reverb { .. } => reverb_heap_bytes(self.sample_rate),
            ProcessorConfig::ModulationDelay { maximum_frames, .. } => {
                (*maximum_frames as u64).saturating_mul(std::mem::size_of::<[f32; 2]>() as u64)
            }
            ProcessorConfig::Stutter { slice_frames, .. } => {
                (*slice_frames as u64).saturating_mul(std::mem::size_of::<[f32; 2]>() as u64)
            }
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
            | ProcessorConfig::Gate { .. }
            | ProcessorConfig::Crusher { .. }
            | ProcessorConfig::Utility { .. }
            | ProcessorConfig::Latency => 0,
        };
        (std::mem::size_of::<ProcessorRuntime>() as u64)
            .saturating_add(algorithm_heap)
            .saturating_add(stereo_delay_heap_bytes(self.declared_latency_frames))
            .saturating_add(stereo_delay_heap_bytes(self.output_latency_frames()))
    }

    pub(crate) fn work_units(&self) -> u64 {
        match &self.config {
            ProcessorConfig::Utility { .. } => 8,
            ProcessorConfig::Eq { bands } => 24_u64.saturating_mul(bands.len() as u64),
            ProcessorConfig::Compressor { .. } => 28,
            ProcessorConfig::Gate { .. } => 32,
            ProcessorConfig::Delay { .. } => 32,
            ProcessorConfig::Convolution(program) => estimate(program.frames, self.sample_rate).1,
            ProcessorConfig::Reverb { .. } => 160,
            ProcessorConfig::Saturation { .. } => saturation_work_units(),
            ProcessorConfig::ModulationDelay { .. } => 48,
            ProcessorConfig::Crusher { .. } => 28,
            ProcessorConfig::Stutter { .. } => 24,
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
            && self.bypass == other.bypass
            && self.config == other.config
    }
}

pub(super) fn processor_scalar(
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

pub(super) fn eq_band_count(settings: &ResolvedProcessorSettings) -> usize {
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
    let intrinsic_latency = match processor.kind {
        ProcessorKind::Saturation => {
            u32::try_from(OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES).unwrap_or(u32::MAX)
        }
        ProcessorKind::Limiter => {
            let lookahead = settings
                .as_ref()
                .and_then(|settings| settings.numeric("lookahead"))
                .unwrap_or(0.005);
            let frames = (lookahead.max(0.0) * f64::from(sample_rate)).round() as usize;
            u32::try_from(frames.saturating_add(OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES))
                .unwrap_or(u32::MAX)
        }
        _ => 0,
    };
    let output_latency = declared_latency.saturating_add(intrinsic_latency);
    let fixed_and_declared = (std::mem::size_of::<ProcessorRuntime>() as u64)
        .saturating_add(stereo_delay_heap_bytes(declared_latency))
        .saturating_add(stereo_delay_heap_bytes(output_latency));
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
        ProcessorKind::Convolution => {
            let cuts = ["low_cut", "high_cut"].map(|key| {
                settings
                    .as_ref()
                    .and_then(|settings| settings.numeric(key))
                    .unwrap_or_default()
            });
            estimate(
                sample_rate as usize * MAX_IR_SECONDS as usize + filter_padding(cuts, sample_rate),
                sample_rate,
            )
        }
        ProcessorKind::Reverb => (reverb_heap_bytes(sample_rate), 160),
        ProcessorKind::ModulationDelay => (
            (modulation_delay_maximum_frames(sample_rate) as u64)
                .saturating_mul(std::mem::size_of::<[f32; 2]>() as u64),
            48,
        ),
        ProcessorKind::Crusher => (0, 28),
        ProcessorKind::Stutter => {
            let slice_frames = settings
                .as_ref()
                .and_then(|settings| settings.choice("slice"))
                .and_then(parse_generic_musical_duration)
                .and_then(|duration| duration.to_frames(tempo_bpm, sample_rate).ok())
                .unwrap_or_else(|| {
                    (0.25 * 60.0 * f64::from(sample_rate) / tempo_bpm).round() as u64
                });
            (
                slice_frames.saturating_mul(std::mem::size_of::<[f32; 2]>() as u64),
                24,
            )
        }
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
        ProcessorKind::Gate => (0, 32),
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

fn musical_setting_frames(
    processor: &Processor,
    settings: &ResolvedProcessorSettings,
    key: &str,
    tempo_bpm: f64,
    sample_rate: u32,
) -> Result<usize, EngineError> {
    let literal = settings.choice(key).ok_or_else(|| {
        EngineError::Compile(format!(
            "processor `{}` is missing resolved `{key}`",
            processor.id
        ))
    })?;
    let duration = parse_generic_musical_duration(literal).ok_or_else(|| {
        EngineError::Compile(format!(
            "processor `{}` has invalid musical duration `{literal}` for `{key}`",
            processor.id
        ))
    })?;
    let frames = duration.to_frames(tempo_bpm, sample_rate)?;
    usize::try_from(frames.max(1)).map_err(|_| {
        EngineError::Compile(format!(
            "processor `{}` `{key}` duration exceeds host limits",
            processor.id
        ))
    })
}

pub(super) fn modulation_delay_maximum_frames(sample_rate: u32) -> usize {
    (MODULATION_DELAY_MAX_SECONDS * f64::from(sample_rate))
        .ceil()
        .max(1.0) as usize
        + 2
}

fn delay_maximum_frames(settings: &ResolvedProcessorSettings, sample_rate: u32) -> u64 {
    if settings.choice("tempo_sync") == Some("true") {
        let seconds = settings.numeric("time").unwrap_or(4.0);
        (seconds * f64::from(sample_rate)).ceil() as u64 + 2
    } else {
        u64::from(sample_rate).saturating_mul(4).saturating_add(2)
    }
}
