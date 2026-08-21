# Architecture Overview

Status: First-release implementation present; Production Control Foundation 1A implemented; Slices 4–6 contain unaccepted roadmap engineering; external certification and Slice acceptance remain open

## Fixed constraints

- Rust is the implementation language.
- Ratatui is the first client, while Project, authoring, persistence, engine, realtime, and render modules remain terminal-independent ([ADR 0007](../adr/0007-terminal-first-client-boundary.md)).
- The Linux baseline is native PipeWire, not a JACK compatibility layer ([ADR 0002](../adr/0002-native-pipewire-linux-baseline.md)).
- Readable declarative source is authoritative ([ADR 0010](../adr/0010-small-declarative-project-language.md)); Production Views make checked structure-preserving replacements ([ADR 0001](../adr/0001-structure-preserving-source-edits.md)).
- Invalid Candidate Revisions never partially or implicitly replace Active audio.
- Playback and offline rendering share compiled schedule, effective-control evaluation, and DSP semantics.
- The process callback receives only fully built state and performs bounded work: no parsing, graph construction, allocation/deallocation by design, blocking locks, filesystem/console I/O, or UI work.
- Persistence, restore, Module installation, and render publication are failure-atomic.
- Active identity, diagnostics, graph/Parameters, History, transport, activity, safety levels, and render provenance remain inspectable.
- First-release scope is the accepted composition, graph/Parameter, external-Agent, Production View, history, and transition slice in ADRs 0003–0011.

## Implemented responsibility chain

```text
project.crump + imported modules + declared WAV bytes
              |
              v
  lossless syntax grouping (dsl/syntax)
              |
              v
 recoverable typed parse (dsl/parser)
              |
              v
whole-Project validation (validate)
        |                  |
        | invalid          | valid + explicit publication
        v                  v
 diagnostics      atomic History snapshot + Active pointer
                           |
                 +---------+----------+
                 |                    |
                 v                    v
 schedule/control/graph compile   offline render
                 |                    |
                 v                    v
       preallocated SongRuntime   float WAV + manifest
                 |
                 v
        bounded PipeWire queue
                 |
                 v
      process callback + telemetry
```

The Candidate is current source bytes. The Active Revision is an exact committed snapshot. The two identities can intentionally differ while a musician inspects valid external work or repairs invalid source.

## Modules

| Module | Responsibility |
| --- | --- |
| `time` | normalized rational musical time, meter, deterministic frame conversion |
| `diagnostic` | serializable stable codes and exact source locations |
| `model` | terminal-independent Project/Song/Instrument/Pattern/Variation/lock/Groove/Clip/Locator/Automation/channel-route/processor domain |
| `dsl` | full-fidelity ranges, block grouping, recoverable Project/Module parsing |
| `validate` | IDs/references, physical domains, typed DAGs, bounds, Musical Sketch checks |
| `project` | secure import/media resolution, exact source-and-asset inventory, revision hashing |
| `media` | bounded portable WAV identity, exact-byte hashing, container metadata, and diagnostics |
| `engine::sample` | immutable WAV decode and deterministic offline Project-rate conversion |
| `edit` | checked literal/binary replacements, stale detection, staged atomic edit publication |
| `history` | exclusive lock, append-only provenance, content-addressed snapshots, diff, restore, recovery |
| `engine::schedule` | off-callback Pattern Variation/lock materialization, deterministic Groove/Clip expansion, Event ordering, IDs, seeds, clipping |
| `engine::control` | stable Parameter-target compilation and callback-safe effective-value evaluation/provenance |
| `engine::graph` | topological native Node compilation, dense control delivery, and preallocated polyphonic voices |
| `engine::drums` | deterministic sample-free kick/snare/hat/clap synthesis |
| `engine::processors` | compiled native shared processor programs and preallocated mutable DSP state |
| `engine::mix_graph` | deterministic Track/Group/Return/Master topology, Sends/sidechains, compensation, metering, inspection, and admission |
| `engine` | shared SongRuntime, Track-source production, compiled mix execution, voice stealing, telemetry, monitor safety |
| `engine::analysis` | streaming finalized-WAV peak, K-weighted loudness, DC, clipping, and non-finite analysis |
| `engine::render` | exact Active offline processing, aligned Track/Group stem capture, finalized-file verification, and atomic multi-WAV/manifest publication |
| `realtime` | native PipeWire audio/MIDI streams, fixed queues, transitions, recovery telemetry |
| `workspace` | Ratatui Production Views, keyboard commands, external-change observation |
| `cli` | human and schema-versioned external-Agent command boundary |

