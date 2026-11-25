//! Acoustic profile, native graph, deterministic rendering, and callback contracts.
use crump::{
    acoustics::{Excitation, ExcitationKind, Exciter, ModalBank},
    engine::{CompiledProject, ProcessingMode},
    project::load_project_sources,
};
use std::{collections::BTreeMap, path::PathBuf};

const SOURCE: &str = include_str!("../fixtures/acoustic/project.crump");
const PROFILE: &str = include_str!("../fixtures/acoustic/wood-body.crump");
fn bundle(source: &str, profile: &str) -> crump::project::ProjectBundle {
    load_project_sources(
        PathBuf::from("/acoustic-test"),
        BTreeMap::from([
            (PathBuf::from("project.crump"), source.into()),
            (PathBuf::from("wood-body.crump"), profile.into()),
        ]),
    )
}
fn render(compiled: &CompiledProject, quantum: usize) -> Vec<[f32; 2]> {
    let mut runtime = compiled.runtime();
    let mut out = vec![[0.; 2]; 48000];
    for block in out.chunks_mut(quantum) {
        runtime
            .process_block(block, ProcessingMode::Offline)
            .unwrap();
    }
    out
}
#[test]
fn native_nodes_match_shared_dsp_and_callback_grouping() {
    let b = bundle(SOURCE, PROFILE);
    assert!(!b.diagnostics.has_errors(), "{:?}", b.diagnostics);
    let p = b.project.unwrap();
    let compiled = CompiledProject::compile(&p).unwrap();
    let actual = render(&compiled, 17);
    assert_eq!(actual, render(&compiled, 256));
    let mut exciter = Exciter::new(48000, 47191);
    exciter.trigger(
        Excitation {
            kind: ExcitationKind::Impact,
            contact_seconds: 0.0001,
            hardness: 0.35,
            rate_hz: 400.,
            duration_seconds: 0.1,
            attack_seconds: 0.0,
        },
        f64::from(0.8_f32),
    );
    let profile = &p.acoustic_profiles[0].body;
    let mut bank = ModalBank::new(3, 48000);
    bank.trigger(profile, 1., 1., 0.);
    let expected: Vec<f64> = (0..48000)
        .map(|_| (bank.process(exciter.process() as f32 as f64) * 0.5) as f32 as f64)
        .collect();
    let gain = actual
        .iter()
        .zip(&expected)
        .map(|(a, b)| f64::from(a[0]) * b)
        .sum::<f64>()
        / expected.iter().map(|x| x * x).sum::<f64>();
    assert!(gain > 0.01);
    let error = actual
        .iter()
        .zip(&expected)
        .map(|(a, b)| (f64::from(a[0]) - b * gain).powi(2))
        .sum::<f64>();
    let energy = actual.iter().map(|x| f64::from(x[0]).powi(2)).sum::<f64>();
    assert!(
        error / energy < 1e-7,
        "shared DSP mismatch: {error} / {energy}"
    );
    let mut runtime = compiled.runtime();
    let mut block = [[0.; 2]; 256];
    let alloc = allocation_counter::measure(|| {
        for _ in 0..200 {
            runtime
                .process_block(&mut block, ProcessingMode::Offline)
                .unwrap();
        }
    });
    assert_eq!(alloc.count_total, 0);
}
#[test]
fn filtered_acoustics_match_probe_primitives_without_callback_allocations() {
    for lowpass_cutoff in [None, Some(1100.0)] {
        let mut source = SOURCE.replace("graph voice 4", "graph voice 6")
        .replace("node stereo mono_to_stereo", "node lowcut-one filter mode highpass cutoff 750hz resonance 0 key_track 0\nnode lowcut-two filter mode highpass cutoff 750hz resonance 0 key_track 0\nnode stereo mono_to_stereo")
        .replace("bus body body.audio -> stereo.audio", "bus cut-one body.audio -> lowcut-one.audio\nbus cut-two lowcut-one.audio -> lowcut-two.audio\nbus body lowcut-two.audio -> stereo.audio");
        if lowpass_cutoff.is_some() {
            source=source.replace("graph voice 6", "graph voice 8")
            .replace("node stereo mono_to_stereo", "node highcut-one filter mode lowpass cutoff 1100hz resonance 0 key_track 0\nnode highcut-two filter mode lowpass cutoff 1100hz resonance 0 key_track 0\nnode stereo mono_to_stereo")
            .replace("bus body lowcut-two.audio -> stereo.audio", "bus highcut-one lowcut-two.audio -> highcut-one.audio\nbus highcut-two highcut-one.audio -> highcut-two.audio\nbus body highcut-two.audio -> stereo.audio");
        }
        let b = bundle(&source, PROFILE);
        assert!(!b.diagnostics.has_errors(), "{:?}", b.diagnostics);
        let p = b.project.unwrap();
        let compiled = CompiledProject::compile(&p).unwrap();
        let actual = render(&compiled, 17);
        assert_eq!(actual, render(&compiled, 256));
        let mut exciter = Exciter::new(48000, 47191);
        exciter.trigger(
            Excitation {
                kind: ExcitationKind::Impact,
                contact_seconds: 0.0001,
                hardness: 0.35,
                rate_hz: 400.,
                duration_seconds: 0.1,
                attack_seconds: 0.0,
            },
            f64::from(0.8_f32),
        );
        let profile = &p.acoustic_profiles[0].body;
        let mut bank = ModalBank::new(3, 48000);
        bank.trigger(profile, 1., 1., 0.);
        let mut filters = [crump::dsp::StateVariableFilter::default(); 2];
        let mut lowpass = [crump::dsp::StateVariableFilter::default(); 2];
        let expected: Vec<f64> = (0..48000)
            .map(|_| {
                let mut y = (bank.process(exciter.process() as f32 as f64) * 0.5) as f32;
                for filter in &mut filters {
                    y = filter.process(f64::from(y), 750., 0., 48000.).highpass as f32;
                }
                if let Some(cutoff) = lowpass_cutoff {
                    for filter in &mut lowpass {
                        y = filter.process(f64::from(y), cutoff, 0., 48000.).lowpass as f32;
                    }
                }
                f64::from(y)
            })
            .collect();
        let gain = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| f64::from(a[0]) * b)
            .sum::<f64>()
            / expected.iter().map(|x| x * x).sum::<f64>();
        assert!(gain > 0.01);
        let error = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| (f64::from(a[0]) - b * gain).powi(2))
            .sum::<f64>();
        let energy = actual.iter().map(|x| f64::from(x[0]).powi(2)).sum::<f64>();
        assert!(
            error / energy < 1e-7,
            "shared DSP mismatch: {error} / {energy}"
        );
        let mut runtime = compiled.runtime();
        let mut block = [[0.; 2]; 256];
        let alloc = allocation_counter::measure(|| {
            for _ in 0..200 {
                runtime
                    .process_block(&mut block, ProcessingMode::Offline)
                    .unwrap();
            }
        });
        assert_eq!(alloc.count_total, 0);
    }
}
#[test]
fn profile_changes_are_authoritative_and_invalid_profiles_fail_closed() {
    let a = bundle(SOURCE, PROFILE);
    let b = bundle(SOURCE, &PROFILE.replace("1329hz", "1430hz"));
    assert_ne!(a.revision, b.revision);
    for invalid in [
        PROFILE.replace("0.018s", "-1s"),
        PROFILE.replace("crump-profile 1", "crump-profile 2"),
        PROFILE.replace("0.8 after", "9 after"),
        PROFILE.replace("261.625565hz", "0hz"),
    ] {
        assert!(bundle(SOURCE, &invalid).diagnostics.has_errors());
    }
    assert!(
        bundle(
            &SOURCE.replace("profile wood-body", "profile missing"),
            PROFILE
        )
        .diagnostics
        .has_errors()
    );
    assert!(
        bundle(&SOURCE.replace("max_modes 3", "max_modes 2"), PROFILE)
            .diagnostics
            .has_errors()
    );
}

