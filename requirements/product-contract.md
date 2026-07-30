# Crump Product Contract

Status: Draft; canonical product requirements

## 1. Product Summary

Crump is a Linux-first, terminal-first, agent-ready guided music composition and production environment. It combines a purpose-built, declarative music language with a detailed Ratatui interface and a real-time audio engine written in Rust.

The product takes inspiration from Strudel's use of code as a musical medium, not from its web architecture or exact language. Its intended depth is closer to an electronic-music DAW: users should be able to sequence samples, create synthesizers, arrange complete songs, automate parameters, mix tracks, inspect signals, and render finished work while keeping the project human-readable and suitable for version control. Like mature open-source production tools used in professional work, Crump aims to earn trust through capability, performance, stability, interoperability, documentation, and reproducible evidence rather than through proprietary status or feature count.

The first compelling release is a guided production instrument rather than primarily a live-coding instrument, recording studio, autonomous song generator, or separate teaching course.

The first supported human client is the Ratatui workspace. The project model, authoring operations, and engine must not depend on terminal rendering, but no desktop or browser client is committed.

The long-term destination remains a complete electronic-music production environment in which a musician can move from a blank Project to a finished track without exporting into another DAW. Crump reaches that destination through validated end-to-end slices; destination breadth is not an instruction to implement unexercised features in the First Public Release.

## 2. Product Vision

A beginner can open a terminal, get to audible music quickly through progressively revealed controls and optional help from an external authoring agent, then inspect and change the result through real production concepts. As the musician gains experience, Crump exposes the precise language, controls, visualizations, and production depth needed to finish a polished track without leaving the application.

Crump should make code a durable, inspectable production medium without requiring code to be the default human interface. Recognizable production views lead the workflow, while text remains the source of truth and an always-available expert and agent surface.

## 3. Target User

The primary user is someone who wants to make electronic music and learn production by making it, whether they begin with no production experience or arrive with professional experience. A Beginner Musician need not know production, synthesis, or Crump's DSL, but may be expected to install and operate documented Linux terminal applications. Crump may ask users to learn a focused DSL and standard production concepts as they take greater control. The product may appeal especially to programmers, but it must not assume that its users want a separate music-theory curriculum.

Crump is optimized for electronic and synthesized music. Other genres may use it where synthetic timbres fit, but realistic emulation of acoustic instruments, orchestras, or human performers is not part of the product promise.

The user values:

- Precise, reproducible control over composition and sound.
- Fast keyboard-driven workflows.
- Immediate feedback while editing.
- Deep synthesis and modulation capabilities.
- Human-readable projects that work naturally with Git.
- Enough mixing and arrangement depth to finish music, not only generate loops.

Crump provides contextual guidance, documentation, examples, and clear diagnostics. Learning through creation is a primary product requirement; a separate music-production or music-theory curriculum is not.

## 4. Product Principles

### 4.1 Text is the source of truth

A project is fully represented by human-readable DSL and configuration files plus referenced media assets. Project state must not depend on an opaque database or binary project document.

Durable edits made through TUI controls must safely rewrite the corresponding source. Reloading the resulting text must reproduce the saved project.

### 4.2 Immediate but safe feedback

Every syntactically and semantically valid workspace edit should update playback automatically. External file edits become Candidate Revisions and require explicit Project Publication through the machine-readable CLI. Invalid or unpublished edits must not interrupt the currently playing Active Revision.

Graph changes should become audible without avoidable clicks, dangerous output, or full-engine restarts. Each supported edit class must declare its publication and restart behavior rather than relying on an undefined “safe boundary.”

In the First Public Release, a topology change may perform an Affected Instrument Restart: reset that instrument's active voices and effect state behind a short protective fade while transport and unchanged Tracks continue. The workspace must report the restart. Exact state migration across topology changes is deferred.

### 4.3 Advance toward complete tracks

At the Complete Production Environment destination, the core workflow supports arranging and finishing a complete electronic track. Each Validated Slice is judged by whether it advances composition, arrangement, production, saving, reopening, and rendering rather than creating an isolated technical demo.

### 4.4 Reproducibility by default

