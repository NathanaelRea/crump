# Crump Domain Glossary

## Product Terms

### Crump

A Linux-first terminal Guided Production Instrument in which text is the durable medium for authoring complete electronic tracks.

### Terminal-First

The first supported human client is a Ratatui workspace, while the Project model, engine, and authoring operations do not depend on terminal rendering. No alternative client is promised by this term.

### Guided Production Instrument

A production environment where a beginner can reach audible music quickly through progressively revealed controls and, optionally, an external Authoring Agent. The user learns by making and progressively assumes direct control; the environment is neither an autonomous song generator nor a separate production course.

### Complete Production Environment

Crump's product destination: one environment in which an electronic musician can compose, synthesize, arrange, automate, mix, save, reopen, and render a finished track. It is reached through validated end-to-end slices and is not the scope of the First Public Release.

### Validated Slice

The smallest end-to-end product expansion that removes the largest observed obstacle between the current workflow and a finished track. Each slice is selected from Beginner Musician and Experienced Producer evidence rather than a precommitted DAW feature backlog.

### Beginner Musician

A target user with no music-production, synthesis, or Crump DSL experience who is nevertheless comfortable installing and operating documented Linux terminal applications. The term does not mean a computer or terminal beginner.

### Experienced Producer

An independent electronic-music producer who understands composition, synthesis, arrangement, and mixing but did not implement Crump or author its Reference Project. This evaluator tests whether familiar concepts transfer and whether the generated instrument, controls, depth, and audio result are musically credible.

### Free Core

Crump's complete author, edit, play, save, and render capabilities require no Authoring Agent, paid service, paid asset, or hosted backend. External harnesses and models may be free or paid, but their availability and pricing are not Crump compatibility guarantees.

### Project

The complete portable authored unit: human-readable DSL and configuration files plus declared Project-owned media assets. A project owns exactly one song and does not depend on an opaque database or binary project document.

### Media Asset

A Project-owned regular WAV file with a durable source ID, normalized Project-relative path, and required lowercase SHA-256 of its complete bytes. An external path may be used only as an import input; Crump copies those exact bytes into the Project and never persists the external path. Distinct paths with identical content remain separate identities and produce a warning.

### Changed Media Asset

A declared Media Asset whose current complete file bytes hash to a different SHA-256 than the digest in Authoritative Source. Filesystem timestamps, ownership, host canonical paths, and decoded sample representations do not define change.

### First Public Release

The first supported release proving Crump's distinctive loop on a documented Linux configuration: an external Authoring Agent creates a brief-derived pitched polyphonic Composable DSP Graph instrument and a Musical Sketch from a blank Project, then a Beginner Musician hears, inspects, changes, restores, saves, reopens, and renders it through Crump. Complete-track production breadth and third-party plugin hosting are outside this release.

### Reference Project

The fixed, versioned Project produced by the First Public Release's creation workflow and shared by human acceptance and machine-verifiable checks. Its Musical Sketch uses stock synthesized percussion and a brief-derived, Agent-authored pitched polyphonic Native Instrument expressed as a Composable DSP Graph, with no imported or model-rendered waveform assets.

### Musical Sketch

A 30-60 second multi-part arrangement with a stock synthesized-percussion role, a pitched role, and more than one section or an audible variation. It requires only simple level balance and a lossless master, making it an integrated product proof rather than a polished complete track.

### Reference Project Acceptance Gate

The release gate combining a Beginner Musician's completion of the blank-to-render workflow, an Experienced Producer's independent inspection and modification of the resulting Reference Project, and objective checks of that Project. A blocked promised capability, data loss, crash, dangerous output, or required developer intervention fails the gate; non-blocking usability friction is recorded.

## Authored And Runtime State

### Crump DSL

The small purpose-built declarative language that represents a Project's musical structure, processing graphs, Parameters, Faceplates, and imports as human-readable Authoritative Source. It is not a general-purpose programming language; its First Public Release grammar is limited to concepts exercised by the Reference Project.

### Authoritative Source

The project text and configuration that own all durable authored state. Interactive controls may change a project only by producing a corresponding source edit.

### Candidate Revision

The current source revision being parsed and validated. It may contain errors and is not necessarily the revision producing audio.

### Active Revision

The most recent valid project revision published to the runtime and responsible for current playback.

### Project Publication