#[test]
fn bandpass_impact_probe_path_matches_native_filter_and_callback_grouping() {
    for (frequency, resonance) in [(1340.0, 0.97), (7900.0, 0.65)] {
        let source = SOURCE
            .replace(
                "node body modal_bank profile wood-body max_modes 3 key_track 0 decay_scale $decay brightness $brightness level 0.5",
                &format!("node body filter mode bandpass cutoff {frequency}hz resonance {resonance} key_track 0"),
            )
            .replace("bus ring note.gate -> body.gate", "")
            .replace("bus pitch note.pitch -> body.pitch", "")
            .replace("control decay \"Ring\" \"Scale the body's damping times\" maps decay", "")
            .replace("control brightness \"Brightness\" \"Tilt the body resonances\" maps brightness", "");
        let bundle = bundle(&source, PROFILE);
        assert!(!bundle.diagnostics.has_errors(), "{:?}", bundle.diagnostics);
        let compiled = CompiledProject::compile(&bundle.project.unwrap()).unwrap();
        let actual = render(&compiled, 17);
        assert_eq!(actual, render(&compiled, 256));
        let mut exciter = Exciter::new(48000, 47191);
        exciter.trigger(
            Excitation {
                kind: ExcitationKind::Impact,
                contact_seconds: 0.0001,
                hardness: 0.35,
                rate_hz: 400.0,
                duration_seconds: 0.1,
                attack_seconds: 0.0,
            },
            f64::from(0.8_f32),
        );
        let mut filter = crump::dsp::StateVariableFilter::default();
        let expected: Vec<f64> = (0..48000)
            .map(|_| {
                let force = exciter.process() as f32;
                f64::from(
                    filter
                        .process(f64::from(force), frequency, resonance, 48000.0)
                        .bandpass as f32,
                )
            })
            .collect();
        let gain = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| f64::from(a[0]) * b)
            .sum::<f64>()
            / expected.iter().map(|x| x * x).sum::<f64>();
        let error = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| (f64::from(a[0]) - gain * b).powi(2))
            .sum::<f64>();
        let energy = actual.iter().map(|a| f64::from(a[0]).powi(2)).sum::<f64>();
        assert!(gain > 0.01 && error / energy < 1e-7);
        let mut runtime = compiled.runtime();
        let mut block = [[0.0; 2]; 256];
        let allocations = allocation_counter::measure(|| {
            for _ in 0..200 {
                runtime
                    .process_block(&mut block, ProcessingMode::Offline)
                    .unwrap();
            }
        });
        assert_eq!(allocations.count_total, 0);
    }
}
#[test]
fn acoustic_controls_reject_wrong_units_and_excess_domains() {
    for invalid in [
        SOURCE.replace("contact $contact", "contact 500hz"),
        SOURCE.replace("contact $contact", "contact $contact attack 2s"),
        SOURCE.replace("contact $contact", "contact $contact attack 2hz"),
        SOURCE.replace("ratio 0 1 0.35", "ratio 0 2 0.35"),
        SOURCE.replace("max_modes 3", "max_modes 300"),
        SOURCE.replace("max_modes 3", "max_modes 3 rise 2s"),
        SOURCE.replace("max_modes 3", "max_modes 3 rise 2hz"),
    ] {
        assert!(bundle(&invalid, PROFILE).diagnostics.has_errors());
    }
}

