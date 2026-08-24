use std::f64::consts::{PI, TAU};

use super::Scalar;

pub(super) const MAX_UNISON_VOICES: usize = 8;

#[derive(Debug, Clone, Copy)]
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
        ((state.rng >> 40) as i32 as f32 / i32::MAX as f32) * self.level.value(controls) as f32
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LfoConfig {
    pub(super) waveform: LfoWaveform,
    pub(super) rate: Scalar,
    pub(super) depth: Scalar,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum LfoWaveform {
    Sine,
    Triangle,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LfoState {
    phase: f64,
}

impl LfoState {
    pub(super) fn new(seed: u64) -> Self {
        Self {
            phase: seed_phase(seed),
        }
    }

    pub(super) fn retrigger(&mut self, seed: u64) {
        self.phase = seed_phase(seed);
    }
}

impl LfoConfig {
    pub(super) fn process(self, state: &mut LfoState, controls: &[f64], sample_rate: f64) -> f32 {
        let value = match self.waveform {
            LfoWaveform::Sine => (state.phase * TAU).sin(),
            LfoWaveform::Triangle => 1.0 - 4.0 * (state.phase - 0.5).abs(),
        } * self.depth.value(controls);
        state.phase =
            (state.phase + self.rate.value(controls).clamp(0.01, 50.0) / sample_rate).fract();
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
