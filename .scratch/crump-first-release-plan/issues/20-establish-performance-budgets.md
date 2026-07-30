# Establish Musical-Sketch Performance and Reliability Budgets

Type: prototype
Mode: HITL
Status: open
Blocked by: [02 - Prove Safe Text-to-Audio Graph Replacement](02-prove-safe-graph-replacement.md), [04 - Establish the Supported Linux Audio and MIDI Baseline](04-establish-linux-baseline.md), [11 - Freeze the Compiled-Project and Runtime-Transition Contract](11-freeze-runtime-transition-contract.md), [12 - Freeze the Native Release Capability Floor](12-freeze-native-capability-floor.md), [13 - Freeze the First-Release Workspace Interaction Contract](13-freeze-workspace-contract.md), [17 - Freeze Offline Rendering and Stem Semantics](17-freeze-rendering-semantics.md)

## Question

On the supported reference system, what callback, xrun, Agent publication, Affected Instrument Restart, MIDI Note Audition, Production View response, history, startup, and WAV-render budgets are realistic for the Musical Sketch workload?

## Research Baseline

Use primary PipeWire and real-time audio sources plus measurements on named reference hardware. Benchmark the accepted graph instrument, stock percussion, minimal effects, required views, retained history, publication path, MIDI audition, playback, and master render. Do not include CLAP, stems, waveform visualization, or professional mixer workloads.
