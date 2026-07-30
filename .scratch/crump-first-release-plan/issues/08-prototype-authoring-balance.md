# Prototype the Code and TUI Authoring Balance

Type: prototype
Mode: HITL
Status: resolved
Blocked by: [01 - Freeze the First-Release Reference-Project Acceptance Contract](01-freeze-reference-project-contract.md), [03 - Prove Structure-Preserving Source-Backed Edits](03-prove-structure-preserving-edits.md), [06 - Define the Composition and Timeline Semantic Model](06-define-composition-model.md)

## Question

Which concrete division of responsibility between source editing, structured inspectors, timeline views, mixer controls, and automation lanes completes the reference workflow efficiently without creating surprising generated source?

## Research Baseline

Before prototyping, research current official Ableton documentation for Arrangement View, keyboard navigation, automation, mixing, and routing, then compare equivalent workflows in other professional DAWs. Record sources and the minimum proven workflow primitives. Use them as the baseline, then prototype the simplest Crump interaction model that completes the reference workflow without copying DAW chrome or adding unsupported modes.

## Comments

- The first reaction artifact compared source-spine, task-workspace, and object-lens models. The human did not select one: seeing them called the code-first premise into question. The investigation must step back and establish which unmet user need justifies Crump's language, considering agent-assisted edits, easier automation, and user-authored instruments or effects without inheriting REAPER-like complexity. Research those opportunities and alternatives before revising the prototype.
- Assets: [professional DAW authoring workflow research](../assets/08-authoring-workflow-research.md) and [first reaction artifact](../assets/authoring-balance-prototype/README.md).

## Answer

The reaction artifacts successfully exposed that the former code-first, complete-DAW premise was the wrong first-release question. The canonical product direction is now resolved: recognizable Production Views lead human authorship; the DSL remains durable and always available; an external model-neutral Authoring Agent edits normal files and uses machine-readable CLI publication; a Beginner Musician progresses through Event Grid, Instrument Faceplate, Graph View, and source depth; and the first proof is a no-sample Musical Sketch with an Agent-authored graph instrument rather than variation/Automation inside a finished DAW project.

The existing HTML prototypes and opportunity research are historical reaction evidence. Issue 09 owns a new narrow prototype and dual-role evaluation; mixer, routing, Automation, waveform, and proposal/approval workflows shown by the old artifacts are not First Public Release requirements.
