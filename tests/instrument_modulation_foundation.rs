//! End-to-end contract for stable Instrument-owned Modulation.

use std::{fs, path::Path};

use crump::{
    dsl::parse_project,
    engine::{CompiledProject, ProcessingMode},
    production_view::{Operation, Provenance, Request, execute},
    validate::validate_project,
};

fn project() -> crump::model::Project {
    let source = include_str!("fixtures/instrument_modulation.crump");
    let document = parse_project(Path::new("instrument_modulation.crump"), source);
    assert!(
        !document.diagnostics.has_errors(),
        "{:?}",
        document.diagnostics.as_slice()
    );
    let project = document.project.expect("version 2 Project");
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.as_slice());
    project
}

fn render_grouped(compiled: &CompiledProject, quantum: usize, frames: usize) -> Vec<[u32; 2]> {
    let mut runtime = compiled.runtime();
    let mut output = Vec::with_capacity(frames);
    while output.len() < frames {
        let count = quantum.min(frames - output.len());
        let mut block = vec![[0.0_f32; 2]; count];
        assert_eq!(
            runtime
                .process_block(&mut block, ProcessingMode::Offline)
                .expect("finite render"),
            count
        );
        output.extend(
            block
                .into_iter()
                .map(|frame| [frame[0].to_bits(), frame[1].to_bits()]),
        );
    }
    output
}

#[test]
fn parser_validation_compiler_and_runtime_share_instrument_modulation() {
    let project = project();
    let compiled = CompiledProject::compile(&project).expect("compile Instrument Modulation");
    let tone = compiled
        .control_snapshot_at_frame(0)
        .into_iter()
        .find(|snapshot| snapshot.target.to_string() == "instrument/lead/tone")
        .expect("tone provenance");
    assert_eq!(
        tone.instrument_modulation_routes
            .iter()
            .map(|route| route.route.as_str())
            .collect::<Vec<_>>(),
        [
            "macro-tone",
            "motion-tone",
            "velocity-tone",
            "key-tone",
            "gate-tone",
            "age-tone",
            "random-tone",
            "envelope-tone",
            "graph-lfo-tone",
        ]
    );
    assert_eq!(
        tone.modulation_routes
            .iter()
            .map(|route| route.route.as_str())
            .collect::<Vec<_>>(),
        ["song-tone"]
    );
    assert!(tone.instrument_modulation_routes[0].contribution.is_some());
    assert!(
        tone.instrument_modulation_routes[1..]
            .iter()
            .all(|route| route.contribution.is_none())
    );
    let q64 = render_grouped(&compiled, 64, 96_000);
    assert_eq!(q64, render_grouped(&compiled, 128, 96_000));
    assert_eq!(q64, render_grouped(&compiled, 256, 96_000));
    assert!(q64.iter().any(|frame| frame != &[0, 0]));
    assert!(q64.iter().all(|frame| frame[0] == frame[1]));
}

