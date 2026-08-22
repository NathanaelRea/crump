use std::f64::consts::PI;

use super::*;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct StereoBiquad {
    pub(super) z1: [f64; 2],
    pub(super) z2: [f64; 2],
}

impl StereoBiquad {
    pub(super) fn process(
        &mut self,
        sample: [f32; 2],
        coefficients: BiquadCoefficients,
    ) -> [f32; 2] {
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
pub(super) struct BiquadCoefficients {
    pub(super) b0: f64,
    pub(super) b1: f64,
    pub(super) b2: f64,
    pub(super) a1: f64,
    pub(super) a2: f64,
}

impl BiquadCoefficients {
    pub(super) fn for_band(
        kind: EqBandKind,
        frequency: f64,
        gain_db: f64,
        q: f64,
        sample_rate: u32,
    ) -> Self {
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
