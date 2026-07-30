# Agent-Assisted DAW Editing: Interfaces, Reliability, and the Case for a DSL

Accessed: 2026-07-26

## Executive conclusion

**Evidence:** None of the reviewed mainstream DAWs documents a first-party LLM, agent, or MCP interface that can inspect and safely edit an entire native project. Current first-party “AI” features are generally bounded generators or signal-processing tools, not language-model project editors. Logic Pro's AI-branded features generate backing performances, split stems, or model saturation; Audacity's documented AI plug-ins generate/remix audio, separate stems, suppress noise, and transcribe audio. Neither announcement describes an agent that reads and mutates arbitrary project structure. [AI1][AI2]

The enabling substrate does exist, unevenly:

- REAPER, Renoise, Ableton Live, and Ardour expose useful structured state and mutation APIs. REAPER and Renoise expose especially broad editing surfaces and explicit undo facilities. [R1][R2][N1][A1][A2][D1]
- REAPER actions provide a command vocabulary, while Renoise and Ardour provide object methods; Ableton's Live Object Model combines properties, observers, and functions. [R1][R2][N1][A1][D1]
- DAWproject supplies an open, schema-defined structured interchange model for arrangements, tracks, clips, notes, automation, routing, and device state, but explicitly is not a native DAW format and defines no editing transactions. [F1][F2]
- Ardour and LMMS persist substantial native state as XML, and REAPER exposes native “state chunks,” but readable serialization is not the same as a supported, transaction-safe source-authoring contract. [D2][D3][L1][L2][R2]
- Real third-party MCP bridges prove that agents can already operate Ableton, REAPER, Bitwig, and FL Studio. They also expose the gaps: broad unauthenticated local bridges, index/name-based addressing, incomplete readback, no dry-run diff, weak or absent transaction boundaries, and “save first” safety advice. [M1-M4]

**Analysis:** A bespoke user-facing DSL is **not necessary merely to make reliable agent edits**. The more direct requirement is a canonical typed project model with stable identities, validated reversible commands, revision preconditions, and inspectable before/after results. A textual DSL can still be justified for Crump's separate product goals: durable human authorship, procedural composition, reuse, source control, and direct editing. It should not be justified as an LLM reliability requirement, and agents should normally target the same semantic command layer as the TUI rather than freely rewriting syntax.

## Scope and method

This report evaluates interfaces available to an agent, not whether a product applies machine learning somewhere in its audio pipeline. Sources are first-party manuals, specifications, and source repositories, plus the repositories of actual third-party integrations as primary evidence about those integrations. Product marketing was used only to establish what first-party AI features claim to do.

The comparison asks:

1. Is project state available as supported text, a structured object model, or both?
2. Can a tool invoke bounded domain actions rather than imitate mouse and keyboard input?
3. Can multiple edits become one named, reversible transaction?
4. Can the user preview, approve, audit, or reject the proposed result?
5. Does the interface protect against stale targets, invalid state, destructive effects, and excess authority?

“Not documented” means the reviewed primary sources do not establish the capability; it is not proof that no private or unsupported mechanism exists.

## What current “AI in a DAW” does and does not establish

### Documented first-party AI

| Product | Documented capability | Agent editing? | Evidence |
| --- | --- | --- | --- |
| Logic Pro | Session Players respond to user controls and chord progressions; Stem Splitter separates mixed audio; ChromaGlow models hardware coloration. | **No documented general agent.** These are bounded generators/processors with purpose-built controls, not an LLM with project read/write tools. | Apple announcement [AI1] |
| Audacity OpenVINO plug-ins | Local noise suppression, transcription, prompt-based music generation/style remix, and source separation. | **No documented project agent.** Results are audio or labels produced by effects. | Audacity announcement [AI2] |

**Analysis:** These systems use the safest common pattern in current products: constrain probabilistic behavior to one typed operation and return ordinary editable project material. That is relevant to agent safety, but it does not solve arbitrary arrangement, routing, automation, or mix editing.

### Documented third-party LLM/MCP integrations

