use std::fs;

use super::*;

fn settle_background(workspace: &mut Workspace) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while workspace.background_work_pending() {
        workspace.poll_background_work();
        assert!(
            Instant::now() < deadline,
            "background workspace work timed out"
        );
        std::thread::yield_now();
    }
}

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
fn graph_inspection_order_follows_feedback_loops_through_delay_lines() {
    let bundle = crate::project::load_project(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/waveguides/plucked-string"
    ))
    .expect("waveguide example");
    let project = bundle.project.expect("Project");
    let instrument = &project.song.instruments[0];
    let graph = instrument.graph.as_ref().expect("graph");
    let nodes = graph_processing_node_indexes(instrument, graph);
    // The loop `excite -> string -> loss -> decay -> damp -> excite` declares `damp` after
    // `excite`, so declaration order cannot satisfy its same-frame Buses.
    assert_ne!(nodes, (0..graph.nodes.len()).collect::<Vec<_>>());
    let ranks = nodes
        .iter()
        .enumerate()
        .map(|(rank, index)| (graph.nodes[*index].id.as_str(), rank))
        .collect::<BTreeMap<_, _>>();
    for bus in &graph.connections {
        let into_delay_line = graph
            .nodes
            .iter()
            .any(|node| node.id == bus.to.node && node.kind.breaks_feedback());
        if bus.from.node != "note" && !into_delay_line {
            assert!(
                ranks[bus.from.node.as_str()] < ranks[bus.to.node.as_str()],
                "Bus `{}` runs backwards",
                bus.id
            );
        }
    }
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
    settle_background(&mut workspace);
    workspace.commit_input(
        "power(0.0000004)".to_owned(),
        InputAction::AutomationCurve {
            lane: "lead-level".to_owned(),
            point: "lift".to_owned(),
        },
    );
    settle_background(&mut workspace);

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
    settle_background(&mut workspace);
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
    assert!(
        workspace
            .audition_plan
            .transient_bypass
            .contains_key(&processor)
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("project.crump")).unwrap(),
        source_before
    );
    workspace.toggle_transient_processor_bypass();
    assert!(
        !workspace
            .audition_plan
            .transient_bypass
            .contains_key(&processor)
    );
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
    assert!(
        sections[0]
            .rows
            .iter()
            .any(|row| row.id == "oscillator-level")
    );
    assert!(
        sections
            .iter()
            .find(|section| section.kind == crate::model::PresentationSectionKind::Routing)
            .expect("routing section")
            .rows
            .iter()
            .any(|row| row.id == "carrier-fm")
    );
    assert!(
        sections
            .iter()
            .find(|section| section.kind == crate::model::PresentationSectionKind::Voicing)
            .expect("voicing section")
            .rows
            .iter()
            .any(|row| row.kind.is_none() && row.id == "policy")
    );
    assert!(
        sections
            .iter()
            .flat_map(|section| &section.rows)
            .all(|row| !row.source.is_empty() && !row.source_file.as_os_str().is_empty())
    );

    let parsed = crate::dsl::parse_project(
        std::path::Path::new("minimal.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    );
    let project = parsed.project.expect("legacy Project");
    let sections = instrument_section_views(&project, &project.song.instruments[0]);
    assert_eq!(sections.len(), 8);
    assert!(
        sections
            .iter()
            .flat_map(|section| &section.rows)
            .any(|row| row.id == "oscillator")
    );
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
    settle_background(&mut workspace);
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
    settle_background(&mut workspace);
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
    settle_background(&mut workspace);
    assert!(workspace.candidate_is_active(), "{}", workspace.status);
    assert_eq!(workspace.history.len(), 2);
    assert_eq!(fs::read_to_string(&project_path).unwrap(), changed);

    let reopened = Workspace::load(root.path()).expect("reopen workspace");
    assert!(reopened.candidate_is_active());
    assert_eq!(reopened.history.len(), 2);
    assert!(
        reopened.instrument_section_cache[0]
            .iter()
            .flat_map(|section| &section.rows)
            .any(|row| row.id == "post" && row.detail.contains("Amplifier"))
    );
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

#[test]
fn timeline_zoom_pan_and_fit_are_transient_keyboard_state() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let revision = workspace.bundle.revision.clone();
    let source = fs::read_to_string(root.path().join("project.crump")).expect("source");
    let song_end = workspace.bundle.project.as_ref().expect("Project").song.end;

    workspace.handle_key(KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE));
    assert_eq!(workspace.timeline_viewport.zoom_factor(), 2);
    let centered = workspace.timeline_viewport.window(song_end);
    workspace.handle_key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE));
    let panned = workspace.timeline_viewport.window(song_end);
    assert!(panned.start < centered.start);
    workspace.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    assert!(workspace.timeline_viewport.is_fit());
    assert_eq!(workspace.bundle.revision, revision);
    assert_eq!(
        fs::read_to_string(root.path().join("project.crump")).expect("unchanged source"),
        source
    );
}

