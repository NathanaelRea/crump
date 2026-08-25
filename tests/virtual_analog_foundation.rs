//! End-to-end virtual-analog oscillator and stereo-filter foundation checks.

use std::path::Path;

use crump::{
    dsl::parse_project,
    engine::{CompiledProject, ProcessingMode},
    model::{GlideMode, NodeKind, PresentationSectionKind, SettingValue, VoicingMode},
    validate::validate_project,
};

fn project_from(source: &str) -> crump::model::Project {
    let parsed = parse_project(Path::new("virtual_analog_foundation.crump"), source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    project
}

fn render(compiled: &CompiledProject, frames: usize, quantum: usize) -> Vec<[f32; 2]> {
    let mut runtime = compiled.runtime();
    let mut output = Vec::with_capacity(frames);
    while output.len() < frames {
        let count = quantum.min(frames - output.len());
        let mut block = vec![[0.0; 2]; count];
        assert_eq!(
            runtime
                .process_block(&mut block, ProcessingMode::Offline)
                .expect("finite render"),
            count
        );
        output.extend(block);
    }
    output
}

#[test]
fn analog_stages_and_stereo_filter_are_typed_deterministic_and_admitted() {
    let source = include_str!("fixtures/virtual_analog_foundation.crump");
    let project = project_from(source);
    let instrument = &project.song.instruments[0];
    assert_eq!(instrument.presentation.sections.len(), 8);
    assert_eq!(
        instrument.presentation.sections[0].kind,
        PresentationSectionKind::Sources
    );
    assert_eq!(instrument.presentation.sections[0].items[0].id, "modulator");
    assert!(
        instrument.presentation.sections[0].items[0].id_site.start
            < instrument.presentation.sections[0].items[0].id_site.end
    );
    let graph = instrument.graph.as_ref().expect("voice graph");
    let carrier = graph
        .nodes
        .iter()
        .find(|node| node.id == "carrier")
        .expect("analog carrier");
    assert_eq!(carrier.kind, NodeKind::AnalogOscillator);
    assert_eq!(carrier.directives.len(), 3);
    assert_eq!(
        carrier.directives[0]
            .tokens
            .iter()
            .map(|token| token.value.as_str())
            .collect::<Vec<_>>(),
        [
            "warp",
            "pulse-shape",
            "pulse_width",
            "amount",
            "$warp-amount"
        ]
    );
    assert!(carrier.directives[0].tokens[4].site.start < carrier.directives[0].tokens[4].site.end);
    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::StereoFilter)
            .count(),
        2
    );
    let serial_source = source.replace(
        "bus edge-audio carrier.audio -> edge.audio",
        "bus edge-audio tone.audio -> edge.audio",
    );
    let serial = project_from(&serial_source);
    let serial_compiled = CompiledProject::compile(&serial).expect("compile serial filter graph");
    assert!(
        render(&serial_compiled, 4_096, 127)
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );

    let compiled = CompiledProject::compile(&project).expect("compile virtual analog graph");
    let resources = &compiled.mix_graph_inspection().resources;
    assert!(resources.admitted);
    assert!(resources.graph_voice_state_bytes > 0);
    assert!(resources.work_units_per_frame >= 100);

    let mut simple_project = project.clone();
    let motion = simple_project.song.instruments[0]
        .graph
        .as_mut()
        .expect("voice graph")
        .nodes
        .iter_mut()
        .find(|node| node.id == "motion")
        .expect("motion LFO");
    motion
        .settings
        .iter_mut()
        .find(|setting| setting.key == "waveform")
        .expect("waveform setting")
        .value = SettingValue::Text("sample_hold".to_owned());
    motion.directives.clear();
    let simple = CompiledProject::compile(&simple_project).expect("compile built-in LFO variant");
    assert!(
        resources.work_units_per_frame
            > simple.mix_graph_inspection().resources.work_units_per_frame,
        "custom point traversal must enter admission"
    );

    let mut am_only_project = project.clone();
    am_only_project.song.instruments[0]
        .graph
        .as_mut()
        .expect("voice graph")
        .connections
        .retain(|bus| {
            bus.to.node != "carrier" || !matches!(bus.to.port.as_str(), "fm" | "pm" | "sync")
        });
    let am_only =
        CompiledProject::compile(&am_only_project).expect("compile AM-only audio-rate carrier");
    assert!(
        resources.work_units_per_frame
            > am_only
                .mix_graph_inspection()
                .resources
                .work_units_per_frame,
        "FM/PM/sync work must enter conservative admission"
    );

    let mut without_am_project = project.clone();
    without_am_project.song.instruments[0]
        .graph
        .as_mut()
        .expect("voice graph")
        .connections
        .retain(|bus| bus.to.node != "carrier" || bus.to.port != "am");
    let without_am =
        CompiledProject::compile(&without_am_project).expect("compile carrier without AM");
    assert!(
        resources.work_units_per_frame
            > without_am
                .mix_graph_inspection()
                .resources
                .work_units_per_frame,
        "AM work must enter conservative admission"
    );

    let at_64 = render(&compiled, 48_000, 64);
    let at_257 = render(&compiled, 48_000, 257);
    assert_eq!(at_64, at_257);
    assert!(at_64.iter().flatten().all(|sample| sample.is_finite()));
    assert!(
        at_64
            .iter()
            .any(|sample| sample[0].abs().max(sample[1].abs()) > 1.0e-5)
    );
    assert!(
        at_64
            .iter()
            .map(|sample| (sample[0] - sample[1]).abs())
            .sum::<f32>()
            > 0.01
    );
}

