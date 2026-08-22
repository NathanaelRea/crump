use std::fs;

use tempfile::tempdir;

use crate::{
    history::{ChangeOrigin, Publication},
    project::PROJECT_FILE,
};

use super::*;

fn write_test_wav(path: &Path, samples: &[i16]) -> Vec<u8> {
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
    for sample in samples {
        writer.write_sample(*sample).expect("sample");
    }
    writer.finalize().expect("finalize");
    fs::read(path).expect("WAV bytes")
}

const SOURCE: &str = r#"crump 1
project "Edit" {
  id edit
  seed 1
  sample_rate 48000
  render_tail 1s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument drums "Drums" stock_drums {
      polyphony 8
      parameter tone "Tone" ratio 0 1 0.5 0.5
      faceplate {
        control tone "Tone" "Brightness" maps tone
      }
    }
    pattern beat 1bar {
      hit 0beat kick 1
    }
    track drums-track "Drums" instrument drums level -6db pan 0 {
      clip beat-clip pattern beat start 0bar duration 1bar mode once
    }
  }
}
"#;

fn sampler_source(sha256: &str) -> String {
    format!(
        r#"crump 1
project "Sampler Edits" {{
  id sampler-edits
  seed 9
  sample_rate 48000
  render_tail 10ms
  asset source "Source" wav "takes/raw.wav" sha256 "{sha256}"
  song {{
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument sampler "Sampler" sampler {{
      polyphony 4
      parameter gain "Gain" db -96db 24db 0db 0db
      faceplate {{
        control gain "Gain" "Sampler level" maps gain
      }}
      mapping zone asset source keys C4 C4 root C4 {{
        region 0 512
        playback gated
        gain 0db
        pan 0
        envelope 0s 10ms
        fade 4 4
      }}
    }}
    pattern phrase 1bar {{
      sample original 0beat mapping zone duration 1beat start 0 end 256 gain 0db pan 0 fade 4 4 velocity 1
    }}
    track samples "Samples" instrument sampler level 0db pan 0 {{
      clip phrase pattern phrase start 0bar duration 1bar mode once
    }}
  }}
}}
"#
    )
}

