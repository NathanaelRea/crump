# Crump Project Language 1

The Crump language is a small declarative Project format. It is not a scripting language: there are no functions, mutable variables, arbitrary code, or opaque binary project document. Exact source bytes are authoritative; binary media is explicitly declared by portable path and exact-byte digest.

A Project directory has a `project.crump` root. Imported Instrument Modules use the same syntax described below and remain readable `.crump` files. Project-owned WAV files conventionally live under `media/`.

## Complete example

[`fixtures/reference/project.crump`](../fixtures/reference/project.crump) is a compact end-to-end fixture. It has a 34.286-second arrangement, three Tracks, two graph Instruments, stock synthesized percussion, section variation, and native creative processors.

[`fixtures/production/project.crump`](../fixtures/production/project.crump) is a 96-bar (3:12) resource and routing fixture. It combines an exact-hash portable Sampler asset with native graph/stock Instruments, arrangement controls, and complete routing for scale, admission, and callback-grouping checks.

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

Root Projects begin with stable `crump 1` or `crump 2`; the header must be the first non-comment declaration. A root may place quoted imports before the Project declaration:

```crump
crump 1
import "instruments/bass.crump"
project "Portable Project" {
  # ...
}
```

Imports must be normalized, relative `.crump` paths beneath the Project root. Absolute paths, `..` traversal, symlink escape, non-source extensions, duplicate canonical paths, and import cycles are rejected.

A reusable module begins with `crump-module 1` or `crump-module 2` as its first non-comment declaration and contains exactly one Instrument:

```crump
crump-module 1
instrument bass "Pulse Bass" graph {
  # ...
}
```

A `crump-module 2` module may also own the Media Assets its Instrument plays, declared at top level with the ordinary [asset syntax](#portable-wav-assets-sampler-sources-and-wavetables):

```crump
crump-module 2
instrument violin "Violin" graph {
  # ... zones referencing violin-g3 ...
}
asset violin-g3 "Violin" wav "media/g3.wav" sha256 "..." {
  usage multisample
}
```

A module asset path is relative to the module's directory and must stay beneath it; importing `instruments/violin/instrument.crump` resolves the asset above to `instruments/violin/media/g3.wav`. Module assets otherwise behave exactly like Project assets: they share one ID namespace and the same budgets, and their exact bytes are part of the Revision. Copying a module directory with its media into another Project therefore needs only the `import`. `asset collect` leaves module media in place. A personal-library module is a single source file, so a `library:` import or `module install` of a module that owns assets fails. `module export` copies only the Instrument declaration; as before, its asset references must then be satisfied by the importing Project. [ADR 0025](adr/0025-module-owned-media-assets.md) records the decision.

A version-1 root imports only version-1 modules. A version-2 root may import version-1 or version-2 modules. Unsupported versions and suffixes such as `2-alpha` fail closed. Use `crump module export`, `install`, `list`, and `inspect` rather than copying into the personal library by hand. The library is resolved through the platform user data directory.

## Project and Song

A Project declares:

| Field | Meaning |
| --- | --- |
| `id` | Stable source-safe Project ID |
| `seed` | Deterministic unsigned random seed |
| `sample_rate` | fixed at `48000` |
| `render_tail` | Explicit finite effect/release tail, at most 30 seconds |
| `asset` | Optional portable Project-owned WAV identity, described below; an imported module may also own assets |

### Portable WAV assets, sampler sources, and wavetables

A Project-level declaration is explicit source:

```crump
asset voice "Voice fragment" wav "media/voice.wav" sha256 "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
```

The path must already be a normalized UTF-8 Project-relative path with a lowercase `.wav` extension. Absolute paths, `.`/`..`, repeated separators, backslashes, `.crump/`, symbolic links (including linked parent directories), duplicate IDs, and duplicate paths are rejected. The digest is exactly 64 lowercase hexadecimal characters and identifies the complete file bytes, not decoded samples or filesystem metadata. A file is **changed** when its current exact-byte SHA-256 differs from the declaration. Distinct paths with identical bytes are allowed with a warning.

A source-v2 asset may declare an exact periodic-frame interpretation:

```crump
asset table "Four-frame table" wav "media/table.wav" sha256 "..." {
  usage wavetable
  frame_length 2048
  frames 4
  normalization global_peak
  morph catmull_rom
}
```

A wavetable WAV is mono and contains exactly `frame_length × frames` consecutive decoded samples. Frame length is a power of two from 32 through 8,192; frame count is 1 through 256; one asset may interpret at most 2,097,152 samples. `preserve` retains source amplitude, `global_peak` scales all frames by one complete-table peak, and `frame_peak` scales each non-silent frame independently. Frame order is exact source order. `linear` interpolates adjacent frames; `catmull_rom` uses a four-frame cubic with repeated boundary frames. The container sample rate does not retime these periodic cycles.

Graph sample assets use the same exact Media Asset declaration with one or more source-v2 usage tags:

```crump
asset texture "Texture" wav "media/texture.wav" sha256 "..." {
  usage sample multisample granular spectral
}
```

`sample` admits a direct graph sample source; `multisample` admits readable zone maps; `granular` admits bounded grain reads; and `spectral` admits versioned off-callback analysis. These tags do not copy, trim, normalize, or reinterpret bytes. Verified source container frames remain the coordinate space for slices/zones. Only assets reached by Track-assigned sampler or graph programs compile to immutable Project-rate stereo frames and, for spectral sources, a second disposable analysis derivative.

Inventory loading currently admits at most 1,024 assets, 64 MiB per file, and 1 GiB total. It validates non-empty mono/stereo WAV containers with a positive source rate and PCM 8/16/24/32-bit or IEEE float32 storage. Engine sample compilation happens away from the callback: it decodes to immutable float frames that keep mono sources mono (read as two identical channels without a gain change), rejects non-finite float input, and deterministically converts source rates to the Project rate with a windowed-sinc low-pass converter. Compiled storage is bounded to 256 MiB per source. Sampler mappings retain these immutable project-rate frames; the callback only performs bounded interpolation, envelopes, fades, panning, and deterministic voice selection.

Use a checked import rather than authoring an external path:

```bash
crump --project ./song asset import ../recordings/phrase.wav \
  --id phrase --name "Spoken phrase"
crump --project ./song asset import-wavetable ../tables/four-frame.wav \
  --id table --name "Four-frame table" --frame-length 2048 --frames 4 \
  --normalization global-peak --morph catmull-rom
```

Both imports read and validate an external regular file, copy its exact bytes without replacement to `media/ID.wav`, insert the hash-bearing declaration, validate the staged Project, and publish source plus binary bytes as one recoverable History transaction. Wavetable import additionally requires stable source version 2, mono input, exact declared sample count, and bounded complete interpretation metadata. External paths are command inputs only and never enter Authoritative Source. `asset collect` atomically moves already-declared files to canonical `media/ASSET_ID.wav` paths without changing exact bytes or hashes. `asset list` and `inspect` report portable path, content hash, byte length, WAV container metadata, and authored wavetable interpretation without exposing retained bytes or host backing paths.

An exact Active range can instead be rendered to a new source-backed asset:

```bash
crump --project ./song asset bounce --source track/vocal:post_fader \
  --from verse --to chorus --tail-seconds 1.5 --id vocal-tail --name "Vocal tail"
```

Bounce performs deterministic preroll, captures `master` or one explicit Track/Group/Return pre/post-fader tap, stops source dispatch at the range boundary for an optional isolated release/effect tail, and stages stereo float32 WAV bytes. It then publishes those bytes and a readable declaration atomically through History. It never replaces Clips, disables processors, or flattens editable source. A generated declaration contains an inspectable `generated` block with the exact source Revision, owner, tap, half-open frame range, tail frames, format, versioned algorithm, and Crump build; the ordinary asset hash still identifies the complete WAV bytes. Bounce requires the current Candidate to equal the exact Active Revision.

One exact asset region can instead be prepared with independent output duration and pitch:

```bash
crump --project ./song asset prepare --source voice \
  --from-frame 0 --to-frame 48000 --output-frames 96000 \
  --pitch-semitones 0 --formants shift --quality effect-v1 \
  --id slowed-voice --name "Slowed voice"
```

`output_frames` is the authoritative generated duration at the Project sample rate; a ratio is never retained as an implicit rounding rule. `pitch_semitones` is independently bounded to −24…+24. Preparation uses the versioned `crump-wsola-effect-v1` algorithm in deterministic domain `x86_64-linux`. Its only admitted formant policy is `shift`: `preserve` is recognized and rejected as unavailable rather than silently approximated. The algorithm makes no claim about exposed-vocal quality.

Prepared assets use tagged readable provenance:

```crump
asset slowed-voice "Slowed voice" wav "media/slowed-voice.wav" sha256 "..." {
  generated sample_transform {
    revision "..."
    source_asset voice
    source_sha256 "..."
    source_range 0 48000
    output_frames 96000
    pitch 0
    formants shift
    quality effect_v1
    format stereo_f32_wav
    algorithm crump-wsola-effect-v1
    domain "x86_64-linux"
    build "0.1.0"
  }
}
```

Decode, conversion, correlation, overlap-add preparation, resampling, and encoding happen off the callback with a fixed working-memory ceiling. Source and generated bytes publish atomically through History. The original asset remains unchanged, no implicit tail is added, and no Clip is placed or replaced automatically. Existing untagged `generated { ... }` blocks remain the legacy spelling of `bounce` provenance. These choices are fixed by [ADR 0012](adr/0012-prepared-sample-transform-contract.md).

A Song declares positive finite `tempo`, a meter such as `4/4`, and an exact positive `end`. The arrangement is half-open: Events occur in `[0, end)`. Song-scoped Automation points may include an endpoint at `end`; that endpoint defines the approach to the boundary and is not itself an Event.

The compiled mix graph contains timeline Tracks, routing-only Groups, parallel Returns, one Master, durable pre/post-fader Sends, named processor sidechains, and locators. Tracks and Groups default to Master; Returns may feed Master, a Group, or a later Return while the complete routing graph remains acyclic. Crump does not host plug-ins.

## Exact musical time

Authored Timeline values are exact rational quarter-note durations. Accepted spellings include:

- `2bar`, `1/2bar`
- `3beat`, `1/2beat`, `0.25beat`
- `3qn`, `3/2qn`

A `beat` is one quarter note; a `bar` is derived from the Song meter. Decimal input is converted to a normalized exact rational. Conversion to frames uses the fixed tempo and sample rate with deterministic half-up rounding. Clip ranges and loop expansion are half-open, preventing a boundary Event from firing twice.

DSP time settings ordinarily use seconds (`2s`, `250ms`, or a bare finite seconds value). In source version 2, graph AHDSR `attack`, `hold`, `decay`, and `release` also accept exact musical periods such as `1/8beat` or `1/2bar`; compilation resolves them with the owning Song meter, tempo, and sample rate.

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
    direction reverse
    loop 9600 38400 crossfade 256
    gain -2db
    pan 0
    envelope 5ms 80ms
    fade 64 128
    choke vocal-cuts
  }
}
```

`direction` is `forward` by default or explicitly `reverse`. `loop START END crossfade FRAMES` is a bounded half-open original-source region inside the mapping region; its crossfade cannot exceed half the loop. Looped mappings require `playback gated`, ensuring Note Off can stop them. `playback one_shot` otherwise ignores mapped-note Note Off and completes the selected region; `gated` enters release on Note Off. Durable `sample` occurrences are always bounded by their authored duration. Starting a mapping releases active peers in its optional choke group. When polyphony is full, a releasing voice is replaced first, then the oldest active voice; a fixed short residual crossfade prevents an abrupt replacement edge. Decode, sample-rate conversion, and loop preparation remain off the callback; traversal and crossfade use preallocated voice state only.

Vocals, ambience, tempo-aligned loops, and rendered stems use this same immutable-source policy; there is no hidden streaming or callback decode path. Each complete WAV must fit the per-asset, aggregate Project, and compiled-source limits stated above or compilation is rejected. Long phrases and ambience use explicit fixed source-frame regions. A tempo-aligned loop must already have the intended duration (or be prepared as a new declared asset), because Crump does not silently stretch it to the fixed Song tempo. Bounce tails and rendered stems likewise remain ordinary bounded generated assets. No source is chunked, truncated, or substituted automatically.

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
| `bits` | `8bit`, `12bits` |
| `pan` | `-1pan`, `0`, `1` |
| `boolean` | `false`, `true` (built-in discrete targets) |

Faceplate controls map a shallow musician-facing label and explanation to one exact Parameter:

```crump
faceplate {
  control tone "Tone" "Percussion brightness" maps tone
}
```

Source-version-2 graph Instruments may additionally declare bounded, Track-instance voicing:

```crump
voicing poly retrigger always glide 40ms glide_mode always \
  note_priority last steal_priority release_oldest seed 41041