#[test]
fn presentation_references_are_source_v2_only_and_fail_closed() {
    let source = include_str!("fixtures/virtual_analog_foundation.crump");
    let missing = source.replace(
        "          node modulator\n",
        "          node missing-node\n",
    );
    let parsed = parse_project(Path::new("missing-presentation-node.crump"), &missing);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let invalid = parsed.project.expect("Project");
    let diagnostics = validate_project(&invalid);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV194" && diagnostic.message.contains("missing Node")
    }));
    assert!(CompiledProject::compile(&invalid).is_err());

    let parsed = parse_project(
        Path::new("legacy-presentation.crump"),
        include_str!("fixtures/minimal.crump"),
    );
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let mut legacy = parsed.project.expect("legacy Project");
    legacy.song.instruments[0].presentation = project_from(source).song.instruments[0]
        .presentation
        .clone();
    let diagnostics = validate_project(&legacy);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV194" && diagnostic.message.contains("source version 2")
    }));
}

#[test]
fn ahdsr_accepts_bounded_tempo_synchronized_segments_and_rejects_malformed_time() {
    let source = include_str!("fixtures/virtual_analog_foundation.crump");
    let synchronized = source.replace("hold 3ms", "hold 1/32beat");
    let project = project_from(&synchronized);
    let compiled = CompiledProject::compile(&project).expect("compile synchronized AHDSR");
    let first = render(&compiled, 24_000, 64);
    let second = render(&compiled, 24_000, 257);
    assert_eq!(first, second);

    let malformed = source.replace("hold 3ms", "hold 1/0beat");
    let parsed = parse_project(Path::new("malformed-envelope-time.crump"), &malformed);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let diagnostics = validate_project(&parsed.project.expect("Project"));
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV056" && diagnostic.message.contains("musical time")
    }));

    let adsr_with_hold = source.replace("mode ahdsr", "mode adsr");
    let parsed = parse_project(Path::new("adsr-with-hold.crump"), &adsr_with_hold);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let invalid = parsed.project.expect("Project");
    let diagnostics = validate_project(&invalid);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV195" && diagnostic.message.contains("mode ahdsr")
    }));
    assert!(CompiledProject::compile(&invalid).is_err());
}

#[test]
fn custom_lfo_is_bounded_tempo_synchronized_and_fail_closed() {
    let source = include_str!("fixtures/virtual_analog_foundation.crump");
    let project = project_from(source);
    let graph = project.song.instruments[0]
        .graph
        .as_ref()
        .expect("voice graph");
    let motion = graph
        .nodes
        .iter()
        .find(|node| node.id == "motion")
        .expect("custom LFO");
    assert_eq!(motion.directives.len(), 3);
    assert_eq!(motion.directives[1].tokens[1].value, "peak");
    assert!(motion.directives[1].tokens[3].site.start < motion.directives[1].tokens[3].site.end);

    let unordered = source.replace("point peak phase 0.5 value 1", "point peak phase 0 value 1");
    let parsed = parse_project(Path::new("unordered-custom-lfo.crump"), &unordered);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let invalid = parsed.project.expect("Project");
    let diagnostics = validate_project(&invalid);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV060")
    );
    assert!(CompiledProject::compile(&invalid).is_err());

    let points = (0..65)
        .map(|index| {
            format!(
                "          point p{index} phase {} value 0",
                index as f64 / 64.0
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let oversized = source.replace(
        "          point start phase 0 value -1\n          point peak phase 0.5 value 1\n          point end phase 1 value -1",
        &points,
    );
    let parsed = parse_project(Path::new("oversized-custom-lfo.crump"), &oversized);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let invalid = parsed.project.expect("Project");
    let diagnostics = validate_project(&invalid);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV060" && diagnostic.message.contains("2..=64")
    }));
    assert!(CompiledProject::compile(&invalid).is_err());

    let malformed_rate = source.replace("rate 1/8beat", "rate 1/0beat");
    let parsed = parse_project(Path::new("malformed-lfo-rate.crump"), &malformed_rate);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let diagnostics = validate_project(&parsed.project.expect("Project"));
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV056" && diagnostic.message.contains("LFO rate")
    }));
}

