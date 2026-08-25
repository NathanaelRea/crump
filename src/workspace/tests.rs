use std::fs;

use super::*;

#[test]
fn generated_sample_rows_show_exact_wave_regions_and_multisample_mapping() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/synth-alpha");
    let path = root.join("minimal-multisample.crump");
    let source = fs::read_to_string(&path).expect("multisample fixture");
    let parsed = crate::dsl::parse_project(&path, &source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let (assets, diagnostics) = crate::media::load_asset_inventory(&root, &project.assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");

    let sections =
        instrument_section_views_with_assets(&project, &project.song.instruments[0], &assets);
    for asset_id in ["zone-low", "zone-high"] {
        let row = sections
            .iter()
            .flat_map(|section| &section.rows)
            .find(|row| row.id == asset_id)
            .expect("sample asset row");
        assert!(row.detail.contains("1ch"), "{}", row.detail);
        assert!(row.detail.contains("48000Hz"), "{}", row.detail);
        assert!(row.detail.contains("24000 frames"), "{}", row.detail);
        assert!(row.detail.contains("wave "), "{}", row.detail);
    }
    let source = sections
        .iter()
        .flat_map(|section| &section.rows)
        .find(|row| row.id == "source")
        .expect("multisample node row");
    assert!(source.detail.contains("zones ["), "{}", source.detail);
    assert!(source.detail.contains("C1..B3"), "{}", source.detail);
    assert!(source.detail.contains("rr low-rr/0"), "{}", source.detail);
}

#[test]
fn generated_spectral_rows_show_analysis_contract_and_source_waveform() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/synth-alpha");
    let path = root.join("minimal-spectral.crump");
    let source = fs::read_to_string(&path).expect("spectral fixture");
    let parsed = crate::dsl::parse_project(&path, &source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let (assets, diagnostics) = crate::media::load_asset_inventory(&root, &project.assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let sections =
        instrument_section_views_with_assets(&project, &project.song.instruments[0], &assets);
    let source = sections
        .iter()
        .flat_map(|section| &section.rows)
        .find(|row| row.id == "source")
        .expect("spectral node row");
    assert!(source.detail.contains("Spectral"), "{}", source.detail);
    assert!(
        source.detail.contains("crump-spectral-peaks-v1"),
        "{}",
        source.detail
    );
    assert!(source.detail.contains("max_peaks"), "{}", source.detail);
    let asset = sections
        .iter()
        .flat_map(|section| &section.rows)
        .find(|row| row.id == "source-texture")
        .expect("spectral asset row");
    assert!(asset.detail.contains("1ch"), "{}", asset.detail);
    assert!(asset.detail.contains("48000 frames"), "{}", asset.detail);
    assert!(asset.detail.contains("wave "), "{}", asset.detail);
}

#[test]
fn generated_source_assets_remain_visible_when_presentation_omits_them() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/synth-alpha");
    for (fixture, expected_asset_ids) in [
        ("minimal-sample-source.crump", &["source-texture"][..]),
        ("minimal-multisample.crump", &["zone-high", "zone-low"][..]),
        ("minimal-granular.crump", &["source-texture"][..]),
        ("minimal-spectral.crump", &["source-texture"][..]),
    ] {
        let path = root.join(fixture);
        let source = fs::read_to_string(&path).expect("source fixture");
        let parsed = crate::dsl::parse_project(&path, &source);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        let mut project = parsed.project.expect("Project");
        for section in &mut project.song.instruments[0].presentation.sections {
            section
                .items
                .retain(|item| item.kind != PresentationItemKind::Asset);
        }

        let sections = instrument_section_views(&project, &project.song.instruments[0]);
        let mut asset_ids = sections
            .iter()
            .flat_map(|section| &section.rows)
            .filter(|row| row.kind == Some(PresentationItemKind::Asset))
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>();
        asset_ids.sort_unstable();
        assert_eq!(asset_ids, expected_asset_ids, "{fixture}");
    }
}

#[test]
fn graph_inspection_order_matches_effective_topology_and_bus_convergence() {
    let parsed = crate::dsl::parse_project(
        std::path::Path::new("stereo_foundation.crump"),
        include_str!("../../tests/fixtures/stereo_foundation.crump"),
    );
    let project = parsed.project.expect("Project");
    let instrument = &project.song.instruments[0];
    let graph = instrument.graph.as_ref().unwrap();
    let nodes = graph_processing_node_indexes(instrument, graph);
    let ranks = nodes
        .iter()
        .enumerate()
        .map(|(rank, index)| (graph.nodes[*index].id.as_str(), rank))
        .collect::<BTreeMap<_, _>>();
    for bus in &graph.connections {
        if bus.from.node != "note" {
            assert!(ranks[bus.from.node.as_str()] < ranks[bus.to.node.as_str()]);
        }
    }
    let buses = graph_processing_bus_indexes(graph, &nodes);
    assert_eq!(
        buses
            .iter()
            .map(|index| &graph.connections[*index])
            .filter(|bus| bus.to.node == "sum")
            .map(|bus| bus.id.as_str())
            .collect::<Vec<_>>(),
        ["a-large", "m-small", "z-cancel"]
    );
}

#[test]
fn graph_inspection_order_includes_same_frame_modulation_dependencies() {
    let parsed = crate::dsl::parse_project(
        std::path::Path::new("instrument_modulation.crump"),
        include_str!("../../tests/fixtures/instrument_modulation.crump"),
    );
    let project = parsed.project.expect("Project");
    let instrument = &project.song.instruments[0];
    let graph = instrument.graph.as_ref().expect("graph");
    let nodes = graph_processing_node_indexes(instrument, graph);
    let ranks = nodes
        .iter()
        .enumerate()
        .map(|(rank, index)| (graph.nodes[*index].id.as_str(), rank))
        .collect::<BTreeMap<_, _>>();
    assert!(ranks["envelope"] < ranks["oscillator"]);
    assert!(ranks["graph-lfo"] < ranks["oscillator"]);
}

#[test]
fn exact_automation_entry_preserves_value_and_curve_precision() {
    let root = tempfile::tempdir().expect("temporary Project");
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/minimal.crump"
    ))
    .expect("read fixture")
    .replace(
        "    track lead \"Lead\" instrument lead level -9db pan -0.1 {",
        r#"    automation lead-level target track/lead/level {
      point start 0bar -18db linear
      point lift 2beat -6db step
    }
    track lead "Lead" instrument lead level -9db pan -0.1 {"#,
    );
    fs::write(root.path().join("project.crump"), source).expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");

    workspace.commit_input(
        "-6.123456789db".to_owned(),
        InputAction::AutomationValue {
            lane: "lead-level".to_owned(),
            point: "lift".to_owned(),
        },
    );
    workspace.commit_input(
        "power(0.0000004)".to_owned(),
        InputAction::AutomationCurve {
            lane: "lead-level".to_owned(),
            point: "lift".to_owned(),
        },
    );

    let source = fs::read_to_string(root.path().join("project.crump"))
        .expect("read exact Automation source");
    assert!(source.contains("point lift 2beat -6.123456789db power(0.0000004)"));
}

