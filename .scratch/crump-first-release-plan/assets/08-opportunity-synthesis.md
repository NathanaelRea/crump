# Crump Opportunity Synthesis

Date: 2026-07-26

## Reframed question

Which user advantage justifies Crump's language, and where should that language appear, if reliable agents can edit a DAW through validated semantic commands without directly rewriting user-visible code?

This synthesis draws on:

- [Professional DAW authoring workflow research](08-authoring-workflow-research.md)
- [Agent-assisted editing research](08-agent-editing-research.md)
- [DAW unmet-needs research](08-daw-unmet-needs-research.md)
- [Extensibility-model research](08-extensibility-models-research.md)
- [Focused music-tool research](08-focused-music-tools-research.md)

## What the evidence changes

1. **A language is not the agent interface.** Reliable agent edits need stable identities, typed queries, bounded semantic commands, revision preconditions, validation, preview, atomic commit, undo, permissions, and an audit trail. A model can emit those commands more reliably than unrestricted source patches. The commands can still compile to minimal source edits.
2. **Readable files are useful but not a complete product wedge.** They materially improve portability, recovery, inspection, deterministic serialization, and version history. They do not by themselves make editing safe, a project reproducible, or a workflow approachable.
3. **The strongest evidenced DAW gaps are durable rather than flashy.** Portability, accessibility, reproducible state, and reliability have the strongest support. Repetitive-work automation, customization, and meaningful history have moderate support. Broad demand for a generally "simpler DAW" is not established.
4. **Deep construction is approachable only behind an ordinary musical surface.** Max, Grid, JSFX, and Ardour all converge on a named device with normal parameters first and implementation second. Bitwig's preset, faceplate, and optional graph progression is the clearest model. A blank graph or language should not be the first sounding action.
5. **Focused tools win by compressing musical intent.** Parameter locks, event conditions, reusable patterns, and procedural automation replace many point edits with one meaningful rule. Hiding a general DAW's controls is less valuable than reducing how many concepts and operations the user needs.

## Product opportunities

### Inspectable variation partner

Turn a conventional clip into controlled development using a small vocabulary of recurrence, probability, conditional events, ratchets, parameter locks, and transformations. An agent proposes a bounded change; the TUI marks every affected event, previews cycles, exposes the seed, and permits rule-by-rule bypass. The durable rule is more useful than generated MIDI because the musician can understand and revise it later.

This is the smallest differentiated proof and already fits Crump's accepted pattern and deterministic-variation semantics.

### Understandable automation

Let the musician draw or record ordinary automation, then express repeated intent as visible recipes: phrase envelopes, pre-drop dips, followers, LFOs, or pattern-derived changes. Show base value, absolute automation, every modulation contribution, constraints, and effective value together. Agents translate requests into the constrained model rather than flattening intent into opaque points.

This is the strongest adjacent production workflow and directly addresses hidden state.

### User-owned devices

Start from normal presets with a small faceplate. Let an agent build or alter a typed graph from host-owned safe nodes, then show the graph diff, CPU and latency implications, exposed controls, and A/B audition. Save the accepted result as an ordinary reusable instrument or effect whose implementation opens only on demand.

This provides the custom-plugin benefit without requiring C++, arbitrary real-time code, or modular-synthesis fluency. It should follow, not precede, a usable preset workflow.

### Auditable arrangement assistant

Promote captured loops into a finite linear song through proposed clip, mute, variation, and automation edits. The agent rearranges the musician's authored material and explains repetition and dependencies; it does not autonomously generate an opaque finished song.

This could solve a valuable loop-to-song boundary, but it risks adding a second session-style product and is larger than the other proofs.

## Recommended product shape to test

Crump should test being **a conventional clip-and-timeline electronic composition environment whose agent creates inspectable variation, automation, and native devices as reviewable source-backed proposals**.

The proposed responsibility split is:

| Concern | Default musician surface | Optional depth | Authoritative mutation path |
| --- | --- | --- | --- |
| Notes, clips, arrangement | Conventional timeline, capture, inspectors | Pattern rule/source view | Validated semantic command to minimal source transaction |
| Mixing and routing | Mixer, channel inspector, route overview | Typed graph trace | Same command/transaction layer |
| Automation and modulation | Lane plus provenance layers | Recipe/source definition | Same command/transaction layer |
| Instruments and effects | Preset plus curated faceplate | Inspectable typed graph and source | Same command/transaction layer |
| Agent assistance | Proposed musical operation, visual diff, audition, approve | Exact semantic and text diffs, audit record | Revision-checked atomic command batch |
| Direct source editing | Not required for the first sounding action | Expert authoring surface | Candidate validation and publication pipeline |

The language remains important where the authored idea is genuinely language-like: reusable transformations, procedural variation, automation recipes, graph definitions, abstraction, and reviewable composition. Routine rename, move, transpose, route, or parameter edits do not need users or agents to manipulate syntax directly.

## Guards against REAPER-like complexity

- Expose a small semantic command vocabulary, not hundreds of raw setters.
- Keep one model and one undo history across TUI, agent, command recipes, and source.
- Make presets and ordinary clips playable before source, graph, routing, or automation representation choices appear.
- Reveal depth only for the selected object or task; no durable state may become undiscoverable when hidden.
- Package custom devices with source, dependencies, typed parameters, version identity, and host-generated controls.
- Separate safe DSP graphs, non-real-time project commands, and explicitly trusted advanced code into different capability tiers.
- Require every agent mutation to show targets, before/after semantics, exact source impact, validation, audition status, and one-step undo before commit.

## Next prototype question

The layout comparison was premature. The next reaction artifact should compare one realistic task across three product contracts:

1. UI-first editing with an agent proposal and source available only as audit detail.
2. UI-first editing with an always-available procedural rule editor for variation and automation.
3. Source-led editing with synchronized visual explanations and structured controls.

Use the same task in each: turn a plain eight-bar drum-and-bass loop into a controlled evolving phrase, then adjust one generated automation rule and one custom-device parameter. Measure confidence, correction cost, and whether the musician can explain the accepted result after the chat is gone. Do not choose the persistent workspace layout until this establishes how much direct language exposure creates value.
