use super::*;

#[test]
fn noise_is_centered_unit_range_and_scales_with_level() {
    for seed in [0, 1, 91473] {
        let full = NoiseConfig {
            level: Scalar::constant(1.0),
        };
        let quiet = NoiseConfig {
            level: Scalar::constant(0.25),
        };
        let mut a = NoiseState::new(seed);
        let mut b = NoiseState::new(seed);
        let mut sum = 0.0_f64;
        let mut square = 0.0_f64;
        let mut lag = 0.0_f64;
        let mut last = 0.0_f64;
        let mut positive = 0;
        for _ in 0..65536 {
            let x = full.process(&mut a, &[]);
            assert!((-1.0..1.0).contains(&x));
            assert_eq!(quiet.process(&mut b, &[]), x * 0.25);
            let x = f64::from(x);
            sum += x;
            square += x * x;
            lag += x * last;
            last = x;
            positive += usize::from(x > 0.0);
        }
        assert!((sum / 65536.0).abs() < 0.01);
        assert!((square / 65536.0 - 1.0 / 3.0).abs() < 0.01);
        assert!(lag.abs() / square < 0.02);
        assert!((31000..34500).contains(&positive));
        a.retrigger(seed);
        b.retrigger(seed);
        let allocations = allocation_counter::measure(|| {
            for _ in 0..1024 {
                assert_eq!(full.process(&mut a, &[]), full.process(&mut b, &[]));
            }
        });
        assert_eq!(allocations.count_total, 0);
    }
}

fn analog_config(voices: usize) -> AnalogOscillatorConfig {
    AnalogOscillatorConfig {
        waveform: OscillatorWaveform::Saw,
        voices,
        coarse: Scalar::constant(0.0),
        fine: Scalar::constant(0.0),
        detune: Scalar::constant(0.12),
        blend: Scalar::constant(1.0),
        width: Scalar::constant(0.8),
        drift: Scalar::constant(0.0),
        spread: UnisonSpread::Even,
        drift_shape: DriftShape::Sine,
        glide: Scalar::constant(0.0),
        fm_depth: Scalar::constant(12.0),
        pm_depth: Scalar::constant(0.2),
        amplitude_mode: AnalogAmplitudeMode::Am,
        amplitude_depth: Scalar::constant(1.0),
        phase: Scalar::constant(0.0),
        random_phase: Scalar::constant(1.0),
        warps: [None, None],
        distortion: AnalogDistortion::None,
        drive: Scalar::constant(0.0),
        velocity_amount: Scalar::constant(0.0),
        level: Scalar::constant(1.0),
        pan: Scalar::constant(0.0),
        node_identity: 0x1234,
    }
}

fn render(config: AnalogOscillatorConfig, seed: u64, frames: usize) -> Vec<[f32; 2]> {
    let mut state = AnalogOscillatorState::new(seed, config.node_identity);
    (0..frames)
        .map(|_| {
            config.process(
                &mut state,
                220.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                1.0,
                false,
                false,
                false,
                false,
                false,
                false,
                &[],
                48_000.0,
            )
        })
        .collect()
}

#[test]
fn polyblep_saw_suppresses_folded_alias_vectors() {
    const SAMPLE_RATE: f64 = 48_000.0;
    const FREQUENCY: f64 = 7_000.0;
    const FRAMES: usize = 48_000;
    let increment = FREQUENCY / SAMPLE_RATE;
    let mut phase = 0.0_f64;
    let mut naive = Vec::with_capacity(FRAMES);
    let mut band_limited = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        naive.push(phase * 2.0 - 1.0);
        band_limited.push(oscillator_sample(OscillatorWaveform::Saw, phase, increment));
        phase = (phase + increment).fract();
    }
    let amplitude = |samples: &[f64], frequency: f64| {
        let (real, imaginary) = samples.iter().enumerate().fold(
            (0.0_f64, 0.0_f64),
            |(real, imaginary), (frame, sample)| {
                let angle = std::f64::consts::TAU * frequency * frame as f64 / SAMPLE_RATE;
                (
                    real + sample * angle.cos(),
                    imaginary + sample * angle.sin(),
                )
            },
        );
        (real.hypot(imaginary) * 2.0) / samples.len() as f64
    };
    // 28/35/42 kHz harmonics of a 7 kHz naive saw fold to these bins.
    let alias_bins = [20_000.0, 13_000.0, 6_000.0];
    let naive_alias = alias_bins
        .into_iter()
        .map(|frequency| amplitude(&naive, frequency))
        .sum::<f64>();
    let limited_alias = alias_bins
        .into_iter()
        .map(|frequency| amplitude(&band_limited, frequency))
        .sum::<f64>();
    assert!(
        limited_alias < naive_alias * 0.3,
        "limited={limited_alias}, naive={naive_alias}"
    );
    assert!(amplitude(&band_limited, FREQUENCY) > 0.5);
}

