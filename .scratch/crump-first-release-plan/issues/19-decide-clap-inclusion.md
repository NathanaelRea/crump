# Make the First-Release CLAP Inclusion Decision

Type: grilling
Mode: HITL
Status: resolved
Blocked by: [05 - Assess CLAP Hosting Against Crump's Release Constraints](05-assess-clap-hosting.md), [11 - Freeze the Compiled-Project and Runtime-Transition Contract](11-freeze-runtime-transition-contract.md), [13 - Freeze the First-Release Workspace Interaction Contract](13-freeze-workspace-contract.md), [14 - Define Project Persistence, Asset, and Compatibility Semantics](14-define-persistence-semantics.md), [17 - Freeze Offline Rendering and Stem Semantics](17-freeze-rendering-semantics.md), [23 - Prove Deadline-Safe Isolated CLAP Hosting](23-prove-isolated-clap-hosting.md)

## Question

Given feasibility evidence and resolved runtime contracts, is CLAP included, experimental, or deferred, and what exact compatibility and failure guarantees apply to that status?

## Research Baseline

Before deciding, refresh official Ableton and other professional DAW plugin-hosting, scanning, quarantine, latency, state, GUI, and recovery behavior, then verify each obligation against the current CLAP specification. Use that as the baseline. Include only the narrowest support tier Crump can test and state honestly; native production must remain complete when CLAP is deferred or unavailable.

## Answer

Deferred. The First Public Release is entirely native and proves the graph-authored synthesis workflow before accepting plugin scanning, opaque state, vendor GUIs, latency, isolation, crash, and reproducibility obligations. Issue 05 remains post-release research; CLAP is not experimental release scope.
