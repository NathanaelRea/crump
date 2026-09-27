//! End-to-end graph sample-source and multisample playback checks.

use std::{fs, path::Path};

use crump::{
    dsl::parse_project,
    engine::{CompiledProject, ProcessingMode},
    media::load_asset_inventory,
    model::{Project, SettingValue},
    validate::{validate_project, validate_project_assets},
};

fn fixture(name: &str) -> (Project, crump::media::AssetInventory) {
    let path = Path::new("fixtures/synth-alpha").join(name);
    let source = fs::read_to_string(&path).expect("read source fixture");
    let parsed = parse_project(&path, &source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let project = parsed.project.expect("Project");
    let diagnostics = validate_project(&project);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let (assets, diagnostics) =
        load_asset_inventory(Path::new("fixtures/synth-alpha"), &project.assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let diagnostics = validate_project_assets(&project, &assets);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    (project, assets)
}

fn render(compiled: &CompiledProject, frames: usize, quantum: usize) -> Vec<[f32; 2]> {
    let mut runtime = compiled.runtime();
    let mut output = Vec::with_capacity(frames);
    while output.len() < frames {
        let count = quantum.min(frames - output.len());
        let mut block = vec![[0.0; 2]; count];
        assert_eq!(
            runtime
                .process_block(&mut block, ProcessingMode::Offline)
                .expect("finite sample render"),
            count
        );
        output.extend(block);
    }
    output
}

fn assert_audible_finite(output: &[[f32; 2]]) {
    assert!(output.iter().flatten().all(|sample| sample.is_finite()));
    assert!(
        output
            .iter()
            .any(|frame| frame[0].abs().max(frame[1].abs()) > 1.0e-5)
    );
}

#[test]
fn sample_and_multisample_fixtures_are_deterministic_and_grouping_independent() {
    for fixture_name in ["minimal-sample-source.crump", "minimal-multisample.crump"] {
        let (project, assets) = fixture(fixture_name);
        let compiled = CompiledProject::compile_with_assets(&project, &assets)
            .expect("compile graph sample fixture");
        let rebuilt = CompiledProject::compile_with_assets(&project, &assets)
            .expect("rebuild graph sample fixture");
        assert!(compiled.mix_graph_inspection().resources.admitted);
        assert_eq!(
            compiled.sample_source_inspection(),
            rebuilt.sample_source_inspection()
        );
        assert!(!compiled.sample_source_inspection().is_empty());
        assert!(compiled.sample_source_inspection().iter().all(|source| {
            source.algorithm == "crump-sample-windowed-sinc-v1"
                && source.cache_identity.len() == 64
                && source.compiled_bytes > 0
        }));
        let at_64 = render(&compiled, 48_000, 64);
        let at_257 = render(&compiled, 48_000, 257);
        assert_eq!(at_64, at_257, "callback grouping changed {fixture_name}");
        assert_audible_finite(&at_64);
    }
}

#[test]
fn sample_direction_loop_and_rate_are_explicit_repitched_semantics() {
    let (project, assets) = fixture("minimal-sample-source.crump");
    let original = CompiledProject::compile_with_assets(&project, &assets).expect("original");

    let mut reverse = project.clone();
    {
        let source = reverse.song.instruments[0]
            .graph
            .as_mut()
            .expect("voice graph")
            .nodes
            .iter_mut()
            .find(|node| node.id == "source")
            .expect("sample source");
        source
            .settings
            .iter_mut()
            .find(|setting| setting.key == "direction")
            .expect("direction")
            .value = SettingValue::Text("reverse".to_owned());
        source
            .settings
            .iter_mut()
            .find(|setting| setting.key == "rate")
            .expect("rate")
            .value = SettingValue::Number(2.0);
    }
    let mut duplicate = reverse.clone();
    let source = duplicate.song.instruments[0]
        .graph
        .as_mut()
        .expect("voice graph")
        .nodes
        .iter_mut()
        .find(|node| node.id == "source")
        .expect("sample source");
    source.settings.push(crump::model::NodeSetting {
        key: "rate".to_owned(),
        value: SettingValue::Number(2.0),
        key_site: source.settings[0].key_site.clone(),
        value_site: source.settings[0].value_site.clone(),
    });
    let diagnostics = validate_project(&duplicate);
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|diagnostic| diagnostic.code == "CRV050"),
        "duplicate rate must fail closed"
    );
    let reverse = CompiledProject::compile_with_assets(&reverse, &assets).expect("reverse");
    let forward_output = render(&original, 12_000, 73);
    let reverse_output = render(&reverse, 12_000, 211);
    assert_ne!(forward_output, reverse_output);
    assert_audible_finite(&reverse_output);

    let mut looped = project.clone();
    let source = looped.song.instruments[0]
        .graph
        .as_mut()
        .expect("voice graph")
        .nodes
        .iter_mut()
        .find(|node| node.id == "source")
        .expect("sample source");
    source
        .settings
        .iter_mut()
        .find(|setting| setting.key == "loop_mode")
        .expect("loop mode")
        .value = SettingValue::Text("forward".to_owned());
    let looped = CompiledProject::compile_with_assets(&looped, &assets).expect("forward loop");
    let unlooped_output = render(&original, 72_000, 127);
    let looped_output = render(&looped, 72_000, 193);
    let late_peak = |output: &[[f32; 2]]| {
        output[60_000..70_000]
            .iter()
            .flatten()
            .map(|sample| sample.abs())
            .fold(0.0_f32, f32::max)
    };
    assert!(late_peak(&unlooped_output) < 1.0e-6);
    assert!(late_peak(&looped_output) > 1.0e-4);

    let mut ping_pong = project;
    let source = ping_pong.song.instruments[0]
        .graph
        .as_mut()
        .expect("voice graph")
        .nodes
        .iter_mut()
        .find(|node| node.id == "source")
        .expect("sample source");
    source
        .settings
        .iter_mut()
        .find(|setting| setting.key == "loop_mode")
        .expect("loop mode")
        .value = SettingValue::Text("ping_pong".to_owned());
    let ping_pong =
        CompiledProject::compile_with_assets(&ping_pong, &assets).expect("ping-pong loop");
    let ping_a = render(&ping_pong, 72_000, 64);
    let ping_b = render(&ping_pong, 72_000, 251);
    assert_eq!(ping_a, ping_b);
    assert_ne!(ping_a, looped_output);
    assert!(late_peak(&ping_a) > 1.0e-4);
}

