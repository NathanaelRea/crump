/// Applies the stateless stereo utility processor in its fixed operation order.
pub(super) fn process_utility(
    sample: [f32; 2],
    gain_db: f64,
    width: f64,
    invert: bool,
    mono: bool,
) -> [f32; 2] {
    let mut output = sample;
    if mono {
        let mono = (output[0] + output[1]) * 0.5;
        output = [mono, mono];
    }
    let mid = (output[0] + output[1]) * 0.5;
    let side = (output[0] - output[1]) * 0.5 * width as f32;
    let polarity = if invert { -1.0 } else { 1.0 };
    let gain = db_gain(gain_db) * polarity;
    [(mid + side) * gain, (mid - side) * gain]
}

pub(super) fn db_gain(decibels: f64) -> f32 {
    10.0_f64.powf(decibels.clamp(-192.0, 96.0) / 20.0) as f32
}

/// Preallocated integer stereo delay shared by latency and bypass alignment.
#[derive(Debug, Clone)]
pub(super) struct StereoDelay {
    pub(super) buffer: Vec<[f32; 2]>,
    pub(super) cursor: usize,
    pub(super) delay_frames: usize,
}

impl StereoDelay {
    pub(super) fn new(delay_frames: usize) -> Self {
        Self {
            buffer: vec![[0.0, 0.0]; delay_frames.max(1)],
            cursor: 0,
            delay_frames,
        }
    }

    pub(super) fn reset(&mut self) {
        self.buffer.fill([0.0; 2]);
        self.cursor = 0;
    }

    pub(super) fn process(&mut self, sample: [f32; 2]) -> [f32; 2] {
        if self.delay_frames == 0 {
            return sample;
        }
        let output = self.buffer[self.cursor];
        self.buffer[self.cursor] = sample;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        output
    }
}
