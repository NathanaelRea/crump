# Architecture Overview

Status: First-release constraints accepted by the draft product contract; component seams proposed

## Fixed Constraints

- The implementation language is Rust.
- The first supported client is a Ratatui terminal application; [ADR 0007](../adr/0007-terminal-first-client-boundary.md) keeps the Project model, authoring operations, and engine independent of terminal rendering.
- The engine runs locally and targets Linux for the first public release.
- The small declarative language in [ADR 0010](../adr/0010-small-declarative-project-language.md) is authoritative and follows the structure-preserving edit contract in [ADR 0001](../adr/0001-structure-preserving-source-edits.md).
- Parsing, validation, file access, logging, UI work, and visualization must not block the real-time audio path.
- Invalid candidate revisions do not partially replace the active revision.
- Under [ADR 0011](../adr/0011-first-release-graph-transition.md), a valid first-release topology change may fade and restart only the affected instrument; transport and unchanged Tracks continue.
- Playback and offline rendering share deterministic musical semantics.
- The audio callback performs only bounded, real-time-safe work over prebuilt state: no allocation or deallocation, blocking or contended synchronization, file or console I/O, parsing, graph construction, plugin scanning, or unbounded traversal.
- Performance claims require measurements against named hardware, sample rates, quanta, and the Musical Sketch workload. Callback deadline utilization, xruns, publication latency, UI responsiveness, startup, and render throughput are release evidence.
- Persistence and render outputs use failure-atomic publication. Interruption must leave either the complete prior result or the complete new result, never a mixed project or apparently successful partial artifact.
- First-release graph structure, Parameter provenance, Active Revision, persistent [Project History](../adr/0009-persistent-project-history.md), and render status remain inspectable rather than hidden behind implementation state.
- Composition and timeline behavior follows the First Public Release subset in [ADR 0003](../adr/0003-composition-and-timeline-semantics.md).
- Graph and Parameter behavior follows the First Public Release subset in [ADR 0004](../adr/0004-routing-modulation-and-parameter-semantics.md).
- The first-release reference platform uses the [native PipeWire Linux baseline](../adr/0002-native-pipewire-linux-baseline.md).

## Provisional Responsibility Chain

```text
authoritative source
        |
        v
parse -> validate -> compile -> candidate runtime snapshot
                                  |
                                  v
                            safe publication
                                  |
                                  v
                         active audio graph
```

Production Views read diagnostics and runtime telemetry and emit Structured Source Edits; the source editor emits direct text edits. External Authoring Agents edit the same files and use the file-and-CLI publication boundary in [ADR 0005](../adr/0005-external-authoring-agent-boundary.md). Offline rendering consumes the same validated musical and graph model as playback.

## Design Discipline

Use [the professional DAW research baseline](../research/professional-daw-baseline.md) as destination guidance, not an automatic First Public Release requirement. Adopt an invariant only when it supports the accepted blank-to-render workflow or a required safety, determinism, persistence, or recovery property.

Prefer one deep, inspectable mechanism over parallel special cases. A new component, mode, state store, or runtime path must either complete the reference workflow or enforce a required correctness, real-time, recovery, compatibility, or observability property. Destination ambition cannot justify unused first-release complexity.

## Unresolved Seams

The rewritten first-release planning map must resolve these implementation seams before decomposition is authoritative:

- Durable-ID syntax and reconciliation across source, compiled, and runtime revisions within the accepted identity semantics.
- Parser technology, minimal grammar, and incremental parsing mechanics within [ADR 0001](../adr/0001-structure-preserving-source-edits.md) and [ADR 0010](../adr/0010-small-declarative-project-language.md).
- Bounded graph compilation, publication, and retirement implementing the Affected Instrument Restart in [ADR 0011](../adr/0011-first-release-graph-transition.md).
- Audio and control graph thread ownership implementing the First Public Release subset in [ADR 0004](../adr/0004-routing-modulation-and-parameter-semantics.md).
- Atomic Project save, Instrument Module import, and persistent append-only Project History from [ADR 0009](../adr/0009-persistent-project-history.md).
- Production View structure-preserving operations required by [ADR 0006](../adr/0006-production-views-lead-human-authorship.md).
- PipeWire stereo output and transient MIDI Note Audition on the [reference platform](../adr/0002-native-pipewire-linux-baseline.md).
- Lossless-master range, tail, failure, and deterministic comparison semantics shared by playback and rendering.

Accepted outcomes that are consequential and hard to reverse belong in `docs/adr/`; this overview should then link to them rather than duplicate their rationale.
