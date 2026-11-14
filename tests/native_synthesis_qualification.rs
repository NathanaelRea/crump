//! Integrated automated evidence for the admitted native-synthesis maximum profile.

use std::{fs, path::Path};

use crump::{
    engine::{
        CompiledProject, ProcessingMode, RenderOptions, mix_graph::MAX_WORK_UNITS_PER_FRAME,
        render_project,
    },
    project::{ProjectBundle, load_project},
};
use serde_json::Value;
use tempfile::TempDir;

fn fixture() -> ProjectBundle {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/synth-alpha/maximum-topology.crump");
    let bundle = load_project(path).expect("load maximum qualification Project");
    assert!(
        bundle.publishable().is_some(),
        "maximum fixture diagnostics: {:?}",
        bundle.diagnostics.as_slice()
    );
    bundle
}

fn render_grouped(compiled: &CompiledProject, frames: usize, grouping: &[usize]) -> Vec<[u32; 2]> {
    let mut runtime = compiled.runtime();
    let mut rendered = Vec::with_capacity(frames);
    let mut group = 0;
    while rendered.len() < frames {
        let count = grouping[group % grouping.len()].min(frames - rendered.len());
        let mut block = vec![[0.0_f32; 2]; count];
        assert_eq!(
            runtime
                .process_block(&mut block, ProcessingMode::Offline)
                .expect("process maximum fixture"),
            count
        );
        rendered.extend(block.into_iter().map(|frame| {
            assert!(frame.into_iter().all(f32::is_finite));
            [frame[0].to_bits(), frame[1].to_bits()]
        }));
        group += 1;
    }
    rendered
}

#[test]
fn maximum_profile_is_admitted_deterministic_grouping_independent_and_rebuildable() {
    let bundle = fixture();
    let project = bundle.publishable().expect("valid Project");
    let compiled =
        CompiledProject::compile_with_assets(project, &bundle.assets).expect("compile profile");
    let rebuilt =
        CompiledProject::compile_with_assets(project, &bundle.assets).expect("rebuild caches");
    let resources = &compiled.mix_graph_inspection().resources;

    assert!(resources.admitted);
    assert_eq!(resources.graph_voices, 7);
    assert_eq!(resources.graph_node_instances, 28_646);
    assert_eq!(resources.shared_graphs, 1);
    assert_eq!(resources.shared_graph_work_units_per_frame, 16);
    assert_eq!(resources.control_targets, 23);
    assert_eq!(resources.work_units_per_frame, 28_696);
    assert_eq!(
        resources.publication_transition_work_units_per_frame,
        57_458
    );
    let expected_runtime_bytes = if cfg!(feature = "pipewire") {
        1_727_008
    } else {
        1_726_968
    };
    assert_eq!(resources.runtime_bytes, expected_runtime_bytes);
    assert_eq!(
        resources.publication_peak_bytes,
        expected_runtime_bytes * resources.publication_runtime_slots
    );
    assert!(resources.work_units_per_frame <= MAX_WORK_UNITS_PER_FRAME);
    assert!(resources.publication_peak_bytes <= resources.publication_budget_bytes);
    assert_eq!(resources.supported_quanta, [64, 128, 256]);
    assert_eq!(
        compiled.sample_source_inspection(),
        rebuilt.sample_source_inspection()
    );
    assert_eq!(
        compiled.spectral_analysis_inspection(),
        rebuilt.spectral_analysis_inspection()
    );
    assert_eq!(
        compiled.wavetable_inspection(),
        rebuilt.wavetable_inspection()
    );

    let frames = 613;
    let at_64 = render_grouped(&compiled, frames, &[64]);
    assert_eq!(at_64, render_grouped(&compiled, frames, &[128]));
    assert_eq!(at_64, render_grouped(&compiled, frames, &[256]));
    assert_eq!(
        at_64,
        render_grouped(&compiled, frames, &[64, 128, 256, 17])
    );
    assert_eq!(at_64, render_grouped(&rebuilt, frames, &[113]));
    assert!(
        at_64.iter().any(|frame| frame[0] != 0 || frame[1] != 0),
        "the admitted profile must produce audible output"
    );

    let sections = &project.song.instruments[0].presentation.sections;
    assert_eq!(
        sections
            .iter()
            .map(|section| section.id.as_str())
            .collect::<Vec<_>>(),
        [
            "sources-view",
            "mixer-view",
            "filters-view",
            "modulators-view",
            "matrix-view",
            "voice-view",
            "routing-view",
            "effects-view",
        ]
    );

    let mut rejected = project.clone();
    rejected.song.instruments[0].polyphony = 8;
    assert!(
        CompiledProject::compile_with_assets(&rejected, &bundle.assets).is_err(),
        "the next combined-profile voice must cross publication-transition admission"
    );
}

#[test]
fn interrupted_disposable_derivative_rebuild_leaves_published_runtime_usable() {
    let bundle = fixture();
    let project = bundle.publishable().expect("valid Project");
    let published =
        CompiledProject::compile_with_assets(project, &bundle.assets).expect("published runtime");
    let expected = render_grouped(&published, 514, &[73, 11, 128]);
    let mut current = published.runtime();
    let mut observed = Vec::with_capacity(expected.len());
    for _ in 0..257 {
        observed.push(
            current
                .process_frame(ProcessingMode::Offline)
                .expect("published runtime before interruption")
                .map(f32::to_bits),
        );
    }

    let interrupted =
        CompiledProject::compile_with_assets(project, &bundle.assets).expect("candidate rebuild");
    assert_eq!(
        interrupted.sample_source_inspection(),
        published.sample_source_inspection()
    );
    assert_eq!(
        interrupted.spectral_analysis_inspection(),
        published.spectral_analysis_inspection()
    );
    assert_eq!(
        interrupted.wavetable_inspection(),
        published.wavetable_inspection()
    );
    drop(interrupted);

    for _ in 257..514 {
        observed.push(
            current
                .process_frame(ProcessingMode::Offline)
                .expect("published runtime after interrupted rebuild")
                .map(f32::to_bits),
        );
    }
    assert_eq!(observed, expected);

    let recovered =
        CompiledProject::compile_with_assets(project, &bundle.assets).expect("recovered rebuild");
    assert_eq!(
        render_grouped(&recovered, 514, &[89]),
        expected,
        "discarded disposable derivatives must rebuild to the same audio"
    );
}

#[test]
fn maximum_profile_repeated_render_bundles_are_byte_identical() {
    let bundle = fixture();
    let output = TempDir::new().expect("render parent");
    let first_path = output.path().join("first");
    let second_path = output.path().join("second");
    let first = render_project(&bundle, &RenderOptions::new(&first_path)).expect("first render");
    let second = render_project(&bundle, &RenderOptions::new(&second_path)).expect("second render");

    assert_eq!(
        fs::read(&first.wav).unwrap(),
        fs::read(&second.wav).unwrap()
    );
    assert_eq!(
        fs::read(&first.manifest_path).unwrap(),
        fs::read(&second.manifest_path).unwrap()
    );
    let manifest: Value =
        serde_json::from_slice(&fs::read(&first.manifest_path).unwrap()).expect("manifest JSON");
    assert_eq!(manifest["wav_sha256"], first.manifest.wav_sha256);
    assert_eq!(first.manifest, second.manifest);
}
