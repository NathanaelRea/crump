//! Feedback graphs, generic signal primitives, the feedback guard, and waveguide modules.
use std::{collections::BTreeMap, f64::consts::PI, path::PathBuf};

use crump::{
    engine::{CompiledProject, ProcessingMode},
    project::{ProjectBundle, load_project_sources},
};

const PLUCKED_STRING: &str = include_str!("../examples/waveguides/plucked-string/instrument.crump");
const SITAR: &str = include_str!("../examples/waveguides/sitar/instrument.crump");

/// Wraps graph statements in a one-control probe Instrument Module.
fn module(graph: &str) -> String {
    format!(
        r#"crump-module 2
instrument probe "Probe" graph {{
  polyphony 2
  parameter level "Level" ratio 0 1 0.5 0.5
  faceplate {{
    control level "Level" "Output level" maps level
  }}
  graph voice 40 {{
{graph}
  }}
}}
"#
    )
}

fn project_with(
    instruments: &[(&str, &str)],
    tracks: &[(&str, &str)],
    notes: &str,
    rate: u32,
) -> ProjectBundle {
    let imports = instruments
        .iter()
        .map(|(file, _)| format!("import \"{file}\"\n"))
        .collect::<String>();
    let tracks = tracks
        .iter()
        .map(|(id, instrument)| {
            format!(
                "    track {id} \"{id}\" instrument {instrument} level 0db pan 0 {{\n      clip {id}-clip pattern notes start 0bar duration 2bar mode once\n    }}\n"
            )
        })
        .collect::<String>();
    let source = format!(
        r#"crump 2
{imports}project "Feedback" {{
  id feedback
  seed 7
  sample_rate {rate}
  render_tail 1s
  song {{
    tempo 120bpm
    meter 4/4
    end 2bar
    pattern notes 2bar {{
{notes}
    }}
{tracks}  }}
}}
"#
    );
    let mut sources = BTreeMap::from([(PathBuf::from("project.crump"), source)]);
    for (file, text) in instruments {
        sources.insert(PathBuf::from(file), (*text).to_owned());
    }
    load_project_sources(PathBuf::from("/feedback-test"), sources)
}

fn probe(graph: &str) -> ProjectBundle {
    project_with(
        &[("instrument.crump", &module(graph))],
        &[("probe", "probe")],
        "      note 0beat C4 1beat 0.8",
        48_000,
    )
}

fn codes(bundle: &ProjectBundle) -> Vec<(String, String)> {
    bundle
        .diagnostics
        .as_slice()
        .iter()
        .map(|diagnostic| (diagnostic.code.clone(), diagnostic.message.clone()))
        .collect()
}

fn compile(bundle: &ProjectBundle) -> CompiledProject {
    assert!(!bundle.diagnostics.has_errors(), "{:?}", codes(bundle));
    CompiledProject::compile(bundle.project.as_ref().expect("parsed Project")).expect("compile")
}

fn render(compiled: &CompiledProject, frames: usize, quantum: usize) -> Vec<[f32; 2]> {
    let mut runtime = compiled.runtime();
    let mut output = vec![[0.0; 2]; frames];
    for block in output.chunks_mut(quantum) {
        runtime
            .process_block(block, ProcessingMode::Offline)
            .expect("finite render");
    }
    output
}

/// Strongest spectral peak within a semitone of `expected`, refined by golden search.
fn fundamental(signal: &[f32], expected: f64, rate: f64) -> f64 {
    let magnitude = |frequency: f64| {
        let (mut real, mut imaginary) = (0.0, 0.0);
        for (index, sample) in signal.iter().enumerate() {
            let window = 0.5 - 0.5 * (2.0 * PI * index as f64 / (signal.len() - 1) as f64).cos();
            let phase = 2.0 * PI * frequency * index as f64 / rate;
            real += f64::from(*sample) * window * phase.cos();
            imaginary -= f64::from(*sample) * window * phase.sin();
        }
        real.hypot(imaginary)
    };
    let semitone = 2.0_f64.powf(1.0 / 12.0);
    let steps = 24;
    let ratio = semitone.powf(2.0 / f64::from(steps));
    let mut best = (expected, 0.0);
    for step in 0..=steps {
        let frequency = expected / semitone * ratio.powi(step);
        let value = magnitude(frequency);
        if value > best.1 {
            best = (frequency, value);
        }
    }
    let (mut left, mut right) = (best.0 / ratio, best.0 * ratio);
    let golden = (5.0_f64.sqrt() - 1.0) / 2.0;
    for _ in 0..40 {
        let first = right - golden * (right - left);
        let second = left + golden * (right - left);
        if magnitude(first) > magnitude(second) {
            right = second;
        } else {
            left = first;
        }
    }
    (left + right) * 0.5
}

fn cents(measured: f64, expected: f64) -> f64 {
    1200.0 * (measured / expected).log2()
}

const PLUCK: &str = "    node pluck exciter kind impact contact 0.02ms hardness 1 level $level
    bus gate note.gate -> pluck.gate
    bus velocity note.velocity -> pluck.velocity";

