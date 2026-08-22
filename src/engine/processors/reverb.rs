use std::f64::consts::PI;

use super::*;

#[derive(Debug, Clone)]
pub(super) struct MonoDelay {
    pub(super) buffer: Vec<f32>,
    pub(super) cursor: usize,
    pub(super) base_frames: f64,
    pub(super) damping_state: f32,
}

impl MonoDelay {
    pub(super) fn new(frames: usize) -> Self {
        Self {
            buffer: vec![0.0; frames.saturating_add(REVERB_COMB_INTERPOLATION_PADDING)],
            cursor: 0,
            base_frames: frames.max(1) as f64,
            damping_state: 0.0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.cursor = 0;
        self.damping_state = 0.0;
    }

    pub(super) fn read(&self, delay_frames: f64) -> f32 {
        let delay_frames = delay_frames.clamp(1.0, self.buffer.len().saturating_sub(2) as f64);
        let position = (self.cursor as f64 - delay_frames).rem_euclid(self.buffer.len() as f64);
        let earlier = position.floor() as usize % self.buffer.len();
        let later = (earlier + 1) % self.buffer.len();
        let fraction = (position - position.floor()) as f32;
        self.buffer[earlier] + (self.buffer[later] - self.buffer[earlier]) * fraction
    }

    pub(super) fn process(
        &mut self,
        input: f32,
        feedback: f32,
        damping: f32,
        modulation_frames: f64,
    ) -> f32 {
        let output = self.read(self.base_frames + modulation_frames);
        self.damping_state += (output - self.damping_state) * (1.0 - damping);
        self.buffer[self.cursor] = input + self.damping_state * feedback;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        output
    }
}

#[derive(Debug, Clone)]
pub(super) struct AllpassDelay {
    pub(super) buffer: Vec<f32>,
    pub(super) cursor: usize,
}

impl AllpassDelay {
    pub(super) fn new(frames: usize) -> Self {
        Self {
            buffer: vec![0.0; frames.max(1)],
            cursor: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.cursor = 0;
    }

    pub(super) fn process(&mut self, input: f32, coefficient: f32) -> f32 {
        let delayed = self.buffer[self.cursor];
        let output = delayed - coefficient * input;
        self.buffer[self.cursor] = input + coefficient * output;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        output
    }
}

#[derive(Debug, Clone)]
pub(super) struct ReverbPreDelay {
    pub(super) buffer: Vec<[f32; 2]>,
    pub(super) cursor: usize,
    pub(super) current_frames: f64,
    pub(super) transition_from_frames: f64,
    pub(super) transition_to_frames: f64,
    pub(super) transition_position: usize,
    pub(super) transition_frames: usize,
}

impl ReverbPreDelay {
    pub(super) fn new(sample_rate: u32) -> Self {
        Self {
            buffer: vec![[0.0, 0.0]; sample_rate as usize + 2],
            cursor: 0,
            current_frames: -1.0,
            transition_from_frames: 0.0,
            transition_to_frames: 0.0,
            transition_position: 0,
            transition_frames: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.buffer.fill([0.0; 2]);
        self.cursor = 0;
        self.current_frames = -1.0;
        self.transition_from_frames = 0.0;
        self.transition_to_frames = 0.0;
        self.transition_position = 0;
        self.transition_frames = 0;
    }

    pub(super) fn read(&self, sample: [f32; 2], delay_frames: f64) -> [f32; 2] {
        if delay_frames <= 0.0 {
            return sample;
        }
        let position = (self.cursor as f64 - delay_frames).rem_euclid(self.buffer.len() as f64);
        let earlier = position.floor() as usize % self.buffer.len();
        let later = (earlier + 1) % self.buffer.len();
        let fraction = (position - position.floor()) as f32;
        [
            self.buffer[earlier][0] + (self.buffer[later][0] - self.buffer[earlier][0]) * fraction,
            self.buffer[earlier][1] + (self.buffer[later][1] - self.buffer[earlier][1]) * fraction,
        ]
    }

    pub(super) fn process(&mut self, sample: [f32; 2], seconds: f64, sample_rate: u32) -> [f32; 2] {
        let target =
            (seconds.clamp(0.0, 1.0) * f64::from(sample_rate)).clamp(0.0, f64::from(sample_rate));
        if self.current_frames < 0.0 {
            self.current_frames = target;
        } else if self.transition_frames == 0 && (target - self.current_frames).abs() > 1.0e-9 {
            self.transition_from_frames = self.current_frames;
            self.transition_to_frames = target;
            self.transition_position = 0;
            self.transition_frames = (0.01 * f64::from(sample_rate)).round().max(2.0) as usize;
        }
        let delayed = if self.transition_frames == 0 {
            self.read(sample, self.current_frames)
        } else {
            let denominator = self.transition_frames.saturating_sub(1).max(1) as f32;
            let blend = self.transition_position as f32 / denominator;
            let from = self.read(sample, self.transition_from_frames);
            let to = self.read(sample, self.transition_to_frames);
            [
                from[0] + (to[0] - from[0]) * blend,
                from[1] + (to[1] - from[1]) * blend,
            ]
        };
        self.buffer[self.cursor] = sample;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        if self.transition_frames > 0 {
            self.transition_position = self.transition_position.saturating_add(1);
            if self.transition_position >= self.transition_frames {
                self.current_frames = self.transition_to_frames;
                self.transition_position = 0;
                self.transition_frames = 0;
            }
        }
        delayed
    }
}

#[derive(Debug, Clone)]
pub(super) struct ReverbState {
    pub(super) pre_delay: ReverbPreDelay,
    pub(super) left_combs: Vec<MonoDelay>,
    pub(super) right_combs: Vec<MonoDelay>,
    pub(super) left_diffusers: Vec<AllpassDelay>,
    pub(super) right_diffusers: Vec<AllpassDelay>,
    pub(super) phase: f64,
}

impl ReverbState {
    pub(super) fn new(sample_rate: u32) -> Self {
        let (left_combs, right_combs) = reverb_comb_lengths(sample_rate);
        let (left_diffusers, right_diffusers) = reverb_diffuser_lengths(sample_rate);
        Self {
            pre_delay: ReverbPreDelay::new(sample_rate),
            left_combs: left_combs.into_iter().map(MonoDelay::new).collect(),
            right_combs: right_combs.into_iter().map(MonoDelay::new).collect(),
            left_diffusers: left_diffusers.into_iter().map(AllpassDelay::new).collect(),
            right_diffusers: right_diffusers.into_iter().map(AllpassDelay::new).collect(),
            phase: 0.0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.pre_delay.reset();
        for delay in self.left_combs.iter_mut().chain(&mut self.right_combs) {
            delay.reset();
        }
        for diffuser in self
            .left_diffusers
            .iter_mut()
            .chain(&mut self.right_diffusers)
        {
            diffuser.reset();
        }
        self.phase = 0.0;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        &mut self,
        sample: [f32; 2],
        pre_delay: f64,
        diffusion: f64,
        decay: f64,
        damping: f64,
        modulation: f64,
        width: f64,
        mix: f64,
        sample_rate: u32,
    ) -> [f32; 2] {
        let delayed = self.pre_delay.process(sample, pre_delay, sample_rate);

        let network_input = [
            (delayed[0] * 0.75 + delayed[1] * 0.25) * REVERB_INPUT_GAIN,
            (delayed[1] * 0.75 + delayed[0] * 0.25) * REVERB_INPUT_GAIN,
        ];
        self.phase = (self.phase + 0.17 / f64::from(sample_rate)) % 1.0;
        let feedback = (0.15 + decay.clamp(0.0, 1.0) * 0.835).clamp(0.0, 0.985) as f32;
        let damping = damping.clamp(0.0, 1.0) as f32;
        let modulation_depth = modulation.clamp(0.0, 1.0)
            * REVERB_MODULATION_DEPTH_FRAMES_48K
            * f64::from(sample_rate)
            / 48_000.0;
        let mut wet = [0.0_f32; 2];
        for (index, delay) in self.left_combs.iter_mut().enumerate() {
            let phase = (self.phase + index as f64 * 0.173).fract() * 2.0 * PI;
            wet[0] += delay.process(
                network_input[0],
                feedback,
                damping,
                phase.sin() * modulation_depth,
            );
        }
        for (index, delay) in self.right_combs.iter_mut().enumerate() {
            let phase = (self.phase + 0.25 + index as f64 * 0.173).fract() * 2.0 * PI;
            wet[1] += delay.process(
                network_input[1],
                feedback,
                damping,
                phase.sin() * modulation_depth,
            );
        }
        wet[0] *= 0.25;
        wet[1] *= 0.25;

        let diffusion = diffusion.clamp(0.0, 1.0) as f32 * 0.75;
        for diffuser in &mut self.left_diffusers {
            wet[0] = diffuser.process(wet[0], diffusion);
        }
        for diffuser in &mut self.right_diffusers {
            wet[1] = diffuser.process(wet[1], diffusion);
        }

        let mid = (wet[0] + wet[1]) * 0.5;
        let side = (wet[0] - wet[1]) * 0.5 * width.clamp(0.0, 1.0) as f32;
        wet = [mid + side, mid - side];
        let mix = mix.clamp(0.0, 1.0) as f32;
        [
            sample[0] + (wet[0] - sample[0]) * mix,
            sample[1] + (wet[1] - sample[1]) * mix,
        ]
    }
}
