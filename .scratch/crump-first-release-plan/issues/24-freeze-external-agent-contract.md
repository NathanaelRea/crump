# Freeze the External Authoring-Agent Contract

Type: grilling
Mode: HITL
Status: open
Blocked by: [03 - Prove Structure-Preserving Source-Backed Edits](03-prove-structure-preserving-edits.md), [10 - Freeze the Minimal DSL and Project-Source Contract](10-freeze-dsl-contract.md), [11 - Freeze the Compiled Project and Runtime Transition Contract](11-freeze-runtime-transition-contract.md), [14 - Freeze Project, History, and Instrument-Module Persistence](14-define-persistence-semantics.md)

## Question

What exact file-editing and machine-readable `crump` CLI contract lets a model-neutral external harness inspect, validate, check, explicitly publish, explain, render, and repair a complete Project while preserving per-publication provenance and Project History whether or not the TUI is running?

## Research Baseline

Start from the accepted external-Agent boundary in ADR 0005 and the canonical Musical Sketch workflow. Prototype the smallest JSON command surface against one documented harness/model configuration; do not introduce MCP, a harness-specific plugin, a bundled model, TUI scraping, or an embedded terminal unless the file-and-CLI loop fails with evidence.