#[test]
fn analog_unison_is_deterministic_finite_and_power_bounded() {
    let one = render(analog_config(1), 41, 16_384);
    let eight = render(analog_config(8), 41, 16_384);
    assert_eq!(eight, render(analog_config(8), 41, 16_384));
    assert!(eight.iter().flatten().all(|sample| sample.is_finite()));
    let rms = |samples: &[[f32; 2]]| {
        (samples
            .iter()
            .skip(512)
            .map(|sample| f64::from(sample[0]).powi(2) + f64::from(sample[1]).powi(2))
            .sum::<f64>()
            / (samples.len() - 512) as f64)
            .sqrt()
    };
    let ratio = rms(&eight) / rms(&one);
    assert!((0.55..=1.8).contains(&ratio), "power ratio {ratio}");
    assert!(
        eight
            .iter()
            .map(|sample| (sample[0] - sample[1]).abs())
            .sum::<f32>()
            > 1.0
    );
}

#[test]
fn analog_blend_am_ring_pan_and_node_identity_are_explicit() {
    let mut dry = analog_config(4);
    dry.random_phase = Scalar::constant(0.0);
    dry.phase = Scalar::constant(0.25);
    dry.blend = Scalar::constant(0.0);
    let dry_sample = render(dry, 77, 1)[0];
    assert!((dry_sample[0] - dry_sample[1]).abs() < 1.0e-6);

    let mut wide = dry;
    wide.blend = Scalar::constant(1.0);
    let wide_render = render(wide, 77, 256);
    assert!(
        wide_render
            .iter()
            .map(|sample| (sample[0] - sample[1]).abs())
            .sum::<f32>()
            > 0.1
    );

    let mut ring = analog_config(1);
    ring.random_phase = Scalar::constant(0.0);
    ring.phase = Scalar::constant(0.25);
    ring.amplitude_mode = AnalogAmplitudeMode::Ring;
    ring.amplitude_depth = Scalar::constant(1.0);
    ring.pan = Scalar::constant(1.0);
    let mut state = AnalogOscillatorState::new(9, ring.node_identity);
    let silent = ring.process(
        &mut state,
        220.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        false,
        false,
        false,
        true,
        false,
        false,
        &[],
        48_000.0,
    );
    assert_eq!(silent, [0.0, 0.0]);
    let audible = ring.process(
        &mut state,
        220.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        1.0,
        false,
        false,
        false,
        true,
        false,
        false,
        &[],
        48_000.0,
    );
    assert!(audible[1].abs() > 0.1);
    assert!(audible[0].abs() < 1.0e-6);

    let mut other_identity = wide;
    other_identity.random_phase = Scalar::constant(1.0);
    let first = render(other_identity, 5, 64);
    other_identity.node_identity ^= 0x55aa;
    assert_ne!(first, render(other_identity, 5, 64));
}