#[test]
fn zero_latency_cycles_are_rejected_with_the_cycle_named() {
    let bundle = probe(&format!(
        "{PLUCK}
    node mix math operation add
    node scaled math operation scale amount 0.5
    node stereo mono_to_stereo
    node out output
    bus excite pluck.audio -> mix.a
    bus loop mix.audio -> scaled.a
    bus back scaled.audio -> mix.b
    bus tap mix.audio -> stereo.audio
    bus output stereo.audio -> out.audio"
    ));
    let diagnostics = codes(&bundle);
    assert!(
        diagnostics.iter().any(|(code, message)| code == "CRV048"
            && message.contains("`mix -> scaled -> mix`")
            && message.contains("delay_line")),
        "{diagnostics:?}"
    );
}

fn one_frame_loop(feedback_target: &str) -> String {
    format!(
        "{PLUCK}
    node mix math operation add
    node previous delay_line mode time time 0.01ms interpolation linear max_time 1ms
    node scaled math operation scale amount 0.5
    node left pan position -1
    node right pan position 1
    node both stereo_mixer a_level 1 b_level 1
    node out output
    bus excite pluck.audio -> mix.a
    bus into mix.audio -> previous.audio
    bus held previous.audio -> scaled.a
    bus back {feedback_target}
    bus dry pluck.audio -> left.audio
    bus wet mix.audio -> right.audio
    bus left-out left.audio -> both.a
    bus right-out right.audio -> both.b
    bus output both.audio -> out.audio"
    )
}

#[test]
fn feedback_through_delay_line_is_valid_and_typed() {
    compile(&probe(&one_frame_loop("scaled.audio -> mix.b")));
    // A feedback edge carries ordinary typed signals: stereo cannot close a mono loop.
    let mistyped = probe(&one_frame_loop(
        "scaled.audio -> widen.audio\n    node widen mono_to_stereo\n    bus wide widen.audio -> mix.b",
    ));
    let diagnostics = codes(&mistyped);
    assert!(
        diagnostics.iter().any(|(code, _)| code == "CRV044"),
        "{diagnostics:?}"
    );
}

#[test]
fn one_frame_feedback_loop_latency_is_exact() {
    let compiled = compile(&probe(&one_frame_loop("scaled.audio -> mix.b")));
    let output = render(&compiled, 4_800, 64);
    let excitation = output.iter().map(|frame| frame[0]).collect::<Vec<_>>();
    let looped = output.iter().map(|frame| frame[1]).collect::<Vec<_>>();
    assert!(excitation.iter().any(|value| value.abs() > 0.01));
    let mut previous = 0.0_f32;
    for (frame, (input, value)) in excitation.iter().zip(&looped).enumerate() {
        // y[n] = x[n] + 0.5 y[n − 1]: the loop is exactly one frame long.
        let expected = input + 0.5 * previous;
        assert!(
            (value - expected).abs() <= 1.0e-6 * (1.0 + expected.abs()),
            "frame {frame}: {value} != {expected}"
        );
        previous = *value;
    }
}

fn string_bundle(source: &str, id: &str, notes: &str, rate: u32) -> ProjectBundle {
    project_with(
        &[("instrument.crump", source)],
        &[("voice", id)],
        notes,
        rate,
    )
}

#[test]
fn waveguide_modules_play_in_tune_across_their_range() {
    for (source, id, tolerance) in [
        (PLUCKED_STRING, "plucked-string", 0.5),
        // Bridge contact shortens the string slightly, so a sitar is a fraction sharp.
        (SITAR, "sitar", 1.0),
    ] {
        for (note, midi) in [("E1", 28), ("E3", 52), ("A4", 69), ("E6", 88), ("C7", 96)] {
            let compiled = compile(&string_bundle(
                source,
                id,
                &format!("      note 0beat {note} 3beat 0.85"),
                48_000,
            ));
            let output = render(&compiled, 48_000, 256);
            let left = output[4_800..36_000]
                .iter()
                .map(|frame| frame[0])
                .collect::<Vec<_>>();
            let expected = 440.0 * 2.0_f64.powf((f64::from(midi) - 69.0) / 12.0);
            let measured = fundamental(&left, expected, 48_000.0);
            assert!(
                cents(measured, expected).abs() < tolerance,
                "{id} {note}: {measured} Hz vs {expected} Hz"
            );
        }
    }
}

#[test]
fn waveguide_renders_are_deterministic_across_runs_and_groupings() {
    let notes = "      note 0beat A2 1beat 0.9\n      note 1beat E4 2beat 0.6";
    let compiled = compile(&string_bundle(
        PLUCKED_STRING,
        "plucked-string",
        notes,
        48_000,
    ));
    let first = render(&compiled, 96_000, 64);
    assert_eq!(first, render(&compiled, 96_000, 256));
    let rebuilt = compile(&string_bundle(
        PLUCKED_STRING,
        "plucked-string",
        notes,
        48_000,
    ));
    assert_eq!(first, render(&rebuilt, 96_000, 128));
    assert!(first.iter().flatten().all(|value| value.is_finite()));
    for source in [PLUCKED_STRING, SITAR] {
        let id = if source == SITAR {
            "sitar"
        } else {
            "plucked-string"
        };
        let compiled = compile(&string_bundle(source, id, notes, 48_000));
        let mut runtime = compiled.runtime();
        let mut block = [[0.0; 2]; 256];
        let allocations = allocation_counter::measure(|| {
            for _ in 0..300 {
                runtime
                    .process_block(&mut block, ProcessingMode::Offline)
                    .unwrap();
            }
        });
        assert_eq!(allocations.count_total, 0, "{id}");
    }
}