## Deep seams

### Source and model

`ProjectBundle` owns canonical root, sorted exact source bytes, verified immutable `AssetInventory`, byte-exact revision, recoverable model, and diagnostics. Asset-free Projects retain the original source-v1 revision domain; Projects with media add normalized path, exact length, and SHA-256 identities. Filesystem Candidates load regular Project-owned files without symbolic-link traversal, while Active snapshots resolve the same declarations from History-owned exact bytes. All clients use this one seam rather than reparsing ad hoc. Parser Nodes retain byte ranges for exact values and whole declarations; arbitrary source is never regenerated from the model.

### Checked authoring

A structured operation resolves a durable domain ID, captures expected revision/range/literal, and produces a `CheckedReplacement`. `ProjectMutation` can additionally stage bounded exact asset writes/removals. The transaction rereads source and media under the Project lock, rejects stale/overlapping/unsafe/no-replace conflicts, validates the complete in-memory source-and-binary overlay, journals both sides, and publishes one action-level History Entry. External import paths are inputs only; authoritative source receives a normalized `media/ID.wav` path and lowercase hash. UI and CLI share the source-edit functions. Structured operations cover portable-media import/collection and durable Timeline Clips, Locators, arrangement-time transforms, Groove Templates, Pattern Variations, Parameter locks, Events, Automation, and mix controls without flattening procedural declarations.

### History and Active publication

`HistoryStore` serializes all mutation with one project-local file lock. Content-addressed source manifests reference immutable, verified media blobs under `.crump/history/assets/<sha256>` and can be reused by multiple action entries. Legacy sample-free schema-v1 snapshots omit the defaulted asset manifest and retain their exact revisions. Atomic Active state replacement is provisional until the final committed marker is durable. Startup recovery rolls back a state pointer without that marker, discards uncommitted metadata, and repairs interrupted source-and-media transactions to the complete pre- or post-commit state before returning a store.

### Musical runtime

`CompiledProject` recursively materializes acyclic Pattern Variations, inherited Parameter-lock timing, Groove-transformed Clip Events, stable Parameter targets, Automation points, instrument graphs, and one `CompiledMixGraph` off-thread. The authored model retains Variation lineage; callback code sees only bounded immutable schedules and lock lanes. `SongRuntime` is the shared mutable semantic machine for monitoring and rendering. It owns a preallocated `ControlRuntime`, fixed Track/voice/Node source state, and a `MixGraphRuntime` that executes Track, Group, Return, and Master channels in stable topological order. Ordered Instrument post chains, channel inserts, pre/post Sends, sidechains, gain/panner/Mute, convergence compensation, and Master processing all run through that graph.

Every authored frame evaluates base → optional Automation → optional Pattern Parameter lock → authored LFO routes in durable-ID order → constraint before graph and channel DSP. A lock is a step override scoped through its Clip traversal and resets at the Clip end; validation rejects overlapping Clips that would give one Instrument ambiguous lock precedence. Global and Instrument routes use absolute transport phase; graph voices apply precompiled per-voice routes from note-relative phase without callback allocation. Graph Instruments expose velocity to amplitude/timbre paths, envelope/LFO filter modulation, key tracking, bounded glide, four deterministic state-variable filter outputs, and a fixed-capacity stereo unison oscillator with seeded drift. Active graph-voice replacement swaps into a second preallocated state slot and crossfades for 64 frames; both storage and worst-case work enter admission. Offline mode fails on non-finite audio output; effective control holds its prior finite value on a non-finite result; monitoring adds only an explicitly non-authoritative audio safety stage. Transient Solo/cue is passed through the callback-safe command seam and is ignored by offline processing.