#[test]
fn one_shot_ignores_note_off_while_gated_playback_has_a_fixed_release() {
    let path = Path::new("fixtures/synth-alpha/minimal-sample-source.crump");
    let authored = fs::read_to_string(path).expect("fixture");
    let short_gated = authored
        .replace("note 0beat C4 1bar 0.8", "note 0beat C4 1/4beat 0.8")
        .lines()
        .filter(|line| {
            !line.contains("node envelope envelope")
                && !line.contains("bus envelope-gate")
                && !line.contains("bus envelope-gain")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let one_shot = short_gated.replace("playback gated", "playback one_shot");
    let compile = |source: &str| {
        let parsed = parse_project(path, source);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("Project");
        let diagnostics = validate_project(&project);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let (assets, diagnostics) =
            load_asset_inventory(Path::new("fixtures/synth-alpha"), &project.assets);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        CompiledProject::compile_with_assets(&project, &assets).expect("sample Project")
    };
    let gated = render(&compile(&short_gated), 20_000, 64);
    let one_shot = render(&compile(&one_shot), 20_000, 251);
    let peak = |output: &[[f32; 2]]| {
        output[10_000..15_000]
            .iter()
            .flatten()
            .map(|sample| sample.abs())
            .fold(0.0_f32, f32::max)
    };
    assert!(peak(&gated) < 1.0e-6);
    assert!(peak(&one_shot) > 1.0e-4);
}

#[test]
fn multisample_velocity_zones_and_round_robin_are_deterministic() {
    let (project, assets) = fixture("minimal-multisample.crump");
    let compiled = CompiledProject::compile_with_assets(&project, &assets).expect("multisample");
    let output = render(&compiled, 84_000, 127);
    let energy = |range: std::ops::Range<usize>| {
        output[range].iter().fold([0.0_f64; 2], |mut sum, frame| {
            sum[0] += f64::from(frame[0]).abs();
            sum[1] += f64::from(frame[1]).abs();
            sum
        })
    };
    let low = energy(1_000..20_000);
    let high = energy(49_000..68_000);
    assert!(
        low[0] > low[1],
        "low zone must retain its authored left pan"
    );
    assert!(
        high[1] > high[0],
        "high zone must retain its authored right pan"
    );

    let mut layered = project;
    let source = layered.song.instruments[0]
        .graph
        .as_mut()
        .expect("voice graph")
        .nodes
        .iter_mut()
        .find(|node| node.id == "source")
        .expect("multisample source");
    source.directives[0].tokens[25].value = "-0.9".to_owned();
    let mut alternate = source.directives[0].clone();
    alternate.tokens[1].value = "low-alt".to_owned();
    alternate.tokens[18].value = "1".to_owned();
    alternate.tokens[25].value = "0.9".to_owned();
    source.directives.push(alternate);

    let mut selected_left = false;
    let mut selected_right = false;
    for seed in 1..=16 {
        layered.seed = seed;
        let compiled =
            CompiledProject::compile_with_assets(&layered, &assets).expect("round robin");
        let first = render(&compiled, 20_000, 113);
        let sums = first.iter().fold([0.0_f64; 2], |mut sum, frame| {
            sum[0] += f64::from(frame[0]).abs();
            sum[1] += f64::from(frame[1]).abs();
            sum
        });
        selected_left |= sums[0] > sums[1];
        selected_right |= sums[1] > sums[0];
        assert_eq!(first, render(&compiled, 20_000, 251));
    }
    assert!(
        selected_left && selected_right,
        "semantic seeds must exercise both round-robin zones"
    );
}

#[test]
fn copied_sample_projects_reopen_without_external_library_paths() {
    for (fixture_name, media) in [
        ("minimal-sample-source.crump", &["sample-source.wav"][..]),
        (
            "minimal-multisample.crump",
            &["multisample-low.wav", "multisample-high.wav"][..],
        ),
    ] {
        let (project, assets) = fixture(fixture_name);
        let original =
            CompiledProject::compile_with_assets(&project, &assets).expect("original Project");
        let copied = tempfile::tempdir().expect("clean account");
        fs::create_dir(copied.path().join("media")).expect("copied media directory");
        fs::copy(
            Path::new("fixtures/synth-alpha").join(fixture_name),
            copied.path().join("project.crump"),
        )
        .expect("copy source");
        for file in media {
            fs::copy(
                Path::new("fixtures/synth-alpha/media").join(file),
                copied.path().join("media").join(file),
            )
            .expect("copy exact sample bytes");
        }
        let source =
            fs::read_to_string(copied.path().join("project.crump")).expect("copied source");
        let parsed = parse_project(&copied.path().join("project.crump"), &source);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        let copied_project = parsed.project.expect("copied Project");
        let (copied_assets, diagnostics) =
            load_asset_inventory(copied.path(), &copied_project.assets);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let reopened = CompiledProject::compile_with_assets(&copied_project, &copied_assets)
            .expect("reopen copied Project");
        assert_eq!(
            original.sample_source_inspection(),
            reopened.sample_source_inspection()
        );
        assert_eq!(
            render(&original, 32_000, 64),
            render(&reopened, 32_000, 239)
        );
    }
}

#[test]
fn malformed_sample_regions_and_zone_assets_fail_before_compilation() {
    let (mut sample, sample_assets) = fixture("minimal-sample-source.crump");
    let source = sample.song.instruments[0]
        .graph
        .as_mut()
        .expect("voice graph")
        .nodes
        .iter_mut()
        .find(|node| node.id == "source")
        .expect("sample source");
    source.directives[0].tokens[3].value = "999999".to_owned();
    let diagnostics = validate_project_assets(&sample, &sample_assets);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV198" && diagnostic.message.contains("sample slice")
    }));
    assert!(CompiledProject::compile_with_assets(&sample, &sample_assets).is_err());

    let (mut unordered, _) = fixture("minimal-sample-source.crump");
    unordered.song.instruments[0].parameters[0].maximum = 1.0;
    let diagnostics = validate_project(&unordered);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV060" && diagnostic.message.contains("start domain")
    }));

    let (mut multisample, assets) = fixture("minimal-multisample.crump");
    let source = multisample.song.instruments[0]
        .graph
        .as_mut()
        .expect("voice graph")
        .nodes
        .iter_mut()
        .find(|node| node.id == "source")
        .expect("multisample source");
    source.directives[0].tokens[3].value = "missing-zone".to_owned();
    let diagnostics = validate_project(&multisample);
    assert!(diagnostics.as_slice().iter().any(|diagnostic| {
        diagnostic.code == "CRV196" && diagnostic.message.contains("missing-zone")
    }));
    assert!(CompiledProject::compile_with_assets(&multisample, &assets).is_err());
}

