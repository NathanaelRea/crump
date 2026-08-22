use std::fs;

use super::*;

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
