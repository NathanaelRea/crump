# Prove Deadline-Safe Isolated CLAP Hosting

Type: prototype
Mode: HITL
Status: resolved
Blocked by: [11 - Freeze the Compiled-Project and Runtime-Transition Contract](11-freeze-runtime-transition-contract.md), [13 - Freeze the First-Release Workspace Interaction Contract](13-freeze-workspace-contract.md), [14 - Define Project Persistence, Asset, and Compatibility Semantics](14-define-persistence-semantics.md), [17 - Freeze Offline Rendering and Stem Semantics](17-freeze-rendering-semantics.md)

## Question

Can a `clack-host`-based worker architecture scan and run a representative CLAP corpus outside Crump's core process while meeting provisional PipeWire callback deadlines, containing plugin crashes and hangs, preserving state, reporting latency, and supporting the selected Linux GUI fallback without hidden durable state?

## Research Baseline

Before prototyping, research current official professional DAW behavior for plugin scanning, process isolation, quarantine, latency, state recovery, and GUI fallback, starting with Ableton and then hosts that document crash protection. Verify all host duties against current CLAP and PipeWire specifications. Use that as the baseline for the smallest isolating architecture that can be measured under deadline and fault injection; unsupported plugins must fail explicitly without weakening native startup or project integrity.

## Answer

Deferred beyond the First Public Release together with all third-party plugin hosting. No isolated CLAP worker prototype is required before the native Musical Sketch implementation backlog. Reopen this as post-release research only if human evidence selects plugin interoperability as a future Validated Slice.
