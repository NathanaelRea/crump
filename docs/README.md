# Crump Documentation

## User and Operator Guides

- [Production Workspace](workspace.md)
- [Project Language 1](language.md)
- [CLI and External Authoring Agents](cli-and-agents.md)
- [Native Audio, Revision Transitions, and MIDI](audio-and-midi.md)
- [Project History and Deterministic Rendering](history-and-rendering.md)
- [First-Release Support Contract](support.md)
- [First Public Release Evidence](release-evidence.md)

The implementation is usable as a preview, but the named-hardware endurance, packaging, accessibility, and independent musician gates remain open. The evidence page is the source of truth for release readiness.

## Authority

- `requirements/product-contract.md` defines the product promise, first-release scope, and explicit non-goals.
- `docs/product/first-public-release.md` turns that scope into release gates and evidence categories without changing the promise.
- `CONTEXT.md` defines canonical domain language only.
- `docs/adr/` records accepted, consequential architecture decisions once they are resolved.
- `docs/research/` records dated external evidence. Research informs requirements and decisions but is not itself normative.
- `.scratch/crump-first-release-plan/map.md` is the workspace-local index of unresolved planning decisions only when it matches the canonical product and release documents.
- Child issues under `.scratch/crump-first-release-plan/issues/` own the detail and resolution of one planning question each.

An issue may propose a requirement or architecture change, but the relevant canonical document must be updated when that proposal is accepted. Issue text alone does not silently redefine the product.

## Evidence Policy

For workflows with an established DAW analogue, research current official Ableton documentation first, then compare other professional DAWs and relevant primary standards. Record observed behavior separately from the Crump implication. Competitor behavior is a baseline and source of tested practices, not an automatic requirement or feature-parity target.

Prefer the simplest Crump design that completes the current Validated Slice and preserves its correctness, real-time, recovery, inspection, and render guarantees. Destination capabilities enter only when human evaluation identifies them as the next obstacle to a finished track.

## Status Vocabulary

- **Draft:** incomplete and open to material change.
- **Proposed:** complete enough for an explicit decision.
- **Accepted:** authoritative until superseded.
- **Deferred:** intentionally outside the current release or planning frontier.
- **Superseded:** retained for history but no longer authoritative.

## Normative Language

- **Must:** required to satisfy the stated contract.
- **Should:** expected unless evidence justifies an exception.
- **May:** permitted but not required.
