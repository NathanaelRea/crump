#!/usr/bin/env python3
"""Regenerate the readable qualification-max-v1 alpha source snapshot."""

from __future__ import annotations

import argparse
import sys
from collections.abc import Sequence
from pathlib import Path

ASSETS = """asset wt-basic \"Basic four-frame table\" wav \"media/basic-table.wav\" sha256 \"2302bf70124b6017439eb385a62677b13284f112e968a2725d64e8f738bf3ca7\" {
  usage wavetable
  frame_length 2048
  frames 4
  normalization preserve
  morph linear
}
asset source-texture \"Deterministic source texture\" wav \"media/sample-source.wav\" sha256 \"ae8757e2a42ad7602b7bd6825ab0987444677a8e7f8515d0447e86aa01907de9\" {
  usage sample granular spectral
}
asset zone-low \"Low multisample zone\" wav \"media/multisample-low.wav\" sha256 \"5675332919d951ff59291a90b991e596a443e5b368b4360dfca3b157e161a0e0\" {
  usage multisample
}
asset zone-high \"High multisample zone\" wav \"media/multisample-high.wav\" sha256 \"1e3309e6b805299f66308bd603d304387cba4e7affadf63f57d6835e82d5345d\" {
  usage multisample
}
"""


def indented(lines: Sequence[str], spaces: int = 2) -> str:
    prefix = " " * spaces
    return "\n".join(f"{prefix}{line}" if line else "" for line in lines)


