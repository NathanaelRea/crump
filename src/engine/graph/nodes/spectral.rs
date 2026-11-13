use crate::engine::{
    control::mix_modulation_identity,
    spectral::{CompiledSpectralAnalysis, MAX_SPECTRAL_PEAKS, SpectralPeak},
};

use super::Scalar;

pub(super) const fn spectral_state_bytes(max_peaks: usize) -> u64 {
    (std::mem::size_of::<SpectralState>() as u64)
        .saturating_add((max_peaks as u64).saturating_mul(std::mem::size_of::<[f64; 2]>() as u64))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SpectralTransientPolicy {
    Preserve,
    Reset,
    Suppress,
}

#[derive(Debug, Clone)]
pub(super) struct SpectralConfig {
    pub(super) analysis: CompiledSpectralAnalysis,
    pub(super) max_peaks: usize,
    pub(super) scan: Scalar,
    pub(super) rate: Scalar,
    pub(super) root_key: u8,
    pub(super) tune: Scalar,
    pub(super) low: Scalar,
    pub(super) high: Scalar,
    pub(super) transients: SpectralTransientPolicy,
    pub(super) phase_randomization: Scalar,
    pub(super) freeze: Scalar,
    pub(super) level: Scalar,
    pub(super) seed: u64,
    pub(super) node_identity: u64,
}

#[derive(Debug, Clone)]
pub(super) struct SpectralState {
    phases: Vec<[f64; 2]>,
    scan_phase: f64,
    last_frame: usize,
    runtime_seed: u64,
    initialized: bool,
}

impl SpectralState {
    pub(super) fn new(max_peaks: usize, runtime_seed: u64) -> Self {
        Self {
            phases: vec![[0.0; 2]; max_peaks],
            scan_phase: 0.0,
            last_frame: usize::MAX,
            runtime_seed,
            initialized: false,
        }
    }

    pub(super) fn retrigger(&mut self, runtime_seed: u64) {
        self.phases.fill([0.0; 2]);
        self.scan_phase = 0.0;
        self.last_frame = usize::MAX;
        self.runtime_seed = runtime_seed;
        self.initialized = false;
    }

    #[cfg(test)]
    pub(super) fn allocation_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>() as u64).saturating_add(
            (self.phases.capacity() as u64).saturating_mul(std::mem::size_of::<[f64; 2]>() as u64),
        )
    }
}

impl SpectralConfig {
    pub(super) fn process(
        &self,
        state: &mut SpectralState,
        pitch: f32,
        controls: &[f64],
        sample_rate: f64,
    ) -> [f32; 2] {
        let scan = self.scan.value(controls).clamp(0.0, 1.0);
        let scan_position = normalized_scan(scan, state.scan_phase);
        let frame_position = scan_position * self.analysis.frame_count().saturating_sub(1) as f64;
        let first_frame = frame_position.floor() as usize;
        let second_frame = (first_frame + 1).min(self.analysis.frame_count().saturating_sub(1));
        let fraction = frame_position - first_frame as f64;
        let selected_frame = if fraction < 0.5 {
            first_frame
        } else {
            second_frame
        };
        let phase_randomization = self.phase_randomization.value(controls).clamp(0.0, 1.0);
        if !state.initialized
            || (self.transients == SpectralTransientPolicy::Reset
                && selected_frame != state.last_frame
                && self.analysis.transient(selected_frame))
        {
            self.reset_phases(state, selected_frame, phase_randomization);
            state.initialized = true;
        }
        state.last_frame = selected_frame;

        let low = self.low.value(controls).clamp(20.0, sample_rate * 0.45);
        let high = self.high.value(controls).clamp(20.0, sample_rate * 0.45);
        let root_hz = midi_frequency(self.root_key);
        let pitch_ratio = f64::from(pitch.max(1.0)) / root_hz;
        let tune_ratio = 2.0_f64.powf(self.tune.value(controls).clamp(-48.0, 48.0) / 12.0);
        let frequency_ratio = pitch_ratio * tune_ratio;
        let transient_gain = if self.transients == SpectralTransientPolicy::Suppress
            && self.analysis.transient(selected_frame)
        {
            0.25
        } else {
            1.0
        };
        let mut output = [0.0_f64; 2];
        let mut active = 0_u32;
        for (slot, phase) in state
            .phases
            .iter_mut()
            .enumerate()
            .take(self.max_peaks.min(MAX_SPECTRAL_PEAKS))
        {
            let first = self.analysis.peak(first_frame, slot);
            let second = self.analysis.peak(second_frame, slot);
            let frequency = interpolate(
                f64::from(first.frequency_hz),
                f64::from(second.frequency_hz),
                fraction,
            );
            if frequency < low || frequency > high {
                continue;
            }
            let output_frequency = frequency * frequency_ratio;
            if !(0.0..sample_rate * 0.5).contains(&output_frequency) {
                continue;
            }
            let left_amplitude = interpolate(
                f64::from(first.left_amplitude),
                f64::from(second.left_amplitude),
                fraction,
            );
            let right_amplitude = interpolate(
                f64::from(first.right_amplitude),
                f64::from(second.right_amplitude),
                fraction,
            );
            if left_amplitude <= 0.0 && right_amplitude <= 0.0 {
                continue;
            }
            active = active.saturating_add(1);
            output[0] += phase[0].cos() * left_amplitude * transient_gain;
            output[1] += phase[1].cos() * right_amplitude * transient_gain;
            let increment = std::f64::consts::TAU * output_frequency / sample_rate;
            phase[0] = (phase[0] + increment).rem_euclid(std::f64::consts::TAU);
            phase[1] = (phase[1] + increment).rem_euclid(std::f64::consts::TAU);
        }
        if self.freeze.value(controls) < 0.5 {
            let rate = self.rate.value(controls).clamp(-4.0, 4.0);
            state.scan_phase = (state.scan_phase
                + rate / self.analysis.source_frames().max(1) as f64)
                .rem_euclid(1.0);
        }
        let level = self.level.value(controls).clamp(0.0, 4.0) / f64::from(active.max(1)).sqrt();
        [finite_f32(output[0] * level), finite_f32(output[1] * level)]
    }

