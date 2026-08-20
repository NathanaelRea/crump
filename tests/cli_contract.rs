//! End-to-end tests for the stable machine-readable command contract.

use std::{
    fs,
    process::{Command, Output},
};

use serde_json::Value;
use tempfile::TempDir;

fn fixture_project() -> TempDir {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/minimal.crump"),
        root.path().join("project.crump"),
    )
    .expect("copy fixture");
    root
}

fn invoke(root: &TempDir, arguments: &[&str]) -> Output {
    invoke_with_data(root, &root.path().join("xdg-data"), arguments)
}

fn invoke_with_data(root: &TempDir, data_home: &std::path::Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crump"))
        .arg("--project")
        .arg(root.path())
        .arg("--json")
        .env("XDG_DATA_HOME", data_home)
        .args(arguments)
        .output()
        .expect("run crump")
}

fn response(output: &Output) -> Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("UTF-8 stdout");
    assert_eq!(
        stdout.lines().count(),
        1,
        "stdout must contain exactly one JSON response: {stdout:?}"
    );
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_str(stdout.trim()).expect("JSON response");
    assert_eq!(value["schema_version"], 1);
    value
}

#[test]
fn json_mode_wraps_usage_help_and_errors_without_stderr() {
    let root = fixture_project();
    let invalid = invoke(&root, &["--definitely-not-an-option"]);
    assert_eq!(invalid.status.code(), Some(2));
    let invalid = response(&invalid);
    assert_eq!(invalid["command"], "cli");
    assert_eq!(invalid["error"]["code"], "usage_error");

    let help = invoke(&root, &["--help"]);
    assert!(help.status.success());
    let help = response(&help);
    assert_eq!(help["command"], "help");
    assert!(help["data"]["text"].as_str().unwrap().contains("Usage:"));
}

fn publish(root: &TempDir) -> Value {
    let output = invoke(
        root,
        &[
            "publish",
            "--origin",
            "system",
            "--actor",
            "test",
            "--summary",
            "Initial publication",
            "--explanation",
            "integration fixture",
        ],
    );
    assert!(output.status.success());
    response(&output)
}

#[test]
fn invalid_candidate_preserves_active_and_render_uses_active_snapshot() {
    let root = fixture_project();
    let published = publish(&root);
    let active = published["data"]["revision"]
        .as_str()
        .expect("revision")
        .to_owned();

    let path = root.path().join("project.crump");
    let source = fs::read_to_string(&path).expect("fixture source");
    fs::write(&path, source.replacen("3200hz", "99999hz", 1)).expect("invalid Candidate");

    let status_output = invoke(&root, &["status"]);
    assert!(status_output.status.success());
    let status = response(&status_output);
    assert_eq!(status["data"]["state"], "invalid_candidate");
    assert_eq!(status["data"]["active_revision"], active);
    assert_eq!(status["data"]["candidate_is_active"], false);

    let publish_output = invoke(
        &root,
        &[
            "publish",
            "--summary",
            "Must fail",
            "--explanation",
            "invalid source",
        ],
    );
    assert_eq!(publish_output.status.code(), Some(2));
    let rejected = response(&publish_output);
    assert_eq!(rejected["error"]["code"], "invalid_candidate");

    let render_output = invoke(&root, &["render", "--output", "master"]);
    assert!(render_output.status.success());
    let rendered = response(&render_output);
    assert_eq!(rendered["data"]["rendered_active_revision"], active);
    assert_eq!(rendered["data"]["candidate_was_active"], false);
    assert!(root.path().join("master/master.wav").is_file());
    assert!(root.path().join("master/manifest.json").is_file());
}