#[test]
fn bypass_and_route_order_are_audible_topology_stable_values() {
    let source = include_str!("fixtures/instrument_modulation.crump");
    let enabled = CompiledProject::compile(&project()).expect("enabled compile");
    let disabled_source = source.replace(
        "route macro-tone source macro-source target tone depth 0.2 map unipolar curve linear order -20 bypass false",
        "route macro-tone source macro-source target tone depth 0.2 map unipolar curve linear order 100 bypass true",
    );
    let parsed = parse_project(Path::new("instrument_modulation.crump"), &disabled_source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let disabled_project = parsed.project.expect("disabled Project");
    assert!(!validate_project(&disabled_project).has_errors());
    let disabled = CompiledProject::compile(&disabled_project).expect("disabled compile");
    assert_ne!(
        render_grouped(&enabled, 128, 4_096),
        render_grouped(&disabled, 128, 4_096)
    );
}

#[test]
fn checked_route_depth_and_bypass_edits_publish_exact_sites() {
    let directory = tempfile::tempdir().expect("temporary Project");
    fs::write(
        directory.path().join("project.crump"),
        include_str!("fixtures/instrument_modulation.crump"),
    )
    .expect("write Project");
    execute(
        directory.path(),
        Request {
            expected_revision: None,
            operation: Operation::SetInstrumentParameter {
                instrument: "lead".to_owned(),
                parameter: "macro".to_owned(),
                value: crump::production_view::PhysicalValue::Authored("0.75".to_owned()),
            },
            provenance: Provenance::human("test", "Macro value", "Exact checked edit"),
        },
    )
    .expect("publish macro value");
    execute(
        directory.path(),
        Request {
            expected_revision: None,
            operation: Operation::SetInstrumentRouteDepth {
                instrument: "lead".to_owned(),
                route: "macro-tone".to_owned(),
                depth: "0.35".to_owned(),
            },
            provenance: Provenance::human("test", "Route depth", "Exact checked edit"),
        },
    )
    .expect("publish route depth");
    execute(
        directory.path(),
        Request {
            expected_revision: None,
            operation: Operation::SetInstrumentRouteBypass {
                instrument: "lead".to_owned(),
                route: "macro-tone".to_owned(),
                bypass: true,
            },
            provenance: Provenance::human("test", "Route bypass", "Exact checked edit"),
        },
    )
    .expect("publish route bypass");
    let changed = fs::read_to_string(directory.path().join("project.crump")).expect("source");
    assert!(changed.contains("parameter macro \"Macro\" ratio 0 1 0.5 0.75"));
    assert!(changed.contains("macro-tone source macro-source target tone depth 0.35"));
    assert!(changed.contains("order -20 bypass true"));
    let parsed = parse_project(Path::new("project.crump"), &changed);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    assert!(!validate_project(parsed.project.as_ref().expect("edited Project")).has_errors());
}

#[test]
fn malformed_nested_and_duplicate_modulation_forms_fail_closed() {
    let source = include_str!("fixtures/instrument_modulation.crump");
    for malformed in [
        source.replace("curve power(2)", "curve power(foo)"),
        source.replace(
            "source motion lfo instrument bipolar sine free 1hz phase 0.25",
            "source motion lfo instrument bipolar sine free 1hz phase 0.25 {\n          add 0\n        }",
        ),
        source.replace(
            "          clamp 0 1",
            "          clamp 0 1 {\n            add 0\n          }",
        ),
        source.replace(
            "          auxiliary macro-source multiply",
            "          auxiliary macro-source multiply\n          auxiliary motion multiply",
        ),
        source.replace(
            "          clamp 0 1",
            "          clamp 0 1\n          clamp -1 1",
        ),
    ] {
        let parsed = parse_project(Path::new("malformed.crump"), &malformed);
        assert!(
            parsed.diagnostics.has_errors(),
            "malformed Instrument Modulation must not remain publishable: {malformed}"
        );
    }

    let version_one = include_str!("fixtures/minimal.crump").replace(
        "      polyphony 4",
        "      polyphony 4\n      modulation {\n        source velocity note_velocity per_voice unipolar\n      }",
    );
    let parsed = parse_project(Path::new("version-one.crump"), &version_one);
    assert!(parsed.diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRP174"
            && diagnostic
                .message
                .contains("Instrument-owned Modulation requires source version 2")
    }));

    let mut in_memory_v1 = parse_project(Path::new("version-two.crump"), source)
        .project
        .expect("version 2 Project");
    in_memory_v1.source_version = 1;
    assert!(
        validate_project(&in_memory_v1)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV190")
    );
    assert!(CompiledProject::compile(&in_memory_v1).is_err());

    let mut unsupported = in_memory_v1;
    unsupported.source_version = 99;
    assert!(
        validate_project(&unsupported)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV189")
    );
    assert!(CompiledProject::compile(&unsupported).is_err());
}

