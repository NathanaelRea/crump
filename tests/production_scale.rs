//! Machine-verifiable contracts for the 3:12 production-scale fixture.
//!
//! These checks prove duration, portable sampler/native coverage, admission, and
//! block grouping only. They do not provide listening or named-hardware evidence.

use crump::{
    engine::{CompiledProject, ProcessingMode},
    model::InstrumentKind,
    project::load_project,
};

fn production_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/production")
}

#[test]
fn production_fixture_is_portable_three_minute_and_admitted() {
    let bundle = load_project(production_root()).expect("load production fixture");
    assert!(
        !bundle.diagnostics.has_errors(),
        "{:?}",
        bundle.diagnostics.as_slice()
    );
    let project = bundle.project.as_ref().expect("parsed production Project");
    let duration_seconds = project.song.end.as_f64() * 60.0 / project.song.tempo_bpm;
    assert!(
        (180.0..=300.0).contains(&duration_seconds),
        "{duration_seconds}"
    );
    assert_eq!(duration_seconds, 192.0);
    assert_eq!(bundle.assets.iter().count(), 1);
    assert!(
        project
            .song
            .instruments
            .iter()
            .any(|instrument| instrument.kind == InstrumentKind::Sampler)
    );
    assert!(
        project
            .song
            .instruments
            .iter()
            .any(|instrument| instrument.kind == InstrumentKind::Graph)
    );
    assert!(
        project
            .song
            .instruments
            .iter()
            .any(|instrument| instrument.kind == InstrumentKind::StockDrums)
    );

    let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)
        .expect("compile production fixture");
    assert_eq!(compiled.arrangement_frames(), 9_216_000);
    assert_eq!(compiled.total_frames(), 9_360_000);
    let resources = &compiled.mix_graph_inspection().resources;
    assert!(resources.admitted);
    assert!(resources.sampler_voice_state_bytes > 0);
    assert!(resources.graph_voice_state_bytes > 0);
    assert!(resources.processor_state_bytes > 0);
    assert!(resources.routes >= 10);
    assert_eq!(resources.supported_quanta, [64, 128, 256]);
}

#[test]
fn production_fixture_sampler_and_native_intro_are_grouping_independent() {
    let bundle = load_project(production_root()).expect("load production fixture");
    let project = bundle.project.as_ref().expect("parsed production Project");
    let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)
        .expect("compile production fixture");
    const FRAMES: usize = 60_000;

    let process = |quantum: usize| {
        let mut runtime = compiled.runtime();
        let mut output = Vec::with_capacity(FRAMES);
        let mut block = vec![[0.0_f32; 2]; quantum];
        while output.len() < FRAMES {
            let count = (FRAMES - output.len()).min(quantum);
            let processed = runtime
                .process_block(&mut block[..count], ProcessingMode::Offline)
                .expect("process production fixture");
            assert_eq!(processed, count);
            output.extend_from_slice(&block[..count]);
        }
        output
    };

    let at_64 = process(64);
    assert_eq!(at_64, process(128));
    assert_eq!(at_64, process(256));
    assert!(at_64.iter().flatten().all(|sample| sample.is_finite()));
    assert!(
        at_64[48_000..]
            .iter()
            .flatten()
            .any(|sample| sample.abs() > 1.0e-5),
        "the range after the authored texture onset must contain Audio"
    );

    let mut isolated = project.clone();
    isolated.song.tracks.retain(|track| track.id == "textures");
    isolated.song.groups.retain(|group| group.id == "music");
    isolated.song.sidechains.clear();
    let isolated = CompiledProject::compile_with_assets(&isolated, &bundle.assets)
        .expect("compile isolated production Sampler path");
    let mut runtime = isolated.runtime();
    let mut sampler_output = vec![[0.0_f32; 2]; FRAMES];
    for block in sampler_output.chunks_mut(64) {
        let processed = runtime
            .process_block(block, ProcessingMode::Offline)
            .expect("process isolated production Sampler path");
        assert_eq!(processed, block.len());
    }
    assert!(
        sampler_output[..48_000]
            .iter()
            .flatten()
            .all(|sample| sample.abs() <= f32::EPSILON),
        "the isolated Sampler path must remain silent before its authored onset"
    );
    assert!(
        sampler_output[48_000..]
            .iter()
            .flatten()
            .any(|sample| sample.abs() > 1.0e-5),
        "the isolated portable sample occurrence must contribute Audio"
    );
}