| Integration | DAW bridge | Demonstrated edit surface | Reliability and safety visible in its own documentation |
| --- | --- | --- | --- |
| AbletonMCP | Python MCP server to a Live MIDI Remote Script over a JSON/TCP socket. | Two-way session readback; tracks; clips and MIDI notes; devices/effects; arrangement building; transport. | Calls itself third-party, recommends saving before experimentation, says complex arrangements may need smaller steps, and documents optional telemetry. No dry run, approval protocol, revision precondition, diff, or atomic multi-command transaction is documented. [M1] |
| Total REAPER MCP | Python MCP server to a Lua ReaScript bridge through JSON files. | Profiles range from 15 higher-level tools to 600+ ReaScript-facing tools; tracks, MIDI, effects, automation, routing, render, transport, and project operations. | Profiles reduce tool overload and authority, but the “full” profile remains enormous. The README warns some integration tests may fail and describes file IPC, not authorization, per-edit approval, preview, stale-write detection, or an audit log. [M2] |
| Bitwig MCP Server | MCP to DrivenByMoss's OSC controller surface. | Project/track/device readback plus transport, tempo, volume, pan, mute, and device parameters. | Explicitly work in progress; documentation and implementation may disagree. Its surface is too narrow for arrangement editing and documents no transaction/preview layer. [M3] |
| FL Studio MCP | MCP to a custom FL Studio MIDI controller script over virtual MIDI. | Sends melodies, chords, and drums for live recording. | The author describes encoding values through 7-bit MIDI as “MIDI nonsense” and asks Image-Line for more API access. No general project readback, structured edits, undo grouping, or safety model is documented. [M4] |

These are real implementations, not vendor-supported product capabilities. AbletonMCP has the broadest visible “make a song from a prompt” claim; Total REAPER MCP has the broadest API-derived tool surface. Both depend on the host's pre-existing scripting/control mechanisms. [M1][M2]

**Analysis:** The integrations show that MCP is an adapter, not the editing model. When the host has a broad structured API (REAPER), an MCP can expose broad editing. When the available bridge is OSC or MIDI (Bitwig and FL Studio examples), the agent is correspondingly shallow or lossy. Adding MCP to an inadequate project API does not repair identity, atomicity, validation, undo, or readback.

## Comparative capability matrix

