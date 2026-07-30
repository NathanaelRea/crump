# Prove Structure-Preserving Source-Backed Edits

Type: prototype
Mode: HITL
Status: resolved
Blocked by: none

## Question

Can a minimal DSL representation apply parameter and automation edits while preserving unrelated comments, formatting, expressions, stable source identity, and undoability well enough for authoritative source files?

## Answer

Yes, at prototype fidelity. The accepted planning baseline is:

- Use a lossless concrete syntax representation and checked minimal text transactions. Every byte outside declared replacement ranges remains identical, and edited source is reparsed and validated before use.
- Preserve procedural expressions. A scalar control may edit a recognized literal leaf, use an explicitly expression-aware rewrite, or refuse the operation; it must not silently flatten an expression to its current value.
- Separate snapshot and edit-chain syntax identity from durable domain identity. Use ephemeral revision-local locators, semantic names for current-revision lookup, and explicit source IDs only for domain entities that demonstrably need continuity across rename, movement, reload, or restart.
- Reject stale or ambiguous structured targets by checking the expected revision, syntax role, and current target text.
- Put direct and structured edits in one transaction-based undo history, with one user action per undo unit and exact byte restoration.
- Treat Rowan as a validated prototype candidate for the lossless Rust CST, not yet the frozen parser choice.

The human accepted the prototype's editing behavior and hybrid identity model after the interactive walkthrough.

Evidence:

- [Interactive prototype](../assets/structure-preserving-edit-prototype/README.md)
- [Industry identity and editing research](../assets/structure-preserving-edit-identity-research.md)
- [ADR 0001: Structure-Preserving Source Edits](../../../docs/adr/0001-structure-preserving-source-edits.md)