#[test]
fn analog_cross_modulation_warp_order_sync_and_distortion_are_effective() {
    let mut forward = analog_config(2);
    forward.random_phase = Scalar::constant(0.0);
    forward.warps = [
        Some(AnalogWarpStage {
            kind: AnalogPhaseWarp::PulseWidth,
            amount: Scalar::constant(0.3),
        }),
        Some(AnalogWarpStage {
            kind: AnalogPhaseWarp::Remap,
            amount: Scalar::constant(0.2),
        }),
    ];
    forward.distortion = AnalogDistortion::Tanh;
    forward.drive = Scalar::constant(0.4);
    let mut reverse = forward;
    reverse.warps.reverse();
    assert_ne!(render(forward, 7, 512), render(reverse, 7, 512));

    let mut dry_state = AnalogOscillatorState::new(99, forward.node_identity);
    let mut modulated_state = AnalogOscillatorState::new(99, forward.node_identity);
    let dry = forward.process(
        &mut dry_state,
        220.0,
        0.0,
        0.0,
        -1.0,
        0.0,
        1.0,
        1.0,
        true,
        true,
        true,
        false,
        false,
        false,
        &[],
        48_000.0,
    );
    let modulated = forward.process(
        &mut modulated_state,
        220.0,
        0.5,
        -0.4,
        -1.0,
        0.0,
        1.0,
        1.0,
        true,
        true,
        true,
        false,
        false,
        false,
        &[],
        48_000.0,
    );
    assert_ne!(dry, modulated);
    assert!(modulated.into_iter().all(f32::is_finite));

    for _ in 0..64 {
        let _ = forward.process(
            &mut dry_state,
            220.0,
            0.0,
            0.0,
            -1.0,
            0.0,
            1.0,
            1.0,
            false,
            false,
            true,
            false,
            false,
            false,
            &[],
            48_000.0,
        );
    }
    let reset = forward.process(
        &mut dry_state,
        220.0,
        0.0,
        0.0,
        1.0,
        0.0,
        1.0,
        1.0,
        false,
        false,
        true,
        false,
        false,
        false,
        &[],
        48_000.0,
    );
    let mut reference = AnalogOscillatorState::new(99, forward.node_identity);
    let reference = forward.process(
        &mut reference,
        220.0,
        0.0,
        0.0,
        1.0,
        0.0,
        1.0,
        1.0,
        false,
        false,
        true,
        false,
        false,
        false,
        &[],
        48_000.0,
    );
    assert_eq!(reset, reference);
}

#[test]
fn analog_distortion_precedes_level_unison_blend_and_pan() {
    let mut loud = analog_config(1);
    loud.waveform = OscillatorWaveform::Sine;
    loud.phase = Scalar::constant(0.125);
    loud.random_phase = Scalar::constant(0.0);
    loud.distortion = AnalogDistortion::Tanh;
    loud.drive = Scalar::constant(0.8);
    let mut quiet = loud;
    quiet.level = Scalar::constant(0.25);

    let loud = render(loud, 77, 1)[0];
    let quiet = render(quiet, 77, 1)[0];
    assert!(loud[0].abs() > 0.1);
    assert!((quiet[0] / loud[0] - 0.25).abs() < 1.0e-6);
    assert!((quiet[1] / loud[1] - 0.25).abs() < 1.0e-6);
}

#[test]
fn graph_lfo_one_shot_direction_phase_and_polarity_are_exact() {
    let config = LfoConfig {
        waveform: crate::model::LfoWaveform::Saw,
        custom_points: [crate::engine::lfo::LfoPoint::default();
            crate::engine::lfo::MAX_CUSTOM_LFO_POINTS],
        custom_point_count: 0,
        rate: Scalar::constant(1.0),
        depth: Scalar::constant(1.0),
        phase: Scalar::constant(0.0),
        random_phase: Scalar::constant(0.0),
        swing: Scalar::constant(0.0),
        mode: LfoMode::OneShot,
        direction: LfoDirection::Forward,
        polarity: crate::model::ControlPolarity::Unipolar,
        retrigger: GraphLfoRetrigger::Note,
    };
    let mut state = LfoState::new(5, GraphLfoRetrigger::Note);
    let values = (0..7)
        .map(|_| config.process(&mut state, &[], 4.0, 0))
        .collect::<Vec<_>>();
    assert_eq!(&values[..4], &[0.0, 0.25, 0.5, 0.75]);
    assert!(values[4] > 0.999_999);
    assert_eq!(values[4], values[5]);
    assert_eq!(values[5], values[6]);

    let mut reverse = config;
    reverse.direction = LfoDirection::Reverse;
    reverse.phase = Scalar::constant(0.5);
    reverse.polarity = crate::model::ControlPolarity::Bipolar;
    let mut state = LfoState::new(5, GraphLfoRetrigger::Note);
    let values = (0..4)
        .map(|_| reverse.process(&mut state, &[], 4.0, 0))
        .collect::<Vec<_>>();
    assert_eq!(values, [0.0, -0.5, -1.0, -1.0]);
}