#[test]
fn sampler_operations_and_collection_are_checked_atomic_and_renderable() {
    let directory = tempdir().expect("Project root");
    fs::create_dir_all(directory.path().join("takes")).expect("takes directory");
    let bytes = write_test_wav(
        &directory.path().join("takes/raw.wav"),
        &(0..512)
            .map(|index| {
                if (index / 16) % 2 == 0 {
                    12_000
                } else {
                    -12_000
                }
            })
            .collect::<Vec<_>>(),
    );
    let sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fs::write(directory.path().join(PROJECT_FILE), sampler_source(&sha256))
        .expect("Project source");

    let mut bundle = load_project(directory.path()).expect("load sampler edits");
    collect_media_assets(
        directory.path(),
        &bundle.revision,
        Publication::new(ChangeOrigin::human("tester"), "Collect", "normalize media"),
    )
    .expect("collect");
    assert_eq!(
        fs::read(directory.path().join("media/source.wav")).expect("collected bytes"),
        bytes
    );
    assert!(!directory.path().join("takes/raw.wav").exists());

    let apply = |bundle: &ProjectBundle, replacements: Vec<CheckedReplacement>, summary: &str| {
        apply_structured_edit(
            &bundle.root,
            &bundle.revision,
            replacements,
            Publication::new(ChangeOrigin::human("tester"), summary, "sampler edit"),
        )
        .expect("apply sampler edit");
    };

    bundle = load_project(directory.path()).expect("reload after collect");
    apply(
        &bundle,
        trim_sample_occurrence(&bundle, "phrase", "original", 8, 248).expect("trim"),
        "Trim",
    );
    bundle = load_project(directory.path()).expect("reload after trim");
    apply(
        &bundle,
        slip_sample_occurrence(&bundle, "phrase", "original", 8).expect("slip"),
        "Slip",
    );
    bundle = load_project(directory.path()).expect("reload after slip");
    apply(
        &bundle,
        fade_sample_occurrence(&bundle, "phrase", "original", 6, 10).expect("fade"),
        "Fade",
    );
    bundle = load_project(directory.path()).expect("reload after fade");
    apply(
        &bundle,
        vec![set_sample_occurrence_gain(&bundle, "phrase", "original", -3.0).expect("gain")],
        "Gain",
    );
    bundle = load_project(directory.path()).expect("reload after gain");
    apply(
        &bundle,
        vec![set_sample_occurrence_pan(&bundle, "phrase", "original", 0.25).expect("pan")],
        "Pan",
    );
    bundle = load_project(directory.path()).expect("reload after pan");
    apply(
        &bundle,
        vec![
            set_sample_occurrence_direction(
                &bundle,
                "phrase",
                "original",
                crate::model::SampleDirection::Reverse,
            )
            .expect("direction"),
        ],
        "Reverse",
    );
    bundle = load_project(directory.path()).expect("reload after direction");
    apply(
        &bundle,
        vec![
            set_sample_occurrence_loop(
                &bundle,
                "phrase",
                "original",
                crate::model::SampleLoopRegion {
                    start_frame: 32,
                    end_frame: 96,
                    crossfade_frames: 8,
                },
            )
            .expect("loop"),
        ],
        "Loop",
    );
    bundle = load_project(directory.path()).expect("reload after loop");
    apply(
        &bundle,
        vec![
            place_sample_occurrence(
                &bundle,
                "phrase",
                &SamplePlacement {
                    id: "placed".to_owned(),
                    mapping: "zone".to_owned(),
                    at: "1beat".to_owned(),
                    duration: "1beat".to_owned(),
                    start_frame: 32,
                    end_frame: 300,
                    gain_db: -2.0,
                    pan: -0.2,
                    fade_in_frames: 5,
                    fade_out_frames: 7,
                    velocity: 0.8,
                    direction: None,
                    loop_region: None,
                },
            )
            .expect("place"),
        ],
        "Place",
    );
    bundle = load_project(directory.path()).expect("reload after place");
    apply(
        &bundle,
        vec![
            duplicate_sample_occurrence(&bundle, "phrase", "original", "copy", "2beat")
                .expect("duplicate"),
        ],
        "Duplicate",
    );
    bundle = load_project(directory.path()).expect("reload after duplicate");
    apply(
        &bundle,
        vec![
            split_sample_occurrence(&bundle, "phrase", "copy", "copy-tail", "5/2beat", 100)
                .expect("split"),
        ],
        "Split",
    );
    bundle = load_project(directory.path()).expect("reload after split");
    apply(
        &bundle,
        vec![move_sample_occurrence(&bundle, "phrase", "copy-tail", "3beat").expect("move")],
        "Move",
    );
    bundle = load_project(directory.path()).expect("reload after move");
    apply(
        &bundle,
        vec![
            create_sample_slice_map(
                &bundle,
                &SampleSliceMapPlacement {
                    instrument: "sampler".to_owned(),
                    asset: "source".to_owned(),
                    id_prefix: "slice".to_owned(),
                    start_key: crate::model::parse_pitch("C5").expect("pitch"),
                    boundaries: vec![0, 128, 256],
                    direction: crate::model::SampleDirection::Reverse,
                },
            )
            .expect("slice map"),
        ],
        "Slice map",
    );

    let final_bundle = load_project(directory.path()).expect("final sampler Project");
    assert!(!final_bundle.diagnostics.has_errors());
    let source = fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source");
    for occurrence in ["original", "placed", "copy", "copy-tail"] {
        assert!(source.contains(&format!("sample {occurrence} ")));
    }
    assert!(source.contains("asset source \"Source\" wav \"media/source.wav\""));
    assert!(source.contains("direction reverse loop 32 96 crossfade 8"));
    assert!(source.contains("mapping slice-1 asset source keys C5 C5 root C5"));
    assert!(source.contains("mapping slice-2 asset source keys C#5 C#5 root C#5"));
    let project = final_bundle
        .publishable()
        .expect("publishable final sampler Project");
    let compiled =
        crate::engine::CompiledProject::compile_with_assets(project, &final_bundle.assets)
            .expect("compile edited sampler");
    let mut runtime = compiled.runtime();
    let rendered = (0..compiled.total_frames())
        .map(|_| {
            runtime
                .process_frame(crate::engine::ProcessingMode::Offline)
                .expect("finite edited sampler")
        })
        .collect::<Vec<_>>();
    assert!(rendered.iter().flatten().any(|sample| sample.abs() > 0.01));
}

