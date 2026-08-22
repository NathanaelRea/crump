use super::*;

/// Envelope and gain state for one linked-stereo compressor.
#[derive(Debug, Clone)]
pub(super) struct CompressorState {
    envelope: f64,
    gain: f64,
    gain_reduction_db: f32,
}

impl CompressorState {
    pub(super) const fn new() -> Self {
        Self {
            envelope: 0.0,
            gain: 1.0,
            gain_reduction_db: 0.0,
        }
    }

    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        &mut self,
        sample: [f32; 2],
        sidechain: Option<[f32; 2]>,
        threshold: f64,
        ratio: f64,
        attack: f64,
        release: f64,
        knee: f64,
        makeup: f64,
        mix: f64,
        rms: bool,
        sample_rate: u32,
    ) -> [f32; 2] {
        let detector_sample = sidechain.unwrap_or(sample);
        let detector = if rms {
            ((f64::from(detector_sample[0]).powi(2) + f64::from(detector_sample[1]).powi(2)) * 0.5)
                .sqrt()
        } else {
            f64::from(detector_sample[0].abs().max(detector_sample[1].abs()))
        };
        let attack_coefficient = time_coefficient(attack, sample_rate);
        let release_coefficient = time_coefficient(release, sample_rate);
        let coefficient = if detector > self.envelope {
            attack_coefficient
        } else {
            release_coefficient
        };
        self.envelope = detector + coefficient * (self.envelope - detector);
        let level_db = 20.0 * self.envelope.max(1.0e-12).log10();
        let compressed_over = compressor_curve_reduction_db(level_db, threshold, ratio, knee);
        self.gain = f64::from(db_gain(-compressed_over));
        self.gain_reduction_db = compressed_over as f32;
        let mix = mix.clamp(0.0, 1.0) as f32;
        let wet_gain = self.gain as f32 * db_gain(makeup);
        [
            sample[0] + (sample[0] * wet_gain - sample[0]) * mix,
            sample[1] + (sample[1] * wet_gain - sample[1]) * mix,
        ]
    }

    pub(super) const fn gain_reduction_db(&self) -> f32 {
        self.gain_reduction_db
    }
}

pub(super) fn time_coefficient(seconds: f64, sample_rate: u32) -> f64 {
    (-1.0 / (seconds.max(1.0e-6) * f64::from(sample_rate))).exp()
}

pub(super) fn compressor_curve_reduction_db(
    level_db: f64,
    threshold: f64,
    ratio: f64,
    knee: f64,
) -> f64 {
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
