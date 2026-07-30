# Freeze the Minimal DSL and Project-Source Contract

Type: grilling
Mode: HITL
Status: open
Blocked by: [03 - Prove Structure-Preserving Source-Backed Edits](03-prove-structure-preserving-edits.md), [06 - Define the Composition and Timeline Semantic Model](06-define-composition-model.md), [07 - Define the Routing, Modulation, and Parameter Semantic Model](07-define-routing-model.md), [09 - Evaluate the Authoring Prototype With a Representative Musician](09-evaluate-authoring-with-musician.md)

## Question

What minimal syntax and semantic contracts express the Musical Sketch's Project, Song, fixed tempo/meter, Tracks, concrete Events and Patterns, Clips, typed DSP graph, Parameters, envelopes/modulation, Instrument Faceplate, Instrument Module, imports, durable IDs, render settings, diagnostics, and safe structure-preserving edits?

## Research Baseline

Start from ADRs 0001, 0003, 0004, and 0010 and include only constructs exercised by the Reference Project. Groups, Sends, Returns, general mixer Buses, Song Automation, samples, plugins, Rust extensions, advanced Pattern transforms/probability, and a general-purpose programming model are destination semantics rather than first-release grammar obligations.
