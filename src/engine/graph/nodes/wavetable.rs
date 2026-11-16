use std::{
    f64::consts::{FRAC_1_SQRT_2, PI, TAU},
    sync::Arc,
};

use crate::engine::{control::mix_modulation_identity, wavetable::CompiledWavetable};

use super::{
    Scalar,
    oscillator::{
        AnalogWarpStage, MAX_UNISON_VOICES, nonzero_seed, seed_phase, slew_frequency, warp_phase,
        xorshift,
    },
};

#[derive(Debug, Clone)]
pub(super) struct WavetableConfig {
    pub(super) table: Arc<CompiledWavetable>,
    pub(super) voices: usize,
    pub(super) coarse: Scalar,
    pub(super) fine: Scalar,
    pub(super) table_position: Scalar,
    pub(super) detune: Scalar,
    pub(super) blend: Scalar,
    pub(super) width: Scalar,
    pub(super) drift: Scalar,
    pub(super) glide: Scalar,
    pub(super) fm_depth: Scalar,
    pub(super) pm_depth: Scalar,
    pub(super) phase: Scalar,
    pub(super) random_phase: Scalar,
    pub(super) warps: [Option<AnalogWarpStage>; 2],
    pub(super) velocity_amount: Scalar,
    pub(super) level: Scalar,
    pub(super) pan: Scalar,
    pub(super) node_identity: u64,
}

#[derive(Debug, Clone)]
pub(super) struct WavetableState {
    phases: [f64; MAX_UNISON_VOICES],
    phase_seeds: [f64; MAX_UNISON_VOICES],
    drift_phases: [f64; MAX_UNISON_VOICES],
    drift_rates: [f64; MAX_UNISON_VOICES],
    frequency: f64,
    last_sync: f64,
    needs_phase_reset: bool,
    node_identity: u64,
}

impl WavetableState {
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

impl WavetableConfig {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        &self,
        state: &mut WavetableState,
        pitch: f32,
        frequency_modulation: f32,
        phase_modulation: f32,
        sync: f32,
        gain: f32,
        velocity: f32,
        has_frequency_modulation: bool,
        has_phase_modulation: bool,
        has_sync: bool,
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
        let target = (f64::from(pitch.max(1.0))
            * 2.0_f64.powf(
                (self.coarse.value(controls) + self.fine.value(controls) + frequency_modulation)
                    / 12.0,
            ))
        .clamp(1.0, sample_rate * 0.45);
        state.frequency = slew_frequency(
            state.frequency,
            target,
            self.glide.value(controls),
            sample_rate,
        );
        let phase_modulation = if has_phase_modulation {
            f64::from(phase_modulation) * self.pm_depth.value(controls)
        } else {
            0.0
        };
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
        let position = self.table_position.value(controls).clamp(0.0, 1.0);
        let detune = self.detune.value(controls);
        let drift = self.drift.value(controls);
        let width = self.width.value(controls).clamp(0.0, 1.0);
        let power = 1.0 / (self.voices as f64).sqrt();
        let mut output = [0.0_f64; 2];
        let mut base_sample = 0.0;
        for subvoice in 0..self.voices {
            let spread = if self.voices == 1 {
                0.0
            } else {
                subvoice as f64 * 2.0 / (self.voices - 1) as f64 - 1.0
            };
            let drift_value = (state.drift_phases[subvoice] * TAU).sin() * drift;
            let frequency = (state.frequency
                * 2.0_f64.powf((spread * detune + drift_value) / 12.0))
            .clamp(1.0, sample_rate * 0.45);
            let increment = frequency / sample_rate;
            let mut read_phase = (state.phases[subvoice] + phase_modulation).rem_euclid(1.0);
            for stage in self.warps.iter().flatten() {
                read_phase = warp_phase(
                    read_phase,
                    stage.kind,
                    stage.amount.value(controls).clamp(-1.0, 1.0),
                );
            }
            let sample = self
                .table
                .sample(position, read_phase, frequency, sample_rate)
                * amplitude;
            if subvoice == 0 {
                base_sample = sample;
            }
            state.phases[subvoice] = (state.phases[subvoice] + increment).fract();
            state.drift_phases[subvoice] =
                (state.drift_phases[subvoice] + state.drift_rates[subvoice] / sample_rate).fract();
            let angle = (spread * width + 1.0) * PI * 0.25;
            output[0] += sample * power * angle.cos();
            output[1] += sample * power * angle.sin();
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

    pub(super) fn allocation_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>() as u64).saturating_add(self.table.allocation_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wavetable_retrigger_reseeds_and_resets_phase_at_the_next_read() {
        let mut state = WavetableState::new(41, 73);
        let first_seeds = state.phase_seeds;
        state.reset_phases(0.2, 0.75);
        let first_phases = state.phases;
        state.phases[0] = 0.91;
        state.last_sync = 1.0;

        state.retrigger(41);
        assert!(state.needs_phase_reset);
        assert_eq!(state.last_sync, 0.0);
        assert_eq!(state.phase_seeds, first_seeds);
        state.reset_phases(0.2, 0.75);
        assert_eq!(state.phases, first_phases);

        state.retrigger(42);
        assert_ne!(state.phase_seeds, first_seeds);
        assert!(state.needs_phase_reset);
    }
}
