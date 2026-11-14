use super::*;

fn stereo_config(model: StereoFilterModel) -> StereoFilterConfig {
    StereoFilterConfig {
        model,
        mode: FilterMode::Lowpass,
        cutoff: Scalar::constant(2_000.0),
        resonance: Scalar::constant(0.75),
        drive: Scalar::constant(0.8),
        mix: Scalar::constant(1.0),
        pan: Scalar::constant(0.0),
        mod_depth: Scalar::constant(0.0),
        velocity_depth: Scalar::constant(0.0),
        key_track: Scalar::constant(0.0),
        key_reference: Scalar::constant(261.625_565),
    }
}

#[test]
fn stereo_filter_channels_are_isolated_and_models_are_finite() {
    for model in [StereoFilterModel::SvfClean, StereoFilterModel::LadderDrive] {
        let config = stereo_config(model);
        let mut state = StereoFilterState::default();
        let first = config.process(
            &mut state,
            [1.0, 0.0],
            0.0,
            0.0,
            0.0,
            false,
            false,
            false,
            &[],
            48_000.0,
        );
        assert_eq!(first[1], 0.0);
        assert!(first[0].is_finite());
        for frame in 0_usize..8_192 {
            let input = if frame.is_multiple_of(2) { 1.0 } else { -1.0 };
            let output = config.process(
                &mut state,
                [input, 0.0],
                0.0,
                0.0,
                0.0,
                false,
                false,
                false,
                &[],
                48_000.0,
            );
            assert!(output.into_iter().all(f32::is_finite));
            assert_eq!(output[1], 0.0);
        }
    }
}

#[test]
fn stereo_filter_lowpass_response_and_parameter_smoothing_are_bounded() {
    for model in [StereoFilterModel::SvfClean, StereoFilterModel::LadderDrive] {
        let config = stereo_config(model);
        let response_rms = |frequency: f64| {
            let mut state = StereoFilterState::default();
            let mut energy = 0.0_f64;
            let frames = 8_192;
            for frame in 0..frames {
                let input =
                    (std::f64::consts::TAU * frequency * frame as f64 / 48_000.0).sin() as f32;
                let output = config.process(
                    &mut state,
                    [input, input],
                    0.0,
                    0.0,
                    0.0,
                    false,
                    false,
                    false,
                    &[],
                    48_000.0,
                );
                if frame >= 1_024 {
                    energy += f64::from(output[0]).powi(2);
                }
            }
            (energy / (frames - 1_024) as f64).sqrt()
        };
        let low = response_rms(200.0);
        let high = response_rms(8_000.0);
        assert!(low > high * 2.0, "{model:?}: low={low}, high={high}");

        let mut state = StereoFilterState::default();
        let _ = config.process(
            &mut state,
            [0.0, 0.0],
            0.0,
            0.0,
            0.0,
            false,
            false,
            false,
            &[],
            48_000.0,
        );
        assert_eq!(state.smoothed_cutoff, 2_000.0);
        let mut moved = config;
        moved.cutoff = Scalar::constant(16_000.0);
        let _ = moved.process(
            &mut state,
            [0.0, 0.0],
            0.0,
            0.0,
            0.0,
            false,
            false,
            false,
            &[],
            48_000.0,
        );
        assert!((2_000.0..16_000.0).contains(&state.smoothed_cutoff));
        for _ in 0..480 {
            let _ = moved.process(
                &mut state,
                [0.0, 0.0],
                0.0,
                0.0,
                0.0,
                false,
                false,
                false,
                &[],
                48_000.0,
            );
        }
        assert!((state.smoothed_cutoff - 16_000.0).abs() < 1.0);
    }
}

#[test]
fn stereo_filter_multimode_frequency_response_vectors_are_distinct() {
    let response = |model: StereoFilterModel, mode: FilterMode, frequency: f64| {
        let mut config = stereo_config(model);
        config.mode = mode;
        config.resonance = Scalar::constant(0.2);
        config.drive = Scalar::constant(0.0);
        let mut state = StereoFilterState::default();
        let frames = 16_384;
        let mut energy = 0.0_f64;
        for frame in 0..frames {
            let input = (std::f64::consts::TAU * frequency * frame as f64 / 48_000.0).sin() as f32;
            let output = config.process(
                &mut state,
                [input, input],
                0.0,
                0.0,
                0.0,
                false,
                false,
                false,
                &[],
                48_000.0,
            );
            if frame >= 2_048 {
                energy += f64::from(output[0]).powi(2);
            }
        }
        (energy / (frames - 2_048) as f64).sqrt()
    };

    let model = StereoFilterModel::SvfClean;
    let lowpass = [
        response(model, FilterMode::Lowpass, 200.0),
        response(model, FilterMode::Lowpass, 2_000.0),
        response(model, FilterMode::Lowpass, 8_000.0),
    ];
    let highpass = [
        response(model, FilterMode::Highpass, 200.0),
        response(model, FilterMode::Highpass, 2_000.0),
        response(model, FilterMode::Highpass, 8_000.0),
    ];
    let bandpass = [
        response(model, FilterMode::Bandpass, 200.0),
        response(model, FilterMode::Bandpass, 2_000.0),
        response(model, FilterMode::Bandpass, 8_000.0),
    ];
    let notch = [
        response(model, FilterMode::Notch, 200.0),
        response(model, FilterMode::Notch, 2_000.0),
        response(model, FilterMode::Notch, 8_000.0),
    ];
    assert!(lowpass[0] > lowpass[2] * 2.0, "{model:?}: {lowpass:?}");
    assert!(highpass[2] > highpass[0] * 2.0, "{model:?}: {highpass:?}");
    assert!(
        bandpass[1] > bandpass[0] * 1.2 && bandpass[1] > bandpass[2] * 1.2,
        "{model:?}: {bandpass:?}"
    );
    assert!(
        notch[1] < notch[0] * 0.9 && notch[1] < notch[2] * 0.9,
        "{model:?}: notch={notch:?}, bandpass={bandpass:?}"
    );
}

