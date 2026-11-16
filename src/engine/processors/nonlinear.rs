use super::super::oversampling::{
    FilteredOversampler, OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES,
    OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES, OversamplingInterpolator,
};
use super::*;

pub(super) fn saturate(sample: f32, mode: SaturationMode) -> f32 {
    match mode {
        SaturationMode::SoftClip => sample / (1.0 + sample.abs()),
        SaturationMode::Tanh => sample.tanh(),
        SaturationMode::Tape => {
            let biased = sample + 0.05;
            biased.tanh() - 0.05_f32.tanh()
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct SaturationState {
    pub(super) oversampler: FilteredOversampler,
    pub(super) dry_delay: StereoDelay,
}

impl SaturationState {
    pub(super) fn new() -> Self {
        Self {
            oversampler: FilteredOversampler::default(),
            dry_delay: StereoDelay::new(OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES),
        }
    }

    pub(super) fn reset(&mut self) {
        self.oversampler.reset();
        self.dry_delay.reset();
    }

    pub(super) fn process(
        &mut self,
        sample: [f32; 2],
        drive: f64,
        mix: f64,
        output: f64,
        mode: SaturationMode,
    ) -> [f32; 2] {
        let drive = drive.max(0.0) as f32;
        let wet = self.oversampler.process(sample, |sample| {
            [
                saturate(sample[0] * drive, mode),
                saturate(sample[1] * drive, mode),
            ]
        });
        let dry = self.dry_delay.process(sample);
        let mix = mix.clamp(0.0, 1.0) as f32;
        let output_gain = db_gain(output);
        [
            (dry[0] + (wet[0] - dry[0]) * mix) * output_gain,
            (dry[1] + (wet[1] - dry[1]) * mix) * output_gain,
        ]
    }
}

#[derive(Debug, Clone)]
pub(super) struct LimiterState {
    pub(super) audio_buffer: Vec<[f32; 2]>,
    pub(super) audio_cursor: usize,
    pub(super) detector: OversamplingInterpolator,
    pub(super) peak_buffer: Vec<f32>,
    pub(super) peak_cursor: usize,
    pub(super) gain: f32,
}

impl LimiterState {
    pub(super) fn new(lookahead_frames: usize) -> Self {
        let latency_frames = lookahead_frames
            .saturating_add(OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES)
            .max(1);
        Self {
            audio_buffer: vec![[0.0, 0.0]; latency_frames],
            audio_cursor: 0,
            detector: OversamplingInterpolator::default(),
            peak_buffer: vec![0.0; lookahead_frames.saturating_add(1).max(1)],
            peak_cursor: 0,
            gain: 1.0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.audio_buffer.fill([0.0; 2]);
        self.audio_cursor = 0;
        self.detector.reset();
        self.peak_buffer.fill(0.0);
        self.peak_cursor = 0;
        self.gain = 1.0;
    }

    pub(super) fn process(
        &mut self,
        sample: [f32; 2],
        ceiling: f32,
        release: f64,
        sample_rate: u32,
    ) -> [f32; 2] {
        let true_peak = self
            .detector
            .process(sample)
            .into_iter()
            .flatten()
            .map(f32::abs)
            .fold(1.0e-12_f32, f32::max);
        self.peak_buffer[self.peak_cursor] = true_peak;
        self.peak_cursor = (self.peak_cursor + 1) % self.peak_buffer.len();

        let output = self.audio_buffer[self.audio_cursor];
        self.audio_buffer[self.audio_cursor] = sample;
        self.audio_cursor = (self.audio_cursor + 1) % self.audio_buffer.len();

        let window_peak = self.peak_buffer.iter().copied().fold(1.0e-12_f32, f32::max);
        let target = (ceiling * LIMITER_TRUE_PEAK_SAFETY_RATIO / window_peak).min(1.0);
        if target < self.gain {
            self.gain = target;
        } else {
            let coefficient = time_coefficient(release, sample_rate) as f32;
            self.gain = target + coefficient * (self.gain - target);
        }
        [output[0] * self.gain, output[1] * self.gain]
    }
}