#[test]
fn render_plan_is_non_mutating_and_no_replace_is_a_conflict() {
    let root = fixture_project();
    let published = publish(&root);
    let active = published["data"]["revision"].as_str().expect("revision");

    let plan_output = invoke(&root, &["render", "--plan", "--output", "master"]);
    assert!(plan_output.status.success());
    let plan = response(&plan_output);
    assert_eq!(plan["data"]["rendered_active_revision"], active);
    assert_eq!(plan["data"]["destination_exists"], false);
    assert_eq!(plan["data"]["would_replace"], false);
    assert_eq!(plan["data"]["would_conflict"], false);
    assert_eq!(plan["data"]["render_range"]["start_frame"], 0);
    assert!(
        plan["data"]["render_range"]["end_frame_exclusive"]
            .as_u64()
            .unwrap()
            > plan["data"]["render_range"]["arrangement_end_frame"]
                .as_u64()
                .unwrap()
    );
    assert!(!root.path().join("master").exists());

    let rendered = invoke(&root, &["render", "--output", "master"]);
    assert!(rendered.status.success());
    let wav_before = fs::read(root.path().join("master/master.wav")).unwrap();
    let conflict = invoke(&root, &["render", "--output", "master", "--no-replace"]);
    assert_eq!(conflict.status.code(), Some(3));
    let conflict = response(&conflict);
    assert_eq!(conflict["error"]["code"], "destination_exists");
    assert_eq!(
        fs::read(root.path().join("master/master.wav")).unwrap(),
        wav_before
    );

    let conflict_plan = invoke(
        &root,
        &["render", "--plan", "--output", "master", "--no-replace"],
    );
    assert!(conflict_plan.status.success());
    let conflict_plan = response(&conflict_plan);
    assert_eq!(conflict_plan["data"]["destination_exists"], true);
    assert_eq!(conflict_plan["data"]["would_replace"], false);
    assert_eq!(conflict_plan["data"]["would_conflict"], true);

    let source_before = fs::read(root.path().join("project.crump")).expect("read source");
    let unsafe_plan = invoke(&root, &["render", "--plan", "--output", "."]);
    assert_eq!(unsafe_plan.status.code(), Some(1));
    let unsafe_plan = response(&unsafe_plan);
    assert_eq!(unsafe_plan["error"]["code"], "unsafe_destination");
    assert_eq!(
        fs::read(root.path().join("project.crump")).expect("source survives unsafe plan"),
        source_before
    );
}

#[test]
fn agent_publication_requires_explanation_and_records_provenance() {
    let root = fixture_project();
    let rejected_output = invoke(
        &root,
        &[
            "publish",
            "--origin",
            "agent",
            "--actor",
            "arranger-bot",
            "--summary",
            "Arrange sketch",
        ],
    );
    assert_eq!(rejected_output.status.code(), Some(2));
    let rejected = response(&rejected_output);
    assert_eq!(rejected["error"]["code"], "publication_rejected");

    let accepted_output = invoke(
        &root,
        &[
            "publish",
            "--origin",
            "agent",
            "--actor",
            "arranger-bot",
            "--summary",
            "Arrange sketch",
            "--explanation",
            "Added a restrained four-note motif for human review",
        ],
    );
    assert!(accepted_output.status.success());
    let accepted = response(&accepted_output);
    let entry = &accepted["data"]["history_entry"];
    assert_eq!(entry["origin"]["kind"], "agent");
    assert_eq!(entry["origin"]["name"], "arranger-bot");
    assert_eq!(
        entry["explanation"],
        "Added a restrained four-note motif for human review"
    );
}

#[test]
fn publishing_the_exact_active_revision_is_a_conflict() {
    let root = fixture_project();
    publish(&root);

    let duplicate_output = invoke(&root, &["publish", "--summary", "Duplicate publication"]);
    assert_eq!(duplicate_output.status.code(), Some(3));
    let duplicate = response(&duplicate_output);
    assert_eq!(duplicate["error"]["code"], "already_active");

    let list_output = invoke(&root, &["history", "list"]);
    assert!(list_output.status.success());
    assert_eq!(
        response(&list_output)["data"]["entries"]
            .as_array()
            .expect("entries")
            .len(),
        1
    );
}