#[test]
fn mixer_exposes_source_backed_controls_for_every_routing_scope() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("project.crump"),
        include_str!("../../tests/fixtures/production_mix.crump"),
    )
    .unwrap();
    let mut workspace = Workspace::load(directory.path()).unwrap();
    workspace.view = View::Mixer;
    let parameters = workspace
        .bundle
        .project
        .as_ref()
        .expect("Project")
        .mix_parameter_specs();
    for expected in [
        "track/bass/left_pan",
        "track/drums/mute",
        "group/mix_bus/level",
        "group/mix_bus/mute",
        "return/room/level",
        "return/room/mute",
        "send/drum_room/level",
        "processor/bass_comp/threshold",
        "master/level",
    ] {
        assert!(
            parameters
                .iter()
                .any(|(target, _)| target.to_string() == expected),
            "{expected}"
        );
    }
    let project = workspace.bundle.project.as_ref().expect("Project");
    assert_eq!(
        mixer_channel_identity(
            project,
            &"send/drum_room/level".parse().expect("Send target")
        )
        .as_deref(),
        Some("track/drums")
    );
    assert_eq!(
        mixer_channel_identity(
            project,
            &"processor/bass_comp/threshold"
                .parse()
                .expect("processor target")
        )
        .as_deref(),
        Some("track/bass")
    );
    assert_eq!(
        mixer_mute_target("return/room")
            .expect("mute target")
            .to_string(),
        "return/room/mute"
    );

    workspace.selected_mixer_parameter = parameters
        .iter()
        .position(|(target, _)| *target == ParameterTarget::Master)
        .expect("Master target");
    workspace.open_exact_edit();
    assert!(matches!(
        workspace.modal,
        Some(Modal::Input {
            action: InputAction::Target {
                target: ParameterTarget::Master,
                ..
            },
            ..
        })
    ));
}

