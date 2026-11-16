//! One fixed filtered 4× oversampling kernel shared by nonlinear processors.
//!
//! The 129-tap linear-phase Blackman-windowed sinc is normalized at DC. Its
//! cutoff is the base-rate Nyquist frequency at the 4× rate. Polyphase
//! interpolation avoids inserting explicit zero samples; decimation uses the
//! same coefficients so the round trip has an integer 32-frame base-rate
//! latency. All histories are fixed-size inline state.

/// Fixed oversampling ratio used by native nonlinear and true-peak processing.
pub(crate) const OVERSAMPLING_FACTOR: u8 = 4;
/// Number of taps in the shared interpolation/anti-alias filter.
pub(crate) const OVERSAMPLING_FILTER_TAPS: u16 = 129;
/// Base-rate latency of one interpolation pass.
pub(crate) const OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES: usize = 16;
/// Base-rate latency of an interpolation/decimation round trip.
pub(crate) const OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES: usize = 32;

const FACTOR: usize = OVERSAMPLING_FACTOR as usize;
const TAPS: usize = OVERSAMPLING_FILTER_TAPS as usize;
const PHASE_HISTORY: usize = TAPS.div_ceil(FACTOR);

#[rustfmt::skip]
const FILTER: [f32; TAPS] = [
     1.352_438_3e-34, -7.754_444e-7, -4.468_924_5e-6, -7.257_711e-6,
     1.005_819_3e-19,  2.112_701_4e-5,  4.416_457e-5,  4.370_762_3e-5,
    -1.425_624_7e-19, -7.675_873e-5, -1.384_340_4e-4, -1.225_075e-4,
    -4.186_229e-19,  1.836_762_1e-4,  3.125_903e-4,  2.635_047e-4,
    -6.475_451e-19, -3.657_702e-4, -6.033_728e-4, -4.948_074_3e-4,
     3.955_845e-18,  6.555_339e-4,  1.059_932_5e-3,  8.534_869e-4,
    -1.677_073_8e-18, -1.094_880_4e-3, -1.745_161_5e-3, -1.386_695_3e-3,
     2.422_151_7e-18,  1.736_820_7e-3,  2.738_645_3e-3,  2.154_270_4e-3,
    -3.313_417_3e-18, -2.649_514e-3, -4.143_783e-3, -3.234_979_2e-3,
     4.323_374e-18,  3.925_815e-3,  6.104_454_4e-3,  4.741_061_5e-3,
    -5.406_455_8e-18, -5.705_471e-3, -8.843_914e-3, -6.852_415_4e-3,
     6.501_806e-18,  8.228_664e-3,  1.276_039_1e-2,  9.902_574e-3,
    -7.538_544_4e-18, -1.197_885_8e-2, -1.869_173_2e-2, -1.462_701e-2,
     8.442_874_5e-18,  1.814_072_6e-2,  2.881_972_5e-2,  2.307_656e-2,
    -9.146_087e-18, -3.063_019_6e-2, -5.119_374_4e-2, -4.391_566_7e-2,
     9.592_372e-18,  7.436_124e-2,  1.585_257_4e-1,  2.248_548_6e-1,
     2.499_978_2e-1,
     2.248_548_6e-1,  1.585_257_4e-1,  7.436_124e-2,  9.592_372e-18,
    -4.391_566_7e-2, -5.119_374_4e-2, -3.063_019_6e-2, -9.146_087e-18,
     2.307_656e-2,  2.881_972_5e-2,  1.814_072_6e-2,  8.442_874_5e-18,
    -1.462_701e-2, -1.869_173_2e-2, -1.197_885_8e-2, -7.538_544_4e-18,
     9.902_574e-3,  1.276_039_1e-2,  8.228_664e-3,  6.501_806e-18,
    -6.852_415_4e-3, -8.843_914e-3, -5.705_471e-3, -5.406_455_8e-18,
     4.741_061_5e-3,  6.104_454_4e-3,  3.925_815e-3,  4.323_374e-18,
    -3.234_979_2e-3, -4.143_783e-3, -2.649_514e-3, -3.313_417_3e-18,
     2.154_270_4e-3,  2.738_645_3e-3,  1.736_820_7e-3,  2.422_151_7e-18,
    -1.386_695_3e-3, -1.745_161_5e-3, -1.094_880_4e-3, -1.677_073_8e-18,
     8.534_869e-4,  1.059_932_5e-3,  6.555_339e-4,  3.955_845e-18,
    -4.948_074_3e-4, -6.033_728e-4, -3.657_702e-4, -6.475_451e-19,
     2.635_047e-4,  3.125_903e-4,  1.836_762_1e-4, -4.186_229e-19,
    -1.225_075e-4, -1.384_340_4e-4, -7.675_873e-5, -1.425_624_7e-19,
     4.370_762_3e-5,  4.416_457e-5,  2.112_701_4e-5,  1.005_819_3e-19,
    -7.257_711e-6, -4.468_924_5e-6, -7.754_444e-7,  1.352_438_3e-34,
];

