//! End-to-end tests for the stable machine-readable command contract.

use std::{
    fs,
    process::{Command, Output},
};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn fixture_project() -> TempDir {
    fixture_named("minimal.crump")
}

fn fixture_named(name: &str) -> TempDir {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
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

fn write_test_wav(path: &std::path::Path) -> Vec<u8> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .expect("WAV writer");
    for sample in [0_i16, 100, -100] {
        writer.write_sample(sample).expect("sample");
    }
    writer.finalize().expect("finalize");
    fs::read(path).expect("WAV bytes")
}

fn write_sampler_wav(path: &std::path::Path) -> Vec<u8> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .expect("WAV writer");
    for index in 0..512 {
        let sample = if (index / 16) % 2 == 0 {
            12_000_i16
        } else {
            -12_000_i16
        };
        writer.write_sample(sample).expect("sample");
    }
    writer.finalize().expect("finalize");
    fs::read(path).expect("WAV bytes")
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
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    response(&output)
}

#[test]
fn portable_asset_import_and_list_are_source_backed() {
    let root = fixture_project();
    publish(&root);
    let input = root.path().join("external.wav");
    let bytes = write_test_wav(&input);
    let input = input.to_str().expect("UTF-8 path");
    let imported = invoke(
        &root,
        &["asset", "import", input, "--id", "voice", "--name", "Voice"],
    );
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stdout)
    );
    assert_eq!(response(&imported)["command"], "asset.import");
    assert_eq!(
        fs::read(root.path().join("media/voice.wav")).expect("portable copy"),
        bytes
    );
    let source = fs::read_to_string(root.path().join("project.crump")).expect("source");
    assert!(source.contains("asset voice \"Voice\" wav \"media/voice.wav\" sha256"));

    let listed = invoke(&root, &["asset", "list"]);
    assert!(listed.status.success());
    let listed = response(&listed);
    assert_eq!(listed["data"]["assets"][0]["id"], "voice");
    assert_eq!(listed["data"]["assets"][0]["path"], "media/voice.wav");
    assert_eq!(listed["data"]["assets"][0]["frames"], 3);

    let inspected = invoke(&root, &["inspect"]);
    assert!(inspected.status.success());
    let inspected = response(&inspected);
    assert_eq!(inspected["data"]["assets"][0]["id"], "voice");
    assert!(inspected["data"]["assets"][0].get("bytes").is_none());
}