#[test]
fn media_import_copies_declares_validates_and_publishes_atomically() {
    let directory = tempdir().expect("Project root");
    fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
    let external = tempdir().expect("external media");
    let input = external.path().join("voice.wav");
    let bytes = write_test_wav(&input, &[0, 100, -100]);
    let before = load_project(directory.path()).expect("Candidate");
    import_media_asset(
        directory.path(),
        &before.revision,
        &input,
        "voice",
        "Voice \"fragment\"",
        Publication::new(
            ChangeOrigin::human("tester"),
            "Import voice",
            "portable copy",
        ),
    )
    .expect("import");

    assert_eq!(
        fs::read(directory.path().join("media/voice.wav")).expect("copied asset"),
        bytes
    );
    let source = fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source");
    assert!(
        source.contains("asset voice \"Voice \\\"fragment\\\"\" wav \"media/voice.wav\" sha256 \"")
    );
    let after = load_project(directory.path()).expect("reloaded Project");
    assert!(!after.diagnostics.has_errors());
    assert_eq!(after.assets.get("voice").expect("asset").bytes(), bytes);
    let store = HistoryStore::open(directory.path()).expect("History");
    assert_eq!(
        store
            .active_state()
            .expect("state")
            .expect("Active")
            .active_revision,
        after.revision
    );
}

#[test]
fn invalid_media_import_leaves_source_and_destination_untouched() {
    let directory = tempdir().expect("Project root");
    fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
    let invalid = directory.path().join("not-a-wave.bin");
    fs::write(&invalid, b"not a wave").expect("invalid input");
    let before = load_project(directory.path()).expect("Candidate");
    let source_before = before
        .source(Path::new(PROJECT_FILE))
        .expect("source")
        .to_owned();
    assert!(matches!(
        import_media_asset(
            directory.path(),
            &before.revision,
            &invalid,
            "broken",
            "Broken",
            Publication::new(ChangeOrigin::human("tester"), "Import", "invalid")
        ),
        Err(EditError::InvalidResult(_))
    ));
    assert_eq!(
        fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source"),
        source_before
    );
    assert!(!directory.path().join("media/broken.wav").exists());
}

#[test]
fn media_import_never_replaces_an_undeclared_destination() {
    let directory = tempdir().expect("Project root");
    fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
    fs::create_dir_all(directory.path().join("media")).expect("media directory");
    fs::write(directory.path().join("media/voice.wav"), b"keep me").expect("destination");
    let external = tempdir().expect("external media");
    let input = external.path().join("voice.wav");
    write_test_wav(&input, &[0, 100]);
    let before = load_project(directory.path()).expect("Candidate");

    assert!(matches!(
        import_media_asset(
            directory.path(),
            &before.revision,
            &input,
            "voice",
            "Voice",
            Publication::new(ChangeOrigin::human("tester"), "Import", "no replace")
        ),
        Err(EditError::AssetDestinationExists(path)) if path == Path::new("media/voice.wav")
    ));
    assert_eq!(
        fs::read(directory.path().join("media/voice.wav")).expect("destination"),
        b"keep me"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source"),
        SOURCE
    );
}