fn runaway(gain: f64) -> String {
    module(&format!(
        "{PLUCK}
    node mix math operation add
    node previous delay_line mode time time 1ms interpolation linear max_time 2ms
    node grow math operation scale amount {gain}
    node stereo mono_to_stereo
    node out output
    bus excite pluck.audio -> mix.a
    bus into mix.audio -> previous.audio
    bus held previous.audio -> grow.a
    bus back grow.audio -> mix.b
    bus tap mix.audio -> stereo.audio
    bus output stereo.audio -> out.audio"
    ))
    .replace(
        "instrument probe \"Probe\"",
        "instrument runaway \"Runaway\"",
    )
}

const TONE: &str = r#"crump-module 2
instrument tone "Tone" graph {
  polyphony 2
  parameter level "Level" ratio 0 1 0.5 0.5
  faceplate {
    control level "Level" "Output level" maps level
  }
  graph voice 4 {
    node osc oscillator waveform sine level $level
    node stereo mono_to_stereo
    node out output
    bus pitch note.pitch -> osc.pitch
    bus tap osc.audio -> stereo.audio
    bus output stereo.audio -> out.audio
  }
}
"#;

#[test]
fn runaway_feedback_is_muted_reported_reset_and_contained_to_its_voice() {
    let notes = "      note 0beat C4 1beat 0.8\n      note 2beat C4 1beat 0.8";
    for gain in ["64", "1.05"] {
        let unstable = runaway(gain.parse().unwrap());
        let both = compile(&project_with(
            &[("runaway.crump", &unstable), ("tone.crump", TONE)],
            &[("runaway", "runaway"), ("tone", "tone")],
            notes,
            48_000,
        ));
        let alone = compile(&project_with(
            &[("tone.crump", TONE)],
            &[("tone", "tone")],
            notes,
            48_000,
        ));
        let mut runtime = both.runtime();
        let mut mixed = vec![[0.0; 2]; 96_000];
        for block in mixed.chunks_mut(128) {
            runtime
                .process_block(block, ProcessingMode::Offline)
                .expect("the guard keeps the render finite");
        }
        // Each note blows up, is muted and reset, and the next note starts from silence.
        assert_eq!(runtime.telemetry().feedback_mutes, 2, "gain {gain}");
        let reference = render(&alone, 96_000, 128);
        // After the second mute the runaway voice is silent and the tone is untouched.
        let settled = 24_000 + 20_000;
        for frame in settled..48_000 {
            assert_eq!(mixed[frame], reference[frame], "gain {gain}, frame {frame}");
        }
        for frame in 72_000..96_000 {
            assert_eq!(mixed[frame], reference[frame], "gain {gain}, frame {frame}");
        }
    }
}

#[test]
fn delay_line_buffers_enter_resource_admission() {
    let graph = |max_time: &str| {
        probe(&format!(
            "{PLUCK}
    node echo delay_line mode time time 5ms max_time {max_time}
    node stereo mono_to_stereo
    node out output
    bus into pluck.audio -> echo.audio
    bus tap echo.audio -> stereo.audio
    bus output stereo.audio -> out.audio"
        ))
    };
    let bytes = |bundle: &ProjectBundle| {
        compile(bundle)
            .mix_graph_inspection()
            .resources
            .graph_voice_state_bytes
    };
    let small = bytes(&graph("100ms"));
    let large = bytes(&graph("1s"));
    // Two voice slots, each with a primary voice and a steal tail, 43,200 more f32 frames.
    assert_eq!(large - small, 4 * 43_200 * 4);
    let too_long = probe(&format!(
        "{PLUCK}
    node echo delay_line mode time time 200ms max_time 100ms
    node stereo mono_to_stereo
    node out output
    bus into pluck.audio -> echo.audio
    bus tap echo.audio -> stereo.audio
    bus output stereo.audio -> out.audio"
    ));
    assert!(
        codes(&too_long)
            .iter()
            .any(|(code, message)| code == "CRV220" && message.contains("max_time")),
        "{:?}",
        codes(&too_long)
    );
}

#[test]
fn waveguide_voices_are_admitted_with_bounded_work() {
    for (source, id) in [(PLUCKED_STRING, "plucked-string"), (SITAR, "sitar")] {
        let compiled = compile(&string_bundle(
            source,
            id,
            "      note 0beat C4 1beat 0.8",
            48_000,
        ));
        let resources = &compiled.mix_graph_inspection().resources;
        assert!(resources.admitted, "{id}");
        assert_eq!(resources.graph_voices, 8, "{id}");
        // Each slot admits a primary voice and a steal tail. A waveguide voice costs about a
        // hundred node-equivalents, most of them the exciter's contact pool.
        let per_voice = resources.graph_node_instances / (2 * resources.graph_voices);
        assert!((60..120).contains(&per_voice), "{id}: {per_voice}");
    }
}