#[test]
fn texture_and_body_rise_survive_quiet_onset_and_short_gate_without_allocations() {
    let source = SOURCE
        .replace("kind impact", "kind texture attack 4ms duration 30ms")
        .replace("max_modes 3", "max_modes 3 rise 4ms")
        .replace("C4 1/4beat", "C4 1/128beat");
    let b = bundle(&source, PROFILE);
    assert!(!b.diagnostics.has_errors(), "{:?}", b.diagnostics);
    let compiled = CompiledProject::compile(&b.project.unwrap()).unwrap();
    let actual = render(&compiled, 17);
    assert_eq!(actual, render(&compiled, 256));
    let energy = |lo: usize, hi: usize| {
        actual[lo..hi]
            .iter()
            .map(|x| f64::from(x[0]).powi(2))
            .sum::<f64>()
    };
    assert!(energy(480, 960) > energy(0, 96) * 10.0);
    assert!(energy(4800, 7200) > 0.0);
    assert!(energy(40000, 48000) < energy(480, 960) * 1e-8);
    let mut runtime = compiled.runtime();
    let mut out = [[0.; 2]; 256];
    let alloc = allocation_counter::measure(|| {
        for _ in 0..200 {
            runtime
                .process_block(&mut out, ProcessingMode::Offline)
                .unwrap();
        }
    });
    assert_eq!(alloc.count_total, 0);
}