Inspection resolves each processor's effective settings/defaults, physical domains, authored-literal and continuous-target provenance, explicit/intrinsic latency, nonlinear evaluations, filtered oversampling factor/tap count, and admitted state/work estimates. Saturation and Limiter share one fixed 4×, 129-tap linear-phase FIR kernel: Saturation filters interpolation and decimation with an aligned 32-frame dry path, while Limiter uses the 16-frame reconstruction latency plus authored structural lookahead and a disclosed fixed 0.1 dB safety margin for inter-sample-peak detection and gain scheduling. Callback telemetry has fixed capacities for every compiled channel and processor; stable `(owner, processor-id)` indexes align telemetry with inspection across compatible publication and currently publish compressor gain reduction. Validation counts repeated Instrument post-chain runtime instances, separately preserves per-voice graph/Node allocations when shared processor state is recalculated, caps processor instances before allocation, and rejects settings or sidechain declarations/routes that the selected native kind does not consume. Utility mid/side narrowing has explicit correlation-safe semantics; EQ high/low cuts, shelves, and parametric bands have finite impulse, measured frequency-response, full-band inspection, and 64/128/256-frame processing-equivalence tests. The linked-stereo feed-forward compressor has explicit peak/RMS detector semantics, one-pole attack/release detection, a continuous soft-knee curve, parallel wet/dry mixing, wet-path makeup, and external-sidechain replacement; vectors and a source-backed kick/bass fixture verify its movement and telemetry. Delay resolves straight/dotted/triplet tempo divisions against the compiled Song tempo, uses interpolated read heads and preallocated time-change crossfades, filters feedback, and supports independent-stereo or cross-fed ping-pong operation; inspection publishes the effective synchronized time and exact admitted state. Reverb uses fixed preallocated stereo comb/gain-neutral-allpass networks with bounded feedback and diffusion, fractional-delay modulation, cross-fed stereo input, wet-side width, and a fractional one-second pre-delay ring with preallocated read-head crossfades; shared Return routing fans all Sends into one continuing processor state whose complete DSP heap is admitted before publication. The callback runtime Arc-shares immutable mix, control, schedule, and Instrument graph programs instead of deep-cloning them. Admission separately counts those programs' owned capacities (including scheduled Event identity strings), the complete `SongRuntime`/Track/control/mix container backing, graph, sampler, and stock-drum voice vectors, unison subvoice work, graph replacement-tail storage/work, machine-word-rounded packed Boolean storage, processor-program clones, auxiliary storage, and the two resident old/new publication slots. These are conservative owned-capacity estimates; allocator-private metadata is not claimed as byte-exact.

### Finalized delivery analysis

`engine::analysis` is the single delivery-measurement seam. It reopens a finalized stereo float WAV, validates its exact format and declared frame count, streams decoded samples through fixed K-weighting and the shared custom 4× reconstruction kernel, and returns a versioned `FinalizedAudioAnalysis`. Analysis schema 2 evaluates integrated 400 ms blocks and three-second Short-term windows at 100 ms hops; Loudness Range derives its `−20 LU` relative gate from the absolute-gated Short-term population before nearest-percentile selection. Sample/reconstructed peak, BS.1770/EBU-derived loudness, per-channel DC, clipping, and non-finite facts therefore describe written bytes rather than transient bus telemetry. `engine::render` rejects non-finite or frame-inconsistent analysis before writing the manifest. Silence and insufficient windows remain explicit `null` values; analysis never normalizes Audio or imposes a universal LUFS target. Published EBU steady-tone vectors are covered, but complete EBU Mode or BS.1770-5 conformance is not claimed.

`engine::qualification` is a separate offline workload-evidence seam. It compiles the exact Active snapshot, prerolls untimed to an explicit frame range, and times supported full-size engine blocks plus one disclosed short final block when needed. Every invocation uses its exact frame deadline; fixed-histogram overflow conservatively reports the observed peak. Its versioned report binds Project Revision, executable/host identity, range, full/final block sizes, finite output, and compile-time admission resources. Atomic report output cannot overlap source, declared media, or `.crump` History. It deliberately leaves PipeWire xruns absent and names external realtime/hardware evidence, so offline speed cannot be mistaken for callback-safety certification.