def build() -> str:
    lines = ['instrument qualification-max-v1 "Qualification Maximum V1" graph {']
    lines.extend(
        [
            "  polyphony 7",
            "  voicing poly retrigger always glide 50ms priority oldest seed 65531",
            "",
        ]
    )

    for index in range(1, 9):
        lines.append(
            f'  parameter macro-{index} "Macro {index}" ratio 0 1 0.5 0.5 automatable modulatable macro'
        )
    for index in range(1, 9):
        lines.append(
            f'  parameter cutoff-{index} "Cutoff {index}" hz 20hz 20khz 800hz {800 + index * 200}hz automatable modulatable'
        )
    lines.extend(
        [
            '  parameter table-position "Table Position" ratio 0 1 0 0.25 modulatable',
            '  parameter grain-density "Grain Density" hz 1hz 100hz 24hz 24hz modulatable',
            '  parameter spectral-scan "Spectral Scan" ratio 0 1 0 0.2 modulatable',
            '  parameter release "Release" seconds 5ms 8s 500ms 750ms modulatable',
            "",
            "  faceplate {",
            '    control macro-1 "Motion" "Scales maximum-profile movement" maps macro-1',
            '    control cutoff-1 "Cutoff" "Sets the first serial filter" maps cutoff-1',
            '    control grain-density "Density" "Sets the two bounded grain pools" maps grain-density',
            '    control release "Release" "Sets the maximum-profile amplitude release" maps release',
            "  }",
            "",
            "  presentation {",
            '    section sources-view sources "Eight Sources" {',
            "      node source-analog",
            "      node source-table",
            "      node source-sample",
            "      node source-multi",
            "      node source-grain-a",
            "      node source-grain-b",
            "      node source-spectral",
            "      node source-noise",
            "    }",
            '    section mixer-view source_mixer "Source Sum" {',
            "      node source-sum",
            "    }",
            '    section filters-view filters "Eight Filters" {',
            "      node filter-1",
            "      node filter-8",
            "    }",
            '    section modulators-view envelopes_lfos "Eight Envelopes and Sixteen LFOs" {',
            "      node envelope-1",
            "      node envelope-8",
            "      source lfo-1",
            "      source lfo-16",
            "    }",
            '    section matrix-view modulation "Sixty-four Routes" {',
            "      route route-1",
            "      route route-64",
            "    }",
            '    section voice-view voicing "Seven Resident Voices" {',
            "      node source-analog",
            "      node source-table",
            "    }",
            '    section routing-view routing "Voice and Shared Domains" {',
            "      bus voice-out",
            "      bus voices-in",
            "    }",
            '    section effects-view effects "Shared Effects" {',
            "      node shared-delay-1",
            "      node shared-reverb-3",
            "    }",
            "  }",
            "",
            "  modulation {",
        ]
    )
    for index in range(1, 17):
        waveform = "sine" if index % 2 else "triangle"
        lines.append(
            f"    source lfo-{index} lfo per_voice bipolar {waveform} sync {index}/16bar phase 0 retrigger note loop forward"
        )
    for index in range(1, 9):
        lines.append(
            f"    source macro-{index} parameter macro-{index} instrument unipolar"
        )
    for index in range(1, 65):
        lfo = (index - 1) % 16 + 1
        cutoff = (index - 1) % 8 + 1
        curve = "smoothstep" if index % 3 == 0 else "linear"
        source = "macro-1" if index == 1 else f"lfo-{lfo}"
        mapping = "unipolar" if index == 1 else "bipolar"
        lines.append(
            f"    route route-{index} source {source} target cutoff-{cutoff} depth 500hz map {mapping} curve {curve} order {index} bypass false"
        )
    lines.extend(["  }", "", "  graph voice 64 {"])
    lines.extend(
        [
            "    node source-analog analog_oscillator waveform saw unison 8 detune 0.15st width 1 phase 0 random_phase 1 level 0.12",
            "    node source-table wavetable asset wt-basic unison 8 table_position $table-position detune 0.1st width 1 phase 0 random_phase 1 level 0.12",
            "    node source-sample sample_source asset source-texture start 0 end 1 loop_mode forward loop_start 0.1 loop_end 0.9 loop_crossfade 256 direction forward tune 0st level 0.08",
            "    node source-multi multisample {",
            "      zone low asset zone-low keys C1 B3 velocity 0 1 root C3 layer body articulation default round_robin low-rr 0 region 0 24000 gain -6db pan -0.2",
            "      zone high asset zone-high keys C4 C7 velocity 0 1 root C5 layer body articulation default round_robin high-rr 0 region 0 24000 gain -6db pan 0.2",
            "      tune 0st",
            "      level 0.08",
            "    }",
            "    node source-grain-a granular asset source-texture max_grains 32 position 0.25 scan 0.1 size 80ms density $grain-density tune 0st spray 0.1 window hann edge reflect direction_probability 0.25 stereo_spread 1 freeze false seed 1 level 0.08",
            "    node source-grain-b granular asset source-texture max_grains 32 position 0.65 scan -0.08 size 120ms density $grain-density tune 7st spray 0.1 window tukey edge wrap direction_probability 0.5 stereo_spread 1 freeze false seed 2 level 0.08",
            "    node source-spectral spectral asset source-texture algorithm crump-spectral-peaks-v1 max_peaks 128 scan $spectral-scan rate 0.5 tune 12st low 80hz high 18khz transients reset phase_randomization 0.1 freeze false level 0.08",
            "    node source-noise noise level 0.02",
            "    node noise-stereo mono_to_stereo",
        ]
    )
    for index in range(1, 9):
        release = "$release" if index == 1 else f"{100 + index * 50}ms"
        lines.append(
            f"    node envelope-{index} envelope mode ahdsr attack {index}ms hold {index - 1}ms decay {80 + index * 20}ms sustain 0.5 release {release} attack_curve 0 decay_curve 0 release_curve 0 retrigger always"
        )
    lines.append("    node source-sum stereo_sum")
    for index in range(1, 9):
        mode = ["lowpass", "highpass", "bandpass", "notch"][(index - 1) % 4]
        lines.append(
            f"    node filter-{index} stereo_filter model svf_clean mode {mode} cutoff $cutoff-{index} resonance 0.2 drive 0 mix 1 pan 0 mod_depth 500hz key_track 0 velocity_depth 0hz"
        )
    lines.extend(
        [
            "    node voice-output output",
            "",
            "    bus pitch-analog note.pitch -> source-analog.pitch",
            "    bus pitch-table note.pitch -> source-table.pitch",
            "    bus pitch-sample note.pitch -> source-sample.pitch",
            "    bus pitch-multi note.pitch -> source-multi.pitch",
            "    bus pitch-grain-a note.pitch -> source-grain-a.pitch",
            "    bus pitch-grain-b note.pitch -> source-grain-b.pitch",
            "    bus pitch-spectral note.pitch -> source-spectral.pitch",
            "    bus gate-sample note.gate -> source-sample.gate",
            "    bus gate-multi note.gate -> source-multi.gate",
            "    bus velocity-multi note.velocity -> source-multi.velocity",
            "    bus gate-grain-a note.gate -> source-grain-a.gate",
            "    bus gate-grain-b note.gate -> source-grain-b.gate",
            "    bus gate-envelope-1 note.gate -> envelope-1.gate",
        ]
    )
    for index in range(2, 9):
        lines.append(
            f"    bus gate-envelope-{index} note.gate -> envelope-{index}.gate"
        )
    for name in [
        "analog",
        "table",
        "sample",
        "multi",
        "grain-a",
        "grain-b",
        "spectral",
    ]:
        lines.append(f"    bus {name}-sum source-{name}.audio -> source-sum.inputs")
    lines.extend(
        [
            "    bus noise-convert source-noise.audio -> noise-stereo.audio",
            "    bus noise-sum noise-stereo.audio -> source-sum.inputs",
            "    bus sum-filter-1 source-sum.audio -> filter-1.audio",
            "    bus envelope-filter-1 envelope-1.control -> filter-1.mod",
        ]
    )
    for index in range(2, 9):
        lines.append(
            f"    bus envelope-filter-{index} envelope-{index}.control -> filter-{index}.mod"
        )
    for index in range(1, 8):
        lines.append(
            f"    bus filter-{index}-filter-{index + 1} filter-{index}.audio -> filter-{index + 1}.audio"
        )
    lines.extend(
        [
            "    bus voice-out filter-8.audio -> voice-output.audio",
            "  }",
            "",
            "  graph shared 64 {",
            "    node shared-entry stereo_sum",
            "    node shared-sum stereo_sum",
        ]
    )
    for index in range(1, 5):
        lines.append(
            f"    node shared-delay-{index} delay time {index * 10}ms feedback 0.2 mix 0.1"
        )
        if index < 4:
            lines.append(
                f"    node shared-reverb-{index} reverb room 0.6 damping 0.5 mix 0.1"
            )
    lines.extend(
        [
            "    node shared-output output",
            "    bus voice-sum-in voice_sum.audio -> shared-entry.inputs",
            "    bus voices-in shared-entry.audio -> shared-sum.inputs",
        ]
    )
    for index in range(1, 5):
        lines.append(
            f"    bus voices-delay-{index} shared-entry.audio -> shared-delay-{index}.audio"
        )
        if index < 4:
            lines.append(
                f"    bus delay-reverb-{index} shared-delay-{index}.audio -> shared-reverb-{index}.audio"
            )
            lines.append(
                f"    bus reverb-sum-{index} shared-reverb-{index}.audio -> shared-sum.inputs"
            )
        else:
            lines.append(
                "    bus delay-sum-4 shared-delay-4.audio -> shared-sum.inputs"
            )
    lines.extend(
        ["    bus shared-out shared-sum.audio -> shared-output.audio", "  }", "}"]
    )
    project = [
        "crump 2",
        'project "Native Synthesis Qualification Maximum" {',
        "  id native-synthesis-qualification-max",
        "  seed 65531",
        "  sample_rate 48000",
        "  render_tail 5ms",
        indented(ASSETS.rstrip().splitlines(), 2),
        "  song {",
        "    tempo 120bpm",
        "    meter 4/4",
        "    end 1bar",
        indented(lines, 4),
        "    pattern maximum-chord 1bar {",
    ]
    for pitch in ["C2", "G2", "C3", "E3", "G3", "C4", "E4", "G4"]:
        project.append(f"      note 0beat {pitch} 1bar 0.7")
    project.extend(
        [
            "    }",
            '    track maximum-track "Qualification Maximum" instrument qualification-max-v1 level -18db pan 0 {',
            "      clip maximum-phrase pattern maximum-chord start 0bar duration 1bar mode once",
            "    }",
            "  }",
            "}",
        ]
    )
    return "\n".join(project) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    path = Path(__file__).with_name("maximum-topology.crump")
    generated = build()
    if args.check:
        if not path.exists() or path.read_text(encoding="utf-8") != generated:
            print(f"{path} is stale; regenerate it without --check", file=sys.stderr)
            raise SystemExit(1)
        return
    path.write_text(generated, encoding="utf-8")


if __name__ == "__main__":
    main()
