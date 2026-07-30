# Prove Transient MIDI Note Audition

Type: prototype
Mode: HITL
Status: open
Blocked by: [04 - Establish the Supported Linux Audio and MIDI Baseline](04-establish-linux-baseline.md), [11 - Freeze the Compiled-Project and Runtime-Transition Contract](11-freeze-runtime-transition-contract.md), [12 - Freeze the Native Musical-Sketch Capability Floor](12-freeze-native-capability-floor.md), [13 - Freeze the First-Release Workspace Interaction Contract](13-freeze-workspace-contract.md)

## Question

Can one documented USB MIDI setup send transient note-on and note-off input through PipeWire to a selected Native Instrument with bounded timing, visible activity, correct stuck-note recovery, and no durable Project change?

## Research Baseline

Use PipeWire and MIDI 1.0 primary specifications plus ADR 0002. Scope is live producer audition only. MIDI capture, timing correction, controller mappings, persistence, clock synchronization, MPE, and export are explicitly deferred and must not enter the source model.
