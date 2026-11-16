#[derive(Debug, Clone)]
pub(super) struct CrusherState {
    phase: f64,
    current: [f32; 2],
    start: [f32; 2],
    target: [f32; 2],
    ramp_cursor: u32,
    ramp_frames: u32,
    initialized: bool,
}

impl CrusherState {
    pub(super) const fn new() -> Self {
        Self {
            phase: 0.0,
            current: [0.0; 2],
            start: [0.0; 2],
            target: [0.0; 2],
            ramp_cursor: 0,
            ramp_frames: 0,
            initialized: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        &mut self,
        input: [f32; 2],
        bit_depth: f64,
        target_rate_hz: f64,
        reconstruction: f64,
        mix: f64,
        output_db: f64,
        sample_rate: u32,
    ) -> [f32; 2] {
        let sample_rate = f64::from(sample_rate.max(1));
        let target_rate = target_rate_hz.clamp(100.0, sample_rate * 0.49);
        let capture = if self.initialized {
            self.phase += target_rate / sample_rate;
            self.phase >= 1.0
        } else {
            true
        };
        if capture {
            self.initialized = true;
            self.phase = self.phase.fract();
            self.start = self.current;
            self.target = [quantize(input[0], bit_depth), quantize(input[1], bit_depth)];
            let period = (sample_rate / target_rate).ceil().max(1.0);
            self.ramp_frames = (period * reconstruction.clamp(0.0, 1.0)).round() as u32;
            self.ramp_cursor = 0;
            if self.ramp_frames == 0 {
                self.current = self.target;
            }
        }
        if self.ramp_frames > 0 && self.ramp_cursor < self.ramp_frames {
            self.ramp_cursor += 1;
            let progress = self.ramp_cursor as f32 / self.ramp_frames as f32;
            self.current = [
                self.start[0] + (self.target[0] - self.start[0]) * progress,
                self.start[1] + (self.target[1] - self.start[1]) * progress,
            ];
        }
        let mix = mix.clamp(0.0, 1.0) as f32;
        let gain = 10.0_f32.powf((output_db.clamp(-24.0, 24.0) / 20.0) as f32);
        [
            (input[0] + (self.current[0] - input[0]) * mix) * gain,
            (input[1] + (self.current[1] - input[1]) * mix) * gain,
        ]
    }

    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }
}

fn quantize(sample: f32, bits: f64) -> f32 {
    let levels = 2.0_f64.powf(bits.clamp(1.0, 24.0) - 1.0);
    (f64::from(sample.clamp(-1.0, 1.0)) * levels).round() as f32 / levels as f32
}

#[cfg(test)]
mod tests {
    use super::{CrusherState, quantize};

    #[test]
    fn quantizer_is_symmetric_bounded_and_contains_zero() {
        for bits in [1.0, 4.0, 8.0, 16.0, 24.0] {
            assert_eq!(quantize(0.0, bits), 0.0);
            assert_eq!(quantize(1.5, bits), 1.0);
            assert_eq!(quantize(-1.5, bits), -1.0);
            for value in [0.1, 0.25, 0.73] {
                assert_eq!(quantize(value, bits), -quantize(-value, bits));
            }
        }
    }

    #[test]
    fn capture_hold_and_reconstruction_are_bounded() {
        let mut hold = CrusherState::new();
        let output = (0..9)
            .map(|frame| {
                hold.process(
                    [frame as f32 / 10.0; 2],
                    24.0,
                    12_000.0,
                    0.0,
                    1.0,
                    0.0,
                    48_000,
                )[0]
            })
            .collect::<Vec<_>>();
        assert!(output[0..4].windows(2).all(|pair| pair[0] == pair[1]));
        assert!(output[4..8].windows(2).all(|pair| pair[0] == pair[1]));
        assert_ne!(output[3], output[4]);

        let mut reconstructed = CrusherState::new();
        let first = reconstructed.process([1.0; 2], 8.0, 12_000.0, 1.0, 1.0, 0.0, 48_000)[0];
        let second = reconstructed.process([0.0; 2], 8.0, 12_000.0, 1.0, 1.0, 0.0, 48_000)[0];
        let third = reconstructed.process([0.0; 2], 8.0, 12_000.0, 1.0, 1.0, 0.0, 48_000)[0];
        assert!(0.0 < first && first < second && second < third && third <= 1.0);

        for _ in 0..960 {
            reconstructed.process([0.0; 2], 8.0, 100.0, 1.0, 1.0, 0.0, 48_000);
        }
        assert!(reconstructed.current[0].abs() < 1.0e-6);
    }
}