#[test]
fn sampler_collect_edit_and_render_are_end_to_end_portable() {
    let root = tempfile::tempdir().expect("temporary sampler Project");
    fs::create_dir(root.path().join("takes")).expect("takes directory");
    let bytes = write_sampler_wav(&root.path().join("takes/raw.wav"));
    let sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let source = format!(
        r#"crump 1
project "Sampler CLI" {{
  id sampler-cli
  seed 19
  sample_rate 48000
  render_tail 10ms
  asset source "Source" wav "takes/raw.wav" sha256 "{sha256}"
  song {{
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument samples "Samples" sampler {{
      polyphony 2
      parameter gain "Gain" db -96db 24db 0db 0db
      faceplate {{
        control gain "Gain" "Sampler level" maps gain
      }}
      mapping zone asset source keys C4 C4 root C4 {{
        region 0 512
        playback one_shot
        gain 0db
        pan 0
        envelope 0s 5ms
        fade 4 4
      }}
    }}
    pattern phrase 1bar {{
    }}
    track samples "Samples" instrument samples level 0db pan 0 {{
      clip phrase pattern phrase start 0bar duration 1bar mode once
    }}
  }}
}}
"#
    );
    fs::write(root.path().join("project.crump"), source).expect("sampler source");
    publish(&root);

    let raw_plan = invoke(
        &root,
        &[
            "play",
            "--target",
            "asset/source",
            "--path",
            "raw",
            "--plan",
        ],
    );
    assert!(
        raw_plan.status.success(),
        "{}",
        String::from_utf8_lossy(&raw_plan.stdout)
    );
    let raw_plan = response(&raw_plan);
    assert_eq!(raw_plan["data"]["audition"]["status"]["start_frame"], 0);
    assert_eq!(raw_plan["data"]["audition"]["status"]["end_frame"], 512);

    let routed_asset = invoke(
        &root,
        &[
            "play",
            "--target",
            "asset/source",
            "--path",
            "master",
            "--plan",
        ],
    );
    assert_eq!(routed_asset.status.code(), Some(2));
    assert!(
        response(&routed_asset)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("supports only the raw audition path")
    );

    let collected = invoke(&root, &["asset", "collect"]);
    assert!(
        collected.status.success(),
        "{}",
        String::from_utf8_lossy(&collected.stdout)
    );
    assert_eq!(response(&collected)["command"], "asset.collect");
    assert_eq!(
        fs::read(root.path().join("media/source.wav")).expect("collected WAV"),
        bytes
    );
    assert!(!root.path().join("takes/raw.wav").exists());

    for arguments in [
        vec![
            "edit",
            "sample-place",
            "phrase",
            "first",
            "zone",
            "0beat",
            "1beat",
            "0",
            "512",
            "--fade-in",
            "8",
            "--fade-out",
            "12",
            "--velocity",
            "0.75",
        ],
        vec!["edit", "sample-gain", "phrase", "first", "-3"],
        vec!["edit", "sample-pan", "phrase", "first", "0.25"],
    ] {
        let edited = invoke(&root, &arguments);
        assert!(
            edited.status.success(),
            "{}",
            String::from_utf8_lossy(&edited.stdout)
        );
        assert_eq!(response(&edited)["ok"], true);
    }

    let final_source =
        fs::read_to_string(root.path().join("project.crump")).expect("edited source");
    assert!(final_source.contains("asset source \"Source\" wav \"media/source.wav\""));
    assert!(final_source.contains(
        "sample first 0qn mapping zone duration 1qn start 0 end 512 gain -3db pan 0.25 fade 8 12 velocity 0.75"
    ));

    let rendered = invoke(&root, &["render", "--output", "master"]);
    assert!(
        rendered.status.success(),
        "{}",
        String::from_utf8_lossy(&rendered.stdout)
    );
    let rendered = response(&rendered);
    assert_eq!(rendered["command"], "render");
    let mut reader =
        hound::WavReader::open(root.path().join("master/master.wav")).expect("rendered master WAV");
    assert!(
        reader
            .samples::<f32>()
            .any(|sample| sample.expect("rendered sample").abs() > 1.0e-5)
    );
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

    let plan_output = invoke(
        &root,
        &[
            "render",
            "--plan",
            "--output",
            "master",
            "--stem",
            "track/lead:post_fader",
        ],
    );
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
    assert_eq!(plan["data"]["stems"].as_array().unwrap().len(), 1);
    assert_eq!(plan["data"]["stems"][0]["owner"]["role"], "track");
    assert_eq!(plan["data"]["stems"][0]["owner"]["id"], "lead");
    assert_eq!(plan["data"]["stems"][0]["tap"], "post_fader");
    assert!(!root.path().join("master").exists());

    let rendered = invoke(
        &root,
        &[
            "render",
            "--output",
            "master",
            "--stem",
            "track/lead:post_fader",
        ],
    );
    assert!(rendered.status.success());
    let rendered_response = response(&rendered);
    assert_eq!(
        rendered_response["data"]["render"]["manifest"]["stems"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        rendered_response["data"]["render"]["manifest"]["analysis"]["non_finite_sample_count"],
        0
    );
    assert!(
        root.path()
            .join("master/stem-track-lead-post_fader.wav")
            .is_file()
    );
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
fn qualification_plan_and_report_keep_offline_and_external_evidence_distinct() {
    let root = fixture_project();
    publish(&root);
    let report_path = root.path().join("evidence/qualification.json");

    let plan = invoke(
        &root,
        &[
            "qualify",
            "--plan",
            "--quantum",
            "64",
            "--output",
            "evidence/qualification.json",
        ],
    );
    assert!(
        plan.status.success(),
        "{}{}",
        String::from_utf8_lossy(&plan.stdout),
        String::from_utf8_lossy(&plan.stderr)
    );
    let plan = response(&plan);
    assert_eq!(plan["command"], "qualify");
    assert_eq!(plan["data"]["backend"], "offline");
    assert_eq!(plan["data"]["quantum_frames"], 64);
    assert_eq!(plan["data"]["final_block_frames"], 32);
    assert_eq!(plan["data"]["report_exists"], false);
    assert_eq!(plan["data"]["would_conflict"], false);
    assert_eq!(
        plan["data"]["resources"]["supported_quanta"],
        serde_json::json!([64, 128, 256])
    );
    assert_eq!(
        plan["data"]["external_evidence_required"]
            .as_array()
            .expect("external evidence")
            .len(),
        3
    );
    assert!(!report_path.exists());

    let qualified = invoke(
        &root,
        &[
            "qualify",
            "--quantum",
            "256",
            "--maximum-p999-utilization",
            "1",
            "--output",
            "evidence/qualification.json",
        ],
    );
    assert!(
        qualified.status.success(),
        "{}{}",
        String::from_utf8_lossy(&qualified.stdout),
        String::from_utf8_lossy(&qualified.stderr)
    );
    let qualified = response(&qualified);
    let report = &qualified["data"]["qualification"];
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["backend"], "offline");
    assert_eq!(report["requested_quantum_frames"], 256);
    assert_eq!(report["actual_quantum_frames"], 256);
    assert_eq!(report["final_block_frames"], 224);
    assert_eq!(report["pipewire_xruns"], Value::Null);
    assert_eq!(report["non_finite_sample_count"], 0);
    assert!(report["callback_blocks"].as_u64().unwrap() > 0);
    assert_eq!(
        report["external_evidence_required"]
            .as_array()
            .expect("external evidence")
            .len(),
        3
    );
    let written: Value =
        serde_json::from_slice(&fs::read(&report_path).expect("qualification report"))
            .expect("report JSON");
    assert_eq!(&written, report);

    let conflict = invoke(
        &root,
        &[
            "qualify",
            "--quantum",
            "256",
            "--output",
            "evidence/qualification.json",
        ],
    );
    assert_eq!(conflict.status.code(), Some(3));
    let conflict = response(&conflict);
    assert_eq!(conflict["error"]["code"], "report_exists");
    assert_eq!(
        serde_json::from_slice::<Value>(
            &fs::read(&report_path).expect("preserved qualification report")
        )
        .expect("preserved report JSON"),
        written
    );
}

#[test]
fn qualification_report_refuses_project_source_history_and_media_destinations() {
    let root = fixture_project();
    let media_path = root.path().join("texture.wav");
    let media_bytes = write_test_wav(&media_path);
    let media_sha256 = Sha256::digest(&media_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let module_path = root.path().join("transition-noise.crump");
    fs::write(
        &module_path,
        include_str!("../fixtures/expressive/transition-noise.crump"),
    )
    .expect("imported module");
    let source_path = root.path().join("project.crump");
    let source = fs::read_to_string(&source_path)
        .expect("Project source")
        .replacen(
            "crump 1\n",
            "crump 1\nimport \"transition-noise.crump\"\n",
            1,
        )
        .replacen(
            "  render_tail 250ms\n",
            &format!(
                "  render_tail 250ms\n  asset texture \"Texture\" wav \"texture.wav\" sha256 \"{media_sha256}\"\n"
            ),
            1,
        );
    fs::write(&source_path, source).expect("Project with module and media");
    publish(&root);

    for relative in [
        "project.crump",
        "transition-noise.crump",
        ".crump/state.json",
        "texture.wav",
    ] {
        let path = root.path().join(relative);
        let before = fs::read(&path).expect("protected bytes");
        let rejected = invoke(
            &root,
            &["qualify", "--plan", "--output", relative, "--replace"],
        );
        assert_eq!(rejected.status.code(), Some(2), "{relative}");
        let rejected = response(&rejected);
        assert_eq!(rejected["error"]["code"], "unsafe_destination");
        assert_eq!(fs::read(path).expect("preserved protected bytes"), before);
    }
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
fn generic_target_edit_covers_mix_routing_scopes() {
    let root = fixture_named("production_mix.crump");
    publish(&root);

    for (target, value) in [
        ("group/mix_bus/level", "-2"),
        ("return/room/balance", "0.2"),
        ("send/drum_room/level", "-9"),
        ("processor/bass_comp/threshold", "-18"),
        ("master/level", "-3"),
    ] {
        let output = invoke(&root, &["edit", "target", target, value]);
        assert!(
            output.status.success(),
            "{}: {}",
            target,
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(response(&output)["ok"], true);
    }

    let source = fs::read_to_string(root.path().join("project.crump")).expect("edited source");
    assert!(source.contains("group mix_bus \"Mix Bus\" level -2db"));
    assert!(source.contains("return room \"Room\" level -3db {\n      panner balance 0.2"));
    assert!(source.contains("send drum_room target return/room tap pre level -9db"));
    assert!(source.contains("setting threshold -18db"));
    assert!(source.contains("master level -3db"));
}

#[test]
fn inspect_exposes_resolved_processor_contracts_and_admission_facts() {
    let root = fixture_named("production_mix.crump");
    publish(&root);

    let output = invoke(&root, &["inspect"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let inspected = response(&output);
    let mix_graph = &inspected["data"]["mix_graph"];
    let processors = mix_graph["processors"]
        .as_array()
        .expect("processor inspection array");
    let compressor = processors
        .iter()
        .find(|processor| processor["id"] == "bass_comp")
        .expect("compressor inspection");
    let threshold = compressor["settings"]
        .as_array()
        .expect("resolved settings")
        .iter()
        .find(|setting| setting["key"] == "threshold")
        .expect("resolved threshold");
    assert_eq!(threshold["unit"], "decibels");
    assert_eq!(threshold["value"], -24.0);
    assert_eq!(threshold["minimum"], -96.0);
    assert_eq!(threshold["authored"], true);
    assert_eq!(threshold["parameter_target"], true);

    let limiter = processors
        .iter()
        .find(|processor| processor["id"] == "master_limiter")
        .expect("Limiter inspection");
    assert_eq!(limiter["intrinsic_latency_frames"], 256);
    assert_eq!(limiter["output_latency_frames"], 256);
    assert_eq!(limiter["nonlinear_substeps"], 1);
    assert_eq!(limiter["true_peak_safety_margin_db"], 0.1);
    let lookahead = limiter["settings"]
        .as_array()
        .expect("Limiter settings")
        .iter()
        .find(|setting| setting["key"] == "lookahead")
        .expect("resolved lookahead");
    assert_eq!(lookahead["parameter_target"], false);
    assert!(
        mix_graph["resources"]["processor_meter_bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0)
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
fn timeline_clip_and_locator_cli_edits_are_source_backed() {
    let root = fixture_project();
    publish(&root);

    for arguments in [
        vec!["edit", "locator-create", "intro", "Intro", "0bar"],
        vec!["edit", "locator-create", "outro", "Outro", "1bar"],
        vec![
            "edit",
            "locator-move",
            "outro",
            "23/8beat",
            "--snap",
            "--grid",
            "1/4beat",
        ],
        vec![
            "edit",
            "groove-create",
            "shuffle",
            "1/2beat",
            "0.625",
            "0beat",
            "0",
            "1",
            "41",
        ],
        vec![
            "edit",
            "clip-create",
            "lead",
            "alternate",
            "melody",
            "0bar",
            "1bar",
            "--mode",
            "loop",
            "--offset",
            "1beat",
            "--transpose",
            "12",
            "--gain-db",
            "-6",
            "--fade-in",
            "1/2beat",
            "--fade-out",
            "1/2beat",
            "--groove",
            "shuffle",
        ],
        vec![
            "edit",
            "clip-duplicate",
            "alternate",
            "alternate-copy",
            "0bar",
        ],
        vec![
            "edit",
            "clip-split",
            "alternate-copy",
            "alternate-tail",
            "2beat",
        ],
        vec!["edit", "clip-move", "alternate-tail", "1beat"],
        vec!["edit", "clip-resize", "alternate-copy", "1beat"],
        vec!["edit", "clip-offset", "alternate", "0beat"],
        vec!["edit", "clip-transpose", "alternate", "7"],
        vec!["edit", "clip-gain", "alternate", "-3"],
        vec!["edit", "clip-fade", "alternate", "1beat", "0beat"],
        vec![
            "edit",
            "groove-update",
            "shuffle",
            "1/2beat",
            "0.6",
            "1/32beat",
            "0.05",
            "0.9",
            "42",
        ],
        vec!["edit", "clip-delete", "alternate-tail"],
    ] {
        let output = invoke(&root, &arguments);
        assert!(
            output.status.success(),
            "{}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(response(&output)["ok"], true);
    }

    let source = fs::read_to_string(root.path().join("project.crump")).expect("edited source");
    assert!(source.contains("locator intro \"Intro\" at 0qn"));
    assert!(source.contains("locator outro \"Outro\" at 3qn"));
    assert!(source.contains("clip alternate pattern melody start 0qn duration 4qn mode loop"));
    assert!(source.contains("offset 0qn"));
    assert!(source.contains("transpose 7st"));
    assert!(source.contains("gain -3db"));
    assert!(source.contains("fade 1qn 0qn"));
    assert!(source.contains("groove shuffle\n"));
    assert!(source.contains(
        "groove shuffle grid 1/2qn swing 0.6 timing 1/32qn velocity 0.05 probability 0.9 seed 42"
    ));
    assert!(source.contains("clip alternate-copy pattern melody start 0qn duration 1qn mode loop"));
    assert!(!source.contains("clip alternate-tail "));

    let validate = invoke(&root, &["validate"]);
    assert!(validate.status.success());
    assert_eq!(response(&validate)["data"]["valid"], true);

    let rejected_delete = invoke(&root, &["edit", "groove-delete", "shuffle"]);
    assert_eq!(rejected_delete.status.code(), Some(2));
    assert_eq!(response(&rejected_delete)["error"]["code"], "edit_rejected");

    let range = invoke(
        &root,
        &[
            "play",
            "--from",
            "intro",
            "--to",
            "outro",
            "--loop-range",
            "--plan",
        ],
    );
    assert!(range.status.success());
    let range = response(&range);
    assert_eq!(range["data"]["range"]["start_frame"], 0);
    assert_eq!(range["data"]["range"]["end_frame"], 72_000);
    assert_eq!(range["data"]["range"]["loop"], true);
}

#[test]
fn parameter_lock_cli_is_source_backed_snapped_and_inspectable() {
    let root = fixture_project();
    let project_path = root.path().join("project.crump");
    let source = fs::read_to_string(&project_path).expect("source").replace(
        "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz",
        "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz automatable",
    );
    fs::write(&project_path, source).expect("automatable fixture Parameter");
    publish(&root);

    let create = invoke(
        &root,
        &[
            "edit",
            "parameter-lock-create",
            "melody",
            "opened",
            "3/16beat",
            "tone",
            "1000hz",
            "--snap",
            "--grid",
            "1/4beat",
        ],
    );
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stdout)
    );
    let update = invoke(
        &root,
        &[
            "edit",
            "parameter-lock-update",
            "melody",
            "opened",
            "2beat",
            "tone",
            "9000hz",
        ],
    );
    assert!(
        update.status.success(),
        "{}",
        String::from_utf8_lossy(&update.stdout)
    );

    let source = fs::read_to_string(&project_path).expect("Parameter lock source");
    assert!(source.contains("lock opened at 2qn parameter tone value 9000hz"));
    let inspect = invoke(&root, &["inspect", "--at", "2beat"]);
    assert!(inspect.status.success());
    let inspection = response(&inspect);
    let tone = inspection["data"]["effective_parameters"]
        .as_array()
        .expect("effective Parameters")
        .iter()
        .find(|value| {
            value["target"]["owner"] == "instrument"
                && value["target"]["instrument"] == "lead"
                && value["target"]["parameter"] == "tone"
        })
        .expect("lead tone snapshot");
    assert_eq!(tone["parameter_lock"], 9_000.0);
    assert_eq!(tone["effective"], 9_000.0);

    let rejected = invoke(
        &root,
        &[
            "edit",
            "parameter-lock-update",
            "melody",
            "opened",
            "2beat",
            "tone",
            "20000hz",
        ],
    );
    assert_eq!(rejected.status.code(), Some(2));
    assert_eq!(response(&rejected)["error"]["code"], "edit_rejected");
    assert_eq!(
        fs::read_to_string(&project_path).expect("unchanged lock source"),
        source
    );

    let delete = invoke(
        &root,
        &["edit", "parameter-lock-delete", "melody", "opened"],
    );
    assert!(delete.status.success());
    assert!(
        !fs::read_to_string(project_path)
            .expect("deleted lock source")
            .contains("lock opened ")
    );
}

#[test]
fn pattern_variation_cli_preserves_lineage_and_checks_references() {
    let root = fixture_project();
    publish(&root);

    for arguments in [
        vec![
            "edit",
            "pattern-variation-create",
            "answer",
            "melody",
            "1beat",
            "7",
            "0.8",
            "false",
            "1",
            "77",
        ],
        vec![
            "edit",
            "pattern-variation-update",
            "answer",
            "melody",
            "2beat",
            "-5",
            "0.6",
            "true",
            "0.9",
            "78",
        ],
        vec![
            "edit",
            "clip-create",
            "lead",
            "answer-clip",
            "answer",
            "0bar",
            "1bar",
            "--mode",
            "loop",
        ],
    ] {
        let output = invoke(&root, &arguments);
        assert!(
            output.status.success(),
            "{}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(response(&output)["ok"], true);
    }

    let project_path = root.path().join("project.crump");
    let source = fs::read_to_string(&project_path).expect("Variation source");
    assert!(source.contains("pattern answer 4qn from melody {"));
    assert!(source.contains("rotate 2qn"));
    assert!(source.contains("transpose -5st"));
    assert!(source.contains("velocity 0.6"));
    assert!(source.contains("reverse true"));
    assert!(source.contains("probability 0.9"));
    assert!(source.contains("seed 78"));
    assert!(!source.contains("pattern answer 4qn from melody {\n      note "));

    let validate = invoke(&root, &["validate"]);
    assert!(validate.status.success());
    assert_eq!(response(&validate)["data"]["valid"], true);

    let before_rejected = source.clone();
    let referenced = invoke(&root, &["edit", "pattern-variation-delete", "answer"]);
    assert_eq!(referenced.status.code(), Some(2));
    assert_eq!(response(&referenced)["error"]["code"], "edit_rejected");
    let cyclic = invoke(
        &root,
        &[
            "edit",
            "pattern-variation-update",
            "answer",
            "answer",
            "0beat",
            "0",
            "1",
            "false",
            "1",
            "79",
        ],
    );
    assert_eq!(cyclic.status.code(), Some(2));
    assert_eq!(response(&cyclic)["error"]["code"], "edit_rejected");
    assert_eq!(
        fs::read_to_string(&project_path).expect("unchanged Variation source"),
        before_rejected
    );

    for arguments in [
        vec!["edit", "clip-delete", "answer-clip"],
        vec!["edit", "pattern-variation-delete", "answer"],
    ] {
        let output = invoke(&root, &arguments);
        assert!(
            output.status.success(),
            "{}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout)
        );
    }
    assert!(
        !fs::read_to_string(project_path)
            .expect("deleted Variation source")
            .contains("pattern answer ")
    );
}

#[test]
fn arrangement_time_cli_edits_are_atomic_and_procedural() {
    let root = fixture_project();
    let project_path = root.path().join("project.crump");
    let source = fs::read_to_string(&project_path)
        .expect("source")
        .replacen("end 1bar", "end 4bar", 1);
    fs::write(&project_path, source).expect("extended Project");
    publish(&root);

    for arguments in [
        vec!["edit", "locator-create", "marker", "Marker", "2bar"],
        vec![
            "edit",
            "time-duplicate",
            "0bar",
            "1bar",
            "1bar",
            "copy",
            "--automation",
            "follow",
            "--free",
        ],
        vec![
            "edit",
            "time-insert",
            "2bar",
            "1bar",
            "--automation",
            "locked",
            "--free",
        ],
        vec![
            "edit",
            "time-delete",
            "1bar",
            "1bar",
            "--automation",
            "follow",
            "--free",
        ],
    ] {
        let output = invoke(&root, &arguments);
        assert!(
            output.status.success(),
            "{}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(response(&output)["ok"], true);
    }

    let source = fs::read_to_string(project_path).expect("edited source");
    assert!(source.contains("end 20qn"));
    assert!(source.contains("locator marker \"Marker\" at 12qn"));
    assert!(!source.contains("clip melody-copy "));
    assert!(!source.contains("clip beat-copy "));
    let validate = invoke(&root, &["validate"]);
    assert!(validate.status.success());
    assert_eq!(response(&validate)["data"]["valid"], true);
}

#[test]
fn automation_cli_edits_and_effective_value_inspection_are_source_backed() {
    let root = fixture_project();
    publish(&root);

    for arguments in [
        vec!["edit", "locator-create", "lift-marker", "Lift", "2beat"],
        vec![
            "edit",
            "automation-create",
            "lead-level",
            "track/lead/level",
            "-18db",
        ],
        vec![
            "edit",
            "automation-point-create",
            "lead-level",
            "lift",
            "lift-marker",
            "-6db",
            "--curve",
            "linear",
        ],
        vec![
            "edit",
            "automation-point-curve",
            "lead-level",
            "start",
            "linear",
        ],
        vec![
            "edit",
            "automation-point-value",
            "lead-level",
            "lift",
            "-6.123456789db",
        ],
        vec![
            "edit",
            "automation-point-curve",
            "lead-level",
            "lift",
            "power(0.0000004)",
        ],
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
    assert!(source.contains("automation lead-level target track/lead/level {"));
    assert!(source.contains("point start 0qn -18db linear"));
    assert!(source.contains("point lift 2qn -6.123456789db power(0.0000004)"));

    let inspected = invoke(&root, &["inspect", "--at", "1beat"]);
    assert!(inspected.status.success());
    let inspected = response(&inspected);
    assert_eq!(inspected["data"]["control_frame"], 24_000);
    let level = inspected["data"]["effective_parameters"]
        .as_array()
        .expect("effective Parameter array")
        .iter()
        .find(|value| {
            value["target"]["owner"] == "track"
                && value["target"]["track"] == "lead"
                && value["target"]["parameter"] == "level"
        })
        .expect("lead level provenance");
    assert_eq!(level["base"], -9.0);
    let midpoint = (-18.0 + -6.123_456_789) / 2.0;
    assert_eq!(level["automation"].as_f64(), Some(midpoint));
    assert_eq!(level["effective"].as_f64(), Some(midpoint));

    let out_of_range = invoke(&root, &["inspect", "--at", "999bar"]);
    assert_eq!(out_of_range.status.code(), Some(2));
    assert_eq!(
        response(&out_of_range)["error"]["code"],
        "invalid_control_time"
    );
}

#[test]
fn audition_and_processor_cli_workflows_are_source_backed_and_inspectable() {
    let root = fixture_named("production_mix.crump");
    publish(&root);

    let plan = invoke(
        &root,
        &[
            "play",
            "--target",
            "track/drums",
            "--path",
            "channel-post",
            "--from",
            "0bar",
            "--to",
            "1bar",
            "--bypass",
            "drum_latency",
            "--plan",
        ],
    );
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stdout)
    );
    let plan = response(&plan);
    assert_eq!(
        plan["data"]["audition"]["status"]["target"]["kind"],
        "track"
    );
    assert_eq!(
        plan["data"]["audition"]["status"]["target"]["track"],
        "drums"
    );
    assert_eq!(
        plan["data"]["audition"]["status"]["path"]["kind"],
        "channel"
    );
    let latency = plan["data"]["audition"]["processors"]
        .as_array()
        .expect("processors")
        .iter()
        .find(|processor| processor["id"] == "drum_latency")
        .expect("drum latency processor");
    assert_eq!(latency["transient_bypass"], true);
    assert_eq!(latency["effective_bypass"], true);

    for arguments in [
        vec![
            "edit",
            "processor-setting",
            "master_saturation",
            "drive",
            "1.75",
        ],
        vec!["edit", "processor-bypass", "master_saturation", "true"],
        vec!["edit", "processor-move", "master_limiter", "earlier"],
    ] {
        let output = invoke(&root, &arguments);
        assert!(
            output.status.success(),
            "{}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout)
        );
    }
    let source = fs::read_to_string(root.path().join("project.crump")).expect("edited source");
    assert!(source.contains("setting drive 1.75"));
    assert!(source.contains("bypass true"));
    assert!(
        source.find("processor master_limiter").unwrap()
            < source.find("processor master_saturation").unwrap()
    );

    let inspected = invoke(&root, &["inspect"]);
    assert!(inspected.status.success());
    let inspected = response(&inspected);
    let saturation = inspected["data"]["mix_graph"]["processors"]
        .as_array()
        .expect("processors")
        .iter()
        .find(|processor| processor["id"] == "master_saturation")
        .expect("master saturation");
    assert_eq!(saturation["authored_bypass"], true);
    assert_eq!(saturation["state_policy"], "continue");
}

#[test]
fn audition_state_isolated_while_durable_processor_choices_change_history_and_render() {
    let root = fixture_named("production_mix.crump");
    let published = publish(&root);
    let baseline_revision = published["data"]["revision"]
        .as_str()
        .expect("published revision")
        .to_owned();
    let source_path = root.path().join("project.crump");
    let baseline_source = fs::read(&source_path).expect("baseline source");
    let baseline_history = invoke(&root, &["history", "list"]);
    assert!(baseline_history.status.success());
    let baseline_history = response(&baseline_history);

    let baseline_render = invoke(&root, &["render", "--output", "baseline-master"]);
    assert!(
        baseline_render.status.success(),
        "{}",
        String::from_utf8_lossy(&baseline_render.stdout)
    );
    let baseline_wav =
        fs::read(root.path().join("baseline-master/master.wav")).expect("baseline Master");

    let audition = invoke(
        &root,
        &[
            "play",
            "--target",
            "track/drums",
            "--path",
            "channel-post",
            "--from",
            "intro",
            "--to",
            "2bar",
            "--loop-range",
            "--bypass",
            "drum_latency",
            "--plan",
        ],
    );
    assert!(
        audition.status.success(),
        "{}",
        String::from_utf8_lossy(&audition.stdout)
    );
    let audition = response(&audition);
    assert_eq!(audition["data"]["audition"]["status"]["looping"], true);
    assert_eq!(
        audition["data"]["audition"]["status"]["active_revision"],
        baseline_revision
    );
    assert_eq!(
        fs::read(&source_path).expect("source after transient audition"),
        baseline_source
    );

    let status_after_audition = invoke(&root, &["status"]);
    assert!(status_after_audition.status.success());
    assert_eq!(
        response(&status_after_audition)["data"]["active_revision"],
        baseline_revision
    );
    let history_after_audition = invoke(&root, &["history", "list"]);
    assert!(history_after_audition.status.success());
    assert_eq!(response(&history_after_audition), baseline_history);

    let transient_render = invoke(&root, &["render", "--output", "transient-master"]);
    assert!(transient_render.status.success());
    assert_eq!(
        fs::read(root.path().join("transient-master/master.wav")).expect("post-audition Master"),
        baseline_wav,
        "monitoring-only target/path/range/loop/bypass state must not enter offline render"
    );

    for arguments in [
        vec![
            "edit",
            "processor-setting",
            "master_saturation",
            "drive",
            "2.0",
        ],
        vec!["edit", "processor-move", "master_limiter", "earlier"],
        vec!["edit", "processor-bypass", "master_saturation", "true"],
    ] {
        let edited = invoke(&root, &arguments);
        assert!(
            edited.status.success(),
            "{}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&edited.stdout)
        );
    }

    let durable_source = fs::read_to_string(&source_path).expect("durably edited source");
    assert!(durable_source.contains("setting drive 2.0"));
    assert!(durable_source.contains("bypass true"));
    assert!(
        durable_source.find("processor master_limiter").unwrap()
            < durable_source.find("processor master_saturation").unwrap()
    );
    let durable_status = invoke(&root, &["status"]);
    assert!(durable_status.status.success());
    let durable_revision = response(&durable_status)["data"]["active_revision"]
        .as_str()
        .expect("durable Active revision")
        .to_owned();
    assert_ne!(durable_revision, baseline_revision);
    let durable_history = invoke(&root, &["history", "list"]);
    assert!(durable_history.status.success());
    assert_ne!(response(&durable_history), baseline_history);

    let durable_render = invoke(&root, &["render", "--output", "durable-master"]);
    assert!(durable_render.status.success());
    assert_ne!(
        fs::read(root.path().join("durable-master/master.wav")).expect("durable Master"),
        baseline_wav,
        "committed bypass/order/setting choices must change the authoritative render"
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
