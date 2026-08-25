use std::path::Path;

use crate::{
    dsl::parse_project,
    engine::CompiledProject,
    model::{ChannelRef, GraphDomain, OutputTarget, SettingValue, Unit},
};

use super::*;

const VALID: &str = r#"crump 1
project "Validation" {
  id validation
  seed 1
  sample_rate 48000
  render_tail 1s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument lead "Lead" graph {
      polyphony 4
      parameter tone "Tone" hz 100hz 10000hz 2000hz 2500hz
      faceplate {
        control tone "Tone" "Brightness" maps tone
      }
      graph 8 {
        node osc oscillator waveform saw
        node env envelope attack 10ms decay 100ms sustain 0.7 release 300ms
        node filter lowpass cutoff $tone resonance 0.1
        node amp amplifier
        node pan pan position 0
        node out output
        connect note.pitch -> osc.pitch
        connect note.gate -> env.gate
        connect osc.audio -> filter.audio
        connect filter.audio -> amp.audio
        connect env.control -> amp.gain
        connect amp.audio -> pan.audio
        connect pan.audio -> out.audio
      }
    }
    pattern notes 1bar {
      note 0beat C4 1beat 0.8
    }
    track lead-track "Lead" instrument lead level -6db pan 0 {
      clip lead-clip pattern notes start 0bar duration 1bar mode once
    }
  }
}
"#;

fn graph_setting<'a>(node: &'a mut crate::model::GraphNode, key: &str) -> &'a mut SettingValue {
    &mut node
        .settings
        .iter_mut()
        .find(|setting| setting.key == key)
        .unwrap_or_else(|| panic!("missing graph setting `{key}`"))
        .value
}

fn automation_project() -> Project {
    let source = VALID
        .replace(
            "parameter tone \"Tone\" hz 100hz 10000hz 2000hz 2500hz",
            "parameter tone \"Tone\" hz 100hz 10000hz 2000hz 2500hz automatable",
        )
        .replace(
            "    track lead-track \"Lead\" instrument lead level -6db pan 0 {",
            r#"    automation tone-sweep target instrument/lead/tone {
      point start 0bar 1000hz linear
      point open 2beat 8000hz power(2)
    }
    track lead-track "Lead" instrument lead level -6db pan 0 {"#,
        );
    let document = parse_project(Path::new("project.crump"), &source);
    assert!(!document.diagnostics.has_errors());
    document.project.expect("Automation Project")
}

#[test]
fn accepts_valid_graph_and_timeline() {
    let document = parse_project(Path::new("project.crump"), VALID);
    assert!(!document.diagnostics.has_errors());
    let diagnostics = validate_project(document.project.as_ref().unwrap());
    for diagnostic in diagnostics.as_slice() {
        eprintln!("{diagnostic}");
    }
    assert!(!diagnostics.has_errors());
}

#[test]
fn accepts_version_two_explicit_stereo_graph_and_exact_domains() {
    let source = include_str!("../../tests/fixtures/stereo_foundation.crump");
    let document = parse_project(Path::new("stereo_foundation.crump"), source);
    assert!(
        !document.diagnostics.has_errors(),
        "{:?}",
        document.diagnostics
    );
    let project = document.project.expect("version 2 Project");
    let graph = project.song.instruments[0]
        .graph
        .as_ref()
        .expect("voice graph");
    assert_eq!(graph.domain, GraphDomain::Voice);
    assert!(graph.connections.iter().all(|bus| bus.id_site.is_some()));
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.as_slice());

    let mixed = source
        .replace(
            "bus pitch-a note.pitch -> osc-a.pitch",
            "connect note.pitch -> osc-a.pitch",
        )
        .replace("bus pitch-b", "bus compat-bus-0001");
    let parsed = parse_project(Path::new("mixed-connections.crump"), &mixed);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let diagnostics = validate_project(parsed.project.as_ref().expect("Project"));
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.as_slice());
}