#[test]
fn undeclared_binary_mutations_cannot_create_or_delete_project_files() {
    let directory = tempdir().expect("Project root");
    fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
    fs::create_dir_all(directory.path().join("media")).expect("media directory");
    let existing = PathBuf::from("media/notes.wav");
    fs::write(directory.path().join(&existing), b"keep me").expect("existing file");
    let before = load_project(directory.path()).expect("Candidate");

    assert!(matches!(
        apply_project_mutation(
            directory.path(),
            &before.revision,
            ProjectMutation {
                replacements: Vec::new(),
                asset_writes: Vec::new(),
                asset_removals: vec![existing.clone()],
            },
            Publication::new(ChangeOrigin::human("tester"), "Remove", "undeclared")
        ),
        Err(EditError::UndeclaredAssetPath(path)) if path == existing
    ));
    assert_eq!(
        fs::read(directory.path().join(&existing)).expect("preserved file"),
        b"keep me"
    );
    assert!(matches!(
        apply_project_mutation(
            directory.path(),
            &before.revision,
            ProjectMutation {
                replacements: Vec::new(),
                asset_writes: vec![StagedAssetWrite {
                    path: existing.clone(),
                    bytes: b"replacement".to_vec(),
                    replace: true,
                }],
                asset_removals: Vec::new(),
            },
            Publication::new(ChangeOrigin::human("tester"), "Replace", "undeclared")
        ),
        Err(EditError::UndeclaredAssetPath(path)) if path == existing
    ));
    assert_eq!(
        fs::read(directory.path().join(&existing)).expect("preserved file"),
        b"keep me"
    );

    let created = PathBuf::from("media/stray.wav");
    assert!(matches!(
        apply_project_mutation(
            directory.path(),
            &before.revision,
            ProjectMutation {
                replacements: Vec::new(),
                asset_writes: vec![StagedAssetWrite {
                    path: created.clone(),
                    bytes: b"stray".to_vec(),
                    replace: false,
                }],
                asset_removals: Vec::new(),
            },
            Publication::new(ChangeOrigin::human("tester"), "Write", "undeclared")
        ),
        Err(EditError::UndeclaredAssetPath(path)) if path == created
    ));
    assert!(!directory.path().join(created).exists());
    assert_eq!(
        fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source"),
        SOURCE
    );
    assert!(
        HistoryStore::open(directory.path())
            .expect("History")
            .list()
            .expect("entries")
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn media_import_rejects_symlink_inputs_and_project_parents() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("Project root");
    fs::write(directory.path().join(PROJECT_FILE), SOURCE).expect("Project source");
    let external = tempdir().expect("external media");
    let input = external.path().join("voice.wav");
    write_test_wav(&input, &[0, 100]);
    let input_link = external.path().join("voice-link.wav");
    symlink(&input, &input_link).expect("input symlink");
    let before = load_project(directory.path()).expect("Candidate");
    assert!(matches!(
        import_media_asset(
            directory.path(),
            &before.revision,
            &input_link,
            "linked",
            "Linked",
            Publication::new(ChangeOrigin::human("tester"), "Import", "reject link")
        ),
        Err(EditError::InvalidValue(_))
    ));

    let outside = tempdir().expect("outside destination");
    symlink(outside.path(), directory.path().join("media")).expect("media parent symlink");
    assert!(matches!(
        import_media_asset(
            directory.path(),
            &before.revision,
            &input,
            "voice",
            "Voice",
            Publication::new(ChangeOrigin::human("tester"), "Import", "confined")
        ),
        Err(EditError::History(HistoryError::UnsupportedRestore(_)))
    ));
    assert!(!outside.path().join("voice.wav").exists());
    assert_eq!(
        fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source"),
        SOURCE
    );
}

#[test]
fn arrangement_time_operations_shift_delete_and_duplicate_atomically() {
    let directory = tempdir().expect("Project root");
    let source =
        include_str!("../../tests/fixtures/minimal.crump").replacen("end 1bar", "end 4bar", 1);
    fs::write(directory.path().join(PROJECT_FILE), source).expect("Project source");
    let publication = |summary: &str| {
        Publication::new(ChangeOrigin::human("tester"), summary, "Arrangement edit")
    };

    let mut bundle = load_project(directory.path()).expect("initial Project");
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![create_locator(&bundle, "marker", "Marker", "2bar").expect("Locator")],
        publication("Create Locator"),
    )
    .expect("apply Locator");

    bundle = load_project(directory.path()).expect("Project with Locator");
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        duplicate_arrangement_time(
            &bundle,
            "0bar",
            "1bar",
            "1bar",
            "copy",
            ArrangementAutomation::FollowArrangement,
        )
        .expect("duplicate arrangement time"),
        publication("Duplicate arrangement time"),
    )
    .expect("apply duplicate time");

    bundle = load_project(directory.path()).expect("duplicated Project");
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        insert_arrangement_time(
            &bundle,
            "2bar",
            "1bar",
            ArrangementAutomation::FollowArrangement,
        )
        .expect("insert arrangement time"),
        publication("Insert arrangement time"),
    )
    .expect("apply insert time");

    bundle = load_project(directory.path()).expect("inserted Project");
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        delete_arrangement_time(
            &bundle,
            "1bar",
            "1bar",
            ArrangementAutomation::FollowArrangement,
        )
        .expect("delete arrangement time"),
        publication("Delete arrangement time"),
    )
    .expect("apply delete time");

    let final_bundle = load_project(directory.path()).expect("final Project");
    assert!(!final_bundle.diagnostics.has_errors());
    let source = fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source");
    assert!(source.contains("end 20qn"));
    assert!(source.contains("locator marker \"Marker\" at 12qn"));
    assert!(!source.contains("clip melody-copy "));
    assert!(!source.contains("clip beat-copy "));
    let project = final_bundle.publishable().expect("publishable arrangement");
    crate::engine::CompiledProject::compile(project).expect("compile arrangement");
}

