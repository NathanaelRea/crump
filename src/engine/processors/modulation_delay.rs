use super::ModulationDelayMode;

#[derive(Debug, Clone)]
pub(super) struct ModulationDelayState {
    pub(super) buffer: Vec<[f32; 2]>,
    cursor: usize,
    phase: f64,
}

impl ModulationDelayState {
    pub(super) fn new(maximum_frames: usize) -> Self {
        Self {
            buffer: vec![[0.0; 2]; maximum_frames.max(4)],
            cursor: 0,
            phase: 0.0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        &mut self,
        input: [f32; 2],
        delay_seconds: f64,
        depth_seconds: f64,
        rate_hz: f64,
        feedback: f64,
        mix: f64,
        width: f64,
        mode: ModulationDelayMode,
        sample_rate: u32,
    ) -> [f32; 2] {
        let sample_rate = f64::from(sample_rate.max(1));
        let maximum_delay = (self.buffer.len().saturating_sub(2)) as f64;
        let base_frames = (delay_seconds * sample_rate).clamp(1.0, maximum_delay);
        let depth_frames = (depth_seconds * sample_rate).clamp(0.0, maximum_delay);
        let width = width.clamp(0.0, 1.0);
        let left_phase = self.phase;
        let right_phase = (self.phase + 0.5 * width).fract();
        let mut wet = [0.0_f32; 2];
        for (channel, phase) in [left_phase, right_phase].into_iter().enumerate() {
            let first_delay = modulated_delay(base_frames, depth_frames, phase, maximum_delay);
            let first = self.read(channel, first_delay);
            wet[channel] = match mode {
                ModulationDelayMode::Flanger => first,
                ModulationDelayMode::Chorus => {
                    let second_delay = modulated_delay(
                        base_frames,
                        depth_frames,
                        (phase + 0.5).fract(),
                        maximum_delay,
                    );
                    0.5 * (first + self.read(channel, second_delay))
                }
            };
        }
        let feedback = feedback.clamp(-0.95, 0.95) as f32;
        self.buffer[self.cursor] = [
            bounded(input[0] + wet[0] * feedback),
            bounded(input[1] + wet[1] * feedback),
        ];
        self.cursor = (self.cursor + 1) % self.buffer.len();
        self.phase = (self.phase + rate_hz.clamp(0.01, 20.0) / sample_rate).fract();
        let mix = mix.clamp(0.0, 1.0) as f32;
        [
            input[0] + (wet[0] - input[0]) * mix,
            input[1] + (wet[1] - input[1]) * mix,
        ]
    }

    pub(super) fn reset(&mut self) {
        self.buffer.fill([0.0; 2]);
        self.cursor = 0;
        self.phase = 0.0;
    }

    fn read(&self, channel: usize, delay_frames: f64) -> f32 {
        let length = self.buffer.len() as f64;
        let position = (self.cursor as f64 - delay_frames).rem_euclid(length);
        let left = position.floor() as usize % self.buffer.len();
        let right = (left + 1) % self.buffer.len();
        let fraction = (position - position.floor()) as f32;
        self.buffer[left][channel]
            + (self.buffer[right][channel] - self.buffer[left][channel]) * fraction
    }
}

fn modulated_delay(base: f64, depth: f64, phase: f64, maximum: f64) -> f64 {
    let lfo = 0.5 + 0.5 * (std::f64::consts::TAU * phase).sin();
    (base + depth * lfo).clamp(1.0, maximum)
}

fn bounded(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-32.0, 32.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_reads_and_lfo_period_are_deterministic() {
        let mut state = ModulationDelayState::new(100);
        state.cursor = 10;
        state.buffer[7] = [0.0, 0.0];
        state.buffer[8] = [1.0, -1.0];
        assert_eq!(state.read(0, 2.5), 0.5);
        assert_eq!(state.read(1, 2.5), -0.5);

        state.reset();
        for _ in 0..48_000 {
            let output = state.process(
                [0.0; 2],
                0.012,
                0.003,
                1.0,
                0.0,
                1.0,
                0.0,
                ModulationDelayMode::Flanger,
                48_000,
            );
            assert_eq!(output, [0.0; 2]);
        }
        assert!(state.phase.min(1.0 - state.phase).abs() < 1.0e-9);
    }

    #[test]
    fn delay_excursion_is_bounded_at_control_extremes() {
        for phase in [0.0, 0.25, 0.5, 0.75] {
            let delay = modulated_delay(4.8, 960.0, phase, 3_360.0);
            assert!((4.8..=964.8).contains(&delay));
        }
    }
}
