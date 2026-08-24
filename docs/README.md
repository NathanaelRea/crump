# Crump Documentation

## User and operator guides

- [Production Workspace](workspace.md)
- [Project Language 1](language.md)
- [CLI and External Authoring Agents](cli-and-agents.md)
- [Production Cookbook](cookbook/README.md)
- [Native Audio, Revision Transitions, and MIDI](audio-and-midi.md)
- [Project History and Deterministic Rendering](history-and-rendering.md)
- [Expressive Instruments](expressive-instruments.md)
- [Supported Environment and Troubleshooting](support.md)
- [Verification](verification.md)

## Technical references

- [Architecture Overview](architecture/overview.md)
- [Native Synthesis Compatibility](native-synthesis-contract.md)
- [Architecture Decision Records](adr/README.md)
- [Research](research/)

## Authority

- [`requirements/product-contract.md`](../requirements/product-contract.md) defines current product behavior and explicit limitations.
- [`CONTEXT.md`](../CONTEXT.md) defines canonical domain language.
- [`docs/adr/`](adr/) records accepted architectural decisions.
- [`docs/research/`](research/) records external evidence. Research is informative and does not redefine Crump behavior.
- User guides describe operation within the product contract and accepted ADRs.

When a guide conflicts with the product contract or an accepted ADR, the contract or ADR is authoritative. Update every affected guide when current behavior changes.

## Evidence policy

Competitor documentation and primary standards can explain established practices, but competitor behavior is not an automatic Crump requirement or feature-parity target. Keep observed external behavior separate from Crump behavior.

## Normative language

- **Must:** required by the current contract.
- **Should:** expected unless a documented constraint justifies an exception.
- **May:** permitted but not required.