    fn reset_phases(&self, state: &mut SpectralState, frame: usize, phase_randomization: f64) {
        for slot in 0..self.max_peaks.min(state.phases.len()) {
            let peak = self.analysis.peak(frame, slot);
            state.phases[slot][0] = randomized_phase(
                f64::from(peak.left_phase),
                self.phase_identity(state, slot, 0),
                phase_randomization,
            );
            state.phases[slot][1] = randomized_phase(
                f64::from(peak.right_phase),
                self.phase_identity(state, slot, 1),
                phase_randomization,
            );
        }
    }

    fn phase_identity(&self, state: &SpectralState, slot: usize, channel: u64) -> u64 {
        mix_modulation_identity(
            self.seed,
            mix_modulation_identity(
                self.node_identity,
                mix_modulation_identity(
                    state.runtime_seed,
                    (slot as u64).wrapping_mul(2).wrapping_add(channel),
                ),
            ),
        )
    }

    pub(super) fn transition_compatible(&self, replacement: &Self) -> bool {
        self.analysis.cache_identity() == replacement.analysis.cache_identity()
            && self.max_peaks == replacement.max_peaks
            && self.root_key == replacement.root_key
            && self.transients == replacement.transients
            && self.seed == replacement.seed
            && self.node_identity == replacement.node_identity
    }

    pub(super) fn allocation_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>() as u64)
            .saturating_add(self.analysis.asset_id().len() as u64)
            .saturating_add(self.analysis.source_sha256().len() as u64)
    }
}

fn normalized_scan(base: f64, phase: f64) -> f64 {
    let value = base + phase;
    if value <= 1.0 {
        value.max(0.0)
    } else {
        value.rem_euclid(1.0)
    }
}

fn interpolate(first: f64, second: f64, fraction: f64) -> f64 {
    first + (second - first) * fraction
}

fn randomized_phase(authored: f64, identity: u64, amount: f64) -> f64 {
    let random = (identity >> 11) as f64 * (1.0 / ((1_u64 << 53) as f64));
    (authored + (random * 2.0 - 1.0) * std::f64::consts::PI * amount)
        .rem_euclid(std::f64::consts::TAU)
}

fn finite_f32(value: f64) -> f32 {
    if value.is_finite() {
        value.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32
    } else {
        0.0
    }
}

fn midi_frequency(pitch: u8) -> f64 {
    440.0 * 2.0_f64.powf((f64::from(pitch) - 69.0) / 12.0)
}

#[allow(dead_code)]
fn _peak_type_is_fixed(_: SpectralPeak) {}