Given the same project, assets, compatible engine version, and explicit inputs, playback and offline rendering must produce the same musical result. Random and generative operations use stable stored or derived seeds unless the user explicitly requests nondeterministic behavior.

Within the First Public Release comparison domain, repeated unchanged renders use one stereo 32-bit-float WAV format and produce byte-identical WAV and manifest files for the same Project, render settings, exact Crump build, sample rate, and documented `amd64` reference environment. Crump makes no bit-identity promise across builds, architectures, or unsupported environments.

### 4.5 Progressive depth

Common operations should have concise high-level forms with contextual explanations of unfamiliar production concepts. Users must be able to descend through increasingly detailed views of the same authored objects into synthesis, modulation, signal flow, graphs, and source without requiring every project to operate at that level. Crump must not maintain separate beginner and expert project models or engines, and experienced users must be able to enter at any depth. Compiled Rust extensions are a possible post-release depth, not a first-release requirement.

### 4.6 Agent-ready, not agent-owning

Crump must not contain or require a model provider, inference runtime, conversation loop, or hosted AI backend. It instead exposes a stable authoring contract that external agent harnesses can use with a model chosen by the user. In the First Public Release the harness runs beside Crump and must not gain hidden state or an alternate editing path.

After each intended valid save, an Agent calls an explicit `crump` publication operation with its origin and explanation. A successful operation makes that complete source state the current durable state and its own named history entry, even when one request causes several publications; runtime activation still follows the normal safe graph-transition path. Crump must show each change's origin, what changed, and why; support immediate comparison with the preceding state; and restore that state exactly with one action. Invalid or unpublished generated source must not replace the Active Revision or enter Project History.

Project history must survive closing and reopening and remain usable without Git knowledge. The First Public Release retains every entry without automatic expiry or compaction. Its source-backed changes may integrate naturally with Git, but version-control mechanics must not be required for ordinary comparison or restoration, and Crump must never silently commit to or branch the user's repository.

### 4.7 Free, model-neutral core

Every core operation needed to author, edit, play, save, and render a Project must work without an Authoring Agent, paid service, paid asset, or hosted backend. External agents accelerate and guide the same complete manual product; they do not unlock otherwise unavailable engine capabilities.

Release evidence may name a harness and model configuration available at test time, including currently free options, but Crump does not guarantee a third party's future model availability or pricing.

### 4.8 As simple as possible, but no simpler

Crump should adopt proven professional workflow and engineering guarantees without inheriting every mode or abstraction of a conventional DAW. A capability belongs in the supported product only when it is necessary for the complete production workflow, protects a required quality attribute, or has been validated by representative users.

For a problem already solved by professional DAWs, discovery must begin with current primary-source web research: official Ableton documentation first where Live has an analogous workflow, followed by other professional DAWs and relevant standards. That evidence establishes a baseline, not a feature-parity requirement. Crump should implement the smallest coherent design that preserves the baseline task and its correctness, performance, recovery, inspection, and export guarantees.

After each supported release, Beginner Musician and Experienced Producer evidence identifies the largest obstacle to extending the same Project toward a finished track. The next release implements the smallest Validated Slice that removes that obstacle. Automation, professional routing, stems, sampling, plugins, structured Graph View topology editing, and other destination capabilities do not enter because an old backlog or competitor inventory lists them.

## 5. Core User Experience

### 5.1 Workspace

The First Public Release presents an integrated keyboard-first workspace containing:

- Transport and playback status.
- Track, clip, and arrangement navigation.
- Terminal-friendly note and drum Event Grids with visible playhead and Event activity.
- Instrument Faceplate and Parameter views plus an inspectable Graph View.
- Basic voice or instrument activity and safe output-level feedback.
- Diagnostics and engine status.
- Project history.
- An always-available source editor for the project DSL.

Mixer, routing, effects, Automation, sample and preset browsing, waveform, oscilloscope, and spectrum views belong to the complete-track destination only when a validated task requires them.

Recognizable production views are the primary human authorship surface. A beginner must be able to perform the ordinary end-to-end workflow without editing source, while experienced users may inspect or directly edit the DSL at any time. Every durable operation in a production view is a structured source edit rather than hidden UI state.

