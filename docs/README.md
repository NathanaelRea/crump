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
- [Native Synthesis Qualification Runbook](qualification/native-synthesis.md)
- [Architecture Decision Records](adr/README.md)
- [Research](research/)
- [Example Instruments](../examples/instruments/README.md) — 184 portable house, orchestral, synth, world and band instruments and audition Projects
- [Native Acoustic Models](native-acoustics.md) — excitation, profiles and graph controls
- [Waveguide Instruments](../examples/waveguides/README.md) — a plucked string, a sitar and a clarinet built from feedback loops in plain `.crump`
- [Audio Comparison](../tools/audio_compare/README.md) — reusable listening and time/frequency reports

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
