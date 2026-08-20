# Crump Project Language 1

The Crump language is a small declarative format for the First Public Release Musical Sketch. It is not a scripting language: there are no loops, functions, mutable variables, arbitrary code, or hidden binary state. Exact source bytes are authoritative and are preserved unless a checked edit targets a specific literal.

A Project directory has a `project.crump` root. Imported Instrument Modules use the same syntax described below and remain readable `.crump` files.

## Complete example

[`fixtures/reference/project.crump`](../fixtures/reference/project.crump) is the acceptance fixture. It has a 34.286-second arrangement, three Tracks, two graph instruments, stock synthesized percussion, section variation, and all first-release creative processors.

A minimal outline is:

```crump
crump 1
project "Example" {
  id example
  seed 42
  sample_rate 48000
  render_tail 500ms
  song {
    tempo 120bpm
    meter 4/4
    end 8bar

    instrument lead "Lead" graph {
      polyphony 4
      parameter cutoff "Cutoff" hz 100hz 10000hz 2000hz 3200hz
      faceplate {
        control cutoff "Cutoff" "Low-pass frequency" maps cutoff
      }
      graph 8 {
        node oscillator oscillator waveform saw level 0.4
        node envelope envelope attack 5ms decay 80ms sustain 0.6 release 150ms
        node filter lowpass cutoff $cutoff resonance 0.15
        node amplifier amplifier level 0.8
        node position pan position 0
        node output output
        connect note.pitch -> oscillator.pitch
        connect note.gate -> envelope.gate
        connect oscillator.audio -> filter.audio
        connect filter.audio -> amplifier.audio
        connect envelope.control -> amplifier.gain
        connect amplifier.audio -> position.audio
        connect position.audio -> output.audio
      }
    }

    pattern melody 1bar {
      note 0beat C4 1beat 0.8
      note 1beat E4 1beat 0.75
    }

    track lead "Lead" instrument lead level -9db pan 0 {
      clip melody pattern melody start 0bar duration 8bar mode loop
    }
  }
}
```

## Headers, files, and imports

Root Projects begin with `crump 1`. A root may place quoted imports before the Project declaration:

```crump
crump 1
import "instruments/bass.crump"
project "Portable Project" {
  # ...
}
```

Imports must be normalized, relative `.crump` paths beneath the Project root. Absolute paths, `..` traversal, symlink escape, non-source extensions, duplicate canonical paths, and import cycles are rejected.

A reusable module begins with `crump-module 1` and contains exactly one Instrument:

```crump
crump-module 1
instrument bass "Pulse Bass" graph {
  # ...
}
```

Use `crump module export`, `install`, `list`, and `inspect` rather than copying into the personal library by hand. The library is resolved through the platform user data directory.

## Project and Song

A Project declares:

| Field | Meaning |
| --- | --- |
| `id` | Stable source-safe Project ID |
| `seed` | Deterministic unsigned random seed |
| `sample_rate` | `48000` in the first release |
| `render_tail` | Explicit finite effect/release tail, at most 30 seconds |

A Song declares positive finite `tempo`, a meter such as `4/4`, and an exact positive `end`. The arrangement is half-open: Events occur in `[0, end)`. Master output is the sum of simple Track level/pan balance; there are no Groups, Sends, or plug-ins in this release.

## Exact musical time

Authored Timeline values are exact rational quarter-note durations. Accepted spellings include:

- `2bar`, `1/2bar`
- `3beat`, `1/2beat`, `0.25beat`
- `3qn`, `3/2qn`

A `beat` is one quarter note; a `bar` is derived from the Song meter. Decimal input is converted to a normalized exact rational. Conversion to frames uses the fixed tempo and sample rate with deterministic half-up rounding. Clip ranges and loop expansion are half-open, preventing a boundary Event from firing twice.

DSP time settings use seconds instead: `2s`, `250ms`, or a bare finite seconds value.

## Instruments and Parameters

An Instrument is either `graph` or `stock_drums`. `polyphony` is a declared hard ceiling. Graph instruments synthesize pitched notes; stock drums synthesize `kick`, `snare`, `hat`, and `clap` without samples.