The successful whole-Project validation that records a Candidate Revision in Project History and schedules it for safe runtime publication. Workspace edits publish automatically; an external file edit requires an explicit machine-readable `crump` CLI command, with Agent origin and explanation when applicable.

### Affected Instrument Restart

The first-release transition for a valid topology change: Crump publishes the new graph with a short protective fade, resets the changed instrument's voices and effect state, and leaves transport and unchanged Tracks running. The workspace reports the restart; exact state migration is not implied.

### Structured Source Edit

A source change initiated by a structured TUI operation. It targets a known source construct, preserves unrelated source material where practical, and participates in the same Project History as direct source edits.

### Production View

A structured, source-backed workspace for manipulating a recognizable production concern such as arrangement, notes, mixing, routing, automation, or an instrument. Production Views are the primary human authorship surface; each durable operation produces a Structured Source Edit.

### Event Grid

The shallow Production View for a Pattern's concrete note, drum, and control Events, shown in musical time and editable without source knowledge. Procedurally produced Events may be inspected there but are changed only through expression-aware operations; the grid never silently flattens them into stored Events.

### Audition Phrase

A concrete looping Pattern containing notes and chords across a useful range for hearing a Native Instrument while changing its Instrument Faceplate. It provides hardware-free audition and remains editable through an Event Grid.

### MIDI Note Audition

Transient live MIDI note-on and note-off input sent to a selected Native Instrument on the documented reference setup. It does not capture Events, map controls, synchronize clocks, or create durable project state.

### Progressive Depth

An ordered set of source-backed views over the same authored object, beginning with a small set of musically meaningful controls and extending through detailed parameters, signal flow, graphs, and source. It does not create separate beginner and expert objects, engines, or project formats.

### Authoring Agent

An external model-driven tool that may author every layer of a Project by editing normal project files and using Crump's documented machine-readable CLI for inspection, validation, checks, Project Publication, and rendering. Its harness runs outside Crump in the First Public Release, and Crump never owns its model, inference, or conversation loop.

### Agent Change

One successful Project Publication requested by an Authoring Agent after an intended project-file save. It records the Agent origin, what changed, and why as an exactly reversible Project History Entry; a single user request may produce several Agent Changes, while unpublished or invalid saves remain Candidate Revisions and do not enter Project History.

### Project History Entry

One named, exactly reversible transition of the durable authored state caused by a direct source edit, a Structured Source Edit, an Agent Change, or a History Restore. Entries retain their origin and support comparison with the immediately preceding project state.

### Project History

The persistent ordered sequence of every Project History Entry, retained without automatic expiry or compaction in the First Public Release and preserved across close and reopen. It supports ordinary comparison and restoration independently of Git; any Git commit, branch, or milestone integration is user-requested rather than automatic.

### History Restore

An action that publishes the exact durable state of a selected earlier Project History Entry as a new entry referencing that source entry. It never deletes existing entries, moves a history pointer, or creates a user-visible branch.

### Durable Domain ID

An explicit source-carried identity for an authored Track, Clip, Group, Return, device instance, Send, or reusable-graph node, unique within its owning scope. Rename, movement, reorder, and cut-and-reinsert preserve it; structured duplication creates a fresh ID and rewrites copied internal references. Missing or duplicate IDs make a candidate revision invalid and are never repaired silently. A runtime node inside a reusable graph is identified by its enclosing instance-ID path plus its definition-local node ID. The singleton Master has intrinsic identity and does not carry an ID.

## Musical Structure

### Song

The single musical arrangement owned by a project: tracks placed on a linear timeline and routed through instruments, effects, groups, sends, returns, and buses. Song time begins at zero and has an explicit positive arrangement end; clips cannot start before zero or extend beyond that end. The arrangement end defines the finite authored core range, while rendering policy separately governs downstream release and effect tails.

### Timeline

The linear, musically addressed arrangement axis used by the first public release. Session-style scene launching is not part of this definition.

### Musical Time

The exact rational quarter-note coordinate shared by the timeline, patterns, clips, events, and durations. Authored bar-and-beat notation resolves through the song's meter map; the tempo map converts musical time to runtime sample positions under deterministic rounding semantics.

### Locator

A durably identified, human-named position on the Song Timeline. It is a navigation and audition boundary, not an owner of material: moving a Locator does not implicitly move Clips, Events, or Automation. Checked time-oriented operations may resolve its ID to exact Musical Time before publishing canonical source.