#[test]
fn rejects_stereo_type_cardinality_sum_bound_and_duplicate_bus_ids() {
    let source = include_str!("../../tests/fixtures/stereo_foundation.crump");
    let cases = [
        (
            source.replace(
                "bus widen-a osc-a.audio -> widen-a.audio",
                "bus widen-a osc-a.audio -> sum.inputs",
            ),
            "CRV044",
        ),
        (
            source.replace(
                "bus output sum.audio -> out.audio",
                "bus output sum.audio -> out.audio\n        bus output-two widen-a.audio -> out.audio",
            ),
            "CRV046",
        ),
        (source.replace("max_inputs 3", "max_inputs 2"), "CRV061"),
        (source.replace("bus z-cancel", "bus a-large"), "CRV060"),
    ];
    for (candidate, code) in cases {
        let document = parse_project(Path::new("invalid-stereo.crump"), &candidate);
        assert!(
            !document.diagnostics.has_errors(),
            "{code}: {:?}",
            document.diagnostics
        );
        let diagnostics = validate_project(document.project.as_ref().expect("Project"));
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == code),
            "missing {code}: {:?}",
            diagnostics.as_slice()
        );
    }
}

#[test]
fn incomplete_asset_source_forms_fail_validation_and_compilation() {
    let source = include_str!("../../tests/fixtures/stereo_foundation.crump");
    let incomplete = source.replace(
        "node osc-a oscillator waveform saw level $gain",
        "node osc-a spectral asset pending-sample",
    );
    let parsed = parse_project(Path::new("incomplete-spectral.crump"), &incomplete);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.as_ref().expect("Project");
    let diagnostics = validate_project(project);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV200" && diagnostic.message.contains("algorithm")
    }));
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV196" && diagnostic.message.contains("pending-sample")
    }));
    assert!(CompiledProject::compile(project).is_err());
}

#[test]
fn musical_sketch_role_requires_an_in_range_event() {
    let document = parse_project(Path::new("project.crump"), VALID);
    let mut project = document.project.expect("Project");
    let before = check_musical_sketch(&project);
    assert!(
        before
            .checks
            .iter()
            .find(|check| check.id == "graph_instrument")
            .expect("graph check")
            .passed
    );

    project.song.patterns[0].events.clear();
    let after = check_musical_sketch(&project);
    assert!(
        !after
            .checks
            .iter()
            .find(|check| check.id == "graph_instrument")
            .expect("graph check")
            .passed
    );
}

#[test]
fn musical_sketch_rejects_declared_media_assets() {
    let source = VALID.replace(
        "  song {",
        concat!(
            "  asset voice \"Voice\" wav \"media/voice.wav\" sha256 \"",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "\"\n  song {"
        ),
    );
    let document = parse_project(Path::new("project.crump"), &source);
    assert!(!document.diagnostics.has_errors());
    let report = check_musical_sketch(document.project.as_ref().expect("Project"));
    assert!(
        !report
            .checks
            .iter()
            .find(|check| check.id == "no_media_assets")
            .expect("media check")
            .passed
    );
}

#[test]
fn unused_effects_graph_does_not_satisfy_creative_processing() {
    let document = parse_project(
        Path::new("project.crump"),
        include_str!("../../fixtures/reference/project.crump"),
    );
    let mut project = document.project.expect("Reference Project");
    project
        .song
        .tracks
        .iter_mut()
        .find(|track| track.id == "lead")
        .expect("lead Track")
        .instrument = "bass".to_owned();

    let report = check_musical_sketch(&project);
    assert!(
        report
            .checks
            .iter()
            .find(|check| check.id == "valid_project")
            .expect("validity check")
            .passed
    );
    assert!(
        report
            .checks
            .iter()
            .find(|check| check.id == "graph_instrument")
            .expect("graph check")
            .passed
    );
    assert!(
        !report
            .checks
            .iter()
            .find(|check| check.id == "creative_processing")
            .expect("processing check")
            .passed
    );
}