| System | Text/native project representation | Structured project API | Actions/commands | Undo and audit | Preview and safety | Agent readiness |
| --- | --- | --- | --- | --- | --- | --- |
| REAPER 7.78 | Native `.RPP` state is externally inspectable in practice; the supported API exposes track/item/envelope “state chunks” and project extension state, but the reviewed docs do not publish a complete schema or promise arbitrary external rewrite compatibility. | Very broad ReaScript/extension API: tracks, items, takes, MIDI, automation, FX, routing, markers, tempo, render, save/load, and state change count. | Any built-in action can be called by command ID; scripts themselves enter the Action List. | Scripts receive undo points by default; `Undo_BeginBlock`/`Undo_EndBlock` create a named block; can-undo/redo and explicit undo/redo calls exist. | API errors and host constraints exist, but scripts can crash REAPER; official guidance says save important projects. No dry-run command plan or optimistic mutation precondition is documented. | **Highest mainstream readiness**, but needs a safe semantic facade over a large, irregular API. [R1][R2] |
| Renoise | Songs are XRNS files, but the supported editing surface in the reviewed source is the in-process Lua object model rather than external text editing. | Rich `renoise.Song` hierarchy: transport, sequence, patterns, tracks, instruments, devices, parameters, selections, observables, render, and constrained insert/delete/swap methods. | Tools add menu entries, scoped keybindings, MIDI mappings, timers, and import hooks. | Automatic undo descriptions; custom `describe_undo`; asynchronous changes can be merged with `describe_batch_undo`; can-undo/redo and undo/redo are exposed. | Mutation methods enforce domain constraints, such as preserving at least one regular track/instrument and remapping references when instruments move. Rendering can silently overwrite a target file. No dry-run diff or revision compare-and-swap is documented. | **Best evidence for a typed model plus reversible commands without a user DSL.** [N1][N2] |
| Ableton Live 12.3.5 | Live Set is represented through the Live API; no supported human-editable native set format is documented in the reviewed API. | Live Object Model for songs, tracks, clips, notes, devices, mixer state, parameters, scenes, and views; supports get/set/observe/call. Note APIs return per-note IDs. | Domain functions create/delete/duplicate tracks and scenes, edit notes and warp markers, move devices, and control transport. | `Song` exposes can-undo/can-redo and undo/redo. `live.object` is documented to mimic user interactions and add to undo history. No public multi-call begin/end transaction or custom undo label is documented. | Property access modes and domain methods constrain calls; deferred warp-setting means call order can differ from intuition. No whole-project export, dry-run plan, or stale-write guard is documented. | **Capable but incomplete and Max-for-Live-centric**; multi-step agent edits need an added transaction layer. [A1-A3] |
| Ardour | Main `.ardour` snapshots are XML; the session also contains a `.history` undo file and backup snapshot. Source writes an XML tree through a temporary file and rename, preserving an old backup. | Most C++ session objects are exposed to Lua; `Session` contains routes, sources, regions, playlists, locations, tempo map, ports, transport, and undo/redo history. | Editor Action scripts, hooks, session scripts, DSP scripts, a console, and a headless `luasession` tool exist. | Native source implements memento/diff commands and can reconstitute commands by persistent object ID; history is saved with the session. | Context-specific interpreters restrict real-time versus GUI operations. The manual says bindings are incomplete/subject to change and that much complex functionality lives in the Editor UI, limiting headless control. No agent approval/diff layer exists. | **Architecturally strong but poorly surfaced/stabilized for external agents.** [D1-D4] |
| Bitwig Studio / DAWproject | Bitwig co-authors DAWproject: ZIP containing UTF-8 `project.xml` and `metadata.xml`; stable 1.0 schema. It is explicitly an exchange format, not a native format. | Controller Extensions provide a host-mediated controller model, but the public repository directs developers to documentation installed inside Bitwig. The reviewed third-party MCP relies on OSC and exposes only a small subset. | Controller operations and OSC, not a documented full project command API in the reviewed public material. | No DAWproject transaction or undo semantics; no relevant undo contract is established by the public extension repository. | XSD validation catches structural/type errors. Importers may flatten unsupported structure; plug-in state is opaque; no command preconditions, dry run, or effect audit. | **Excellent interchange/context candidate, insufficient mutation protocol.** [F1-F3][M3] |
| LMMS | Uncompressed `.mmp` is XML; source models project data with `QDomDocument`, validates at a basic extension/type level, and upgrades old versions. Compressed `.mmpz` is not directly diffable. | Internal C++ model, but no reviewed stable end-user project scripting API. | No reviewed external action API. | GUI undo exists, but no external transaction contract was found in the reviewed primary material. | Parsing/upgrades help compatibility; direct file writes bypass an open session's history and can depend on internal numeric IDs and plug-in-specific XML. | **Text-accessible but not agent-safe by virtue of XML.** [L1][L2] |
| Sonic Pi | User code is the durable composition surface and can be saved as text. | Language/runtime functions address synths, samples, effects, timing, control, MIDI, and OSC. | `Run` reevaluates code; `Stop` halts running code. | Ordinary editor operations apply, but the reviewed tutorial does not define DAW-style semantic transactions or persistent project undo. | Immediate audition and logs are strong; a bad edit can be stopped and code restored. It is a live-coding composition environment, not a complete object model for conventional recorded arrangements and mixes. | **Highly LLM-editable text, but not evidence that a full DAW must use a DSL.** [C1] |

## Formats: readable state is not an editing contract

### DAWproject: the strongest open canonical model reviewed

**Documented facts:** DAWproject 1.0 is a stable, language-agnostic ZIP/XML exchange format. Its goals include audio, notes, note expression, automation, embedded/referenced audio, and embedded plug-in states. The XSD defines typed tracks and channels, ID/IDREF links, beat/second timelines, clips, notes, automation targets and points, devices, sends, and parameter units. It lists support in Bitwig Studio, Studio One, Cubase, Cubasis, VST Live, and n-Track Studio. Its non-goals explicitly include being a DAW's native format or storing non-session UI/preferences. Exporters may retain structures that importers then flatten to fit their own model. [F1][F2]

