# Native Synthesis Compatibility

Crump accepts graph-authored native Instrument behavior in stable source versions 1 and 2. This page records the current compatibility boundary; it does not promise unfinished synthesis families merely because their source names are reserved for fail-closed diagnostics.

## Supported behavior

A graph-authored Instrument contains:

- stable Parameters with physical units and bounded domains, including normalized macro sources;
- a Faceplate mapped directly or through an acyclic macro route DAG to audible Parameters;
- one typed per-voice Composable DSP Graph and one optional Track-owned shared post-voice Graph;
- bounded Track-owned mono/poly voicing with explicit legato/retrigger, log-frequency glide, note priority, deterministic stealing, replacement crossfades, stereo voice summing, and one explicit `voice_sum.audio` crossing;
- explicit Event, mono/stereo Audio, and uni/bipolar Control Ports and Buses;
- legacy oscillators plus a stereo virtual-analog oscillator with bounded unison, deterministic phase/drift, typed FM/PM/sync/AM/ring ports, ordered phase warps and post-read distortion;
- a stereo exact-asset wavetable oscillator with deterministic mip compilation, frame morphing, phase warps, unison, typed FM/PM/sync/gain/velocity ports, and immutable callback lookup;
- stereo graph `sample_source` and `multisample` Nodes with exact slices/zones, deterministic repitch, direction/loops, velocity layering, seeded round robin, and bounded gate release;
- a bounded stereo `granular` Node with preallocated semantic-identity grains, deterministic density scheduling, windows, edge laws, scan/freeze, spray/direction, and bounded tails;
- a deterministic stereo `spectral` Node with off-callback 2,048/256 periodic-Hann peak analysis, at most 128 preallocated sinusoidal phases, independent scan/rate and pitch, bounded frequency regions, transient policy, phase randomization, and immutable cache identity;
- AHDSR envelopes with curves/retrigger/synchronized segments, reusable expressive/custom LFOs, noise, mono/stereo filters, amplification, panning, explicit channel conversion, stereo mixing, and bounded stereo summing;
- stable Instrument-owned Modulation sources and routes with per-voice, per-Track Instrument, or Song-global state scope, route curves/operators, auxiliary multiplication, bypass, deterministic order, same-frame primary/auxiliary graph-Control dependencies, and preallocated compatible-publication ramps;
- shared graph processing once per Track before the ordered Instrument processor chain;
- sound-neutral presentation sections and deterministic generated Workspace views with exact source navigation and checked Parameter/route edits; and
- exact source locations, route contribution provenance, resource estimates, inspection, History, playback, and render behavior.

Graph validation rejects missing or duplicate IDs, invalid topology, incompatible Port types or units, implicit channel conversion, unbounded or otherwise implicit fan-in, cycles that do not pass through a `delay_line` input, disconnected required inputs, non-finite values, unsupported settings, and resource excess. `sum` (mono) and `stereo_sum` are the only repeated Audio inputs in this slice; each is bounded to at most 64 inputs and folds its inputs in durable Bus-ID order. Compilation and immutable runtime construction occur away from the audio callback.

The Project media model accepts exact-hash Project-owned WAV assets. Sampler Instruments and graph sample/multisample/granular/spectral Nodes share one off-callback decode/resample cache and provide the bounded regions, loops, direction, repitch, zone, velocity, grain, and scan behavior documented in the [language guide](language.md). Spectral assets additionally derive immutable versioned sinusoidal-peak analysis from those Project-rate frames. Wavetable assets instead declare an exact mono periodic-frame interpretation, normalization, and morph law; off-callback compilation derives immutable Nyquist-bounded mip levels without changing the authoritative bytes.

## Source compatibility

`crump 1`, `crump-module 1`, `crump 2`, and `crump-module 2` are stable headers and must be the first non-comment declaration. Version 1 retains its existing grammar and behavior, including inline-only Node settings. A version-1 root imports version-1 modules only; a version-2 root may compose version-1 and version-2 modules. Unsupported versions and candidate suffixes such as `2-alpha` fail closed.

Version 2 accepts the complete version-1 floor; version 1 rejects Instrument-owned Modulation rather than interpreting it partially. Legacy `connect FROM -> TO` declarations receive deterministic graph-local compatibility IDs from an internal namespace disjoint from valid authored IDs. Version 2 also admits the durable authored `bus ID FROM -> TO` form, explicit voice/shared graph domains, ordered lossless Node settings with exact key/value sites, conversion/mixing/summing Nodes, macro-capable Parameters, Instrument-owned `modulation` blocks, expressive `voicing`, sound-neutral `presentation`, `analog_oscillator`, `wavetable`, `sample_source`, `multisample`, `granular`, `spectral`, `stereo_filter`, the generic feedback and signal primitives, AHDSR extensions, expressive/custom graph LFO settings, and complete wavetable Media Asset metadata. Authored Bus, presentation, exact-asset derivative identities, and source sites appear in inspection and navigation. Instrument Parameter values and route depths/bypasses have exact checked-edit sites.

