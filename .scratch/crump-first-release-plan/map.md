# Chart Crump's Musical-Sketch First-Release Plan

Label: `wayfinder:map`

## Destination

An implementation-ready plan for the narrow First Public Release defined by `requirements/product-contract.md` and `docs/product/first-public-release.md`: a terminal-capable Beginner Musician starts from a Creative Brief, an external Authoring Agent creates a no-sample 30-60 second Musical Sketch and pitched polyphonic graph instrument, the musician inspects and changes it through Production Views and persistent Project History, and Crump saves, reopens, auditions, and deterministically renders it. An Experienced Producer independently validates transfer and musical credibility.

## Notes

- Canonical product and release documents outrank this workspace-local map and every child issue.
- This map supersedes the former 3-5 minute professional-DAW first-release plan. Historical tickets and research remain evidence, not current scope.
- Implementation begins only after [Synthesize the Dependency-Ordered Implementation Backlog](issues/22-synthesize-implementation-backlog.md) resolves.
- Every session reads `CONTEXT.md`, relevant tracked docs, and this map before claiming a ticket.
- Research professional analogues only for a task exercised by the Musical Sketch or for a required safety, determinism, persistence, recovery, or support guarantee.
- Prefer a thin `Creative Brief -> external file edit -> validation -> explicit Agent publication -> safe graph update -> Production View edit -> history restore -> save/reopen -> WAV render` tracer bullet.
- The complete production environment remains the destination, but every expansion after this release is selected from Beginner Musician and Experienced Producer evidence as one Validated Slice.

## Confirmed Direction

- [Former Reference-Project contract](issues/01-freeze-reference-project-contract.md) - The old sample-using 3-5 minute gate is superseded by the canonical Musical Sketch workflow and dual human evaluation.
- [Safe graph replacement evidence](issues/02-prove-safe-graph-replacement.md) - Retain bounded publication and retirement evidence; ADR 0011 now permits a visible protective fade and Affected Instrument Restart.
- [Structure-preserving source edits](issues/03-prove-structure-preserving-edits.md) - Lossless checked edits and exact persistent Project History support Production Views and Agent authorship.
- [Linux audio and MIDI baseline](issues/04-establish-linux-baseline.md) - Ubuntu 26.04 `amd64`, native PipeWire stereo audio, RTKit, and one transient MIDI Note Audition setup remain the release fixture.
- [CLAP research](issues/05-assess-clap-hosting.md) - Findings remain post-release evidence; third-party plugin hosting is not a release dependency.
- [Composition semantics](issues/06-define-composition-model.md) - The first-release subset is one finite Song, fixed tempo/meter, concrete Events in finite Patterns, Clips, exact Musical Time, and deterministic playback/reopen/render.
- [Graph and Parameter semantics](issues/07-define-routing-model.md) - The first-release subset is typed hierarchical DSP, physical-unit Parameters, required modulation, simple Track balance into Master, minimal creative effects, and monitoring safety.
- [Authoring-direction discovery](issues/08-prototype-authoring-balance.md) - Production Views lead human work; source remains durable and always available; external Agents use files plus machine-readable CLI publication.
- Rust extensions, CLAP, waveform/sample editing, professional mixer breadth, Automation views, stems, and advanced MIDI capture/mapping are resolved as deferred rather than open first-release decisions.

## Planning Frontier

- [09 - Validate the Progressive Authoring Workflow](issues/09-evaluate-authoring-with-musician.md): prototype Timeline, Event Grid, Faceplate, inspectable Graph View, history, and contextual explanations with the two target roles.
- [10 - Freeze the Minimal DSL and Project-Source Contract](issues/10-freeze-dsl-contract.md): specify only syntax and types exercised by the Musical Sketch, graph instrument, Faceplate, Instrument Module, history identity, and render settings.
- [11 - Freeze the Compiled Project and Runtime Transition Contract](issues/11-freeze-runtime-transition-contract.md): implement ADR 0011's affected-instrument restart and bounded real-time ownership.
- [12 - Freeze the Native Musical-Sketch Capability Floor](issues/12-freeze-native-capability-floor.md): graph primitives, stock synthesized percussion, polyphony, minimal effects, level balance, and monitoring safety.
- [13 - Freeze the First-Release Workspace Contract](issues/13-freeze-workspace-contract.md): production views, source, diagnostics, activity/level feedback, keyboard completeness, optional mouse, and history.
- [14 - Freeze Project, History, and Instrument-Module Persistence](issues/14-define-persistence-semantics.md): atomic save/reopen, append-only retained history, source-module reuse, versioning, and recovery.
- [15 - Prove Transient MIDI Note Audition](issues/15-resolve-midi-semantics.md): note-on/off only on the documented producer setup, with no capture or mapping.
- [17 - Freeze Lossless-Master Rendering](issues/17-freeze-rendering-semantics.md): 32-bit-float stereo WAV, manifest, explicit Render Tail, atomic publication, and same-build bit identity.
- [20 - Establish Musical-Sketch Performance Budgets](issues/20-establish-performance-budgets.md): benchmark the actual graph, views, history, publication, playback, and render workload.
- [21 - Define the Narrow Public-Release Support Contract](issues/21-define-support-contract.md): installation, reference harness/model evidence, no-model core, PipeWire/MIDI fixture, diagnostics, and known exclusions.
- [24 - Freeze the External Authoring-Agent Contract](issues/24-freeze-external-agent-contract.md): file editing, JSON CLI inspection/validation/checks/publication/rendering, provenance, skill guidance, and operation with the TUI closed.
- [25 - Freeze the Musical-Sketch Acceptance Fixture](issues/25-freeze-musical-sketch-acceptance.md): exact Creative Brief, reference source, evaluator checklists, objective fixtures, and pass/fail records.
- [22 - Synthesize the Dependency-Ordered Implementation Backlog](issues/22-synthesize-implementation-backlog.md): convert the resolved contracts into the fewest end-to-end tracer bullets.

## Remaining Fog

- Concrete source syntax, parser, type checker, compiler representations, and durable-ID spelling.
- Exact DSP algorithms, graph resource-bound declarations, voice allocation, and minimal-effect quality.
- Production View layouts, command vocabulary, help, focus, and optional mouse interactions.
- File layout and atomic persistence for append-only history and personal Instrument Modules.
- CLI command names and JSON schemas for Agent inspection, checks, publication, diagnostics, and rendering.
- Numerical real-time, interaction, startup, and render budgets plus named audio/MIDI hardware.
- Exact Creative Brief, Sound Brief, Render Tail, test harness/model configuration, and evaluator procedures.
- Test-suite and crate decomposition; these graduate into issue 22 only after their governing contracts resolve.

## Explicitly Deferred

- A polished 3-5 minute complete-track release gate; professional mixer breadth; Groups, Sends, Returns, general Buses, Automation views, and stems.
- Imported samples, waveform/spectrum tooling, destructive audio editing, and model-rendered waveform generation.
- Rust extensions, Agent-generated native code, CLAP/VST hosting, plugin GUIs, and plugin state.
- MIDI capture, correction, mappings, persistence, MPE, OSC, synchronization, export, and DAW interchange beyond transient Note Audition.
- Structured Graph View topology editing, an embedded Agent terminal/PTY, MCP, and harness-specific plugins.
- Live audio recording, session launching, desktop/browser clients, macOS/Windows support, realistic vocals/acoustic emulation, and built-in curricula.
- Implementation itself; this map ends at an executable backlog.