**Analysis:** This proves that a useful agent context can be conventional structured data rather than a music-specific programming language. It does **not** prove safe in-place editing. The schema has no command semantics, revision token, invariant transaction, inverse operation, user intent, or preview artifact. XML validity also cannot prove that a referenced plug-in is present, an imported result sounds equivalent, routing is supported by the destination, or opaque plug-in state is safe.

### Native XML and text-like formats

**Documented facts:**

- Ardour's main snapshot, backups, and undo history are separate session artifacts. The implementation parses a `Session` XML root, checks file-version compatibility, writes through a temporary file/rename, and backs up the previous snapshot. [D2][D3]
- LMMS's `DataFile` is a `QDomDocument`; its source distinguishes songs, templates, instrument settings, clipboard data, journals, effects, and MIDI clips and contains explicit version-upgrade routines. A repository test fixture demonstrates detailed song, track, instrument, automation, mixer, and UI XML. [L1][L2]
- REAPER's API exposes getters/setters for track, item, envelope, and project state and offers persistent project extension state. Cockos recommends APIs and actions for edits and explicitly integrates undo around scripts. [R1][R2]

**Analysis:** All three are easier to inspect, diff, back up, and migrate than opaque binary documents. None of the cited sources promises that arbitrary text surgery is equivalent to a live, validated domain operation. External edits can use stale locations, break cross-references, bypass the DAW's undo history, race an open application, or accidentally replace opaque device state. Text is valuable for review and recovery; supported commands are safer for mutation.

## Structured APIs and commands

### Why the Renoise and REAPER models matter

Renoise documents a coherent object graph and constrained methods. For example, deleting tracks cannot remove the last regular track, moving instruments remaps existing notes and links, and edits automatically enter undo with overridable descriptions. A batch API groups asynchronous modifications. [N1] This is almost exactly the semantic substrate an agent needs, despite XRNS not being presented as a user-authored DSL.

REAPER exposes a less uniform but much broader surface. ReaScript can call most extension API functions and any built-in action, scripts become actions, and one named undo block can contain direct API edits plus action invocations. The API exposes `GetProjectStateChangeCount`, GUID accessors, validation functions, and render analysis as ingredients for safer wrappers, though mutation calls themselves do not accept an expected revision. [R1][R2]

**Analysis:** These systems suggest a two-level design:

1. A small semantic command API for frequent, well-understood operations.
2. A lower-level typed object/query API for discovery and uncommon operations.

An agent should not receive hundreds of unrelated setters by default. Total REAPER MCP independently arrived at profiles and a 15-tool higher-level layer because a 600+ tool surface can overwhelm models. Its so-called DSL is an API facade accepting convenient references such as `"bass"`, `"track 3"`, `"-6dB"`, and `"8 bars"`; it is not the native project representation. [M2]

### Limits of controller APIs

The Bitwig MCP example can read project/track/device summaries and set transport or mixer controls through OSC, but cannot document arrangement construction. The FL Studio example can encode note data through virtual MIDI but says it lacks sufficient DAW API access. [M3][M4]

**Analysis:** Control-surface APIs are designed for immediate gestures over a bounded bank of controls, not complete project serialization or referential editing. They often expose indices, currently selected objects, normalized values, or limited feedback. An agent bridge over that layer inherits those limitations and cannot reliably answer “what exactly changed everywhere?”

## Undo, preview, and auditability

### What is documented

- **REAPER:** automatic script undo, explicit named begin/end blocks, can-undo/redo, and direct undo/redo. Deferred scripts must create undo points explicitly. Cockos warns that an unclosed block causes undesired undo behavior. [R1][R2]
- **Renoise:** automatic descriptions, custom descriptions, and time-bounded batch undo for asynchronous operations. [N1]
- **Ableton Live:** API operations through `live.object` add to undo history; the Song object can report and execute undo/redo. [A2][A3]
- **Ardour:** session artifacts include persistent history; source represents undo as reconstitutable memento and state-diff commands keyed to object IDs. [D2][D4]
- **Sonic Pi:** rerun gives fast audition and the log exposes executed sound events, but this is runtime feedback rather than a semantic project-change ledger. [C1]