### Tempo Map

The song-wide sequence of positive BPM points that maps musical time to elapsed runtime. An explicit marker is required at song time zero; marker positions are strictly increasing and unique. Each segment explicitly uses step or linear interpolation in BPM over musical time, and playback and offline rendering share the same mapping.

### Meter Map

The song-wide sequence of meter markers used to interpret bar-and-beat notation. An explicit marker is required at song time zero; marker positions are strictly increasing and unique. A marker may occur at any absolute musical time; if it interrupts a bar, the preceding bar is a legal fragment and the marker begins the next numbered bar at beat one. Each meter has a positive numerator and a power-of-two denominator.

### Pattern

Reusable, timeline-independent musical material expressed in relative time. A pattern owns its events and compositional logic, declares a positive finite musical span, and may be referenced by multiple clips. Its canonical times are rational quarter-note units and never inherit meter from placement; bar-and-beat notation requires an explicit pattern-local meter used only for authoring and display. Pure composition uses sequence, whose span is the sum of its operands; parallel composition, whose span is their maximum; and positive integer repetition, which multiplies the operand span. The required pure transformation floor is rational time shift, positive rational time scale, reversal within the span, cyclic rotation, pitch transposition, velocity or value scaling, event filtering, and deterministic probability gating. Transformations are type-checked, applied left to right, and explicitly map events and span without mutating referenced patterns. Events and generators are evaluated within the resulting span; ongoing material is expressed through repetition rather than an unbounded pattern.

### Pattern Variation

A Pattern whose authoritative source retains an acyclic reference to another equal-span Pattern plus explicit rotation, transposition, velocity scaling, reversal, deterministic probability, and seed values instead of copied Events. Variations may chain. They are validated and materialized into immutable runtime Events before publication while inspection preserves lineage; reordering source Events cannot change probability identity. Inherited Parameter Locks follow the Variation's time transforms but are not removed by Event probability.

### Groove Template

A durably identified reusable Clip transform with an exact subdivision grid, straight-to-swung odd-subdivision placement, bounded deterministic timing and velocity displacement, independent deterministic Event probability, and an authored seed. It changes only a Clip's materialized Event excitation and onset; source Events and other Clips remain unchanged. Semantic Event identity makes its result independent of source order and callback grouping.

### Clip

An arranged occurrence of a pattern on a track, carrying an explicit source ID that survives rename, movement, reload, and restart. A clip owns its timeline start, positive duration, source offset, explicit `once` or `loop` playback mode, and placement-level transformations. Evaluation first produces the finite pattern value, applies clip transformations left to right, normalizes the source offset against the transformed span, traverses once or loops, crops to the clip interval, and translates events into song time. `once` traverses the transformed pattern at most once from the source offset; `loop` wraps at its span until the clip ends. Its timeline interval is half-open: events at the end are excluded and active note-like events are terminated there, while downstream release and effect tails may continue. Loop boundaries do not truncate prior-cycle events, and a source offset does not chase earlier events. Editing the referenced pattern affects every clip that references it. Placement-local source offset, whole-semitone transposition, excitation gain, boundary fades, and optional Groove Template remain explicit and do not mutate the Pattern.

### Track

A stable timeline lane with an explicit source ID that survives rename, reorder, reload, and restart. Authored track order controls presentation and navigation only and cannot affect sound. A track's clips emit typed events into an assigned processing chain; overlapping clips contribute additively and their events are merged deterministically without implicit precedence. The exception is Parameter locking: Clips sharing one Instrument cannot overlap when either carries locks, because an absolute lock must never acquire hidden precedence. Tracks do not have fixed MIDI, audio, or instrument kinds; validation determines whether their events and assigned devices are compatible. Groups and Returns are separate routing-only channels, while Buses are explicit typed signal paths rather than channels.

### Group

A routing-only hierarchical summing channel for Tracks and nested Groups. It cannot own Clips, may host processing and mixer controls, and provides the presentation boundary for folding or navigating its members. Membership requires a member's main output to flow through its parent Group; parallel routing is expressed separately rather than by bypassing the Group.

### Return

A routing-only parallel-processing channel whose input is contributed through explicit Sends. It cannot own Clips and may host processing and mixer controls. Its main output defaults to the Master and may instead target another Return or a Group when the resulting graph remains acyclic.

