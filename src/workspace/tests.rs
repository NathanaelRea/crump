use std::fs;

use super::*;

pub(super) fn settle_background(workspace: &mut Workspace) {
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
fn timeline_labels_meter_denominator_beats() {
    let meter = crate::time::Meter::new(6, 8).expect("6/8 meter");
    assert_eq!(
        timeline_time_label(
            crate::time::MusicalTime::new(1, 2).expect("half quarter note"),
            meter,
        ),
        "1:2.0"
    );
    assert_eq!(
        timeline_time_label(crate::time::MusicalTime::whole(3), meter),
        "2:1.0"
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
fn an_overloaded_callback_ends_playback_even_while_the_stream_runs() {
    let streaming = PlayerTelemetry {
        stream_state: StreamStatus::Streaming,
        ..PlayerTelemetry::default()
    };
    assert!(playback_failure_message(&streaming).is_none());
    let overloaded = PlayerTelemetry {
        overloaded: true,
        ..streaming
    };
    assert_eq!(
        playback_failure_message(&overloaded),
        Some(OVERLOAD_MESSAGE)
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
fn play_on_an_unpublished_candidate_publishes_then_starts() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    workspace.playback_backend = PlaybackBackend::InMemory;
    settle_background(&mut workspace);
    assert!(workspace.active_revision.is_none());

    workspace.toggle_playback();
    assert!(
        workspace
            .status
            .starts_with("Publishing Candidate before playback")
    );
    settle_background(&mut workspace);
    assert!(workspace.candidate_is_active(), "{}", workspace.status);
    assert_eq!(workspace.history[0].summary, "Publish on play");
    assert!(workspace.player.is_some(), "{}", workspace.status);
}

#[test]
fn stopping_before_publication_lands_cancels_the_deferred_play() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    workspace.playback_backend = PlaybackBackend::InMemory;
    settle_background(&mut workspace);

    workspace.toggle_playback();
    workspace.stop_playback();
    settle_background(&mut workspace);
    assert!(workspace.candidate_is_active(), "{}", workspace.status);
    assert!(workspace.player.is_none());
}

#[test]
fn resuming_a_paused_audition_publishes_an_external_edit_first() {
    let root = tempfile::tempdir().expect("temporary Project");
    let original = include_str!("../../tests/fixtures/minimal.crump");
    let project_path = root.path().join("project.crump");
    fs::write(&project_path, original).expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    workspace.playback_backend = PlaybackBackend::InMemory;
    workspace.toggle_playback();
    settle_background(&mut workspace);
    workspace.toggle_playback();
    assert!(workspace.playback_paused, "{}", workspace.status);

    fs::write(
        &project_path,
        original.replace(
            "track lead \"Lead\" instrument lead level -9db",
            "track lead \"Lead\" instrument lead level -12db",
        ),
    )
    .expect("write level edit");
    workspace.refresh();
    settle_background(&mut workspace);
    assert!(!workspace.candidate_is_active());

    workspace.toggle_playback();
    assert!(workspace.playback_paused, "stays paused until published");
    settle_background(&mut workspace);
    assert!(workspace.candidate_is_active(), "{}", workspace.status);
    assert!(!workspace.playback_paused, "{}", workspace.status);
}

#[test]
fn playing_the_active_revision_leaves_an_unpublished_candidate_alone() {
    let root = tempfile::tempdir().expect("temporary Project");
    let original = include_str!("../../tests/fixtures/minimal.crump");
    let project_path = root.path().join("project.crump");
    fs::write(&project_path, original).expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    workspace.playback_backend = PlaybackBackend::InMemory;
    workspace.commit_input("Initial".to_owned(), InputAction::Publish);
    settle_background(&mut workspace);
    fs::write(
        &project_path,
        original.replace(
            "track lead \"Lead\" instrument lead level -9db",
            "track lead \"Lead\" instrument lead level -12db",
        ),
    )
    .expect("write level edit");
    workspace.refresh();
    settle_background(&mut workspace);
    let history = workspace.history.len();

    workspace.play_active_revision();
    settle_background(&mut workspace);
    assert!(workspace.player.is_some(), "{}", workspace.status);
    assert!(!workspace.candidate_is_active());
    assert_eq!(workspace.history.len(), history);
}

#[test]
fn in_memory_playback_publishes_edits_through_the_callback_and_promotes_inspection() {
    let root = tempfile::tempdir().expect("temporary Project");
    let original = include_str!("../../tests/fixtures/minimal.crump");
    let project_path = root.path().join("project.crump");
    fs::write(&project_path, original).expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    workspace.playback_backend = PlaybackBackend::InMemory;
    workspace.commit_input("Initial".to_owned(), InputAction::Publish);
    settle_background(&mut workspace);
    assert!(workspace.candidate_is_active(), "{}", workspace.status);

    workspace.toggle_playback();
    settle_background(&mut workspace);
    let first = workspace
        .pending_active_inspection
        .as_ref()
        .expect("started audition awaits callback confirmation")
        .key
        .generation;
    let player = workspace.player.as_mut().expect("in-memory player started");
    let audio = player.render(512).expect("render the in-memory callback");
    assert!(audio.iter().flatten().all(|sample| sample.is_finite()));
    workspace.poll_audio();
    assert!(workspace.pending_active_inspection.is_none());
    assert_eq!(workspace.telemetry.publication_generation, first);
    assert_eq!(workspace.status, "Active audition is now audible");
    let before = workspace.telemetry.engine.frame;
    assert!(before >= 511, "transport advanced to {before}");

    // A published edit replaces the audible runtime through the same callback.
    let changed = original.replace(
        "track lead \"Lead\" instrument lead level -9db",
        "track lead \"Lead\" instrument lead level -12db",
    );
    assert_ne!(changed, original);
    fs::write(&project_path, &changed).expect("write level edit");
    workspace.refresh();
    settle_background(&mut workspace);
    workspace.commit_input("Quieter lead".to_owned(), InputAction::Publish);
    settle_background(&mut workspace);
    let second = workspace
        .pending_active_inspection
        .as_ref()
        .expect("replacement awaits callback confirmation")
        .key
        .generation;
    assert!(second > first);
    assert!(
        workspace.status.starts_with("Active audition queued"),
        "{}",
        workspace.status
    );

    // The callback crossfades, retires the old runtime, and reports the new generation.
    let player = workspace
        .player
        .as_mut()
        .expect("player survives publication");
    player.render(2_048).expect("render through the crossfade");
    workspace.poll_audio();
    assert_eq!(workspace.telemetry.publication_generation, second);
    assert!(!workspace.telemetry.transition_active);
    assert!(workspace.pending_active_inspection.is_none());
    assert_eq!(workspace.status, "Active audition is now audible");
    assert!(
        workspace.telemetry.engine.frame > before,
        "callback-relative transport is retained across publication"
    );

    workspace.stop_playback();
    settle_background(&mut workspace);
    assert!(workspace.player.is_none());
}

#[test]
fn actions_drive_the_session_without_a_client_adapter() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::open(root.path()).expect("open workspace");
    settle_background(&mut workspace);
    assert!(workspace.bundle.project.is_some());

    workspace.dispatch(Action::ShowView(View::Mixer));
    assert_eq!(workspace.view, View::Mixer);
    workspace.dispatch(Action::CycleView(-1));
    assert_eq!(workspace.view, View::Timeline);

    workspace.dispatch(Action::Timeline(TimelineAction::Zoom(1)));
    assert_eq!(workspace.timeline_viewport.zoom_factor(), 2);
    workspace.dispatch(Action::Timeline(TimelineAction::Fit));
    assert_eq!(workspace.timeline_viewport.zoom_factor(), 1);

    workspace.dispatch(Action::Timeline(TimelineAction::OpenGoto));
    assert!(matches!(workspace.modal, Some(Modal::Input { .. })));
    workspace.dispatch(Action::ModalSelectAll);
    workspace.dispatch(Action::ModalInsert('2'));
    let Some(Modal::Input { value, .. }) = &workspace.modal else {
        panic!("goto prompt stays open while typing");
    };
    assert_eq!(value, "2");
    workspace.dispatch(Action::ModalDismiss);
    assert!(workspace.modal.is_none());

    workspace.dispatch(Action::EditSource);
    assert!(workspace.take_editor_request());
    assert!(!workspace.take_editor_request());

    assert!(!workspace.quit_requested());
    workspace.dispatch(Action::Quit);
    assert!(workspace.quit_requested());
    workspace.shutdown();
}

#[test]
fn pointer_actions_place_the_cursor_on_the_grid_and_replace_modal_input() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let song_end = workspace.bundle.project.as_ref().expect("Project").song.end;
    assert_eq!(song_end, crate::time::MusicalTime::whole(4));

    // The fitted viewport spans four beats, so column 26 of 100 lies just past beat one and
    // snaps to it on the default one-beat grid.
    workspace.dispatch(Action::Timeline(TimelineAction::PlaceCursor {
        track: 0,
        column: 26,
        columns: 100,
    }));
    assert_eq!(
        workspace.timeline_position().at,
        crate::time::MusicalTime::whole(1)
    );
    // A pointer past the last Track or column stays inside the Song.
    workspace.dispatch(Action::Timeline(TimelineAction::PlaceCursor {
        track: 99,
        column: 99,
        columns: 100,
    }));
    assert_eq!(workspace.timeline_position().at, song_end);
    assert_eq!(workspace.selected_track, workspace.track_count() - 1);
    // Dragging after BeginRange moves the active edge, as the keyboard does.
    workspace.dispatch(Action::Timeline(TimelineAction::BeginRange));
    workspace.dispatch(Action::Timeline(TimelineAction::PlaceCursor {
        track: 0,
        column: 50,
        columns: 100,
    }));
    let range = workspace
        .timeline_state
        .visual_range()
        .expect("range selection");
    assert_eq!(range.start, crate::time::MusicalTime::whole(2));
    assert_eq!(range.end, song_end);
    workspace.dispatch(Action::CancelMode);

    workspace.dispatch(Action::Timeline(TimelineAction::OpenGoto));
    workspace.dispatch(Action::ModalSetInput("1.3".to_owned()));
    let Some(Modal::Input {
        value,
        replace_on_type,
        ..
    }) = &workspace.modal
    else {
        panic!("goto prompt stays open while its input is replaced");
    };
    assert_eq!(value, "1.3");
    assert!(!replace_on_type);
    workspace.dispatch(Action::ModalDismiss);

    workspace.dispatch(Action::ShowView(View::Source));
    let path = workspace
        .source_edit_path()
        .expect("prepare source")
        .expect("selected source");
    assert_eq!(path, workspace.root.join("project.crump"));
    workspace.shutdown();
}

#[test]
fn back_to_back_edits_queue_instead_of_being_dropped() {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/deep-house-reference");
    let root = tempfile::tempdir().expect("temporary Project");
    let copied = std::process::Command::new("cp")
        .arg("-r")
        .arg(source.join("."))
        .arg(root.path())
        .status()
        .expect("copy fixture");
    assert!(copied.success());
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    settle_background(&mut workspace);
    // A local `.crump/` History may lag the checked-in source; edits require Candidate == Active.
    workspace.commit_input("Fixture baseline".to_owned(), InputAction::Publish);
    settle_background(&mut workspace);
    assert!(workspace.candidate_is_active(), "{}", workspace.status);

    // Three edits in one burst, as a GUI frame can produce: the second and third arrive while the
    // first is in flight, and the last two write the same value so only the final one replays.
    for (target, value) in [
        ("track/bass/level", -9.0),
        ("track/keys/level", -12.0),
        ("track/keys/level", -13.0),
    ] {
        workspace.dispatch(Action::SetParameterTargetValue {
            target: target.to_owned(),
            value,
        });
        assert!(workspace.edits_pending());
    }
    assert_eq!(
        workspace.queued_mutations.len(),
        1,
        "same-value edits merge"
    );
    settle_background(&mut workspace);
    assert!(!workspace.edits_pending(), "{}", workspace.status);

    let song = &workspace.bundle.project.as_ref().expect("Project").song;
    let level = |id: &str| {
        song.tracks
            .iter()
            .find(|track| track.id == id)
            .expect("Track")
            .channel
            .level_db
    };
    assert!((level("bass") - -9.0).abs() < 1e-9);
    assert!((level("keys") - -13.0).abs() < 1e-9);
}