#[test]
fn native_collision_attack_survives_quiet_onset_and_is_grouping_independent() {
    let b = bundle(
        &SOURCE.replace(
            "kind impact",
            "kind collisions attack 100ms rate 6000hz duration 120ms",
        ),
        PROFILE,
    );
    assert!(!b.diagnostics.has_errors(), "{:?}", b.diagnostics);
    let compiled = CompiledProject::compile(&b.project.unwrap()).unwrap();
    let actual = render(&compiled, 17);
    assert_eq!(actual, render(&compiled, 256));
    let energy = |lo: usize, hi: usize| {
        actual[lo..hi]
            .iter()
            .map(|f| f64::from(f[0]).powi(2))
            .sum::<f64>()
    };
    assert!(energy(0, 480) < energy(3360, 3840) * 0.1);
    assert!(energy(3360, 3840) > 0.0);
}

#[test]
fn acoustic_note_off_preserves_long_body_ring() {
    let b = bundle(
        &SOURCE.replace("end 1bar", "end 6bar"),
        &PROFILE.replace("0.018s", "2s").replace("0.04s", "3s"),
    );
    assert!(!b.diagnostics.has_errors(), "{:?}", b.diagnostics);
    let compiled = CompiledProject::compile(&b.project.unwrap()).unwrap();
    let mut runtime = compiled.runtime();
    let mut block = [[0.; 2]; 480];
    let mut late_energy = 0.;
    for i in 0..1000 {
        runtime
            .process_block(&mut block, ProcessingMode::Offline)
            .unwrap();
        if i > 900 {
            late_energy += block.iter().map(|x| f64::from(x[0]).powi(2)).sum::<f64>();
        }
    }
    assert!(
        late_energy > 1e-6,
        "modal ring was cut at the legacy eight-second voice limit"
    );
}

