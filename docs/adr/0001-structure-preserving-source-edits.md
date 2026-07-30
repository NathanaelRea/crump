# ADR 0001: Structure-Preserving Source Edits

Status: Accepted
Date: 2026-07-23

## Context

Crump's source files are authoritative, but its Production Views must support structured editing. Reprinting an abstract syntax tree after each structured operation would risk changing unrelated formatting and comments, flattening procedural expressions, and creating Project History inconsistent with direct source edits.

Syntax-node identity also has limited scope. Roslyn, SwiftSyntax, Rowan, and tree-sitter do not promise durable node identity across arbitrary reparses or process restarts. Crump needs to distinguish editor-local occurrence tracking from durable musical identity.

## Decision

- Parse source into a lossless concrete syntax representation that retains every token, comment, whitespace run, malformed region, and exact literal spelling. Rowan is a leading Rust candidate, not fixed by this decision.
- Execute each structured operation as one checked transaction containing the smallest ordered set of non-overlapping text replacements. Verify the expected source revision, target kind, and current target text; then reparse and validate before publication.
- Preserve every byte outside the declared replacement ranges. Format only newly synthesized syntax when an operation requires it.
- Do not replace a procedural expression with its current scalar value. A structured command may edit a recognized literal leaf, perform an explicitly expression-aware transformation, or report that the construct remains source-only.
- Use hybrid identity with three scopes: snapshot identity for one parsed revision, best-effort edit-chain identity across related in-memory revisions, and durable domain identity recovered from authoritative source.
- Use semantic names and paths where rename stability is unnecessary. Add explicit source-carried IDs selectively to domain entities that must retain identity across rename, movement, reload, or restart; do not add IDs to every syntax occurrence.
- Put direct and structured source edits in one transaction-based Project History. One user action creates one entry; a History Restore republishes exact prior source bytes as a new entry without deleting existing history.

## First Public Release Scope

Structured editing is required for the Timeline, Event Grid, Instrument Faceplate, and exact Parameter views exercised by the Reference Project. Automation lanes, mixer and routing views, and structured Graph View topology editing enter only through later Validated Slices; their destination semantics do not expand the first-release editor.

## Consequences

- Comments, unusual formatting, and unsupported expressions can survive unrelated TUI edits without requiring a canonical formatter.
- Stale or ambiguous TUI targets fail rather than modifying the wrong construct.
- Source-ID syntax, duplicate handling, copy/paste behavior, and which domain entities require durable IDs remain DSL and domain-model decisions.
- Parser technology, grammar, incremental parsing, identity reconciliation, and compact history storage remain implementation decisions constrained by this contract.
- Semantic rename must resolve a declaration and its references and apply all replacements atomically.

## Alternatives Considered

- Regenerate an enclosing declaration or file from an AST: rejected because it cannot guarantee byte preservation outside the intended edit.
- Use semantic qualified names as all durable identity: rejected because rename and movement would break identity for entities that need continuity.
- Put explicit IDs on most syntax constructs: rejected because it adds source noise and duplicate, merge, generation, and copy/paste rules without a demonstrated domain need.
- Treat syntax-node object identity as durable: rejected because reviewed parser platforms do not provide that contract across arbitrary reparse or restart.

## Evidence

- [Resolving issue](../../.scratch/crump-first-release-plan/issues/03-prove-structure-preserving-edits.md)
- [Accepted interactive prototype](../../.scratch/crump-first-release-plan/assets/structure-preserving-edit-prototype/README.md)
- [Primary-source industry research](../../.scratch/crump-first-release-plan/assets/structure-preserving-edit-identity-research.md)
