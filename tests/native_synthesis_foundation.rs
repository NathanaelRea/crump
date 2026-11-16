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

#[test]
fn mono_sum_matches_a_unit_gain_mixer_and_rejects_stereo_inputs() {
    let render = |graph: &str| {
        let source = include_str!("fixtures/stereo_foundation.crump");
        let start = source.find("      graph voice 14 {").expect("graph");
        let end = source[start..].find("\n      }\n").expect("graph end") + start;
        let source = format!("{}{graph}{}", &source[..start], &source[end..]);
        let document = parse_project(Path::new("sum.crump"), &source);
        assert!(
            !document.diagnostics.has_errors(),
            "{:?}",
            document.diagnostics
        );
        let project = document.project.expect("Project");
        let diagnostics = validate_project(&project);
        if diagnostics.has_errors() {
            return Err(diagnostics);
        }
        Ok(render_grouped(
            &CompiledProject::compile(&project).expect("compile"),
            128,
            4_096,
        ))
    };
    let graph = |mix: &str, buses: &str| {
        format!(
            "      graph voice 5 {{
        node osc-a oscillator waveform saw level $gain
        node osc-b oscillator waveform square level $gain detune 7st
        {mix}
        node wide mono_to_stereo
        node out output
        bus pitch-a note.pitch -> osc-a.pitch
        bus pitch-b note.pitch -> osc-b.pitch
{buses}
        bus wide mix.audio -> wide.audio
        bus output wide.audio -> out.audio"
        )
    };
    let mixer = render(&graph(
        "node mix mixer a_level 1 b_level 1",
        "        bus a osc-a.audio -> mix.a\n        bus b osc-b.audio -> mix.b",
    ))
    .expect("mixer graph");
    let summed = render(&graph(
        "node mix sum max_inputs 2",
        "        bus a osc-a.audio -> mix.inputs\n        bus b osc-b.audio -> mix.inputs",
    ))
    .expect("sum graph");
    assert_eq!(mixer, summed);
    assert!(summed.iter().any(|frame| frame != &[0, 0]));

    let over_bound = render(&graph(
        "node mix sum max_inputs 1",
        "        bus a osc-a.audio -> mix.inputs\n        bus b osc-b.audio -> mix.inputs",
    ));
    assert!(over_bound.is_err());
    let stereo = render(&graph(
        "node mix sum\n        node pre mono_to_stereo",
        "        bus pre osc-a.audio -> pre.audio\n        bus a pre.audio -> mix.inputs\n        bus b osc-b.audio -> mix.inputs",
    ));
    assert!(stereo.is_err());
}