#[test]
fn one_shot_sources_preserve_long_tails_and_fade_at_the_release_ceiling() {
    use sha2::{Digest, Sha256};
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("ring.wav");
    let mut wav = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for frame in 0..48000 * 32 {
        let value = (std::f64::consts::TAU * 440.0 * f64::from(frame) / 48000.0).sin();
        wav.write_sample((value * 8192.0) as i16).unwrap();
    }
    wav.finalize().unwrap();
    let digest: String = Sha256::digest(fs::read(&path).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    for (kind, mapping) in [
        ("sample_source", "asset ring\nroot A4"),
        (
            "multisample",
            "zone body asset ring keys C0 G9 velocity 0 1 root A4 layer body articulation default round_robin body 0 region 0 1536000 gain 0db pan 0",
        ),
    ] {
        let source = format!(
            r#"crump 2
project "Long one-shot" {{
  id long-one-shot
  seed 53
  sample_rate 48000
  render_tail 0s
  asset ring "Ring" wav "ring.wav" sha256 "{digest}" {{
    usage sample multisample
  }}
  song {{
    tempo 120bpm
    meter 4/4
    end 16bar
    instrument ring "Ring" graph {{
      polyphony 1
      parameter level "Level" ratio 0 1 0.5 0.5
      faceplate {{
        control level "Level" "Output level" maps level
      }}
      graph voice 4 {{
        node source {kind} {{
          {mapping}
          playback one_shot
          level $level
        }}
        node out output
        bus pitch note.pitch -> source.pitch
        bus gate note.gate -> source.gate
        bus velocity note.velocity -> source.velocity
        bus audio source.audio -> out.audio
      }}
    }}
    pattern strike 16bar {{
      note 0beat A4 0.25beat 1
    }}
    track ring "Ring" instrument ring level 0db pan 0 {{
      clip strike pattern strike start 0bar duration 16bar mode once
    }}
  }}
}}
"#
        );
        let parsed = parse_project(&folder.path().join("project.crump"), &source);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        let project = parsed.project.unwrap();
        let (assets, diagnostics) = load_asset_inventory(folder.path(), &project.assets);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        let compiled = CompiledProject::compile_with_assets(&project, &assets).unwrap();
        let output = render(&compiled, 48000 * 31, 257);
        let energy = |start: usize, end: usize| {
            output[start..end]
                .iter()
                .map(|x| f64::from(x[0]).powi(2))
                .sum::<f64>()
                / (end - start) as f64
        };
        assert!(
            energy(48000 * 9, 48000 * 10) > 0.001,
            "{kind} lost its natural tail"
        );
        // Note-off is at 125 ms; the last 50 ms of its 30-second budget fade.
        assert!(energy(1444800, 1445760) < energy(1440000, 1440960) * 0.3);
        assert_eq!(energy(48000 * 30 + 12000, 48000 * 31), 0.0);
    }
}