#[test]
fn explicit_conversion_lets_envelopes_drive_audio_rate_pitch() {
    let kick = |route: &str| {
        probe(&format!(
            "    node sweep envelope attack 0ms decay 60ms sustain 0 release 50ms
    node sweep-audio to_audio
    node body analog_oscillator waveform sine coarse -24st fm_depth 36st level $level
    node out output
    bus gate note.gate -> sweep.gate
    bus pitch note.pitch -> body.pitch
    {route}
    bus output body.audio -> out.audio"
        ))
    };
    let direct = kick("bus fm sweep.control -> body.fm");
    assert!(
        codes(&direct)
            .iter()
            .any(|(code, message)| code == "CRV044" && message.contains("explicit conversion")),
        "{:?}",
        codes(&direct)
    );
    let compiled = compile(&kick(
        "bus convert sweep.control -> sweep-audio.control\n    bus fm sweep-audio.audio -> body.fm",
    ));
    let output = render(&compiled, 12_000, 64);
    let crossings = |range: std::ops::Range<usize>| {
        output[range]
            .windows(2)
            .filter(|pair| (pair[0][0] < 0.0) != (pair[1][0] < 0.0))
            .count()
    };
    // C4 two octaves down is 65 Hz; the envelope starts it three octaves higher.
    let early = crossings(0..960);
    let late = crossings(9_600..12_000);
    assert!(early >= 12, "early crossings {early}");
    assert!(late <= 8, "late crossings {late}");
}

#[test]
fn primitive_port_and_setting_contracts_fail_closed() {
    let out = "    node stereo mono_to_stereo
    node out output
    bus tap shaped.audio -> stereo.audio
    bus output stereo.audio -> out.audio";
    let cases = [
        (
            format!(
                "{PLUCK}\n    node shaped math operation add\n    bus a pluck.audio -> shaped.a\n{out}"
            ),
            "CRV220",
            "requires input `b`",
        ),
        (
            format!(
                "{PLUCK}\n    node shaped math operation abs\n    bus a pluck.audio -> shaped.a\n    bus b pluck.audio -> shaped.b\n{out}"
            ),
            "CRV220",
            "uses only input `a`",
        ),
        (
            format!(
                "{PLUCK}\n    node shaped math operation add amount 2\n    bus a pluck.audio -> shaped.a\n    bus b pluck.audio -> shaped.b\n{out}"
            ),
            "CRV220",
            "applies only to offset and scale",
        ),
        (
            format!(
                "{PLUCK}\n    node shaped shaper {{\n      curve table\n      point low input 0.5 output 0\n      point high input 0.5 output 1\n    }}\n    bus a pluck.audio -> shaped.audio\n{out}"
            ),
            "CRV060",
            "strictly increase",
        ),
        (
            format!(
                "{PLUCK}\n    node shaped shaper {{\n      curve tanh\n      point low input 0 output 0\n      point high input 1 output 1\n    }}\n    bus a pluck.audio -> shaped.audio\n{out}"
            ),
            "CRV060",
            "only a table shaper",
        ),
        (
            format!(
                "{PLUCK}\n    node shaped delay_line mode pitch\n    bus a pluck.audio -> shaped.audio\n{out}"
            ),
            "CRV220",
            "requires input `pitch`",
        ),
        (
            format!(
                "{PLUCK}\n    node shaped delay_line mode time ratio 2\n    bus a pluck.audio -> shaped.audio\n{out}"
            ),
            "CRV220",
            "applies only in pitch mode",
        ),
    ];
    for (graph, code, text) in cases {
        let diagnostics = codes(&probe(&graph));
        assert!(
            diagnostics
                .iter()
                .any(|(found, message)| found == code && message.contains(text)),
            "{code} `{text}` missing from {diagnostics:?}"
        );
    }
    let table = compile(&probe(&format!(
        "{PLUCK}
    node shaped shaper {{
      curve table
      drive 4
      point low input -1 output -0.5
      point mid input 0 output 0.25
      point high input 1 output 0.75
    }}
    bus a pluck.audio -> shaped.audio
{out}"
    )));
    assert!(
        render(&table, 4_800, 64)
            .iter()
            .flatten()
            .all(|value| value.is_finite())
    );
}

#[test]
fn shared_graph_delay_lines_modulate_and_feed_back() {
    let source = r#"crump-module 2
instrument chorus "Chorus" graph {
  polyphony 2
  parameter level "Level" ratio 0 1 0.5 0.5
  faceplate {
    control level "Level" "Output level" maps level
  }
  graph voice 4 {
    node osc oscillator waveform saw level $level
    node stereo mono_to_stereo
    node out output
    bus pitch note.pitch -> osc.pitch
    bus tap osc.audio -> stereo.audio
    bus output stereo.audio -> out.audio
  }
  graph shared 12 {
    node fold stereo_to_mono
    node wobble lfo waveform sine rate 0.7hz depth 1 polarity bipolar
    node wobble-audio to_audio
    node comb math operation add
    node sweep delay_line mode time time 4ms max_time 10ms mod_depth 0.5
    node regen math operation scale amount 0.6
    node wide mono_to_stereo
    node shared-out output
    bus sum voice_sum.audio -> fold.audio
    bus dry fold.audio -> comb.a
    bus modulation wobble.control -> wobble-audio.control
    bus time wobble-audio.audio -> sweep.time_mod
    bus into comb.audio -> sweep.audio
    bus held sweep.audio -> regen.a
    bus back regen.audio -> comb.b
    bus wet comb.audio -> wide.audio
    bus output wide.audio -> shared-out.audio
  }
}
"#;
    let compiled = compile(&project_with(
        &[("chorus.crump", source)],
        &[("chorus", "chorus")],
        "      note 0beat C3 3beat 0.8",
        48_000,
    ));
    let output = render(&compiled, 48_000, 128);
    assert!(output.iter().flatten().all(|value| value.is_finite()));
    assert!(output.iter().any(|frame| frame[0].abs() > 0.05));
    assert_eq!(output, render(&compiled, 48_000, 64));
}

