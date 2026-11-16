//! End-to-end shared post-voice graph acceptance checks.

use std::path::Path;

use crump::{
    dsl::parse_project,
    engine::{CompiledProject, ProcessingMode},
    validate::validate_project,
};

fn project() -> crump::model::Project {
    let parsed = parse_project(
        Path::new("shared_graph_foundation.crump"),
        include_str!("fixtures/shared_graph_foundation.crump"),
    );
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    project
}

fn assert_rejected(source: &str, code: &str) {
    let parsed = parse_project(Path::new("invalid-shared.crump"), source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let diagnostics = validate_project(&project);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == code),
        "missing {code}: {:?}",
        diagnostics.as_slice()
    );
    assert!(CompiledProject::compile(&project).is_err());
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
fn shared_graph_is_track_owned_admitted_and_grouping_independent() {
    let project = project();
    let compiled = CompiledProject::compile(&project).expect("compile shared graph");
    let resources = &compiled.mix_graph_inspection().resources;
    assert_eq!(resources.shared_graphs, 1);
    assert!(resources.shared_graph_work_units_per_frame > 0);
    assert!(resources.shared_graph_state_bytes > 0);
    assert!(resources.admitted);

    let at_64 = render(&compiled, 48_000, 64);
    let at_128 = render(&compiled, 48_000, 128);
    let at_256 = render(&compiled, 48_000, 256);
    assert_eq!(at_64, at_128);
    assert_eq!(at_128, at_256);
    assert!(at_64.iter().flatten().all(|sample| sample.is_finite()));
    assert!(
        at_64
            .iter()
            .any(|sample| sample[0].abs().max(sample[1].abs()) > 1.0e-5)
    );

    let mut without_shared = project;
    without_shared.song.instruments[0].shared_graph = None;
    without_shared.song.instruments[0]
        .faceplate
        .retain(|control| control.key != "fx");
    let direct = CompiledProject::compile(&without_shared).expect("compile direct voice sum");
    assert_ne!(at_64, render(&direct, 48_000, 128));
}

#[test]
fn per_voice_graph_port_edges_do_not_leak_into_the_shared_domain() {
    let source = include_str!("fixtures/shared_graph_foundation.crump")
        .replace(
            "        source velocity note_velocity per_voice unipolar",
            "        source velocity note_velocity per_voice unipolar\n        source graph-lfo port shared-delay.control per_voice bipolar",
        )
        .replace(
            "        route velocity-fx source velocity target fx depth 0.5 map unipolar curve linear order 0 bypass false",
            "        route velocity-fx source velocity target fx depth 0.5 map unipolar curve linear order 0 bypass false\n        route graph-lfo-fx source graph-lfo target fx depth 0.1 map bipolar curve linear order 10 bypass false",
        )
        .replace("      graph voice 5 {", "      graph voice 6 {")
        .replace(
            "        node oscillator oscillator waveform saw level $tone",
            "        node oscillator oscillator waveform saw level $tone\n        node shared-delay lfo waveform sine rate 1hz depth 1",
        )
        .replace(
            "        node amplifier amplifier level 0.8",
            "        node amplifier amplifier level $fx",
        );
    let parsed = parse_project(Path::new("domain-local-node-ids.crump"), &source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let diagnostics = validate_project(&project);
    assert!(
        !diagnostics.has_errors(),
        "per-voice edges became shared edges: {diagnostics:?}"
    );
    CompiledProject::compile(&project).expect("compile domain-local duplicate Node IDs");
}

#[test]
fn shared_crossing_and_domain_errors_fail_closed() {
    let source = include_str!("fixtures/shared_graph_foundation.crump");
    let cases = [
        (
            source.replace(
                "        bus voice-sum voice_sum.audio -> shared-delay.audio\n",
                "",
            ),
            "CRV062",
        ),
        (
            source.replace(
                "        bus voice-sum voice_sum.audio -> shared-delay.audio",
                "        bus voice-sum voice_sum.audio -> shared-delay.audio\n        bus voice-sum-copy voice_sum.audio -> shared-delay.audio",
            ),
            "CRV062",
        ),
        (
            source.replace("voice_sum.audio -> shared-delay.audio", "note.gate -> shared-delay.audio"),
            "CRV045",
        ),
        (
            source.replace(
                "voice_sum.audio -> shared-delay.audio",
                "voice_sum.control -> shared-delay.audio",
            ),
            "CRV045",
        ),
        (
            source.replace(
                "note.pitch -> oscillator.pitch",
                "voice_sum.audio -> oscillator.pitch",
            ),
            "CRV045",
        ),
    ];
    for (candidate, code) in cases {
        assert_rejected(&candidate, code);
    }

    let mut legacy = project();
    legacy.source_version = 1;
    let diagnostics = validate_project(&legacy);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV190")
    );
    assert!(CompiledProject::compile(&legacy).is_err());
}
