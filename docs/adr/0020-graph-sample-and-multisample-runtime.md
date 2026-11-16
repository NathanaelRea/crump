# ADR 0020: Graph Sample and Multisample Runtime

Status: Accepted
Date: 2026-08-24

## Context

Crump already has exact Project-owned WAV assets, deterministic off-callback decode/resampling, and a separate sampler Instrument. Milestone 4 requires the same source material inside Composable DSP Graphs without duplicating media authority, introducing callback allocation, or implying independent time stretching. It also requires readable multisample zones rather than an opaque mapping database.

The graph source must coexist with Track-owned graph voices, Parameters and Modulation, ADR 0011 publication transitions, and two-runtime admission. Asset bytes may be stereo and may use a different source rate, so source-frame regions must remain exact while runtime cursors address deterministic Project-rate derivatives.

## Decision

### Exact source and derived sample cache

`sample_source` and `multisample` are runnable source-version-2 stereo voice Nodes. They reference Project-owned exact-hash WAV Media Assets declared with `usage sample` or `usage multisample`. External paths remain import inputs only; source declarations retain normalized Project-relative paths and complete-byte SHA-256 identities.

`crump-sample-windowed-sinc-v1` decodes and converts every asset reachable from a Track-assigned sampler or graph program to immutable Project-rate stereo frames away from the callback. Mono duplicates to both channels without gain change. Source-rate conversion uses the existing deterministic windowed-sinc law; cache identity hashes the versioned algorithm, exact source SHA-256, and Project sample rate. Unreferenced assets and Instruments allocate no decoded runtime derivative.

Before decoding frame payloads, compilation computes their exact converted frame counts from verified container metadata and adds conservative object, string, Arc-header, and stereo-frame storage to the complete two-resident-runtime admission estimate. Final program accounting counts shared frame payload once and every retained graph/sampler source handle. Non-finite input, missing/changed assets, invalid regions, unsupported formats, and combined work/memory excess fail before publication.

### Graph sample source

A `sample_source` requires typed `pitch` and `gate` inputs and optionally consumes Control `gain` and `velocity`. Its scalar settings are:

- normalized `start`/`end` within the selected slice;
- `loop_mode off|forward|ping_pong`, normalized loop bounds, and a bounded source-frame crossfade;
- initial `direction forward|reverse`;
- `playback gated|one_shot`;
- root pitch, semitone tune, positive repitch `rate`, velocity response, level, and pan; and
- an optional `selected_slice` ID.

`slice ID START END` directives declare at most 256 exact half-open original-source frame regions. When slices exist and no explicit selection is authored, declaration order selects the first. Start/end and loop values are note-on-latched from the complete same-frame Parameter projection; validation requires their full source/Parameter domains to remain ordered. This makes Modulation deterministic without moving an active cursor discontinuously.

Pitch is ordinary repitch: playback increment is note/root ratio × semitone tune × rate. Pitch and duration therefore change together. No time stretching or formant preservation is implied. Runtime uses bounded linear frame interpolation. Forward loops crossfade into the loop start; reverse traversal uses the symmetric boundary; ping-pong reflects direction. Gated playback applies a fixed 64-frame internal release when gate falls, so a loop cannot create an unbounded source tail. One-shot ignores gate after excitation and cannot enable a loop; it ends at the selected region boundary.

### Multisample map

A `multisample` requires `pitch`, `gate`, and normalized `velocity`, with optional `gain`. It owns 1–256 ordered `zone` directives. Each zone declares a stable ID, exact asset, inclusive key and velocity ranges, root pitch, layer ID, articulation, round-robin group/index, exact half-open original-source region, gain, and pan.

Amended 2026-11-12: only the asset, keys, and root are required. The other clauses may follow the ID in any order and default to velocity `0 1`, layer and articulation `default`, the whole asset, `0db`, and centre pan. A zone without `round_robin` joins its layer's implicit group; because a group sounds one matching zone per note, non-overlapping zones there behave as if each had its own group. The defaults are in [the language reference](../language.md).

The first stable articulation vocabulary is only `default`; other authored values fail closed rather than pretending an unavailable selector exists. Every matching round-robin group chooses exactly one zone from stable index order using Project/Track/voice/note/Node seed identity. Distinct groups layer additively with their explicit gains and pans. Selection allocates nothing, scans a bounded precompiled group/index program only on note start, and remains callback-grouping independent. Global tune/rate/velocity/level/pan settings then use the same repitch and gated/one-shot laws as the single sample source.

SFZ import is not part of this slice. A later importer may translate a strict subset into these ordinary readable zone directives, but an external SFZ path can never become runtime authority.

### Ownership, transitions, and inspection

Every cursor, selected-zone set, gate release, and round-robin seed belongs to one graph voice inside its referencing Track runtime. Immutable decoded frames are shared through `Arc`; no cursor or expressive state is Song-global.

Asset/cache identity, slice choice and frame bounds, loop/playback/direction/root structure, zone maps, region/gain/pan facts, and round-robin grouping are transition compatibility facts. Changes restart only the affected Track Instrument behind ADR 0011's bounded equal-power transition. Compatible scalar Parameter and route edits retain active state; note-on-latched region values affect subsequent retriggers.

Inspection reports source/container/project rates, channels, converted frame count, exact source hash, versioned cache identity, and admitted bytes. Generated Instrument rows show verified waveform previews, slice regions, zone key/velocity/root/round-robin mappings, and exact source locations without owning playback state.

## Consequences

Graph sample and multisample fixtures parse, validate, compile, render finite Audio, and produce identical output under supported callback groupings. Objective checks cover exact region rejection, rate/direction, forward and ping-pong loops, velocity zones, deterministic round robin, cache rebuild identity, source navigation, transition compatibility, and resource admission. Copying their Project directories requires no external sample path.

These mechanics establish portable deterministic source playback, not producer credibility or transparent time manipulation. Independent listening may motivate additional mappings, articulations, loop laws, or import adapters without weakening the exact source and realtime boundary.

Bounded granular behavior was subsequently accepted by [ADR 0021](0021-bounded-granular-runtime.md). Deterministic spectral-peak behavior was subsequently accepted by [ADR 0022](0022-deterministic-spectral-peak-runtime.md).

## Rejected alternatives

- A second graph-only decoder/cache would duplicate exact media and resampling semantics.
- Retaining sample-library or SFZ paths would violate Project portability and History.
- Treating rate as time stretch would misstate pitch/duration behavior.
- Dynamic zone/cursor collections or callback decoding would violate bounded realtime execution.
- Implicit key-zone precedence would hide layering and round-robin behavior; groups and indexes are authored.
- Accepting unsupported articulations as labels would create valid-looking no-ops.