#[test]
fn timeline_keys_seek_select_ranges_and_only_ctrl_c_exits() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    workspace.commit_input("Initial publication".to_owned(), InputAction::Publish);
    settle_background(&mut workspace);
    workspace.view = View::Timeline;

    workspace.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(workspace.selected_track, 1);
    workspace.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    assert_eq!(workspace.selected_track, 0);

    workspace.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
    assert_eq!(
        workspace.timeline_position().at,
        crate::time::MusicalTime::whole(1)
    );
    assert!(matches!(
        workspace.work_in_flight,
        Some(WorkspaceWorkKind::Active {
            explicit_seek: true,
            initial_frame: Some(24_000),
            ..
        })
    ));
    settle_background(&mut workspace);

    workspace.handle_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT));
    workspace.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
    workspace.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        workspace.audition_plan.range,
        AuditionRange {
            from: Some(ref start),
            to: Some(ref end),
            looping: false,
        } if start == "1qn" && end == "2qn"
    ));
    assert!(!workspace.timeline_state.visual());
    settle_background(&mut workspace);

    let one_shot = workspace.audition_plan.clone();
    workspace.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT));
    let mut expected_loop = one_shot;
    expected_loop.range.looping = true;
    assert_eq!(workspace.audition_plan, expected_loop);
    settle_background(&mut workspace);

    workspace.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
    assert_eq!(
        workspace.timeline_position().at,
        crate::time::MusicalTime::whole(2)
    );
    assert!(workspace.status.contains("outside the armed range"));
    assert_eq!(
        workspace.timeline_frame_at(crate::time::MusicalTime::whole(2)),
        Some(48_000)
    );
    assert!(workspace.timeline_cursor_frame_for_plan().is_err());
    workspace.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    assert!(!workspace.playback_requested);
    assert!(workspace.status.contains("outside the armed range"));

    workspace.handle_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT));
    assert!(workspace.timeline_state.visual());
    workspace.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!workspace.timeline_state.visual());
    workspace.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(!workspace.quit);
    workspace.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(workspace.quit);
}

#[test]
fn global_navigation_note_mode_and_timeline_context_jumps_are_modal() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");

    workspace.view = View::Mixer;
    workspace.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(workspace.selected_mixer_parameter, 1);
    workspace.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    assert_eq!(workspace.selected_mixer_parameter, 0);

    workspace.view = View::Help;
    workspace.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
    assert_eq!(
        usize::from(workspace.help_scroll),
        rendering::help_line_count().saturating_sub(1)
    );
    workspace.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    workspace.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    assert_eq!(workspace.help_scroll, 0);

    workspace.view = View::Instrument;
    let selected = (
        workspace.selected_instrument_section,
        workspace.selected_instrument_row,
    );
    workspace.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
    assert!(workspace.note_mode);
    workspace.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(
        (
            workspace.selected_instrument_section,
            workspace.selected_instrument_row,
        ),
        selected
    );
    workspace.handle_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::CONTROL));
    assert!(!workspace.note_mode);
    assert!(!workspace.quit);
    workspace.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_ne!(
        (
            workspace.selected_instrument_section,
            workspace.selected_instrument_row,
        ),
        selected
    );

    workspace.view = View::Timeline;
    workspace.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    workspace.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    assert_eq!(workspace.view, View::EventGrid);
    workspace.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    assert_eq!(workspace.view, View::Timeline);
}