#[test]
fn audition_target_text_round_trips_every_workspace_target() {
    let targets = [
        AuditionTarget::Song,
        AuditionTarget::Track {
            track: "drums".to_owned(),
        },
        AuditionTarget::Clip {
            clip: "verse".to_owned(),
            track: None,
        },
        AuditionTarget::Clip {
            clip: "verse".to_owned(),
            track: Some("drums".to_owned()),
        },
        AuditionTarget::Pattern {
            pattern: "beat".to_owned(),
            track: Some("drums".to_owned()),
        },
        AuditionTarget::Asset {
            asset: "snare".to_owned(),
            start_frame: Some(12),
            end_frame: Some(240),
        },
        AuditionTarget::Mapping {
            instrument: "kit".to_owned(),
            mapping: "snare".to_owned(),
            track: Some("drums".to_owned()),
        },
        AuditionTarget::Occurrence {
            pattern: "beat".to_owned(),
            occurrence: "snare-2".to_owned(),
            track: None,
        },
    ];
    for target in targets {
        let spec = audition_target_spec(&target);
        assert_eq!(parse_audition_target(&spec), Ok(target), "{spec}");
    }
    assert!(parse_audition_target("asset/snare/20/10").is_err());
}

#[test]
fn processing_view_exposes_source_settings_and_transient_bypass_without_editing() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("project.crump"),
        include_str!("../../tests/fixtures/production_mix.crump"),
    )
    .unwrap();
    let mut workspace = Workspace::load(directory.path()).unwrap();
    workspace.view = View::Processing;
    let source_before = fs::read_to_string(directory.path().join("project.crump")).unwrap();
    let processor = workspace
        .selected_processor_inspection()
        .expect("compiled processor")
        .id
        .clone();

    workspace.toggle_transient_processor_bypass();
    assert!(workspace.audition_plan.transient_bypass.is_empty());
    assert!(workspace.status.contains("Publish the Candidate"));
    workspace.commit_input("Initial publication".to_owned(), InputAction::Publish);
    assert!(workspace.candidate_is_active());

    workspace.open_exact_edit();
    assert!(matches!(
        workspace.modal,
        Some(Modal::Input {
            action: InputAction::ProcessorSetting { .. },
            ..
        })
    ));
    workspace.modal = None;
    workspace.toggle_transient_processor_bypass();
    assert!(workspace
        .audition_plan
        .transient_bypass
        .contains_key(&processor));
    assert_eq!(
        fs::read_to_string(directory.path().join("project.crump")).unwrap(),
        source_before
    );
    workspace.toggle_transient_processor_bypass();
    assert!(!workspace
        .audition_plan
        .transient_bypass
        .contains_key(&processor));
}