#[test]
fn arrangement_time_automation_uses_half_open_follow_and_explicit_lock_policies() {
    let source = include_str!("../../tests/fixtures/minimal.crump")
        .replacen("end 1bar", "end 4bar", 1)
        .replacen("3000hz 3200hz", "3000hz 3200hz automatable", 1)
        .replacen(
            "    track lead \"Lead\"",
            r#"    automation arrangement-tone target instrument/lead/tone {
      point start 0qn 3000hz step
      point at-left 4qn 3000hz step
      point inside 6qn 3000hz step
      point at-right 8qn 3000hz step
      point later 12qn 3000hz step
      point terminal 16qn 3000hz step
    }
    track lead "Lead""#,
            1,
        );
    let publication = |summary: &str| {
        Publication::new(ChangeOrigin::human("tester"), summary, "Automation policy")
    };

    let follow_directory = tempdir().expect("follow Project root");
    fs::write(follow_directory.path().join(PROJECT_FILE), &source).expect("follow source");
    let bundle = load_project(follow_directory.path()).expect("follow Project");
    assert!(matches!(
        delete_arrangement_time(
            &bundle,
            "1bar",
            "1bar",
            ArrangementAutomation::LockToSong,
        ),
        Err(EditError::InvalidValue(message))
            if message.contains("locked Automation would extend beyond")
    ));
    apply_structured_edit(
        follow_directory.path(),
        &bundle.revision,
        delete_arrangement_time(
            &bundle,
            "1bar",
            "1bar",
            ArrangementAutomation::FollowArrangement,
        )
        .expect("follow deletion"),
        publication("Follow arrangement"),
    )
    .expect("apply follow deletion");
    let followed =
        fs::read_to_string(follow_directory.path().join(PROJECT_FILE)).expect("followed source");
    assert!(!followed.contains("point at-left "));
    assert!(!followed.contains("point inside "));
    assert!(followed.contains("point at-right 4qn "));
    assert!(followed.contains("point later 8qn "));
    assert!(followed.contains("point terminal 12qn "));
    let followed = load_project(follow_directory.path()).expect("followed Project");
    assert!(!followed.diagnostics.has_errors());

    let locked_source = source.replace("      point terminal 16qn 3000hz step\n", "");
    let locked_directory = tempdir().expect("locked Project root");
    fs::write(locked_directory.path().join(PROJECT_FILE), locked_source).expect("locked source");
    let bundle = load_project(locked_directory.path()).expect("locked Project");
    apply_structured_edit(
        locked_directory.path(),
        &bundle.revision,
        delete_arrangement_time(&bundle, "1bar", "1bar", ArrangementAutomation::LockToSong)
            .expect("locked deletion"),
        publication("Lock to Song"),
    )
    .expect("apply locked deletion");
    let locked =
        fs::read_to_string(locked_directory.path().join(PROJECT_FILE)).expect("locked source");
    assert!(locked.contains("point at-left 4qn "));
    assert!(locked.contains("point inside 6qn "));
    assert!(locked.contains("point at-right 8qn "));
    assert!(locked.contains("point later 12qn "));
    let locked = load_project(locked_directory.path()).expect("locked Project after deletion");
    assert!(!locked.diagnostics.has_errors());
}