#[test]
fn cycles_are_named_even_with_downstream_sinks_and_unused_instruments() {
    let source = module(&format!(
        "{PLUCK}
    node mix math operation add
    node shaped shaper
    node scaled math operation scale amount 0.5
    node stereo mono_to_stereo
    node aout output
    bus excite pluck.audio -> mix.a
    bus shape mix.audio -> shaped.audio
    bus loop shaped.audio -> scaled.a
    bus back scaled.audio -> mix.b
    bus tap mix.audio -> stereo.audio
    bus output stereo.audio -> aout.audio"
    ));
    for instrument in ["probe", "tone"] {
        let bundle = project_with(
            &[("probe.crump", &source), ("tone.crump", TONE)],
            &[("voice", instrument)],
            "      note 0beat C4 1beat 0.8",
            48_000,
        );
        let diagnostics = codes(&bundle);
        assert!(
            diagnostics.iter().any(|(code, message)| code == "CRV048"
                && message.contains("mix -> shaped -> scaled -> mix")),
            "{instrument}: {diagnostics:?}"
        );
    }
}

#[test]
fn delay_line_capacity_requires_a_literal_allocation_bound() {
    let source = module(&format!(
        "{PLUCK}
    node echo delay_line time 50ms max_time $capacity interpolation linear
    node stereo mono_to_stereo
    node out output
    bus into pluck.audio -> echo.audio
    bus tap echo.audio -> stereo.audio
    bus output stereo.audio -> out.audio"
    ))
    .replace(
        "  faceplate {",
        "  parameter capacity \"Capacity\" seconds 1ms 100ms 10ms 10ms\n  faceplate {",
    );
    let bundle = project_with(
        &[("instrument.crump", &source)],
        &[("probe", "probe")],
        "      note 0beat C4 1beat 0.8",
        48_000,
    );
    assert!(
        codes(&bundle).iter().any(|(code, message)| code == "CRV220"
            && message.contains("max_time")
            && message.contains("literal")),
        "{:?}",
        codes(&bundle)
    );
    let literal = source.replace("max_time $capacity", "max_time 100ms");
    let compiled = compile(&project_with(
        &[("instrument.crump", &literal)],
        &[("probe", "probe")],
        "      note 0beat C4 1beat 0.8",
        48_000,
    ));
    let output = render(&compiled, 4_800, 64);
    assert!(output[..2_400].iter().flatten().all(|value| *value == 0.0));
    assert!(
        output[2_400..]
            .iter()
            .flatten()
            .any(|value| value.abs() > 0.01)
    );
}

#[test]
fn converted_audio_drives_parameter_modulation_on_the_same_frame() {
    let source = module(
        "    node input to_audio
    node converted to_control
    node gain math operation scale amount $level
    node stereo mono_to_stereo
    node out output
    bus velocity note.velocity -> input.control
    bus convert input.audio -> converted.audio
    bus input input.audio -> gain.a
    bus tap gain.audio -> stereo.audio
    bus output stereo.audio -> out.audio",
    )
    .replace("0.5 0.5\n", "0.5 0.5 modulatable\n")
    .replace("  graph voice", "  modulation {\n    source signal port converted.control per_voice bipolar\n    route gain source signal target level depth 0.25 map bipolar curve linear order 0 bypass false\n  }\n  graph voice");
    let bundle = |source: &str| {
        project_with(
            &[("instrument.crump", source)],
            &[("probe", "probe")],
            "      note 0beat C4 1beat 0.8",
            48_000,
        )
    };
    let compiled = compile(&bundle(&source));
    let output = render(&compiled, 256, 64);
    // Effective gain = 0.5 + 0.8 * 0.25 = 0.7, without a one-frame cache.
    assert!((output[0][0] - 0.56).abs() < 1.0e-6, "{:?}", output[0]);
    assert_eq!(output, render(&compiled, 256, 128));
    let wrong_polarity = source.replace("per_voice bipolar", "per_voice unipolar");
    assert!(
        codes(&bundle(&wrong_polarity))
            .iter()
            .any(|(code, _)| code == "CRV191")
    );
    let same_frame_cycle = source.replace(
        "input.audio -> converted.audio",
        "gain.audio -> converted.audio",
    );
    assert!(
        codes(&bundle(&same_frame_cycle))
            .iter()
            .any(|(code, _)| code == "CRV048")
    );
}