#[test]
fn every_graph_item_class_is_selectable_and_source_navigable() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/minimal.crump"),
        root.path().join("project.crump"),
    )
    .expect("copy fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    workspace.view = View::Graph;

    let instrument = &workspace
        .bundle
        .project
        .as_ref()
        .expect("parsed Project")
        .song
        .instruments[0];
    let graph = instrument.graph.as_ref().expect("graph Instrument");
    let node_count = graph.nodes.len();
    let connection_count = graph.connections.len();
    let parameter_count = instrument.parameters.len();
    let faceplate_count = instrument.faceplate.len();
    assert_eq!(
        workspace.graph_node_count(),
        node_count + connection_count + parameter_count + faceplate_count
    );

    for (index, expected) in [
        (0, "Graph Node"),
        (node_count, "Bus"),
        (node_count + connection_count, "Parameter"),
        (
            node_count + connection_count + parameter_count,
            "Faceplate mapping",
        ),
    ] {
        workspace.view = View::Graph;
        workspace.selected_graph_node = index;
        workspace.open_graph_source();
        assert_eq!(workspace.view, View::Source);
        assert!(workspace.status.contains(expected), "{}", workspace.status);
        assert_eq!(workspace.selected_source, 0);
    }
}

#[test]
fn generated_instrument_sections_preserve_metadata_and_show_unassigned_topology() {
    let parsed = crate::dsl::parse_project(
        std::path::Path::new("virtual_analog_foundation.crump"),
        include_str!("../../tests/fixtures/virtual_analog_foundation.crump"),
    );
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let diagnostics = crate::validate::validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let instrument = &project.song.instruments[0];
    let sections = instrument_section_views(&project, instrument);
    assert_eq!(sections.len(), 8);
    assert_eq!(sections[0].id, "source-panel");
    assert_eq!(sections[0].name, "Sources");
    let carrier = instrument
        .graph
        .as_ref()
        .expect("voice graph")
        .nodes
        .iter()
        .find(|node| node.id == "carrier")
        .expect("carrier Node");
    let carrier_row = sections[0]
        .rows
        .iter()
        .find(|row| row.id == "carrier")
        .expect("authored carrier row");
    assert_eq!(carrier_row.source_file, carrier.location.file);
    assert_eq!(carrier_row.source_offset, carrier.location.start);
    assert!(sections[0]
        .rows
        .iter()
        .any(|row| row.id == "oscillator-level"));
    assert!(sections
        .iter()
        .find(|section| section.kind == crate::model::PresentationSectionKind::Routing)
        .expect("routing section")
        .rows
        .iter()
        .any(|row| row.id == "carrier-fm"));
    assert!(sections
        .iter()
        .find(|section| section.kind == crate::model::PresentationSectionKind::Voicing)
        .expect("voicing section")
        .rows
        .iter()
        .any(|row| row.kind.is_none() && row.id == "policy"));
    assert!(sections
        .iter()
        .flat_map(|section| &section.rows)
        .all(|row| !row.source.is_empty() && !row.source_file.as_os_str().is_empty()));

    let parsed = crate::dsl::parse_project(
        std::path::Path::new("minimal.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    );
    let project = parsed.project.expect("legacy Project");
    let sections = instrument_section_views(&project, &project.song.instruments[0]);
    assert_eq!(sections.len(), 8);
    assert!(sections
        .iter()
        .flat_map(|section| &section.rows)
        .any(|row| row.id == "oscillator"));
}

#[test]
fn generated_wavetable_rows_include_cached_exact_asset_wave_inspection() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/synth-alpha");
    let path = root.join("wavetable-demo.crump");
    let source = fs::read_to_string(&path).expect("wavetable fixture");
    let parsed = crate::dsl::parse_project(&path, &source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let mut project = parsed.project.expect("Project");
    let (assets, diagnostics) = crate::media::load_asset_inventory(&root, &project.assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");

    let sections =
        instrument_section_views_with_assets(&project, &project.song.instruments[0], &assets);
    let row = sections
        .iter()
        .flat_map(|section| &section.rows)
        .find(|row| row.id == "wt-basic")
        .expect("authored wavetable asset row");
    assert!(
        row.detail.contains("4 frames × 2048 samples"),
        "{}",
        row.detail
    );
    assert!(row.detail.contains("GlobalPeak"), "{}", row.detail);
    assert!(row.detail.contains("CatmullRom"), "{}", row.detail);
    assert!(row.detail.contains("wave "), "{}", row.detail);
    assert!(row.detail.contains('│'), "{}", row.detail);
    assert_eq!(row.source_offset, project.assets[0].location.start);

    project.song.instruments[0].presentation.sections[0]
        .items
        .retain(|item| item.id != "wt-basic");
    let sections =
        instrument_section_views_with_assets(&project, &project.song.instruments[0], &assets);
    assert_eq!(
        sections
            .iter()
            .flat_map(|section| &section.rows)
            .filter(|row| row.id == "wt-basic")
            .count(),
        1,
        "a referenced unassigned table remains visible in generated topology"
    );
}

#[test]
fn source_topology_edit_publishes_and_returns_to_generated_views_without_flattening() {
    let root = tempfile::tempdir().expect("temporary Project");
    let original = include_str!("../../tests/fixtures/minimal.crump");
    let project_path = root.path().join("project.crump");
    fs::write(&project_path, original).expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    workspace.commit_input("Initial topology".to_owned(), InputAction::Publish);
    assert!(workspace.candidate_is_active(), "{}", workspace.status);
    assert_eq!(workspace.history.len(), 1);

    let changed = original
        .replace(
            "        node output output",
            "        node post amplifier level 1\n        node output output",
        )
        .replace(
            "        connect amplifier.audio -> position.audio",
            "        connect amplifier.audio -> post.audio\n        connect envelope.control -> post.gain\n        connect post.audio -> position.audio",
        );
    assert_ne!(changed, original);
    fs::write(&project_path, &changed).expect("write direct topology edit");
    workspace.refresh();
    assert!(
        !workspace.bundle.diagnostics.has_errors(),
        "{:?}",
        workspace.bundle.diagnostics
    );
    assert!(!workspace.candidate_is_active());
    assert!(
        workspace.instrument_section_cache[0]
            .iter()
            .flat_map(|section| &section.rows)
            .any(|row| row.id == "post" && row.detail.contains("Amplifier")),
        "custom topology is absent from generated Instrument sections"
    );

    workspace.commit_input(
        "Insert explicit post amplifier".to_owned(),
        InputAction::Publish,
    );
    assert!(workspace.candidate_is_active(), "{}", workspace.status);
    assert_eq!(workspace.history.len(), 2);
    assert_eq!(fs::read_to_string(&project_path).unwrap(), changed);

    let reopened = Workspace::load(root.path()).expect("reopen workspace");
    assert!(reopened.candidate_is_active());
    assert_eq!(reopened.history.len(), 2);
    assert!(reopened.instrument_section_cache[0]
        .iter()
        .flat_map(|section| &section.rows)
        .any(|row| row.id == "post" && row.detail.contains("Amplifier")));
    assert_eq!(fs::read_to_string(project_path).unwrap(), changed);
}

#[test]
fn shared_graph_nodes_and_buses_are_selectable_and_source_navigable() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/shared_graph_foundation.crump"
        ),
        root.path().join("project.crump"),
    )
    .expect("copy fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let instrument = &workspace
        .bundle
        .project
        .as_ref()
        .expect("parsed Project")
        .song
        .instruments[0];
    let voice = instrument.graph.as_ref().expect("voice graph");
    let shared = instrument.shared_graph.as_ref().expect("shared graph");
    let shared_offset = voice.nodes.len() + voice.connections.len();
    assert_eq!(
        workspace.graph_node_count(),
        shared_offset
            + shared.nodes.len()
            + shared.connections.len()
            + instrument.parameters.len()
            + instrument.faceplate.len()
    );

    for (index, expected) in [
        (shared_offset, "Shared Graph Node"),
        (shared_offset + shared.nodes.len(), "Shared Bus"),
    ] {
        workspace.view = View::Graph;
        workspace.selected_graph_node = index;
        workspace.open_graph_source();
        assert_eq!(workspace.view, View::Source);
        assert!(workspace.status.contains(expected), "{}", workspace.status);
        assert_eq!(workspace.selected_source, 0);
    }
}
