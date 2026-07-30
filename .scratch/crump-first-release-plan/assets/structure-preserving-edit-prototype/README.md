# PROTOTYPE: Structure-Preserving Source-Backed Edits

## Question

Can a minimal lossless DSL representation apply parameter, automation, and rename edits while preserving unrelated bytes and procedural expressions, maintaining useful source identity, rejecting stale edits, and sharing exact undo/redo with direct text changes?

This is throwaway planning evidence, not Crump's parser or DSL design. The deliberately tiny grammar tests editing mechanics, not final syntax.

## Identity Model Under Test

- Rowan syntax nodes and source ranges identify occurrences only within an in-memory revision.
- Semantic names locate ordinary declarations and automation points in the current revision.
- The track carries a selective durable source ID, `trk-bass`, which survives rename and reload.
- Every structured command checks its expected revision and source text before applying.

This follows the recommendation in [the supporting industry research](../structure-preserving-edit-identity-research.md).

## Run

From this directory:

```sh
bash run.sh
```

Try these commands and inspect the full source after each one:

```text
gain 0.91
point drop 0.63
rename sub
cutoff 0.5
conflict
undo
redo
reload
quit
```

`cutoff` deliberately refuses to flatten the `lfo(...)` expression. `conflict` simulates a direct text edit landing after a TUI locator was captured and demonstrates stale-edit rejection. Undo restores exact prior bytes.

## Deliberate Limits

- The parser recognizes only enough structure for this editing experiment.
- The source syntax is a reaction artifact, not a proposal for the final Crump DSL.
- The prototype uses whole-source snapshots for undo; production storage is an implementation decision.
- Rename updates only one declaration because references are outside this fixture. A real semantic rename must resolve and patch all references in one transaction.
