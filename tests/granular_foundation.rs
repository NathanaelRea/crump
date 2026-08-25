//! End-to-end bounded granular source checks.

use std::{fs, path::Path};

use crump::{
    dsl::parse_project,
    engine::{CompiledProject, ProcessingMode},
    media::load_asset_inventory,
    model::{Project, SettingValue},
    validate::{validate_project, validate_project_assets},
};

fn fixture() -> (Project, crump::media::AssetInventory) {
    let path = Path::new("fixtures/synth-alpha/minimal-granular.crump");
    let source = fs::read_to_string(path).expect("granular fixture");
    let parsed = parse_project(path, &source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let (assets, diagnostics) =
        load_asset_inventory(Path::new("fixtures/synth-alpha"), &project.assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let diagnostics = validate_project_assets(&project, &assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    (project, assets)
}

fn render(compiled: &CompiledProject, frames: usize, quantum: usize) -> Vec<[f32; 2]> {
    let mut runtime = compiled.runtime();
    let mut output = Vec::with_capacity(frames);
    while output.len() < frames {
        let count = quantum.min(frames - output.len());
        let mut block = vec![[0.0; 2]; count];
        runtime
            .process_block(&mut block, ProcessingMode::Offline)
            .expect("finite granular output");
        output.extend(block);
    }
    output
}

fn source_node(project: &mut Project) -> &mut crump::model::GraphNode {
    project.song.instruments[0]
        .graph
        .as_mut()
        .expect("voice graph")
        .nodes
        .iter_mut()
        .find(|node| node.id == "source")
        .expect("granular source")
}

fn set(project: &mut Project, key: &str, value: SettingValue) {
    source_node(project)
        .settings
        .iter_mut()
        .find(|setting| setting.key == key)
        .unwrap_or_else(|| panic!("missing {key}"))
        .value = value;
}

#[test]
fn granular_fixture_is_finite_stereo_seeded_and_grouping_independent() {
    let (project, assets) = fixture();
    let compiled = CompiledProject::compile_with_assets(&project, &assets).expect("granular");
    let at_64 = render(&compiled, 96_000, 64);
    let at_251 = render(&compiled, 96_000, 251);
    assert_eq!(at_64, at_251);
    assert!(at_64.iter().flatten().all(|sample| sample.is_finite()));
    assert!(at_64.iter().any(|frame| frame[0].abs() > 1.0e-5));
    assert!(
        at_64
            .iter()
            .map(|frame| (frame[0] - frame[1]).abs())
            .sum::<f32>()
            > 0.01
    );
    assert!(compiled.mix_graph_inspection().resources.admitted);

    let mut changed = project;
    set(&mut changed, "seed", SettingValue::Number(72.0));
    let changed = CompiledProject::compile_with_assets(&changed, &assets).expect("changed seed");
    assert_ne!(at_64, render(&changed, 96_000, 127));
}

#[test]
fn grain_windows_edges_freeze_and_bounds_are_explicit() {
    let (project, assets) = fixture();
    let baseline = CompiledProject::compile_with_assets(&project, &assets).expect("baseline");
    let baseline = render(&baseline, 48_000, 97);
    for window in ["triangle", "tukey"] {
        let mut changed = project.clone();
        set(
            &mut changed,
            "window",
            SettingValue::Text(window.to_owned()),
        );
        let changed = CompiledProject::compile_with_assets(&changed, &assets).expect("variant");
        let output = render(&changed, 48_000, 113);
        assert_ne!(baseline, output, "{window} window must affect audio");
        assert!(output.iter().flatten().all(|sample| sample.is_finite()));
    }

    let mut reflected = project.clone();
    reflected.song.instruments[0]
        .parameters
        .iter_mut()
        .find(|parameter| parameter.key == "position")
        .expect("position")
        .value = 0.99;
    set(&mut reflected, "size", SettingValue::Text("1s".to_owned()));
    let reflected =
        CompiledProject::compile_with_assets(&reflected, &assets).expect("reflected edge");
    let mut wrapped = project.clone();
    wrapped.song.instruments[0]
        .parameters
        .iter_mut()
        .find(|parameter| parameter.key == "position")
        .expect("position")
        .value = 0.99;
    set(&mut wrapped, "size", SettingValue::Text("1s".to_owned()));
    set(&mut wrapped, "edge", SettingValue::Text("wrap".to_owned()));
    let wrapped = CompiledProject::compile_with_assets(&wrapped, &assets).expect("wrapped edge");
    let reflected_output = render(&reflected, 48_000, 89);
    let wrapped_output = render(&wrapped, 48_000, 181);
    assert_ne!(reflected_output, wrapped_output);
    let mut clamped = project.clone();
    clamped.song.instruments[0]
        .parameters
        .iter_mut()
        .find(|parameter| parameter.key == "position")
        .expect("position")
        .value = 0.99;
    set(&mut clamped, "size", SettingValue::Text("1s".to_owned()));
    set(&mut clamped, "edge", SettingValue::Text("clamp".to_owned()));
    let clamped = CompiledProject::compile_with_assets(&clamped, &assets).expect("clamped edge");
    let clamped_output = render(&clamped, 48_000, 157);
    assert_ne!(reflected_output, clamped_output);
    assert!(
        clamped_output
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );

    let mut moving = project.clone();
    set(&mut moving, "scan", SettingValue::Number(0.5));
    let moving = CompiledProject::compile_with_assets(&moving, &assets).expect("moving scan");
    let mut frozen = project.clone();
    set(&mut frozen, "scan", SettingValue::Number(0.5));
    set(&mut frozen, "freeze", SettingValue::Text("true".to_owned()));
    let frozen = CompiledProject::compile_with_assets(&frozen, &assets).expect("frozen scan");
    assert_ne!(render(&moving, 48_000, 79), render(&frozen, 48_000, 173));

    let mut maximum = project.clone();
    set(&mut maximum, "max_grains", SettingValue::Number(64.0));
    let maximum = CompiledProject::compile_with_assets(&maximum, &assets).expect("64 grains");
    assert!(maximum.mix_graph_inspection().resources.admitted);
    assert!(
        render(&maximum, 8_192, 127)
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );

    let mut excessive = project;
    set(&mut excessive, "max_grains", SettingValue::Number(65.0));
    let diagnostics = validate_project(&excessive);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV051" && diagnostic.message.contains("max_grains")
    }));
    assert!(CompiledProject::compile_with_assets(&excessive, &assets).is_err());
}