#[test]
fn clip_operations_are_checked_procedural_and_renderable() {
    let directory = tempdir().expect("Project root");
    fs::write(
        directory.path().join(PROJECT_FILE),
        include_str!("../../tests/fixtures/minimal.crump"),
    )
    .expect("Project source");
    let publication =
        |summary: &str| Publication::new(ChangeOrigin::human("tester"), summary, "Timeline edit");

    let mut bundle = load_project(directory.path()).expect("initial Project");
    let placement = ClipPlacement {
        id: "alternate".to_owned(),
        pattern: "melody".to_owned(),
        start: "0bar".to_owned(),
        duration: "1bar".to_owned(),
        mode: ClipMode::Loop,
        source_offset: "1beat".to_owned(),
        transpose_semitones: 12,
        gain_db: -6.0,
        fade_in: "1/2beat".to_owned(),
        fade_out: "1/2beat".to_owned(),
        groove: None,
    };
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![create_clip(&bundle, "lead", &placement).expect("create Clip")],
        publication("Create Clip"),
    )
    .expect("apply create");

    bundle = load_project(directory.path()).expect("created Project");
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![
            duplicate_clip(&bundle, "alternate", "alternate-copy", "0bar").expect("duplicate Clip"),
        ],
        publication("Duplicate Clip"),
    )
    .expect("apply duplicate");

    bundle = load_project(directory.path()).expect("duplicated Project");
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        split_clip(&bundle, "alternate-copy", "alternate-tail", "2beat").expect("split Clip"),
        publication("Split Clip"),
    )
    .expect("apply split");

    bundle = load_project(directory.path()).expect("split Project");
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![
            move_clip(&bundle, "alternate-tail", "1beat").expect("move Clip"),
            resize_clip(&bundle, "alternate-copy", "1beat").expect("resize Clip"),
        ],
        publication("Move and resize Clips"),
    )
    .expect("apply move/resize");

    bundle = load_project(directory.path()).expect("moved Project");
    let mut replacements =
        set_clip_source_offset(&bundle, "alternate", "0beat").expect("set Clip offset");
    replacements.extend(set_clip_transpose(&bundle, "alternate", 7).expect("transpose Clip"));
    replacements.extend(set_clip_gain(&bundle, "alternate", -3.0).expect("gain Clip"));
    replacements.extend(set_clip_fades(&bundle, "alternate", "1beat", "0beat").expect("fade Clip"));
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        replacements,
        publication("Transform Clip"),
    )
    .expect("apply transforms");

    bundle = load_project(directory.path()).expect("transformed Project");
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![delete_clip(&bundle, "alternate-tail").expect("delete Clip")],
        publication("Delete Clip"),
    )
    .expect("apply delete");

    let final_bundle = load_project(directory.path()).expect("final Project");
    assert!(!final_bundle.diagnostics.has_errors());
    let source = fs::read_to_string(directory.path().join(PROJECT_FILE)).expect("source");
    assert!(source.contains("clip alternate pattern melody start 0qn duration 4qn mode loop"));
    assert!(source.contains("offset 0qn"));
    assert!(source.contains("transpose 7st"));
    assert!(source.contains("gain -3db"));
    assert!(source.contains("fade 1qn 0qn"));
    assert!(source.contains("clip alternate-copy pattern melody start 0qn duration 1qn mode loop"));
    assert!(!source.contains("clip alternate-tail "));
    let project = final_bundle
        .publishable()
        .expect("publishable Clip Project");
    let compiled = crate::engine::CompiledProject::compile(project).expect("compile Clips");
    let mut runtime = compiled.runtime();
    let sample = runtime
        .process_frame(crate::engine::ProcessingMode::Offline)
        .expect("finite Clip audio");
    assert!(sample.into_iter().all(f32::is_finite));
}

#[test]
fn event_grid_edits_change_only_selected_literals() {
    let directory = tempdir().unwrap();
    let source = include_str!("../../tests/fixtures/minimal.crump");
    fs::write(directory.path().join("project.crump"), source).unwrap();
    let bundle = load_project(directory.path()).unwrap();
    let replacements = vec![
        set_event_at(&bundle, "melody", 0, "1/2beat").unwrap(),
        set_note_pitch(&bundle, "melody", 0, 62).unwrap(),
        set_note_duration(&bundle, "melody", 0, "1/2beat").unwrap(),
        set_event_velocity(&bundle, "melody", 0, 0.65).unwrap(),
    ];
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        replacements,
        Publication::new(
            ChangeOrigin::human("tester"),
            "Edit note",
            "Event Grid change",
        ),
    )
    .unwrap();
    let changed = fs::read_to_string(directory.path().join("project.crump")).unwrap();
    assert_eq!(
        changed,
        source.replacen("note 0beat C4 1beat 0.8", "note 1/2qn D4 1/2qn 0.65", 1)
    );
}