#[test]
fn timeline_goto_search_marks_and_durable_locators_share_exact_positions() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    workspace.view = View::Timeline;

    workspace.commit_input("1:2".to_owned(), InputAction::TimelineGoto);
    workspace.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    assert_eq!(
        workspace.timeline_position().at,
        crate::time::MusicalTime::ZERO
    );
    workspace.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
    assert_eq!(
        workspace.timeline_position().at,
        crate::time::MusicalTime::whole(1)
    );

    workspace.commit_input("1:3".to_owned(), InputAction::TimelineGoto);
    assert_eq!(
        workspace.timeline_position().at,
        crate::time::MusicalTime::whole(2)
    );
    workspace.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
    workspace.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    workspace.commit_input("drums".to_owned(), InputAction::TimelineSearch);
    assert_eq!(workspace.timeline_position().track, 1);
    workspace.handle_key(KeyEvent::new(KeyCode::Char('\''), KeyModifiers::NONE));
    workspace.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    assert_eq!(
        workspace.timeline_position(),
        TimelinePosition {
            at: crate::time::MusicalTime::whole(2),
            track: 0,
        }
    );

    workspace.handle_key(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::SHIFT));
    assert!(matches!(
        workspace.modal.as_ref(),
        Some(Modal::Input {
            action: InputAction::TimelineLocator { at },
            ..
        }) if at == "2qn"
    ));
    workspace.modal = None;
    workspace.commit_input(
        "middle Middle".to_owned(),
        InputAction::TimelineLocator {
            at: "2qn".to_owned(),
        },
    );
    settle_background(&mut workspace);
    let source = fs::read_to_string(root.path().join("project.crump")).expect("edited source");
    assert!(
        source.contains("locator middle \"Middle\" at 2qn"),
        "{source}"
    );
}

#[test]
fn timeline_renders_arrangement_activity_at_full_and_narrow_sizes() {
    fn render_text(workspace: &Workspace, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, workspace))
            .expect("render Timeline");
        let buffer = terminal.backend().buffer();
        (0..height)
            .flat_map(|y| {
                (0..width)
                    .map(move |x| buffer[(x, y)].symbol().to_owned())
                    .chain(std::iter::once("\n".to_owned()))
            })
            .collect()
    }

    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    workspace.timeline_ascii = false;

    let full = render_text(&workspace, 120, 28);
    assert!(full.contains("Track · FIT"), "{full}");
    assert!(full.contains("Selected Track"), "{full}");
    assert!(full.contains("Lead"), "{full}");
    assert!(full.contains('▌'), "{full}");
    assert!(full.contains('▐'), "{full}");
    assert!(
        ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█']
            .into_iter()
            .any(|glyph| full.contains(glyph)),
        "{full}"
    );

    let narrow = render_text(&workspace, 40, 12);
    assert!(narrow.contains("Timeline"), "{narrow}");
    assert!(narrow.contains("Lead"), "{narrow}");
    assert!(
        ['▌', '▐', '━', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█']
            .into_iter()
            .any(|glyph| narrow.contains(glyph)),
        "{narrow}"
    );

    // Extremely constrained layouts must degrade without panicking.
    let _ = render_text(&workspace, 20, 8);
    let _ = render_text(&workspace, 8, 5);

    workspace.timeline_ascii = true;
    let ascii = render_text(&workspace, 40, 12);
    assert!(ascii.contains('['), "{ascii}");
    assert!(
        ![
            '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█', '▌', '▐', '━', '▓', '◆', '┊'
        ]
        .into_iter()
        .any(|glyph| ascii.contains(glyph)),
        "{ascii}"
    );
}

#[test]
fn timeline_labels_meter_denominator_beats() {
    let meter = crate::time::Meter::new(6, 8).expect("6/8 meter");
    assert_eq!(
        rendering::timeline_time_label(
            crate::time::MusicalTime::new(1, 2).expect("half quarter note"),
            meter,
        ),
        "1:2.0"
    );
    assert_eq!(
        rendering::timeline_time_label(crate::time::MusicalTime::whole(3), meter),
        "2:1.0"
    );
}

#[test]
fn background_startup_draws_loading_state_before_exact_reconstruction() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");

    let mut workspace = Workspace::begin_load(root.path()).expect("begin background load");
    assert!(workspace.bundle.project.is_none());
    assert!(workspace.background_work_pending());
    let backend = ratatui::backend::TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("loading terminal");
    terminal
        .draw(|frame| render(frame, &workspace))
        .expect("draw loading state");

    settle_background(&mut workspace);
    assert_eq!(
        workspace
            .bundle
            .project
            .as_ref()
            .expect("loaded Project")
            .name,
        "Minimal"
    );
}