#[test]
fn converted_audio_can_modulate_the_delay_that_breaks_its_feedback_cycle() {
    let source = module(&format!(
        "{PLUCK}
    node held delay_line time $delay-time max_time 10ms interpolation linear
    node converted to_control
    node stereo mono_to_stereo
    node out output
    bus excite pluck.audio -> held.audio
    bus convert held.audio -> converted.audio
    bus tap held.audio -> stereo.audio
    bus output stereo.audio -> out.audio"
    ))
    .replace("  faceplate {", "  parameter delay-time \"Time\" seconds 1ms 5ms 2ms 2ms modulatable\n  faceplate {")
    .replace("  graph voice", "  modulation {\n    source signal port converted.control per_voice bipolar\n    route time source signal target delay-time depth 0.5ms map bipolar curve linear order 0 bypass false\n  }\n  graph voice");
    let compiled = compile(&project_with(
        &[("instrument.crump", &source)],
        &[("probe", "probe")],
        "      note 0beat C4 1beat 0.8",
        48_000,
    ));
    let output = render(&compiled, 4_800, 64);
    assert!(output.iter().flatten().any(|value| value.abs() > 0.01));
    assert_eq!(output, render(&compiled, 4_800, 128));
}

#[test]
fn reused_voice_gets_a_fresh_feedback_guard_interval() {
    let source = module(
        "    node input to_audio
    node gain math operation scale amount $level
    node held delay_line time 0.01ms interpolation linear max_time 1ms
    node stereo mono_to_stereo
    node out output
    bus velocity note.velocity -> input.control
    bus gain input.audio -> gain.a
    bus delay gain.audio -> held.audio
    bus tap held.audio -> stereo.audio
    bus output stereo.audio -> out.audio",
    )
    .replace("polyphony 2", "polyphony 1")
    .replace("ratio 0 1 0.5 0.5", "ratio 0 64 32 32");
    let compiled = compile(&project_with(
        &[("instrument.crump", &source)],
        &[("probe", "probe")],
        "      note 0beat C4 1beat 0.8\n      note 1/25beat C4 3beat 0\n      note 2beat C4 1beat 0.8",
        48_000,
    ));
    let mut runtime = compiled.runtime();
    let mut output = vec![[0.0; 2]; 50_399];
    runtime
        .process_block(&mut output, ProcessingMode::Offline)
        .unwrap();
    assert_eq!(runtime.telemetry().feedback_mutes, 0);
    assert!(output[49_500][0] > 15.0, "fresh note was prematurely muted");
    let mut tail = [[0.0; 2]; 64];
    runtime
        .process_block(&mut tail, ProcessingMode::Offline)
        .unwrap();
    assert_eq!(runtime.telemetry().feedback_mutes, 1);
    assert_eq!(tail.last(), Some(&[0.0, 0.0]));
}

#[test]
fn non_finite_loop_samples_are_reported_before_delay_sanitization() {
    let mut graph = format!(
        "{PLUCK}
    node mix math operation add
    node large math operation offset amount 64
    node poison dc_block
    node previous delay_line time 1ms interpolation linear max_time 2ms
    node stereo mono_to_stereo
    node out output
    bus excite pluck.audio -> mix.a
    bus back previous.audio -> mix.b
    bus large-input mix.audio -> large.a\n"
    );
    let mut previous = "large".to_owned();
    for index in 0..6 {
        graph.push_str(&format!("    node square{index} math operation multiply\n    bus square{index}a {previous}.audio -> square{index}.a\n    bus square{index}b {previous}.audio -> square{index}.b\n"));
        previous = format!("square{index}");
    }
    graph.push_str(&format!(
        "    bus poison {previous}.audio -> poison.audio
    bus into poison.audio -> previous.audio
    bus tap previous.audio -> stereo.audio
    bus output stereo.audio -> out.audio"
    ));
    let source = module(&graph);
    let compiled = compile(&project_with(
        &[("instrument.crump", &source)],
        &[("probe", "probe")],
        "      note 0beat C4 1beat 0.8\n      note 1beat C4 1beat 0.8",
        48_000,
    ));
    let mut runtime = compiled.runtime();
    let mut output = vec![[0.0; 2]; 48_000];
    runtime
        .process_block(&mut output, ProcessingMode::Offline)
        .unwrap();
    assert_eq!(runtime.telemetry().feedback_mutes, 2);
    assert!(output.iter().flatten().all(|value| *value == 0.0));
}

