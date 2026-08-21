# Crump Project Language 1

The Crump language is a small declarative format for the First Public Release Musical Sketch. It is not a scripting language: there are no loops, functions, mutable variables, arbitrary code, or opaque binary project document. Exact source bytes are authoritative; optional binary media is explicitly declared by portable path and exact-byte digest.

A Project directory has a `project.crump` root. Imported Instrument Modules use the same syntax described below and remain readable `.crump` files. Project-owned WAV files conventionally live under `media/`.

## Complete example

[`fixtures/reference/project.crump`](../fixtures/reference/project.crump) is the First Public Release acceptance fixture. It has a 34.286-second arrangement, three Tracks, two graph instruments, stock synthesized percussion, section variation, and all first-release creative processors.

[`fixtures/production/project.crump`](../fixtures/production/project.crump) is a separate 96-bar (3:12) Slice 7 engineering fixture. It combines an exact-hash portable Sampler asset with native graph/stock Instruments, arrangement controls, and complete routing for scale/admission/grouping checks. It carries no independent listening or named-hardware acceptance claim.

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
      parameter cutoff "Cutoff" hz 100hz 10000hz 2000hz 3200hz automatable
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

    automation lead-cutoff target instrument/lead/cutoff {
      point dark 0bar 800hz linear
      point open 4bar 8000hz power(2)
      point settle 8bar 2400hz step
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
| `asset` | Optional portable Project-owned WAV identity, described below |

### Portable WAV assets and sampler sources

A Project-level declaration is explicit source:

```crump
asset voice "Voice fragment" wav "media/voice.wav" sha256 "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
```

The path must already be a normalized UTF-8 Project-relative path with a lowercase `.wav` extension. Absolute paths, `.`/`..`, repeated separators, backslashes, `.crump/`, symbolic links (including linked parent directories), duplicate IDs, and duplicate paths are rejected. The digest is exactly 64 lowercase hexadecimal characters and identifies the complete file bytes, not decoded samples or filesystem metadata. A file is **changed** when its current exact-byte SHA-256 differs from the declaration. Distinct paths with identical bytes are allowed with a warning.

Inventory loading currently admits at most 256 assets, 64 MiB per file, and 256 MiB total. It validates non-empty mono/stereo WAV containers with a positive source rate and PCM 8/16/24/32-bit or IEEE float32 storage. Engine sample compilation happens away from the callback: it decodes to immutable stereo float frames, rejects non-finite float input, duplicates mono without a gain change, and deterministically converts source rates to the Project rate with a windowed-sinc low-pass converter. Compiled storage is bounded to 256 MiB per source. Sampler mappings retain these immutable project-rate frames; the callback only performs bounded interpolation, envelopes, fades, panning, and deterministic voice selection.

Use the checked import rather than authoring an external path:

```bash
crump --project ./song asset import ../recordings/phrase.wav \
  --id phrase --name "Spoken phrase"
```

Import reads and validates the external regular file, copies its exact bytes without replacement to `media/phrase.wav`, inserts the hash-bearing declaration, validates the staged Project, and publishes source plus binary bytes as one recoverable History transaction. External paths are command inputs only and never enter Authoritative Source. `asset collect` atomically moves already-declared files to canonical `media/ASSET_ID.wav` paths without changing exact bytes or hashes. `asset list` and `inspect` report portable path, content hash, byte length, and WAV container metadata without exposing retained bytes or host backing paths.

A Song declares positive finite `tempo`, a meter such as `4/4`, and an exact positive `end`. The arrangement is half-open: Events occur in `[0, end)`. Song-scoped Automation points may include an endpoint at `end`; that endpoint defines the approach to the boundary and is not itself an Event.

The compiled mix graph contains timeline Tracks, routing-only Groups, parallel Returns, one Master, durable pre/post-fader Sends, named processor sidechains, and locators. Tracks and Groups default to Master; Returns may feed Master, a Group, or a later Return while the complete routing graph remains acyclic. Plug-in hosting is not implemented.

## Exact musical time

Authored Timeline values are exact rational quarter-note durations. Accepted spellings include:

- `2bar`, `1/2bar`
- `3beat`, `1/2beat`, `0.25beat`
- `3qn`, `3/2qn`

A `beat` is one quarter note; a `bar` is derived from the Song meter. Decimal input is converted to a normalized exact rational. Conversion to frames uses the fixed tempo and sample rate with deterministic half-up rounding. Clip ranges and loop expansion are half-open, preventing a boundary Event from firing twice.

DSP time settings use seconds instead: `2s`, `250ms`, or a bare finite seconds value.

## Instruments and Parameters

An Instrument is `graph`, `stock_drums`, or `sampler`. `polyphony` is a declared hard ceiling. Graph instruments synthesize pitched notes; stock drums synthesize `kick`, `snare`, `hat`, and `clap`; sampler Instruments map verified Project assets to pitches and durable sample occurrences.

A sampler declares non-overlapping inclusive MIDI key zones. Source regions and fades use original WAV frames even when compilation converts the asset to the Project rate:

```crump
instrument fragments "Fragments" sampler {
  polyphony 8
  parameter gain "Gain" db -96db 24db 0db 0db
  parameter tune "Tune" semitones -24st 24st 0st 0st
  faceplate {
    control gain "Gain" "Sampler output trim" maps gain
    control tune "Tune" "Repitch in semitones" maps tune
  }
  mapping phrase asset voice keys C3 C5 root C4 {
    region 1200 49200
    playback gated
    gain -2db
    pan 0
    envelope 5ms 80ms
    fade 64 128
    choke vocal-cuts
  }
}
```

`playback one_shot` ignores mapped-note Note Off and completes the selected region; `gated` enters release on Note Off. Durable `sample` occurrences are always bounded by their authored duration. Starting a mapping releases active peers in its optional choke group. When polyphony is full, a releasing voice is replaced first, then the oldest active voice; a fixed short residual crossfade prevents an abrupt replacement edge.

A Parameter declaration is:

```text
parameter KEY "NAME" UNIT MINIMUM MAXIMUM DEFAULT VALUE [automatable] [modulatable]
```

The domain is inclusive. `automatable` explicitly admits a Parameter to absolute Song Automation. `modulatable` admits routed Parameter Modulation as well as graph Control-port use. Existing graph-local LFO connections remain graph signal flow and are not reported as authored Parameter routes. Capabilities may appear in either order. Supported units and literals are:

| Unit | Examples |
| --- | --- |
| `ratio` | `0.5`, `75%` |
| `hz` | `440hz`, `2.5khz` |
| `seconds` | `0.2s`, `200ms` |
| `db` | `-9db`, `3dB` |
| `semitones` | `-12st`, `7` |
| `pan` | `-1pan`, `0`, `1` |
| `boolean` | `false`, `true` (built-in discrete targets) |

Faceplate controls map a shallow musician-facing label and explanation to one exact Parameter:

```crump
faceplate {
  control tone "Tone" "Percussion brightness" maps tone
}
```

`$parameter_key` in a Node setting binds that setting to the Parameter's effective physical value. Without Automation this is the authored base value. The setting's required physical unit must match.

Channel controls are built-in Parameters. Tracks, Groups, and Returns expose `level`, mode-specific `pan`/`balance` or `left_pan` + `right_pan`, and step-only `mute`. The existing Track header continues to spell its backward-compatible balance literal as `pan`; an explicit `panner` directive selects `mono_pan`, `balance`, or `stereo_pan`. Mute is authoritative source state. Solo and cue exist only as transient monitoring state and never affect offline rendering.

## Song Automation

One absolute point lane may target each automatable Parameter:

```crump
automation lead-cutoff target instrument/lead/cutoff {
  point dark 0bar 800hz linear
  point open 4bar 8khz power(2)
  point settle 8bar 2400hz step
}

automation lead-ride target track/lead/level {
  point intro 0bar -18db linear
  point lift 4bar -8db step
}
```

Lane and point IDs are durable source identities. Targets use an owner path plus immutable key:

- `instrument/INSTRUMENT_ID/PARAMETER_KEY`
- `track/TRACK_ID/level|pan|balance|left_pan|right_pan|mute`
- `group/GROUP_ID/level|pan|balance|left_pan|right_pan|mute`
- `return/RETURN_ID/level|pan|balance|left_pan|right_pan|mute`
- `send/SEND_ID/level`
- `processor/PROCESSOR_ID/SETTING_KEY`
- `master/level`

A lane must contain at least one point, begin exactly at Song time zero, use strictly increasing exact times, and resolve to distinct increasing sample frames. Points must lie in `0…end`. Values are authored and interpolated in the target's canonical physical unit and must remain inside its domain.

The curve on a point describes the segment leaving that point:

- `step` holds the point value until the next point;
- `linear` uses normalized linear progress; and
- `power(P)` raises normalized progress to finite positive exponent `P` before interpolating.

The final point holds through the arrangement and Render Tail. Evaluation is sample-accurate and independent of callback block size. Boolean Mute Automation must use `step`; continuous targets may use all three curves. Pattern Parameter locks provide a narrower step override described below. The effective control order is base value → optional absolute Automation → optional Pattern Parameter lock → additive Modulation contribution → domain constraint → node-owned smoothing.

## Routed Modulation

An authored LFO route is:

```crump
modulation cutoff-motion lfo per_voice/lead bipolar sine sync 1bar target instrument/lead/cutoff depth 1200hz {
  scale 0.75
  bias 0.1
  clamp -1 1
}
```

Scopes are `global`, `instrument/INSTRUMENT_ID`, and `per_voice/INSTRUMENT_ID`. Global and Instrument phases follow absolute Song time; every per-voice phase resets when its note activates. Sine and triangle LFOs may use `free RATE` in hertz or `sync PERIOD`, where the period is the exact musical duration of one cycle. Sources explicitly choose `unipolar` (`0…1`) or `bipolar` (`-1…1`).

Operators transform the normalized source in declaration order. `add`/`bias` add a unitless scalar, `multiply`/`scale` multiply by one, and `clamp MIN MAX` constrains the route-local value. The result is multiplied by `depth` in the target's physical unit. Routes combine additively in durable route-ID order, then the Parameter domain is applied. Instrument and per-voice routes may target only their owning Instrument; per-voice routes require a graph Instrument. Inspection reports each route in deterministic order; per-voice contributions are note-relative and therefore do not pretend to have one Song-time value.

A per-voice graph Delay whose `time` is Parameter-bound changes voice-buffer allocation and is therefore rejected as an Automation or Modulation target. An authored edit recompiles and restarts the affected topology behind the protective publication fade. Other graph scalars, stock-drum tone/punch, channel fader/panner/Mute, Send gain, supported processor scalars, and Master level use the shared stable target model. Native processor presence is engineering capability; it does not by itself satisfy processor-quality or listening acceptance in `plan-production.md`.

## Graphs

`graph N` declares a per-voice Node ceiling; the actual graph cannot exceed it or the global release limits. Nodes have durable graph-local IDs, one native kind, and named settings. Connections are typed and directed.

First-release Nodes:

| Node | Inputs | Outputs | Settings |
| --- | --- | --- | --- |
| `oscillator` | `pitch` | mono `audio` | `waveform`, `detune`, `glide`, `level` |
| `unison` | `pitch`, optional `gain`, `velocity`, `mod` | stereo `audio` | `waveform`, `voices`, `detune`, `width`, `drift`, `glide`, `mod_depth`, `velocity_amount`, `level` |
| `noise` | — | `audio` | `level` |
| `envelope` | `gate` | `control` | `attack`, `decay`, `sustain`, `release` |
| `lfo` | — | `control` | `rate`, `depth`, `waveform` |
| `mixer` | `a`, optional `b` | `audio` | `a_level`, `b_level` |
| `lowpass` / `filter` | `audio`, optional `mod`, `velocity`, `key` | `audio` | `mode`, `cutoff`, `resonance`, `mod_depth`, `velocity_depth`, `key_track`, `key_reference` |
| `amplifier` | `audio`, `gain` | `audio` | `level` |
| `saturation` | `audio` | `audio` | `drive` |
| `pan` | mono `audio` | stereo `audio` | `position` |
| `delay` | stereo `audio` | stereo `audio` | `time`, `feedback`, `mix` |
| `reverb` | stereo `audio` | stereo `audio` | `room`, `damping`, `mix` |
| `output` | stereo `audio` | — | — |

Special per-note sources are `note.pitch`, `note.gate`, and normalized `note.velocity`. Every required input must have exactly one compatible source, the graph must be acyclic, every authored Node must reach the one Output, and unknown settings are errors. Topological compilation is deterministic.

A filter `mod` input accepts either uni- or bipolar Control, so an ADSR timbre envelope and an LFO use the same explicit port without weakening Audio typing. `velocity` adds `velocity_depth` hertz. `key` scales the base cutoff around `key_reference` (C4 / 261.625565 Hz by default); `key_track 1` doubles cutoff for each played-note octave. Filter modes are `lowpass`, `highpass`, `bandpass`, and `notch`; implementation and objective vectors do not substitute for the roadmap's listening selection.

`unison` owns 1–8 deterministic subvoices, equal-power stereo placement, bounded semitone spread/drift, power normalization, optional envelope gain and velocity response, and bounded Control-port pitch movement. `width 0` collapses placement to a mono-compatible center. Oscillator `glide` is a deterministic note-to-note exponential frequency slew in seconds. Unison subvoice work and the preallocated short graph-voice replacement tail are included in runtime admission. For tempo-synchronized global or per-voice movement, bind these scalar settings to Parameters and use authored Modulation routes rather than inventing callback state. The curated readable modules and their frozen abstract CPU/memory costs are documented in [Expressive native Instrument Modules](expressive-instruments.md).

## Grooves, Patterns, Events, Clips, and Tracks

A Groove Template is a reusable deterministic Clip-level timing and excitation transform:

```crump
groove shuffle grid 1/2beat swing 0.625 timing 1/32beat velocity 0.05 probability 0.95 seed 41
```

The positive `grid` defines subdivisions. `swing` is `0.5` for straight timing and may delay odd subdivisions up to `0.75`. `timing` adds a deterministic signed displacement no larger than half the grid, `velocity` adds a deterministic signed normalized displacement, and `probability` independently gates Events. Timing is kept inside the Clip's half-open range: negative displacement clamps to the Clip start, while a right-edge displacement that would leave the Clip retains that Event's authored onset instead of dropping it. Velocity is constrained to `[0, 1]`. Stable semantic Event identity plus the authored seed makes results independent of source Event order and callback grouping.

Patterns are finite reusable spans. A concrete Pattern may contain Events and durable Parameter locks:

```crump
pattern groove 1bar {
  lock open at 0beat parameter cutoff value 3200hz
  lock close at 2beat parameter cutoff value 900hz
  note 0beat C4 1/2beat 0.8
  hit 1beat snare 0.75
  sample breath 2beat mapping phrase duration 1beat start 8400 end 22800 gain -3db pan 0.15 fade 32 64 velocity 0.9
}
```

A note has onset, scientific pitch (`C4`, `F#3`, `Bb2`), positive duration, and velocity in `[0, 1]`. A hit has onset, stock voice, and velocity. A sample occurrence has a durable Pattern-local ID, sampler mapping, positive timeline duration, half-open original-source region, gain, pan, source-edge fades, and velocity. Its region is non-destructive: the Project asset bytes never change. Source order is presentation only; runtime ordering is normalized deterministically by frame and semantic Event identity.

A Pattern Variation retains procedural lineage instead of copying flattened Events:

```crump
pattern answer 1bar from groove {
  rotate 1beat
  transpose 7st
  velocity 0.8
  reverse false
  probability 0.9
  seed 77
}
```

Every directive is required. Source and derived spans must match, lineage must be acyclic, rotation lies inside the half-open span, transposition is whole-semitone, velocity scales into `[0, 1]`, and probability lies in `[0, 1]`. Reverse and rotation transform onsets; transpose affects notes; velocity affects every Event kind. Probability uses canonical semantic Event identity, so reordering source lines does not change inclusion. Variations may chain. Compilation recursively materializes them once off the realtime callback while the authored model and Event Grid retain the lineage. Inherited Parameter-lock times follow reverse and rotation, but Event probability never removes locks.

A Parameter lock is a physical-unit step override for the consuming Track's assigned Instrument Parameter. Its onset lies inside the Pattern span, the key must resolve to a topology-stable `automatable` Parameter, and the value must match that Parameter's unit and domain. A lock takes precedence over Song Automation, then routed Modulation remains additive. It follows source offset, looping, and Variation timing and resets at the Clip end. To keep precedence unambiguous, Clips that share one Instrument may not overlap when either contains locks.

Tracks assign one Instrument and begin with backward-compatible level/balance values. Their bodies may add an explicit panner, ordered processors, Sends, Mute, main output, and Clips:

```crump
track lead "Lead" instrument lead level -9db pan -0.1 {
  panner stereo_pan -0.8 0.8
  processor lead_eq eq {
    setting high_pass 80hz
    setting mid1_frequency 1.2khz
    setting mid1_gain -2db
    setting mid1_q 1
  }
  send lead_room target return/room tap post level -15db
  mute false
  output group/music
  clip verse pattern melody start 0bar duration 4bar mode loop {
    offset 1beat
    transpose 7st
    gain -3db
    fade 1/8beat 1/8beat
    groove shuffle
  }
}
```

`tap pre|post` is accepted shorthand for the explicit `pre_fader|post_fader` model. A Clip references one Pattern, starts on the linear Timeline, has positive duration, and uses `once` or `loop`. `once` traverses a Pattern no more than once; `loop` repeats it until the Clip's half-open end. The optional source offset is Pattern-local and half-open. Whole-semitone transpose, event-excitation gain, left/right fades, and a Groove assignment remain placement-local, so repeated material can share one Pattern while retaining explicit local treatment. Note Offs are clipped safely to the Clip/Song boundary.

Groups and Returns use the same channel directives; Master owns an ordered processor chain and final level:

```crump
group music "Music" level 0db {
  output master
}
return room "Room" level -6db {
  processor room_fx reverb {
    setting mix 1
  }
  output master
}
sidechain duck from track/kick tap post to processor/bass_comp/detector
master level -1db {
  processor ceiling limiter {
    setting ceiling -1db
  }
}
locator chorus "Chorus" at 9bar
```

A Locator is a Song-wide durable ID, human-facing label, and exact time in `0…end`. Locators name navigation and audition boundaries and can be supplied to checked CLI time operands; moving one does not implicitly move Clips or Automation.

Native processor kinds are `utility`, `eq`, `compressor`, `delay`, `reverb`, `saturation`, `limiter`, and the testing/compensation `latency` kind. A processor may declare `setting KEY VALUE` and deterministic `latency FRAMES`; `compressor` alone may declare at most one `sidechain input NAME`. The compressor is linked stereo and feed-forward: `peak` detects the larger absolute channel, `rms` detects the instantaneous stereo RMS, and a deterministic one-pole amplitude envelope applies the authored attack/release before the threshold/ratio/soft-knee curve. An external sidechain replaces program Audio only as the detector source; makeup applies to the compressed wet path before the exact parallel `mix`. Every declared sidechain input requires exactly one route, and every sidechain route targets exactly one declared `processor/ID/INPUT`; disconnected declarations and undeclared destinations are rejected by both validation and compilation.

Delay supports free time or deterministic tempo sync:

```crump
processor slap delay {
  setting time 95ms
  setting feedback 0.3
  setting low_pass 7.5khz
  setting high_pass 120hz
  setting time_crossfade 10ms
  setting mode stereo
}
processor dotted_echo delay {
  setting tempo_sync true
  setting division 1/8d
  setting mode ping_pong
}
```

`division` accepts `1/1`, `1/2`, `1/4`, `1/8`, `1/16`, or `1/32`, with optional `d` dotted or `t` triplet suffix. It is legal only when `tempo_sync true`; synchronized Delay rejects a separately authored `time`. Fractional times use deterministic linear interpolation, feedback passes through the authored high/low filters, and `time_crossfade` (`1ms…100ms`, default `10ms`) moves between read heads without reallocating the pre-sized Delay buffer. `stereo` feeds each channel back to itself; `ping_pong` cross-feeds left and right.

Reverb uses fixed preallocated stereo comb and gain-neutral allpass networks. `pre_delay` is `0s…1s`; `diffusion`, `decay`, `damping`, `modulation`, `width`, and `mix` are ratios in `0…1`. Pre-delay uses fractional interpolation and a fixed 10 ms read-head crossfade for continuous changes. Diffusion changes the bounded allpass coefficient rather than output gain, decay controls bounded comb feedback, damping low-passes recirculated Audio, and modulation moves fractional comb taps by at most eight frames at 48 kHz with deterministic phase offsets. A fixed input-headroom coefficient keeps sustained network buildup bounded. Stereo input is cross-fed without discarding side-only material. Width scales only the wet side component, so width zero is exactly mono and fold-down preserves the wet mid. Put one wet Reverb on a Return and route multiple Sends to it when the sources must share one continuing room state.

Accepted settings are validated against the exact implemented kind-specific schema—unknown settings and runtime no-ops are rejected. Compiled inspection resolves authored values and effective defaults with units/domains, separate authored-literal and continuous-Parameter-target provenance, explicit versus intrinsic latency, nonlinear evaluations, the filtered oversampling factor/tap count, and state/work admission facts. Saturation and Limiter share one fixed 4×, 129-tap linear-phase FIR kernel. Saturation filters both interpolation and decimation, evaluates its selected curve four times per input frame, aligns the dry path, and introduces 32 frames of intrinsic latency. Limiter reconstructs inter-sample peaks through the same kernel, adds the filter's 16-frame detector latency to authored lookahead, attacks immediately, and releases with the authored time constant. Lookahead changes allocated delay topology, so it is an authored structural literal rather than an automatable/modulatable Parameter target. Inspection publishes the Limiter's fixed 0.1 dB true-peak safety margin; neither processor applies hidden normalization or a universal loudness target. Utility width is correlation-preserving mid/side narrowing in `0…1`; it preserves the mid signal while scaling side energy, and widening is not silently accepted. EQ and Delay filter-frequency domains are `10hz…23.52khz` at the fixed 48 kHz Project rate, keeping authored targets below Nyquist without a hidden runtime clamp.

## Validation and editing

```bash
crump validate
crump --json inspect --at 2bar
```

Diagnostics carry stable codes, severity, file, line, column, and byte ranges. Parsing recovers enough structure to report multiple errors in one pass.

Production View commands do not reformat a file. They compare the expected revision and literal, replace only the selected source range, validate the whole resulting Project, and atomically publish it. If source changed concurrently, the edit fails with conflict exit class `3` rather than guessing.

Arbitrary text editors and Agents may edit source directly. Such bytes remain a Candidate until an explicit successful `publish`.

## Limits

The validator enforces finite numbers, domains, unique IDs, references, portable asset identities and exact hashes, graph compatibility/reachability, acyclic legal channel routing, Send/sidechain destinations, panner modes, processor schemas and latency, supported sample rate, maximum render tail, maximum Nodes, at most 256 media assets within the declared byte ceilings, at most 256 graph voices across actual runtime Track instances, at most 128 Automation lanes, at most 1,024 points per lane, at most 8,192 Automation points per Project, at most 8,192 materialized Parameter-lock points, and at most 512 dense callback-time Parameter targets. It also rejects duplicate targets, non-automatable or allocation-changing targets, non-step boolean Automation, ambiguous same-frame points, conflicting lock-bearing Clips, out-of-domain physical values, and Projects exceeding named compiled runtime work/memory budgets before allocation. These are safety and product-scope limits, not invitations to generate at the ceiling. Use `crump check` for the narrower Reference Musical Sketch acceptance checks.