### What is importantly missing

No reviewed DAW API or MCP integration documents all of the following as one operation:

1. Query project revision `r`.
2. Submit a typed command plan conditioned on `r`.
3. Validate all commands without changing active state.
4. Return a human-readable semantic diff and affected source ranges.
5. Audition a candidate without replacing the accepted revision.
6. Commit the whole plan atomically as one named undo step.
7. Return durable command/result IDs and exact inverses for audit.

REAPER and Renoise cover the transaction/undo portion best. DAWproject and text formats cover inspectability best. Sonic Pi covers immediate code audition. None joins them into an agent-safe revision protocol.

**Analysis:** Undo after mutation is necessary but weaker than preview before mutation. It does not prevent a destructive render from overwriting a file, a command from loading untrusted plug-in state, a route change from producing dangerous output, or fifty individually undoable calls from leaving a half-applied plan after call 23 fails. Agent safety requires validation and atomicity before relying on undo.

## MCP does not supply DAW safety semantics

**Documented facts:** MCP tools have JSON Schema inputs and optional structured outputs. The specification says servers must validate inputs, apply access controls, rate-limit calls, and sanitize outputs; clients should show tool inputs, ask for confirmation on sensitive operations, use timeouts, validate results, and log tool usage. It says a human should be able to deny tool invocations. Tool annotations are explicitly untrusted unless the server is trusted. MCP elicitation supports accept/decline/cancel user responses but does not prescribe a UI and forbids requesting sensitive information through that mechanism. Local-server guidance recommends explicit installation consent, sandboxing, least privilege, and `stdio` or protected IPC. [P1-P3]

**Analysis:** MCP can transport a good DAW command API and can help a client present confirmations. It does not define:

- musical/project invariants;
- stable track, clip, device, or parameter identity;
- command atomicity or inverse commands;
- stale-revision rejection;
- candidate audio audition;
- source-preserving edits;
- the boundary between transient audition state and authored state;
- permission classes such as “mix only,” “no file writes,” or “never arm/record”;
- a required per-call confirmation or audit implementation.

The reviewed DAW MCP repositories do not document complete implementations of those controls. “Localhost” is not an authorization model, and a socket/file/MIDI bridge running inside a DAW can exercise the DAW user's authority.

## Is a bespoke user-facing DSL necessary?

### Evidence against necessity

1. Renoise exposes broad, constrained, undoable project editing through ordinary Lua objects and methods, including coherent reference-remapping behavior, without making its native project a user-facing DSL. [N1][N2]
2. REAPER exposes broad edits through functions and actions; the MCP project's higher-level “DSL” is a convenience/tool-selection layer over those APIs, not required storage. [R1][R2][M2]
3. Ableton's notes are returned as typed dictionaries with unique note IDs and modified by dedicated functions. Typed structured arguments are sufficient for that edit class. [A2]
4. DAWproject represents a substantial common music-production model with XML, IDs, references, and schemas rather than executable bespoke syntax. [F1][F2]
5. Tracktion Engine explicitly describes itself as a high-level data model for sequencers and DAWs; its edit loader/saver operates on a structured `ValueTree` and migrates legacy XML. This is further architectural evidence that a canonical model can underlie rich audio editing without a user language. [T1][T2]

### Evidence for a DSL's separate value

Sonic Pi demonstrates properties that object serializers do not automatically provide: concise music-oriented expressions, functions, iteration, concurrency, live replacement, comments, and a portable text composition when built-in sounds are used. [C1] Crump additionally requires procedural patterns, reusable DSP graphs, deterministic transformations, and authoritative source. Those product goals may justify a language independently of agents.

### Analysis

LLMs are good at producing text, but that does not make unrestricted source rewriting the most reliable control channel. A model can just as readily emit schema-constrained command objects. Reliability comes from reducing what an accepted operation can mean:

```json
{
  "command": "move_clip",
  "project_revision": 184,
  "target": { "clip_id": "clip-7f2" },
  "to": { "quarter_notes": "32/1" },
  "preconditions": {
    "current_start": { "quarter_notes": "16/1" },
    "track_id": "track-bass"
  }
}
```