#[test]
fn validates_automation_identity_time_domain_and_runtime_safety() {
    let project = automation_project();
    assert!(!validate_project(&project).has_errors());

    let mut invalid = project.clone();
    invalid.song.automations[0].points[0].at = crate::time::MusicalTime::new(1, 1).unwrap();
    invalid.song.automations[0].points[1].value = "20000hz".to_owned();
    let mut duplicate = invalid.song.automations[0].clone();
    duplicate.id = "duplicate-target".to_owned();
    invalid.song.automations.push(duplicate);
    let diagnostics = validate_project(&invalid);
    for code in ["CRV103", "CRV109", "CRV115"] {
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == code),
            "missing {code}: {:?}",
            diagnostics.as_slice()
        );
    }

    let mut not_automatable = project.clone();
    not_automatable.song.instruments[0].parameters[0].automatable = false;
    assert!(
        validate_project(&not_automatable)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV105")
    );

    let mut allocation_changing = project;
    let parameter = &mut allocation_changing.song.instruments[0].parameters[0];
    parameter.unit = Unit::Seconds;
    parameter.minimum = 0.001;
    parameter.maximum = 2.0;
    parameter.default = 0.2;
    parameter.value = 0.2;
    let filter = allocation_changing.song.instruments[0]
        .graph
        .as_mut()
        .expect("graph")
        .nodes
        .iter_mut()
        .find(|node| node.id == "filter")
        .expect("filter");
    filter.kind = NodeKind::Delay;
    filter.settings.retain(|setting| setting.key == "cutoff");
    filter.settings[0].key = "time".to_owned();
    filter.settings[0].value = SettingValue::Parameter("tone".to_owned());
    allocation_changing.song.automations[0].points[0].value = "100ms".to_owned();
    allocation_changing.song.automations[0].points[1].value = "1s".to_owned();
    assert!(
        validate_project(&allocation_changing)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV106")
    );
}

#[test]
fn validates_variation_lineage_pitch_and_parameter_lock_conflicts() {
    let source = VALID
        .replace(
            "parameter tone \"Tone\" hz 100hz 10000hz 2000hz 2500hz",
            "parameter tone \"Tone\" hz 100hz 10000hz 2000hz 2500hz automatable",
        )
        .replace(
            r#"    pattern notes 1bar {
      note 0beat C4 1beat 0.8
    }"#,
            r#"    pattern notes 1bar {
      lock bright at 0beat parameter tone value 8000hz
      note 0beat C4 1beat 0.8
    }
    pattern answer 1bar from notes {
      rotate 1beat
      transpose 12st
      velocity 0.8
      reverse false
      probability 1
      seed 77
    }"#,
        )
        .replace(
            "clip lead-clip pattern notes",
            "clip lead-clip pattern answer",
        );
    let document = parse_project(Path::new("project.crump"), &source);
    assert!(
        !document.diagnostics.has_errors(),
        "{:?}",
        document.diagnostics.as_slice()
    );
    let valid = document.project.expect("Variation and lock Project");
    assert!(
        !validate_project(&valid).has_errors(),
        "valid Variation and lock Project"
    );

    let mut cumulative_pitch = valid.clone();
    let mut chained = cumulative_pitch
        .song
        .patterns
        .iter()
        .find(|pattern| pattern.id == "answer")
        .expect("answer Pattern")
        .clone();
    chained.id = "answer-two".to_owned();
    let variation = chained.variation.as_mut().expect("Variation lineage");
    variation.source = "answer".to_owned();
    variation.transpose_semitones = 56;
    cumulative_pitch.song.patterns.push(chained);
    cumulative_pitch.song.tracks[0].clips[0].pattern = "answer-two".to_owned();
    assert!(
        validate_project(&cumulative_pitch)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV088")
    );

    let mut cyclic = valid.clone();
    cyclic
        .song
        .patterns
        .iter_mut()
        .find(|pattern| pattern.id == "answer")
        .expect("answer Pattern")
        .variation
        .as_mut()
        .expect("Variation lineage")
        .source = "answer".to_owned();
    assert!(
        validate_project(&cyclic)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV160")
    );

    let mut overlapping = valid.clone();
    let mut duplicate = overlapping.song.tracks[0].clone();
    duplicate.id = "lead-track-two".to_owned();
    duplicate.name = "Lead Two".to_owned();
    duplicate.clips[0].id = "lead-clip-two".to_owned();
    overlapping.song.tracks.push(duplicate);
    assert!(
        validate_project(&overlapping)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV169")
    );

    let mut out_of_domain = valid;
    out_of_domain
        .song
        .patterns
        .iter_mut()
        .find(|pattern| pattern.id == "notes")
        .expect("source Pattern")
        .locks[0]
        .value = "20000hz".to_owned();
    assert!(
        validate_project(&out_of_domain)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV167")
    );
}

#[test]
fn rejects_out_of_domain_parameter() {
    let invalid = VALID.replace("2500hz", "25000hz");
    let document = parse_project(Path::new("project.crump"), &invalid);
    let diagnostics = validate_project(document.project.as_ref().unwrap());
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV024")
    );
}

