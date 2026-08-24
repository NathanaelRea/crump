//! Checks for the curated expressive Instrument Modules.
//!
//! These checks cover readable modules, deterministic DSP, and resource admission. They do not
//! measure subjective listening quality.

use std::{fs, path::Path};

use crump::{
    dsl::parse_module,
    engine::{CompiledProject, ProcessingMode},
    project::load_project,
    validate::validate_instrument_module,
};

const MODULES: [&str; 5] = [
    "warm-pad.crump",
    "melodic-pluck.crump",
    "deep-bass.crump",
    "textured-percussion.crump",
    "transition-noise.crump",
];

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/expressive")
}

#[test]
fn curated_devices_are_readable_modules_with_progressive_depth() {
    let root = fixture_root();
    for name in MODULES {
        let path = root.join(name);
        let source = fs::read_to_string(&path).expect("read expressive module");
        let document = parse_module(&path, &source);
        assert!(
            document.diagnostics.is_empty(),
            "{name}: {:?}",
            document.diagnostics
        );
        let diagnostics = validate_instrument_module(&document.instruments);
        assert!(!diagnostics.has_errors(), "{name}: {diagnostics:?}");
        let instrument = document.instruments.first().expect("one Instrument");
        assert_eq!(document.instruments.len(), 1);
        assert!((3..=4).contains(&instrument.faceplate.len()));
        assert!(instrument.parameters.len() >= instrument.faceplate.len());
        assert!(instrument.graph.as_ref().is_some_and(|graph| {
            graph.nodes.len() >= instrument.faceplate.len()
                && graph.nodes.len() <= usize::from(graph.max_nodes)
        }));
    }
}

#[test]
fn expressive_fixture_is_deterministic_finite_stereo_and_admitted() {
    let bundle = load_project(fixture_root()).expect("load expressive fixture");
    assert!(
        !bundle.diagnostics.has_errors(),
        "{:?}",
        bundle.diagnostics.as_slice()
    );
    let project = bundle.project.as_ref().expect("Project");
    let compiled = CompiledProject::compile(project).expect("compile expressive fixture");
    let resources = &compiled.mix_graph_inspection().resources;
    assert!(resources.admitted);
    assert_eq!(resources.graph_voices, 27);
    assert_eq!(resources.graph_node_instances, 506);
    assert_eq!(resources.processors, 6);
    assert_eq!(resources.work_units_per_frame, 2_437);
    assert_eq!(resources.publication_runtime_slots, 2);
    assert!(resources.graph_voice_state_bytes > 100_000);
    assert!(resources.processor_state_bytes > 1_000_000);

    fn first_second(compiled: &CompiledProject, quantum: usize) -> Vec<[f32; 2]> {
        let mut runtime = compiled.runtime();
        let mut rendered = Vec::with_capacity(48_000);
        while rendered.len() < 48_000 {
            let count = quantum.min(48_000 - rendered.len());
            let mut block = vec![[0.0_f32; 2]; count];
            let written = runtime
                .process_block(&mut block, ProcessingMode::Offline)
                .expect("finite render");
            assert_eq!(written, count);
            rendered.extend(block);
        }
        rendered
    }

    let at_64 = first_second(&compiled, 64);
    let at_128 = first_second(&compiled, 128);
    let at_256 = first_second(&compiled, 256);
    assert_eq!(at_64, at_128);
    assert_eq!(at_128, at_256);
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
            > 0.1
    );
}