#[test]
fn svf_drive_is_audible_finite_and_zero_is_exactly_clean() {
    let mut clean = stereo_config(StereoFilterModel::SvfClean);
    clean.drive = Scalar::constant(0.0);
    clean.resonance = Scalar::constant(0.2);
    let mut driven = clean;
    driven.drive = Scalar::constant(1.0);
    let mut clean_state = StereoFilterState::default();
    let mut driven_state = StereoFilterState::default();
    let mut difference = 0.0_f64;
    for _ in 0..2_048 {
        let input = 0.5;
        let clean_sample = clean.process(
            &mut clean_state,
            [input, input],
            0.0,
            0.0,
            0.0,
            false,
            false,
            false,
            &[],
            48_000.0,
        );
        let driven_sample = driven.process(
            &mut driven_state,
            [input, input],
            0.0,
            0.0,
            0.0,
            false,
            false,
            false,
            &[],
            48_000.0,
        );
        assert!(clean_sample.into_iter().all(f32::is_finite));
        assert!(driven_sample.into_iter().all(f32::is_finite));
        difference += f64::from((clean_sample[0] - driven_sample[0]).abs());
    }
    assert!(difference > 1.0);
}

#[test]
fn stereo_filter_dry_mix_and_pan_are_exact() {
    let mut config = stereo_config(StereoFilterModel::LadderDrive);
    config.mix = Scalar::constant(0.0);
    config.pan = Scalar::constant(1.0);
    let mut state = StereoFilterState::default();
    let output = config.process(
        &mut state,
        [0.25, -0.5],
        0.0,
        0.0,
        0.0,
        false,
        false,
        false,
        &[],
        48_000.0,
    );
    assert!(output[0].abs() < 1.0e-7);
    assert_eq!(output[1], -0.5);
}

#[test]
fn ladder_below_full_resonance_never_self_oscillates_at_any_drive() {
    for drive in [0.0, 0.1, 0.25, 0.5, 0.75, 1.0] {
        for cutoff in [40.0, 200.0, 1_000.0, 4_000.0, 12_000.0, 20_000.0] {
            for resonance in [0.5, 0.9, 0.99] {
                let mut config = stereo_config(StereoFilterModel::LadderDrive);
                config.drive = Scalar::constant(drive);
                config.cutoff = Scalar::constant(cutoff);
                config.resonance = Scalar::constant(resonance);
                let mut state = StereoFilterState::default();
                let mut process = |input: f32| {
                    config.process(
                        &mut state,
                        [input, input],
                        0.0,
                        0.0,
                        0.0,
                        false,
                        false,
                        false,
                        &[],
                        48_000.0,
                    )[0]
                };
                // A loud burst, then two seconds of silence: the ring must be dying away.
                for frame in 0..2_400 {
                    process(if frame % 40 < 20 { 0.9 } else { -0.9 });
                }
                let mut middle_peak = 0.0_f32;
                let mut tail_peak = 0.0_f32;
                for frame in 0..96_000 {
                    let output = process(0.0);
                    assert!(output.is_finite());
                    if (38_400..48_000).contains(&frame) {
                        middle_peak = middle_peak.max(output.abs());
                    } else if frame >= 86_400 {
                        tail_peak = tail_peak.max(output.abs());
                    }
                }
                assert!(
                    tail_peak < 1.0e-4 || tail_peak < middle_peak * 0.5,
                    "drive {drive} cutoff {cutoff} resonance {resonance} rings at {tail_peak} after {middle_peak}"
                );
            }
        }
    }
}

#[test]
fn ladder_resonance_peak_does_not_grow_with_drive() {
    // Resonance means the same small-signal loop gain at every drive: the normalized ring
    // after a tiny impulse has the same decay whatever the drive.
    let ring = |drive: f64| {
        let mut config = stereo_config(StereoFilterModel::LadderDrive);
        config.drive = Scalar::constant(drive);
        config.cutoff = Scalar::constant(1_000.0);
        config.resonance = Scalar::constant(0.9);
        let mut state = StereoFilterState::default();
        let mut peak_late = 0.0_f32;
        let mut peak_early = 0.0_f32;
        for frame in 0..9_600 {
            let input = if frame == 0 { 1.0e-4 } else { 0.0 };
            let output = config.process(
                &mut state,
                [input, input],
                0.0,
                0.0,
                0.0,
                false,
                false,
                false,
                &[],
                48_000.0,
            )[0]
            .abs();
            if frame < 480 {
                peak_early = peak_early.max(output);
            } else if frame >= 4_800 {
                peak_late = peak_late.max(output);
            }
        }
        peak_late / peak_early
    };
    let clean = ring(0.0);
    for drive in [0.25, 0.5, 1.0] {
        let driven = ring(drive);
        assert!(
            (driven / clean - 1.0).abs() < 0.05,
            "drive {drive}: decay ratio {driven} vs {clean}"
        );
    }
}
