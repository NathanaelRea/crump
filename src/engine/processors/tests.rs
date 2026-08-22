use std::f64::consts::PI;

use crate::model::ProcessorKind;

use super::super::{
    control::ControlIndex,
    oversampling::{
        OVERSAMPLING_FACTOR, OVERSAMPLING_FILTER_TAPS, OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES,
        OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES, OversamplingInterpolator,
    },
};
use super::runtime::ProcessorState;
use super::*;

fn test_program(kind: ProcessorKind, config: ProcessorConfig) -> ProcessorProgram {
    ProcessorProgram {
        id: "test".to_owned(),
        kind,
        sidechain_inputs: Vec::new(),
        bypass: Scalar::Constant(0.0),
        authored_bypass: false,
        authored_latency_frames: 0,
        declared_latency_frames: 0,
        config,
        sample_rate: 48_000,
    }
}

fn eq_band(kind: EqBandKind, frequency: f64, gain: f64, q: f64) -> EqBandProgram {
    EqBandProgram {
        kind,
        frequency: Scalar::Constant(frequency),
        gain: Scalar::Constant(gain),
        q: Scalar::Constant(q),
    }
}

#[derive(Debug, Clone, Copy)]
struct TestCompressor {
    threshold: f64,
    ratio: f64,
    attack: f64,
    release: f64,
    knee: f64,
    makeup: f64,
    mix: f64,
    rms: bool,
}

impl Default for TestCompressor {
    fn default() -> Self {
        Self {
            threshold: -24.0,
            ratio: 4.0,
            attack: 0.001,
            release: 0.1,
            knee: 0.0,
            makeup: 0.0,
            mix: 1.0,
            rms: false,
        }
    }
}

impl TestCompressor {
    fn runtime(self) -> ProcessorRuntime {
        test_program(
            ProcessorKind::Compressor,
            ProcessorConfig::Compressor {
                threshold: Scalar::Constant(self.threshold),
                ratio: Scalar::Constant(self.ratio),
                attack: Scalar::Constant(self.attack),
                release: Scalar::Constant(self.release),
                knee: Scalar::Constant(self.knee),
                makeup: Scalar::Constant(self.makeup),
                mix: Scalar::Constant(self.mix),
                rms: self.rms,
            },
        )
        .runtime()
    }
}

#[derive(Debug, Clone, Copy)]
struct TestReverb {
    pre_delay: f64,
    diffusion: f64,
    decay: f64,
    damping: f64,
    modulation: f64,
    width: f64,
    mix: f64,
}

impl Default for TestReverb {
    fn default() -> Self {
        Self {
            pre_delay: 0.0,
            diffusion: 0.7,
            decay: 0.65,
            damping: 0.35,
            modulation: 0.1,
            width: 1.0,
            mix: 1.0,
        }
    }
}

impl TestReverb {
    fn runtime(self) -> ProcessorRuntime {
        test_program(
            ProcessorKind::Reverb,
            ProcessorConfig::Reverb {
                pre_delay: Scalar::Constant(self.pre_delay),
                diffusion: Scalar::Constant(self.diffusion),
                decay: Scalar::Constant(self.decay),
                damping: Scalar::Constant(self.damping),
                modulation: Scalar::Constant(self.modulation),
                width: Scalar::Constant(self.width),
                mix: Scalar::Constant(self.mix),
            },
        )
        .runtime()
    }

    fn impulse_from(self, initial: [f32; 2], frames: usize) -> Vec<[f32; 2]> {
        let mut runtime = self.runtime();
        (0..frames)
            .map(|frame| {
                let input = if frame == 0 { initial } else { [0.0, 0.0] };
                runtime.process(input, None, &[])
            })
            .collect()
    }

    fn impulse(self, frames: usize) -> Vec<[f32; 2]> {
        self.impulse_from([1.0, 1.0], frames)
    }
}

fn tone_amplitude(samples: &[f32], frequency: f64) -> f64 {
    let (sine, cosine) =
        samples
            .iter()
            .enumerate()
            .fold((0.0_f64, 0.0_f64), |(sine, cosine), (frame, sample)| {
                let phase = 2.0 * PI * frequency * frame as f64 / 48_000.0;
                (
                    sine + f64::from(*sample) * phase.sin(),
                    cosine + f64::from(*sample) * phase.cos(),
                )
            });
    2.0 * sine.hypot(cosine) / samples.len() as f64
}

fn reconstructed_true_peak(samples: &[f32]) -> f64 {
    let mut interpolator = OversamplingInterpolator::default();
    samples
        .iter()
        .flat_map(|sample| interpolator.process([*sample, *sample]))
        .skip(usize::from(OVERSAMPLING_FILTER_TAPS) * usize::from(OVERSAMPLING_FACTOR))
        .flatten()
        .map(|sample| f64::from(sample.abs()))
        .fold(0.0_f64, f64::max)
}

fn measured_eq_gain(band: EqBandProgram, test_frequency: f64) -> f64 {
    let mut runtime =
        test_program(ProcessorKind::Eq, ProcessorConfig::Eq { bands: vec![band] }).runtime();
    let mut input_energy = 0.0_f64;
    let mut output_energy = 0.0_f64;
    for frame in 0..48_000 {
        let phase = 2.0 * PI * test_frequency * frame as f64 / 48_000.0;
        let input = (phase.sin() * 0.1) as f32;
        let output = runtime.process([input, input], None, &[])[0];
        if frame >= 12_000 {
            input_energy += f64::from(input).powi(2);
            output_energy += f64::from(output).powi(2);
        }
    }
    (output_energy / input_energy).sqrt()
}

fn stereo_correlation(samples: &[[f32; 2]]) -> f64 {
    let cross = samples
        .iter()
        .map(|sample| f64::from(sample[0]) * f64::from(sample[1]))
        .sum::<f64>();
    let left = samples
        .iter()
        .map(|sample| f64::from(sample[0]).powi(2))
        .sum::<f64>();
    let right = samples
        .iter()
        .map(|sample| f64::from(sample[1]).powi(2))
        .sum::<f64>();
    cross / (left * right).sqrt()
}

#[test]
fn utility_gain_polarity_mono_and_width_have_explicit_safe_semantics() {
    let mut narrowing = test_program(
        ProcessorKind::Utility,
        ProcessorConfig::Utility {
            gain: Scalar::Constant(0.0),
            width: Scalar::Constant(0.25),
            invert: false,
            mono: false,
        },
    )
    .runtime();
    let input = [[1.0, -0.5], [0.25, 0.75], [-0.8, -0.2], [0.1, -0.9]];
    let output = input.map(|sample| narrowing.process(sample, None, &[]));
    for (input, output) in input.into_iter().zip(output) {
        let input_mid = (input[0] + input[1]) * 0.5;
        let input_side = (input[0] - input[1]) * 0.5;
        let output_mid = (output[0] + output[1]) * 0.5;
        let output_side = (output[0] - output[1]) * 0.5;
        assert!((output_mid - input_mid).abs() < 1.0e-6);
        assert!((output_side - input_side * 0.25).abs() < 1.0e-6);
    }
    assert!(stereo_correlation(&output) >= stereo_correlation(&input));

    let mut mono_inverted = test_program(
        ProcessorKind::Utility,
        ProcessorConfig::Utility {
            gain: Scalar::Constant(6.0),
            width: Scalar::Constant(1.0),
            invert: true,
            mono: true,
        },
    )
    .runtime();
    let processed = mono_inverted.process([1.0, -0.5], None, &[]);
    let expected = -0.25 * db_gain(6.0);
    assert!((processed[0] - expected).abs() < 1.0e-6);
    assert!((processed[1] - expected).abs() < 1.0e-6);
}

