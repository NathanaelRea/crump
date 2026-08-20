# Architecture Overview

Status: First-release implementation present; external certification gates remain open

## Fixed constraints

- Rust is the implementation language.
- Ratatui is the first client, while Project, authoring, persistence, engine, realtime, and render modules remain terminal-independent ([ADR 0007](../adr/0007-terminal-first-client-boundary.md)).
- The Linux baseline is native PipeWire, not a JACK compatibility layer ([ADR 0002](../adr/0002-native-pipewire-linux-baseline.md)).
- Readable declarative source is authoritative ([ADR 0010](../adr/0010-small-declarative-project-language.md)); Production Views make checked structure-preserving replacements ([ADR 0001](../adr/0001-structure-preserving-source-edits.md)).
- Invalid Candidate Revisions never partially or implicitly replace Active audio.
- Playback and offline rendering share compiled schedule and DSP semantics.
- The process callback receives only fully built state and performs bounded work: no parsing, graph construction, allocation/deallocation by design, blocking locks, filesystem/console I/O, or UI work.
- Persistence, restore, Module installation, and render publication are failure-atomic.
- Active identity, diagnostics, graph/Parameters, History, transport, activity, safety levels, and render provenance remain inspectable.
- First-release scope is the accepted composition, graph/Parameter, external-Agent, Production View, history, and transition slice in ADRs 0003–0011.

## Implemented responsibility chain

```text
project.crump + imported modules
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
        schedule/graph compile    offline render
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
| `model` | terminal-independent Project/Song/Instrument/Pattern/Track/graph domain |
| `dsl` | full-fidelity ranges, block grouping, recoverable Project/Module parsing |
| `validate` | IDs/references, physical domains, typed DAGs, bounds, Musical Sketch checks |
| `project` | secure import resolution, exact source inventory and revision hashing |
| `edit` | checked literal replacements, stale detection, staged atomic edit publication |
| `history` | exclusive lock, append-only provenance, snapshots, diff, restore, recovery |
| `engine::schedule` | deterministic Clip expansion, Event ordering, IDs, seeds, clipping |
| `engine::graph` | topological native Node compilation and preallocated polyphonic voices |
| `engine::drums` | deterministic sample-free kick/snare/hat/clap synthesis |
| `engine` | shared SongRuntime, Track balance, voice stealing, telemetry, monitor safety |
| `engine::render` | exact Active offline processing and atomic WAV/manifest bundle publication |
| `realtime` | native PipeWire audio/MIDI streams, fixed queues, transitions, recovery telemetry |
| `workspace` | Ratatui Production Views, keyboard commands, external-change observation |
| `cli` | human and schema-versioned external-Agent command boundary |

## Deep seams

### Source and model

`ProjectBundle` owns canonical root, sorted exact source bytes, byte-exact revision, recoverable model, and diagnostics. All clients use this one seam rather than reparsing ad hoc. Parser Nodes retain byte ranges for exact values and whole declarations; arbitrary source is never regenerated from the model.

### Checked authoring

A structured operation resolves a durable domain ID, captures expected revision/range/literal, and produces a `CheckedReplacement`. The transaction rereads source under the Project lock, rejects stale or overlapping ranges, writes staged siblings, validates the complete result, and publishes one action-level History Entry. UI and CLI share these functions.

### History and Active publication

`HistoryStore` serializes all mutation with one project-local file lock. Content-addressed exact snapshots can be reused by multiple action entries. Atomic Active state replacement is provisional until the final committed marker is durable. Startup recovery rolls back a state pointer without that marker, discards uncommitted metadata, and repairs interrupted source transactions before returning a store.

### Musical runtime

`CompiledProject` normalizes Clips and graph topology off-thread. `SongRuntime` is the shared mutable semantic machine for monitoring and rendering. It owns fixed Track/voice/Node state and exposes frame/block processing plus transient audition. Offline mode fails on non-finite output; monitoring adds only an explicitly non-authoritative safety stage.

### Native publication

The controller compiles and provisionally transport-aligns a replacement runtime away from PipeWire. The callback realigns it to its exact current frame through an SPSC ring and performs a 10 ms equal-power transition at a block boundary. Unchanged Tracks retain state; affected Tracks restart at that frame. Retired runtime destruction is returned through another ring. This realizes [ADR 0011](../adr/0011-first-release-graph-transition.md) without restarting transport; exact cross-revision state migration remains deferred.

### External Agents

No model runtime is linked into Crump. Agents read source and use `inspect`, `validate`, checked `edit`, explicit `publish`, History, Module, and render commands through one JSON envelope ([ADR 0005](../adr/0005-external-authoring-agent-boundary.md)). Agent rationale becomes durable provenance; invalid work remains Candidate-only.

## Determinism and failure boundaries

Deterministic identity is semantic where ordering should not matter (Events, seeds, topological schedule) and byte-exact where authorship must remain reviewable (source revisions and snapshots). Rendering records Project/build/platform/frame identity and hashes both artifacts. The byte comparison promise is bounded to the declared build and Linux `amd64` reference environment.

Every irreversible seam stages and fsyncs before an atomic rename or exchange. The intended observable states are complete old or complete new. Release qualification still needs comprehensive fault injection and named-hardware evidence; see [release evidence](../release-evidence.md).

## Destination boundaries

Professional DAW research guides later Validated Slices but does not expand the first release. Samples, recording, automation, advanced routing/mixer topology, plug-in hosting, native extensions, complete-track breadth, exact live state migration, and non-Linux clients remain deferred until human evaluation identifies the next real obstacle.