#[test]
fn workspace_recovers_operation_latches_after_contained_worker_panics() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");

    workspace.mutation_in_flight = true;
    workspace.work_in_flight = Some(WorkspaceWorkKind::Mutation);
    workspace
        .worker
        .submit(|| -> WorkspaceWorkResult { panic!("injected mutation panic") })
        .expect("submit mutation panic");
    settle_background(&mut workspace);
    assert!(!workspace.mutation_in_flight);

    workspace.queue_mutation(
        ProductionViewRequest {
            expected_revision: Some(workspace.bundle.revision.clone()),
            operation: ProductionViewOperation::PublishCandidate,
            provenance: ProductionViewProvenance::human(
                "workspace-test",
                "Publish fixture",
                "verify mutation recovery",
            ),
        },
        "Published",
        "Publish failed",
    );
    assert!(workspace.mutation_in_flight || workspace.pending_mutation.is_some());
    settle_background(&mut workspace);
    assert!(!workspace.mutation_in_flight);
    assert!(workspace.active_bundle.is_some());

    let active_key = ActiveRequestKey {
        active_revision: workspace.active_revision.clone().expect("Active Revision"),
        audition_epoch: workspace.audition_epoch,
        playback_epoch: workspace.playback_epoch,
        generation: 1,
    };
    workspace.active_build_in_flight = true;
    workspace.work_in_flight = Some(WorkspaceWorkKind::Active {
        key: active_key,
        explicit_seek: false,
        initial_frame: None,
    });
    workspace
        .worker
        .submit(|| -> WorkspaceWorkResult { panic!("injected Active panic") })
        .expect("submit Active panic");
    settle_background(&mut workspace);
    assert!(!workspace.active_build_in_flight);
    assert!(!workspace.active_prepare_requested);

    let generation_before_cancel = workspace.next_publication_generation;
    let canceled_key = ActiveRequestKey {
        active_revision: workspace.active_revision.clone().expect("Active Revision"),
        audition_epoch: workspace.audition_epoch,
        playback_epoch: workspace.playback_epoch,
        generation: 2,
    };
    workspace.active_build_in_flight = true;
    workspace.work_in_flight = Some(WorkspaceWorkKind::Active {
        key: canceled_key,
        explicit_seek: false,
        initial_frame: None,
    });
    workspace
        .worker
        .submit(|| -> WorkspaceWorkResult { panic!("injected canceled Active panic") })
        .expect("submit canceled Active panic");
    workspace.playback_epoch = workspace.playback_epoch.saturating_add(1);
    workspace.active_prepare_requested = false;
    settle_background(&mut workspace);
    assert!(!workspace.active_build_in_flight);
    assert!(!workspace.active_prepare_requested);
    assert_eq!(
        workspace.next_publication_generation, generation_before_cancel,
        "a canceled panicking Active request must not dispatch a replacement preparation"
    );

    let candidate_reloads = workspace.performance.candidate_reloads;
    workspace.work_in_flight = Some(WorkspaceWorkKind::Reload {
        candidate: true,
        history: false,
    });
    workspace
        .worker
        .submit(|| -> WorkspaceWorkResult { panic!("injected reload panic") })
        .expect("submit reload panic");
    settle_background(&mut workspace);
    assert_eq!(
        workspace.performance.candidate_reloads,
        candidate_reloads + 1
    );
}

#[test]
fn stopped_stream_is_terminal_even_before_the_first_callback() {
    assert!(stream_failure_message(StreamStatus::Stopped).is_some());
    assert!(stream_failure_message(StreamStatus::Error).is_some());
    assert!(stream_failure_message(StreamStatus::Starting).is_none());
    assert!(stream_failure_message(StreamStatus::Connecting).is_none());
}