Crump does not accept a Node or setting as runnable unless parsing, validation, compilation, inspection, resource accounting, playback, rendering, and failure behavior are defined for it. Unsupported declarations never compile as silent no-ops. An invalid Candidate leaves the prior Active Revision unchanged and audible; playback and rendering continue to consume the exact Active snapshot rather than unchecked source or media.

## Unsupported synthesis boundaries

`analog_oscillator`, `wavetable`, `sample_source`, `multisample`, `granular`, `spectral`, `stereo_filter`, and the generic primitives `delay_line`, `math`, `stereo_math`, `shaper`, `to_audio`, `to_control`, `dc_block`, and `linear_phase_lowpass` are runnable source-v2 Nodes. Graph cycles are valid only through a `delay_line` input ([ADR 0026](adr/0026-feedback-and-waveguide-primitives.md)). Candidate headers such as `2-alpha` remain invalid, and every unknown or malformed Node/setting/directive fails closed rather than becoming a runtime no-op.

The current runnable model has no:

- graph cross-domain route other than the single typed `voice_sum.audio` crossing;
- transparent or formant-preserving spectral time stretching;
- implicit mono/stereo conversion or implicit graph fan-in;
- opaque native preset, dynamically loaded plug-in, or arbitrary Rust extension; or
- external authoritative sample-library path.

## Current ownership rules

Song Automation, Pattern Parameter Locks, Instrument-owned routes, Song-owned routed LFO Modulation, and the Envelope Follower use the precedence and ownership documented in the language guide. Effective values fold base → Automation → lock → Instrument routes → Song routes/Envelope Followers → constraint. Parameter-lock lanes are keyed by durable Track ID, and Song-global Parameter sources that would ambiguously consume a Track-owned lock fail validation. Instrument routes travel with the Instrument; global Instrument state is Song-owned, Instrument-scope state belongs to each referencing Track runtime instance, and per-voice note-relative state continues through release. Voicing held-note/allocation state, graph LFO/envelope/oscillator state, sample/multisample cursor and zone-selection state, granular grain-pool/spawn state, spectral scan/phase state, and shared DSP state are also owned separately by each referencing Track. A shared graph uses the singular per-Track projection: it includes global/Instrument and non-per-voice Song contributions while omitting every per-voice contribution. Its Node, control, and tail state also belongs to that Track and resets on an explicit off-callback seek. Song routes remain arrangement state. Project-owned media bytes and `.crump` source are Authoritative Source; decoded/resampled sample frames, compiled wavetable mips, spectral peak analyses, and graph programs are disposable runtime representations. Generated views project authored presentation metadata plus any unassigned topology and do not own sound state. Presentation-only edits therefore do not alter semantic audio hashes or restart runtime state.

## Stable algorithm and cache identities

These names are compatibility identities, not mutable cache filenames. All caches are disposable, rebuilt in memory during compilation, and bound to exact authoritative source/media identities.

| Layer | Stable identity | Compatibility input |
| --- | --- | --- |
| Project language | `crump 2` / `crump-module 2` | complete source grammar and fail-closed semantics above |
| Engine/build domain | Cargo package version plus executable SHA-256 | exact build and documented `x86_64` comparison domain |
| Sample decode/project-rate cache | `crump-sample-windowed-sinc-v1` | source WAV SHA-256 and Project sample rate |
| Wavetable mip cache | `crump-wavetable-radix2-v1` | source SHA-256, sample rate, frame structure, normalization, and morph law |
| Spectral peak cache/resynthesis | `crump-spectral-peaks-v1` | source/sample-cache identity, sample rate, 2,048/256 analysis shape, and peak limit |
| Pattern Variation identity | `crump-pattern-variation-v1` | semantic Pattern/Variation inputs |
| Pattern/Event schedule identities | `crump-pattern-event-v1`, `crump-event-instance-v1`, `crump-event-v1` | durable semantic event path and deterministic seed inputs |
| Offline qualification | `crump-offline-wall-clock-block-profile-v1` | exact build, host, Project revision, range, quantum, and threshold |

Stable source v2 is resolved. Sound compatibility remains experimental until the independent-producer and named-hardware protocol in [the qualification runbook](qualification/native-synthesis.md) passes. In particular, filter palette/defaults, category-level listening compatibility, starter content credibility, and final Serum-class acceptance may still change; the stable source parser will fail closed rather than silently reinterpret unsupported sound behavior.