#[test]
fn exported_module_carries_profiles_and_previews_without_original_project() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("project.crump"), SOURCE).unwrap();
    std::fs::write(project.join("wood-body.crump"), PROFILE).unwrap();
    let data = temp.path().join("data");
    let output = assert_cmd::cargo::cargo_bin_cmd!("crump")
        .env("XDG_DATA_HOME", &data)
        .args([
            "--project",
            project.to_str().unwrap(),
            "--json",
            "module",
            "export",
            "wood",
            "--name",
            "acoustic-test",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let path = result["data"]["path"].as_str().unwrap();
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("profile wood-body")
    );
    std::fs::remove_dir_all(&project).unwrap();
    let wav = temp.path().join("preview.wav");
    let output = assert_cmd::cargo::cargo_bin_cmd!("crump")
        .env("XDG_DATA_HOME", &data)
        .current_dir(temp.path())
        .args([
            "--json",
            "module",
            "preview",
            "acoustic-test",
            "--output",
            wav.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(wav.exists());
}

fn acoustic_module(instrument_id: &str, instrument_name: &str, profile_block: &str) -> String {
    format!(
        r#"crump-module 2
instrument {instrument_id} "{instrument_name}" graph {{
  polyphony 2
  parameter hardness "Hardness" ratio 0 1 0.35 0.35
  parameter contact "Contact" seconds 0.00002s 0.003s 0.0001s 0.0001s
  parameter decay "Ring" ratio 0.1 4 1 1
  parameter brightness "Brightness" ratio -2 2 0 0
  faceplate {{
    control hardness "Hardness" "Concentrate the contact force" maps hardness
    control contact "Contact" "Duration of the strike" maps contact
    control decay "Ring" "Scale the body's damping times" maps decay
    control brightness "Brightness" "Tilt the body resonances" maps brightness
  }}
  graph voice 4 {{
    node contact exciter kind impact contact $contact hardness $hardness
    node body modal_bank profile wood-body max_modes 3 key_track 0 decay_scale $decay brightness $brightness level 0.5
    node stereo mono_to_stereo
    node out output
    bus hit note.gate -> contact.gate
    bus touch note.velocity -> contact.velocity
    bus ring note.gate -> body.gate
    bus pitch note.pitch -> body.pitch
    bus force contact.audio -> body.audio
    bus body body.audio -> stereo.audio
    bus output stereo.audio -> out.audio
  }}
}}
{profile_block}
"#
    )
}

const WOOD_PROFILE_BLOCK: &str = r#"profile wood-body {
  root 261.625565hz
  mode body frequency 1329hz decay 0.018s gain 0.8 after_decay 0.04s after_gain 0.08
  mode upper frequency 2870hz decay 0.009s gain 0.22 after_decay 0.018s after_gain 0.02
  mode contact frequency 5340hz decay 0.003s gain 0.08 after_decay 0.006s after_gain 0.01
}"#;

const TWO_MODULE_PROJECT: &str = r#"crump 2
import "instruments/wood-a.crump"
import "instruments/wood-b.crump"
project "Shared acoustic profile" {
  id shared-acoustic-profile
  seed 47191
  sample_rate 48000
  render_tail 1s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    pattern strike 1bar {
      note 0beat C4 1/4beat 0.8
    }
    track wood-a "Wood A" instrument wood-a level -6db pan 0 {
      clip strike-a pattern strike start 0bar duration 1bar mode once
    }
    track wood-b "Wood B" instrument wood-b level -6db pan 0 {
      clip strike-b pattern strike start 0bar duration 1bar mode once
    }
  }
}
"#;