### Send

A durably identified, gain-controlled branch from a Track, Group, or Return to exactly one Return. Each Send explicitly taps its source before or after the source's mixer stage; its branch cannot participate in a routing cycle.

### Master

The Song's single final processing channel and the default destination for audible routing. A non-authoritative safety stage after the Master sanitizes and emergency-limits real-time monitoring only; offline rendering bypasses it, reports non-finite output as failure, and is limited only by explicit authored Master processing.

### Bus

A typed signal path connecting an output to an input. A Bus is routing between Tracks, Groups, Returns, the Master, and device ports; it is not a separate kind of authored channel. A Track or nested Group's main Bus goes to its parent Group, while other Tracks and Groups default to the Master; timeline Tracks cannot serve as ad hoc main-output destinations. Inter-channel and inter-device Buses cannot form cycles; intentional feedback is encapsulated by an explicit bounded stateful device.

### Channel Strip

The fixed processing order shared by Tracks, Groups, and Returns: sum inputs, run ordered inserts, expose the pre-fader Send tap, apply gain and an explicitly selected panner mode, apply mute, expose the post-fader Send tap, then route the main output. `mono_pan` uses equal-power mono-to-stereo placement, `balance` attenuates stereo sides without crossfeed, and `stereo_pan` positions the two input sides independently; the declared input layout determines which modes are valid.

### Stem

A delivery WAV captured from an explicitly selected Track or Group at its `pre_fader` or `post_fader` Channel Strip tap. It shares the Master Render Range, sample rate, stereo layout, exact frame count, and tail policy. Zero padding aligns the selected tap to the compiled Master output latency. The stem includes upstream main-route material and processing through the selected owner's inserts; a post-fader stem also includes that owner's gain, panner, and Mute. It excludes outgoing Send branches and all downstream Group, Return, and Master processing. Consequently, a set of stems reconstructs the Master only when the selected routing and processing policy mathematically permits it; Crump does not imply that arbitrary requested stems sum to Master.

### Audition State

Transient workspace state used to play, pause, seek, loop, Solo, cue, bypass, or monitor part of the Active Revision without changing the authored mix. It is not Authoritative Source, Project History, or offline-render state. Referenced Durable Domain IDs are re-resolved when a runtime is published; a missing or invalid target is reported while the prior audible runtime continues.

### Audition Plan

One inspectable request naming an Active Revision target, an Audition Path, an exact half-open range, loop policy, and monitoring-only processor-bypass, Solo, and cue overrides. Track, Clip, Pattern, Media Asset, sampler Mapping, and durable sample Occurrence targets use source identities rather than runtime indexes. Correct seek preparation and deterministic preroll happen away from the audio callback before the prepared runtime is published at a block boundary.

### Audition Path

The explicit depth at which selected material is monitored before the common non-authoritative monitoring safety stage: `raw` is the decoded and Project-rate-converted asset region; `mapping` includes sampler mapping/occurrence behavior and the Instrument post chain; `channel` selects a Track's pre- or post-fader Channel Strip tap; and `master` follows the complete authored route. Selecting a path does not rewrite routing or create a render dependency.

### Transient Processor Bypass

An Audition State override addressed by a processor's song-wide Durable Domain ID. It affects Monitoring only, is excluded from Authoritative Source, Project History, and offline rendering, and uses a short guarded transition to a full-output-latency-aligned dry path. The wet processor continues to run silently so detector, delay, and tail state survive re-enable.

### Durable Processor Bypass

The source-backed boolean `bypass` value on a processor. It is a step Parameter, affects playback and offline rendering after Project Publication, preserves the processor's declared output latency, and is changed only through a checked source edit. A missing directive means `false` and may be inserted by that edit.

### Processor Chain Order

The authoritative declaration order of processor blocks within one Instrument, Track, Group, Return, or Master owner. A checked adjacent move retains the processor ID and complete block with attached leading comments; moving across owner chains is invalid. Reorder is a topology change and uses protective runtime publication.

### Event

A typed occurrence emitted by a pattern at an exact relative musical time. Events form an ordered multiset and carry a typed payload plus type-specific properties such as duration, velocity, or probability. An event onset lies within the pattern's half-open span, but its duration may cross the span and overlap a later loop iteration. Notes in a chord are simultaneous note events, rests are gaps without events, one-shot triggers may be instantaneous, and note-like events have positive duration. Simultaneous events use a deterministic tie-break order without making source order musically significant unless their consumer explicitly requires ordering.