#[test]
fn automation_edits_preserve_readable_source_and_durable_point_identity() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("project.crump"), SOURCE).unwrap();
    let publication = || {
        Publication::new(
            ChangeOrigin::human("tester"),
            "Edit Automation",
            "Automation View change",
        )
    };

    let bundle = load_project(directory.path()).unwrap();
    let target: ParameterTarget = "track/drums-track/level".parse().unwrap();
    let replacement = create_automation_lane(&bundle, "drums-level", &target, "-12db").unwrap();
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![replacement],
        publication(),
    )
    .unwrap();

    let bundle = load_project(directory.path()).unwrap();
    let replacement = create_automation_point(
        &bundle,
        "drums-level",
        "lift",
        "3beat",
        "-6db",
        AutomationInterpolation::Linear,
    )
    .unwrap();
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![replacement],
        publication(),
    )
    .unwrap();

    let bundle = load_project(directory.path()).unwrap();
    let replacement = create_automation_point(
        &bundle,
        "drums-level",
        "middle",
        "2beat",
        "-9db",
        AutomationInterpolation::Step,
    )
    .unwrap();
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![replacement],
        publication(),
    )
    .unwrap();
    let inserted = fs::read_to_string(directory.path().join("project.crump")).unwrap();
    assert!(inserted.find("point middle").unwrap() < inserted.find("point lift").unwrap());

    let bundle = load_project(directory.path()).unwrap();
    let replacement = set_automation_point_at(&bundle, "drums-level", "lift", "1beat").unwrap();
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![replacement],
        publication(),
    )
    .unwrap();
    let moved = fs::read_to_string(directory.path().join("project.crump")).unwrap();
    assert!(moved.find("point lift").unwrap() < moved.find("point middle").unwrap());

    let bundle = load_project(directory.path()).unwrap();
    assert!(set_automation_point_at(&bundle, "drums-level", "middle", "1beat").is_err());
    assert_eq!(
        fs::read_to_string(directory.path().join("project.crump")).unwrap(),
        moved
    );

    let replacements = vec![
        set_automation_point_value(&bundle, "drums-level", "lift", "-3db").unwrap(),
        set_automation_point_interpolation(
            &bundle,
            "drums-level",
            "lift",
            AutomationInterpolation::Power(2.0),
        )
        .unwrap(),
    ];
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        replacements,
        publication(),
    )
    .unwrap();
    let changed = fs::read_to_string(directory.path().join("project.crump")).unwrap();
    assert!(changed.contains("automation drums-level target track/drums-track/level {"));
    assert!(changed.contains("point start 0qn -12db step"));
    assert!(changed.contains("point lift 1qn -3db power(2)"));
    assert!(changed.contains("point middle 2qn -9db step"));

    let bundle = load_project(directory.path()).unwrap();
    let replacement = delete_automation_point(&bundle, "drums-level", "lift").unwrap();
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![replacement],
        publication(),
    )
    .unwrap();
    let changed = fs::read_to_string(directory.path().join("project.crump")).unwrap();
    assert!(!changed.contains("point lift"));

    let bundle = load_project(directory.path()).unwrap();
    let replacement = delete_automation_lane(&bundle, "drums-level").unwrap();
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![replacement],
        publication(),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("project.crump")).unwrap(),
        SOURCE
    );
}

