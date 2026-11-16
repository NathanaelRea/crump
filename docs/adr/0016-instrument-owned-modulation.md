# ADR 0016: Instrument-Owned Modulation Foundation

Status: Accepted
Date: 2026-08-24

## Context

Song-owned LFO routes are arrangement controls and do not travel with an Instrument Module. Native synthesis also needs portable envelopes, LFOs, note-relative controls, deterministic random holds, and macros without introducing hidden Control delay or a second Parameter-precedence model. Graph Control outputs are produced during per-voice execution, so treating them as a prior-frame cache would violate the same-frame requirement.

## Decision

- Stable source version 2 permits an Instrument-local `modulation` block containing durable source and route IDs. Version 1 remains byte- and behavior-compatible and rejects this form.
- Sources are graph Envelope/LFO Control Ports, free or synchronized sine/triangle LFOs, note velocity/key/gate/age, deterministic random hold, and normalized `macro` Parameters. State scope is explicit as global, Instrument, or per voice.
- A route owns source, local Parameter target, physical depth, mapping polarity, linear/power/smoothstep curve, signed order, durable bypass, optional multiplicative auxiliary source, and bounded route-local operators.
- Effective evaluation is base → Automation → lock → Instrument routes → the separate Song route/follower tier → final domain constraint. Routes targeting one Parameter use signed order then durable route ID. Acyclic macro dependencies are evaluated on the same frame, and validation rejects a broader-scope consumer that depends on a narrower-scope macro producer.
- Global Instrument sources/routes execute once in the Song control runtime. Instrument-scope and per-voice sources/routes execute in each Track's graph runtime: Instrument scope uses the absolute Song frame plus a durable Track identity, while note-relative per-voice time continues from note-on through release. Pattern-lock lanes are also projected by durable Track identity. Because a global Parameter source has no Track from which to select a lock, validation rejects that source when its consumed macro can be locked. Two Tracks referencing one Instrument therefore share global state but not Instrument or lock state.
- Per-voice graph-Control routes lower with Audio/Control graph Nodes into one combined dependency schedule. Primary and auxiliary source Nodes execute before the route, and the route executes before every Node consuming its target Parameter. Validation and defensive compilation reject the combined cycle. No prior-frame cache, hidden delay, or no-op fallback is permitted.
- Runtime programs, per-voice arrays, route provenance, steady-state work, and worst-case old/new publication-transition work are fixed and admitted before publication. Random values derive from stable semantic identities and exact hold index, not callback grouping or mutable ambient RNG; the mixing step keeps Track, voice, and note-event identities distinct.
- Exact route depth and bypass sites support checked edits. Macro values use the existing checked Parameter operation. Depth, bypass, and macro value changes are compatible scalar publications: changed route targets and their macro-dependency closure ramp for the 10 ms ADR 0011 handoff while active graph voices survive. Source, target, or dependency-topology changes participate in the affected-Instrument restart policy from ADR 0011.
- Shared post-voice execution and its singular control projection are specified separately by [ADR 0017](0017-shared-post-voice-graph.md). Asset source Audio and richer alpha-fixture LFO modes remain outside this decision and fail closed.

## Consequences

Instrument Modules now carry reusable modulation behavior while Song routes remain arrangement-owned. Envelope and graph-LFO routes have same-frame semantics within the validated per-voice DAG. Inspection can distinguish ownership tiers and singular versus per-voice contributions. Route and source ceilings, operator bounds, authored Nodes plus every lowered synthetic route/source/Song-tier operation, immutable program bytes, all preallocated per-voice control and transition arrays, both graph voice slots, both resident publication runtimes, and their combined transition-frame callback work enter admission.

Source-v2 route syntax is intentionally narrower than the non-normative `2-alpha` fixtures. Unsupported retrigger, one-shot, direction, and asset-source forms remain errors rather than being silently ignored. Shared graphs omit per-voice routes by contract rather than sampling or silently substituting their values.

## Rejected alternatives

- Reusing Song routes as patch state would lose Instrument portability and ownership clarity.
- Reading graph Control outputs on the following frame would add hidden latency and change envelope/LFO behavior.
- Evaluating graph routes before their source exists would turn accepted source into a no-op.
- Allowing macro or graph-Control feedback would make same-frame values schedule-dependent.
- Allocating route maps or provenance on the callback would violate the realtime contract.