### Procedural Automation

A parameter change over time generated by a code expression, pattern, envelope, or modulator rather than stored only as editable points. Structured editing must not silently flatten it into point data.

### Automation

The single Song-scoped time-varying absolute value for a target Parameter. Automation replaces the Parameter's base value at a given time before an optional Parameter Lock and Modulation are applied; procedural and point-based representations preserve their distinct source forms. A point lane begins at Song time zero, has strictly increasing unique positions within the arrangement, and holds its last value through the arrangement end. Each segment explicitly selects `step`, `linear`, or `power(p)` interpolation in the Parameter's canonical physical unit. For `power(p)`, positive `p` raises normalized segment progress to that power before interpolating the endpoint values. Playback and offline rendering evaluate the same curve at runtime sample positions derived through the Tempo Map. Continuous reusable control from Patterns, envelopes, and LFOs reaches Parameters as Modulation rather than competing absolute Automation. Continuous Automation has sample-accurate, block-size-independent semantics.

### Parameter Lock

A durably identified, Pattern-relative physical-unit step override for one automatable, topology-stable Parameter on the Instrument consuming that Pattern through a Clip. It is applied after Song Automation and before additive Modulation, follows Pattern Variation timing, source offset, and looping, and resets at the Clip's end. It is intentionally narrow rather than a second free-form Automation lane; validation rejects ambiguous overlapping Clips that share its Instrument.

### Modulation

A routed normalized unipolar or bipolar Control signal that contributes a time-varying offset after a Parameter's base value, Automation, and optional Parameter Lock are evaluated. Each route has an explicit depth in the target's declared modulation unit; multiple routes combine additively in deterministic order before the result is constrained to the Parameter's legal domain. Continuous Modulation has sample-accurate, block-size-independent semantics.

### Composable DSP Graph

A project-language definition that connects sound and control nodes into reusable instruments or effects through an explicit typed interface. Definitions remain hierarchical authored units even if compilation flattens them.

### Graph Validation

Machine enforcement of a Composable DSP Graph's topology, types, units, Parameter mappings, declared resource bounds, deterministic contract, and finite authored values before publication. Loud but finite real-time output is contained separately by the non-authoritative monitoring safety stage; neither mechanism scores sound quality, useful note range, playability, or musical value.

### Native Instrument

A reusable sound generator executed by Crump that turns musical Events and controls into Audio without requiring recorded source audio. It may be built in or defined as an inspectable Composable DSP Graph, including through an Agent Change.

### Sound Brief

A musician's plain-language description of an intended sound and playing behavior without requiring oscillator, filter, modulation, or other DSP instructions. An Authoring Agent may translate it into a Native Instrument, but the brief is not itself executable project state.

### Creative Brief

A beginner's short everyday-language description of a Musical Sketch's intended mood, energy, pace, and sound, optionally including a Sound Brief. An Authoring Agent translates it into an explained Musical Sketch; the brief is creative direction rather than executable project state or a finished-song request.

### Human Listening Loop

The musician, not the Authoring Agent, judges the audible and musical result and supplies ordinary-language feedback for another Agent Change. An Agent may receive render files, but audio-input or audio-understanding capability is not part of Crump's agent contract.

### Instrument Faceplate

The shallowest Production View of a Native Instrument: a small declarative set of musically meaningful controls with names, explanations, units, ranges, defaults, and explicit mappings into the underlying instrument. Crump renders it from authored metadata; deeper views reveal parameters, modulation, the Composable DSP Graph, and source.

### Graph View

The first-release inspection surface for navigating a Composable DSP Graph's nodes, connections, Parameters, Faceplate mappings, and source locations. It does not create, delete, or reconnect nodes; topology changes are direct source edits or Agent Changes.

### Instrument Module

A portable human-readable source unit containing a Native Instrument's Composable DSP Graph, typed interface, and Instrument Faceplate. It may be copied or imported across Projects and saved to a personal library without compilation, a package registry, or an opaque preset format.

### Synthesis-Complete