#[test]
fn stable_mix_targets_edit_exact_source_literals() {
    let directory = tempdir().unwrap();
    fs::write(
        directory.path().join("project.crump"),
        include_str!("../../tests/fixtures/production_mix.crump"),
    )
    .unwrap();
    let bundle = load_project(directory.path()).unwrap();
    let edits = [
        ("track/bass/left_pan", -0.75),
        ("track/drums/mute", 1.0),
        ("group/mix_bus/pan", 0.2),
        ("send/drum_room/level", -10.0),
        ("processor/bass_comp/threshold", -20.0),
        ("master/level", -2.0),
    ]
    .into_iter()
    .map(|(target, value)| {
        set_parameter_target(
            &bundle,
            &target.parse::<ParameterTarget>().expect("target"),
            value,
        )
        .expect("mix edit")
    })
    .collect();
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        edits,
        Publication::new(
            ChangeOrigin::human("tester"),
            "Edit production mix",
            "Stable target edits",
        ),
    )
    .unwrap();
    let changed = load_project(directory.path()).unwrap();
    let project = changed.project.expect("edited Project");
    for (target, value) in [
        ("track/bass/left_pan", -0.75),
        ("track/drums/mute", 1.0),
        ("group/mix_bus/pan", 0.2),
        ("send/drum_room/level", -10.0),
        ("processor/bass_comp/threshold", -20.0),
        ("master/level", -2.0),
    ] {
        let spec = project
            .parameter_spec(&target.parse().expect("target"))
            .expect("edited target");
        assert_eq!(spec.base, value, "{target}");
    }
}

#[test]
fn processor_edits_preserve_exact_blocks_comments_and_history() {
    let directory = tempdir().unwrap();
    let source = include_str!("../../tests/fixtures/production_mix.crump").replace(
        "      processor master_limiter limiter {",
        "      # mastering ceiling\n      processor master_limiter limiter {",
    );
    fs::write(directory.path().join("project.crump"), source).unwrap();
    let publication = |summary| {
        Publication::new(
            ChangeOrigin::human("tester"),
            summary,
            "Processing View change",
        )
    };

    let bundle = load_project(directory.path()).unwrap();
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![set_processor_setting(&bundle, "master_saturation", "drive", "1.75").unwrap()],
        publication("Set exact processor setting"),
    )
    .unwrap();

    let bundle = load_project(directory.path()).unwrap();
    let target = ParameterTarget::Processor {
        processor: "master_saturation".to_owned(),
        parameter: "bypass".to_owned(),
    };
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![set_parameter_target(&bundle, &target, 1.0).unwrap()],
        publication("Bypass processor"),
    )
    .unwrap();

    let bundle = load_project(directory.path()).unwrap();
    apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![move_processor(&bundle, "master_limiter", ProcessorMove::Earlier).unwrap()],
        publication("Reorder mastering processors"),
    )
    .unwrap();

    let changed = fs::read_to_string(directory.path().join("project.crump")).unwrap();
    assert!(changed.contains("setting drive 1.75"));
    assert!(changed.contains("bypass true"));
    let limiter = changed.find("processor master_limiter").unwrap();
    let comment = changed.find("# mastering ceiling").unwrap();
    let saturation = changed.find("processor master_saturation").unwrap();
    assert!(comment < limiter && limiter < saturation);

    let bundle = load_project(directory.path()).unwrap();
    let project = bundle.project.expect("edited Project");
    let master = project.song.master.expect("Master");
    assert_eq!(master.inserts[0].id, "master_limiter");
    assert_eq!(master.inserts[1].id, "master_saturation");
    assert!(master.inserts[1].bypass);
    assert_eq!(
        master.inserts[1]
            .settings
            .iter()
            .find(|setting| setting.key == "drive")
            .unwrap()
            .value,
        "1.75"
    );
    assert_eq!(
        HistoryStore::open(directory.path())
            .unwrap()
            .list()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn changes_only_the_selected_literal_and_publishes() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("project.crump"), SOURCE).unwrap();
    let bundle = load_project(directory.path()).unwrap();
    let replacement = set_parameter(&bundle, "drums", "tone", 0.75).unwrap();
    let entry = apply_structured_edit(
        directory.path(),
        &bundle.revision,
        vec![replacement],
        Publication::new(
            ChangeOrigin::human("tester"),
            "Brighten drums",
            "Faceplate change",
        ),
    )
    .unwrap();
    let changed = fs::read_to_string(directory.path().join("project.crump")).unwrap();
    assert_eq!(changed, SOURCE.replacen("0.5\n", "0.75\n", 1));
    assert_eq!(entry.changes.len(), 1);
}
