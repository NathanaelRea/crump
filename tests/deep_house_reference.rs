//! Structural qualification for the portable Milestone 7 reference.
//!
//! These tests prove source-family placement, duration, portability, and runtime
//! admission. They deliberately do not claim listening quality or producer credibility.

use std::{collections::BTreeSet, fs, path::Path};

use crump::{
    dsl::parse_module,
    engine::CompiledProject,
    model::{ClipMode, PatternEvent},
    project::load_project,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/deep-house-reference")
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read JSON")).expect("parse JSON")
}

#[test]
fn deep_house_reference_is_portable_complete_track_content() {
    let root = fixture_root();
    let bundle = load_project(&root).expect("load deep-house reference");
    assert!(
        !bundle.diagnostics.has_errors(),
        "{:?}",
        bundle.diagnostics.as_slice()
    );
    let project = bundle.project.as_ref().expect("parsed Project");
    let duration_seconds = project.song.end.as_f64() * 60.0 / project.song.tempo_bpm;
    assert!((180.0..=300.0).contains(&duration_seconds));
    assert!((duration_seconds - 228.196_721_311_475_4).abs() < 1.0e-9);
    assert_eq!(bundle.assets.iter().count(), 6);
    assert_eq!(project.song.instruments.len(), 15);
    assert_eq!(project.song.tracks.len(), 10);

    let source = std::fs::read_to_string(root.join("project.crump")).expect("project source");
    for required in [
        "node sub analog_oscillator",
        "node source wavetable",
        "node texture sample_source",
        "node source multisample",
        "node source granular",
        "node source spectral",
        "sampler {",
        "mapping phrase asset vocal",
        "modulation pad-width-drift lfo global",
        "modulation bass-grit-wobble lfo instrument/dh-bass",
        "automation ostinato-cutoff",
        "sidechain music-pump-key",
        "modulation bass-touch lfo per_voice/dh-bass",
        "follower kick-motion",
        "sidechain kick-detector",
        "group music",
        "group drums",
        "return room",
        "return echo",
    ] {
        let found_in_project = source.contains(required);
        let found_in_import = bundle
            .sources
            .values()
            .any(|source| source.contains(required));
        assert!(found_in_project || found_in_import, "missing `{required}`");
    }

    let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)
        .expect("compile admitted complete-track fixture");
    let resources = &compiled.mix_graph_inspection().resources;
    assert!(resources.admitted);
    assert!(resources.graph_voice_state_bytes > 0);
    assert!(resources.processor_state_bytes > 0);

    let evidence = json(&root.join("render-evidence.json"));
    assert_eq!(evidence["project_revision"], bundle.revision.to_string());
    assert_eq!(evidence["hashes_identical"], true);
    assert_eq!(evidence["listening_qualification"], "pending");
    let runs = evidence["runs"].as_array().expect("render runs");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0]["master_sha256"], runs[1]["master_sha256"]);
    assert_eq!(runs[0]["executable_sha256"], runs[1]["executable_sha256"]);
    assert_eq!(runs[0]["stems"], runs[1]["stems"]);
}