/// Stateful polyphase reconstruction filter used directly by true-peak detection.
#[derive(Debug, Clone)]
pub(crate) struct OversamplingInterpolator {
    history: [[f32; 2]; PHASE_HISTORY],
    cursor: usize,
}

impl Default for OversamplingInterpolator {
    fn default() -> Self {
        Self {
            history: [[0.0; 2]; PHASE_HISTORY],
            cursor: 0,
        }
    }
}

impl OversamplingInterpolator {
    /// Clears the fixed history without allocating or replacing storage.
    pub(crate) fn reset(&mut self) {
        self.history.fill([0.0; 2]);
        self.cursor = 0;
    }

    /// Reconstructs four filtered high-rate samples from one stereo base-rate sample.
    pub(crate) fn process(&mut self, sample: [f32; 2]) -> [[f32; 2]; FACTOR] {
        self.cursor = (self.cursor + PHASE_HISTORY - 1) % PHASE_HISTORY;
        self.history[self.cursor] = sample;
        std::array::from_fn(|phase| {
            let mut output = [0.0_f32; 2];
            let mut tap = phase;
            let mut age = 0_usize;
            while tap < TAPS {
                let coefficient = FILTER[tap] * f32::from(OVERSAMPLING_FACTOR);
                let delayed = self.history[(self.cursor + age) % PHASE_HISTORY];
                output[0] += delayed[0] * coefficient;
                output[1] += delayed[1] * coefficient;
                tap += FACTOR;
                age += 1;
            }
            output
        })
    }
}

#[derive(Debug, Clone)]
struct OversamplingDecimator {
    history: [[f32; 2]; TAPS],
    cursor: usize,
}

impl Default for OversamplingDecimator {
    fn default() -> Self {
        Self {
            history: [[0.0; 2]; TAPS],
            cursor: 0,
        }
    }
}

impl OversamplingDecimator {
    fn push(&mut self, sample: [f32; 2]) {
        self.cursor = (self.cursor + TAPS - 1) % TAPS;
        self.history[self.cursor] = sample;
    }

    fn output(&self) -> [f32; 2] {
        let mut output = [0.0_f32; 2];
        for (tap, coefficient) in FILTER.into_iter().enumerate() {
            let delayed = self.history[(self.cursor + tap) % TAPS];
            output[0] += delayed[0] * coefficient;
            output[1] += delayed[1] * coefficient;
        }
        output
    }
}

/// Complete filtered interpolation/nonlinearity/decimation path.
#[derive(Debug, Clone, Default)]
pub(crate) struct FilteredOversampler {
    interpolator: OversamplingInterpolator,
    decimator: OversamplingDecimator,
}

impl FilteredOversampler {
    /// Clears both fixed filter histories without allocating.
    pub(crate) fn reset(&mut self) {
        self.interpolator.reset();
        self.decimator.history.fill([0.0; 2]);
        self.decimator.cursor = 0;
    }

    /// Runs one base-rate stereo sample through a callback at each filtered 4× phase.
    pub(crate) fn process(
        &mut self,
        sample: [f32; 2],
        mut nonlinear: impl FnMut([f32; 2]) -> [f32; 2],
    ) -> [f32; 2] {
        let high_rate = self.interpolator.process(sample);
        let mut output = [0.0_f32; 2];
        for (phase, sample) in high_rate.into_iter().enumerate() {
            self.decimator.push(nonlinear(sample));
            if phase == 0 {
                output = self.decimator.output();
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtered_round_trip_has_unity_dc_and_declared_integer_latency() {
        let mut dc = FilteredOversampler::default();
        let mut output = [0.0; 2];
        for _ in 0..2_000 {
            output = dc.process([1.0, 1.0], |sample| sample);
        }
        assert!((output[0] - 1.0).abs() < 1.0e-5);
        assert!((output[1] - 1.0).abs() < 1.0e-5);

        let mut impulse = FilteredOversampler::default();
        let response = (0..96)
            .map(|frame| {
                impulse.process(if frame == 0 { [1.0, 1.0] } else { [0.0, 0.0] }, |sample| {
                    sample
                })[0]
            })
            .collect::<Vec<_>>();
        let peak = response
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left.abs().total_cmp(&right.abs()))
            .map(|(index, _)| index)
            .expect("round-trip impulse peak");
        assert_eq!(peak, OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES);
    }

    #[test]
    fn interpolator_reconstructs_every_dc_phase_at_unity() {
        let mut interpolator = OversamplingInterpolator::default();
        let mut output = [[0.0; 2]; FACTOR];
        for _ in 0..1_000 {
            output = interpolator.process([1.0, 1.0]);
        }
        for phase in output {
            assert!((phase[0] - 1.0).abs() < 1.0e-5, "{phase:?}");
            assert!((phase[1] - 1.0).abs() < 1.0e-5, "{phase:?}");
        }
    }
}