#[test]
fn invalid_owner_scope_and_auxiliary_graph_cycles_fail_validation() {
    let source = include_str!("fixtures/instrument_modulation.crump");
    let non_graph = source.replacen(
        "instrument lead \"Lead\" graph {",
        "instrument lead \"Lead\" stock_drums {",
        1,
    );
    let parsed = parse_project(Path::new("non-graph.crump"), &non_graph);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let diagnostics = validate_project(parsed.project.as_ref().expect("non-graph Project"));
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV169")
    );

    let mixed_scope = source
        .replace(
            "parameter tone \"Tone\" ratio 0 1 0.1 0.2 automatable modulatable",
            "parameter tone \"Tone\" ratio 0 1 0.1 0.2 automatable modulatable macro",
        )
        .replace(
            "source motion lfo instrument bipolar sine free 1hz phase 0.25",
            "source motion parameter tone instrument unipolar",
        )
        .replace(
            "route motion-tone source motion target tone depth 0.1",
            "route motion-tone source motion target macro depth 0.1",
        );
    let parsed = parse_project(Path::new("mixed-scope.crump"), &mixed_scope);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let diagnostics = validate_project(parsed.project.as_ref().expect("mixed-scope Project"));
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV188")
    );

    let auxiliary_cycle = source
        .replace(
            "parameter tone \"Tone\" ratio 0 1 0.1 0.2 automatable modulatable",
            "parameter tone \"Tone\" ratio 0 1 0.1 0.2 automatable modulatable\n      parameter rate \"Rate\" hz 0.01hz 20hz 1hz 2hz modulatable",
        )
        .replace("node graph-lfo lfo waveform triangle rate 2hz depth 1", "node graph-lfo lfo waveform triangle rate $rate depth 1")
        .replace(
            "route velocity-tone source velocity target tone depth 0.1 map unipolar curve linear order 0 bypass false",
            "route velocity-tone source velocity target rate depth 1hz map unipolar curve linear order 0 bypass false {\n          auxiliary graph-lfo multiply\n        }",
        );
    let parsed = parse_project(Path::new("auxiliary-cycle.crump"), &auxiliary_cycle);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let diagnostics = validate_project(parsed.project.as_ref().expect("cycle Project"));
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV186")
    );

    let global_locked_macro = source.replace(
        "      note 0beat C4 1beat 0.8",
        "      lock locked-macro at 0beat parameter macro value 0.9\n      note 0beat C4 1beat 0.8",
    );
    let parsed = parse_project(Path::new("global-locked-macro.crump"), &global_locked_macro);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let diagnostics = validate_project(parsed.project.as_ref().expect("locked macro Project"));
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV192"),
        "Song-global sources must not sample a Track-owned lock"
    );

    for polarity_mismatch in [
        source.replace(
            "source envelope port envelope.control per_voice unipolar",
            "source envelope port envelope.control per_voice bipolar",
        ),
        source.replace(
            "source graph-lfo port graph-lfo.control per_voice bipolar",
            "source graph-lfo port graph-lfo.control per_voice unipolar",
        ),
    ] {
        let parsed = parse_project(Path::new("port-polarity.crump"), &polarity_mismatch);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        assert!(
            validate_project(parsed.project.as_ref().expect("polarity Project"))
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV191")
        );
    }
}

#[test]
fn expressive_lfo_modes_are_scoped_and_macro_cycles_fail_closed() {
    let source = include_str!("fixtures/instrument_modulation.crump");
    let version_one = source.replacen("crump 2", "crump 1", 1);
    assert!(
        parse_project(Path::new("version-one.crump"), &version_one)
            .diagnostics
            .has_errors(),
        "source-v1 must not acquire Instrument Modulation grammar"
    );
    let expressive = source.replace(
        "source motion lfo instrument bipolar sine free 1hz phase 0.25",
        "source motion lfo instrument bipolar square free 1hz phase 0.25 retrigger transport one_shot ping_pong",
    );
    let parsed = parse_project(Path::new("expressive-lfo.crump"), &expressive);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("expressive LFO Project");
    assert!(!validate_project(&project).has_errors());
    CompiledProject::compile(&project).expect("compile expressive Instrument LFO");

    let wrong_scope = source.replace(
        "source motion lfo instrument bipolar sine free 1hz phase 0.25",
        "source motion lfo instrument bipolar sine free 1hz phase 0.25 retrigger note",
    );
    let parsed = parse_project(Path::new("wrong-lfo-retrigger.crump"), &wrong_scope);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let invalid = parsed.project.expect("wrong-scope Project");
    assert!(
        validate_project(&invalid)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV175")
    );
    assert!(CompiledProject::compile(&invalid).is_err());

    let malformed = source.replace(
        "source motion lfo instrument bipolar sine free 1hz phase 0.25",
        "source motion lfo instrument bipolar sine free 1hz phase 0.25 retrigger free",
    );
    assert!(
        parse_project(Path::new("malformed-lfo.crump"), &malformed)
            .diagnostics
            .has_errors()
    );

    let cycle = source
        .replace(
            "parameter tone \"Tone\" ratio 0 1 0.1 0.2 automatable modulatable",
            "parameter tone \"Tone\" ratio 0 1 0.1 0.2 automatable modulatable macro",
        )
        .replace(
            "source motion lfo instrument bipolar sine free 1hz phase 0.25",
            "source motion parameter tone instrument unipolar",
        );
    let parsed = parse_project(Path::new("cycle.crump"), &cycle);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let diagnostics = validate_project(parsed.project.as_ref().expect("cycle Project"));
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV187")
    );
}
