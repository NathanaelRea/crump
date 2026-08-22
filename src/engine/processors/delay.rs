use std::f64::consts::PI;

use super::*;

#[derive(Debug, Clone)]
pub(super) struct DelayState {
    pub(super) buffer: Vec<[f32; 2]>,
    pub(super) cursor: usize,
    pub(super) low_pass_state: [f32; 2],
    pub(super) high_pass_state: [f32; 2],
    pub(super) high_pass_input: [f32; 2],
    pub(super) current_delay_frames: f64,
    pub(super) transition_from_frames: f64,
    pub(super) transition_to_frames: f64,
    pub(super) transition_position: usize,
    pub(super) transition_frames: usize,
}

impl DelayState {
    pub(super) fn new(maximum_frames: usize) -> Self {
        Self {
            buffer: vec![[0.0, 0.0]; maximum_frames.max(3)],
            cursor: 0,
            low_pass_state: [0.0, 0.0],
            high_pass_state: [0.0, 0.0],
            high_pass_input: [0.0, 0.0],
            current_delay_frames: 0.0,
            transition_from_frames: 0.0,
            transition_to_frames: 0.0,
            transition_position: 0,
            transition_frames: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.buffer.fill([0.0; 2]);
        self.cursor = 0;
        self.low_pass_state = [0.0; 2];
        self.high_pass_state = [0.0; 2];
        self.high_pass_input = [0.0; 2];
        self.current_delay_frames = 0.0;
        self.transition_from_frames = 0.0;
        self.transition_to_frames = 0.0;
        self.transition_position = 0;
        self.transition_frames = 0;
    }

    pub(super) fn read(&self, delay_frames: f64) -> [f32; 2] {
        let integer = delay_frames.floor() as usize;
        let fraction = (delay_frames - integer as f64) as f32;
        let first =
            (self.cursor + self.buffer.len() - integer % self.buffer.len()) % self.buffer.len();
        let second = (first + self.buffer.len() - 1) % self.buffer.len();
        [
            self.buffer[first][0] * (1.0 - fraction) + self.buffer[second][0] * fraction,
            self.buffer[first][1] * (1.0 - fraction) + self.buffer[second][1] * fraction,
        ]
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        &mut self,
        sample: [f32; 2],
        time: f64,
        feedback: f64,
        mix: f64,
        low_pass: f64,
        high_pass: f64,
        time_crossfade: f64,
        mode: DelayMode,
        maximum_frames: usize,
        sample_rate: u32,
    ) -> [f32; 2] {
        let target_delay = (time.max(1.0 / f64::from(sample_rate)) * f64::from(sample_rate))
            .clamp(1.0, maximum_frames.saturating_sub(2) as f64);
        if self.current_delay_frames == 0.0 {
            self.current_delay_frames = target_delay;
        } else if self.transition_frames == 0
            && (target_delay - self.current_delay_frames).abs() > 1.0e-9
        {
            self.transition_from_frames = self.current_delay_frames;
            self.transition_to_frames = target_delay;
            self.transition_position = 0;
            self.transition_frames = (time_crossfade.clamp(0.001, 0.1) * f64::from(sample_rate))
                .round()
                .max(2.0) as usize;
        }
        let mut delayed = if self.transition_frames == 0 {
            self.read(self.current_delay_frames)
        } else {
            let denominator = self.transition_frames.saturating_sub(1).max(1) as f32;
            let blend = self.transition_position as f32 / denominator;
            let from = self.read(self.transition_from_frames);
            let to = self.read(self.transition_to_frames);
            [
                from[0] + (to[0] - from[0]) * blend,
                from[1] + (to[1] - from[1]) * blend,
            ]
        };
        if self.transition_frames > 0 {
            self.transition_position = self.transition_position.saturating_add(1);
            if self.transition_position >= self.transition_frames {
                self.current_delay_frames = self.transition_to_frames;
                self.transition_position = 0;
                self.transition_frames = 0;
            }
        }

        let low_alpha = (-2.0 * PI * low_pass.clamp(10.0, f64::from(sample_rate) * 0.49)
            / f64::from(sample_rate))
        .exp() as f32;
        let high_alpha = (-2.0 * PI * high_pass.clamp(10.0, f64::from(sample_rate) * 0.49)
            / f64::from(sample_rate))
        .exp() as f32;
        for (channel, delayed_sample) in delayed.iter_mut().enumerate() {
            self.low_pass_state[channel] =
                *delayed_sample * (1.0 - low_alpha) + self.low_pass_state[channel] * low_alpha;
            self.high_pass_state[channel] = high_alpha
                * (self.high_pass_state[channel] + self.low_pass_state[channel]
                    - self.high_pass_input[channel]);
            self.high_pass_input[channel] = self.low_pass_state[channel];
            *delayed_sample = self.high_pass_state[channel];
        }
        let feedback = feedback.clamp(0.0, 0.999) as f32;
        let feedback_signal = match mode {
            DelayMode::Stereo => delayed,
            DelayMode::PingPong => [delayed[1], delayed[0]],
        };
        self.buffer[self.cursor] = [
            sample[0] + feedback_signal[0] * feedback,
            sample[1] + feedback_signal[1] * feedback,
        ];
        self.cursor = (self.cursor + 1) % self.buffer.len();
        let mix = mix.clamp(0.0, 1.0) as f32;
        [
            sample[0] + (delayed[0] - sample[0]) * mix,
            sample[1] + (delayed[1] - sample[1]) * mix,
        ]
    }
}