The Complete Production Environment's capability to create every sound required for a finished electronic track with Native Instruments and native processing, without imported or model-rendered waveform assets. The First Public Release proves the smaller Reference Project subset; samples may be supported later, but no core workflow or release gate depends on them.

### Processing Node

An audio-engine operation with named, typed input and output Ports. Processing Nodes may consume or produce Event streams, channel-count-aware Audio streams, and scalar Control signals. Each output declares deterministic sample latency for its active configuration. Event, Audio, and Control streams share a sample-time domain, and the graph compensates every convergence point so a node observes all inputs at the same source time, including parallel, Send, Return, sidechain, and cross-domain paths. Playback and offline rendering use the same compensation semantics. The host supplies exact effective Parameter values without universal smoothing; any smoothing is declared deterministic DSP behavior of the node itself.

### Port

A named, typed connection point on a Processing Node. A Sidechain is an auxiliary Audio input Port rather than a separate routing mechanism. Audio Ports declare their channel layout; incompatible layouts require an explicit conversion node rather than an implicit upmix or downmix. Control Ports declare a scalar unit and legal domain; incompatible units require an explicit conversion node. Generic modulation sources explicitly produce unipolar `[0,1]` or bipolar `[-1,1]` Control. Each input declares single, optional, or summing cardinality; multiple Buses may meet only at an explicit summing input, where stable identity determines combination order.

### Parameter

An addressable control exposed by a device or routing channel. Its stable identity is its owner's durable or intrinsic identity plus an immutable key defined by the owner type, so rename and reorder preserve references while replacement creates a different target. First-release value kinds are continuous real, bounded integer, boolean, and enum; only continuous values support non-step interpolation or Modulation, while momentary actions are Events rather than Parameters. Each Parameter declares its legal domain, default, canonical physical unit, display mapping, and automation and modulation capabilities. Authoritative Source uses physical values; normalized values exist only at controller or plugin adapter boundaries. Out-of-domain authored values invalidate a Candidate Revision; finite runtime modulation overshoot is clamped, while a non-finite control result holds the last finite effective value and emits a diagnostic.

### Rust Extension

An advanced post-release mechanism for implementing DSP nodes, instruments, or effects below the level exposed by the Composable DSP Graph. It is not part of the First Public Release or the ordinary Agent Change trust model.

## Reproducibility

### Render Range

The First Public Release's offline interval from Song time zero through the arrangement end plus the authored Render Tail. The range is visible before rendering and identical for unchanged repeated renders.

### Render Tail

A non-negative duration in readable Project render settings for processing release, delay, and reverb after the arrangement end. Crump does not infer it through silence detection or a hidden global duration.

### Finalized Audio Analysis

A versioned set of delivery measurements derived by reopening the completed stereo float WAV rather than observing an in-memory bus. It contains analyzed frame count, sample peak, a named fixed-oversampling reconstructed-peak estimate, BS.1770/EBU-derived K-weighted gated integrated loudness, maximum three-second Short-term loudness at 100 ms steps, Loudness Range, per-channel DC offset, clipping counts, and non-finite count. Silence or insufficient measurement windows remain explicit missing values. The method and kernel identities disclose the exact product algorithm; they do not claim complete standards conformance, normalization, or a universal loudness target.

### Qualification Report

A versioned machine-readable record from profiling one exact Active Revision and half-open frame range at a supported offline full-block quantum, with one disclosed short final block when needed. Each block is timed against its exact frame deadline. It binds build/host/resource identity, timing distribution, deadline and finite-output facts, and the operator's p99.9 threshold. It is not PipeWire evidence: xruns remain absent and named-hardware endurance plus callback-safety auditing stay external.

### Deterministic Evaluation

Evaluation in which the same project, assets, compatible engine version, and explicit inputs produce the same musical result. By default, a random choice derives from the project seed, pattern semantic path, clip durable ID, loop iteration, and a stable source-local choice key. Separate clips and iterations vary, while clip movement or rename does not reroll choices; source may explicitly override seed or scope.

### Deterministic Comparison Domain

Repeated rendering of the same Project and render settings with the exact same Crump build and sample rate on the documented `amd64` reference environment produces a byte-identical stereo 32-bit-float WAV and manifest. It makes no bit-identity promise across engine builds, architectures, or unsupported environments.

## Terms Requiring Decisions

The following name appears in the product contract but does not yet have a sufficiently precise, agreed meaning: compatible engine version.