#[test]
fn graph_custom_lfo_transport_clock_and_swing_are_exact() {
    let mut points =
        [crate::engine::lfo::LfoPoint::default(); crate::engine::lfo::MAX_CUSTOM_LFO_POINTS];
    points[..3].copy_from_slice(&[
        crate::engine::lfo::LfoPoint {
            phase: 0.0,
            value: -1.0,
        },
        crate::engine::lfo::LfoPoint {
            phase: 0.5,
            value: 1.0,
        },
        crate::engine::lfo::LfoPoint {
            phase: 1.0,
            value: -1.0,
        },
    ]);
    let config = LfoConfig {
        waveform: crate::model::LfoWaveform::Sine,
        custom_points: points,
        custom_point_count: 3,
        rate: Scalar::constant(1.0),
        depth: Scalar::constant(1.0),
        phase: Scalar::constant(0.0),
        random_phase: Scalar::constant(0.0),
        swing: Scalar::constant(0.0),
        mode: LfoMode::Loop,
        direction: LfoDirection::Forward,
        polarity: crate::model::ControlPolarity::Bipolar,
        retrigger: GraphLfoRetrigger::Transport,
    };
    let mut state = LfoState::new(7, GraphLfoRetrigger::Transport);
    assert_eq!(config.process(&mut state, &[], 4.0, 0), -1.0);
    assert_eq!(config.process(&mut state, &[], 4.0, 1), 0.0);
    assert_eq!(config.process(&mut state, &[], 4.0, 2), 1.0);
    state.retrigger(99);
    assert_eq!(config.process(&mut state, &[], 4.0, 2), 1.0);

    let mut swung = config;
    swung.swing = Scalar::constant(1.0);
    assert!(swung.process(&mut state, &[], 4.0, 2) < 1.0);
}

/// Largest normalised autocorrelation of the 20 ms loudness envelope for lags of
/// 0.25 to 3 s. Near 1 means the level rises and falls at one regular rate.
fn envelope_periodicity(samples: &[[f32; 2]], sample_rate: f64) -> f64 {
    let hop = (sample_rate * 0.02) as usize;
    let envelope: Vec<f64> = samples
        .chunks(hop)
        .skip(5)
        .map(|chunk| {
            let power = chunk
                .iter()
                .map(|s| (f64::from(s[0]) + f64::from(s[1])).powi(2))
                .sum::<f64>()
                / chunk.len() as f64;
            10.0 * (power + 1e-12).log10()
        })
        .collect();
    let mean = envelope.iter().sum::<f64>() / envelope.len() as f64;
    let centred: Vec<f64> = envelope.iter().map(|v| v - mean).collect();
    let energy: f64 = centred.iter().map(|v| v * v).sum();
    (12..=150)
        .map(|lag| {
            centred
                .iter()
                .zip(&centred[lag..])
                .map(|(a, b)| a * b)
                .sum::<f64>()
                / energy
        })
        .fold(f64::MIN, f64::max)
}