#[test]
fn voicing_validation_is_source_v2_only_and_rejects_ambiguous_ownership() {
    let source = include_str!("fixtures/virtual_analog_foundation.crump");
    let invalid_mono = source.replace(
        "voicing poly retrigger always glide 40ms glide_mode always note_priority last steal_priority release_oldest seed 41041",
        "voicing mono retrigger legato glide 40ms glide_mode legato note_priority low steal_priority release_oldest seed 41041",
    );
    let parsed = parse_project(Path::new("invalid-mono.crump"), &invalid_mono);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let invalid = parsed.project.expect("Project");
    let diagnostics = validate_project(&invalid);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV193" && diagnostic.message.contains("polyphony 1")
    }));
    assert!(CompiledProject::compile(&invalid).is_err());

    let valid_mono = invalid_mono.replacen("polyphony 4", "polyphony 1", 1);
    let project = project_from(&valid_mono);
    assert_eq!(project.song.instruments[0].voicing.mode, VoicingMode::Mono);
    assert_eq!(
        project.song.instruments[0].voicing.glide_mode,
        GlideMode::Legato
    );
    CompiledProject::compile(&project).expect("compile valid mono voicing");

    let parsed = parse_project(
        Path::new("legacy-direct-model.crump"),
        include_str!("fixtures/minimal.crump"),
    );
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let mut legacy = parsed.project.expect("legacy Project");
    legacy.song.instruments[0].voicing.mode = VoicingMode::Mono;
    legacy.song.instruments[0].voicing.glide_mode = GlideMode::Legato;
    legacy.song.instruments[0].voicing.glide_seconds = 0.02;
    legacy.song.instruments[0].polyphony = 1;
    let diagnostics = validate_project(&legacy);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV190")
    );
    assert!(CompiledProject::compile(&legacy).is_err());
}

#[test]
fn mixed_flat_and_structured_warps_follow_authoritative_source_order() {
    let source = include_str!("fixtures/virtual_analog_foundation.crump");
    let mixed = source.replace(
        "          warp phase-shape phase_remap amount 0.1",
        "          warp bend\n          warp_amount 0.1",
    );
    let structured = source.replace(
        "          warp phase-shape phase_remap amount 0.1",
        "          warp phase-shape bend amount 0.1",
    );
    let mixed = CompiledProject::compile(&project_from(&mixed)).expect("compile mixed warps");
    let structured =
        CompiledProject::compile(&project_from(&structured)).expect("compile structured warps");
    assert_eq!(render(&mixed, 8_192, 127), render(&structured, 8_192, 127));
}

#[test]
fn analog_bounds_and_audio_rate_port_types_fail_closed() {
    let source = include_str!("fixtures/virtual_analog_foundation.crump");
    let too_many_warps = source.replace(
        "          distortion output-drive tanh amount 0.15",
        "          warp third-warp bend amount 0.2\n          distortion output-drive tanh amount 0.15",
    );
    let parsed = parse_project(Path::new("too-many-warps.crump"), &too_many_warps);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let invalid = parsed.project.expect("Project");
    let diagnostics = validate_project(&invalid);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV060")
    );
    assert!(CompiledProject::compile(&invalid).is_err());

    let wrong_type = source.replace(
        "bus carrier-fm modulator.audio -> carrier.fm",
        "bus carrier-fm amplitude.control -> carrier.fm",
    );
    let parsed = parse_project(Path::new("wrong-fm-type.crump"), &wrong_type);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let invalid = parsed.project.expect("Project");
    let diagnostics = validate_project(&invalid);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV044")
    );
    assert!(CompiledProject::compile(&invalid).is_err());

    let unsupported_ladder_mode = source.replace(
        "model svf_clean mode highpass",
        "model ladder_drive mode highpass",
    );
    let parsed = parse_project(
        Path::new("unsupported-ladder-mode.crump"),
        &unsupported_ladder_mode,
    );
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let invalid = parsed.project.expect("Project");
    let diagnostics = validate_project(&invalid);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV056" && diagnostic.message.contains("only lowpass")
    }));
    assert!(CompiledProject::compile(&invalid).is_err());

    let source_v1 = include_str!("fixtures/minimal.crump").replace(
        "node oscillator oscillator waveform saw level 0.5",
        "node oscillator analog_oscillator waveform saw level 0.5",
    );
    let parsed = parse_project(Path::new("analog-v1.crump"), &source_v1);
    assert!(
        parsed
            .diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRP173")
    );
}