#[test]
fn structured_edit_and_restore_are_append_only_and_exact() {
    let root = fixture_project();
    let original_source = fs::read_to_string(root.path().join("project.crump")).expect("source");
    let initial = publish(&root);
    let initial_entry = initial["data"]["history_entry"]["id"]
        .as_str()
        .expect("entry ID")
        .to_owned();
    let initial_revision = initial["data"]["revision"]
        .as_str()
        .expect("revision")
        .to_owned();

    let edit_output = invoke(
        &root,
        &[
            "edit",
            "track-level",
            "lead",
            "-8",
            "--summary",
            "Raise lead",
        ],
    );
    assert!(edit_output.status.success());
    let edited = fs::read_to_string(root.path().join("project.crump")).expect("edited source");
    assert_eq!(
        edited,
        original_source.replacen("level -9db", "level -8db", 1)
    );

    let restore_output = invoke(
        &root,
        &[
            "history",
            "restore",
            &initial_entry,
            "--actor",
            "tester",
            "--explanation",
            "Verify exact restore",
        ],
    );
    assert!(restore_output.status.success());
    let restored = response(&restore_output);
    assert_eq!(
        restored["data"]["history_entry"]["revision"],
        initial_revision
    );
    assert_eq!(
        fs::read_to_string(root.path().join("project.crump")).expect("restored source"),
        original_source
    );

    let list_output = invoke(&root, &["history", "list"]);
    assert!(list_output.status.success());
    let list = response(&list_output);
    assert_eq!(
        list["data"]["entries"].as_array().expect("entries").len(),
        3
    );
}

#[test]
fn event_grid_cli_edits_are_checked_published_and_source_preserving() {
    let root = fixture_project();
    publish(&root);

    for arguments in [
        vec!["edit", "event-time", "melody", "0", "1/2beat"],
        vec!["edit", "note-duration", "melody", "0", "1/2beat"],
        vec!["edit", "event-velocity", "melody", "0", "0.65"],
        vec!["edit", "hit-voice", "beat", "0", "clap"],
    ] {
        let output = invoke(&root, &arguments);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(response(&output)["ok"], true);
    }

    let source = fs::read_to_string(root.path().join("project.crump")).expect("edited source");
    assert!(source.contains("note 1/2qn C4 1/2qn 0.65"));
    assert!(source.contains("hit 0beat clap 0.9"));

    let before_rejected = source.clone();
    let rejected = invoke(&root, &["edit", "note-duration", "melody", "0", "0beat"]);
    assert_eq!(rejected.status.code(), Some(2));
    assert_eq!(response(&rejected)["error"]["code"], "edit_rejected");
    assert_eq!(
        fs::read_to_string(root.path().join("project.crump")).expect("unchanged source"),
        before_rejected
    );
}

#[test]
fn workspace_json_is_rejected_without_terminal_control_bytes() {
    let root = fixture_project();
    let output = invoke(&root, &["workspace"]);
    assert_eq!(output.status.code(), Some(2));
    let value = response(&output);
    assert_eq!(value["error"]["code"], "interactive_command");
}