fn held_unison(
    voices: usize,
    spread: UnisonSpread,
    shape: DriftShape,
    drift: f64,
) -> Vec<[f32; 2]> {
    let config = AnalogOscillatorConfig {
        detune: Scalar::constant(0.14),
        drift: Scalar::constant(drift),
        spread,
        drift_shape: shape,
        fm_depth: Scalar::constant(0.0),
        pm_depth: Scalar::constant(0.0),
        width: Scalar::constant(0.0),
        ..analog_config(voices)
    };
    render(config, 7, 48_000 * 8)
}

#[test]
fn unison_positions_keep_even_law_and_natural_rows_are_uneven() {
    for voices in 2..=MAX_UNISON_VOICES {
        let mut gaps = Vec::new();
        for subvoice in 0..voices {
            let even = unison_position(UnisonSpread::Even, subvoice, voices);
            assert_eq!(even, subvoice as f64 * 2.0 / (voices - 1) as f64 - 1.0);
            let natural = unison_position(UnisonSpread::Natural, subvoice, voices);
            assert!((-1.0..=1.0).contains(&natural));
            if subvoice > 0 {
                let gap = natural - unison_position(UnisonSpread::Natural, subvoice - 1, voices);
                assert!(gap > 0.0, "{voices} voices must stay ordered");
                gaps.push(gap);
            }
        }
        if voices > 2 {
            let widest = gaps.iter().copied().fold(f64::MIN, f64::max);
            let narrowest = gaps.iter().copied().fold(f64::MAX, f64::min);
            assert!(
                widest / narrowest > 1.2,
                "{voices} voices must not be evenly spaced"
            );
        }
        assert_eq!(unison_position(UnisonSpread::Natural, 0, 1), 0.0);
    }
}

#[test]
fn sine_drift_matches_the_original_law_and_wander_stays_bounded() {
    let seed = xorshift(nonzero_seed(99));
    let mut bank = DriftBank::new();
    bank.retrigger(0, seed);
    let mut phase = seed_phase(xorshift(seed));
    let rate = 0.05 + seed_phase(xorshift(xorshift(seed))) * 0.1;
    for _ in 0..200_000 {
        assert_eq!(bank.value(0, DriftShape::Sine), (phase * TAU).sin());
        bank.advance(0, DriftShape::Sine, 48_000.0);
        phase = (phase + rate / 48_000.0).fract();
    }
    let mut wander = DriftBank::new();
    wander.retrigger(0, seed);
    let mut values = Vec::new();
    for _ in 0..48_000 * 20 {
        let value = wander.value(0, DriftShape::Wander);
        assert!((-1.0..=1.0).contains(&value));
        values.push(value);
        wander.advance(0, DriftShape::Wander, 48_000.0);
    }
    let steps = values
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f64::max);
    assert!(
        steps < 1e-3,
        "wander must move smoothly, largest step {steps}"
    );
    let span = values.iter().copied().fold(f64::MIN, f64::max)
        - values.iter().copied().fold(f64::MAX, f64::min);
    assert!(span > 0.5, "wander must actually move, span {span}");
}

#[test]
fn natural_spread_and_wander_smear_regular_unison_beating() {
    for voices in [3, 5, 7] {
        let even = envelope_periodicity(
            &held_unison(voices, UnisonSpread::Even, DriftShape::Sine, 0.0),
            48_000.0,
        );
        let natural = envelope_periodicity(
            &held_unison(voices, UnisonSpread::Natural, DriftShape::Wander, 0.02),
            48_000.0,
        );
        // Measured: 3 voices 0.93 -> 0.68, 5 voices 0.86 -> 0.28, 7 voices 0.77 -> 0.25.
        assert!(
            natural < even,
            "{voices} voices: natural {natural} even {even}"
        );
        if voices >= 5 {
            assert!(
                natural < 0.45,
                "{voices} voices still beat regularly: {natural}"
            );
        }
    }
    let natural = held_unison(5, UnisonSpread::Natural, DriftShape::Wander, 0.02);
    assert_eq!(
        natural,
        held_unison(5, UnisonSpread::Natural, DriftShape::Wander, 0.02)
    );
    assert!(natural.iter().flatten().all(|s| s.is_finite()));
}