#[test]
fn rejects_runtime_only_graph_setting_failures_before_publication() {
    let document = parse_project(Path::new("project.crump"), VALID);
    let mut project = document.project.expect("Project");
    let graph = project.song.instruments[0].graph.as_mut().expect("graph");
    let envelope = graph
        .nodes
        .iter_mut()
        .find(|node| node.kind == NodeKind::Envelope)
        .expect("envelope");
    *graph_setting(envelope, "release") = SettingValue::Text("9s".to_owned());
    let diagnostics = validate_project(&project);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV055")
    );
}

#[test]
fn rejects_missing_physical_unit_suffixes() {
    let document = parse_project(Path::new("project.crump"), VALID);
    let mut project = document.project.expect("Project");
    let graph = project.song.instruments[0].graph.as_mut().expect("graph");
    let envelope = graph
        .nodes
        .iter_mut()
        .find(|node| node.kind == NodeKind::Envelope)
        .expect("envelope");
    *graph_setting(envelope, "attack") = SettingValue::Number(0.01);
    let diagnostics = validate_project(&project);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV056")
    );
}

#[test]
fn rejects_parameter_domains_that_exceed_mapped_setting_domain() {
    let document = parse_project(Path::new("project.crump"), VALID);
    let mut project = document.project.expect("Project");
    let instrument = &mut project.song.instruments[0];
    let parameter = &mut instrument.parameters[0];
    parameter.unit = Unit::Seconds;
    parameter.minimum = 0.0;
    parameter.maximum = 20.0;
    parameter.default = 0.3;
    parameter.value = 0.3;
    let envelope = instrument
        .graph
        .as_mut()
        .expect("graph")
        .nodes
        .iter_mut()
        .find(|node| node.kind == NodeKind::Envelope)
        .expect("envelope");
    *graph_setting(envelope, "release") = SettingValue::Parameter("tone".to_owned());
    let diagnostics = validate_project(&project);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV057")
    );
}

#[test]
fn rejects_faceplate_controls_without_a_sound_mapping() {
    let document = parse_project(Path::new("project.crump"), VALID);
    let mut project = document.project.expect("Project");
    let graph = project.song.instruments[0].graph.as_mut().expect("graph");
    let filter = graph
        .nodes
        .iter_mut()
        .find(|node| node.kind == NodeKind::Lowpass)
        .expect("filter");
    *graph_setting(filter, "cutoff") = SettingValue::Text("2000hz".to_owned());
    let diagnostics = validate_project(&project);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV034")
    );
}

#[test]
fn rejects_stock_drum_parameters_without_native_mappings() {
    let document = parse_project(
        Path::new("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    );
    let mut project = document.project.expect("Project");
    let drums = project
        .song
        .instruments
        .iter_mut()
        .find(|instrument| instrument.kind == InstrumentKind::StockDrums)
        .expect("drums");
    drums.parameters[0].key = "unused".to_owned();
    drums.parameters[0].maximum = 4.0;
    drums.faceplate[0].parameter = "unused".to_owned();
    let diagnostics = validate_project(&project);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV025")
    );
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV026")
    );
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV034")
    );
}

#[test]
fn standalone_module_requires_exactly_one_instrument() {
    let document = parse_project(Path::new("project.crump"), VALID);
    let instrument = document
        .project
        .expect("Project")
        .song
        .instruments
        .remove(0);
    let diagnostics = validate_instrument_module(&[instrument.clone(), instrument]);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV017")
    );
}

#[test]
fn graph_voice_bound_counts_each_runtime_track_instance() {
    let document = parse_project(Path::new("project.crump"), VALID);
    let mut project = document.project.expect("Project");
    project.song.instruments[0].polyphony = 64;
    let template = project.song.tracks[0].clone();
    project.song.tracks.clear();
    for index in 0..4 {
        let mut track = template.clone();
        track.id = format!("lead-{index}");
        track.name = format!("Lead {index}");
        track.clips[0].id = format!("lead-clip-{index}");
        project.song.tracks.push(track);
    }
    assert!(
        validate_project(&project)
            .as_slice()
            .iter()
            .all(|diagnostic| diagnostic.code != "CRV015"),
        "exactly {MAX_GRAPH_VOICES} runtime voices must be admitted"
    );

    let mut extra = template;
    extra.id = "lead-overflow".to_owned();
    extra.name = "Lead Overflow".to_owned();
    extra.clips[0].id = "lead-clip-overflow".to_owned();
    project.song.tracks.push(extra);
    assert!(
        validate_project(&project)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV015")
    );
}