That object is easier to validate, authorize, explain, reverse, and reject as stale than an unconstrained patch to a source file. It can still compile to a minimal source edit so text remains authoritative.

A DSL becomes necessary only if the desired authored semantics are themselves language-like: procedural generation, abstraction, composition, user-defined functions/graphs, or code review as the primary workflow. It is not necessary for commands such as rename, move, transpose, set parameter, add route, or insert automation points.

## Implications for Crump's first release

### Recommended architecture

1. **Keep one canonical semantic model.** Parse authoritative source into typed entities with durable IDs and explicit references. The TUI, agent, renderer, and diagnostics must resolve the same model.
2. **Make reversible semantic commands the agent boundary.** Commands should target IDs rather than list indices or mutable names, carry an expected project revision and relevant old values, validate as a batch, and either commit entirely or not at all.
3. **Compile commands to minimal source patches.** The source remains durable and reviewable; the agent does not need to understand trivia-preserving rewrite mechanics. Return both semantic diff and exact text diff.
4. **Separate propose, audition, and commit.** A proposal creates a Candidate Revision. Validation and an optional bounded render/audition occur before publication. Commit publishes an Active Revision and one named undo transaction.
5. **Use capability-scoped tools.** Start with read-only inspection, transport/audition, and a small editing set. Gate file overwrite, render destinations, asset deletion/import, plug-in loading, record arm, monitoring, and commands that can alter external MIDI/audio output separately.
6. **Keep an append-only operation record.** Record user request, model/server identity, tool and arguments, base revision, validation diagnostics, semantic/text diff hashes, approval, commit result, and undo/redo linkage. Do not put secrets or raw private prompts into portable project source by default.
7. **Expose MCP only as a thin adapter.** MCP schemas should mirror domain commands and return structured results. Do not expose parser internals, arbitrary source replacement, arbitrary shell/file access, or hundreds of raw setters merely because MCP can carry them.
8. **Retain direct text editing as a distinct expert surface.** Direct edits use the same candidate/validation/publish pipeline and source undo history. Agent reliability should not depend on asking the model to edit text directly.

### Minimum safe command result

Every mutating command or batch should return:

- accepted/rejected/stale status;
- base and candidate revision IDs;
- resolved durable targets;
- normalized typed arguments and units;
- semantic before/after summary;
- exact source patch or diff;
- diagnostics and invariant checks;
- whether audio graph replacement is safe now or deferred;
- undo transaction ID and description after commit;
- any external side effects that are not undoable.

### Important capabilities absent from the reviewed market

These are opportunities, not established DAW conventions:

- first-party, vendor-supported MCP or equivalent agent endpoint;
- whole-project typed query plus complete supported mutation API;
- stable cross-reload IDs exposed consistently to integrations;
- optimistic concurrency on every mutation;
- transactional validation and dry-run semantic diffs;
- candidate-revision audio preview/A-B before commit;
- source-preserving structured edits with exact text diffs;
- persistent, user-readable agent audit history;
- capability permissions tailored to recording, monitoring, rendering, files, plug-ins, and external MIDI;
- explicit accounting for irreversible side effects;
- deterministic project-state test fixtures and post-edit render assertions;
- a standard way to distinguish authored mix state from transient solo/cue/audition state.

## Bottom line

The market has three incomplete pieces: readable structured formats, powerful in-process scripting APIs, and nascent third-party MCP bridges. It does not yet have a documented end-to-end safe agent editing contract. The evidence favors **canonical structured state plus reversible semantic commands** as that contract. Crump's DSL may still be central to the product, but it should sit behind and alongside this command model, not substitute for transaction, identity, validation, preview, permission, and audit design.

## Primary sources

All sources accessed 2026-07-26.

### First-party AI claims

- **[AI1] Apple, “Logic Pro takes music-making to the next level with new AI features,” May 7, 2024.** https://www.apple.com/newsroom/2024/05/logic-pro-takes-music-making-to-the-next-level-with-new-ai-features/
- **[AI2] Audacity, “Introducing OpenVINO AI effects for Audacity,” January 2, 2024.** https://www.audacityteam.org/blog/openvino-ai-effects/

