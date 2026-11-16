//! End-to-end deterministic spectral analysis/resynthesis checks.

use std::{fs, path::Path};

use crump::{
    dsl::{parse_module, parse_project},
    engine::{CompiledProject, ProcessingMode},
    media::load_asset_inventory,
    model::{MediaAssetUsage, Project, SettingValue},
    validate::{MAX_SPECTRAL_PEAKS, validate_project, validate_project_assets},
};

fn fixture() -> (Project, crump::media::AssetInventory) {
    let path = Path::new("fixtures/synth-alpha/minimal-spectral.crump");
    let source = fs::read_to_string(path).expect("spectral fixture");
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
            .expect("finite spectral output");
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
        .expect("spectral source")
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
fn spectral_fixture_is_analyzed_finite_stereo_and_grouping_independent() {
    let (project, assets) = fixture();
    let first = CompiledProject::compile_with_assets(&project, &assets).expect("spectral");
    let second = CompiledProject::compile_with_assets(&project, &assets).expect("spectral rebuild");
    let at_64 = render(&first, 96_000, 64);
    let at_251 = render(&first, 96_000, 251);
    assert_eq!(at_64, at_251);
    assert!(at_64.iter().flatten().all(|sample| sample.is_finite()));
    assert!(at_64.iter().any(|frame| frame[0].abs() > 1.0e-6));
    assert!(
        at_64
            .iter()
            .map(|frame| (frame[0] - frame[1]).abs())
            .sum::<f32>()
            > 0.01
    );
    assert!(first.mix_graph_inspection().resources.admitted);

    let inspection = first.spectral_analysis_inspection();
    assert_eq!(inspection.len(), 1);
    assert_eq!(inspection[0].algorithm, "crump-spectral-peaks-v1");
    assert_eq!(inspection[0].window_frames, 2_048);
    assert_eq!(inspection[0].hop_frames, 256);
    assert_eq!(inspection[0].peaks_per_frame, MAX_SPECTRAL_PEAKS);
    assert!(inspection[0].analysis_frames > 1);
    assert!(inspection[0].compiled_bytes > 0);
    assert_eq!(
        inspection[0].cache_identity,
        second.spectral_analysis_inspection()[0].cache_identity
    );
}

#[test]
fn tags_and_preview_metadata_are_sound_neutral() {
    let (project, assets) = fixture();
    let baseline = CompiledProject::compile_with_assets(&project, &assets).expect("baseline");
    let baseline = render(&baseline, 48_000, 127);
    let mut metadata = project;
    metadata.song.instruments[0].presentation.tags =
        vec!["changed".to_owned(), "metadata".to_owned()];
    let preview = metadata.song.instruments[0]
        .presentation
        .preview
        .as_mut()
        .expect("preview metadata");
    preview.low_pitch = 36;
    preview.high_pitch = 84;
    preview.low_velocity = 0.1;
    preview.high_velocity = 1.0;
    let metadata = CompiledProject::compile_with_assets(&metadata, &assets).expect("metadata");
    assert_eq!(baseline, render(&metadata, 48_000, 193));
}

#[test]
fn unused_spectral_assets_and_instruments_allocate_no_derivative() {
    let (mut project, _) = fixture();
    let mut unused_asset = project.assets[0].clone();
    unused_asset.id = "unused-texture".to_owned();
    unused_asset.name = "Unused texture".to_owned();
    unused_asset.path = "media/multisample-low.wav".into();
    unused_asset.sha256 =
        "5675332919d951ff59291a90b991e596a443e5b368b4360dfca3b157e161a0e0".to_owned();
    unused_asset.usages = vec![MediaAssetUsage::Spectral];
    project.assets.push(unused_asset);
    let mut unused_instrument = project.song.instruments[0].clone();
    unused_instrument.id = "unused-spectral".to_owned();
    unused_instrument.name = "Unused Spectral".to_owned();
    unused_instrument
        .graph
        .as_mut()
        .expect("graph")
        .nodes
        .iter_mut()
        .find(|node| node.id == "source")
        .expect("source")
        .settings
        .iter_mut()
        .find(|setting| setting.key == "asset")
        .expect("asset")
        .value = SettingValue::Text("unused-texture".to_owned());
    project.song.instruments.push(unused_instrument);
    assert!(!validate_project(&project).has_errors());
    let (assets, diagnostics) =
        load_asset_inventory(Path::new("fixtures/synth-alpha"), &project.assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let compiled =
        CompiledProject::compile_with_assets(&project, &assets).expect("referenced only");
    assert_eq!(compiled.sample_source_inspection().len(), 1);
    assert_eq!(compiled.spectral_analysis_inspection().len(), 1);
    assert_eq!(
        compiled.spectral_analysis_inspection()[0].asset_id,
        "source-texture"
    );
}

#[test]
fn scan_pitch_region_transient_and_phase_controls_are_effective_and_bounded() {
    let (project, assets) = fixture();
    let baseline = CompiledProject::compile_with_assets(&project, &assets).expect("baseline");
    let baseline = render(&baseline, 48_000, 97);

    let variants = [
        ("rate", SettingValue::Number(-0.75)),
        ("tune", SettingValue::Text("12st".to_owned())),
        ("low", SettingValue::Text("5khz".to_owned())),
        ("transients", SettingValue::Text("suppress".to_owned())),
        ("phase_randomization", SettingValue::Number(0.9)),
        ("freeze", SettingValue::Text("true".to_owned())),
    ];
    for (key, value) in variants {
        let mut changed = project.clone();
        set(&mut changed, key, value);
        let changed =
            CompiledProject::compile_with_assets(&changed, &assets).expect("spectral variant");
        let output = render(&changed, 48_000, 113);
        assert_ne!(baseline, output, "{key} must affect resynthesis");
        assert!(output.iter().flatten().all(|sample| sample.is_finite()));
    }

    let mut maximum = project;
    set(
        &mut maximum,
        "max_peaks",
        SettingValue::Number(MAX_SPECTRAL_PEAKS as f64),
    );
    let maximum = CompiledProject::compile_with_assets(&maximum, &assets).expect("maximum peaks");
    assert!(maximum.mix_graph_inspection().resources.admitted);
    assert!(
        render(&maximum, 8_192, 127)
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
}

#[test]
fn malformed_spectral_contracts_fail_before_runtime() {
    let source =
        fs::read_to_string("fixtures/synth-alpha/minimal-spectral.crump").expect("spectral source");
    let version_one = parse_module(
        Path::new("spectral-v1.crump"),
        r#"crump-module 1
instrument spectral-v1 "Spectral v1" graph {
  polyphony 1
  graph 2 {
    node source spectral asset texture algorithm crump-spectral-peaks-v1
    node out output
    connect note.pitch -> source.pitch
    connect source.audio -> out.audio
  }
}
"#,
    );
    assert!(
        version_one.diagnostics.as_slice().iter().any(|diagnostic| {
            diagnostic.code == "CRP173" && diagnostic.message.contains("spectral")
        }),
        "{:?}",
        version_one.diagnostics
    );
    let nested = parse_project(
        Path::new("spectral-nested.crump"),
        &source.replace(
            "          algorithm crump-spectral-peaks-v1",
            "          algorithm crump-spectral-peaks-v1 {\n            ignored true\n          }",
        ),
    );
    assert!(
        nested
            .diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRP170")
    );

    let (project, assets) = fixture();
    let mut excessive = project.clone();
    set(
        &mut excessive,
        "max_peaks",
        SettingValue::Number((MAX_SPECTRAL_PEAKS + 1) as f64),
    );
    assert!(
        validate_project(&excessive)
            .as_slice()
            .iter()
            .any(|diagnostic| {
                diagnostic.code == "CRV051" && diagnostic.message.contains("max_peaks")
            })
    );
    assert!(CompiledProject::compile_with_assets(&excessive, &assets).is_err());

    let mut wrong_algorithm = project.clone();
    set(
        &mut wrong_algorithm,
        "algorithm",
        SettingValue::Text("unknown-spectral".to_owned()),
    );
    assert!(
        validate_project(&wrong_algorithm)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV051" || diagnostic.code == "CRV200")
    );

    let mut crossed = project.clone();
    set(&mut crossed, "low", SettingValue::Text("10khz".to_owned()));
    set(&mut crossed, "high", SettingValue::Text("5khz".to_owned()));
    assert!(
        validate_project(&crossed)
            .as_slice()
            .iter()
            .any(|diagnostic| {
                diagnostic.code == "CRV200" && diagnostic.message.contains("strictly below")
            })
    );

    let mut wrong_usage = project.clone();
    wrong_usage.assets[0]
        .usages
        .retain(|usage| *usage != MediaAssetUsage::Spectral);
    assert!(
        validate_project(&wrong_usage)
            .as_slice()
            .iter()
            .any(|diagnostic| {
                diagnostic.code == "CRV196" && diagnostic.message.contains("spectral usage")
            })
    );

    let mut too_many = project;
    let mut second = source_node(&mut too_many).clone();
    second.id = "source-two".to_owned();
    let mut third = second.clone();
    third.id = "source-three".to_owned();
    too_many.song.instruments[0]
        .graph
        .as_mut()
        .expect("graph")
        .nodes
        .extend([second, third]);
    assert!(
        validate_project(&too_many)
            .as_slice()
            .iter()
            .any(|diagnostic| {
                diagnostic.code == "CRV200" && diagnostic.message.contains("at most 2")
            })
    );
}

#[test]
fn copied_spectral_project_rebuilds_without_an_external_path() {
    let (project, assets) = fixture();
    let original = CompiledProject::compile_with_assets(&project, &assets).expect("original");
    let copied = tempfile::tempdir().expect("clean account");
    fs::create_dir(copied.path().join("media")).expect("media");
    fs::copy(
        "fixtures/synth-alpha/minimal-spectral.crump",
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
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let copied_project = parsed.project.expect("Project");
    let (copied_assets, diagnostics) = load_asset_inventory(copied.path(), &copied_project.assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let reopened =
        CompiledProject::compile_with_assets(&copied_project, &copied_assets).expect("reopened");
    assert_eq!(
        original.spectral_analysis_inspection()[0].cache_identity,
        reopened.spectral_analysis_inspection()[0].cache_identity
    );
    assert_eq!(
        render(&original, 48_000, 64),
        render(&reopened, 48_000, 239)
    );
}