The mix runtime retains fixed-capacity current pre/post tap samples without allocation. Render planning resolves each selected durable Track/Group owner to that dense tap, computes initial padding as `Master output latency − selected channel output latency`, and freezes an explicit processing policy. Both taps include upstream main routing and owner inserts; post-fader also includes owner gain/panner/Mute; outgoing Sends and downstream Group/Return/Master processing remain outside the file. Tap blocks spool to raw staging shards of at most 16 Stems, then one shard at a time streams into at most 16 alignment-aware WAV writers. This bounds both open descriptors and memory instead of retaining a file or full latency buffer per Stem. Master and every Stem receive the same frame count and tail, then each file is independently finalized, synced, analyzed, and hashed before one directory exchange publishes the exact artifact set. Existing-bundle replacement revalidates manifest names, regular-file status, policy, format, frame count, every hash, and a fresh finalized-file analysis against the manifest. A 192-Stem maximum-topology fixture and a post-spool failure injection cover descriptor pressure, destination preservation, and handled-failure staging cleanup.

### Native publication

The controller compiles, provisionally transport-aligns, and clears every replacement DSP history, delay ring, meter, and oversampling kernel away from PipeWire. One atomic admission slot is reserved before candidate runtime allocation, so the process retains at most the Active runtime plus one queued or transitioning replacement; when that slot is busy, the workspace retains the newest Active revision and retries after transition telemetry clears. The callback performs only bounded cursor realignment to its exact current frame through an SPSC ring and begins a bounded 10 ms transition at a block boundary; it neither sweeps large DSP buffers nor constructs or drops callback-owned heaps. A wholly topology-compatible publication processes each Track source and shared mix graph once, retains compatible voice and processor/compensation state, and applies a preallocated monitoring-only ramp only to changed continuous effective targets; unchanged step/linear Automation remains sample-exact and boolean Mute remains discrete. Configuration or topology changes use the equal-power old/new fade rather than unsafe state adoption. Retired runtime destruction is returned through another ring before the publication slot reopens. This realizes [ADR 0011](../adr/0011-first-release-graph-transition.md) without universal semantic smoothing or transport restart; arbitrary incompatible topology state migration remains deferred.

### External Agents

No model runtime is linked into Crump. Agents read source and use `inspect`, `validate`, checked `edit` operations (including Timeline, Groove, Variation, lock, Automation, and media transforms), explicit `publish`, History, Module, and render commands through one JSON envelope ([ADR 0005](../adr/0005-external-authoring-agent-boundary.md)). Agent rationale becomes durable provenance; invalid work remains Candidate-only.

## Determinism and failure boundaries

Deterministic identity is semantic where ordering should not matter (Events, seeds, topological schedule) and byte-exact where authorship must remain reviewable (source/media revisions and snapshots). Rendering records Project/build/platform/frame identity and hashes both artifacts. The byte comparison promise is bounded to the declared build and Linux `amd64` reference environment.

Every irreversible seam stages and fsyncs before an atomic rename or exchange. The intended observable states are complete old or complete new. Release qualification still needs comprehensive fault injection and named-hardware evidence; see [release evidence](../release-evidence.md).

## Destination boundaries

Professional DAW research guides later Validated Slices but does not expand the certified first-release claim. Roadmap engineering now includes source-backed point Automation, routed LFO Modulation, stable Instrument/channel/Send/processor/Master targets, the compiled mix graph and processor floor, portable WAV ownership plus sampler playback, expressive native Instrument contracts, and checked Slice 6 Clip/Locator/arrangement/Groove/Pattern-Variation/Parameter-lock workflows. Those capabilities are not a claim that Slice listening, named-hardware, accessibility, complete-track, or release-certification gates have passed. Media Tracks, recording, time stretching, non-LFO modulation sources, plug-in hosting, native extensions, arbitrary incompatible topology state migration, and non-Linux clients remain deferred or later roadmap work.