### REAPER

- **[R1] Cockos, “ReaScript.”** https://www.reaper.fm/sdk/reascript/reascript.php
- **[R2] Cockos, REAPER v7.78 ReaScript API reference.** https://www.reaper.fm/sdk/reascript/reascripthelp.html

### Renoise

- **[N1] Renoise, `renoise.Song` scripting API.** https://renoise.github.io/xrnx/API/renoise/renoise.Song.html
- **[N2] Renoise, `renoise.ScriptingTool` API and official scripting repository.** https://renoise.github.io/xrnx/API/renoise/renoise.ScriptingTool.html and https://github.com/renoise/xrnx

### Ableton Live / Max for Live

- **[A1] Cycling '74, Live Object Model, for Ableton Live 12.3.5.** https://docs.cycling74.com/apiref/lom/
- **[A2] Cycling '74, Live Object Model: `Song` and `Clip`.** https://docs.cycling74.com/apiref/lom/song/ and https://docs.cycling74.com/apiref/lom/clip/
- **[A3] Cycling '74, “Creating Devices that use the Live API.”** https://docs.cycling74.com/userguide/m4l/live_api/

### Ardour

- **[D1] Ardour Manual, “Lua Scripting” and Lua class reference.** https://manual.ardour.org/lua-scripting/ and https://manual.ardour.org/lua-scripting/class_reference/
- **[D2] Ardour Manual, “What's in a Session?”** https://manual.ardour.org/working-with-sessions/whats-in-a-session/
- **[D3] Ardour source, `libs/ardour/session_state.cc`.** https://github.com/Ardour/ardour/blob/master/libs/ardour/session_state.cc
- **[D4] Ardour source, `libs/ardour/session_command.cc`.** https://github.com/Ardour/ardour/blob/master/libs/ardour/session_command.cc

### Open formats and models

- **[F1] Bitwig, DAWproject README/specification.** https://github.com/bitwig/dawproject
- **[F2] Bitwig, DAWproject 1.0 project XML schema.** https://github.com/bitwig/dawproject/blob/main/Project.xsd
- **[F3] Bitwig, official Controller Extensions repository.** https://github.com/bitwig/bitwig-extensions
- **[L1] LMMS source, `DataFile` XML persistence model.** https://github.com/LMMS/lmms/blob/master/include/DataFile.h
- **[L2] LMMS source, example `.mmp` XML project fixture.** https://github.com/LMMS/lmms/blob/master/tests/emptyproject.mmp
- **[T1] Tracktion, Tracktion Engine repository.** https://github.com/Tracktion/tracktion_engine
- **[T2] Tracktion Engine source, edit file operations and `ValueTree` loading.** https://github.com/Tracktion/tracktion_engine/blob/develop/modules/tracktion_engine/model/edit/tracktion_EditFileOperations.h
- **[C1] Sonic Pi, official tutorial, especially “Live Coding,” interface, programming structures, and sharing.** https://sonic-pi.net/tutorial.html

### Actual third-party agent integrations

- **[M1] Siddharth Ahuja, AbletonMCP repository.** https://github.com/ahujasid/ableton-mcp
- **[M2] `shiehn/total-reaper-mcp`, REAPER MCP Server repository.** https://github.com/shiehn/total-reaper-mcp
- **[M3] `WeModulate/bitwig-mcp-server`, Bitwig MCP Server repository.** https://github.com/WeModulate/bitwig-mcp-server
- **[M4] Veena Studio, FL Studio MCP repository.** https://github.com/veenastudio/flstudio-mcp

### MCP specification

- **[P1] Model Context Protocol specification 2025-06-18, “Tools.”** https://modelcontextprotocol.io/specification/2025-06-18/server/tools
- **[P2] Model Context Protocol specification 2025-06-18, “Security Best Practices.”** https://modelcontextprotocol.io/specification/2025-06-18/basic/security_best_practices
- **[P3] Model Context Protocol specification 2025-06-18, “Elicitation.”** https://modelcontextprotocol.io/specification/2025-06-18/client/elicitation