```

`poly` allocates independent slots up to `polyphony`; `mono` requires `polyphony 1` and selects held notes by `last`, `low`, or `high`. Polyphonic stealing chooses a releasing oldest slot before an active oldest slot with `release_oldest`, or the active oldest slot directly with `active_oldest`; stable slot index breaks ties. Mono `legato` retains general graph state while switching held notes, although a Node's more-specific `retrigger always`/`retrigger note` policy still restarts that envelope/LFO; voicing `always` restarts the complete voice behind the preallocated replacement fade. Central glide is linear in log frequency, may apply `off`, `always`, or only to overlapping `legato` transitions, and is independent of optional oscillator-local drift/glide. Note-Off restores the selected surviving held note. Each Track remembers at most 64 simultaneously held mono Note-On identities; on saturation, the oldest identity is evicted deterministically, and its later Note-Off has no effect. Held-note storage, replacement tails, and work are fixed at compilation, deterministic from the authored seed and note-event identity, and owned separately by every referencing Track runtime.

Sound-neutral `presentation` metadata groups stable declarations into generated Instrument sections without becoming DSP state:

```crump
presentation {
  section source-panel sources "Sources" {
    node carrier
  }
  section matrix-panel modulation "Modulation" {
    source motion
    route motion-cutoff
    parameter cutoff
  }
}
```

Section kinds are `sources`, `source_mixer`, `filters`, `envelopes_lfos`, `modulation`, `voicing`, `routing`, and `effects`. Items are `node`, `bus`, `parameter`, `source`, `route`, `processor`, or Project `asset` IDs. References and limits (16 sections, 256 items) validate before publication, retain exact source sites, and may intentionally repeat an item in different sections. The Workspace preserves authored section order, generates every missing first-release section, appends unassigned custom topology deterministically, opens exact source locations, and uses checked edits for Parameters and route depth/bypass. Presentation-only edits do not restart audio state.

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

The final point holds through the arrangement and Render Tail. Evaluation is sample-accurate and independent of callback block size. Boolean Mute Automation must use `step`; continuous targets may use all three curves. Pattern Parameter locks provide a narrower step override described below. The effective control order is base value → optional absolute Automation → optional Pattern Parameter lock → Instrument-owned Modulation routes → Song-owned Modulation/Envelope Follower contributions → domain constraint → node-owned smoothing.

## Routed Modulation

An authored LFO route is:

```crump
modulation cutoff-motion lfo per_voice/lead bipolar sine sync 1bar target instrument/lead/cutoff depth 1200hz {
  scale 0.75
  bias 0.1
  clamp -1 1
}
```

Scopes are `global`, `instrument/INSTRUMENT_ID`, and `per_voice/INSTRUMENT_ID`. Global and Instrument phases follow absolute Song time; global state is evaluated once for the Song, while Instrument state is owned separately by every Track runtime instance and includes durable Track identity. Every per-voice phase resets when its note activates and continues through its release. Sine and triangle LFOs may use `free RATE` in hertz or `sync PERIOD`, where the period is the exact musical duration of one cycle. Sources explicitly choose `unipolar` (`0…1`) or `bipolar` (`-1…1`).

Operators transform the normalized source in declaration order. `add`/`bias` add a unitless scalar, `multiply`/`scale` multiply by one, and `clamp MIN MAX` constrains the route-local value. The result is multiplied by `depth` in the target's physical unit. Routes combine additively in durable route-ID order, then the Parameter domain is applied. Instrument and per-voice routes may target only their owning Instrument; per-voice routes require a graph Instrument. Inspection reports each route in deterministic order; per-voice contributions are note-relative and therefore do not pretend to have one Song-time value.

### Instrument-owned Modulation (source version 2)

A graph Instrument may carry portable sources and routes with the patch:

```crump
parameter motion "Motion" ratio 0 1 0.5 0.5 automatable modulatable macro
parameter cutoff "Cutoff" hz 20hz 20khz 800hz 1200hz modulatable
modulation {
  source motion-source parameter motion instrument unipolar
  source envelope-source port envelope.control per_voice unipolar
  source velocity note_velocity per_voice unipolar
  source slow lfo instrument bipolar sine sync 1bar phase 0.25
  source held random_hold per_voice bipolar rate free 4hz seed 81
  route envelope-cutoff source envelope-source target cutoff depth 6khz map unipolar curve power(0.7) order 20 bypass false {
    auxiliary motion-source multiply
    clamp 0 1
  }
}
```

Source IDs and route IDs are unique within the Instrument. Supported sources are `port NODE.control`, `parameter KEY`, `lfo SCOPE POLARITY sine|triangle|saw|square|sample_hold free RATE|sync PERIOD [phase RATIO] [retrigger note|transport] [loop|one_shot] [forward|reverse|ping_pong]`, `note_velocity`, `note_key ... reference PITCH range SEMITONES`, `key_curve` with nested `point` directives, `note_gate`, `note_age ... duration TIME`, and `random_hold ... rate free RATE|sync PERIOD seed INTEGER`. Graph-port and note-relative sources are `per_voice`; other sources may be `global`, `instrument`, or `per_voice`. A per-voice LFO uses note-relative time and therefore requires `retrigger note`; global and per-Track Instrument LFOs use absolute Song time and require `retrigger transport`. Omitting the token selects that scope-correct behavior. Native Envelope Control Ports are unipolar; native LFO and `to_control` Control Ports are bipolar, so graph-source declarations must use that matching polarity.

A `parameter` source must name a `macro` Parameter with exact `ratio 0 1` domain. Macro routes form a validated acyclic same-frame DAG; a broader-scope consumer cannot read a macro produced by a narrower-scope route. Source and auxiliary scopes cannot require narrower state than the route's primary source. A Song-global Parameter source cannot read a macro with a Track-owned Pattern lock, because no singular Track projection exists at global scope; that combination fails validation. Random holds derive repeatable values from Project/Instrument/source identity, authored seed, durable Track or voice/note-event identity as required by scope, and exact hold index; callback grouping does not participate.

Routes use `linear`, positive sign-preserving `power(P)`, or `smoothstep` curves. `power(P)` requires a finite positive exponent. The optional auxiliary source multiplies the curved primary source before authored `add`, `multiply`, `scale`, `bias`, and `clamp` operators. The physical depth is applied last. Routes targeting one Parameter fold by signed `order` and then durable route ID; macro dependencies and both primary and auxiliary graph-Control dependencies add required DAG edges without introducing a sample delay. A graph Envelope, LFO or `to_control` Control output therefore affects a downstream Parameter consumer on the same frame. Feedback through a `delay_line` Parameter is valid under the same latency rule as an Audio Bus. Zero-latency cycles, backward graph dependencies, invalid scope/type/unit, nested source/operator directives, duplicate auxiliary/clamp directives, allocation-changing targets, excess sources/routes/operators, and non-finite domains fail before publication.

Instrument-owned routes fold after base, Automation, and the Pattern lock but before the separate Song-owned route tier and final Parameter constraint. Provenance distinguishes the two ownership tiers and reports Instrument source, scope, order, bypass, normalized/auxiliary values where singular, physical depth, contribution, unconstrained value, clamp, and held-non-finite status. Per-voice values remain explicitly note-relative rather than pretending to have one Song-time value.

`crump edit instrument-route-depth INSTRUMENT ROUTE DEPTH` and `crump edit instrument-route-bypass INSTRUMENT ROUTE BOOL` use exact route sites, whole-Project validation, atomic publication, and History. Macro values use the ordinary checked Instrument Parameter edit. Depth, bypass, and macro value edits retain compatible voice state while changed route targets and their macro-dependent closure ramp over ADR 0011's 10 ms scalar handoff; source/target/topology changes restart the affected Instrument behind the protective fade.

A graph Delay whose `time` is Parameter-bound changes its voice- or Track-owned buffer allocation and is therefore rejected as an Automation or Modulation target. An authored edit recompiles and restarts the affected topology behind the protective publication fade. Other graph scalars, stock-drum tone/punch, channel fader/panner/Mute, Send gain, supported processor scalars, and Master level use the shared stable target model.

A channel-tap Envelope Follower is a separate Audio-derived additive route:

```crump
follower drum-motion from track/drums tap post detector peak attack 5ms release 100ms floor -60db ceiling -6db target processor/crush/mix depth 20% {
  clamp 0 1
}
```

`peak` and `rms` use the linked-stereo detector definitions shared with dynamics processors. Attack/release are `0.00001s…10s`; finite floor and ceiling lie in `-144db…24db` with `floor < ceiling`. The smoothed level maps to normalized `[0,1]`, receives the ordinary add/multiply/scale/bias/clamp operators, and multiplies `depth` in the destination's physical unit. Routes are evaluated in durable ID order and share the base → Automation → lock → additive Modulation → constraint order.

Controls are evaluated before current-frame Audio, so a tap observed at frame N contributes exactly at frame N+1. This one-sample control latency is source semantics, not accidental scheduling. Inspection names source/tap, detector, target/unit/depth/operators, and latency; fixed-capacity runtime telemetry reports normalized value and the physical contribution scheduled for the next frame. Seek and loop discontinuities clear pending contributions and reset detector state. Same-frame Audio/control cycles and normalized replacement macros are not supported.

## Graphs

In source version 1, `graph N` declares a per-voice Node ceiling. Source version 2 additionally accepts the explicit `graph voice N` form and one optional Track-owned `graph shared N` post-voice domain. The actual graphs cannot exceed their authored or global resource limits. Nodes have durable graph-local IDs, one native kind, and ordered, typed settings with exact source sites. Connections are typed and directed. The complete version-1 floor, including `connect FROM -> TO`, remains accepted by version 2; these legacy connections receive deterministic compatibility IDs in an internal namespace that cannot collide with a valid authored ID. Version 2 additionally provides the durable authored form `bus ID FROM -> TO`, whose IDs must be valid and graph-local unique. Neither connection form accepts nested directives.

A shared graph consumes the deterministic stereo voice sum through exactly one authored `voice_sum.audio` Bus and produces one stereo Instrument output:

```crump
graph shared 2 {
  node echo delay time 187.5ms feedback 0.28 mix $delay_mix
  node shared-out output
  bus voice-sum voice_sum.audio -> echo.audio
  bus shared-output echo.audio -> shared-out.audio
}
```

The voice graph runs for each active voice; its outputs sum in stable voice-slot order. One separate shared Node runtime then runs once per Track and frame—even when the sum is silent so tails continue—before that Track's ordinary native processor chain. `note.*` is voice-only, `voice_sum.audio` is shared-only, and no shared-to-voice edge exists. Shared Parameter consumers use base, Automation, the Track's active lock, global/Instrument routes, and the non-per-voice Song tier; all per-voice route contributions are omitted. Shared Node/control/transition state is preallocated per Track, counted for both resident publication runtimes, and retained across compatible ADR 0011 scalar publications. Topology and allocation changes restart the affected Instrument state; an explicit seek resets shared Node and tail state off-callback before publication.

Graph Nodes:

| Node | Inputs | Outputs | Settings |
| --- | --- | --- | --- |
| `oscillator` | `pitch` | mono `audio` | `waveform`, `detune`, `glide`, `level` |
| `unison` | `pitch`, optional `gain`, `velocity`, `mod` | stereo `audio` | `waveform`, `voices`, `detune`, `width`, `drift`, `glide`, `mod_depth`, `velocity_amount`, `level` |
| `noise` | — | `audio` | `level` |
| `exciter` | `gate`, `velocity` | mono `audio` | impact/collisions/texture kind, contact time, hardness, collision rate, gesture duration/attack, level |
| `modal_bank` | mono `audio`, `pitch`, `gate` | mono `audio` | imported acoustic profile, maximum modes, key tracking, decay scale, brightness, optional body rise, level |
| `envelope` | `gate` | `control` | `mode`, `attack`, `hold`, `decay`, `sustain`, `release`, three segment curves, `retrigger` |
| `lfo` | — | `control` | `rate`, `depth`, `phase`, `random_phase`, `swing`, `waveform`, `mode`, `direction`, `polarity`, `retrigger` |
| `mixer` | mono `a`, optional mono `b` | mono `audio` | `a_level`, `b_level` |
| `mono_to_stereo` | mono `audio` | stereo `audio` | — |
| `stereo_to_mono` | stereo `audio` | mono `audio` | — |
| `stereo_mixer` | stereo `a`, optional stereo `b` | stereo `audio` | `a_level`, `b_level` |
| `stereo_sum` | one or more stereo `inputs` | stereo `audio` | `max_inputs` (`1…64`, default `64`) |
| `sum` | one or more mono `inputs` | mono `audio` | `max_inputs` (`1…64`, default `64`) |
| `lowpass` / `filter` | mono `audio`, optional `mod`, `velocity`, `key` | mono `audio` | `mode`, `cutoff`, `resonance`, `mod_depth`, `velocity_depth`, `key_track`, `key_reference` |
| `stereo_filter` | stereo `audio`, optional `mod`, `velocity`, `key` | stereo `audio` | `model`, `mode`, `cutoff`, `resonance`, `drive`, `mix`, `pan`, `mod_depth`, `velocity_depth`, `key_track`, `key_reference` |
| `analog_oscillator` | `pitch`, optional mono `fm`, `pm`, `sync`, `am`, Control `gain`, `velocity` | stereo `audio` | waveform, tuning, unison/blend, phase, drift/glide, cross-mod depths, level/pan, bounded warp/distortion stages |
| `wavetable` | `pitch`, optional mono `fm`, `pm`, `sync`, Control `gain`, `velocity` | stereo `audio` | exact asset, table position, tuning, unison/blend, phase, drift/glide, FM/PM depth, velocity, level/pan, bounded phase-warp stages |
| `sample_source` | `pitch`, `gate`, optional Control `gain`, `velocity` | stereo `audio` | exact asset/slice, normalized region/loop, direction/playback, root/tune/rate, velocity, level/pan |
| `multisample` | `pitch`, `gate`, `velocity`, optional Control `gain` | stereo `audio` | 1–256 exact zones, playback, tune/rate, velocity, level/pan |
| `granular` | `pitch`, `gate` | stereo `audio` | exact asset, 1–64 grains, position/scan/freeze, size/density, tune, spray/direction, window/edge, spread, seed, level |
| `spectral` | `pitch` | stereo `audio` | exact asset/algorithm, 1–128 peaks, scan/rate/freeze, root/tune, low/high region, transient policy, phase randomization, seed, level |
| `amplifier` | `audio`, `gain` | `audio` | `level` |
| `saturation` | `audio` | `audio` | `drive` |
| `pan` | mono `audio` | stereo `audio` | `position` |
| `delay` | stereo `audio` | stereo `audio` | `time`, `feedback`, `mix` |
| `reverb` | stereo `audio` | stereo `audio` | `room`, `damping`, `mix` |
| `delay_line` | mono `audio`, optional `pitch`, mono `time_mod` | mono `audio` | `mode`, `time`, `ratio`, `max_time`, `mod_depth`, `interpolation`, `tune_compensation` |
| `math` | mono `a`, mono `b` for binary operations | mono `audio` | `operation`, `amount` |
| `stereo_math` | stereo `a`, stereo `b` for binary operations | stereo `audio` | `operation`, `amount` |
| `shaper` | mono `audio` | mono `audio` | `curve`, `drive`, `bias`, `asymmetry`, table `point` directives |
| `to_audio` | Control `control` | mono `audio` | — |
| `to_control` | mono `audio` | bipolar `control` | — |
| `dc_block` | mono `audio` | mono `audio` | `cutoff` |
| `linear_phase_lowpass` | mono `audio` | mono `audio` | `amount` |
| `biquad` | mono `audio`, optional `key` | mono `audio` | `mode`, `frequency`, `q`, `gain`, `key_track`, `key_reference` |
| `onepole` | mono `audio` | mono `audio` | `mode`, `cutoff`, `time` |
| `biquad_chain` | mono `audio`, optional `key` | mono `audio` | 1–32 `band` directives |
| `constant` | — | `pitch` | `frequency` |
| `output` | stereo `audio` | — | — |

`noise` produces deterministic, centered uniform white noise in `[-1, 1)` before applying `level`; unit-level RMS approaches `1 / sqrt(3)`. A previous unsigned-to-signed scaling bug produced positive-only noise at roughly 1/256 of the intended AC amplitude. Corrected renders therefore change the balance of existing graphs that use this Node. The acoustic `exciter` and `modal_bank` generators do not use that noise path.

Version-1 Node settings remain inline on the `node` declaration. Version-2 runnable Node block settings are normally non-nested scalar `key value` directives. `analog_oscillator` and `wavetable` each admit at most two ordered `warp ID KIND amount VALUE` stages; analog oscillator additionally admits one ordered `distortion ID KIND amount VALUE` stage. A custom graph LFO admits 2–64 strictly phase-ordered `point ID phase 0..1 value -1..1` directives beginning at phase 0 and ending at phase 1. A `curve table` shaper admits 2–64 `point ID input X output Y` directives with strictly increasing inputs and values within `-16..16`. `sample_source` admits `slice ID START END`; `multisample` admits the complete zone form described below. Other nested or multi-token runnable directives fail closed, including on `granular` and `spectral`.

Special per-note sources are `note.pitch`, `note.gate`, and normalized `note.velocity` in the voice domain; the shared domain instead has only the external stereo source `voice_sum.audio`. Every required input must have exactly one compatible source except the bounded repeated `sum.inputs` and `stereo_sum.inputs` Ports. Summed inputs fold in durable Bus-ID order. Mono-to-stereo duplicates the sample without gain change; stereo-to-mono produces `(left + right) / 2`. No other channel coercion or fan-in is implicit. A cycle is valid only through a `delay_line` input (see [Feedback loops](#feedback-loops)); every authored Node must reach its one Output, and duplicate or unknown settings are errors. Topological compilation is deterministic.

A filter `mod` input accepts either uni- or bipolar Control, so an AHDSR timbre envelope and an LFO use the same explicit port without weakening Audio typing. `velocity` adds `velocity_depth` hertz. `key` scales the base cutoff around `key_reference` (C4 / 261.625565 Hz by default); `key_track 1` doubles cutoff for each played-note octave. The stereo `svf_clean` model provides low-pass, high-pass, band-pass, and notch modes. `ladder_drive` is the bounded nonlinear four-pole low-pass model; unsupported ladder mode combinations fail closed rather than silently approximating another response. `svf_clean` applies its smoothed `drive` as normalized input soft saturation before the state-variable stages; zero drive is exact clean input. `ladder_drive` uses deterministic 2× oversampling with midpoint interpolation, four saturating stages, and per-substep resonance feedback. Both models use 1 ms cutoff/resonance/drive smoothing, zero declared latency, channel-isolated state, and deterministic reset. Resonance is clamped to `0…1`: `svf_clean` does not promise self-oscillation. `ladder_drive` divides its feedback by the drive stage's small-signal gain, so its loop gain is `4 × resonance` at every drive; below resonance 1 it rings but always decays, and it is not specified as a free-running oscillator. `mix 0` is exact dry; `pan` attenuates the opposite output side.

`analog_oscillator` owns 1–8 preallocated subvoices and supports sine, polyBLEP saw, square/pulse, and triangle laws, coarse/fine tuning, deterministic phase/random-phase/drift, blend, power normalization, width, velocity, level, and pan. Typed mono Audio ports provide linear-FM pitch, phase modulation, rising-edge hard sync, and AM or bipolar ring multiplication. FM/PM/sync are applied before the phase read; authored warp stages then transform phase in declaration order; one optional normalized post-read distortion stage follows; amplitude, blend/power normalization, and pan follow last. Pulse-width and phase-remap/bend stages are structural and bounded; unsupported stage counts/kinds, wrong Audio/Control types, and non-finite domains fail validation and defensive compilation. The Project, Instrument, Track, graph Node, voice slot, note event, and authored seed identities mix without callback order or block size.

`wavetable` reuses the same fixed 1–8-subvoice phase, tuning, glide, detune, drift, blend, width, velocity, level, pan, FM, PM, and rising-edge sync contracts. Its required `asset` resolves only to a complete exact wavetable declaration. At most two `pulse_width`, `bend`, `remap`, or `phase_remap` stages run in authored order before the read. Off-callback `crump-wavetable-radix2-v1` compilation materializes only Track-referenced tables, normalizes as declared, transforms each frame, and stores immutable mip levels from `frame_length / 2` harmonics down by powers of two to one. Runtime picks the first level within the current Nyquist bound, uses cyclic linear phase interpolation, and then applies the authored linear or boundary-clamped Catmull–Rom frame morph. Table data is `Arc`-shared and immutable; complete table storage, per-subvoice work/state, and both resident publication runtimes are admitted conservatively. Exact WAV bytes remain Authoritative Source, while decoded samples and mip tables are disposable. Structural asset/metadata/stage/allocation changes restart only affected Track Instrument state under ADR 0011; compatible position, warp, Parameter, and Modulation changes retain state.

`sample_source` uses exact half-open original-source frames for at most 256 `slice ID START END` directives. `selected_slice` chooses one; omission chooses the first declared slice or the complete asset. Normalized `start`/`end` and loop bounds are evaluated from the same-frame Parameter/Modulation projection and latched on note start; their complete Parameter domains must remain ordered. `loop_mode` is `off`, forward wrap, or reflected `ping_pong`; `loop_crossfade` is a bounded original-source frame count. Direction is forward/reverse. `playback gated` applies a fixed 64-frame source release on Note Off; `one_shot` ignores Note Off, forbids loops, and ends at the region boundary. Linear interpolation reads immutable Project-rate stereo frames. Root/tune and positive `rate 0.125…8` are repitch only—pitch and duration move together, with no implied time stretch or formant preservation.

A multisample zone names its asset, inclusive keys, and root pitch. Every other clause is optional, may appear in any order after the zone ID, and may appear at most once:

```crump
zone soft asset violin-g3-soft keys G3 B3 velocity 0 0.65 root G3 loop 24000 72000 crossfade 4800
zone low asset piano-c3 keys C1 B3 velocity 0 1 root C3 layer body articulation default round_robin body-low 0 region 0 24000 gain -3db pan -0.2
```

| Clause | Default |
| --- | --- |
| `velocity LOW HIGH` | `0 1` |
| `layer ID` | `default` |
| `articulation ID` | `default`, the only supported value |
| `round_robin GROUP INDEX` | the layer's implicit group |
| `region START END` | the whole asset |
| `gain DB` | `0db` |
| `pan PAN` | `0` |
| `loop START END crossfade FRAMES` | no loop |

Zones without `round_robin` share their layer's implicit group. A group sounds at most one zone per note, chosen at random among the zones matching the key and velocity. Non-overlapping zones in the implicit group therefore play exactly as if each had its own group, and overlapping ones become round-robin alternatives. An explicit `round_robin` group keeps its authored indexes, unique within the layer and group.

The optional `loop START END crossfade FRAMES` clause repeats a half-open original-frame range inside the zone region while the note is held. The crossfade is at most half the loop and 8,192 frames. After Note Off, a `one_shot` zone leaves the loop and plays through the rest of its region, so a sample can hold an attack, a sustain loop and a natural release. A `gated` zone instead takes the fixed source release.

`multisample` owns 1–256 zones. Key bounds are inclusive; source regions remain exact half-open original frames. Every matching round-robin group chooses one stable index from semantic note identity, while distinct groups layer additively with authored gain/pan. The first stable articulation selector accepts only `default`; unsupported values fail rather than becoming inert labels. Pitch/root/tune/rate, linear interpolation, gate/one-shot release, velocity response, immutable cache sharing, and callback bounds match `sample_source`. SFZ import is deferred; no external mapping path is runtime authority.

`crump-sample-windowed-sinc-v1` binds each disposable decoded cache to exact source SHA-256 and Project sample rate. Asset-aware admission computes converted frame storage before payload allocation, counts per-zone cursor state, charges one check per zone plus playback for the most round-robin groups any single key can sound, and includes both publication runtimes. Mono sources are stored as one channel. Asset/cache, slice/loop/playback/direction/root, and zone-map changes restart only affected Track Instrument state; compatible scalar changes retain state and apply to future note-on-latched regions.

`granular` reuses that immutable exact-asset cache and owns 1–64 preallocated slots per graph voice. A sample-accurate fractional density accumulator may spawn at most one grain per sample. Stable authored/note/Node identity plus a monotonic per-note spawn index determines spray, reverse probability, and equal-power pan, independently of callback grouping and unrelated topology insertion. Position combines normalized control, signed scan phase, and spray; `freeze true` stops scan movement. Grain sizes are 5 ms…1 s, density is 0.1…1,000 Hz, and pitch is root-relative repitch. `hann`, `triangle`, and `tukey` windows are exact finite laws. `wrap`, `reflect`, and `clamp` define source edges; reflect uses bounded triangular folding for any admitted multi-boundary overshoot. Note Off stops spawning while active grains finish, bounding source tail to one second. Maximum slots, window, edge, root, and seed are structural; compatible scalar changes affect future spawn snapshots. Work/state for every slot, both graph-voice storage positions, and both publication runtimes is admitted before callback execution.

`spectral` requires `algorithm crump-spectral-peaks-v1`. Away from the callback it converts the exact asset through the shared Project-rate sample cache, applies a fixed 2,048-frame periodic Hann window every 256 frames, performs deterministic radix-2 stereo FFT analysis, and retains up to 128 local-magnitude peaks per frame in magnitude-descending/bin-ascending order. Runtime linearly interpolates corresponding ranked peak slots and advances one preallocated stereo phase pair per admitted peak. `scan` selects normalized analysis position; non-frozen `rate -4…4` advances source time independently of root-relative note/tune pitch. `low`/`high` select source-analysis frequencies before repitch and their complete domains must remain ordered. `transients preserve` keeps oscillator phase, `reset` adopts analyzed phase on versioned spectral-flux transient frames, and `suppress` applies a fixed 0.25 transient gain. `phase_randomization` adds a deterministic semantic-identity phase offset when phases initialize/reset; it is not stochastic callback state. Analysis-frame interpolation is linear by deterministic ranked slot and is not claimed as transparent time stretching, phase-vocoder reconstruction, or formant preservation. Frequencies at or above resynthesis Nyquist are omitted; active-peak power normalization and finite-output clamping bound malformed extreme-but-finite media behavior. The exact asset hash, sample-cache identity, Project rate, 2,048/256 geometry, 128-slot ceiling, channel/ranking law, and algorithm name form the disposable cache identity. One voice graph admits at most two spectral sources. Immutable analysis payload, fixed phase state, peak work, graph replacement storage, and both publication runtimes are admitted before publication. Analysis/cache, peak-count, transient-policy, root, seed, or Node-identity changes are structural; scalar scan/rate/tune/region/phase-randomization/freeze/level changes retain state.

An `envelope` defaults to backward-compatible `mode adsr` and additionally supports `mode ahdsr`, hold, signed curve amounts for attack/decay/release, and `retrigger always|legato`. ADSR rejects an authored `hold`; AHDSR accepts it with a zero default, so `mode` is never an inert label. `always` restarts on a gate edge or a retained mono selected-note transition, while `legato` preserves the current level. Zero-duration boundaries advance deterministically; release begins from the current level. Segment durations may be physical seconds or source-v2 synchronized musical time. Several envelope Nodes require no shared mutable state and are bounded by the ordinary graph Node limit.

A graph `lfo` supports sine, triangle, saw, square, sample-and-hold, or a bounded custom piecewise-linear shape; free hertz or synchronized musical period; phase/random phase; bipolar/unipolar output; loop/one-shot; forward/reverse/ping-pong; bounded swing phase relation; and `retrigger note|transport`. Note retrigger state is per graph voice; transport mode derives phase from the absolute Song frame. Sample-hold and random phase use stable semantic identity. All laws are finite and callback-block independent, and custom-point interpolation work is charged per point by admission.

`unison` owns 1–8 deterministic subvoices, equal-power stereo placement, bounded semitone spread/drift, power normalization, optional envelope gain and velocity response, and bounded Control-port pitch movement. `width 0` collapses placement to a mono-compatible center. Oscillator `glide` is a deterministic note-to-note exponential frequency slew in seconds. Unison subvoice work and the preallocated short graph-voice replacement tail are included in runtime admission. For tempo-synchronized global or per-voice movement, bind these scalar settings to Parameters and use authored Modulation routes rather than inventing callback state. The curated readable modules and their frozen abstract CPU/memory costs are documented in [Expressive native Instrument Modules](expressive-instruments.md).

`exciter` and `modal_bank` compose finite contact gestures and profile-driven resonant bodies. Their numerical data may live in separate imported `crump-profile 1` sources or embedded in `crump-module 2` Instrument Modules; Instrument Module export includes required profiles. Profiles share one Project-wide namespace: multiple imported sources declaring the same profile ID with semantically identical parsed bodies deduplicate deterministically (keeping the first in import order), while differing bodies or same-file duplicates fail with `CRV210` naming both locations. Physical controls latch on note onset. Acoustic graphs and graphs with one-shot sample sources have a 30-second release ceiling with a terminal 50 ms fade; gated graph release limits are unchanged. See [Native acoustic models](native-acoustics.md) for the complete profile grammar, physical domains, admission and tail contracts.

### Feedback loops

A Bus into a `delay_line` may close a cycle. Its output depends only on earlier frames: each frame, every other Node runs in topological order, then every `delay_line` reads its inputs from that frame and they all publish their outputs for the next frame together. A loop's period is therefore exactly the authored delay plus the delay of the filters in it. Any other cycle fails with CRV048, which names the Nodes in the cycle. The existing `delay` cannot close a loop, because its dry mix passes the current frame through. Loops work in voice and shared graphs.

```crump
node excite math operation add
node string delay_line mode pitch interpolation lagrange3 tune_compensation 1 max_time 0.1s
node loss linear_phase_lowpass amount 0.1
node decay math operation scale amount 0.99
bus pitch note.pitch -> string.pitch
bus into excite.audio -> string.audio
bus along string.audio -> loss.audio
bus lossy loss.audio -> decay.a
bus back decay.audio -> excite.b
```

`delay_line` has no internal feedback. `mode time` delays by `time` (0–2 s). `mode pitch` requires a `pitch` input and delays by `ratio / pitch` (ratio 0.01–16), so a loop follows the played note. `time` applies only to time mode and `ratio` only to pitch mode. A connected mono `time_mod` scales the delay by `1 + mod_depth × time_mod`, with `time_mod` clamped to `-1…1` and `mod_depth` (0–1, default 0.5). Modulation is evaluated every frame, so it does not step. `tune_compensation` (0–16 frames) is subtracted from the delay to cancel a loop filter's delay. `interpolation` is `linear` (minimum 1 frame), `allpass`, a first-order Thiran allpass (minimum 1.5 frames), or the default `lagrange3` (minimum 2 frames). Each tunes a loop within 2 cents from C1 to C7. Linear interpolation also low-passes the loop. `max_time` (1 ms–2 s, default 100 ms) is a literal buffer size; Parameter references are rejected because controls cannot resize the allocation. It sizes the preallocated buffer. A time-mode `time` beyond it fails with CRV220. In pitch mode the delay clamps to the buffer, so notes below `ratio / max_time` play sharp. A voice start clears the buffer.

`math` applies `add`, `subtract`, `multiply`, `min` or `max` to `a` and `b`, or `abs`, `negate`, `offset` (`a + amount`) or `scale` (`a × amount`) to `a` alone. A binary operation requires `b`, a unary one rejects it, and `amount` (−64–64) applies only to offset (default 0) and scale (default 1). `stereo_math` applies the same law to each channel. `shaper` evaluates its curve at `drive × audio + bias` (drive 0–64, default 1; bias −16–16). The curves are `tanh`, `hard_clip` (±1), `soft_clip` (cubic, ±1), `one_sided` (negative values become 0), `asymmetric` (tanh, with the negative side limited to `1 / (1 + 3 × asymmetry)`), and `table`, which interpolates its points linearly and holds the end values beyond them. `to_audio` passes a uni- or bipolar Control into mono Audio, for example an envelope into `analog_oscillator.fm` or `delay_line.time_mod`. `to_control` turns mono Audio into bipolar Control clamped to `-1…1`. Its `control` output may feed Control inputs or an Instrument-owned `port NODE.control per_voice bipolar` Modulation source. `dc_block` is a first-order high-pass (1–500 Hz, default 20 Hz). `linear_phase_lowpass` is the symmetric filter `g·x[n] + (1 − 2g)·x[n−1] + g·x[n−2]` with `g = amount / 4` (default 0.5). Its response is never negative, so it delays every frequency by exactly one frame, and `tune_compensation 1` cancels it.

`biquad` is a second-order filter with the Audio EQ Cookbook responses: `lowpass`, `highpass`, `bandpass` (0 dB at its centre), `notch`, `peak`, `lowshelf`, `highshelf` and `allpass` (default `lowpass`). `frequency` is 1 Hz–24 kHz (default 1 kHz), held below 0.49 of the sample rate; `q` is 0.05–64 (default 0.7071); `gain` (−48–48 dB) applies only to `peak` and the shelves. A connected `key` multiplies the frequency by `(key / key_reference) ^ key_track`, so a body resonance stays fixed with `key_track 0` and a formant or string-stiffness all-pass follows the note with `key_track 1`; `key` requires `key_track`. Coefficients are recomputed only when the frequency, Q or gain changes. An all-pass passes every frequency at unit level, so a chain of them inside a string loop makes the upper partials sharp, as a stiff string does. `onepole` is first order: `lowpass` and `highpass` (complementary, `cutoff` 1 Hz–24 kHz), or `leak`, `y = x + e^(−1 / (time × rate)) × y[n−1]` with `time` 10 μs–60 s. A short strike into a leak becomes an exact exponential decay with e-folding `time`, so one Node replaces an envelope loop; `cutoff` applies only to the filters and `time` only to the leak. Both recover from a non-finite input by resetting.

`biquad_chain` runs 1–32 `biquad` bands in series, for a bank of fitted formants, a set of phase-spreading all-passes, or a high-pass and low-pass that band-limit a noise:

```crump
node formants biquad_chain {
  band chest mode peak frequency 472hz q 2.5 gain 15db
  band bright mode highshelf frequency 2.1khz gain -2.7db
  band below-note mode highpass frequency 157hz key_track 1
}
bus key note.pitch -> formants.key
```

Each band is `band ID mode MODE frequency HZ` followed by optional `q`, `gain`, `key_track` and `key_reference` clauses in any order, each at most once. Modes, domains and defaults are those of `biquad`, and each band processes exactly as a separate `biquad` Node would, so a chain sounds identical to the Nodes it replaces while counting as one Node. Band values are literals: a band that must follow a Parameter stays a `biquad` Node, and editing any band restarts the Node's state. The chain reads `key` only when at least one band sets `key_track`, and a band with `key_track` requires the connected `key`. Each band costs the work of one `biquad`.

`constant` outputs a fixed `frequency` (1 Hz–24 kHz, Parameter-bindable) on a `pitch` port typed like `note.pitch`. It drives any `pitch` or `key` input that must not follow the played note: a drum machine's oscillators, a drone, or a body resonance:

```crump
node drone-pitch constant frequency 110hz
bus drone-pitch drone-pitch.pitch -> drone.pitch
```

A `key_curve` Modulation source is a piecewise-linear curve over the played key:

```crump
source register key_curve per_voice bipolar {
  point low key E3 value 0
  point mid key A#4 value -1
  point high key E6 value -0.74
}
route register-by-key source register target register depth 0.82 map bipolar curve linear order 0 bypass false
```

It declares 2–64 `point ID key PITCH value VALUE` directives with unique IDs, strictly increasing keys, and values within the source's polarity (`0…1` unipolar, `-1…1` bipolar). Between points it interpolates linearly in semitones; beyond the end points it holds the end values. It is note-relative, so it requires `per_voice` scope. One `key_curve` route replaces a stack of clamped `note_key` routes that draw a register curve.

Validation bounds a loop's ranges, memory and work, but it cannot prove the loop stable. In a voice or shared runtime that contains a `delay_line`, a non-finite Node output anywhere in the graph, a loop or output sample above +60 dBFS, or a loop or output continuously above +24 dBFS for 50 ms mutes the runtime and resets its state. A muted voice ends, and the next note starts from silence. Runtime telemetry and the render manifest's `feedback_mutes` count mutes, and `crump render` prints a warning when the count is not zero. Each `delay_line` costs four work units and its buffer. The [waveguide examples](../examples/waveguides/README.md) build a plucked string, a sitar and a clarinet this way.

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
  sample breath 2beat mapping phrase duration 1beat start 8400 end 22800 gain -3db pan 0.15 fade 32 64 velocity 0.9 direction reverse loop 9600 21600 crossfade 128
}
```

