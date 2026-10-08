use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

use crate::{
    engine::control::mix_modulation_identity,
    model::{ControlPolarity, LfoDirection, LfoMode, LfoWaveform},
};

use super::Scalar;

pub(super) const MAX_UNISON_VOICES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OscillatorWaveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct OscillatorConfig {
    pub(super) waveform: OscillatorWaveform,
    pub(super) detune: Scalar,
    pub(super) glide: Scalar,
    pub(super) level: Scalar,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct OscillatorState {
    phase: f64,
    frequency: f64,
}

impl OscillatorState {
    pub(super) fn new(seed: u64) -> Self {
        Self {
            phase: seed_phase(seed),
            frequency: 0.0,
        }
    }

    pub(super) fn retrigger(&mut self, seed: u64) {
        self.phase = seed_phase(seed);
    }
}

impl OscillatorConfig {
    pub(super) fn process(
        self,
        state: &mut OscillatorState,
        pitch: f32,
        controls: &[f64],
        sample_rate: f64,
    ) -> f32 {
        let pitch = pitch.max(1.0) as f64;
        let detune = self.detune.value(controls);
        let target = (pitch * 2.0_f64.powf(detune / 12.0)).clamp(1.0, sample_rate * 0.45);
        state.frequency = slew_frequency(
            state.frequency,
            target,
            self.glide.value(controls),
            sample_rate,
        );
        let increment = state.frequency / sample_rate;
        let sample =
            oscillator_sample(self.waveform, state.phase, increment) * self.level.value(controls);
        state.phase = (state.phase + increment).fract();
        sample as f32
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct UnisonConfig {
    pub(super) waveform: OscillatorWaveform,
    pub(super) voices: usize,
    pub(super) detune: Scalar,
    pub(super) width: Scalar,
    pub(super) drift: Scalar,
    pub(super) glide: Scalar,
    pub(super) mod_depth: Scalar,
    pub(super) velocity_amount: Scalar,
    pub(super) level: Scalar,
}

#[derive(Debug, Clone)]
pub(super) struct UnisonState {
    phases: [f64; MAX_UNISON_VOICES],
    drift_phases: [f64; MAX_UNISON_VOICES],
    drift_rates: [f64; MAX_UNISON_VOICES],
    frequency: f64,
}

impl UnisonState {
    pub(super) fn new(seed: u64) -> Self {
        let mut state = Self {
            phases: [0.0; MAX_UNISON_VOICES],
            drift_phases: [0.0; MAX_UNISON_VOICES],
            drift_rates: [0.0; MAX_UNISON_VOICES],
            frequency: 0.0,
        };
        state.retrigger(seed);
        state
    }

    pub(super) fn retrigger(&mut self, seed: u64) {
        for index in 0..MAX_UNISON_VOICES {
            let voice_seed = xorshift(nonzero_seed(seed ^ index as u64));
            self.phases[index] = seed_phase(voice_seed);
            self.drift_phases[index] = seed_phase(xorshift(voice_seed));
            self.drift_rates[index] = 0.05 + seed_phase(xorshift(xorshift(voice_seed))) * 0.1;
        }
    }
}

impl UnisonConfig {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        self,
        state: &mut UnisonState,
        pitch: f32,
        modulation: f32,
        gain: f32,
        velocity: f32,
        has_modulation: bool,
        has_gain: bool,
        has_velocity: bool,
        controls: &[f64],
        sample_rate: f64,
    ) -> [f32; 2] {
        let pitch = pitch.max(1.0) as f64;
        let modulation = if has_modulation {
            modulation as f64
        } else {
            0.0
        };
        let target = (pitch * 2.0_f64.powf(modulation * self.mod_depth.value(controls) / 12.0))
            .clamp(1.0, sample_rate * 0.45);
        state.frequency = slew_frequency(
            state.frequency,
            target,
            self.glide.value(controls),
            sample_rate,
        );
        let gain = if has_gain { gain as f64 } else { 1.0 };
        let velocity = if has_velocity { velocity as f64 } else { 1.0 };
        let velocity_amount = self.velocity_amount.value(controls).clamp(0.0, 1.0);
        let amplitude = gain
            * ((1.0 - velocity_amount) + velocity_amount * velocity.clamp(0.0, 1.0))
            * self.level.value(controls);
        let detune = self.detune.value(controls);
        let drift = self.drift.value(controls);
        let width = self.width.value(controls).clamp(0.0, 1.0);
        let normalization = 1.0 / (self.voices as f64).sqrt();
        let mut output = [0.0_f64, 0.0_f64];
        for subvoice in 0..self.voices {
            let position = if self.voices == 1 {
                0.0
            } else {
                subvoice as f64 * 2.0 / (self.voices - 1) as f64 - 1.0
            };
            let drift_value = (state.drift_phases[subvoice] * TAU).sin() * drift;
            let frequency = (state.frequency
                * 2.0_f64.powf((position * detune + drift_value) / 12.0))
            .clamp(1.0, sample_rate * 0.45);
            let increment = frequency / sample_rate;
            let sample = oscillator_sample(self.waveform, state.phases[subvoice], increment)
                * amplitude
                * normalization;
            state.phases[subvoice] = (state.phases[subvoice] + increment).fract();
            state.drift_phases[subvoice] =
                (state.drift_phases[subvoice] + state.drift_rates[subvoice] / sample_rate).fract();
            let angle = (position * width + 1.0) * PI * 0.25;
            output[0] += sample * angle.cos();
            output[1] += sample * angle.sin();
        }
        [output[0] as f32, output[1] as f32]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnalogPhaseWarp {
    None,
    PulseWidth,
    Bend,
    Remap,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AnalogWarpStage {
    pub(super) kind: AnalogPhaseWarp,
    pub(super) amount: Scalar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnalogDistortion {
    None,
    SoftClip,
    Tanh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnalogAmplitudeMode {
    Am,
    Ring,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AnalogOscillatorConfig {
    pub(super) waveform: OscillatorWaveform,
    pub(super) voices: usize,
    pub(super) coarse: Scalar,
    pub(super) fine: Scalar,
    pub(super) detune: Scalar,
    pub(super) blend: Scalar,
    pub(super) width: Scalar,
    pub(super) drift: Scalar,
    pub(super) glide: Scalar,
    pub(super) fm_depth: Scalar,
    pub(super) pm_depth: Scalar,
    pub(super) amplitude_mode: AnalogAmplitudeMode,
    pub(super) amplitude_depth: Scalar,
    pub(super) phase: Scalar,
    pub(super) random_phase: Scalar,
    pub(super) warps: [Option<AnalogWarpStage>; 2],
    pub(super) distortion: AnalogDistortion,
    pub(super) drive: Scalar,
    pub(super) velocity_amount: Scalar,
    pub(super) level: Scalar,
    pub(super) pan: Scalar,
    pub(super) node_identity: u64,
}

#[derive(Debug, Clone)]
pub(super) struct AnalogOscillatorState {
    phases: [f64; MAX_UNISON_VOICES],
    phase_seeds: [f64; MAX_UNISON_VOICES],
    drift_phases: [f64; MAX_UNISON_VOICES],
    drift_rates: [f64; MAX_UNISON_VOICES],
    frequency: f64,
    last_sync: f64,
    needs_phase_reset: bool,
    node_identity: u64,
}

impl AnalogOscillatorState {
    pub(super) fn new(seed: u64, node_identity: u64) -> Self {
        let mut state = Self {
            phases: [0.0; MAX_UNISON_VOICES],
            phase_seeds: [0.0; MAX_UNISON_VOICES],
            drift_phases: [0.0; MAX_UNISON_VOICES],
            drift_rates: [0.0; MAX_UNISON_VOICES],
            frequency: 0.0,
            last_sync: 0.0,
            needs_phase_reset: true,
            node_identity,
        };
        state.retrigger(seed);
        state
    }

    pub(super) fn retrigger(&mut self, seed: u64) {
        let seed = mix_modulation_identity(seed, self.node_identity);
        for index in 0..MAX_UNISON_VOICES {
            let voice_seed = xorshift(nonzero_seed(seed ^ index as u64));
            self.phase_seeds[index] = seed_phase(voice_seed);
            self.drift_phases[index] = seed_phase(xorshift(voice_seed));
            self.drift_rates[index] = 0.05 + seed_phase(xorshift(xorshift(voice_seed))) * 0.1;
        }
        self.last_sync = 0.0;
        self.needs_phase_reset = true;
    }

    fn reset_phases(&mut self, phase: f64, random_phase: f64) {
        for index in 0..MAX_UNISON_VOICES {
            self.phases[index] = (phase + self.phase_seeds[index] * random_phase).fract();
        }
        self.needs_phase_reset = false;
    }
}

impl AnalogOscillatorConfig {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        self,
        state: &mut AnalogOscillatorState,
        pitch: f32,
        frequency_modulation: f32,
        phase_modulation: f32,
        sync: f32,
        amplitude_modulation: f32,
        gain: f32,
        velocity: f32,
        has_frequency_modulation: bool,
        has_phase_modulation: bool,
        has_sync: bool,
        has_amplitude_modulation: bool,
        has_gain: bool,
        has_velocity: bool,
        controls: &[f64],
        sample_rate: f64,
    ) -> [f32; 2] {
        let phase = self.phase.value(controls).clamp(0.0, 1.0);
        let random_phase = self.random_phase.value(controls).clamp(0.0, 1.0);
        if state.needs_phase_reset {
            state.reset_phases(phase, random_phase);
        }
        let sync = if has_sync { f64::from(sync) } else { 0.0 };
        if has_sync && sync > 0.0 && state.last_sync <= 0.0 {
            state.reset_phases(phase, random_phase);
        }
        state.last_sync = sync;

        let frequency_modulation = if has_frequency_modulation {
            f64::from(frequency_modulation) * self.fm_depth.value(controls)
        } else {
            0.0
        };
        let transposition =
            self.coarse.value(controls) + self.fine.value(controls) + frequency_modulation;
        let target = (f64::from(pitch.max(1.0)) * 2.0_f64.powf(transposition / 12.0))
            .clamp(1.0, sample_rate * 0.45);
        state.frequency = slew_frequency(
            state.frequency,
            target,
            self.glide.value(controls),
            sample_rate,
        );

        let gain = if has_gain { f64::from(gain) } else { 1.0 };
        let velocity = if has_velocity {
            f64::from(velocity).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let velocity_amount = self.velocity_amount.value(controls).clamp(0.0, 1.0);
        let amplitude = gain
            * ((1.0 - velocity_amount) + velocity_amount * velocity)
            * self.level.value(controls);
        let phase_modulation = if has_phase_modulation {
            f64::from(phase_modulation) * self.pm_depth.value(controls)
        } else {
            0.0
        };
        let amplitude_modulation = if has_amplitude_modulation {
            f64::from(amplitude_modulation).clamp(-4.0, 4.0)
        } else {
            0.0
        };
        let amplitude_depth = self.amplitude_depth.value(controls).clamp(0.0, 1.0);
        let amplitude_multiplier = if has_amplitude_modulation {
            match self.amplitude_mode {
                AnalogAmplitudeMode::Am => {
                    (1.0 - amplitude_depth)
                        + amplitude_depth * (amplitude_modulation.clamp(-1.0, 1.0) + 1.0) * 0.5
                }
                AnalogAmplitudeMode::Ring => {
                    (1.0 - amplitude_depth) + amplitude_depth * amplitude_modulation
                }
            }
        } else {
            1.0
        };
        let detune = self.detune.value(controls);
        let drift = self.drift.value(controls);
        let width = self.width.value(controls).clamp(0.0, 1.0);
        let normalization = 1.0 / (self.voices as f64).sqrt();
        let drive = self.drive.value(controls).clamp(0.0, 1.0);
        let mut output = [0.0_f64, 0.0_f64];
        let mut base_sample = 0.0_f64;
        for subvoice in 0..self.voices {
            let position = if self.voices == 1 {
                0.0
            } else {
                subvoice as f64 * 2.0 / (self.voices - 1) as f64 - 1.0
            };
            let drift_value = (state.drift_phases[subvoice] * TAU).sin() * drift;
            let frequency = (state.frequency
                * 2.0_f64.powf((position * detune + drift_value) / 12.0))
            .clamp(1.0, sample_rate * 0.45);
            let increment = frequency / sample_rate;
            let mut warped_phase = (state.phases[subvoice] + phase_modulation).rem_euclid(1.0);
            for stage in self.warps.into_iter().flatten() {
                warped_phase = warp_phase(
                    warped_phase,
                    stage.kind,
                    stage.amount.value(controls).clamp(-1.0, 1.0),
                );
            }
            // The authored stage law is phase read → AM/ring → distortion →
            // amplitude/unison blend/pan. Keeping level out of the nonlinear stage
            // makes a level edit a true post-shaper gain change.
            let shaped = distort(
                oscillator_sample(self.waveform, warped_phase, increment) * amplitude_multiplier,
                self.distortion,
                drive,
            );
            let amplified = shaped * amplitude;
            if subvoice == 0 {
                base_sample = amplified;
            }
            let sample = amplified * normalization;
            state.phases[subvoice] = (state.phases[subvoice] + increment).fract();
            state.drift_phases[subvoice] =
                (state.drift_phases[subvoice] + state.drift_rates[subvoice] / sample_rate).fract();
            let angle = (position * width + 1.0) * PI * 0.25;
            output[0] += sample * angle.cos();
            output[1] += sample * angle.sin();
        }
        if self.voices > 1 {
            let blend = self.blend.value(controls).clamp(0.0, 1.0);
            let dry = 1.0 - blend;
            let normalization = (dry * dry + blend * blend).sqrt().max(0.000_001);
            output[0] = (base_sample * FRAC_1_SQRT_2 * dry + output[0] * blend) / normalization;
            output[1] = (base_sample * FRAC_1_SQRT_2 * dry + output[1] * blend) / normalization;
        }
        let pan = self.pan.value(controls).clamp(-1.0, 1.0);
        if pan > 0.0 {
            output[0] *= (pan * PI * 0.5).cos();
        } else if pan < 0.0 {
            output[1] *= (-pan * PI * 0.5).cos();
        }
        [output[0] as f32, output[1] as f32]
    }
}

pub(super) fn warp_phase(phase: f64, warp: AnalogPhaseWarp, amount: f64) -> f64 {
    match warp {
        AnalogPhaseWarp::None => phase,
        AnalogPhaseWarp::PulseWidth => {
            let width = (0.5 + amount * 0.45).clamp(0.05, 0.95);
            if phase < width {
                phase * 0.5 / width
            } else {
                0.5 + (phase - width) * 0.5 / (1.0 - width)
            }
        }
        AnalogPhaseWarp::Bend => phase.powf(2.0_f64.powf(amount * 2.0)),
        AnalogPhaseWarp::Remap => (phase + amount * (phase * TAU).sin() / TAU).rem_euclid(1.0),
    }
}

fn distort(sample: f64, distortion: AnalogDistortion, amount: f64) -> f64 {
    if distortion == AnalogDistortion::None || amount <= 0.0 {
        return sample;
    }
    let drive = 1.0 + amount * 15.0;
    match distortion {
        AnalogDistortion::None => sample,
        AnalogDistortion::SoftClip => {
            let shaped = sample * drive / (1.0 + (sample * drive).abs());
            shaped / (drive / (1.0 + drive))
        }
        AnalogDistortion::Tanh => (sample * drive).tanh() / drive.tanh(),
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NoiseConfig {
    pub(super) level: Scalar,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NoiseState {
    rng: u64,
}

impl NoiseState {
    pub(super) fn new(seed: u64) -> Self {
        Self {
            rng: nonzero_seed(seed),
        }
    }

    pub(super) fn retrigger(&mut self, seed: u64) {
        self.rng = nonzero_seed(seed);
    }
}

impl NoiseConfig {
    pub(super) fn process(self, state: &mut NoiseState, controls: &[f64]) -> f32 {
        state.rng = xorshift(state.rng);
        // The upper 24 bits are an unsigned fraction, not a signed i32. Map
        // their complete range to bipolar audio without introducing DC.
        let white = (state.rng >> 40) as f32 * (1.0 / 8_388_608.0) - 1.0;
        white * self.level.value(controls) as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GraphLfoRetrigger {
    Note,
    Transport,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LfoConfig {
    pub(super) waveform: LfoWaveform,
    pub(super) custom_points:
        [crate::engine::lfo::LfoPoint; crate::engine::lfo::MAX_CUSTOM_LFO_POINTS],
    pub(super) custom_point_count: u8,
    pub(super) rate: Scalar,
    pub(super) depth: Scalar,
    pub(super) phase: Scalar,
    pub(super) random_phase: Scalar,
    pub(super) swing: Scalar,
    pub(super) mode: LfoMode,
    pub(super) direction: LfoDirection,
    pub(super) polarity: ControlPolarity,
    pub(super) retrigger: GraphLfoRetrigger,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LfoState {
    phase_seed: f64,
    identity: u64,
    elapsed_cycles: f64,
    retrigger: GraphLfoRetrigger,
}

impl LfoState {
    pub(super) fn new(seed: u64, retrigger: GraphLfoRetrigger) -> Self {
        Self {
            phase_seed: seed_phase(seed),
            identity: seed,
            elapsed_cycles: 0.0,
            retrigger,
        }
    }

    pub(super) fn reset(&mut self, seed: u64) {
        self.phase_seed = seed_phase(seed);
        self.identity = seed;
        self.elapsed_cycles = 0.0;
    }

    pub(super) fn retrigger(&mut self, seed: u64) {
        if self.retrigger == GraphLfoRetrigger::Note {
            self.reset(seed);
        }
    }
}

impl LfoConfig {
    pub(super) fn process(
        &self,
        state: &mut LfoState,
        controls: &[f64],
        sample_rate: f64,
        song_frame: u64,
    ) -> f32 {
        let initial_phase = (self.phase.value(controls).clamp(0.0, 1.0)
            + state.phase_seed * self.random_phase.value(controls).clamp(0.0, 1.0))
        .fract();
        let rate = self.rate.value(controls).clamp(0.01, 50.0);
        let elapsed_cycles = match self.retrigger {
            GraphLfoRetrigger::Note => state.elapsed_cycles,
            GraphLfoRetrigger::Transport => song_frame as f64 * rate / sample_rate,
        };
        let (phase, cycle) =
            crate::engine::lfo::position(initial_phase, elapsed_cycles, self.mode, self.direction);
        let phase = crate::engine::lfo::swing_phase(phase, self.swing.value(controls));
        let bipolar = if self.custom_point_count == 0 {
            crate::engine::lfo::bipolar_sample(self.waveform, phase, cycle, state.identity)
        } else {
            crate::engine::lfo::custom_bipolar_sample(
                &self.custom_points[..usize::from(self.custom_point_count)],
                phase,
            )
        };
        let value = match self.polarity {
            ControlPolarity::Unipolar => (bipolar + 1.0) * 0.5,
            ControlPolarity::Bipolar => bipolar,
        } * self.depth.value(controls);
        if self.retrigger == GraphLfoRetrigger::Note {
            state.elapsed_cycles = (state.elapsed_cycles + rate / sample_rate).min(u64::MAX as f64);
        }
        value as f32
    }
}

pub(in crate::engine::graph) fn slew_frequency(
    current: f64,
    target: f64,
    glide_seconds: f64,
    sample_rate: f64,
) -> f64 {
    if current <= 0.0 || glide_seconds <= 0.0 {
        return target;
    }
    let coefficient = 1.0 - (-1.0 / (glide_seconds * sample_rate).max(1.0)).exp();
    let current_octaves = current.log2();
    let target_octaves = target.log2();
    2.0_f64.powf(current_octaves + (target_octaves - current_octaves) * coefficient)
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

pub(super) const fn xorshift(mut value: u64) -> u64 {
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    value
}

pub(super) const fn nonzero_seed(seed: u64) -> u64 {
    if seed == 0 {
        0x9e37_79b9_7f4a_7c15
    } else {
        seed
    }
}

pub(super) fn seed_phase(seed: u64) -> f64 {
    (nonzero_seed(seed) >> 11) as f64 / (1_u64 << 53) as f64
}

#[cfg(test)]
#[path = "oscillator_tests.rs"]
mod tests;
