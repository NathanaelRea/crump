//! Shared deterministic LFO waveform law for Song, Instrument, and Graph modulators.

use std::f64::consts::TAU;

use crate::model::{LfoDirection, LfoMode, LfoWaveform};

pub(crate) const MAX_CUSTOM_LFO_POINTS: usize = 64;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LfoPoint {
    pub(crate) phase: f64,
    pub(crate) value: f64,
}

pub(crate) fn position(
    initial_phase: f64,
    elapsed_cycles: f64,
    mode: LfoMode,
    direction: LfoDirection,
) -> (f64, u64) {
    let initial_phase = initial_phase.clamp(0.0, 1.0);
    match direction {
        LfoDirection::Forward => {
            let position = initial_phase + elapsed_cycles.max(0.0);
            if mode == LfoMode::OneShot && position >= 1.0 {
                (1.0 - f64::EPSILON, 1)
            } else {
                (position.fract(), position.floor().max(0.0) as u64)
            }
        }
        LfoDirection::Reverse => {
            let initial_phase = if initial_phase == 0.0 {
                1.0
            } else {
                initial_phase
            };
            let position = initial_phase - elapsed_cycles.max(0.0);
            if mode == LfoMode::OneShot && position <= 0.0 {
                (0.0, 1)
            } else {
                (position.rem_euclid(1.0), (-position).ceil().max(0.0) as u64)
            }
        }
        LfoDirection::PingPong => {
            let position = initial_phase + elapsed_cycles.max(0.0);
            if mode == LfoMode::OneShot && position >= 2.0 {
                return (0.0, 2);
            }
            let wrapped = if mode == LfoMode::Loop {
                position.rem_euclid(2.0)
            } else {
                position
            };
            let phase = if wrapped <= 1.0 {
                wrapped
            } else {
                2.0 - wrapped
            };
            (phase.clamp(0.0, 1.0), position.floor().max(0.0) as u64)
        }
    }
}

pub(crate) fn custom_bipolar_sample(points: &[LfoPoint], phase: f64) -> f64 {
    let Some(first) = points.first().copied() else {
        return 0.0;
    };
    let phase = phase.clamp(0.0, 1.0);
    if phase <= first.phase {
        return first.value.clamp(-1.0, 1.0);
    }
    for pair in points.windows(2) {
        let left = pair[0];
        let right = pair[1];
        if phase <= right.phase {
            let span = (right.phase - left.phase).max(f64::EPSILON);
            let progress = ((phase - left.phase) / span).clamp(0.0, 1.0);
            return (left.value + (right.value - left.value) * progress).clamp(-1.0, 1.0);
        }
    }
    points
        .last()
        .map_or(0.0, |point| point.value.clamp(-1.0, 1.0))
}

pub(crate) fn swing_phase(phase: f64, swing: f64) -> f64 {
    let phase = phase.clamp(0.0, 1.0);
    let split = 0.5 + swing.clamp(-1.0, 1.0) * 0.24;
    if phase <= split {
        phase * 0.5 / split
    } else {
        0.5 + (phase - split) * 0.5 / (1.0 - split)
    }
}

pub(crate) fn bipolar_sample(waveform: LfoWaveform, phase: f64, cycle: u64, identity: u64) -> f64 {
    let phase = phase.rem_euclid(1.0);
    match waveform {
        LfoWaveform::Sine => (phase * TAU).sin(),
        LfoWaveform::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
        LfoWaveform::Saw => phase * 2.0 - 1.0,
        LfoWaveform::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        LfoWaveform::SampleHold => signed_random(mix(identity, cycle)),
    }
}

const fn mix(seed: u64, value: u64) -> u64 {
    let mut mixed = seed ^ value.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    mixed ^= mixed >> 30;
    mixed = mixed.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    mixed ^= mixed >> 27;
    mixed = mixed.wrapping_mul(0x94d0_49bb_1331_11eb);
    mixed ^ (mixed >> 31)
}

fn signed_random(value: u64) -> f64 {
    let mantissa = value >> 11;
    (mantissa as f64 / (1_u64 << 53) as f64) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_have_stable_boundaries_and_sample_hold_identity() {
        assert_eq!(bipolar_sample(LfoWaveform::Saw, 0.0, 0, 0), -1.0);
        assert_eq!(bipolar_sample(LfoWaveform::Square, 0.0, 0, 0), 1.0);
        assert_eq!(bipolar_sample(LfoWaveform::Square, 0.5, 0, 0), -1.0);
        let held = bipolar_sample(LfoWaveform::SampleHold, 0.1, 7, 19);
        assert_eq!(held, bipolar_sample(LfoWaveform::SampleHold, 0.9, 7, 19));
        assert_ne!(held, bipolar_sample(LfoWaveform::SampleHold, 0.1, 8, 19));
    }

    #[test]
    fn bounded_custom_shape_interpolation_and_swing_are_exact() {
        let points = [
            LfoPoint {
                phase: 0.0,
                value: -1.0,
            },
            LfoPoint {
                phase: 0.5,
                value: 1.0,
            },
            LfoPoint {
                phase: 1.0,
                value: -1.0,
            },
        ];
        assert_eq!(custom_bipolar_sample(&points, 0.0), -1.0);
        assert_eq!(custom_bipolar_sample(&points, 0.25), 0.0);
        assert_eq!(custom_bipolar_sample(&points, 0.5), 1.0);
        assert_eq!(custom_bipolar_sample(&points, 1.0), -1.0);
        assert_eq!(swing_phase(0.5, 0.0), 0.5);
        assert!(swing_phase(0.5, 1.0) < 0.5);
        assert!(swing_phase(0.5, -1.0) > 0.5);
    }
}