A note has onset, scientific pitch (`C4`, `F#3`, `Bb2`), positive duration, and velocity in `[0, 1]`. A hit has onset, stock voice, and velocity. A sample occurrence has a durable Pattern-local ID, sampler mapping, positive timeline duration, half-open original-source region, gain, pan, source-edge fades, and velocity. Optional `direction` and `loop ... crossfade ...` suffixes override the mapping for that occurrence; omission inherits it. Its region is non-destructive: the Project asset bytes never change. Source order is presentation only; runtime ordering is normalized deterministically by frame and semantic Event identity.

Checked operations `edit sample-direction`, `edit sample-loop`, and `edit sample-loop-clear` preserve exact source structure and publish through the same Production View path as gain, pan, trim, slip, split, and placement. Reverse changes traversal only. Slip changes a fixed sample offset and moves an occurrence-local loop by the same exact source-frame delta. Splitting through a local loop is rejected rather than silently changing playback.

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

A Parameter lock is a physical-unit step override for the consuming Track's assigned Instrument Parameter. Its onset lies inside the Pattern span, the key must resolve to a topology-stable `automatable` Parameter, and the value must match that Parameter's unit and domain. Runtime lanes retain durable Track ownership: a lock changes only that Track's voices and shared tail, never another Track that references the same Instrument. A lock takes precedence over Song Automation, then routed Modulation remains additive. It follows source offset, looping, and Variation timing and resets at the Clip end. To keep precedence unambiguous, Clips that share one Instrument may not overlap when either contains locks.

