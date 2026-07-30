# Freeze Project, History, and Instrument-Module Persistence

Type: grilling
Mode: HITL
Status: open
Blocked by: [10 - Freeze the DSL and Project-Source Contract](10-freeze-dsl-contract.md), [11 - Freeze the Compiled-Project and Runtime-Transition Contract](11-freeze-runtime-transition-contract.md)

## Question

How are Project files, DSL/build identity, atomic save/reopen, every append-only Project History Entry, History Restore references, Agent provenance, personal Instrument Modules, and lossless-render publication stored and recovered without making metadata authoritative over source?

## Research Baseline

Start from ADR 0009 and the no-sample Reference Project. Retain every history entry without automatic expiry or compaction; History Restore appends rather than deleting or branching. Define simple readable Instrument Module copy/import and atomic failure recovery. Defer sample assets, plugin state, extension packages, and broad dependency acquisition.