#[test]
fn biquad_and_onepole_contracts_fail_closed() {
    let out = "    node stereo mono_to_stereo
    node out output
    bus tap shaped.audio -> stereo.audio
    bus output stereo.audio -> out.audio";
    let cases = [
        (
            format!(
                "{PLUCK}\n    node shaped biquad mode lowpass gain 6db\n    bus a pluck.audio -> shaped.audio\n{out}"
            ),
            "applies only to peak and shelf modes",
        ),
        (
            format!(
                "{PLUCK}\n    node shaped biquad mode bandpass\n    bus a pluck.audio -> shaped.audio\n    bus k note.pitch -> shaped.key\n{out}"
            ),
            "reads `key` only with `key_track`",
        ),
        (
            format!(
                "{PLUCK}\n    node shaped onepole mode lowpass time 0.1s\n    bus a pluck.audio -> shaped.audio\n{out}"
            ),
            "applies only in leak mode",
        ),
        (
            format!(
                "{PLUCK}\n    node shaped onepole mode leak cutoff 100hz\n    bus a pluck.audio -> shaped.audio\n{out}"
            ),
            "applies only in lowpass and highpass mode",
        ),
        (
            format!(
                "{PLUCK}\n    node shaped biquad mode peak q 100\n    bus a pluck.audio -> shaped.audio\n{out}"
            ),
            "outside its supported",
        ),
    ];
    for (graph, text) in cases {
        let diagnostics = codes(&probe(&graph));
        assert!(
            diagnostics
                .iter()
                .any(|(_, message)| message.contains(text)),
            "`{text}` missing from {diagnostics:?}"
        );
    }
}

#[test]
fn onepole_leak_shapes_a_strike_into_an_exponential_and_biquads_filter_it() {
    // A short strike integrates to an exponential decay, which scales filtered noise.
    let graph = format!(
        "{PLUCK}
    node fall onepole mode leak time 0.1s
    node hiss noise level 1
    node burst math operation multiply
    node body biquad mode peak frequency 1khz q 4 gain 12db key_track 1
    node air biquad mode allpass frequency 3khz q 0.7
    node shaped onepole mode highpass cutoff 40hz
    node stereo mono_to_stereo
    node out output
    bus strike pluck.audio -> fall.audio
    bus noise hiss.audio -> burst.a
    bus envelope fall.audio -> burst.b
    bus into burst.audio -> body.audio
    bus key note.pitch -> body.key
    bus phase body.audio -> air.audio
    bus clean air.audio -> shaped.audio
    bus tap shaped.audio -> stereo.audio
    bus output stereo.audio -> out.audio"
    );
    let compiled = compile(&probe(&graph));
    let audio = render(&compiled, 48_000, 128);
    assert!(audio.iter().flatten().all(|value| value.is_finite()));
    let level = |from: usize| {
        (audio[from..from + 2_400]
            .iter()
            .map(|f| f64::from(f[0]).powi(2))
            .sum::<f64>()
            / 2_400.0)
            .sqrt()
    };
    // Each 0.1 s, one e-folding time, the level falls by about e.
    let ratio = level(4_800) / level(9_600);
    assert!((ratio - std::f64::consts::E).abs() < 0.6, "{ratio}");
}

#[test]
fn biquad_chain_matches_the_same_biquads_in_series() {
    // Noise is the first Node in both graphs, so both draw the same sequence.
    let separate = "    node hiss noise level $level
    node low biquad mode lowshelf frequency 200hz gain -6db
    node peak biquad mode peak frequency 1.5khz q 3 gain 9db
    node follow biquad mode highpass frequency 80hz key_track 1 key_reference 220hz
    node stereo mono_to_stereo
    node out output
    bus noise hiss.audio -> low.audio
    bus first low.audio -> peak.audio
    bus second peak.audio -> follow.audio
    bus key note.pitch -> follow.key
    bus tap follow.audio -> stereo.audio
    bus output stereo.audio -> out.audio";
    let chained = "    node hiss noise level $level
    node tone biquad_chain {
      band low mode lowshelf frequency 200hz gain -6db
      band peak mode peak frequency 1.5khz q 3 gain 9db
      band follow mode highpass frequency 80hz key_track 1 key_reference 220hz
    }
    node stereo mono_to_stereo
    node out output
    bus noise hiss.audio -> tone.audio
    bus key note.pitch -> tone.key
    bus tap tone.audio -> stereo.audio
    bus output stereo.audio -> out.audio";
    let a = render(&compile(&probe(separate)), 24_000, 128);
    let b = render(&compile(&probe(chained)), 24_000, 128);
    assert!(a.iter().flatten().any(|value| *value != 0.0));
    assert_eq!(a, b);
}

#[test]
fn constant_holds_an_oscillator_on_one_pitch_for_every_key() {
    let graph = "    node fixed constant frequency 110hz
    node tone oscillator waveform sine level $level
    node stereo mono_to_stereo
    node out output
    bus pitch fixed.pitch -> tone.pitch
    bus tap tone.audio -> stereo.audio
    bus output stereo.audio -> out.audio";
    for key in ["C2", "A4", "C7"] {
        let compiled = compile(&project_with(
            &[("instrument.crump", &module(graph))],
            &[("probe", "probe")],
            &format!("      note 0beat {key} 1beat 0.8"),
            48_000,
        ));
        let audio = render(&compiled, 24_000, 128);
        let left = audio[4_800..14_400]
            .iter()
            .map(|f| f[0])
            .collect::<Vec<_>>();
        let measured = fundamental(&left, 110.0, 48_000.0);
        assert!(
            (measured / 110.0).log2().abs() * 1_200.0 < 1.0,
            "{key}: {measured}"
        );
    }
}

