use std::f64::consts::PI;

use super::Scalar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FilterMode {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FilterConfig {
    pub(super) mode: FilterMode,
    pub(super) cutoff: Scalar,
    pub(super) resonance: Scalar,
    pub(super) mod_depth: Scalar,
    pub(super) velocity_depth: Scalar,
    pub(super) key_track: Scalar,
    pub(super) key_reference: Scalar,
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::engine::graph) struct FilterState {
    integrator_one: f64,
    integrator_two: f64,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::engine::graph) struct FilterOutputs {
    pub(in crate::engine::graph) lowpass: f64,
    pub(in crate::engine::graph) highpass: f64,
    pub(in crate::engine::graph) bandpass: f64,
    pub(in crate::engine::graph) notch: f64,
}

impl FilterState {
    pub(in crate::engine::graph) fn process(
        &mut self,
        input: f64,
        cutoff: f64,
        resonance: f64,
        sample_rate: f64,
    ) -> FilterOutputs {
        let g = (PI * cutoff / sample_rate).tan().min(10.0);
        let damping = 2.0 - 1.9 * resonance.clamp(0.0, 1.0);
        let denominator = 1.0 + g * (g + damping);
        let bandpass = (self.integrator_one + g * (input - self.integrator_two)) / denominator;
        let lowpass = self.integrator_two + g * bandpass;
        let highpass = input - damping * bandpass - lowpass;
        self.integrator_one = 2.0 * bandpass - self.integrator_one;
        self.integrator_two = 2.0 * lowpass - self.integrator_two;
        FilterOutputs {
            lowpass,
            highpass,
            bandpass,
            notch: lowpass + highpass,
        }
    }
}

impl FilterConfig {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        self,
        state: &mut FilterState,
        audio: f32,
        key: f32,
        modulation: f32,
        velocity: f32,
        has_key: bool,
        has_modulation: bool,
        has_velocity: bool,
        controls: &[f64],
        sample_rate: f64,
    ) -> f32 {
        let key_multiplier = if has_key {
            let key = key as f64;
            let reference = self.key_reference.value(controls);
            if key > 0.0 && reference > 0.0 {
                2.0_f64.powf((key / reference).log2() * self.key_track.value(controls))
            } else {
                1.0
            }
        } else {
            1.0
        };
        let modulation = if has_modulation {
            modulation as f64
        } else {
            0.0
        };
        let velocity = if has_velocity { velocity as f64 } else { 0.0 };
        let cutoff = (self.cutoff.value(controls) * key_multiplier
            + modulation * self.mod_depth.value(controls)
            + velocity * self.velocity_depth.value(controls))
        .clamp(10.0, sample_rate * 0.45);
        let outputs = state.process(
            audio as f64,
            cutoff,
            self.resonance.value(controls),
            sample_rate,
        );
        let output = match self.mode {
            FilterMode::Lowpass => outputs.lowpass,
            FilterMode::Highpass => outputs.highpass,
            FilterMode::Bandpass => outputs.bandpass,
            FilterMode::Notch => outputs.notch,
        };
        output as f32
    }
}
