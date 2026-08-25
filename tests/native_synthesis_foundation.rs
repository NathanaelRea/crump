//! Executable contract for source-v2 explicit stereo graph foundations.

use std::path::Path;

use crump::{
    dsl::parse_project,
    engine::{CompiledProject, ProcessingMode},
    validate::validate_project,
};

fn compiled_foundation() -> CompiledProject {
    let source = include_str!("fixtures/stereo_foundation.crump");
    let document = parse_project(Path::new("stereo_foundation.crump"), source);
    assert!(
        !document.diagnostics.has_errors(),
        "{:?}",
        document.diagnostics.as_slice()
    );
    let project = document.project.expect("version 2 Project");
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.as_slice());
    CompiledProject::compile(&project).expect("compile admitted stereo foundation")
}

fn render_grouped(compiled: &CompiledProject, quantum: usize, frames: usize) -> Vec<[u32; 2]> {
    let mut runtime = compiled.runtime();
    let mut rendered = Vec::with_capacity(frames);
    while rendered.len() < frames {
        let count = quantum.min(frames - rendered.len());
        let mut block = vec![[0.0_f32; 2]; count];
        assert_eq!(
            runtime
                .process_block(&mut block, ProcessingMode::Offline)
                .expect("process foundation block"),
            count
        );
        rendered.extend(block.into_iter().map(|frame| {
            assert!(frame.into_iter().all(f32::is_finite));
            [frame[0].to_bits(), frame[1].to_bits()]
        }));
    }
    rendered
}

#[test]
fn explicit_stereo_foundation_is_grouping_independent_finite_and_audible() {
    let compiled = compiled_foundation();
    let q64 = render_grouped(&compiled, 64, 8_192);
    assert_eq!(q64, render_grouped(&compiled, 128, 8_192));
    assert_eq!(q64, render_grouped(&compiled, 256, 8_192));
    assert!(q64.iter().any(|frame| frame != &[0, 0]));
    assert!(q64.iter().all(|frame| frame[0] == frame[1]));
}