Tracks assign one Instrument and begin with backward-compatible level/balance values. Their bodies may add an explicit panner, ordered processors, Sends, Mute, main output, and Clips:

```crump
track lead "Lead" instrument lead level -9db pan -0.1 {
  panner stereo_pan -0.8 0.8
  processor lead_eq eq {
    bypass false
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

Native processor kinds are `utility`, `eq`, `compressor`, `gate`, `delay`, `reverb`, `convolution`, `saturation`, `modulation_delay`, `crusher`, `stutter`, `limiter`, and the testing/compensation `latency` kind. A processor may declare `bypass true|false`, `setting KEY VALUE`, and deterministic `latency FRAMES`; `compressor` and `gate` may each declare at most one `sidechain input NAME`. Omitted bypass is exactly `false`. Durable bypass is a source-backed, automatable step Parameter at `processor/ID/bypass`; a checked edit inserts the directive when absent. It affects playback and rendering identically, switches at the exact authored step frame, preserves complete output latency through an aligned dry path, and continues running wet state silently so tails and detector state survive re-enable. Monitoring-only transient bypass uses the same stable ID and a fixed 64-frame click guard, but is not source or render state. The compressor is linked stereo and feed-forward: `peak` detects the larger absolute channel, `rms` detects the instantaneous stereo RMS, and a deterministic one-pole amplitude envelope applies the authored attack/release before the threshold/ratio/soft-knee curve. An external sidechain replaces program Audio only as the detector source; makeup applies to the compressed wet path before the exact parallel `mix`. Every declared sidechain input requires exactly one route, and every sidechain route targets exactly one declared `processor/ID/INPUT`; disconnected declarations and undeclared destinations are rejected by both validation and compilation.

### Convolution reverb

`convolution` is a native insert for Track, Group, Return, Master, and Instrument post-processing chains. Its required structural `ir` setting references a Project or imported module Media Asset declared with `usage impulse_response`. The asset must be a nonempty mono or stereo WAV of at most 10 seconds after Project-rate conversion. Mono applies the same response independently to both inputs. Stereo applies the left and right responses to their respective inputs. The processor preserves the authored amplitude and sample order at matching rates. Other rates use the Project sample cache's deterministic resampler.

| Setting | Domain | Default | Meaning |
| --- | --- | --- | --- |
| `ir` | Media Asset ID | Required | Structural impulse-response reference |
| `mix` | 0–1 | 0.25 | Linear dry/wet blend |
| `pre_delay` | 0–1 s | 0 s | Wet-only fractional delay with 10 ms tap crossfades |
| `level` | −96–24 dB | 0 dB | Post-mix output gain |
| `low_cut` | 0–23.52 kHz | 0 Hz | Structural one-pole wet high-pass |
| `high_cut` | 0–23.52 kHz | 0 Hz | Structural one-pole wet low-pass |
| `width` | 0–1 | 1 | Wet stereo side gain |

Zero disables each cut. Nonzero cut frequencies are capped at 0.49 of the Project rate. Cuts are applied while preparing the kernel and extend it by one second of filter decay. Changes to the IR bytes or cut settings replace the compiled kernel and restart its history. Mix, pre-delay, level, and width are continuous Parameters.

The processor declares zero intrinsic latency. A direct 256-frame head supplies immediate output while fixed-size overlap-add FFT partitions supply the rest. Inspection reports a bounded tail covering the prepared response, FFT overlap, and maximum pre-delay. The authored Project render tail and explicit bounce tail still set the output length. See the [hall Return recipe](cookbook/recipes/convolution-hall.md).

One sidechain-capable `gate` kind covers hard gating and downward expansion:

```crump
processor duck gate {
  setting mode expander
  setting threshold -30db
  setting ratio 4
  setting attack 2ms
  setting hold 20ms
  setting release 80ms
  setting range 48db
  setting hysteresis 3db
  setting detector peak
  setting mix 1
  sidechain input detector
}
sidechain trigger from track/kick tap post to processor/duck/detector
```

Linked `peak`/`rms` detector definitions match Compressor. The gate opens at `threshold`; close eligibility begins at `threshold - hysteresis`, then exact sample-count `hold` expires. `gate` moves between zero and `range` dB reduction. `expander` applies `min(range, (threshold - level) * (ratio - 1))` below threshold. Linear gain uses `attack` while opening and `release` while closing; `mix` is parallel dry/wet. There is no lookahead or intrinsic latency. The named sidechain replaces program Audio only for detection and reaches the processor through existing latency compensation. Detector, hold, gain, and reduction telemetry continue while bypassed; no Audio tail is generated from silent input.

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

One bounded modulation-delay family owns Chorus and Flanger behavior:

```crump
processor motion modulation_delay {
  setting mode chorus
  setting delay 12ms
  setting depth 3ms
  setting rate 0.5hz
  setting feedback 0
  setting mix 50%
  setting width 100%
}
```

`mode` is structural: `chorus` averages phase-opposed fractional taps while `flanger` uses one stronger comb-producing tap. Continuous domains are `delay 0.1ms…50ms`, `depth 0ms…20ms`, `rate 0.01hz…20hz`, signed `feedback -0.95…0.95`, and ratio `mix`/`width`. One fixed 70 ms stereo ring is allocated at compile time; direct sine LFO evaluation, fractional reads, feedback, and mixing perform no callback allocation. Width zero gives coincident left/right modulation; width one offsets them by half a cycle. Intrinsic latency is zero because dry Audio remains immediate. Feedback state continues while bypassed and is bounded by the explicit Project/render/bounce tail rather than hidden silence detection.

Crusher semantics remain separate from Saturation:

```crump
processor crushed crusher {
  setting bit_depth 12bit
  setting sample_rate 12000hz
  setting reconstruction 0
  setting mix 100%
  setting output 0db
}
```

`bit_depth` is a continuous `bits` Parameter in `1bit…24bit`; `sample_rate` is `100hz…23.52khz` and is defensively limited to 0.49 of the Project rate. A shared stereo phase accumulator captures on deterministic boundaries. `reconstruction 0` is zero-order hold; larger values linearly approach each new quantized capture over that fraction of its capture period. This processor intentionally aliases, adds no dither/noise, has zero intrinsic latency, and applies explicit post-mix output compensation. Its held/reconstructed state clears within at most two periods at the 100 Hz minimum. Inspection reports `project_render_tail` for modulation delay and `bounded_frames` plus that crusher ceiling. Numeric settings are ordinary automatable/modulatable physical-unit targets; this deterministic availability is not a claim that audio-rate crusher control is universally musical.

Stutter captures preceding Audio into one fixed ring and repeats it finitely:

```crump
processor repeat stutter {
  setting slice 1/4beat
  setting repeats 4
  setting trigger false
  setting chance 0.25
  setting interval 1beat
  setting seed 41
  setting crossfade 5%
  setting mix 1
}
```

`slice` and `interval` are exact structural musical durations resolved once from fixed Song tempo; interval must be at least slice. Slice spans `1/64beat…4beat`, interval spans `1/64beat…16beat`, and repeats are `1…64`. A false→true `trigger` edge or an eligible absolute interval boundary freezes the preceding complete slice and replays its oldest frame first. `chance` uses a versioned SplitMix64 decision from authored seed, stable processor ID, and absolute cycle; no wall clock, callback grouping, or mutable RNG participates. Triggers during warm-up or a repeat are ignored and counted. Held triggers cannot retrigger. After `repeats * slice` frames, recording resumes only after a fresh complete warm-up. `crossfade` is a ratio `0…50%` of the frozen slice, so both reads remain bounded; `mix` is continuous. Intrinsic latency is zero, tail is finite and inspectable, bypass continues capture/repeat state, and telemetry exposes repeating plus ignored-trigger count. This is a preceding-audio repeater, not a lookahead or proprietary warp transient mode.

Reverb uses fixed preallocated stereo comb and gain-neutral allpass networks. `pre_delay` is `0s…1s`; `diffusion`, `decay`, `damping`, `modulation`, `width`, and `mix` are ratios in `0…1`. Pre-delay uses fractional interpolation and a fixed 10 ms read-head crossfade for continuous changes. Diffusion changes the bounded allpass coefficient rather than output gain, decay controls bounded comb feedback, damping low-passes recirculated Audio, and modulation moves fractional comb taps by at most eight frames at 48 kHz with deterministic phase offsets. A fixed input-headroom coefficient keeps sustained network buildup bounded. Stereo input is cross-fed without discarding side-only material. Width scales only the wet side component, so width zero is exactly mono and fold-down preserves the wet mid. Put one wet Reverb on a Return and route multiple Sends to it when the sources must share one continuing room state.

Accepted settings are validated against the exact kind-specific schema—unknown settings and runtime no-ops are rejected. `edit processor-setting ID KEY VALUE` replaces one exact source token or inserts one missing supported setting; `edit processor-bypass ID true|false` inserts/replaces durable bypass; and `edit processor-move ID earlier|later` moves one complete adjacent processor block with its attached leading comments inside the same owner chain. Every operation is revision-checked, whole-Project validated, atomically published, and recorded in History. Compiled inspection resolves authored values and effective defaults with units/domains, separate authored-literal and continuous-Parameter-target provenance, authored/transient/effective bypass, continuing-state policy, explicit versus intrinsic latency, nonlinear evaluations, activity, the filtered oversampling factor/tap count, and state/work admission facts. Saturation and Limiter share one fixed 4×, 129-tap linear-phase FIR kernel. Saturation filters both interpolation and decimation, evaluates its selected curve four times per input frame, aligns the dry path, and introduces 32 frames of intrinsic latency. Limiter reconstructs inter-sample peaks through the same kernel, adds the filter's 16-frame detector latency to authored lookahead, attacks immediately, and releases with the authored time constant. Lookahead changes allocated delay topology, so it is an authored structural literal rather than an automatable/modulatable Parameter target. Inspection publishes the Limiter's fixed 0.1 dB true-peak safety margin; neither processor applies hidden normalization or a universal loudness target. Utility width is correlation-preserving mid/side narrowing in `0…1`; it preserves the mid signal while scaling side energy, and widening is not silently accepted. EQ and Delay filter-frequency domains are `10hz…23.52khz` at the fixed 48 kHz Project rate, keeping authored targets below Nyquist without a hidden runtime clamp.

## Validation and editing

```bash
crump validate
crump --json inspect --at 2bar
```

Diagnostics carry stable codes, severity, file, line, column, and byte ranges. Parsing recovers enough structure to report multiple errors in one pass.

Production View commands do not reformat a file. They compare the expected revision and literal, replace only the selected source range, validate the whole resulting Project, and atomically publish it. If source changed concurrently, the edit fails with conflict exit class `3` rather than guessing.

Arbitrary text editors and Agents may edit source directly. Such bytes remain a Candidate until an explicit successful `publish`.

## Limits

The validator enforces finite numbers, domains, unique IDs, references, portable asset identities and exact hashes, exact mono wavetable shape and its 32…8,192-sample power-of-two frame / 1…256-frame / 2,097,152-sample interpretation ceilings, at most 256 sample slices or multisample zones with exact asset-frame bounds, at most 64 preallocated grains per granular source, at most two spectral sources per voice with 1–128 peaks each and ordered frequency domains, graph compatibility/reachability, acyclic legal channel routing, Send/sidechain destinations, panner modes, processor schemas and latency, supported sample rate, maximum render tail, maximum Nodes, at most 1,024 media assets within the declared byte ceilings, at most 512 graph voices across actual runtime Track instances, at most 128 Automation lanes, at most 1,024 points per lane, at most 8,192 Automation points per Project, at most 8,192 materialized Parameter-lock points, and at most 1,024 dense callback-time Parameter targets. It also rejects duplicate targets, non-automatable or allocation-changing targets, non-step boolean Automation, ambiguous same-frame points, conflicting lock-bearing Clips, out-of-domain physical values, and Projects exceeding structural or single-runtime memory limits before allocation (CRV092). A Project that exceeds only Live admission remains valid for publication and offline rendering and produces warning CRV093. These are safety and product-scope limits, not invitations to generate at the ceiling. Use `crump check` for the narrower Reference Musical Sketch structural checks.