#[test]
fn reload_bursts_coalesce_while_navigation_and_draw_remain_available() {
    use std::sync::{Arc, Barrier, mpsc};

    let root = tempfile::tempdir().expect("temporary Project");
    let path = root.path().join("project.crump");
    let source = include_str!("../../tests/fixtures/minimal.crump");
    fs::write(&path, source).expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let candidate_reloads = workspace.performance.candidate_reloads;
    let gate = Arc::new(Barrier::new(2));
    let worker_gate = Arc::clone(&gate);
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    workspace
        .worker
        .submit(move || {
            started_tx.send(()).expect("signal blocked job");
            worker_gate.wait();
            WorkspaceWorkResult::Reload(ReloadBatch {
                candidate: None,
                history: None,
            })
        })
        .expect("submit blocked job");
    started_rx.recv().expect("blocked job started");

    fs::write(
        &path,
        source.replace("project \"Minimal\"", "project \"Burst B\""),
    )
    .expect("write intermediate Candidate");
    workspace.refresh_if_changed();
    workspace.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(workspace.view, View::Mixer);
    let backend = ratatui::backend::TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("responsive terminal");
    terminal
        .draw(|frame| render(frame, &workspace))
        .expect("draw while reload is blocked");

    fs::write(
        &path,
        source.replace("project \"Minimal\"", "project \"Burst C\""),
    )
    .expect("write newest Candidate");
    workspace.refresh_if_changed();
    gate.wait();
    settle_background(&mut workspace);

    assert_eq!(
        workspace.bundle.project.as_ref().expect("Project").name,
        "Burst C"
    );
    assert_eq!(
        workspace.performance.candidate_reloads,
        candidate_reloads + 1,
        "the superseded intermediate Candidate must never be reconstructed"
    );
}

#[test]
fn result_changed_before_ui_acceptance_never_replaces_stable_candidate() {
    let root = tempfile::tempdir().expect("temporary Project");
    let path = root.path().join("project.crump");
    let source = include_str!("../../tests/fixtures/minimal.crump");
    fs::write(&path, source).expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    fs::write(
        &path,
        source.replace("project \"Minimal\"", "project \"First\""),
    )
    .expect("write first Candidate");
    let before = WorkspaceWatch::capture_candidate(&workspace.root, &workspace.bundle);
    let result = reload::load_candidate(
        workspace.root.clone(),
        workspace.bundle.revision.clone(),
        before,
    );
    fs::write(
        &path,
        source.replace("project \"Minimal\"", "project \"Newest\""),
    )
    .expect("supersede completed Candidate");

    workspace.finish_candidate_reload(result);
    assert_eq!(
        workspace
            .bundle
            .project
            .as_ref()
            .expect("stable Project")
            .name,
        "Minimal",
        "a result invalidated before acceptance must not become visible"
    );
    settle_background(&mut workspace);
    assert_eq!(
        workspace
            .bundle
            .project
            .as_ref()
            .expect("newest Project")
            .name,
        "Newest"
    );
}

#[test]
fn unchanged_workspace_poll_skips_exact_reload_and_derived_rebuild() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let candidate_reloads = workspace.performance.candidate_reloads;
    let history_reloads = workspace.performance.history_reloads;
    let derived_rebuilds = workspace.performance.derived_rebuilds;

    workspace.refresh_if_changed();
    settle_background(&mut workspace);

    assert_eq!(workspace.performance.candidate_reloads, candidate_reloads);
    assert_eq!(workspace.performance.history_reloads, history_reloads);
    assert_eq!(workspace.performance.derived_rebuilds, derived_rebuilds);
    assert_eq!(workspace.performance.refresh_checks, 1);
}

#[test]
fn candidate_stamp_change_triggers_one_exact_reload_and_rebuild() {
    let root = tempfile::tempdir().expect("temporary Project");
    let project_path = root.path().join("project.crump");
    let source = include_str!("../../tests/fixtures/minimal.crump");
    fs::write(&project_path, source).expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let candidate_reloads = workspace.performance.candidate_reloads;
    let history_reloads = workspace.performance.history_reloads;
    let derived_rebuilds = workspace.performance.derived_rebuilds;
    fs::write(
        &project_path,
        source.replace("project \"Minimal\"", "project \"Changed\""),
    )
    .expect("replace Candidate source");

    workspace.refresh_if_changed();
    settle_background(&mut workspace);

    assert_eq!(
        workspace.performance.candidate_reloads,
        candidate_reloads + 1
    );
    assert_eq!(workspace.performance.history_reloads, history_reloads);
    assert_eq!(workspace.performance.derived_rebuilds, derived_rebuilds + 1);
    assert_eq!(
        workspace.bundle.project.as_ref().expect("Project").name,
        "Changed"
    );
}