#[test]
fn unknown_history_ids_are_blocked_input_not_operational_failures() {
    let root = fixture_project();
    publish(&root);
    let output = invoke(&root, &["history", "show", "999999999999"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(response(&output)["error"]["code"], "unknown_history_entry");
}

#[test]
fn oversized_modules_are_rejected_before_parsing_or_installation() {
    let root = fixture_project();
    let source = root.path().join("oversized.crump");
    fs::write(
        &source,
        vec![b' '; crump::project::MAX_SOURCE_BYTES as usize + 1],
    )
    .expect("oversized module");
    let output = invoke(
        &root,
        &[
            "module",
            "install",
            source.to_str().expect("source path"),
            "--name",
            "oversized",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(response(&output)["error"]["code"], "module_too_large");
    assert!(
        !root
            .path()
            .join("xdg-data/Crump/instruments/oversized.crump")
            .exists()
    );
}

#[test]
fn module_export_is_validated_atomic_and_conflict_safe() {
    let root = fixture_project();
    let exported_output = invoke(&root, &["module", "export", "lead", "--name", "test-lead"]);
    assert!(exported_output.status.success());
    let exported = response(&exported_output);
    let module_path = exported["data"]["path"].as_str().expect("module path");
    assert!(std::path::Path::new(module_path).is_file());

    let inspected_output = invoke(&root, &["module", "inspect", "test-lead"]);
    assert!(inspected_output.status.success());
    let inspected = response(&inspected_output);
    assert_eq!(inspected["data"]["instruments"][0]["id"], "lead");

    let conflict_output = invoke(&root, &["module", "export", "lead", "--name", "test-lead"]);
    assert_eq!(conflict_output.status.code(), Some(3));
    assert_eq!(response(&conflict_output)["error"]["code"], "module_exists");

    let invalid_path = root.path().join("invalid-module.crump");
    let invalid = fs::read_to_string(module_path)
        .expect("exported source")
        .replacen("3200hz", "99999hz", 1);
    fs::write(&invalid_path, invalid).expect("invalid module");
    let invalid_output = invoke(
        &root,
        &[
            "module",
            "install",
            invalid_path.to_str().expect("UTF-8 path"),
            "--name",
            "invalid-lead",
        ],
    );
    assert_eq!(invalid_output.status.code(), Some(2));
    let rejected = response(&invalid_output);
    assert_eq!(rejected["error"]["code"], "invalid_module");
    assert!(
        rejected["error"]["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["code"] == "CRV024")
    );

    let consumer = tempfile::tempdir().expect("consumer Project");
    let consumer_source = r#"crump 1
import "library:test-lead"
project "Library Consumer" {
  id library-consumer
  seed 99
  sample_rate 48000
  render_tail 250ms
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    pattern melody 1bar {
      note 0beat C4 1beat 0.8
      note 1beat E4 1beat 0.8
    }
    track lead-track "Lead" instrument lead level -9db pan 0 {
      clip melody-clip pattern melody start 0bar duration 1bar mode once
    }
  }
}
"#;
    fs::write(consumer.path().join("project.crump"), consumer_source).expect("consumer source");
    let data_home = root.path().join("xdg-data");
    let validate = invoke_with_data(&consumer, &data_home, &["validate"]);
    assert!(
        validate.status.success(),
        "{}",
        String::from_utf8_lossy(&validate.stdout)
    );
    let inspect = response(&invoke_with_data(&consumer, &data_home, &["inspect"]));
    assert!(
        inspect["data"]["source_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "library/test-lead.crump")
    );

    let publication = invoke_with_data(
        &consumer,
        &data_home,
        &[
            "publish",
            "--origin",
            "system",
            "--actor",
            "test",
            "--summary",
            "Publish library consumer",
            "--explanation",
            "reuse test",
        ],
    );
    assert!(publication.status.success());
    let publication = response(&publication);
    let active_revision = publication["data"]["revision"].as_str().unwrap().to_owned();
    let active_entry = publication["data"]["history_entry"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let original_module = fs::read_to_string(module_path).expect("installed module");
    fs::write(module_path, original_module.replacen("3200hz", "4000hz", 1))
        .expect("change personal library Module");

    let render = invoke_with_data(
        &consumer,
        &data_home,
        &["render", "--output", "consumer-master"],
    );
    assert!(render.status.success());
    let render = response(&render);
    assert_eq!(render["data"]["rendered_active_revision"], active_revision);
    assert_eq!(render["data"]["candidate_was_active"], false);

    let restore = invoke_with_data(
        &consumer,
        &data_home,
        &[
            "history",
            "restore",
            &active_entry,
            "--actor",
            "test",
            "--explanation",
            "materialize the exact library snapshot",
        ],
    );
    assert!(
        restore.status.success(),
        "{}",
        String::from_utf8_lossy(&restore.stdout)
    );
    assert_eq!(
        fs::read_to_string(consumer.path().join("library/test-lead.crump"))
            .expect("materialized Module"),
        original_module
    );
}