#[test]
fn granular_randomness_is_stable_under_unrelated_topology_insertion() {
    let (project, assets) = fixture();
    let baseline = CompiledProject::compile_with_assets(&project, &assets).expect("baseline");
    let baseline = render(&baseline, 48_000, 127);
    let path = Path::new("fixtures/synth-alpha/minimal-granular.crump");
    let source = fs::read_to_string(path)
        .expect("fixture")
        .replace(
            "        node voice-out output",
            "        node a-noise noise level 0\n        node a-wide mono_to_stereo\n        node source-sum stereo_sum max_inputs 2\n        node voice-out output",
        )
        .replace(
            "        bus output-audio source.audio -> voice-out.audio",
            "        bus zero-mono a-noise.audio -> a-wide.audio\n        bus granular-to-sum source.audio -> source-sum.inputs\n        bus zero-to-sum a-wide.audio -> source-sum.inputs\n        bus output-audio source-sum.audio -> voice-out.audio",
        );
    let parsed = parse_project(path, &source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let inserted = parsed.project.expect("Project");
    assert!(!validate_project(&inserted).has_errors());
    let inserted = CompiledProject::compile_with_assets(&inserted, &assets).expect("inserted");
    let inserted = render(&inserted, 48_000, 193);
    if baseline != inserted {
        let index = baseline
            .iter()
            .zip(&inserted)
            .position(|(left, right)| left != right)
            .expect("different output");
        assert_eq!(
            baseline[index], inserted[index],
            "first difference at {index}"
        );
    }
}

#[test]
fn copied_granular_project_rebuilds_without_an_external_path() {
    let (project, assets) = fixture();
    let original = CompiledProject::compile_with_assets(&project, &assets).expect("original");
    let copied = tempfile::tempdir().expect("clean account");
    fs::create_dir(copied.path().join("media")).expect("media");
    fs::copy(
        "fixtures/synth-alpha/minimal-granular.crump",
        copied.path().join("project.crump"),
    )
    .expect("source");
    fs::copy(
        "fixtures/synth-alpha/media/sample-source.wav",
        copied.path().join("media/sample-source.wav"),
    )
    .expect("asset");
    let source = fs::read_to_string(copied.path().join("project.crump")).expect("source");
    let parsed = parse_project(&copied.path().join("project.crump"), &source);
    let copied_project = parsed.project.expect("Project");
    let (copied_assets, diagnostics) = load_asset_inventory(copied.path(), &copied_project.assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let reopened =
        CompiledProject::compile_with_assets(&copied_project, &copied_assets).expect("reopened");
    assert_eq!(
        render(&original, 48_000, 64),
        render(&reopened, 48_000, 239)
    );
}