Every workflow has a discoverable keyboard path and exact value entry. Mouse input may select clips, Events, controls, and graph nodes where the terminal supports it, but mouse availability, precision dragging, or gesture behavior cannot be required for correctness.

The shallow composition workflow uses a conventional clip Timeline and Event Grids for concrete notes and rhythms. Reusable Pattern composition, transformations, probability, and procedural expressions are progressively deeper views of the same musical model. A grid may inspect procedural results but must not flatten them silently; edits require a recognized expression-aware operation or remain source-only.

### 5.2 External Authoring Agents

Crump must publish concise authoritative language guidance and examples suitable for installation as an agent skill or equivalent harness context. The first-release integration contract is normal project-file editing plus stable `crump` CLI operations with machine-readable output to inspect a Project, validate source, run project checks, publish a complete source state with origin and explanation, and render an authored result. Publication must work whether or not the TUI is running. An Agent must be able to diagnose and repair its work without scraping the TUI.

Machine checks enforce graph topology, types, units, Parameter mappings, declared resource bounds, deterministic behavior, finite values, and output safety. They must not claim to determine whether an instrument sounds good, has a musically useful range, plays expressively, or exposes good controls; the Beginner Musician and Experienced Producer decide those properties by listening and interaction.

The musician closes the sound-design loop by listening and giving ordinary-language feedback such as “less harsh” or “shorter decay.” Audio-capable models are optional; the agent contract requires source, diagnostics, and render access but never model-side audio understanding.

The contract may author every durable project layer, including musical material, arrangement, instruments, effects, routing, automation, and mix. Instrument construction is a distinctive capability, not a separate or privileged agent API.

The agent contract must remain useful across different harnesses and model providers. Using an agent is optional: every core authorship, editing, playback, persistence, and rendering capability must remain available manually through Production Views or source.

MCP servers and harness-specific plugins are deferred until evidence shows the file-and-CLI contract is insufficient. In the first release, the harness runs in a neighboring terminal or a user-managed multiplexer pane; embedding a nested terminal or PTY inside Crump is deferred until workflow evidence justifies it.

### 5.3 Song Model

A Song consists of Tracks arranged on a linear Timeline. Tracks contain Clips that reference reusable Patterns and route Events and Audio through instruments and effects. The Complete Production Environment adds Groups, Sends, Returns, and general mixer Buses; the First Public Release uses simple Track balance into Master.

At the Complete Production Environment destination, the language supports:

- Defining reusable musical patterns and clips.
- Placing, repeating, transforming, and composing clips on a timeline.
- Expressing notes, chords, rhythm, rests, velocity, duration, and probability.
- Referring to time in musically useful units such as beats and bars.
- Tempo and meter changes where required by an arrangement.
- Structuring project code across multiple readable files.

Ableton-style scenes and clip launching are deferred until the core timeline workflow is proven.

### 5.4 Editing and Evaluation

When a workspace edit becomes valid, Crump automatically publishes it and updates the running project. An external filesystem edit is parsed as a Candidate Revision but is published only by an explicit CLI operation. Runtime publication follows the applicable transition contract; for first-release topology changes, Affected Instrument Restart defines exactly what continues and resets.

When source is invalid, Crump must:

- Keep the last valid audio graph running.
- Show precise diagnostics with source locations.
- Avoid partially applying an invalid project state.
- Make it clear which source revision is currently producing audio.

When a user changes a value through a Production View, Crump must perform a Structured Source Edit rather than storing hidden runtime state. The edit should preserve unrelated formatting and comments where practical and create one Project History Entry.

## 6. Music Language

Crump uses a purpose-built declarative DSL. It should be concise for sequencing and arrangement while remaining explicit enough to understand routing and sound construction.

The first-release grammar is intentionally limited to the accepted Musical Sketch slice: Project and Song declarations; Tracks; concrete Events in Patterns; Clips on a linear Timeline; graph nodes, typed connections, Parameters, envelopes, modulation, and Instrument Faceplates; reusable Instrument Modules; and imports. Language features enter only when the reference workflow exercises them. The DSL is not a general-purpose programming language.

The language must provide first-class concepts for:

- Projects, tempo, meter, and timeline position.
- Tracks, clips, patterns, instruments, effects, and buses.
- Notes, chords, drums, samples, and control values.
- Pattern composition and transformation.
- Parameters, modulation, envelopes, and automation.
- Audio and control-signal routing.
- Reusable definitions and imports.
- Deterministic randomness.
- Native instruments and Composable DSP Graphs.
- Declarative Instrument Faceplates and their mappings to graph Parameters.

The language should permit high-level musical expressions without preventing sample-accurate or DSP-level control where the engine supports it.

Syntax design, the minimal type system, and the exact model for incremental evaluation remain design work. The language must produce diagnostics suitable for continuous editing, Agent repair, and safe source rewriting.

## 7. Sound Sources

### 7.1 Sampling

Imported samples are an optional sound source, not part of the synthesis-complete release guarantee. No first-release acceptance item may depend on a bundled, purchased, imported, or model-rendered waveform asset. If sample import is included, it must preserve the text-based project model and define coherent browsing, loading, sequencing, tuning, envelope, and processing behavior.

Deep sample-level editing, waveform drawing, and advanced time warping are not committed.

### 7.2 Built-in Synthesis

The first public release must include native synthesizers capable of producing a musically useful range of electronic sounds. At minimum, the native synthesis system exposes oscillators, noise, envelopes, filters, amplification, modulation, polyphony, and Parameter control, plus a simple stock synthesized-percussion instrument for the Musical Sketch's rhythmic role.

The First Public Release must synthesize every Reference Project sound without samples: stock synthesized percussion supplies its rhythmic role, and the Agent-authored pitched polyphonic graph instrument supplies its pitched role. The destination Synthesis-Complete guarantee extends that independence to finished electronic tracks.

The first release supports custom instrument development through:

- A composable DSP graph expressed in the project language for connecting oscillators, filters, envelopes, modulators, and effects into reusable instruments.

The graph DSL is part of the normal composition workflow. A Rust extension API for lower-level DSP is deferred until a separate post-release trust, isolation, real-time safety, compatibility, and versioning contract is accepted.

Crump's defining agent-authored synthesis workflow creates an inspectable, reusable native instrument rather than rendering a prompted waveform that the project treats as a sample. The resulting instrument must expose meaningful controls and remain editable at progressively deeper levels. Producing convincing human vocals is a separate research problem and is not implied by the native instrument requirement.

Every graph-authored instrument must declare an Instrument Faceplate with a small set of musically meaningful controls. Each control declares its name, short explanation, unit, legal range, default, and explicit mapping to graph Parameters. Crump renders this metadata as the first instrument view; an all-parameter list or raw graph alone does not satisfy the beginner workflow.

The First Public Release's Graph View supports navigation of nodes, connections, Parameters, Faceplate mappings, and source locations, but not structured topology editing. Creating, deleting, or reconnecting nodes remains a direct source edit or Agent Change until producer evidence justifies a terminal graph editor.

A graph-authored instrument and its Faceplate form a portable human-readable Instrument Module. Users must be able to copy or import that module into another Project and save it to a personal library. A package registry, dependency resolver, public marketplace, and opaque compiled preset format are outside the first release.

### 7.3 Effects

The First Public Release provides the minimal creative processing needed by the Musical Sketch: filtering, gain and pan, simple delay, simple reverb, and saturation. These nodes may be used inside Instrument Modules or on Tracks where the first-release graph permits. A separate non-authoritative monitoring safety stage after Master contains dangerous real-time output and is not an authored or rendered effect.

Compressors, detailed EQ, mastering tools, sidechains, Sends, Returns, and advanced routing are complete-track destination capabilities rather than first-release requirements. Exact algorithms remain implementation and release-planning decisions.

## 8. External Plugins

Third-party plugin hosting is deferred beyond the first public release. The native release must prove Crump's synthesis-complete workflow before accepting the scanning, opaque-state, vendor-GUI, latency, crash-isolation, and reproducibility costs of a plugin ecosystem.

Any future CLAP or VST3 hosting decision requires a focused contract covering:

- Cross-platform and Linux host behavior.
- Licensing implications.
- Plugin scanning and crash isolation.
- Persistence and reproducibility of opaque plugin state.
- Handling plugins that require graphical interfaces in a terminal-first product.

Rust libraries exist for both ecosystems, including CLAP host abstractions and VST3 bindings, but library availability alone does not satisfy the reliability and UX requirements. Creating Crump-native plugins and hosting third-party plugins are separate capabilities.

## 9. MIDI and External Control

The First Public Release supports MIDI Note Audition on one documented reference setup: transient note-on and note-off messages play a selected Native Instrument without creating durable state. The Beginner Musician's workflow remains hardware-free through an editable looping Audition Phrase; the Experienced Producer uses MIDI to judge live playability.

The complete-track product should additionally let a user:

- Play a selected native instrument.
- Record or capture note input into a text-representable clip.
- Map knobs, faders, and pads to project parameters.
- See active mappings and incoming control activity.
- Persist mappings in human-readable project configuration.

Advanced requirements such as MPE, OSC, external clock synchronization, and comprehensive MIDI file interchange are deferred.

## 10. Automation and Modulation

Parameters may vary over time through code expressions, patterns, envelopes, LFOs, and automation data. Crump must display automation as editable terminal lanes where a meaningful visualization is possible.

Changes made in an automation lane must rewrite the source representation safely. Code-generated or procedural automation must remain identifiable as procedural rather than being silently flattened into points.

The engine must define parameter ranges, units, interpolation behavior, and timing precision clearly enough that automation is deterministic and portable across playback and offline rendering.

## 11. Mixer and Routing

The complete-track product requires a professional mixer with:

- Track and bus gain and pan.
- Mute, solo, and monitoring state as applicable.
- Group tracks.
- Insert effect chains.
- Sends and return tracks.
- Audio buses and explicit routing.
- Parameter automation.
- Peak metering and enough level information to mix safely.
- A protected master output with safeguards against dangerous levels.

The destination routing architecture must not preclude later sidechaining or resampling, but neither capability nor its UX is a First Public Release requirement.

## 12. Visualization and Inspection

The First Public Release must show the Timeline playhead, Event activity, instrument or voice activity, safe output levels, and current graph and Parameter state needed to understand the Reference Project. Waveform, oscilloscope, spectrum, Automation, and advanced routing views are deferred until validated tasks require them.

Visualizations must remain responsive and must never compromise real-time audio processing. They may downsample or summarize data to fit terminal resolution.

Direct waveform editing is outside the first release.

## 13. Saving, Assets, and Portability

A project must be understandable and reviewable as text. Crump must:

- Save all authored musical structure, routing, parameters, automation, mappings, and deterministic seeds in human-readable files.
- Refer to media assets predictably and report missing or changed assets.
- Support portable project paths or an explicit asset-collection workflow.
- Preserve enough version information to diagnose incompatible language, engine, extension, or plugin behavior.
- Save and reload without changing the audible project except for documented external-plugin limitations.
- Persist every First Public Release Project History Entry without automatic expiry or compaction and without making history the authoritative current state.

Large audio assets do not need to be embedded in text or stored directly in Git.

## 14. Rendering and Export

Crump must render projects offline to a lossless master. The complete-track product should additionally export:

- Individual track stems.
- Relevant group or bus stems through a clear selection mechanism.

The first release exports a stereo 32-bit-float WAV plus a machine-readable manifest identifying the Project revision, render settings and range, exact Crump build, sample rate, reference environment, frame count, and hashes. Supporting FLAC or other delivery formats is part of the complete-track destination rather than the initial proof.

The First Public Release renders from Song time zero through the arrangement end plus an explicit non-negative Render Tail stored in readable Project settings. Crump displays the resulting Render Range before rendering and does not use hidden silence detection or a global hard-coded tail.

Offline rendering must preserve timing, automation, routing, deterministic random behavior, and plugin delay handling to the extent promised by supported plugins.

MIDI export and conventional DAW project interchange are deferred.

## 15. Reliability and Real-Time Safety

Audio continuity is a core product requirement. The application must:

- Keep the last valid graph playing through source errors.
- Prevent UI rendering, file access, parsing, compilation, and logging from blocking the real-time audio path.
- Apply graph changes without unsafe memory access or uncontrolled output.
- Recover or isolate affected components when feasible instead of crashing the entire application.
- Report audio underruns, device failures, extension failures, and plugin failures clearly.
- Include output limiting or equivalent safety protection during sound design and graph editing.

The first release should prioritize predictable, robust behavior over supporting every plugin or DSP feature.

Professional-grade performance means meeting published budgets on named hardware and workloads, not merely feeling fast in light use. The release must measure callback deadline utilization, xruns, graph-publication latency, interaction latency, startup, and rendering against the reference project. Real-time processing must use bounded preallocated work and must not perform file or console I/O, parsing, graph construction, blocking synchronization, or other unbounded work on the audio callback.

## 16. Platform and Technology Constraints

- The implementation language is Rust.
- The primary interface is a terminal UI built with Ratatui.
- Linux is the only officially supported platform for the first public release.
- The architecture must not unnecessarily prevent later macOS and Windows support.
- The first release may select a constrained set of Linux audio backends and distributions, but supported configurations must be documented.
- The audio engine runs locally and must not require a web browser or hosted service.

## 17. Explicit Non-Goals for the First Release

- Live microphone or line-input recording, takes, comping, and punch workflows.
- Replacing every workflow in Ableton Live or FL Studio.
- Built-in music-theory lessons or a guided curriculum.
- Ableton-style session scenes and live clip launching.
- Advanced destructive waveform editing or audio repair before prototyping validates the approach.
- A graphical desktop interface outside the terminal.
- Official macOS or Windows support.
- MPE, OSC, and comprehensive external synchronization.
- MIDI capture, timing correction, controller mapping, persistence, and synchronization beyond transient Note Audition.
- MIDI export and DAW project interchange.
- Guaranteed compatibility with arbitrary VST3 plugins.
- Third-party plugin hosting, including CLAP.
- A Rust extension API or agent-generated native code inside ordinary projects.
- An embedded Authoring Agent terminal or PTY inside the Ratatui workspace.
- A complete professional mixer with Groups, Sends, Returns, arbitrary Buses, and stem export.
- A polished 3-5 minute complete-track acceptance gate.
- Intelligible or realistic singing synthesis.
- Realistic acoustic-instrument or orchestral emulation.
- Compressors, detailed EQ, mastering tools, sidechains, and advanced effects routing.
- Structured graph-topology editing in the TUI.

## 18. First Public Release

The first public release succeeds when a Beginner Musician with no production experience but basic Linux terminal capability can complete this end-to-end workflow on a supported system:

1. Start with a blank Project and connect a supported external Authoring Agent harness.
2. Give the Agent an everyday-language Creative Brief, including a Sound Brief for a pitched polyphonic instrument, and ask it to create an explained 30-60 second Musical Sketch with stock synthesized percussion and that instrument expressed as a Composable DSP Graph, without samples.
3. Hear the valid result and see clear diagnostics while the Agent iterates on invalid source.
4. Listen and give one ordinary-language sound revision through the Human Listening Loop.
5. Inspect the arrangement and instrument through Production Views, with unfamiliar concepts explained in context.
6. Loop an Audition Phrase, change one concrete note or rhythm through its Event Grid and one meaningful Instrument Faceplate control, hear both changes, and inspect the same instrument's Graph View.
7. Inspect what an Agent Change changed and why, compare it with the preceding Project History Entry, and perform an exact History Restore.
8. Save the generated Instrument Module to a personal library and instantiate it from source in another Project context.
9. Save, close, and reopen the Project without changing its authored or audible result.
10. Render the result as a lossless master.

Third-party plugin hosting is outside this release. The end-to-end workflow is entirely native.

## 19. Success Measures

Before calling the first public release complete:

- At least one representative beginner must complete the documented blank-to-render workflow without developer intervention or prior knowledge of Crump's DSL.
- At least one independent Experienced Producer must inspect and modify the same Reference Project and judge its terminology, transferable workflow, Instrument Faceplate, graph depth, and audio result musically credible.
- The Beginner Musician must audition through the looping phrase without MIDI hardware; the Experienced Producer must play the graph instrument through transient MIDI note-on/off on the documented setup.
- The beginner must be able to begin with an everyday-language Creative Brief rather than production terminology, while the Agent's resulting change explains its musical and production choices.
- The beginner must listen and request at least one ordinary-language sound revision; the Agent is not required to analyze audio.
- The resulting Reference Project must contain a 30-60 second Musical Sketch with a stock synthesized-percussion role, a pitched role using an Agent-authored polyphonic Composable DSP Graph instrument derived from the brief, and more than one section or an audible variation, without imported or model-rendered waveform assets.
- The Reference Project requires only simple level balance and a lossless master; professional mixing and mastering are not part of this gate.
- The Agent-authored Instrument Module must be saved to a personal library and instantiated from its human-readable source in another Project context.
- The beginner must make and explain an audible Event Grid change and Instrument Faceplate change, inspect the same instrument's Graph View, inspect an Agent Change, and use Project History to compare and restore states.
- Invalid edits during playback must leave the last valid project audible and recover automatically after correction.
- Repeated renders of the unchanged Reference Project must meet the documented deterministic comparison rule.
- Saving and reopening must reproduce the Project's authored structure and audible result.
- Routine editing and visualization must not cause audible dropouts on the documented reference hardware and audio configuration.

Neither human evaluator may be a Crump implementer or the prior author of the Reference Project. Accessibility and professional credibility are recorded separately; success by one audience does not substitute for failure with the other.

Specific latency, project-size, CPU, and startup-time budgets must be set after an audio-engine prototype establishes realistic baselines.

## 20. Required Discovery Work

The following decisions are deliberately unresolved and must be tested before the product contract becomes more specific:

Each discovery item must first record a dated professional baseline from primary web sources. Search official Ableton documentation first where applicable, compare other professional DAWs or governing standards, distinguish observed behavior from proposed Crump requirements, and justify any added complexity against the reference workflow.

### 20.1 Production-view depth

Prototype progressively deeper views over the same arrangement and instrument objects. Evaluate whether a beginner can make and explain an audible change without editing source while an experienced producer can reach exact parameters, graphs, and source without changing project models.

### 20.2 Structured source editing

Prove that Production View operations can rewrite DSL source while preserving comments, stable formatting, user-defined expressions, and Project History. This is foundational rather than an optional convenience.

### 20.3 Incremental audio graph updates

Prove that valid code changes can be parsed, validated, compiled, and swapped into playback without blocking the audio thread or interrupting unaffected sound.

### 20.4 External Authoring Agent contract

Prototype the external harness contract against one documented agent configuration. Verify that authoritative guidance plus machine-readable inspection, validation, checks, Project Publication, and rendering let an Agent create and repair a graph-authored instrument without scraping the TUI.

### 20.5 Performance budgets

Use the Reference Project's synthesis, arrangement, history, and visualization workload to define supported project scale, acceptable interaction latency, audio latency, and offline render performance.

### 20.6 Rendering proof

Define the Musical Sketch's render range, effect-tail policy, lossless output format, and deterministic comparison domain before freezing the Reference Project Acceptance Gate.

### 20.7 Complete-track production baseline

Maintain the existing professional-production research as guidance for the destination beyond the First Public Release. Promote routing, metering, automation, latency compensation, portability, and advanced rendering requirements only when a validated workflow needs them; competitor breadth does not enter the initial proof by default.

## 21. Deferred Product Questions

- Whether generative and ambient composition should receive specialized language primitives beyond the genre-neutral pattern system.
- Whether waveform, spectrum, or sample-editing work is selected by evidence as a future Validated Slice.
- Whether source-backed synthetic voice or vocal-like instruments merit research; intelligible or realistic singing is not currently promised.
- Whether a scene/clip-launching performance mode should follow the timeline release.
- How much traditional music notation or piano-roll editing belongs in a terminal-native workflow.
- Whether Rust extensions run in-process, out-of-process, or through more than one trust tier.
- How projects declare and obtain third-party packages, samples, extensions, and presets.
- Which compatibility guarantees apply across DSL and engine versions.
- Whether collaboration features should extend beyond Git-friendly project files.