#[test]
fn rejects_unbounded_callback_time_parameter_targets() {
    let document = parse_project(Path::new("project.crump"), VALID);
    let mut project = document.project.expect("Project");
    let template = project.song.instruments[0].parameters[0].clone();
    for index in 0..MAX_CONTROL_TARGETS {
        let mut parameter = template.clone();
        parameter.key = format!("extra-{index}");
        parameter.name = format!("Extra {index}");
        project.song.instruments[0].parameters.push(parameter);
    }
    assert!(
        validate_project(&project)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV118")
    );
}

#[test]
fn routing_cycles_and_sidechain_contracts_have_focused_diagnostics() {
    let document = parse_project(
        Path::new("production_mix.crump"),
        include_str!("../../tests/fixtures/production_mix.crump"),
    );
    assert!(
        !document.diagnostics.has_errors(),
        "{:?}",
        document.diagnostics
    );
    let project = document.project.expect("Project");

    let mut send_cycle = project.clone();
    send_cycle.song.returns[0].channel.output = OutputTarget::Group("mix_bus".to_owned());
    let diagnostics = validate_project(&send_cycle);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| { diagnostic.code == "CRV113" && diagnostic.location.is_some() })
    );

    let mut sidechain_cycle = project.clone();
    sidechain_cycle.song.sidechains[0].from = ChannelRef::Group("mix_bus".to_owned());
    assert!(
        validate_project(&sidechain_cycle)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV113")
    );

    let mut missing_port = project.clone();
    missing_port.song.sidechains[0].input = "missing".to_owned();
    assert!(
        validate_project(&missing_port)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV111" && diagnostic.location.is_some())
    );

    let mut duplicate_destination = project;
    let mut duplicate = duplicate_destination.song.sidechains[0].clone();
    duplicate.id = "duplicate_detector".to_owned();
    duplicate_destination.song.sidechains.push(duplicate);
    assert!(
        validate_project(&duplicate_destination)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV112" && diagnostic.location.is_some())
    );
}

#[test]
fn processor_contracts_reject_unimplemented_or_unconsumed_configuration() {
    let document = parse_project(
        Path::new("production_mix.crump"),
        include_str!("../../tests/fixtures/production_mix.crump"),
    );
    assert!(
        !document.diagnostics.has_errors(),
        "{:?}",
        document.diagnostics
    );
    let project = document.project.expect("Project");

    let mut disconnected_sidechain = project.clone();
    disconnected_sidechain.song.sidechains.clear();
    assert!(
        validate_project(&disconnected_sidechain)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV135" && diagnostic.location.is_some())
    );

    let mut wrong_sidechain_kind = project.clone();
    wrong_sidechain_kind.song.returns[0].channel.inserts[0]
        .sidechain_inputs
        .push("detector".to_owned());
    assert!(
        validate_project(&wrong_sidechain_kind)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV133" && diagnostic.location.is_some())
    );

    let mut too_many_compressor_inputs = project.clone();
    too_many_compressor_inputs.song.tracks[1].channel.inserts[0]
        .sidechain_inputs
        .push("alternate".to_owned());
    assert!(
        validate_project(&too_many_compressor_inputs)
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV134" && diagnostic.location.is_some())
    );

    let supported_sync = include_str!("../../tests/fixtures/production_mix.crump").replace(
            "    master level -1db {",
            "    master level -1db {\n      processor sync_probe delay {\n        setting tempo_sync true\n        setting division 1/8d\n        setting mode ping_pong\n      }",
        );
    let parsed = parse_project(Path::new("supported-sync.crump"), &supported_sync);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    assert!(!validate_project(&parsed.project.expect("Project")).has_errors());

    let conflicting_sync = supported_sync.replace(
        "        setting tempo_sync true",
        "        setting tempo_sync true\n        setting time 0.2s",
    );
    let parsed = parse_project(Path::new("conflicting-sync.crump"), &conflicting_sync);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    assert!(
        validate_project(&parsed.project.expect("Project"))
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV136" && diagnostic.location.is_some())
    );

    let orphaned_division = supported_sync.replace(
        "        setting tempo_sync true\n",
        "        setting tempo_sync false\n",
    );
    let parsed = parse_project(Path::new("orphaned-division.crump"), &orphaned_division);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    assert!(
        validate_project(&parsed.project.expect("Project"))
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV136" && diagnostic.location.is_some())
    );

    let invalid_division = supported_sync.replace("setting division 1/8d", "setting division 1/7");
    let parsed = parse_project(Path::new("invalid-division.crump"), &invalid_division);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    assert!(
        validate_project(&parsed.project.expect("Project"))
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV124" && diagnostic.location.is_some())
    );

    let excessive_lookahead = include_str!("../../tests/fixtures/production_mix.crump")
        .replace("setting lookahead 0.005s", "setting lookahead 0.5s");
    let parsed = parse_project(Path::new("lookahead.crump"), &excessive_lookahead);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    assert!(
        validate_project(&parsed.project.expect("Project"))
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV122" && diagnostic.location.is_some())
    );
}

