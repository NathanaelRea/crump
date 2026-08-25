# ADR 0017: Instrument-Owned Shared Post-Voice Graph

Status: Accepted  
Date: 2026-08-24

## Context

A polyphonic Instrument needs effects and filters that run once after its voices are summed. Lowering those processors into every voice multiplies state and work, while lowering them into the Song MixGraph loses Instrument ownership and makes a reusable patch incomplete. The boundary also needs explicit cross-domain semantics so a source edit cannot introduce a hidden sample of latency, a shared-to-voice dependency, or a callback allocation.

## Decision

Source version 2 admits one optional `graph shared N { ... }` beside a graph Instrument's required `graph voice N { ... }`.

- The voice graph runs once per active graph-voice slot. Its stereo outputs are accumulated in deterministic slot order.
- The shared graph owns exactly one external crossing: exactly one authored Bus whose source is `voice_sum.audio`. The source is stereo. `note.*` is unavailable in the shared domain, and `voice_sum.*` is unavailable in the voice domain.
- The shared graph has its own validated acyclic typed schedule, exactly one reachable Output, and the same bounded runnable Node schemas as the voice graph. It executes once per output frame for each Track that references the Instrument, including frames where the current voice sum is silent, so delay and reverb tails continue.
- Processing order is voice execution, deterministic voice sum, shared graph, then the Track's existing ordered native processor chain and channel routing. A shared graph is not lowered into the Song MixGraph.
- Shared Parameter consumers use that Track's singular projection: base, Automation, that Track's active Pattern lock, global and Instrument-scoped Instrument routes, and non-per-voice Song routes/followers, followed by the ordinary final constraint. Lock lanes are compiled by durable Track ID rather than folded into Song-global control state. A Song-global Parameter source that would read a Track-owned lock is rejected as ambiguous. Per-voice Instrument and Song contributions are deliberately omitted rather than sampled from an arbitrary voice. Graph-Port and note-relative sources remain per-voice and therefore cannot cross into the shared schedule.
- Global modulation state remains Song-owned. Instrument-scoped source state is evaluated with absolute Song time and durable Track identity. The shared graph's Node state, signal workspace, effective controls, and transition workspace are owned by that Track runtime.
- Shared graph topology and allocation-sensitive settings participate in the Track semantic hash. Declaration reordering is canonicalized. Compatible scalar publications retain shared Node state and use ADR 0011's preallocated 10 ms affected-target handoff; incompatible topology or allocation changes restart the affected Track's Instrument state behind the normal publication fade. Explicit transport seeks reset Track-owned shared Node and tail state with the durable Track seed while the replacement runtime is prepared off the audio callback.
- Admission includes each Track-owned shared program reference, shared Node work once per frame, Node heap state, signal and control arrays, packed transition bits, both resident publication runtimes, and their worst-case overlap work. Compilation and callback processing allocate no shared DSP storage.
- Runtime resource inspection reports shared instance count, work, and mutable bytes. The Workspace Graph view labels voice and shared Nodes/Buses separately and can descend from either domain to exact source.

Asset-backed source Audio remains outside this decision and continues to fail closed until its owning source-family milestone.

## Consequences

A reusable Instrument now owns a complete post-voice signal path without multiplying effects by polyphony or hiding them in Song routing. Shared tails and compatible state survive publication according to one policy. The single typed crossing and singular control projection keep cross-domain behavior deterministic and inspectable, at the cost of intentionally disallowing arbitrary voice-derived control signals in shared processing.
