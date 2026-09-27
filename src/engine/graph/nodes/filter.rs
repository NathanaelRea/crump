use std::f64::consts::PI;

use super::Scalar;
pub(in crate::engine::graph) use crate::dsp::StateVariableFilter as FilterState;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StereoFilterModel {
    SvfClean,
    LadderDrive,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct StereoFilterConfig {
    pub(super) model: StereoFilterModel,
    pub(super) mode: FilterMode,
    pub(super) cutoff: Scalar,
    pub(super) resonance: Scalar,
    pub(super) drive: Scalar,
    pub(super) mix: Scalar,
    pub(super) pan: Scalar,
    pub(super) mod_depth: Scalar,
    pub(super) velocity_depth: Scalar,
    pub(super) key_track: Scalar,
    pub(super) key_reference: Scalar,
}

#[derive(Debug, Clone, Copy, Default)]
struct LadderState {
    stages: [f64; 4],
    previous_input: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct StereoFilterChannelState {
    svf: FilterState,
    ladder: LadderState,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct StereoFilterState {
    left: StereoFilterChannelState,
    right: StereoFilterChannelState,
    smoothed_cutoff: f64,
    smoothed_resonance: f64,
    smoothed_drive: f64,
    smoothing_initialized: bool,
}

impl LadderState {
    fn process(
        &mut self,
        input: f64,
        cutoff: f64,
        resonance: f64,
        drive: f64,
        mode: FilterMode,
        sample_rate: f64,
    ) -> f64 {
        let drive_gain = 1.0 + drive.clamp(0.0, 1.0) * 15.0;
        let normalizer = drive_gain.tanh().max(0.000_001);
        let oversampled_rate = sample_rate * 2.0;
        let coefficient = (1.0 - (-2.0 * PI * cutoff / oversampled_rate).exp()).clamp(0.0, 1.0);
        let midpoint = (self.previous_input + input) * 0.5;
        for oversampled_input in [midpoint, input] {
            let feedback = self.stages[3] * resonance.clamp(0.0, 1.0) * 3.8;
            let driven = ((oversampled_input - feedback) * drive_gain).tanh() / normalizer;
            let mut stage_input = driven;
            for stage in &mut self.stages {
                *stage += coefficient * (stage_input - *stage);
                stage_input = stage.tanh();
            }
        }
        self.previous_input = input;
        let [pole_one, pole_two, pole_three, lowpass] = self.stages;
        // Binomial combinations of the four one-pole stages provide deterministic
        // complementary multimode responses without a hidden parallel filter state.
        let highpass = input - 4.0 * pole_one + 6.0 * pole_two - 4.0 * pole_three + lowpass;
        let bandpass = pole_two - lowpass;
        match mode {
            FilterMode::Lowpass => lowpass,
            FilterMode::Highpass => highpass,
            FilterMode::Bandpass => bandpass,
            FilterMode::Notch => lowpass + highpass,
        }
    }
}

impl StereoFilterConfig {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        self,
        state: &mut StereoFilterState,
        audio: [f32; 2],
        key: f32,
        modulation: f32,
        velocity: f32,
        has_key: bool,
        has_modulation: bool,
        has_velocity: bool,
        controls: &[f64],
        sample_rate: f64,
    ) -> [f32; 2] {
        let key_multiplier = if has_key {
            let key = f64::from(key);
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
            f64::from(modulation)
        } else {
            0.0
        };
        let velocity = if has_velocity {
            f64::from(velocity)
        } else {
            0.0
        };
        let target_cutoff = (self.cutoff.value(controls) * key_multiplier
            + modulation * self.mod_depth.value(controls)
            + velocity * self.velocity_depth.value(controls))
        .clamp(10.0, sample_rate * 0.45);
        let target_resonance = self.resonance.value(controls).clamp(0.0, 1.0);
        let target_drive = self.drive.value(controls).clamp(0.0, 1.0);
        if !state.smoothing_initialized {
            state.smoothed_cutoff = target_cutoff;
            state.smoothed_resonance = target_resonance;
            state.smoothed_drive = target_drive;
            state.smoothing_initialized = true;
        } else {
            let smoothing = 1.0 - (-1.0 / (sample_rate * 0.001)).exp();
            state.smoothed_cutoff += smoothing * (target_cutoff - state.smoothed_cutoff);
            state.smoothed_resonance += smoothing * (target_resonance - state.smoothed_resonance);
            state.smoothed_drive += smoothing * (target_drive - state.smoothed_drive);
        }
        let cutoff = state.smoothed_cutoff;
        let resonance = state.smoothed_resonance;
        let drive = state.smoothed_drive;
        let process_channel = |channel: &mut StereoFilterChannelState, input: f64| match self.model
        {
            StereoFilterModel::SvfClean => {
                let driven = if drive <= 0.0 {
                    input
                } else {
                    let gain = 1.0 + drive * 15.0;
                    (input * gain).tanh() / gain.tanh().max(0.000_001)
                };
                let outputs = channel.svf.process(driven, cutoff, resonance, sample_rate);
                match self.mode {
                    FilterMode::Lowpass => outputs.lowpass,
                    FilterMode::Highpass => outputs.highpass,
                    FilterMode::Bandpass => outputs.bandpass,
                    FilterMode::Notch => outputs.notch,
                }
            }
            StereoFilterModel::LadderDrive => {
                channel
                    .ladder
                    .process(input, cutoff, resonance, drive, self.mode, sample_rate)
            }
        };
        let wet = [
            process_channel(&mut state.left, f64::from(audio[0])),
            process_channel(&mut state.right, f64::from(audio[1])),
        ];
        let mix = self.mix.value(controls).clamp(0.0, 1.0);
        let mut output = [
            f64::from(audio[0]) * (1.0 - mix) + wet[0] * mix,
            f64::from(audio[1]) * (1.0 - mix) + wet[1] * mix,
        ];
        let pan = self.pan.value(controls).clamp(-1.0, 1.0);
        if pan > 0.0 {
            output[0] *= (pan * PI * 0.5).cos();
        } else if pan < 0.0 {
            output[1] *= (-pan * PI * 0.5).cos();
        }
        [output[0] as f32, output[1] as f32]
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

#[cfg(test)]
#[path = "filter_tests.rs"]
mod tests;