#[test]
fn key_curve_interpolates_between_points_and_holds_beyond_them() {
    let source = r#"crump-module 2
instrument probe "Probe" graph {
  polyphony 2
  parameter level "Level" ratio 0 1 0 0 modulatable
  faceplate {
    control level "Level" "Output level" maps level
  }
  modulation {
    source register key_curve per_voice unipolar {
      point low key C3 value 0.25
      point high key C5 value 1
    }
    route register-level source register target level depth 0.8 map unipolar curve linear order 0 bypass false
  }
  graph voice 5 {
    node tone oscillator waveform sine
    node gain math operation scale amount $level
    node stereo mono_to_stereo
    node out output
    bus pitch note.pitch -> tone.pitch
    bus level tone.audio -> gain.a
    bus tap gain.audio -> stereo.audio
    bus output stereo.audio -> out.audio
  }
}
"#;
    // Below, at, between, and above the points: 0.25, 0.25, 0.625 and 1, times the 0.8 depth.
    for (key, expected) in [("C1", 0.2), ("C3", 0.2), ("C4", 0.5), ("C7", 0.8)] {
        let compiled = compile(&project_with(
            &[("instrument.crump", source)],
            &[("probe", "probe")],
            &format!("      note 0beat {key} 1beat 0.8"),
            48_000,
        ));
        let audio = render(&compiled, 24_000, 128);
        let peak = audio[4_800..14_400]
            .iter()
            .map(|frame| frame[0].abs())
            .fold(0.0_f32, f32::max);
        assert!((f64::from(peak) - expected).abs() < 0.01, "{key}: {peak}");
    }
}

#[test]
fn biquad_chain_constant_and_key_curve_contracts_fail_closed() {
    let out = "    node stereo mono_to_stereo
    node out output
    bus tap shaped.audio -> stereo.audio
    bus output stereo.audio -> out.audio";
    let chain = |bands: &str, key: bool| {
        format!(
            "{PLUCK}\n    node shaped biquad_chain {{\n{bands}\n    }}\n    bus a pluck.audio -> shaped.audio\n{}{out}",
            if key {
                "    bus k note.pitch -> shaped.key\n"
            } else {
                ""
            }
        )
    };
    let cases = [
        (
            chain("      band a mode lowpass frequency 1khz gain 6db", false),
            "applies only to peak and shelf modes",
        ),
        (
            chain("      band a mode bandpass frequency 1khz", true),
            "reads `key` only when a band sets `key_track`",
        ),
        (
            chain(
                "      band a mode bandpass frequency 1khz key_track 1",
                false,
            ),
            "has key-tracked bands but no `key` input",
        ),
        (
            chain("      band a mode peak frequency 1khz q 100", false),
            "band `q` must be a literal",
        ),
        (
            chain("      band a mode peak q 2", false),
            "biquad_chain band syntax",
        ),
        (
            chain(
                "      band a mode peak frequency 1khz\n      band a mode notch frequency 2khz",
                false,
            ),
            "invalid or duplicated",
        ),
        (
            format!(
                "{PLUCK}\n    node fixed constant\n    node tone oscillator\n    node shaped math operation multiply\n    bus p fixed.pitch -> tone.pitch\n    bus a pluck.audio -> shaped.a\n    bus b tone.audio -> shaped.b\n{out}"
            ),
            "requires a `frequency`",
        ),
    ];
    for (graph, text) in cases {
        let diagnostics = codes(&probe(&graph));
        assert!(
            diagnostics
                .iter()
                .any(|(_, message)| message.contains(text)),
            "`{text}` missing from {diagnostics:?}"
        );
    }
    let curve = |polarity: &str, points: &str| {
        format!(
            r#"crump-module 2
instrument probe "Probe" graph {{
  polyphony 2
  parameter level "Level" ratio 0 1 0 0 modulatable
  faceplate {{
    control level "Level" "Output level" maps level
  }}
  modulation {{
    source register key_curve per_voice {polarity} {{
{points}
    }}
    route r source register target level depth 1 map {polarity} curve linear order 0 bypass false
  }}
  graph voice 4 {{
    node tone oscillator
    node gain math operation scale amount $level
    node stereo mono_to_stereo
    node out output
    bus pitch note.pitch -> tone.pitch
    bus level tone.audio -> gain.a
    bus tap gain.audio -> stereo.audio
    bus output stereo.audio -> out.audio
  }}
}}
"#
        )
    };
    for (source, text) in [
        (
            curve("unipolar", "      point a key C4 value 0.5"),
            "key curve requires 2..=64 points",
        ),
        (
            curve(
                "unipolar",
                "      point a key C4 value 0.5\n      point b key C3 value 1",
            ),
            "keys must strictly increase",
        ),
        (
            curve(
                "unipolar",
                "      point a key C3 value -0.5\n      point b key C4 value 1",
            ),
            "must lie within 0..=1 for a unipolar source",
        ),
        (
            curve(
                "bipolar",
                "      point a key C3 value 0\n      point a key C4 value 1",
            ),
            "invalid or duplicated",
        ),
    ] {
        let bundle = project_with(
            &[("instrument.crump", &source)],
            &[("probe", "probe")],
            "      note 0beat C4 1beat 0.8",
            48_000,
        );
        let diagnostics = codes(&bundle);
        assert!(
            diagnostics
                .iter()
                .any(|(_, message)| message.contains(text)),
            "`{text}` missing from {diagnostics:?}"
        );
    }
}