#[test]
fn rejects_fixed_processor_telemetry_capacity_before_runtime_allocation() {
    let document = parse_project(
        Path::new("production_mix.crump"),
        include_str!("../../tests/fixtures/production_mix.crump"),
    );
    let mut project = document.project.expect("Project");
    let template = project.song.tracks[0].channel.inserts[0].clone();
    for index in 0..crate::engine::mix_graph::MAX_PROCESSOR_INSTANCES {
        let mut processor = template.clone();
        processor.id = format!("extra_latency_{index}");
        project.song.tracks[0].channel.inserts.push(processor);
    }
    let diagnostic = validate_project(&project)
        .as_slice()
        .iter()
        .find(|diagnostic| diagnostic.code == "CRV092")
        .cloned()
        .expect("processor telemetry capacity diagnostic");
    assert!(diagnostic.message.contains("fixed telemetry capacity"));
}

#[test]
fn repeated_instrument_post_chains_count_as_runtime_processor_instances() {
    let document = parse_project(
        Path::new("production_mix.crump"),
        include_str!("../../tests/fixtures/production_mix.crump"),
    );
    let mut project = document.project.expect("Project");
    let mut track_template = project.song.tracks[0].clone();
    track_template.channel.inserts.clear();
    track_template.channel.sends.clear();
    track_template.channel.output = crate::model::OutputTarget::Master;
    project.song.tracks.clear();
    for index in 0..crate::engine::schedule::MAX_RUNTIME_TRACKS {
        let mut track = track_template.clone();
        track.id = format!("runtime_track_{index}");
        for (clip_index, clip) in track.clips.iter_mut().enumerate() {
            clip.id = format!("runtime_clip_{index}_{clip_index}");
        }
        project.song.tracks.push(track);
    }
    project.song.groups.clear();
    project.song.returns.clear();
    project.song.sidechains.clear();
    project.song.envelope_followers.clear();
    project
        .song
        .master
        .as_mut()
        .expect("Master")
        .inserts
        .clear();
    let instrument = project
        .song
        .instruments
        .iter_mut()
        .find(|instrument| instrument.id == track_template.instrument)
        .expect("Instrument");
    let processor_template = instrument.post_processors[0].clone();
    instrument.post_processors = (0..17)
        .map(|index| {
            let mut processor = processor_template.clone();
            processor.id = format!("runtime_post_{index}");
            processor
        })
        .collect();
    let post_processor_count = instrument.post_processors.len();

    let estimate = crate::engine::mix_graph::RuntimeResourceEstimate::for_project(&project, 0)
        .expect("resource estimate");
    assert_eq!(
        estimate.processors,
        crate::engine::schedule::MAX_RUNTIME_TRACKS * post_processor_count
    );
    assert!(!estimate.admitted);
    assert!(
        validate_project(&project)
            .as_slice()
            .iter()
            .any(|diagnostic| {
                diagnostic.code == "CRV092"
                    && diagnostic.message.contains("fixed telemetry capacity")
            })
    );
}