#[test]
fn two_modules_embedding_identical_profile_dedupe_compile_and_render() {
    let module_a = acoustic_module("wood-a", "Wood A", WOOD_PROFILE_BLOCK);
    let module_b = acoustic_module("wood-b", "Wood B", WOOD_PROFILE_BLOCK);
    let bundle = load_project_sources(
        PathBuf::from("/shared-acoustic-test"),
        BTreeMap::from([
            (PathBuf::from("project.crump"), TWO_MODULE_PROJECT.into()),
            (PathBuf::from("instruments/wood-a.crump"), module_a.clone()),
            (PathBuf::from("instruments/wood-b.crump"), module_b.clone()),
        ]),
    );
    assert!(!bundle.diagnostics.has_errors(), "{:?}", bundle.diagnostics);
    let project = bundle.project.expect("deduplicated Project");
    assert_eq!(project.acoustic_profiles.len(), 1);
    assert_eq!(project.acoustic_profiles[0].id, "wood-body");
    assert_eq!(
        project.acoustic_profiles[0].location.file,
        PathBuf::from("instruments/wood-a.crump")
    );

    let compiled = CompiledProject::compile(&project).expect("compile shared-profile Project");
    let rendered = render(&compiled, 17);
    assert_eq!(rendered, render(&compiled, 256));
    let energy: f64 = rendered
        .iter()
        .map(|frame| f64::from(frame[0]).powi(2))
        .sum();
    assert!(
        energy > 1e-4,
        "expected audible render, got energy {energy}"
    );

    // Exporting one instrument from a deduplicated Project emits the profile once.
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join("instruments")).unwrap();
    std::fs::write(root.join("project.crump"), TWO_MODULE_PROJECT).unwrap();
    std::fs::write(root.join("instruments/wood-a.crump"), &module_a).unwrap();
    std::fs::write(root.join("instruments/wood-b.crump"), &module_b).unwrap();
    let data = temp.path().join("data");
    let output = assert_cmd::cargo::cargo_bin_cmd!("crump")
        .env("XDG_DATA_HOME", &data)
        .args([
            "--project",
            root.to_str().unwrap(),
            "--json",
            "module",
            "export",
            "wood-b",
            "--name",
            "wood-b-exported",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let exported_path = result["data"]["path"].as_str().unwrap();
    let exported_source = std::fs::read_to_string(exported_path).unwrap();
    assert_eq!(exported_source.matches("profile wood-body {").count(), 1);
}

#[test]
fn formatting_comment_and_mode_label_differences_still_dedupe() {
    // Reformat with comments, extra blank lines, renamed mode labels (as `module export`
    // emits), and equivalent numeric spellings that parse to the exact same floats.
    let reformatted_profile = r#"# Re-exported body with comments and different mode labels
profile wood-body {

  # Reference pitch
  root 261.625565hz

  mode m0   frequency 1.329khz  decay 0.018s  gain 0.80  after_decay 40ms   after_gain 0.080
  mode m1   frequency 2870.0hz  decay 0.009s  gain 0.22  after_decay 0.018s after_gain 0.02
  mode m2   frequency 5340hz    decay 0.003s  gain 0.08  after_decay 0.006s after_gain 0.01
}"#;
    let project_with_root_profile = format!(
        "crump 2\nimport \"wood-body.crump\"\n{}",
        TWO_MODULE_PROJECT
            .strip_prefix("crump 2\n")
            .expect("crump 2 header")
    );
    let bundle = load_project_sources(
        PathBuf::from("/reformatted-acoustic-test"),
        BTreeMap::from([
            (PathBuf::from("project.crump"), project_with_root_profile),
            (PathBuf::from("wood-body.crump"), PROFILE.into()),
            (
                PathBuf::from("instruments/wood-a.crump"),
                acoustic_module("wood-a", "Wood A", WOOD_PROFILE_BLOCK),
            ),
            (
                PathBuf::from("instruments/wood-b.crump"),
                acoustic_module("wood-b", "Wood B", reformatted_profile),
            ),
        ]),
    );
    assert!(!bundle.diagnostics.has_errors(), "{:?}", bundle.diagnostics);
    let project = bundle.project.expect("deduplicated Project");
    assert_eq!(project.acoustic_profiles.len(), 1);
    assert_eq!(
        project.acoustic_profiles[0].location.file,
        PathBuf::from("wood-body.crump")
    );
    let compiled = CompiledProject::compile(&project).expect("compile reformatted shared profile");
    let rendered = render(&compiled, 256);
    assert!(
        rendered
            .iter()
            .map(|frame| f64::from(frame[0]).powi(2))
            .sum::<f64>()
            > 1e-4
    );
}

#[test]
fn conflicting_acoustic_profile_bodies_report_crv210_with_both_locations() {
    let differing_profile = WOOD_PROFILE_BLOCK.replace("1329hz", "1430hz");
    let bundle = load_project_sources(
        PathBuf::from("/conflicting-acoustic-test"),
        BTreeMap::from([
            (PathBuf::from("project.crump"), TWO_MODULE_PROJECT.into()),
            (
                PathBuf::from("instruments/wood-a.crump"),
                acoustic_module("wood-a", "Wood A", WOOD_PROFILE_BLOCK),
            ),
            (
                PathBuf::from("instruments/wood-b.crump"),
                acoustic_module("wood-b", "Wood B", &differing_profile),
            ),
        ]),
    );
    assert!(bundle.diagnostics.has_errors());
    let conflict = bundle
        .diagnostics
        .as_slice()
        .iter()
        .find(|diagnostic| diagnostic.code == "CRV210")
        .expect("expected CRV210 conflict diagnostic");
    assert!(
        conflict.message.contains("wood-body"),
        "expected profile id in message: {}",
        conflict.message
    );
    assert!(
        conflict.message.contains("instruments/wood-a.crump:"),
        "expected first location in message: {}",
        conflict.message
    );
    assert!(
        conflict.message.contains("instruments/wood-b.crump:"),
        "expected second location in message: {}",
        conflict.message
    );
    assert_eq!(
        conflict
            .location
            .as_ref()
            .map(|location| location.file.as_path()),
        Some(std::path::Path::new("instruments/wood-b.crump"))
    );
    assert!(
        conflict
            .help
            .as_deref()
            .is_some_and(|help| help.contains("rename")),
        "expected rename suggestion in help: {:?}",
        conflict.help
    );

    // Reordered modes are semantically different and also fail with CRV210 naming both files.
    let reordered_profile = r#"profile wood-body {
  root 261.625565hz
  mode upper frequency 2870hz decay 0.009s gain 0.22 after_decay 0.018s after_gain 0.02
  mode body frequency 1329hz decay 0.018s gain 0.8 after_decay 0.04s after_gain 0.08
  mode contact frequency 5340hz decay 0.003s gain 0.08 after_decay 0.006s after_gain 0.01
}"#;
    let reordered = load_project_sources(
        PathBuf::from("/reordered-acoustic-test"),
        BTreeMap::from([
            (PathBuf::from("project.crump"), TWO_MODULE_PROJECT.into()),
            (
                PathBuf::from("instruments/wood-a.crump"),
                acoustic_module("wood-a", "Wood A", WOOD_PROFILE_BLOCK),
            ),
            (
                PathBuf::from("instruments/wood-b.crump"),
                acoustic_module("wood-b", "Wood B", reordered_profile),
            ),
        ]),
    );
    let reordered_conflict = reordered
        .diagnostics
        .as_slice()
        .iter()
        .find(|diagnostic| diagnostic.code == "CRV210")
        .expect("expected CRV210 for reordered modes");
    assert!(
        reordered_conflict
            .message
            .contains("instruments/wood-a.crump:")
            && reordered_conflict
                .message
                .contains("instruments/wood-b.crump:"),
        "expected both locations: {}",
        reordered_conflict.message
    );

    // Repeating an identical profile inside one single module file still fails validation.
    let duplicated_in_one_file = format!(
        "{}\n{WOOD_PROFILE_BLOCK}\n",
        acoustic_module("wood-a", "Wood A", WOOD_PROFILE_BLOCK)
    );
    let same_file = load_project_sources(
        PathBuf::from("/same-file-acoustic-test"),
        BTreeMap::from([
            (PathBuf::from("project.crump"), TWO_MODULE_PROJECT.into()),
            (
                PathBuf::from("instruments/wood-a.crump"),
                duplicated_in_one_file,
            ),
            (
                PathBuf::from("instruments/wood-b.crump"),
                acoustic_module("wood-b", "Wood B", WOOD_PROFILE_BLOCK),
            ),
        ]),
    );
    let duplicate = same_file
        .diagnostics
        .as_slice()
        .iter()
        .find(|diagnostic| diagnostic.code == "CRV210")
        .expect("expected CRV210 for same-file duplicate profile");
    assert!(
        duplicate.message.contains("instruments/wood-a.crump:"),
        "expected same-file location: {}",
        duplicate.message
    );
}