#[test]
fn starter_library_has_exact_assets_readable_modules_and_authored_audition_phrases() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let starter = repository.join("fixtures/starter");
    let reference = fixture_root();
    let manifest = json(&starter.join("manifest.json"));
    let modules = manifest["modules"].as_array().expect("starter modules");
    assert_eq!(modules.len(), 15);

    let expected_ids = BTreeSet::from([
        "dh-bass",
        "dh-brass",
        "dh-evolving-texture",
        "dh-granular-texture",
        "dh-keys",
        "dh-kick",
        "dh-lead",
        "dh-octave-lead",
        "dh-open-hat",
        "dh-pad",
        "dh-percussion",
        "dh-pluck",
        "dh-spectral-transform",
        "dh-transition",
        "dh-vocal",
    ]);
    let mut actual_ids = BTreeSet::new();
    for entry in modules {
        let id = entry["id"].as_str().expect("module ID");
        let relative = entry["path"].as_str().expect("module path");
        let bytes = fs::read(starter.join(relative)).expect("starter module");
        assert_eq!(sha256(&bytes), entry["sha256"].as_str().unwrap());
        assert_eq!(
            bytes,
            fs::read(reference.join(relative)).expect("reference module copy"),
            "{id} diverged between the starter library and reference Project"
        );
        let source = std::str::from_utf8(&bytes).expect("UTF-8 module");
        let parsed = parse_module(Path::new(relative), source);
        assert!(
            !parsed.diagnostics.has_errors(),
            "{id}: {:?}",
            parsed.diagnostics
        );
        assert_eq!(parsed.source_version, Some(2));
        assert_eq!(parsed.instruments.len(), 1);
        let instrument = &parsed.instruments[0];
        assert_eq!(instrument.id, id);
        assert!(!instrument.presentation.tags.is_empty());
        let preview = instrument
            .presentation
            .preview
            .as_ref()
            .expect("bounded preview metadata");
        assert!(preview.low_velocity < preview.high_velocity);
        if id != "dh-percussion" {
            assert!(preview.low_pitch < preview.high_pitch);
        }
        assert!(actual_ids.insert(id));
    }
    assert_eq!(actual_ids, expected_ids);

    let assets = manifest["assets"].as_array().expect("starter assets");
    assert_eq!(assets.len(), 6);
    assert_eq!(
        assets,
        json(&reference.join("media/manifest.json"))["assets"]
            .as_array()
            .expect("reference assets")
    );
    for entry in assets {
        let relative = entry["path"].as_str().expect("asset path");
        let bytes = fs::read(starter.join(relative)).expect("starter asset");
        assert_eq!(sha256(&bytes), entry["sha256"].as_str().unwrap());
        assert_eq!(
            bytes,
            fs::read(reference.join(relative)).expect("reference asset copy")
        );
        assert_eq!(entry["license"], "MIT");
        assert!(
            entry["origin"]
                .as_str()
                .is_some_and(|origin| origin.contains("generate_assets.py"))
        );
    }

    let audition = &manifest["audition_project"];
    let audition_path = starter.join(audition["path"].as_str().expect("audition path"));
    let audition_bytes = fs::read(&audition_path).expect("audition Project");
    assert_eq!(
        sha256(&audition_bytes),
        audition["sha256"].as_str().unwrap()
    );
    let bundle = load_project(&audition_path).expect("load audition Project");
    assert!(!bundle.diagnostics.has_errors(), "{:?}", bundle.diagnostics);
    let project = bundle.publishable().expect("valid audition Project");
    assert_eq!(project.song.instruments.len(), 15);
    assert_eq!(project.song.patterns.len(), 15);
    assert_eq!(project.song.tracks.len(), 15);
    assert_eq!(bundle.assets.iter().count(), 6);

    let declared_patterns = audition["patterns"]
        .as_array()
        .expect("audition pattern IDs")
        .iter()
        .map(|id| id.as_str().expect("pattern ID"))
        .collect::<BTreeSet<_>>();
    assert_eq!(declared_patterns.len(), 15);
    for pattern in &project.song.patterns {
        assert!(declared_patterns.contains(pattern.id.as_str()));
        assert!(pattern.events.len() >= 3);
        let velocities = pattern
            .events
            .iter()
            .map(|event| match event {
                PatternEvent::Note { velocity, .. } | PatternEvent::Hit { velocity, .. } => {
                    *velocity
                }
                _ => panic!("Audition Phrase contains a non-audition Event"),
            })
            .collect::<Vec<_>>();
        let low = velocities.iter().copied().fold(f64::INFINITY, f64::min);
        let high = velocities.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(high - low >= 0.3, "{} velocity span", pattern.id);
    }
    for track in &project.song.tracks {
        assert_eq!(track.clips.len(), 1);
        assert_eq!(track.clips[0].mode, ClipMode::Loop);
        assert!(declared_patterns.contains(track.clips[0].pattern.as_str()));
    }
    CompiledProject::compile_with_assets(project, &bundle.assets)
        .expect("compile complete Audition Phrase Project");
}