#[test]
fn history_stamp_change_does_not_reload_unchanged_candidate() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let candidate_reloads = workspace.performance.candidate_reloads;
    let history_reloads = workspace.performance.history_reloads;
    let derived_rebuilds = workspace.performance.derived_rebuilds;
    let revision = workspace.bundle.revision.clone();
    let store = HistoryStore::open(root.path()).expect("History");
    store
        .publish(
            &workspace.bundle,
            crate::history::Publication::new(
                crate::history::ChangeOrigin::system("workspace-test"),
                "Publish fixture",
                "exercise history invalidation",
            ),
        )
        .expect("publish fixture");
    drop(store);

    workspace.refresh_if_changed();
    settle_background(&mut workspace);

    assert_eq!(workspace.performance.candidate_reloads, candidate_reloads);
    assert_eq!(workspace.performance.history_reloads, history_reloads + 1);
    assert_eq!(workspace.performance.derived_rebuilds, derived_rebuilds);
    assert_eq!(workspace.active_revision.as_ref(), Some(&revision));
}

#[test]
#[ignore = "manual release-mode workspace performance profile"]
fn profile_workspace_hot_paths() {
    const DRAW_ITERATIONS: u32 = 50;
    const TERMINAL_WIDTH: u16 = 140;
    const TERMINAL_HEIGHT: u16 = 32;
    let project = std::env::var_os("CRUMP_WORKSPACE_PROFILE_PROJECT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/fixtures/minimal.crump"));

    let started = Instant::now();
    let mut workspace = Workspace::load(&project).expect("load profiled workspace");
    eprintln!(
        "workspace-profile load_ms={:.3} project={} terminal={}x{}",
        started.elapsed().as_secs_f64() * 1_000.0,
        project.display(),
        TERMINAL_WIDTH,
        TERMINAL_HEIGHT,
    );

    for sample in 0..5 {
        let started = Instant::now();
        let changed = workspace.refresh_if_changed();
        let queued = started.elapsed();
        settle_background(&mut workspace);
        eprintln!(
            "workspace-profile unchanged-poll sample={sample} changed={changed} queue_us={:.3} completion_us={:.3}",
            queued.as_secs_f64() * 1_000_000.0,
            started.elapsed().as_secs_f64() * 1_000_000.0
        );
    }
    let started = Instant::now();
    workspace.refresh();
    let queued = started.elapsed();
    settle_background(&mut workspace);
    eprintln!(
        "workspace-profile forced-refresh queue_us={:.3} completion_ms={:.3}",
        queued.as_secs_f64() * 1_000_000.0,
        started.elapsed().as_secs_f64() * 1_000.0
    );

    for view in VIEWS {
        workspace.view = view;
        workspace.clamp_selections();
        let backend = ratatui::backend::TestBackend::new(TERMINAL_WIDTH, TERMINAL_HEIGHT);
        let mut terminal = Terminal::new(backend).expect("profile terminal");
        terminal
            .draw(|frame| render(frame, &workspace))
            .expect("warm profile draw");
        let started = Instant::now();
        let allocations = allocation_counter::measure(|| {
            for _ in 0..DRAW_ITERATIONS {
                terminal
                    .draw(|frame| render(frame, std::hint::black_box(&workspace)))
                    .expect("profile draw");
            }
        });
        let elapsed = started.elapsed();
        eprintln!(
            "workspace-profile draw view={} iterations={DRAW_ITERATIONS} total_ms={:.3} mean_us={:.3} allocations={} allocated_bytes={} allocations_per_draw={:.1} bytes_per_draw={:.1}",
            view.title(),
            elapsed.as_secs_f64() * 1_000.0,
            elapsed.as_secs_f64() * 1_000_000.0 / f64::from(DRAW_ITERATIONS),
            allocations.count_total,
            allocations.bytes_total,
            allocations.count_total as f64 / f64::from(DRAW_ITERATIONS),
            allocations.bytes_total as f64 / f64::from(DRAW_ITERATIONS),
        );
    }
}
