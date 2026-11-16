//! Contract checks for the frozen no-sample Musical Sketch fixture.

use crump::{
    engine::{CompiledProject, ProcessingMode},
    project::load_project,
    validate::check_musical_sketch,
};

#[test]
fn reference_project_passes_objective_release_checks() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/reference");
    let bundle = load_project(root).expect("load Reference Project");
    assert!(
        !bundle.diagnostics.has_errors(),
        "{:?}",
        bundle.diagnostics.as_slice()
    );
    let project = bundle.project.as_ref().expect("parsed Project");
    let report = check_musical_sketch(project);
    assert!(report.passed, "{:#?}", report.checks);
    assert_eq!(report.checks.len(), 7);

    let compiled = CompiledProject::compile(project).expect("compile Reference Project");
    assert_eq!(compiled.arrangement_frames(), 1_645_714);
    assert_eq!(compiled.total_frames(), 1_741_714);

    let mut runtime = compiled.runtime();
    let mut block = [[0.0_f32; 2]; 4_096];
    assert_eq!(
        runtime
            .process_block(&mut block, ProcessingMode::Offline)
            .expect("process"),
        4_096
    );
    assert!(block.into_iter().flatten().all(f32::is_finite));
}

#[test]
fn reference_runtime_can_loop_an_audition_phrase_without_stopping() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/reference");
    let bundle = load_project(root).expect("load Reference Project");
    let compiled = CompiledProject::compile(bundle.project.as_ref().expect("parsed Project"))
        .expect("compile");
    let mut runtime = compiled.runtime();
    runtime.advance_to(100).expect("advance");
    runtime.enable_audition_loop(100, 200).expect("enable loop");

    let mut block = [[0.0_f32; 2]; 250];
    assert_eq!(
        runtime
            .process_block(&mut block, ProcessingMode::Monitoring)
            .expect("process"),
        250
    );
    assert_eq!(runtime.frame(), 150);
    assert!(!runtime.finished());
}
