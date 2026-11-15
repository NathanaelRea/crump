//! Generic signal primitives: tuned delay, arithmetic, shaping, conversion and loop filters.
//!
//! `delay_line` is the only Node whose output depends solely on earlier frames. The graph
//! runtime evaluates it after every other Node of a frame, so a Bus into it may close a
//! feedback loop with exactly the authored delay.

use std::f64::consts::PI;

use super::Scalar;

/// Largest authored `delay_line` buffer.
pub(super) const MAX_DELAY_LINE_SECONDS: f64 = 2.0;
/// Default buffer: one period of 10 Hz, enough for every pitch-mode note at ratio 1.
pub(super) const DEFAULT_DELAY_LINE_MAX_SECONDS: f64 = 0.1;
/// Most points in one authored `shaper` table.
pub(super) const MAX_SHAPER_POINTS: usize = 64;
/// Guard frames after the authored maximum, covering four-point interpolation reads.
const DELAY_LINE_GUARD_FRAMES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DelayLineMode {
    Time,
    Pitch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DelayInterpolation {
    Linear,
    Lagrange3,
    Allpass,
}

impl DelayInterpolation {
    /// Shortest delay, in frames, the interpolator reads without using the current frame.
    pub(super) const fn minimum_frames(self) -> f64 {
        match self {
            Self::Linear => 1.0,
            Self::Allpass => 1.5,
            Self::Lagrange3 => 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DelayLineConfig {
    pub(super) mode: DelayLineMode,
    pub(super) interpolation: DelayInterpolation,
    pub(super) time: Scalar,
    pub(super) ratio: Scalar,
    pub(super) mod_depth: Scalar,
    pub(super) tune_compensation: Scalar,
    pub(super) capacity_frames: usize,
}

#[derive(Debug, Clone)]
pub(super) struct DelayLineState {
    buffer: Vec<f32>,
    /// Index of the most recently written frame.
    newest: usize,
    allpass_output: f64,
}

pub(super) fn delay_line_capacity(max_seconds: f64, sample_rate: u32) -> usize {
    (max_seconds * f64::from(sample_rate)).ceil().max(1.0) as usize + DELAY_LINE_GUARD_FRAMES
}

pub(super) fn delay_line_state_bytes(capacity_frames: usize) -> u64 {
    (std::mem::size_of::<DelayLineState>() as u64)
        .saturating_add((capacity_frames as u64).saturating_mul(std::mem::size_of::<f32>() as u64))
}

impl DelayLineState {
    pub(super) fn new(capacity_frames: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity_frames.max(DELAY_LINE_GUARD_FRAMES + 1)],
            newest: 0,
            allpass_output: 0.0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.newest = 0;
        self.allpass_output = 0.0;
    }

    /// Frame written `offset` frames before the newest one.
    fn past(&self, offset: usize) -> f64 {
        let length = self.buffer.len();
        f64::from(self.buffer[(self.newest + length - offset % length) % length])
    }

    #[cfg(test)]
    pub(super) fn heap_bytes(&self) -> u64 {
        (self.buffer.capacity() as u64).saturating_mul(std::mem::size_of::<f32>() as u64)
    }
}

impl DelayLineConfig {
    /// Commits this frame's input and returns the output for the next frame.
    ///
    /// The returned value is `input[n + 1 - delay]`, so an output never depends on the frame
    /// being written. Pitch and modulation inputs set the delay of that next output.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        self,
        state: &mut DelayLineState,
        audio: f32,
        pitch: f32,
        time_modulation: f32,
        has_time_modulation: bool,
        controls: &[f64],
        sample_rate: f64,
    ) -> f32 {
        state.newest = (state.newest + 1) % state.buffer.len();
        state.buffer[state.newest] = if audio.is_finite() { audio } else { 0.0 };
        let base = match self.mode {
            DelayLineMode::Time => self.time.value(controls).max(0.0) * sample_rate,
            DelayLineMode::Pitch => {
                let pitch = f64::from(pitch);
                if pitch.is_finite() && pitch > 0.0 {
                    self.ratio.value(controls).max(0.0) * sample_rate / pitch
                } else {
                    0.0
                }
            }
        };
        let modulation = if has_time_modulation && time_modulation.is_finite() {
            f64::from(time_modulation).clamp(-1.0, 1.0) * self.mod_depth.value(controls)
        } else {
            0.0
        };
        let maximum = (self.capacity_frames.saturating_sub(DELAY_LINE_GUARD_FRAMES)).max(1) as f64;
        let delay = (base * (1.0 + modulation) - self.tune_compensation.value(controls))
            .clamp(self.interpolation.minimum_frames(), maximum.max(2.0));
        // `delay` frames before the next output is `delay - 1` frames before the newest input.
        let behind = delay - 1.0;
        let output = match self.interpolation {
            DelayInterpolation::Linear => {
                let whole = behind.floor();
                let fraction = behind - whole;
                let index = whole as usize;
                state.past(index) * (1.0 - fraction) + state.past(index + 1) * fraction
            }
            DelayInterpolation::Lagrange3 => {
                let whole = behind.floor();
                let index = whole as usize;
                // Read four frames from `index - 1`; the fractional position within them is
                // `1 + fraction`, the low-error centre of a third-order Lagrange kernel.
                let d = 1.0 + (behind - whole);
                let h0 = -(d - 1.0) * (d - 2.0) * (d - 3.0) / 6.0;
                let h1 = d * (d - 2.0) * (d - 3.0) / 2.0;
                let h2 = -d * (d - 1.0) * (d - 3.0) / 2.0;
                let h3 = d * (d - 1.0) * (d - 2.0) / 6.0;
                h0 * state.past(index - 1)
                    + h1 * state.past(index)
                    + h2 * state.past(index + 1)
                    + h3 * state.past(index + 2)
            }
            DelayInterpolation::Allpass => {
                // First-order Thiran allpass with its fractional part kept in 0.5..1.5.
                let whole = (behind - 0.5).floor().max(0.0);
                let fraction = behind - whole;
                let coefficient = (1.0 - fraction) / (1.0 + fraction);
                let index = whole as usize;
                let output = coefficient * (state.past(index) - state.allpass_output)
                    + state.past(index + 1);
                state.allpass_output = output;
                output
            }
        };
        output as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MathOperation {
    Add,
    Subtract,
    Multiply,
    Minimum,
    Maximum,
    Absolute,
    Negate,
    Offset,
    Scale,
}

impl MathOperation {
    pub(super) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "add" => Self::Add,
            "subtract" => Self::Subtract,
            "multiply" => Self::Multiply,
            "min" => Self::Minimum,
            "max" => Self::Maximum,
            "abs" => Self::Absolute,
            "negate" => Self::Negate,
            "offset" => Self::Offset,
            "scale" => Self::Scale,
            _ => return None,
        })
    }

    pub(super) const fn default_amount(self) -> f64 {
        match self {
            Self::Scale => 1.0,
            _ => 0.0,
        }
    }

    fn apply(self, a: f32, b: f32, amount: f32) -> f32 {
        match self {
            Self::Add => a + b,
            Self::Subtract => a - b,
            Self::Multiply => a * b,
            Self::Minimum => a.min(b),
            Self::Maximum => a.max(b),
            Self::Absolute => a.abs(),
            Self::Negate => -a,
            Self::Offset => a + amount,
            Self::Scale => a * amount,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MathConfig {
    pub(super) operation: MathOperation,
    pub(super) amount: Scalar,
}

impl MathConfig {
    pub(super) fn process_mono(self, a: f32, b: f32, controls: &[f64]) -> f32 {
        self.operation
            .apply(a, b, self.amount.value(controls) as f32)
    }

    pub(super) fn process_stereo(self, a: [f32; 2], b: [f32; 2], controls: &[f64]) -> [f32; 2] {
        let amount = self.amount.value(controls) as f32;
        [
            self.operation.apply(a[0], b[0], amount),
            self.operation.apply(a[1], b[1], amount),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShaperCurve {
    Tanh,
    HardClip,
    SoftClip,
    OneSided,
    Asymmetric,
    Table,
}

impl ShaperCurve {
    pub(super) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "tanh" => Self::Tanh,
            "hard_clip" => Self::HardClip,
            "soft_clip" => Self::SoftClip,
            "one_sided" => Self::OneSided,
            "asymmetric" => Self::Asymmetric,
            "table" => Self::Table,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ShaperConfig {
    pub(super) curve: ShaperCurve,
    pub(super) drive: Scalar,
    pub(super) bias: Scalar,
    pub(super) asymmetry: Scalar,
    pub(super) points: [(f32, f32); MAX_SHAPER_POINTS],
    pub(super) point_count: u8,
}

impl ShaperConfig {
    pub(super) fn process(&self, audio: f32, controls: &[f64]) -> f32 {
        let shaped = self.drive.value(controls) * f64::from(audio) + self.bias.value(controls);
        let output = match self.curve {
            ShaperCurve::Tanh => shaped.tanh(),
            ShaperCurve::HardClip => shaped.clamp(-1.0, 1.0),
            ShaperCurve::SoftClip => {
                let clipped = shaped.clamp(-1.0, 1.0);
                1.5 * (clipped - clipped * clipped * clipped / 3.0)
            }
            ShaperCurve::OneSided => shaped.max(0.0),
            ShaperCurve::Asymmetric => {
                if shaped >= 0.0 {
                    shaped.tanh()
                } else {
                    let knee = 1.0 + 3.0 * self.asymmetry.value(controls).clamp(0.0, 1.0);
                    (shaped * knee).tanh() / knee
                }
            }
            ShaperCurve::Table => f64::from(self.table(shaped as f32)),
        };
        output as f32
    }

    /// Piecewise-linear lookup with constant extrapolation beyond the end points.
    fn table(&self, input: f32) -> f32 {
        let points = &self.points[..usize::from(self.point_count)];
        let (Some(first), Some(last)) = (points.first(), points.last()) else {
            return 0.0;
        };
        if input.is_nan() {
            return 0.0;
        }
        if input <= first.0 {
            return first.1;
        }
        if input >= last.0 {
            return last.1;
        }
        let upper = points.partition_point(|point| point.0 <= input);
        let (x0, y0) = points[upper - 1];
        let (x1, y1) = points[upper];
        y0 + (y1 - y0) * (input - x0) / (x1 - x0)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DcBlockConfig {
    pub(super) cutoff: Scalar,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DcBlockState {
    input: f64,
    output: f64,
}

impl DcBlockConfig {
    pub(super) fn process(
        self,
        state: &mut DcBlockState,
        audio: f32,
        controls: &[f64],
        sample_rate: f64,
    ) -> f32 {
        let cutoff = self.cutoff.value(controls).clamp(1.0, sample_rate * 0.25);
        let pole = (-2.0 * PI * cutoff / sample_rate).exp();
        let input = f64::from(audio);
        let output = input - state.input + pole * state.output;
        state.input = input;
        state.output = output;
        output as f32
    }
}

/// Responses of the second-order [`BiquadConfig`], from the Audio EQ Cookbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BiquadMode {
    Lowpass,
    Highpass,
    /// Constant 0 dB peak gain.
    Bandpass,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
    Allpass,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BiquadConfig {
    pub(super) mode: BiquadMode,
    pub(super) frequency: Scalar,
    pub(super) q: Scalar,
    pub(super) gain: Scalar,
    pub(super) key_track: Scalar,
    pub(super) key_reference: Scalar,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct BiquadState {
    /// Transposed direct form II state.
    first: f64,
    second: f64,
    /// Coefficients and the settings they were computed from.
    coefficients: [f64; 5],
    computed_for: Option<[u64; 3]>,
}

impl BiquadConfig {
    /// Normalized `[b0, b1, b2, a1, a2]` for one frequency, Q and gain.
    fn coefficients(self, frequency: f64, q: f64, gain_db: f64, sample_rate: f64) -> [f64; 5] {
        let w = 2.0 * PI * frequency / sample_rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        let amplitude = 10.0_f64.powf(gain_db / 40.0);
        let shelf = 2.0 * amplitude.sqrt() * alpha;
        let [b0, b1, b2, a0, a1, a2] = match self.mode {
            BiquadMode::Lowpass => {
                let b = (1.0 - cos) / 2.0;
                [b, 1.0 - cos, b, 1.0 + alpha, -2.0 * cos, 1.0 - alpha]
            }
            BiquadMode::Highpass => {
                let b = (1.0 + cos) / 2.0;
                [b, -(1.0 + cos), b, 1.0 + alpha, -2.0 * cos, 1.0 - alpha]
            }
            BiquadMode::Bandpass => [alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
            BiquadMode::Notch => [1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
            BiquadMode::Allpass => [
                1.0 - alpha,
                -2.0 * cos,
                1.0 + alpha,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ],
            BiquadMode::Peak => [
                1.0 + alpha * amplitude,
                -2.0 * cos,
                1.0 - alpha * amplitude,
                1.0 + alpha / amplitude,
                -2.0 * cos,
                1.0 - alpha / amplitude,
            ],
            BiquadMode::LowShelf => {
                let (p, m) = (amplitude + 1.0, amplitude - 1.0);
                [
                    amplitude * (p - m * cos + shelf),
                    2.0 * amplitude * (m - p * cos),
                    amplitude * (p - m * cos - shelf),
                    p + m * cos + shelf,
                    -2.0 * (m + p * cos),
                    p + m * cos - shelf,
                ]
            }
            BiquadMode::HighShelf => {
                let (p, m) = (amplitude + 1.0, amplitude - 1.0);
                [
                    amplitude * (p + m * cos + shelf),
                    -2.0 * amplitude * (m + p * cos),
                    amplitude * (p + m * cos - shelf),
                    p - m * cos + shelf,
                    2.0 * (m - p * cos),
                    p - m * cos - shelf,
                ]
            }
        };
        [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
    }

    pub(super) fn process(
        self,
        state: &mut BiquadState,
        audio: f32,
        key: f32,
        has_key: bool,
        controls: &[f64],
        sample_rate: f64,
    ) -> f32 {
        let mut frequency = self.frequency.value(controls);
        if has_key {
            let reference = self.key_reference.value(controls);
            let key = f64::from(key);
            if key > 0.0 && reference > 0.0 {
                frequency *= (key / reference).powf(self.key_track.value(controls).clamp(0.0, 1.0));
            }
        }
        // Stay clear of DC and Nyquist, where the cookbook forms degenerate.
        let frequency = frequency.clamp(1.0, sample_rate * 0.49);
        let q = self.q.value(controls).clamp(0.05, 64.0);
        let gain = self.gain.value(controls).clamp(-48.0, 48.0);
        let key = [frequency.to_bits(), q.to_bits(), gain.to_bits()];
        if state.computed_for != Some(key) {
            state.coefficients = self.coefficients(frequency, q, gain, sample_rate);
            state.computed_for = Some(key);
        }
        let [b0, b1, b2, a1, a2] = state.coefficients;
        let input = f64::from(audio);
        let output = b0 * input + state.first;
        state.first = b1 * input - a1 * output + state.second;
        state.second = b2 * input - a2 * output;
        if !output.is_finite() {
            *state = BiquadState::default();
            return 0.0;
        }
        output as f32
    }
}

/// Responses of the first-order [`OnePoleConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OnePoleMode {
    Lowpass,
    Highpass,
    /// `y = x + e^(-1 / (time × rate)) y[n-1]`: an impulse becomes an exact exponential
    /// decay with e-folding `time`, and a step becomes a smooth rise.
    Leak,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct OnePoleConfig {
    pub(super) mode: OnePoleMode,
    pub(super) cutoff: Scalar,
    pub(super) time: Scalar,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct OnePoleState {
    output: f64,
}

impl OnePoleConfig {
    pub(super) fn process(
        self,
        state: &mut OnePoleState,
        audio: f32,
        controls: &[f64],
        sample_rate: f64,
    ) -> f32 {
        let input = f64::from(audio);
        let output = match self.mode {
            OnePoleMode::Leak => {
                let time = self.time.value(controls).clamp(0.000_01, 60.0);
                input + (-1.0 / (time * sample_rate)).exp() * state.output
            }
            OnePoleMode::Lowpass | OnePoleMode::Highpass => {
                let cutoff = self.cutoff.value(controls).clamp(1.0, sample_rate * 0.49);
                let pole = (-2.0 * PI * cutoff / sample_rate).exp();
                (1.0 - pole) * input + pole * state.output
            }
        };
        state.output = if output.is_finite() { output } else { 0.0 };
        match self.mode {
            OnePoleMode::Highpass => (input - state.output) as f32,
            _ => state.output as f32,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LinearPhaseLowpassConfig {
    pub(super) amount: Scalar,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LinearPhaseLowpassState {
    previous: f64,
    before_previous: f64,
}

impl LinearPhaseLowpassConfig {
    /// `g·x[n] + (1 − 2g)·x[n−1] + g·x[n−2]` with `g = amount / 4`.
    ///
    /// The response is non-negative for every amount, so its phase delay is exactly one frame
    /// at all frequencies and a loop containing it can be compensated exactly.
    pub(super) fn process(
        self,
        state: &mut LinearPhaseLowpassState,
        audio: f32,
        controls: &[f64],
    ) -> f32 {
        let side = self.amount.value(controls).clamp(0.0, 1.0) * 0.25;
        let input = f64::from(audio);
        let output =
            side * input + (1.0 - 2.0 * side) * state.previous + side * state.before_previous;
        state.before_previous = state.previous;
        state.previous = input;
        output as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_1_SQRT_2;

    fn delay_config(mode: DelayLineMode, interpolation: DelayInterpolation) -> DelayLineConfig {
        DelayLineConfig {
            mode,
            interpolation,
            time: Scalar::constant(0.0),
            ratio: Scalar::constant(1.0),
            mod_depth: Scalar::constant(0.0),
            tune_compensation: Scalar::constant(0.0),
            capacity_frames: delay_line_capacity(0.1, 48_000),
        }
    }

    /// Feeds an impulse, then the delay's own output scaled by `gain`, and returns the output.
    fn impulse_loop(config: DelayLineConfig, pitch: f32, gain: f32, frames: usize) -> Vec<f32> {
        impulse_loop_at(config, pitch, gain, frames, 48_000.0)
    }

    fn impulse_loop_at(
        config: DelayLineConfig,
        pitch: f32,
        gain: f32,
        frames: usize,
        rate: f64,
    ) -> Vec<f32> {
        let mut state = DelayLineState::new(config.capacity_frames);
        let mut output = 0.0;
        (0..frames)
            .map(|frame| {
                let current = output;
                let input = if frame == 0 { 1.0 } else { current * gain };
                output = config.process(&mut state, input, pitch, 0.0, false, &[], rate);
                current
            })
            .collect()
    }

    #[test]
    fn pitch_mode_tuning_is_sample_rate_independent() {
        for rate in [44_100_u32, 48_000, 96_000] {
            for frequency in [55.0, 440.0, 1_760.0] {
                let mut config = delay_config(DelayLineMode::Pitch, DelayInterpolation::Lagrange3);
                config.capacity_frames = delay_line_capacity(0.1, rate);
                let frames = rate as usize / 2;
                let signal =
                    impulse_loop_at(config, frequency as f32, 0.999, frames, f64::from(rate));
                let measured = fundamental(&signal[frames / 4..], frequency, f64::from(rate));
                let cents = 1200.0 * (measured / frequency).log2();
                assert!(
                    cents.abs() < 2.0,
                    "{rate} Hz, {frequency} Hz: {cents} cents"
                );
            }
        }
    }

    #[test]
    fn integer_delay_is_exact_for_every_interpolation() {
        for interpolation in [
            DelayInterpolation::Linear,
            DelayInterpolation::Lagrange3,
            DelayInterpolation::Allpass,
        ] {
            let mut config = delay_config(DelayLineMode::Time, interpolation);
            config.time = Scalar::constant(5.0 / 48_000.0);
            let mut state = DelayLineState::new(config.capacity_frames);
            let mut output = 0.0;
            let mut seen = Vec::new();
            for frame in 0..12 {
                seen.push(output);
                output = config.process(
                    &mut state,
                    f32::from(frame == 0),
                    0.0,
                    0.0,
                    false,
                    &[],
                    48_000.0,
                );
            }
            let peak = seen
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .unwrap();
            assert_eq!(peak.0, 5, "{interpolation:?}: {seen:?}");
            assert!((peak.1 - 1.0).abs() < 1.0e-6, "{interpolation:?}: {seen:?}");
        }
    }

    #[test]
    fn minimum_delay_never_reads_the_frame_being_written() {
        let mut config = delay_config(DelayLineMode::Time, DelayInterpolation::Linear);
        config.time = Scalar::constant(0.0);
        let mut state = DelayLineState::new(config.capacity_frames);
        let first = config.process(&mut state, 1.0, 0.0, 0.0, false, &[], 48_000.0);
        // The output returned at the impulse frame is the next frame's sample: the impulse,
        // delayed by exactly one frame.
        assert_eq!(first, 1.0);
    }

    #[test]
    fn modulation_and_bounds_stay_finite() {
        let mut config = delay_config(DelayLineMode::Pitch, DelayInterpolation::Lagrange3);
        config.mod_depth = Scalar::constant(1.0);
        let mut state = DelayLineState::new(config.capacity_frames);
        for frame in 0..48_000 {
            let modulation = ((frame as f32) * 0.37).sin();
            let audio = if frame % 97 == 0 { 1.0 } else { 0.0 };
            // Pitches far below the buffer's lowest supported note clamp to its capacity.
            let output = config.process(
                &mut state,
                audio,
                if frame % 2 == 0 { 1.0 } else { 20_000.0 },
                modulation,
                true,
                &[],
                48_000.0,
            );
            assert!(output.is_finite() && output.abs() <= 2.0);
        }
    }

    #[test]
    fn pitch_mode_loops_tune_within_two_cents_from_c1_to_c7() {
        for interpolation in [
            DelayInterpolation::Linear,
            DelayInterpolation::Lagrange3,
            DelayInterpolation::Allpass,
        ] {
            for midi in (24_u8..=96).step_by(6) {
                let frequency = 440.0 * 2.0_f64.powf((f64::from(midi) - 69.0) / 12.0);
                let config = delay_config(DelayLineMode::Pitch, interpolation);
                let frames = 24_000;
                let signal = impulse_loop(config, frequency as f32, 0.999, frames);
                let measured = fundamental(&signal[frames / 4..], frequency, 48_000.0);
                let cents = 1200.0 * (measured / frequency).log2();
                assert!(
                    cents.abs() < 2.0,
                    "{interpolation:?} MIDI {midi}: {measured} Hz vs {frequency} Hz ({cents} cents)"
                );
            }
        }
    }

    /// Finds the strongest spectral peak within a semitone of `expected` by golden search on
    /// the single-bin DFT magnitude.
    fn fundamental(signal: &[f32], expected: f64, rate: f64) -> f64 {
        let window = |index: usize| {
            let phase = index as f64 / (signal.len() - 1) as f64;
            0.5 - 0.5 * (2.0 * PI * phase).cos()
        };
        let magnitude = |frequency: f64| {
            let (mut real, mut imaginary) = (0.0, 0.0);
            for (index, sample) in signal.iter().enumerate() {
                let phase = 2.0 * PI * frequency * index as f64 / rate;
                let value = f64::from(*sample) * window(index);
                real += value * phase.cos();
                imaginary -= value * phase.sin();
            }
            real.hypot(imaginary)
        };
        let semitone = 2.0_f64.powf(1.0 / 12.0);
        let steps = 24;
        let low = expected / semitone;
        let high = expected * semitone;
        let mut best = (expected, 0.0);
        for step in 0..=steps {
            let frequency = low * (high / low).powf(step as f64 / steps as f64);
            let value = magnitude(frequency);
            if value > best.1 {
                best = (frequency, value);
            }
        }
        let ratio = (high / low).powf(1.0 / steps as f64);
        let (mut left, mut right) = (best.0 / ratio, best.0 * ratio);
        let golden = (5.0_f64.sqrt() - 1.0) / 2.0;
        for _ in 0..40 {
            let first = right - golden * (right - left);
            let second = left + golden * (right - left);
            if magnitude(first) > magnitude(second) {
                right = second;
            } else {
                left = first;
            }
        }
        (left + right) * 0.5
    }

    #[test]
    fn math_operations_are_exact() {
        let cases = [
            (MathOperation::Add, 0.5 + -0.25),
            (MathOperation::Subtract, 0.5 - -0.25),
            (MathOperation::Multiply, 0.5 * -0.25),
            (MathOperation::Minimum, -0.25),
            (MathOperation::Maximum, 0.5),
            (MathOperation::Absolute, 0.5),
            (MathOperation::Negate, -0.5),
            (MathOperation::Offset, 0.5 + 2.0),
            (MathOperation::Scale, 0.5 * 2.0),
        ];
        for (operation, expected) in cases {
            let config = MathConfig {
                operation,
                amount: Scalar::constant(2.0),
            };
            assert_eq!(
                config.process_mono(0.5, -0.25, &[]),
                expected,
                "{operation:?}"
            );
            assert_eq!(
                config.process_stereo([0.5, 0.5], [-0.25, -0.25], &[]),
                [expected, expected]
            );
        }
    }

    fn shaper(curve: ShaperCurve) -> ShaperConfig {
        ShaperConfig {
            curve,
            drive: Scalar::constant(1.0),
            bias: Scalar::constant(0.0),
            asymmetry: Scalar::constant(0.5),
            points: [(0.0, 0.0); MAX_SHAPER_POINTS],
            point_count: 0,
        }
    }

    #[test]
    fn shaper_curves_follow_their_laws() {
        assert!((shaper(ShaperCurve::Tanh).process(0.5, &[]) - 0.5_f32.tanh()).abs() < 1.0e-7);
        assert_eq!(shaper(ShaperCurve::HardClip).process(3.0, &[]), 1.0);
        assert_eq!(shaper(ShaperCurve::HardClip).process(-0.25, &[]), -0.25);
        assert_eq!(shaper(ShaperCurve::SoftClip).process(4.0, &[]), 1.0);
        assert_eq!(shaper(ShaperCurve::SoftClip).process(-4.0, &[]), -1.0);
        assert_eq!(shaper(ShaperCurve::OneSided).process(-0.75, &[]), 0.0);
        assert_eq!(shaper(ShaperCurve::OneSided).process(0.75, &[]), 0.75);
        let asymmetric = shaper(ShaperCurve::Asymmetric);
        assert!(asymmetric.process(4.0, &[]) > 0.99);
        assert!((asymmetric.process(-4.0, &[]) + 0.4).abs() < 0.01);
        let mut biased = shaper(ShaperCurve::OneSided);
        biased.drive = Scalar::constant(2.0);
        biased.bias = Scalar::constant(-0.5);
        assert_eq!(biased.process(0.5, &[]), 0.5);
        assert_eq!(biased.process(0.2, &[]), 0.0);
    }

    #[test]
    fn shaper_table_interpolates_and_extrapolates_flat() {
        let mut table = shaper(ShaperCurve::Table);
        for (index, point) in [(-1.0, -0.5), (0.0, 0.25), (1.0, -1.0)].iter().enumerate() {
            table.points[index] = *point;
        }
        table.point_count = 3;
        assert_eq!(table.process(-3.0, &[]), -0.5);
        assert_eq!(table.process(-0.5, &[]), -0.125);
        assert_eq!(table.process(0.0, &[]), 0.25);
        assert_eq!(table.process(0.5, &[]), -0.375);
        assert_eq!(table.process(9.0, &[]), -1.0);
        assert_eq!(table.process(f32::NAN, &[]), 0.0);
    }

    fn biquad(mode: BiquadMode, frequency: f64, q: f64, gain: f64) -> BiquadConfig {
        BiquadConfig {
            mode,
            frequency: Scalar::constant(frequency),
            q: Scalar::constant(q),
            gain: Scalar::constant(gain),
            key_track: Scalar::constant(0.0),
            key_reference: Scalar::constant(261.625_565),
        }
    }

    /// Steady-state gain in dB of a sine at `hz` through `config`.
    fn sine_gain(config: BiquadConfig, hz: f64, key: Option<f32>) -> f64 {
        let mut state = BiquadState::default();
        let (mut input, mut output) = (0.0, 0.0);
        for frame in 0..48_000 {
            let x = (2.0 * PI * hz * f64::from(frame) / 48_000.0).sin();
            let y = f64::from(config.process(
                &mut state,
                x as f32,
                key.unwrap_or(0.0),
                key.is_some(),
                &[],
                48_000.0,
            ));
            if frame >= 24_000 {
                input += x * x;
                output += y * y;
            }
        }
        10.0 * (output / input).log10()
    }

    #[test]
    fn biquad_responses_match_their_cookbook_shapes() {
        let lowpass = biquad(BiquadMode::Lowpass, 1_000.0, FRAC_1_SQRT_2, 0.0);
        assert!(sine_gain(lowpass, 100.0, None).abs() < 0.05);
        assert!((sine_gain(lowpass, 1_000.0, None) + 3.01).abs() < 0.1);
        assert!(sine_gain(lowpass, 8_000.0, None) < -34.0);
        let highpass = biquad(BiquadMode::Highpass, 1_000.0, FRAC_1_SQRT_2, 0.0);
        assert!(sine_gain(highpass, 100.0, None) < -38.0);
        let bandpass = biquad(BiquadMode::Bandpass, 1_000.0, 4.0, 0.0);
        assert!(sine_gain(bandpass, 1_000.0, None).abs() < 0.05);
        assert!(sine_gain(bandpass, 3_000.0, None) < -12.0);
        let notch = biquad(BiquadMode::Notch, 1_000.0, 2.0, 0.0);
        assert!(sine_gain(notch, 1_000.0, None) < -40.0);
        let peak = biquad(BiquadMode::Peak, 2_000.0, 3.0, 9.0);
        assert!((sine_gain(peak, 2_000.0, None) - 9.0).abs() < 0.1);
        assert!(sine_gain(peak, 200.0, None).abs() < 0.2);
        let low_shelf = biquad(BiquadMode::LowShelf, 300.0, FRAC_1_SQRT_2, -6.0);
        assert!((sine_gain(low_shelf, 40.0, None) + 6.0).abs() < 0.2);
        assert!(sine_gain(low_shelf, 6_000.0, None).abs() < 0.2);
        let high_shelf = biquad(BiquadMode::HighShelf, 3_000.0, FRAC_1_SQRT_2, 6.0);
        assert!((sine_gain(high_shelf, 15_000.0, None) - 6.0).abs() < 0.3);
    }

    #[test]
    fn biquad_allpass_keeps_every_level() {
        let allpass = biquad(BiquadMode::Allpass, 2_000.0, 0.7, 0.0);
        for hz in [50.0, 500.0, 2_000.0, 9_000.0, 18_000.0] {
            assert!(sine_gain(allpass, hz, None).abs() < 0.02, "{hz}");
        }
    }

    #[test]
    fn biquad_frequency_follows_the_key() {
        let mut tracked = biquad(BiquadMode::Bandpass, 1_000.0, 8.0, 0.0);
        tracked.key_track = Scalar::constant(1.0);
        // An octave above the reference doubles the centre frequency.
        assert!(sine_gain(tracked, 2_000.0, Some(523.251_13)).abs() < 0.05);
        assert!(sine_gain(tracked, 1_000.0, Some(523.251_13)) < -15.0);
    }

    #[test]
    fn biquad_recovers_from_non_finite_input() {
        let config = biquad(BiquadMode::Lowpass, 1_000.0, 1.0, 0.0);
        let mut state = BiquadState::default();
        assert_eq!(
            config.process(&mut state, f32::NAN, 0.0, false, &[], 48_000.0),
            0.0
        );
        assert!(
            config
                .process(&mut state, 1.0, 0.0, false, &[], 48_000.0)
                .is_finite()
        );
    }

    #[test]
    fn onepole_leak_turns_an_impulse_into_an_exact_exponential() {
        let config = OnePoleConfig {
            mode: OnePoleMode::Leak,
            cutoff: Scalar::constant(1_000.0),
            time: Scalar::constant(0.25),
        };
        let mut state = OnePoleState::default();
        let mut output = 0.0;
        for frame in 0..=12_000 {
            output = config.process(&mut state, f32::from(frame == 0), &[], 48_000.0);
        }
        // 12,000 frames is exactly one e-folding time.
        assert!(
            (f64::from(output) - (-1.0_f64).exp()).abs() < 1.0e-5,
            "{output}"
        );
    }

    #[test]
    fn onepole_lowpass_and_highpass_split_a_signal() {
        let low = OnePoleConfig {
            mode: OnePoleMode::Lowpass,
            cutoff: Scalar::constant(500.0),
            time: Scalar::constant(0.1),
        };
        let high = OnePoleConfig {
            mode: OnePoleMode::Highpass,
            ..low
        };
        let (mut a, mut b) = (OnePoleState::default(), OnePoleState::default());
        for frame in 0..4_800 {
            let x = (f64::from(frame) * 0.37).sin() as f32;
            let sum =
                low.process(&mut a, x, &[], 48_000.0) + high.process(&mut b, x, &[], 48_000.0);
            assert!((sum - x).abs() < 1.0e-5);
        }
        let mut steady = 0.0;
        for _ in 0..48_000 {
            steady = low.process(&mut a, 1.0, &[], 48_000.0);
        }
        assert!((steady - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn dc_block_removes_offset() {
        let config = DcBlockConfig {
            cutoff: Scalar::constant(20.0),
        };
        let mut state = DcBlockState::default();
        let mut output = 1.0;
        for _ in 0..48_000 {
            output = config.process(&mut state, 1.0, &[], 48_000.0);
        }
        assert!(output.abs() < 1.0e-6, "{output}");
    }

    #[test]
    fn linear_phase_lowpass_delays_every_frequency_by_one_frame() {
        for amount in [0.0, 0.4, 1.0] {
            let config = LinearPhaseLowpassConfig {
                amount: Scalar::constant(amount),
            };
            let mut state = LinearPhaseLowpassState::default();
            let response = (0..3)
                .map(|frame| config.process(&mut state, f32::from(frame == 0), &[]))
                .collect::<Vec<_>>();
            assert_eq!(response[0], response[2]);
            assert!((response.iter().sum::<f32>() - 1.0).abs() < 1.0e-6);
            assert!(response[1] >= response[0]);
        }
    }
}