#[test]
fn every_eq_band_has_the_expected_frequency_response() {
    let high_pass_low = measured_eq_gain(
        eq_band(
            EqBandKind::HighPass,
            500.0,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        50.0,
    );
    let high_pass_high = measured_eq_gain(
        eq_band(
            EqBandKind::HighPass,
            500.0,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        5_000.0,
    );
    assert!(high_pass_low < 0.02 && high_pass_high > 0.99);

    let low_pass_low = measured_eq_gain(
        eq_band(
            EqBandKind::LowPass,
            2_000.0,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        200.0,
    );
    let low_pass_high = measured_eq_gain(
        eq_band(
            EqBandKind::LowPass,
            2_000.0,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        15_000.0,
    );
    assert!(low_pass_low > 0.99 && low_pass_high < 0.02);

    let low_shelf_low = measured_eq_gain(eq_band(EqBandKind::LowShelf, 500.0, 6.0, 1.0), 50.0);
    let low_shelf_high = measured_eq_gain(eq_band(EqBandKind::LowShelf, 500.0, 6.0, 1.0), 5_000.0);
    assert!(low_shelf_low > 1.9 && (0.95..1.05).contains(&low_shelf_high));

    let high_shelf_low = measured_eq_gain(eq_band(EqBandKind::HighShelf, 5_000.0, 6.0, 1.0), 500.0);
    let high_shelf_high =
        measured_eq_gain(eq_band(EqBandKind::HighShelf, 5_000.0, 6.0, 1.0), 15_000.0);
    assert!((0.95..1.05).contains(&high_shelf_low) && high_shelf_high > 1.9);

    let peak_center = measured_eq_gain(eq_band(EqBandKind::Peak, 1_000.0, 6.0, 1.0), 1_000.0);
    let peak_far = measured_eq_gain(eq_band(EqBandKind::Peak, 1_000.0, 6.0, 1.0), 100.0);
    assert!(peak_center > 1.9 && (0.95..1.05).contains(&peak_far));
    let wide_flank = measured_eq_gain(eq_band(EqBandKind::Peak, 1_000.0, 6.0, 0.5), 1_500.0);
    let narrow_flank = measured_eq_gain(eq_band(EqBandKind::Peak, 1_000.0, 6.0, 5.0), 1_500.0);
    assert!(wide_flank > narrow_flank + 0.2);
}

#[test]
fn every_eq_band_has_a_finite_decaying_impulse_response() {
    for band in [
        eq_band(EqBandKind::HighPass, 1_000.0, 0.0, 0.707),
        eq_band(EqBandKind::LowShelf, 500.0, 6.0, 1.0),
        eq_band(EqBandKind::Peak, 1_000.0, -6.0, 2.0),
        eq_band(EqBandKind::HighShelf, 5_000.0, 6.0, 1.0),
        eq_band(EqBandKind::LowPass, 4_000.0, 0.0, 0.707),
    ] {
        let mut runtime =
            test_program(ProcessorKind::Eq, ProcessorConfig::Eq { bands: vec![band] }).runtime();
        let mut last = 0.0_f32;
        for frame in 0..16_384 {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            last = runtime.process([input, input], None, &[])[0];
            assert!(last.is_finite());
        }
        assert!(last.abs() < 1.0e-6);
    }
}

#[test]
fn eq_contract_boundaries_remain_finite_and_bounded() {
    let mut bands = Vec::new();
    for frequency in [10.0, 23_520.0] {
        bands.push(eq_band(EqBandKind::HighPass, frequency, 0.0, 0.707));
        bands.push(eq_band(EqBandKind::LowPass, frequency, 0.0, 0.707));
        for gain in [-24.0, 24.0] {
            bands.push(eq_band(EqBandKind::LowShelf, frequency, gain, 1.0));
            bands.push(eq_band(EqBandKind::HighShelf, frequency, gain, 1.0));
            for q in [0.1, 20.0] {
                bands.push(eq_band(EqBandKind::Peak, frequency, gain, q));
            }
        }
    }
    for band in bands {
        let coefficients = BiquadCoefficients::for_band(
            band.kind,
            band.frequency.value(&[]),
            band.gain.value(&[]),
            band.q.value(&[]),
            48_000,
        );
        for value in [
            coefficients.b0,
            coefficients.b1,
            coefficients.b2,
            coefficients.a1,
            coefficients.a2,
        ] {
            assert!(value.is_finite());
        }
        let mut runtime =
            test_program(ProcessorKind::Eq, ProcessorConfig::Eq { bands: vec![band] }).runtime();
        for frame in 0..48_000 {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            let output = runtime.process([input, -input], None, &[]);
            assert!(output.into_iter().all(|sample| sample.is_finite()));
            assert!(output.into_iter().all(|sample| sample.abs() < 1.0e6));
        }
    }
}

#[test]
fn eq_output_is_independent_of_processing_quantum() {
    fn render(quantum: usize) -> Vec<[f32; 2]> {
        let bands = vec![
            eq_band(EqBandKind::HighPass, 30.0, 0.0, 0.707),
            eq_band(EqBandKind::LowShelf, 120.0, 2.0, 1.0),
            eq_band(EqBandKind::Peak, 500.0, -2.0, 0.8),
            eq_band(EqBandKind::Peak, 2_500.0, 1.5, 1.2),
            eq_band(EqBandKind::HighShelf, 8_000.0, -1.0, 1.0),
            eq_band(EqBandKind::LowPass, 20_000.0, 0.0, 0.707),
        ];
        let mut runtime = test_program(ProcessorKind::Eq, ProcessorConfig::Eq { bands }).runtime();
        let input = (0..8_192)
            .map(|frame| {
                let phase = frame as f64 / 48_000.0;
                [
                    (0.3 * (2.0 * PI * 110.0 * phase).sin()
                        + 0.1 * (2.0 * PI * 3_100.0 * phase).sin()) as f32,
                    (0.2 * (2.0 * PI * 220.0 * phase).sin()
                        - 0.1 * (2.0 * PI * 7_000.0 * phase).sin()) as f32,
                ]
            })
            .collect::<Vec<_>>();
        let mut output = Vec::with_capacity(input.len());
        for block in input.chunks(quantum) {
            output.extend(
                block
                    .iter()
                    .map(|sample| runtime.process(*sample, None, &[])),
            );
        }
        output
    }

    let baseline = render(64);
    assert_eq!(render(128), baseline);
    assert_eq!(render(256), baseline);
}

#[test]
fn delay_read_interpolates_fractional_frames() {
    let mut delay = DelayState::new(8);
    delay.cursor = 4;
    delay.buffer[3] = [1.0, 0.0];
    delay.buffer[2] = [0.0, 2.0];
    assert_eq!(delay.read(1.25), [0.75, 0.5]);
}

#[test]
fn delay_ping_pong_crossfeeds_feedback_while_stereo_does_not() {
    fn echoes(mode: DelayMode) -> [[f32; 2]; 3] {
        let mut delay = DelayState::new(16);
        let mut output = [[0.0, 0.0]; 3];
        for (frame, sample) in [[1.0, 0.0], [0.0, 0.0], [0.0, 0.0]].into_iter().enumerate() {
            output[frame] = delay.process(
                sample,
                1.0 / 48_000.0,
                0.5,
                1.0,
                23_520.0,
                10.0,
                0.001,
                mode,
                16,
                48_000,
            );
        }
        output
    }

    let stereo = echoes(DelayMode::Stereo);
    let ping_pong = echoes(DelayMode::PingPong);
    assert!(stereo[1][0] > 0.9 && ping_pong[1][0] > 0.9);
    assert!(stereo[2][0].abs() > stereo[2][1].abs() * 5.0);
    assert!(ping_pong[2][1].abs() > ping_pong[2][0].abs() * 5.0);
}

#[test]
fn delay_feedback_filters_have_expected_frequency_selectivity() {
    fn measured_gain(frequency: f64, low_pass: f64, high_pass: f64) -> f64 {
        let mut delay = DelayState::new(16);
        let mut input_energy = 0.0_f64;
        let mut output_energy = 0.0_f64;
        for frame in 0..48_000 {
            let input = (2.0 * PI * frequency * frame as f64 / 48_000.0).sin() as f32;
            let output = delay.process(
                [input, input],
                1.0 / 48_000.0,
                0.0,
                1.0,
                low_pass,
                high_pass,
                0.001,
                DelayMode::Stereo,
                16,
                48_000,
            )[0];
            if frame >= 4_800 {
                input_energy += f64::from(input).powi(2);
                output_energy += f64::from(output).powi(2);
            }
        }
        (output_energy / input_energy).sqrt()
    }

    fn repeat_ratio(frequency: f64) -> f64 {
        const BURST_FRAMES: usize = 1_024;
        const DELAY_FRAMES: usize = 2_048;
        let mut delay = DelayState::new(DELAY_FRAMES + 2);
        let mut first_energy = 0.0_f64;
        let mut second_energy = 0.0_f64;
        for frame in 0..(DELAY_FRAMES * 2 + BURST_FRAMES) {
            let input = if frame < BURST_FRAMES {
                let window = (PI * frame as f64 / BURST_FRAMES as f64).sin().powi(2);
                (window * (2.0 * PI * frequency * frame as f64 / 48_000.0).sin()) as f32
            } else {
                0.0
            };
            let output = delay.process(
                [input, input],
                DELAY_FRAMES as f64 / 48_000.0,
                0.8,
                1.0,
                1_000.0,
                10.0,
                0.001,
                DelayMode::Stereo,
                DELAY_FRAMES + 2,
                48_000,
            )[0];
            if (DELAY_FRAMES..DELAY_FRAMES + BURST_FRAMES).contains(&frame) {
                first_energy += f64::from(output).powi(2);
            }
            if (DELAY_FRAMES * 2..DELAY_FRAMES * 2 + BURST_FRAMES).contains(&frame) {
                second_energy += f64::from(output).powi(2);
            }
        }
        (second_energy / first_energy).sqrt()
    }

    let low_pass_low = measured_gain(200.0, 1_000.0, 10.0);
    let low_pass_high = measured_gain(10_000.0, 1_000.0, 10.0);
    assert!(low_pass_low > low_pass_high * 5.0);
    let high_pass_low = measured_gain(200.0, 23_520.0, 1_000.0);
    let high_pass_high = measured_gain(10_000.0, 23_520.0, 1_000.0);
    assert!(high_pass_high > high_pass_low * 4.0);
    assert!(repeat_ratio(200.0) > repeat_ratio(10_000.0) * 5.0);
}

#[test]
fn delay_time_change_crossfade_avoids_an_abrupt_read_head_jump() {
    let mut delay = DelayState::new(64);
    let old_time = 10.0 / 48_000.0;
    let new_time = 34.0 / 48_000.0;
    let mut frame = 0_usize;
    let previous = loop {
        let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
        let output = delay.process(
            [input, input],
            old_time,
            0.0,
            1.0,
            23_520.0,
            10.0,
            0.001,
            DelayMode::Stereo,
            64,
            48_000,
        )[0];
        frame += 1;
        if frame > 128 && output > 0.85 {
            break output;
        }
    };
    let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
    let mut abrupt = delay.clone();
    abrupt.current_delay_frames = 34.0;
    let abrupt_output = abrupt.process(
        [input, input],
        new_time,
        0.0,
        1.0,
        23_520.0,
        10.0,
        0.001,
        DelayMode::Stereo,
        64,
        48_000,
    )[0];
    let safe_output = delay.process(
        [input, input],
        new_time,
        0.0,
        1.0,
        23_520.0,
        10.0,
        0.001,
        DelayMode::Stereo,
        64,
        48_000,
    )[0];
    assert!((safe_output - previous).abs() < 0.2);
    assert!((abrupt_output - previous).abs() > 1.5);
    for _ in 1..48 {
        frame += 1;
        let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
        let output = delay.process(
            [input, input],
            new_time,
            0.0,
            1.0,
            23_520.0,
            10.0,
            0.001,
            DelayMode::Stereo,
            64,
            48_000,
        );
        assert!(output.into_iter().all(f32::is_finite));
    }
    assert_eq!(delay.transition_frames, 0);
    assert!((delay.current_delay_frames - 34.0).abs() < 1.0e-9);
}

#[test]
fn delay_remains_finite_at_maximum_feedback_during_repeated_time_changes() {
    let mut delay = DelayState::new(258);
    let mut maximum = 0.0_f32;
    for frame in 0..200_000 {
        let delay_frames = [1.25, 17.5, 127.75][(frame / 997) % 3];
        let input = if frame == 0 { [1.0, -1.0] } else { [0.0, 0.0] };
        let output = delay.process(
            input,
            delay_frames / 48_000.0,
            0.999,
            1.0,
            23_520.0,
            10.0,
            0.001,
            DelayMode::PingPong,
            258,
            48_000,
        );
        assert!(output.into_iter().all(f32::is_finite));
        maximum = maximum.max(output[0].abs()).max(output[1].abs());
    }
    assert!(maximum <= 1.0);
}

#[test]
fn transient_bypass_is_latency_aligned_smoothed_and_monitoring_only() {
    let mut program = test_program(
        ProcessorKind::Utility,
        ProcessorConfig::Utility {
            gain: Scalar::Constant(-12.0),
            width: Scalar::Constant(1.0),
            invert: false,
            mono: false,
        },
    );
    program.authored_latency_frames = 5;
    program.declared_latency_frames = 5;
    let mut runtime = program.clone().runtime();
    runtime.set_transient_bypass(Some(true));
    for _ in 0..BYPASS_GUARD_FRAMES as usize {
        assert_eq!(
            runtime.process_with_audition([0.0, 0.0], None, &[], true),
            [0.0, 0.0]
        );
    }
    let (_, transient, effective, mix) = runtime.bypass_state(&[], true);
    assert_eq!(transient, Some(true));
    assert!(effective);
    assert_eq!(mix, 1.0);
    for frame in 0..=5 {
        let input = if frame == 0 { [1.0, -1.0] } else { [0.0, 0.0] };
        let output = runtime.process_with_audition(input, None, &[], true);
        if frame < 5 {
            assert_eq!(output, [0.0, 0.0]);
        } else {
            assert_eq!(output, [1.0, -1.0]);
        }
    }

    for _ in 0..8 {
        let _ = runtime.process_with_audition([1.0, 1.0], None, &[], true);
    }
    runtime.set_transient_bypass(None);
    let mut previous = runtime.process_with_audition([1.0, 1.0], None, &[], true)[0];
    for _ in 1..BYPASS_GUARD_FRAMES as usize + 8 {
        let next = runtime.process_with_audition([1.0, 1.0], None, &[], true)[0];
        assert!((next - previous).abs() <= 0.02, "{previous} -> {next}");
        previous = next;
    }
    let (_, transient, effective, mix) = runtime.bypass_state(&[], true);
    assert_eq!(transient, None);
    assert!(!effective);
    assert_eq!(mix, 0.0);

    let mut offline = program.runtime();
    offline.set_transient_bypass(Some(true));
    for _ in 0..5 {
        assert_eq!(
            offline.process_with_audition([0.0, 0.0], None, &[], false),
            [0.0, 0.0]
        );
    }
    let output = offline.process_with_audition([1.0, 1.0], None, &[], false);
    assert_eq!(output, [0.0, 0.0]);
    let output = offline.process_with_audition([0.0, 0.0], None, &[], false);
    assert_eq!(output, [0.0, 0.0]);
    for _ in 0..3 {
        let _ = offline.process_with_audition([0.0, 0.0], None, &[], false);
    }
    let output = offline.process_with_audition([0.0, 0.0], None, &[], false);
    let wet = db_gain(-12.0);
    assert_eq!(output, [wet, wet]);
}

#[test]
fn durable_bypass_control_is_sample_exact_without_the_monitoring_guard() {
    let mut program = test_program(
        ProcessorKind::Utility,
        ProcessorConfig::Utility {
            gain: Scalar::Constant(-12.0),
            width: Scalar::Constant(1.0),
            invert: false,
            mono: false,
        },
    );
    program.bypass = Scalar::Control(ControlIndex(0));
    let wet = db_gain(-12.0);
    for audition in [false, true] {
        let mut runtime = program.clone().runtime();
        assert_eq!(
            runtime.process_with_audition([1.0, -1.0], None, &[0.0], audition),
            [wet, -wet]
        );
        assert_eq!(
            runtime.process_with_audition([1.0, -1.0], None, &[1.0], audition),
            [1.0, -1.0]
        );
        assert_eq!(
            runtime.process_with_audition([1.0, -1.0], None, &[0.0], audition),
            [wet, -wet]
        );
    }
}

#[test]
fn algorithmic_latency_bypass_is_exact_across_supported_quanta() {
    let program = test_program(
        ProcessorKind::Saturation,
        ProcessorConfig::Saturation {
            drive: Scalar::Constant(6.0),
            mix: Scalar::Constant(1.0),
            output: Scalar::Constant(0.0),
            mode: SaturationMode::Tape,
        },
    );
    let latency = program.output_latency_frames() as usize;
    assert!(latency > 0);
    let input = (0..1_024)
        .map(|frame| {
            let left = ((frame as f32 + 1.0) / 1_025.0) * 0.8;
            [left, -left]
        })
        .collect::<Vec<_>>();
    let render = |quantum: usize| {
        let mut runtime = program.clone().runtime();
        let mut output = Vec::with_capacity(input.len());
        for block in input.chunks(quantum) {
            for sample in block {
                let frame = output.len();
                if frame == 256 {
                    runtime.set_transient_bypass(Some(true));
                } else if frame == 512 {
                    runtime.set_transient_bypass(Some(false));
                }
                output.push(runtime.process_with_audition(*sample, None, &[], true));
            }
        }
        output
    };
    let reference = render(64);
    assert_eq!(reference[400], input[400 - latency]);
    for quantum in [128, 256] {
        assert_eq!(render(quantum), reference, "callback quantum {quantum}");
    }
}

#[test]
fn delay_and_reverb_tails_continue_while_transiently_bypassed() {
    let programs = [
        test_program(
            ProcessorKind::Delay,
            ProcessorConfig::Delay {
                time: Scalar::Constant(0.002),
                feedback: Scalar::Constant(0.95),
                mix: Scalar::Constant(1.0),
                low_pass: Scalar::Constant(23_520.0),
                high_pass: Scalar::Constant(10.0),
                time_crossfade: Scalar::Constant(0.001),
                mode: DelayMode::Stereo,
                maximum_frames: 512,
            },
        ),
        test_program(
            ProcessorKind::Reverb,
            ProcessorConfig::Reverb {
                pre_delay: Scalar::Constant(0.0),
                diffusion: Scalar::Constant(0.7),
                decay: Scalar::Constant(0.9),
                damping: Scalar::Constant(0.25),
                modulation: Scalar::Constant(0.1),
                width: Scalar::Constant(1.0),
                mix: Scalar::Constant(1.0),
            },
        ),
    ];
    for program in programs {
        let mut baseline = program.clone().runtime();
        let mut audition = program.runtime();
        let mut heard_retained_tail = false;
        for frame in 0..20_000 {
            let input = if frame % 37 == 0 {
                [0.35, -0.2]
            } else {
                [0.0, 0.0]
            };
            if frame == 64 {
                audition.set_transient_bypass(Some(true));
            } else if frame == 192 {
                audition.set_transient_bypass(Some(false));
            }
            let expected = baseline.process_with_audition(input, None, &[], true);
            let actual = audition.process_with_audition(input, None, &[], true);
            if frame >= 256 {
                assert_eq!(actual, expected, "{:?} frame {frame}", program.kind);
                heard_retained_tail |= expected.into_iter().any(|sample| sample.abs() > 1.0e-6);
            }
        }
        assert!(
            heard_retained_tail,
            "{:?} tail must remain active after re-enable",
            program.kind
        );
    }
}

#[test]
fn declared_delay_is_exact() {
    let mut delay = StereoDelay::new(3);
    assert_eq!(delay.process([1.0, -1.0]), [0.0, 0.0]);
    assert_eq!(delay.process([0.0, 0.0]), [0.0, 0.0]);
    assert_eq!(delay.process([0.0, 0.0]), [0.0, 0.0]);
    assert_eq!(delay.process([0.0, 0.0]), [1.0, -1.0]);
}

#[test]
fn reverb_diffuser_is_a_gain_neutral_allpass() {
    let mut dc = AllpassDelay::new(17);
    let mut output = 0.0;
    for _ in 0..20_000 {
        output = dc.process(1.0, 0.75);
    }
    assert!((output - 1.0).abs() < 1.0e-5);

    let mut impulse = AllpassDelay::new(17);
    let energy = (0..100_000)
        .map(|frame| {
            let output = impulse.process(if frame == 0 { 1.0 } else { 0.0 }, 0.75);
            f64::from(output).powi(2)
        })
        .sum::<f64>();
    assert!((energy - 1.0).abs() < 1.0e-5);

    for frequency in [100.0, 5_000.0, 20_000.0] {
        let mut allpass = AllpassDelay::new(17);
        let mut input_energy = 0.0_f64;
        let mut output_energy = 0.0_f64;
        for frame in 0..96_000 {
            let input = (2.0 * PI * frequency * frame as f64 / 48_000.0).sin() as f32;
            let output = allpass.process(input, 0.75);
            if frame >= 48_000 {
                input_energy += f64::from(input).powi(2);
                output_energy += f64::from(output).powi(2);
            }
        }
        let gain = (output_energy / input_energy).sqrt();
        assert!((gain - 1.0).abs() < 1.0e-4, "{frequency} Hz: {gain}");
    }
}

#[test]
fn reverb_zero_pre_delay_reaches_the_network_without_a_ring_wrap() {
    let mut reverb = ReverbState::new(48_000);
    let allocated_bytes = reverb.pre_delay.buffer.capacity() as u64
        * std::mem::size_of::<[f32; 2]>() as u64
        + (reverb.left_combs.capacity() + reverb.right_combs.capacity()) as u64
            * std::mem::size_of::<MonoDelay>() as u64
        + (reverb.left_diffusers.capacity() + reverb.right_diffusers.capacity()) as u64
            * std::mem::size_of::<AllpassDelay>() as u64
        + reverb
            .left_combs
            .iter()
            .chain(&reverb.right_combs)
            .map(|delay| delay.buffer.capacity() as u64 * std::mem::size_of::<f32>() as u64)
            .sum::<u64>()
        + reverb
            .left_diffusers
            .iter()
            .chain(&reverb.right_diffusers)
            .map(|delay| delay.buffer.capacity() as u64 * std::mem::size_of::<f32>() as u64)
            .sum::<u64>();
    assert_eq!(reverb_heap_bytes(48_000), allocated_bytes);
    let program = test_program(
        ProcessorKind::Reverb,
        ProcessorConfig::Reverb {
            pre_delay: Scalar::Constant(0.0),
            diffusion: Scalar::Constant(0.7),
            decay: Scalar::Constant(0.65),
            damping: Scalar::Constant(0.35),
            modulation: Scalar::Constant(0.1),
            width: Scalar::Constant(1.0),
            mix: Scalar::Constant(0.25),
        },
    );
    assert_eq!(
        program.state_bytes(),
        std::mem::size_of::<ProcessorRuntime>() as u64
            + allocated_bytes
            + 2 * std::mem::size_of::<[f32; 2]>() as u64
    );
    assert_eq!(program.work_units(), 160);

    let first_wet_frame = (0..5_000).find(|frame| {
        let input = if *frame == 0 { [1.0, 1.0] } else { [0.0, 0.0] };
        let output = reverb.process(input, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 48_000);
        output[0].abs().max(output[1].abs()) > 1.0e-6
    });
    assert!(
        first_wet_frame.is_some_and(|frame| frame < 5_000),
        "zero pre-delay must not wait for the one-second pre-delay ring to wrap"
    );
}

#[test]
fn reverb_pre_delay_shifts_the_wet_onset_by_the_authored_frames() {
    let first_wet_frame = |response: &[[f32; 2]]| {
        response
            .iter()
            .position(|sample| sample[0].abs().max(sample[1].abs()) > 1.0e-7)
            .expect("wet onset")
    };
    let immediate = TestReverb {
        pre_delay: 0.0,
        ..TestReverb::default()
    }
    .impulse(8_000);
    let delayed = TestReverb {
        pre_delay: 0.01,
        ..TestReverb::default()
    }
    .impulse(8_480);
    let maximum = TestReverb {
        pre_delay: 1.0,
        ..TestReverb::default()
    }
    .impulse(56_000);
    assert_eq!(first_wet_frame(&delayed) - first_wet_frame(&immediate), 480);
    assert_eq!(
        first_wet_frame(&maximum) - first_wet_frame(&immediate),
        48_000
    );
}

#[test]
fn reverb_pre_delay_automation_crossfades_between_fractional_taps() {
    let mut delay = ReverbPreDelay::new(48_000);
    let old_time = 10.0 / 48_000.0;
    let new_time = 34.5 / 48_000.0;
    let mut frame = 0_usize;
    let previous = loop {
        let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
        let output = delay.process([input, input], old_time, 48_000)[0];
        frame += 1;
        if frame > 128 && output > 0.85 {
            break output;
        }
    };
    let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
    let mut abrupt = delay.clone();
    abrupt.current_frames = 34.5;
    let abrupt_output = abrupt.process([input, input], new_time, 48_000)[0];
    let safe_output = delay.process([input, input], new_time, 48_000)[0];
    assert!((safe_output - previous).abs() < 0.2);
    assert!((abrupt_output - previous).abs() > 1.5);
    for _ in 1..480 {
        frame += 1;
        let input = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32;
        let output = delay.process([input, input], new_time, 48_000);
        assert!(output.into_iter().all(f32::is_finite));
    }
    assert_eq!(delay.transition_frames, 0);
    assert!((delay.current_frames - 34.5).abs() < 1.0e-9);

    let mut fractional = ReverbPreDelay::new(48_000);
    let response = (0..40)
        .map(|frame| {
            fractional.process(
                if frame == 0 { [1.0, -1.0] } else { [0.0, 0.0] },
                34.5 / 48_000.0,
                48_000,
            )
        })
        .collect::<Vec<_>>();
    assert!((response[34][0] - 0.5).abs() < 1.0e-7);
    assert!((response[35][0] - 0.5).abs() < 1.0e-7);
    assert!((response[34][1] + 0.5).abs() < 1.0e-7);
    assert!((response[35][1] + 0.5).abs() < 1.0e-7);
}

#[test]
fn reverb_comb_modulation_moves_a_fractional_delay_tap() {
    let onset = |offset: f64| {
        let mut delay = MonoDelay::new(100);
        (0..120)
            .find(|frame| {
                let input = if *frame == 0 { 1.0 } else { 0.0 };
                delay.process(input, 0.0, 0.0, offset).abs() > 1.0e-7
            })
            .expect("modulated comb onset")
    };
    assert_eq!(onset(0.0), 100);
    assert_eq!(onset(8.0), 108);

    let mut fractional = MonoDelay::new(100);
    let response = (0..103)
        .map(|frame| fractional.process(if frame == 0 { 1.0 } else { 0.0 }, 0.0, 0.0, 0.5))
        .collect::<Vec<_>>();
    assert!((response[100] - 0.5).abs() < 1.0e-7);
    assert!((response[101] - 0.5).abs() < 1.0e-7);
}

#[test]
fn reverb_diffusion_increases_echo_density_without_changing_topology() {
    let sparse = TestReverb {
        diffusion: 0.0,
        decay: 0.4,
        damping: 0.0,
        modulation: 0.0,
        ..TestReverb::default()
    }
    .impulse(48_000);
    let dense = TestReverb {
        diffusion: 1.0,
        decay: 0.4,
        damping: 0.0,
        modulation: 0.0,
        ..TestReverb::default()
    }
    .impulse(48_000);
    let active_frames = |response: &[[f32; 2]]| {
        response
            .iter()
            .skip(2_000)
            .filter(|sample| sample[0].abs().max(sample[1].abs()) > 1.0e-7)
            .count()
    };
    assert!(active_frames(&dense) > active_frames(&sparse) * 4);
}

#[test]
fn reverb_decay_and_damping_shape_the_tail() {
    let short = TestReverb {
        decay: 0.1,
        damping: 0.2,
        modulation: 0.0,
        ..TestReverb::default()
    }
    .impulse(192_000);
    let long = TestReverb {
        decay: 0.9,
        damping: 0.2,
        modulation: 0.0,
        ..TestReverb::default()
    }
    .impulse(192_000);
    let energy = |response: &[[f32; 2]], start: usize| {
        response[start..]
            .iter()
            .map(|sample| f64::from(sample[0]).powi(2) + f64::from(sample[1]).powi(2))
            .sum::<f64>()
    };
    let short_tail = energy(&short, 96_000);
    let long_tail = energy(&long, 96_000);
    assert!(
        long_tail > short_tail * 100.0,
        "short={short_tail:e}, long={long_tail:e}"
    );
    assert!(long_tail > 1.0e-12, "long={long_tail:e}");

    let bright = TestReverb {
        decay: 0.85,
        damping: 0.0,
        modulation: 0.0,
        ..TestReverb::default()
    }
    .impulse(96_000);
    let dark = TestReverb {
        decay: 0.85,
        damping: 0.85,
        modulation: 0.0,
        ..TestReverb::default()
    }
    .impulse(96_000);
    let roughness = |response: &[[f32; 2]]| {
        let samples = &response[4_800..];
        let signal = samples
            .iter()
            .map(|sample| f64::from(sample[0]).powi(2) + f64::from(sample[1]).powi(2))
            .sum::<f64>();
        let derivative = samples
            .windows(2)
            .map(|pair| {
                f64::from(pair[1][0] - pair[0][0]).powi(2)
                    + f64::from(pair[1][1] - pair[0][1]).powi(2)
            })
            .sum::<f64>();
        derivative / signal
    };
    assert!(roughness(&dark) < roughness(&bright));
}

#[test]
fn reverb_modulation_is_deterministic_and_bounded() {
    let unmodulated = TestReverb {
        decay: 0.9,
        modulation: 0.0,
        ..TestReverb::default()
    }
    .impulse(192_000);
    let config = TestReverb {
        decay: 0.9,
        modulation: 1.0,
        ..TestReverb::default()
    };
    let modulated = config.impulse(192_000);
    assert_eq!(modulated, config.impulse(192_000));
    let difference = modulated
        .iter()
        .zip(&unmodulated)
        .map(|(left, right)| {
            f64::from(left[0] - right[0]).powi(2) + f64::from(left[1] - right[1]).powi(2)
        })
        .sum::<f64>();
    assert!(difference > 1.0e-8);
    assert!(modulated.iter().flatten().all(|sample| sample.is_finite()));
    assert!(modulated.iter().flatten().all(|sample| sample.abs() <= 1.0));
}

#[test]
fn reverb_width_preserves_mono_fold_down_and_zero_width_is_mono() {
    let mono = TestReverb {
        width: 0.0,
        modulation: 0.0,
        ..TestReverb::default()
    }
    .impulse(96_000);
    let wide = TestReverb {
        width: 1.0,
        modulation: 0.0,
        ..TestReverb::default()
    }
    .impulse(96_000);
    assert!(mono.iter().all(|sample| sample[0] == sample[1]));
    let side_energy = wide
        .iter()
        .map(|sample| f64::from(sample[0] - sample[1]).powi(2))
        .sum::<f64>();
    assert!(side_energy > 1.0e-8);
    let side_only = TestReverb {
        width: 1.0,
        modulation: 0.0,
        ..TestReverb::default()
    }
    .impulse_from([1.0, -1.0], 96_000);
    let side_only_energy = side_only
        .iter()
        .map(|sample| f64::from(sample[0]).powi(2) + f64::from(sample[1]).powi(2))
        .sum::<f64>();
    assert!(side_only_energy > 1.0e-8);
    for (mono, wide) in mono.iter().zip(&wide) {
        let folded = (wide[0] + wide[1]) * 0.5;
        assert!((mono[0] - folded).abs() < 1.0e-7);
    }
}

#[test]
fn reverb_maximum_controls_remain_finite_with_a_long_tail() {
    let mut runtime = TestReverb {
        diffusion: 1.0,
        decay: 1.0,
        damping: 0.0,
        modulation: 1.0,
        width: 1.0,
        mix: 1.0,
        ..TestReverb::default()
    }
    .runtime();
    let mut maximum = 0.0_f32;
    let mut late_maximum = 0.0_f32;
    for frame in 0..576_000 {
        let input = if frame == 0 { [1.0, 1.0] } else { [0.0, 0.0] };
        let output = runtime.process(input, None, &[]);
        assert!(output.into_iter().all(f32::is_finite));
        maximum = maximum.max(output[0].abs()).max(output[1].abs());
        if frame >= 480_000 {
            late_maximum = late_maximum.max(output[0].abs()).max(output[1].abs());
        }
    }
    assert!(maximum <= 1.0);
    assert!(late_maximum > 1.0e-7);
}

#[test]
fn reverb_is_bounded_for_sustained_dc_and_low_frequency_input() {
    let mut runtime = TestReverb {
        diffusion: 1.0,
        decay: 1.0,
        damping: 0.0,
        modulation: 1.0,
        width: 1.0,
        mix: 1.0,
        ..TestReverb::default()
    }
    .runtime();
    let mut maximum = 0.0_f32;
    for frame in 0..480_000 {
        let low = (2.0 * PI * 20.0 * frame as f64 / 48_000.0).sin() as f32 * 0.2;
        let output = runtime.process([0.3 + low, 0.3 - low], None, &[]);
        assert!(output.into_iter().all(f32::is_finite));
        maximum = maximum.max(output[0].abs()).max(output[1].abs());
    }
    assert!(maximum <= 2.0);
    for _ in 0..480_000 {
        let output = runtime.process([0.0, 0.0], None, &[]);
        assert!(output.into_iter().all(f32::is_finite));
        maximum = maximum.max(output[0].abs()).max(output[1].abs());
    }
    assert!(maximum <= 2.0);
}

#[test]
fn state_bytes_match_delay_runtime_allocations() {
    let program = ProcessorProgram {
        id: "delay".to_owned(),
        kind: ProcessorKind::Delay,
        sidechain_inputs: Vec::new(),
        bypass: Scalar::Constant(0.0),
        authored_bypass: false,
        authored_latency_frames: 3,
        declared_latency_frames: 3,
        config: ProcessorConfig::Delay {
            time: Scalar::Constant(0.25),
            feedback: Scalar::Constant(0.25),
            mix: Scalar::Constant(0.25),
            low_pass: Scalar::Constant(20_000.0),
            high_pass: Scalar::Constant(20.0),
            time_crossfade: Scalar::Constant(0.01),
            mode: DelayMode::Stereo,
            maximum_frames: 17,
        },
        sample_rate: 48_000,
    };
    let expected = program.state_bytes();
    let runtime = program.runtime();
    let ProcessorState::Delay(delay) = &runtime.state else {
        panic!("Delay state");
    };
    let allocated = std::mem::size_of::<ProcessorRuntime>() as u64
        + runtime.declared_delay.buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64
        + runtime.bypass_delay.buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64
        + delay.buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64;
    assert_eq!(expected, allocated);
}

#[test]
fn compressor_curve_honors_threshold_ratio_and_soft_knee() {
    assert_eq!(compressor_curve_reduction_db(-24.0, -24.0, 4.0, 0.0), 0.0);
    assert_eq!(compressor_curve_reduction_db(-12.0, -24.0, 4.0, 0.0), 9.0);
    assert_eq!(compressor_curve_reduction_db(0.0, -24.0, 1.0, 0.0), 0.0);
    assert_eq!(compressor_curve_reduction_db(-28.0, -24.0, 4.0, 8.0), 0.0);
    assert_eq!(compressor_curve_reduction_db(-20.0, -24.0, 4.0, 8.0), 3.0);
    assert_eq!(compressor_curve_reduction_db(-24.0, -24.0, 4.0, 8.0), 0.75);
    let below_center = compressor_curve_reduction_db(-24.001, -24.0, 4.0, 8.0);
    let above_center = compressor_curve_reduction_db(-23.999, -24.0, 4.0, 8.0);
    assert!(below_center < 0.75 && above_center > 0.75);
}

#[test]
fn compressor_peak_and_rms_detectors_have_distinct_linked_stereo_levels() {
    fn settled_reduction(rms: bool) -> f32 {
        let mut runtime = TestCompressor {
            rms,
            ..TestCompressor::default()
        }
        .runtime();
        for _ in 0..96_000 {
            runtime.process([1.0, 0.0], None, &[]);
        }
        runtime.gain_reduction_db().expect("compressor telemetry")
    }

    let peak = settled_reduction(false);
    let rms = settled_reduction(true);
    assert!((peak - 18.0).abs() < 0.01);
    assert!((rms - 15.742).abs() < 0.01);
    assert!(peak > rms + 2.2);
}

#[test]
fn compressor_envelope_matches_attack_and_release_vectors() {
    let mut runtime = TestCompressor::default().runtime();
    let mut attack_vector = Vec::new();
    for frame in 0..480 {
        runtime.process([0.25, -0.25], Some([1.0, 1.0]), &[]);
        if matches!(frame, 0 | 47 | 479) {
            attack_vector.push(runtime.gain_reduction_db().expect("attack telemetry"));
        }
    }
    for (actual, expected) in attack_vector.into_iter().zip([0.0, 15.012, 18.0]) {
        assert!((actual - expected).abs() < 0.02, "{actual} != {expected}");
    }

    let mut release_vector = Vec::new();
    for frame in 0..14_400 {
        runtime.process([0.25, -0.25], Some([0.0, 0.0]), &[]);
        if matches!(frame, 0 | 479 | 4_799 | 14_399) {
            release_vector.push(runtime.gain_reduction_db().expect("release telemetry"));
        }
    }
    for (actual, expected) in release_vector
        .into_iter()
        .zip([17.998, 17.348, 11.485, 0.0])
    {
        assert!((actual - expected).abs() < 0.02, "{actual} != {expected}");
    }
}

#[test]
fn compressor_wet_dry_mix_is_parallel_and_exact_at_endpoints() {
    fn settled_output(mix: f64) -> [f32; 2] {
        let mut runtime = TestCompressor {
            mix,
            ..TestCompressor::default()
        }
        .runtime();
        let mut output = [0.0, 0.0];
        for _ in 0..48_000 {
            output = runtime.process([0.25, -0.25], Some([1.0, 1.0]), &[]);
        }
        output
    }

    let dry = settled_output(0.0);
    let half = settled_output(0.5);
    let wet = settled_output(1.0);
    assert_eq!(dry, [0.25, -0.25]);
    assert!(wet[0] < 0.04 && wet[1] > -0.04);
    for channel in 0..2 {
        assert!((half[channel] - (dry[channel] + wet[channel]) * 0.5).abs() < 1.0e-6);
    }
}

#[test]
fn compressor_uses_external_sidechain_instead_of_program_audio() {
    fn settled(sidechain: Option<[f32; 2]>) -> ([f32; 2], f32) {
        let mut runtime = TestCompressor::default().runtime();
        let mut output = [0.0, 0.0];
        for _ in 0..48_000 {
            output = runtime.process([0.05, -0.05], sidechain, &[]);
        }
        (
            output,
            runtime.gain_reduction_db().expect("compressor telemetry"),
        )
    }

    let (program_detected, program_reduction) = settled(None);
    let (sidechain_detected, sidechain_reduction) = settled(Some([1.0, 1.0]));
    assert!(program_reduction < 1.0e-4);
    assert!(sidechain_reduction > 17.9);
    assert!(sidechain_detected[0] < program_detected[0] * 0.14);
    assert!(sidechain_detected[1] > program_detected[1] * 0.14);
}

#[test]
fn compressor_reduction_telemetry_excludes_makeup_gain() {
    fn settle(makeup: f64) -> ([f32; 2], f32) {
        let program = ProcessorProgram {
            id: "compressor".to_owned(),
            kind: ProcessorKind::Compressor,
            sidechain_inputs: Vec::new(),
            bypass: Scalar::Constant(0.0),
            authored_bypass: false,
            authored_latency_frames: 0,
            declared_latency_frames: 0,
            config: ProcessorConfig::Compressor {
                threshold: Scalar::Constant(-24.0),
                ratio: Scalar::Constant(4.0),
                attack: Scalar::Constant(0.001),
                release: Scalar::Constant(0.1),
                knee: Scalar::Constant(0.0),
                makeup: Scalar::Constant(makeup),
                mix: Scalar::Constant(1.0),
                rms: false,
            },
            sample_rate: 48_000,
        };
        let mut runtime = program.runtime();
        let mut output = [0.0, 0.0];
        for _ in 0..48_000 {
            output = runtime.process([1.0, -1.0], None, &[]);
        }
        (
            output,
            runtime.gain_reduction_db().expect("compressor telemetry"),
        )
    }

    let (without_makeup, reduction_without_makeup) = settle(0.0);
    let (with_makeup, reduction_with_makeup) = settle(12.0);
    assert!(reduction_without_makeup > 17.9);
    assert!((reduction_without_makeup - reduction_with_makeup).abs() < 1.0e-6);
    assert!(with_makeup[0] > without_makeup[0] * 3.9);
    assert!(with_makeup[1] < without_makeup[1] * 3.9);
}

#[test]
fn saturation_filtered_oversampling_suppresses_aliasing_and_preserves_thd() {
    let input = (0..96_000)
        .map(|frame| (2.0 * PI * 10_000.0 * frame as f64 / 48_000.0).sin() as f32 * 0.8)
        .collect::<Vec<_>>();
    for mode in [
        SaturationMode::SoftClip,
        SaturationMode::Tanh,
        SaturationMode::Tape,
    ] {
        let base_rate = input[48_000..]
            .iter()
            .map(|sample| saturate(*sample * 6.0, mode))
            .collect::<Vec<_>>();
        let mut runtime = test_program(
            ProcessorKind::Saturation,
            ProcessorConfig::Saturation {
                drive: Scalar::Constant(6.0),
                mix: Scalar::Constant(1.0),
                output: Scalar::Constant(0.0),
                mode,
            },
        )
        .runtime();
        let filtered = input
            .iter()
            .map(|sample| runtime.process([*sample, *sample], None, &[])[0])
            .skip(48_000)
            .collect::<Vec<_>>();
        let base_alias = tone_amplitude(&base_rate, 18_000.0);
        let filtered_alias = tone_amplitude(&filtered, 18_000.0);
        let filtered_fundamental = tone_amplitude(&filtered, 10_000.0);
        assert!(base_alias > 0.01, "{mode:?}: base alias={base_alias:e}");
        assert!(
            filtered_alias < base_alias * 0.03,
            "{mode:?}: base={base_alias:e}, filtered={filtered_alias:e}"
        );
        assert!(filtered_fundamental > 0.2, "{mode:?}");
    }

    let mut signatures = Vec::new();
    for mode in [
        SaturationMode::SoftClip,
        SaturationMode::Tanh,
        SaturationMode::Tape,
    ] {
        let mut runtime = test_program(
            ProcessorKind::Saturation,
            ProcessorConfig::Saturation {
                drive: Scalar::Constant(4.0),
                mix: Scalar::Constant(1.0),
                output: Scalar::Constant(0.0),
                mode,
            },
        )
        .runtime();
        let response = (0..96_000)
            .map(|frame| {
                let sample = (2.0 * PI * 1_000.0 * frame as f64 / 48_000.0).sin() as f32 * 0.25;
                runtime.process([sample, sample], None, &[])[0]
            })
            .skip(48_000)
            .collect::<Vec<_>>();
        assert!(response.iter().all(|sample| sample.is_finite()));
        let fundamental = tone_amplitude(&response, 1_000.0);
        let harmonic_energy = (2..=10)
            .map(|harmonic| tone_amplitude(&response, f64::from(harmonic) * 1_000.0).powi(2))
            .sum::<f64>();
        let thd = harmonic_energy.sqrt() / fundamental;
        assert!(thd > 0.01, "{mode:?}: THD={thd:e}");
        signatures.push(thd);
    }
    assert!(
        signatures
            .windows(2)
            .all(|pair| (pair[0] - pair[1]).abs() > 1.0e-4)
    );
}

#[test]
fn saturation_aligns_dry_mix_and_reports_exact_state() {
    let program = test_program(
        ProcessorKind::Saturation,
        ProcessorConfig::Saturation {
            drive: Scalar::Constant(10.0),
            mix: Scalar::Constant(0.0),
            output: Scalar::Constant(0.0),
            mode: SaturationMode::SoftClip,
        },
    );
    assert_eq!(
        program.output_latency_frames(),
        OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES as u32
    );
    let expected = program.state_bytes();
    let mut runtime = program.runtime();
    let allocated = std::mem::size_of::<ProcessorRuntime>() as u64
        + runtime.declared_delay.buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64
        + runtime.bypass_delay.buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64
        + match &runtime.state {
            ProcessorState::Saturation(state) => {
                state.dry_delay.buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64
            }
            _ => unreachable!("Saturation state"),
        };
    assert_eq!(expected, allocated);
    for frame in 0..=OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES {
        let input = if frame == 0 { [1.0, -1.0] } else { [0.0, 0.0] };
        let output = runtime.process(input, None, &[]);
        if frame < OVERSAMPLING_ROUND_TRIP_LATENCY_FRAMES {
            assert_eq!(output, [0.0, 0.0]);
        } else {
            assert_eq!(output, [1.0, -1.0]);
        }
    }
}

#[test]
fn nonlinear_processors_are_exact_across_block_grouping_and_finite_at_bounds() {
    let input = (0..16_384)
        .map(|frame| {
            let tone = (2.0 * PI * 11_731.0 * frame as f64 / 48_000.0).sin() as f32;
            let impulse = if frame % 997 == 0 { 8.0 } else { 0.0 };
            [tone * 4.0 + impulse, tone * -3.0 - impulse]
        })
        .collect::<Vec<_>>();
    let programs = [
        test_program(
            ProcessorKind::Saturation,
            ProcessorConfig::Saturation {
                drive: Scalar::Constant(10.0),
                mix: Scalar::Constant(1.0),
                output: Scalar::Constant(24.0),
                mode: SaturationMode::Tape,
            },
        ),
        test_program(
            ProcessorKind::Limiter,
            ProcessorConfig::Limiter {
                ceiling: Scalar::Constant(-24.0),
                release: Scalar::Constant(2.0),
                lookahead_frames: 960,
            },
        ),
    ];
    let process = |program: &ProcessorProgram, quantum: usize| {
        let mut runtime = program.runtime();
        let mut output = Vec::with_capacity(input.len());
        for block in input.chunks(quantum) {
            output.extend(
                block
                    .iter()
                    .map(|sample| runtime.process(*sample, None, &[])),
            );
        }
        output
    };
    for program in &programs {
        let reference = process(program, 64);
        assert!(reference.iter().flatten().all(|sample| sample.is_finite()));
        assert_eq!(reference, process(program, 128));
        assert_eq!(reference, process(program, 256));
    }
}

#[test]
fn limiter_latency_includes_detector_filter_and_authored_lookahead() {
    let mut limiter = LimiterState::new(1);
    let latency = OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES + 1;
    for frame in 0..=latency {
        let input = if frame == 0 { [1.0, -1.0] } else { [0.0, 0.0] };
        let output = limiter.process(input, 0.8, 0.1, 48_000);
        if frame < latency {
            assert_eq!(output, [0.0, 0.0]);
        } else {
            let expected = 0.8 * LIMITER_TRUE_PEAK_SAFETY_RATIO;
            assert!((output[0] - expected).abs() < 1.0e-4, "{output:?}");
            assert!((output[1] + expected).abs() < 1.0e-4, "{output:?}");
        }
    }
}

#[test]
fn limiter_controls_an_intersample_peak_above_its_sample_peak() {
    let ceiling = 0.9_f32;
    let lookahead = 32;
    let mut limiter = LimiterState::new(lookahead);
    let mut output = Vec::with_capacity(96_000);
    let mut input_sample_peak = 0.0_f32;
    for frame in 0..96_000 {
        let sample = (2.0 * PI * 12_000.0 * frame as f64 / 48_000.0 + std::f64::consts::FRAC_PI_4)
            .sin() as f32
            * 1.2;
        input_sample_peak = input_sample_peak.max(sample.abs());
        output.push(limiter.process([sample, sample], ceiling, 0.08, 48_000)[0]);
    }
    assert!(input_sample_peak < ceiling);
    let steady = &output[48_000..];
    let output_true_peak = reconstructed_true_peak(steady);
    assert!(
        output_true_peak <= f64::from(ceiling) * 1.001,
        "true peak={output_true_peak:e}"
    );
    assert!(output_true_peak > 0.8);
}

#[test]
fn limiter_controls_reconstructed_output_while_gain_changes() {
    let ceiling = 0.8_f32;
    let mut limiter = LimiterState::new(64);
    let output = (0..96_000)
        .map(|frame| {
            let amplitude = match (frame / 257) % 4 {
                0 => 0.1,
                1 => 2.0,
                2 => 0.35,
                _ => 1.4,
            };
            let carrier = (2.0 * PI * 11_731.0 * frame as f64 / 48_000.0 + 0.37).sin() as f32;
            let impulse = if frame % 997 == 0 { 2.5 } else { 0.0 };
            limiter.process(
                [carrier * amplitude + impulse, carrier * amplitude - impulse],
                ceiling,
                0.02,
                48_000,
            )[0]
        })
        .collect::<Vec<_>>();
    let true_peak = reconstructed_true_peak(&output);
    assert!(
        true_peak <= f64::from(ceiling) * 1.001,
        "gain-changing reconstructed peak={true_peak:e}"
    );
}

#[test]
fn limiter_release_recovers_without_changing_declared_latency_or_state() {
    let recovered_gain = |release: f64| {
        let mut limiter = LimiterState::new(32);
        for _ in 0..2_000 {
            let _ = limiter.process([2.0, -2.0], 0.8, release, 48_000);
        }
        assert!(limiter.gain < 0.41);
        for _ in 0..4_800 {
            let _ = limiter.process([0.1, -0.1], 0.8, release, 48_000);
        }
        limiter.gain
    };
    let fast = recovered_gain(0.01);
    let slow = recovered_gain(0.5);
    assert!(fast > 0.99, "fast={fast}");
    assert!(slow < 0.6, "slow={slow}");
    assert!(fast > slow);

    let program = test_program(
        ProcessorKind::Limiter,
        ProcessorConfig::Limiter {
            ceiling: Scalar::Constant(-1.0),
            release: Scalar::Constant(0.08),
            lookahead_frames: 32,
        },
    );
    assert_eq!(
        program.output_latency_frames(),
        (32 + OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES) as u32
    );
    let expected = program.state_bytes();
    let runtime = program.runtime();
    let ProcessorState::Limiter(state) = &runtime.state else {
        unreachable!("Limiter state")
    };
    let allocated = std::mem::size_of::<ProcessorRuntime>() as u64
        + runtime.declared_delay.buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64
        + runtime.bypass_delay.buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64
        + state.audio_buffer.capacity() as u64 * std::mem::size_of::<[f32; 2]>() as u64
        + state.peak_buffer.capacity() as u64 * std::mem::size_of::<f32>() as u64;
    assert_eq!(expected, allocated);
}

#[test]
fn limiter_never_exceeds_sample_ceiling() {
    let mut limiter = LimiterState::new(4);
    let latency = 4 + OVERSAMPLING_INTERPOLATOR_LATENCY_FRAMES;
    let mut outputs = Vec::new();
    for frame in 0..(latency + 8) {
        let sample = if frame == 0 { [2.0, -2.0] } else { [0.0, 0.0] };
        outputs.push(limiter.process(sample, 0.8, 0.1, 48_000));
    }
    assert!(
        outputs
            .iter()
            .flatten()
            .all(|sample| sample.abs() <= 0.800_1)
    );
    assert!(outputs.iter().flatten().any(|sample| sample.abs() > 0.78));
}