#[test]
fn graph_voice_effect_state_survives_compiled_resource_recalculation() {
    let source = include_str!("../../tests/fixtures/minimal.crump")
            .replace("polyphony 4", "polyphony 64")
            .replace(
                "        node output output",
                "        node long_delay delay time 2s feedback 0 mix 0\n        node output output",
            )
            .replace(
                "        connect position.audio -> output.audio",
                "        connect position.audio -> long_delay.audio\n        connect long_delay.audio -> output.audio",
            )
            .replace(
                "    track drums \"Drums\" instrument drums level -7db pan 0.1 {",
                "    track lead-b \"Lead B\" instrument lead level -9db pan -0.1 {\n      clip melody-b pattern melody start 0bar duration 1bar mode once\n    }\n    track lead-c \"Lead C\" instrument lead level -9db pan -0.1 {\n      clip melody-c pattern melody start 0bar duration 1bar mode once\n    }\n    track lead-d \"Lead D\" instrument lead level -9db pan -0.1 {\n      clip melody-d pattern melody start 0bar duration 1bar mode once\n    }\n    track drums \"Drums\" instrument drums level -7db pan 0.1 {",
            );
    let document = parse_project(Path::new("graph-budget.crump"), &source);
    assert!(
        !document.diagnostics.has_errors(),
        "{:?}",
        document.diagnostics
    );
    let project = document.project.expect("Project");
    let estimate = crate::engine::mix_graph::RuntimeResourceEstimate::for_project(&project, 0)
        .expect("resource estimate");
    let delay_payload = 256_u64 * 2 * 48_000 * std::mem::size_of::<[f32; 2]>() as u64;
    assert!(estimate.graph_voice_state_bytes >= delay_payload);
    assert!(!estimate.admitted);
    let diagnostics = validate_project(&project);
    let diagnostic = diagnostics
        .as_slice()
        .iter()
        .find(|diagnostic| diagnostic.code == "CRV092")
        .cloned()
        .unwrap_or_else(|| panic!("graph voice state budget diagnostic: {diagnostics:?}"));
    assert!(diagnostic.message.contains("bytes at publication"));
}

#[test]
fn rejects_named_mix_runtime_budget_before_allocation() {
    let processors = (0..90)
        .map(|index| {
            format!("      processor delay_{index} delay {{\n        setting time 1s\n      }}\n")
        })
        .collect::<String>();
    let source = VALID.replace(
        "    track lead-track \"Lead\" instrument lead level -6db pan 0 {\n",
        &format!("    track lead-track \"Lead\" instrument lead level -6db pan 0 {{\n{processors}"),
    );
    let document = parse_project(Path::new("budget.crump"), &source);
    assert!(
        !document.diagnostics.has_errors(),
        "{:?}",
        document.diagnostics
    );
    let diagnostics = validate_project(&document.project.expect("Project"));
    let diagnostic = diagnostics
        .as_slice()
        .iter()
        .find(|diagnostic| diagnostic.code == "CRV092")
        .expect("named runtime budget diagnostic");
    assert!(diagnostic.message.contains("bytes at publication"));
    assert!(diagnostic.message.contains("budget"));
}

#[test]
fn rejects_runtime_track_bound_before_publication() {
    let document = parse_project(Path::new("project.crump"), VALID);
    let mut project = document.project.expect("Project");
    let template = project.song.tracks[0].clone();
    for index in 1..=32 {
        let mut track = template.clone();
        track.id = format!("lead-track-{index}");
        track.clips[0].id = format!("lead-clip-{index}");
        project.song.tracks.push(track);
    }
    let diagnostics = validate_project(&project);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV091")
    );
}

#[test]
fn rejects_more_than_bounded_events_at_one_frame() {
    let document = parse_project(Path::new("project.crump"), VALID);
    let mut project = document.project.expect("Project");
    let event = project.song.patterns[0].events[0].clone();
    project.song.patterns[0].events = vec![event; 257];
    let diagnostics = validate_project(&project);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV091")
    );
}

#[test]
fn follower_and_stutter_contracts_reject_ambiguous_bounds() {
    let source = include_str!("../../tests/fixtures/production_mix.crump");
    let bad_follower = source.replace("floor -60db ceiling -6db", "floor -6db ceiling -60db");
    let document = parse_project(Path::new("bad-follower.crump"), &bad_follower);
    assert!(
        !document.diagnostics.has_errors(),
        "{:?}",
        document.diagnostics
    );
    let diagnostics = validate_project(&document.project.expect("Follower Project"));
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV153" && diagnostic.message.contains("floor < ceiling")
    }));

    let bad_stutter = source.replace("setting interval 1beat", "setting interval 1/8beat");
    let document = parse_project(Path::new("bad-stutter.crump"), &bad_stutter);
    assert!(
        !document.diagnostics.has_errors(),
        "{:?}",
        document.diagnostics
    );
    let diagnostics = validate_project(&document.project.expect("Stutter Project"));
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV092" && diagnostic.message.contains("interval must be at least")
    }));
}
