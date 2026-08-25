//! End-to-end exact-asset wavetable compilation, playback, and morph checks.

use std::{fs, path::Path};

use crump::{
    dsl::parse_project,
    engine::{CompiledProject, ProcessingMode},
    media::load_asset_inventory,
    model::{SettingValue, WavetableMorph, WavetableNormalization},
    validate::{validate_project, validate_project_assets},
};

fn fixture() -> (crump::model::Project, crump::media::AssetInventory) {
    let path = Path::new("fixtures/synth-alpha/wavetable-demo.crump");
    let source = fs::read_to_string(path).expect("read wavetable demo");
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
    let mut rendered = Vec::with_capacity(frames);
    while rendered.len() < frames {
        let count = quantum.min(frames - rendered.len());
        let mut block = vec![[0.0; 2]; count];
        assert_eq!(
            runtime
                .process_block(&mut block, ProcessingMode::Offline)
                .expect("finite wavetable render"),
            count
        );
        rendered.extend(block);
    }
    rendered
}

#[test]
fn exact_table_compiles_rebuilds_and_renders_independently_of_callback_grouping() {
    let (project, assets) = fixture();
    let definition = project.assets[0]
        .wavetable
        .as_ref()
        .expect("wavetable definition");
    assert_eq!(definition.frame_length, 2_048);
    assert_eq!(definition.frames, 4);
    assert_eq!(definition.normalization, WavetableNormalization::GlobalPeak);
    assert_eq!(definition.morph, WavetableMorph::CatmullRom);

    let first = CompiledProject::compile_with_assets(&project, &assets)
        .expect("compile exact wavetable Project");
    let rebuilt = CompiledProject::compile_with_assets(&project, &assets)
        .expect("rebuild disposable wavetable derivatives");
    assert!(first.mix_graph_inspection().resources.admitted);
    assert!(
        first
            .mix_graph_inspection()
            .resources
            .instrument_program_bytes
            > 0
    );
    assert!(first.mix_graph_inspection().resources.work_units_per_frame > 0);
    let inspection = &first.wavetable_inspection()[0];
    assert_eq!(inspection.asset_id, "wt-basic");
    assert_eq!(inspection.source_sha256, project.assets[0].sha256);
    assert_eq!(inspection.algorithm, "crump-wavetable-radix2-v1");
    assert_eq!(inspection.frame_length, 2_048);
    assert_eq!(inspection.frames, 4);
    assert_eq!(
        inspection.max_harmonics,
        [1_024, 512, 256, 128, 64, 32, 16, 8, 4, 2, 1]
    );
    assert_eq!(inspection, &rebuilt.wavetable_inspection()[0]);

    let frames = 48_000;
    let at_64 = render(&first, frames, 64);
    let at_257 = render(&first, frames, 257);
    let rebuilt_output = render(&rebuilt, frames, 113);
    assert_eq!(at_64, at_257);
    assert_eq!(at_64, rebuilt_output);
    assert!(at_64.iter().flatten().all(|sample| sample.is_finite()));
    assert!(
        at_64
            .iter()
            .any(|frame| frame[0].abs().max(frame[1].abs()) > 1.0e-5)
    );
    assert!(
        at_64
            .iter()
            .map(|frame| (frame[0] - frame[1]).abs())
            .sum::<f32>()
            > 0.01,
        "authored unison width must produce deterministic stereo movement"
    );
}

#[test]
fn table_metadata_and_asset_binding_fail_closed() {
    let (project, assets) = fixture();

    let mut wrong_length = project.clone();
    wrong_length.assets[0]
        .wavetable
        .as_mut()
        .expect("definition")
        .frames = 3;
    let diagnostics = validate_project_assets(&wrong_length, &assets);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV197" && diagnostic.message.contains("WAV contains")
    }));
    assert!(CompiledProject::compile_with_assets(&wrong_length, &assets).is_err());

    let missing = crump::media::AssetInventory::default();
    let diagnostics = validate_project_assets(&project, &missing);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV197" && diagnostic.message.contains("not loaded")
    }));
    assert!(CompiledProject::compile_with_assets(&project, &missing).is_err());

    let mut missing_reference = project.clone();
    let source = missing_reference.song.instruments[0]
        .graph
        .as_mut()
        .expect("voice graph")
        .nodes
        .iter_mut()
        .find(|node| node.id == "source")
        .expect("wavetable source");
    source
        .settings
        .iter_mut()
        .find(|setting| setting.key == "asset")
        .expect("asset setting")
        .value = SettingValue::Text("missing-table".to_owned());
    let diagnostics = validate_project(&missing_reference);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV196" && diagnostic.message.contains("missing-table")
    }));
    assert!(CompiledProject::compile_with_assets(&missing_reference, &assets).is_err());
}

#[test]
fn copied_wavetable_project_reopens_without_an_external_asset_path() {
    let (original_project, original_assets) = fixture();
    let original = CompiledProject::compile_with_assets(&original_project, &original_assets)
        .expect("compile original wavetable Project");

    let copied = tempfile::tempdir().expect("temporary clean account");
    fs::create_dir(copied.path().join("media")).expect("create copied media directory");
    fs::copy(
        "fixtures/synth-alpha/wavetable-demo.crump",
        copied.path().join("project.crump"),
    )
    .expect("copy authoritative source");
    fs::copy(
        "fixtures/synth-alpha/media/basic-table.wav",
        copied.path().join("media/basic-table.wav"),
    )
    .expect("copy exact table bytes");

    let source = fs::read_to_string(copied.path().join("project.crump"))
        .expect("read copied authoritative source");
    let parsed = parse_project(&copied.path().join("project.crump"), &source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("copied Project");
    assert_eq!(project.assets[0].path, Path::new("media/basic-table.wav"));
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let (assets, diagnostics) = load_asset_inventory(copied.path(), &project.assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let diagnostics = validate_project_assets(&project, &assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let reopened = CompiledProject::compile_with_assets(&project, &assets)
        .expect("compile copied wavetable Project");

    assert_eq!(
        original.wavetable_inspection(),
        reopened.wavetable_inspection()
    );
    assert_eq!(
        render(&original, 16_384, 64),
        render(&reopened, 16_384, 251)
    );
}