A Parameter declaration is:

```text
parameter KEY "NAME" UNIT MINIMUM MAXIMUM DEFAULT VALUE
```

The domain is inclusive. Supported units and literals are:

| Unit | Examples |
| --- | --- |
| `ratio` | `0.5`, `75%` |
| `hz` | `440hz`, `2.5khz` |
| `seconds` | `0.2s`, `200ms` |
| `db` | `-9db`, `3dB` |
| `semitones` | `-12st`, `7` |
| `pan` | `-1pan`, `0`, `1` |

Faceplate controls map a shallow musician-facing label and explanation to one exact Parameter:

```crump
faceplate {
  control tone "Tone" "Percussion brightness" maps tone
}
```

`$parameter_key` in a Node setting binds that setting to the current authored Parameter value. The setting's required physical unit must match.

## Graphs

`graph N` declares a per-voice Node ceiling; the actual graph cannot exceed it or the global release limits. Nodes have durable graph-local IDs, one native kind, and named settings. Connections are typed and directed.

First-release Nodes:

| Node | Inputs | Outputs | Settings |
| --- | --- | --- | --- |
| `oscillator` | `pitch` | `audio` | `waveform`, `detune`, `level` |
| `noise` | — | `audio` | `level` |
| `envelope` | `gate` | `control` | `attack`, `decay`, `sustain`, `release` |
| `lfo` | — | `control` | `rate`, `depth`, `waveform` |
| `mixer` | `a`, optional `b` | `audio` | `a_level`, `b_level` |
| `lowpass` | `audio`, optional `mod` | `audio` | `cutoff`, `resonance`, `mod_depth` |
| `amplifier` | `audio`, `gain` | `audio` | `level` |
| `saturation` | `audio` | `audio` | `drive` |
| `pan` | mono `audio` | stereo `audio` | `position` |
| `delay` | stereo `audio` | stereo `audio` | `time`, `feedback`, `mix` |
| `reverb` | stereo `audio` | stereo `audio` | `room`, `damping`, `mix` |
| `output` | stereo `audio` | — | — |

Special per-note sources are `note.pitch` and `note.gate`. Every required input must have exactly one compatible source, the graph must be acyclic, every authored Node must reach the one Output, and unknown settings are errors. Topological compilation is deterministic.

## Patterns, Events, Clips, and Tracks

Patterns are finite reusable spans:

```crump
pattern groove 1bar {
  note 0beat C4 1/2beat 0.8
  hit 1beat snare 0.75
}
```

A note has onset, scientific pitch (`C4`, `F#3`, `Bb2`), positive duration, and velocity in `[0, 1]`. A hit has onset, stock voice, and velocity. Source order is presentation only; runtime ordering is normalized deterministically by frame and semantic Event identity.

Tracks assign one Instrument and source-backed balance:

```crump
track lead "Lead" instrument lead level -9db pan -0.1 {
  clip verse pattern melody start 0bar duration 4bar mode loop
}
```

A Clip references one Pattern, starts on the linear Timeline, has positive duration, and uses `once` or `loop`. `once` traverses a Pattern no more than once; `loop` repeats it until the Clip's half-open end. Note Offs are clipped safely to the Clip/Song boundary.

## Validation and editing

```bash
crump validate
crump inspect --json
```

Diagnostics carry stable codes, severity, file, line, column, and byte ranges. Parsing recovers enough structure to report multiple errors in one pass.

Production View commands do not reformat a file. They compare the expected revision and literal, replace only the selected source range, validate the whole resulting Project, and atomically publish it. If source changed concurrently, the edit fails with conflict exit class `3` rather than guessing.

Arbitrary text editors and Agents may edit source directly. Such bytes remain a Candidate until an explicit successful `publish`.

## Limits

The validator enforces finite numbers, domains, unique IDs, references, graph compatibility, graph reachability, supported sample rate, maximum render tail, maximum Nodes, and maximum total graph voices. These are safety and product-scope limits, not invitations to generate at the ceiling. Use `crump check` for the stricter Reference Musical Sketch acceptance checks.
