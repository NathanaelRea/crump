# First Public Release

Status: Draft release interpretation

This document organizes the release promise in the [product contract](../../requirements/product-contract.md). It does not add capabilities to that contract.

## Native Baseline

The release must let a Beginner Musician with no production, synthesis, or DSL experience, but basic Linux terminal capability, complete one narrow but real creation loop:

1. Start from a blank Project and connect a supported external Authoring Agent harness.
2. Give the Agent an everyday-language Creative Brief, including a Sound Brief for a pitched polyphonic instrument, and ask it to create an explained 30-60 second Musical Sketch with stock synthesized percussion and that instrument expressed as a Composable DSP Graph, without samples.
3. Hear valid results while invalid source leaves the last Active Revision audible and gives the Agent actionable diagnostics.
4. Listen and give one ordinary-language sound revision through the Human Listening Loop.
5. Inspect the arrangement through a clip Timeline and Event Grid, and inspect the instrument through progressively deeper Production Views.
6. Loop an Audition Phrase, make and understand one audible Event Grid change and one Instrument Faceplate change, then inspect the same instrument's Graph View.
7. Inspect an Agent Change, compare Project History states, and perform an exact History Restore.
8. Save the generated Instrument Module to a personal library and instantiate it from source in another Project context.
9. Save, close, reopen, and reproduce the Project.
10. Render a lossless master.

This is an integrated product proof, not yet a professional production baseline or complete-track release. It validates Crump's distinctive agent-authored synthesis and progressive human-control loop before DAW breadth is allowed onto the critical path.

The Musical Sketch belongs to electronic or synthesized music. Realistic acoustic, orchestral, or vocal emulation is not part of the quality judgment.

## Professional Quality Bar

The release must demonstrate these properties under the reference workload:

- **Complete:** the blank-to-render workflow reaches a real lossless master without samples, third-party plugins, hidden manual repair, or developer intervention.
- **Fast:** callback deadlines, xruns, update latency, UI response, startup, and render throughput meet published budgets on named hardware; averages cannot hide worst-case failures.
- **Safe:** the audio callback performs bounded real-time-safe work, invalid edits preserve the active revision, dangerous or non-finite monitoring output is contained, and failures do not corrupt source.
- **Explainable:** the beginner can inspect the musical structure, instrument controls and graph, Agent Change provenance, Active Revision, and render result rather than relying on hidden state.
- **Recoverable:** saves and render publication are atomic, and interruption preserves the last known-good Project and completed artifacts.
- **Reproducible:** saving, reopening, playback, and rendering preserve the same authored musical result under a documented deterministic comparison rule.
- **Minimal:** every supported capability is justified by the reference workflow or one of these quality properties; competitor feature inventories do not define scope.

The evidence baseline for these properties is [Professional DAW Baseline for Crump](../research/professional-daw-baseline.md), whose competitor observations are informative destination guidance. The product contract defines capabilities, this document defines first-release interpretation and evidence, and accepted ADRs constrain implementation.

## Release Evidence

Release approval requires linked evidence for all of these categories:

| Evidence | Required proof |
| --- | --- |
| Beginner workflow | A representative terminal-capable Beginner Musician starts with an everyday-language Creative Brief and completes the documented workflow without prior production, synthesis, or DSL knowledge, developer intervention, or an undisclosed workaround. |
| Producer transfer | An independent Experienced Producer inspects and modifies the resulting Project and judges its terminology, workflow, Instrument Faceplate, graph depth, and audio result musically credible. |
| Instrument audition | The Beginner Musician uses an editable looping phrase without hardware; the Experienced Producer plays transient note-on/off through one documented MIDI setup without capture or mapping. |
| Reference Project | The resulting Project contains an explained 30-60 second Musical Sketch with a stock synthesized-percussion role, an Agent-authored pitched polyphonic graph instrument derived from the brief, and more than one section or an audible variation, without imported or model-rendered waveform assets. |
| Creative processing | Filtering, gain/pan, simple delay, simple reverb, and saturation shape the sketch; a separate non-authoritative monitoring safety stage after Master contains dangerous real-time output without affecting the render. |
| Progressive control | The beginner changes and explains one audible note or rhythm in an Event Grid and one Instrument Faceplate control, then inspects nodes, connections, Parameters, mappings, and source locations in the same instrument's Graph View without being forced to edit source. |
| Minimal feedback | The workspace shows playhead, Event and instrument activity, safe output levels, and graph or Parameter state without requiring waveform, spectrum, oscilloscope, Automation, or advanced routing views. |
| Agent contract | One documented harness/model configuration available at test time inspects, validates, runs checks, explicitly publishes each intended valid save with origin and explanation, renders, and repairs its work through machine-readable operations rather than TUI scraping; publication also works while the TUI is closed. |
| Human Listening Loop | The Beginner Musician listens and requests one ordinary-language sound revision; the supported Agent does not need audio-input capability. |
| Graph validation | Machine checks reject invalid topology, types, units, Parameter mappings, declared resource bounds, invalid deterministic contracts, and non-finite authored values; the monitoring stage separately contains loud but finite real-time output without claiming to judge musical quality. |
| Free core | The same Project can be authored, edited, played, saved, reopened, and rendered manually without a model, paid service, paid asset, or hosted backend. |
| Project history | Each successful Agent publication and human workspace action identifies its origin and compares exactly with the preceding state. History Restore appends the chosen earlier state as a new referencing entry without deleting or branching; invalid and unpublished external saves do not enter history; no entry expires or is compacted automatically. |
| Instrument reuse | The Agent-authored Instrument Module is saved to a personal library and instantiated from readable source in another Project context without a registry or compiled preset. |
| Source continuity | Invalid syntax and semantic edits never partially apply; the active revision remains audible and identifiable until a valid replacement is published. |
| Graph transition | A valid topology change may visibly fade and restart the affected instrument, but transport and unchanged Tracks continue without requiring voice or effect-state migration. |
| Round trip | Saving and reopening restores the authored musical structure, instrument graph, parameters, history, and audible result. |
| Rendering | The visible Render Range begins at Song time zero and ends after the arrangement plus the authored Render Tail. On the documented `amd64` environment, the same Project, settings, exact build, and sample rate produce byte-identical stereo 32-bit-float WAV and manifest files; no cross-build or cross-architecture identity is promised. |
| Real-time safety | The accepted reference workload meets callback, underrun, update-latency, and UI responsiveness budgets on documented hardware. |
| Keyboard completeness | The documented native workflow can be completed with a consistent command vocabulary, visible focus, exact value entry, and non-visual status equivalents. |
| Optional mouse | Supported terminals may use pointing for selection and navigation, but every action remains keyboard-complete and no precision drag is required. |
| Recovery | Fault-injected save, restart, device-loss, and render-interruption tests retain a complete known-good Project and never present partial output as successful. |

The exact Reference Project fixture, budgets, and verification procedures remain required discovery in the product contract and the workspace-local implementation-planning map. Issue 25 freezes them only after the governing first-release contracts resolve.

The Beginner Musician and Experienced Producer must be independent of Crump implementation and prior Reference Project authorship. Their outcomes are recorded separately; one cannot compensate for the other's failure.

## After This Release

The two human evaluations determine the largest obstacle to extending the same Project toward a finished track. The next release is the smallest end-to-end slice that removes that observed obstacle; the old professional-DAW backlog does not predetermine whether Automation, routing, stems, sampling, plugins, structured Graph View topology editing, or another capability comes next.

## Explicitly Outside This Release

- Live audio recording, takes, comping, and punch workflows.
- Session scenes and live clip launching.
- MIDI capture, timing correction, controller mapping, persistence, and synchronization beyond transient Note Audition.
- Professional mixer breadth including Groups, Sends, Returns, arbitrary Buses, and stem export.
- Compressors, detailed EQ, mastering tools, sidechains, and advanced effects routing.
- Structured graph-topology editing in the TUI; topology changes use source or an Agent.
- Waveform, oscilloscope, spectrum, Automation, and advanced routing visualizations.
- Advanced destructive waveform editing, repair, and time warping.
- A desktop GUI, browser runtime, or hosted service.
- Official macOS or Windows support.
- MPE, OSC, comprehensive external synchronization, MIDI export, and DAW project interchange.
- Guaranteed arbitrary VST3 compatibility.
- Third-party plugin hosting, including CLAP.
- A Rust extension API or agent-generated native code.
- An embedded Authoring Agent terminal or PTY inside Crump; the harness runs in a neighboring terminal or user-managed multiplexer pane.
- A polished 3-5 minute complete-track acceptance gate.
- Intelligible or realistic singing synthesis.
- Realistic acoustic-instrument or orchestral emulation.
- Built-in music-theory teaching.
- Collaboration beyond Git-friendly project files.
