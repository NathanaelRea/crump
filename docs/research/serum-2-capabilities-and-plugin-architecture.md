# Serum 2 Capabilities and Plug-in Architecture

- **Retrieved:** 2026-08-21
- **Serum version priority:** Serum 2; Serum 1 is mentioned only where Xfer makes a direct comparison
- **Evidence policy:** Xfer Records primary sources only. Inferences and source limits are labeled explicitly.

## Summary

Serum 2 is not merely a larger Serum 1 wavetable synth. Xfer documents three independently selectable primary oscillators spanning wavetable, sample, multisample, granular, and spectral engines; dedicated sub and noise sources; dual filters; a mixer and multi-bus effect graph; substantially expanded modulation; and internal clip and arpeggiator modules. Its product identity also depends on deep content editing, visual feedback, preset/content management, and host integration, not only oscillator DSP.


## Source scope and confidence

The official sources available for this pass were:

1. Xfer's current [Serum 2 product page](https://xferrecords.com/products/serum-2), including its product sections and system requirements.
2. Xfer's [*What's New in Serum 2*](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf), identified inside the document as Serum 2 version 2.0, manual version 1.0.0, dated March 17, 2025 (p. 1).

The second source calls itself a “What's New” document and the product page calls it a “highlight reel,” so it is not treated as a complete reference manual. No full in-product manual, release-note history, authenticated support articles, or official plug-in-format specifications were available in the supplied source set. Accordingly, exact parameter inventories, limits, timing semantics, state serialization, host event behavior, and DSP-quality internals are not filled in from memory or secondary sources.

Confidence labels used below:

- **Verified:** explicitly stated or labeled in an available Xfer source.
- **Partial:** the broad capability is explicit, but requested details are not documented by these sources.
- **Not established:** absent or too ambiguous to claim.

## Evidence-backed Serum capability map

### 1. Sound generation and oscillator routing

| Capability | Serum 2 evidence | Confidence / Serum 1 distinction |
| --- | --- | --- |
| Primary sources | Serum 2 has **three primary oscillators**, each selectable as Wavetable, Multisample, Sample, Granular, or Spectral. [Xfer, *What's New*, pp. 4-5](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=4) | **Verified.** Xfer explicitly contrasts this with **two** primary oscillators in Serum 1 on p. 4. |
| Additional sources | The instrument retains a dedicated **sub oscillator** and dedicated **noise oscillator**. [Xfer, *What's New*, pp. 3-4](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=3) | **Verified.** Xfer says the sub is similar to Serum 1 and the noise oscillator is “as in Serum 1” (p. 4). |
| Wavetable engine | Wavetable remains central; Serum 2 adds smooth wavetable-position interpolation, tuning modes, oscillator copy/paste, routing to multiple targets, phase memory, additional warp modes, and two simultaneous warps. [Xfer, *What's New*, p. 6](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=6) | **Verified** at feature level. The exact tuning values, routing graph, and warp inventory are not enumerated. |
| Sample engine | Sample mode provides playback/loop modes, modulatable start/end and loop points, loop crossfade, FM/PM warps, automatic/manual slicing, and forward-reverse looping tails for real-time stretching. [Xfer, *What's New*, p. 7](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=7) The product page additionally names snap loop detection, loop modulation, rate control, score extraction/playback, and tails mode. [Xfer, “Sample Oscillator”](https://xferrecords.com/products/serum-2) | **Verified.** Granular and Spectral share some sample start/end, looping, and slicing functions (pp. 7-8), but exact parity is not specified. |
| Multisample engine | Multisample mode supports loading **SFZ** files, a customizable envelope, timbre shifting, and warping. [Xfer, *What's New*, p. 8](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=8) Xfer describes SFZ as open-standard and human-readable and says Serum 2 includes recorded instruments such as orchestra, choir, pianos, and guitars. [Xfer, “Multisample Oscillator”](https://xferrecords.com/products/serum-2) | **Verified** for support and broad content categories. No SFZ opcode/conformance profile is given. |
| Granular engine | Granular mode recombines grains from samples and supports up to **256 grains**, grain-window amount, warping, and additional granular parameters. [Xfer, *What's New*, pp. 5, 8](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=5) | **Verified** at this level; scheduling, interpolation, stereo, and quality behavior are not documented. |
| Spectral engine | Spectral mode analyzes and resynthesizes a sample's frequency spectrum with independent time/pitch control; highlighted controls include scan rate/direction and high/low frequency bounds. [Xfer, *What's New*, pp. 5, 8](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=5) Xfer also describes harmonic-level resynthesis and transient-detection processing. [Xfer, “Spectral Oscillator”](https://xferrecords.com/products/serum-2) | **Verified** in broad terms; analysis/resynthesis algorithm and transient behavior are unspecified. |
| Source/mixer routing | The mixer exposes oscillator, filter, FX-bus, and main/direct-output channels with level/pan and routing controls; it can balance routing to filters and FX buses. [Xfer, *What's New*, p. 15](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=15) | **Verified** at UI/capability level. The full graph grammar, feedback rules, and summing order are not established. |

### 2. Wavetable authoring, morph, and warp

Xfer presents the ability to **create, import, edit, morph, and manipulate wavetables in real time** as a defining Serum workflow. [Xfer, “Why Serum?”](https://xferrecords.com/products/serum-2) For Serum 2 specifically, smooth interpolation permits near-continuous frame positions without first creating morph tables; the wavetable oscillator offers filtering, FM, phase distortion, ring modulation, distortions, new warp modes, and dual simultaneous warps. [Xfer, “Wavetable Oscillator”](https://xferrecords.com/products/serum-2) The What's New document also identifies true FM, filter-derived phase distortion, distortions, and dual warp. [Xfer, *What's New*, p. 6](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=6)

This verifies the overall authoring and playback-manipulation surface, but **does not establish** the editor's frame limits, import formats, FFT tools, formulas, morph algorithms, normalization, export behavior, or complete warp list. The product-page sentence begins with Serum's original motivation, so it should not be read as evidence that every editor operation is new in Serum 2.

### 3. Unison, voicing, and per-note phase

Serum 2 has “enhanced unison configuration,” and its wavetable panel controls phase and phase randomization for new notes. [Xfer, *What's New*, p. 6](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=6) A voicing area also exposes legato and portamento settings. [Xfer, *What's New*, p. 3](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=3)

The available official sources **do not establish** the requested exact unison voice-count range or the presence/ranges/semantics of detune, blend, stereo width, per-unison phase, or randomization controls. Those details require the full manual or verified in-product documentation. It would be unsafe to project familiar Serum 1 controls onto Serum 2 from these sources.

### 4. Filters and routing

Serum 2 has **two filter modules**, usable singly or together, with **series or parallel** routing, new virtual-analog and other filter types, direct cutoff/resonance manipulation, and a clean drive mode. [Xfer, *What's New*, pp. 2, 9](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=9) The mixer provides filter channels with routing, pan, mix, level, and graphical cutoff/resonance control. [Xfer, *What's New*, p. 15](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=15)

The source set does not provide a complete filter-type list, slopes, nonlinear models, oversampling behavior, self-oscillation behavior, filter-to-filter modulation semantics, or an exhaustive routing truth table.

### 5. Envelopes and LFOs

- Serum 2 expands to **four envelopes**. Envelopes can follow host tempo through a BPM option, and Invert Legato can force triggering at note-on even when legato is enabled. [Xfer, *What's New*, pp. 2, 13](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=13)
- Serum 2 provides up to **10 LFOs**; LFOs 7-10 appear after LFO 6 is assigned. Documented additions include rates up to **1000 Hz**, swing following, Lorenz and Rössler chaos modes, directional playback, a dedicated editor/drawing tools, presets, independent X/Y grids, and modulatable phase. [Xfer, *What's New*, p. 13](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=13)

An envelope **hold stage is not identified** in either available source. Nor do these sources define envelope segment shapes, loop modes, trigger modes beyond Invert Legato, LFO synchronization units, polarity, smoothing, retrigger rules, or control-rate versus audio-rate execution.

### 6. Modulation matrix, macros, sources, and destinations

Serum 2 has **eight macros**, and each macro can itself be a modulation destination; Xfer also documents applying/deleting a macro. Any oscillator or filter can be used as a modulation source. [Xfer, *What's New*, p. 13](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=13) The expanded modulation matrix adds editable source and auxiliary-source scale curves, dynamic modulation visualization, route reorder, per-route bypass, and route removal. [Xfer, *What's New*, p. 14](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=14) Xfer says envelopes can modulate “almost any parameter.” [Xfer, *What's New*, p. 3](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=3)

The word “almost” matters: the available sources do not supply exhaustive source/destination lists, routing limits, modulation combination order, polarity, per-voice/global scope, cycle policy, update rate, or exact meaning of oscillator/filter-as-source. Macro application/deletion semantics are also not defined.

### 7. Velocity, keyboard tracking, note expression, and MPE

The interface labels a “Velocity/Note” modulation area, but only calls it “more modulation options.” [Xfer, *What's New*, p. 3](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=3) The oscillator-mapping editor can constrain oscillator and arpeggiator response by **note and velocity ranges**; keyboard transpose, key/scale processing, and swing are also documented. [Xfer, *What's New*, p. 18](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=18)

This is enough to verify velocity/note-range mapping, but not a complete velocity modulation source, generic key tracking, release velocity, channel pressure, polyphonic pressure, per-note pitch bend, timbre, MIDI 2.0 note expression, or **MPE**. No MPE claim should be made from this source set.

### 8. Effects

Serum 2 provides **two FX buses**, an expanded rack, **13 effects**, and three splitter modules. Modules may be reordered, bypassed, instantiated more than once, directly manipulated, and loaded/saved through rack or module presets. [Xfer, *What's New*, p. 10](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=10) The named new/enhanced modules are Bode frequency shifter, Convolve, Delay with HQ mode now default, Distortion with Overdrive and DC bias, Reverb with Vintage/Nitrous/Basin types, and Utility. [Xfer, *What's New*, p. 11](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=11) Splitters divide low/high, low/mid/high, or mid/side bands. [Xfer, *What's New*, p. 12](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=12)

The 13-effect **complete list is not present** in the available sources. “High Quality FX” is marketing-level labeling on p. 10, not evidence of a universal oversampling mode or a measurable quality contract.

### 9. Clip sequencing and arpeggiation

Serum 2 includes both rather than relying entirely on the host:

- The **clip sequencer** has a flexible grid and piano-roll editor, record modes (Overdub and Extend), loop/offset markers, multiple automation lanes, key/scale/transpose and oscillator mapping, 12 slots per clip bank, factory/user clip banks, and MIDI output selection. [Xfer, *What's New*, p. 16](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=16)
- The **arpeggiator** has pattern editing plus transpose shift/range, offset, repeats, gate, chance, retrigger, velocity-over-time, 12 slots per arp bank, factory/user banks, and MIDI output selection. [Xfer, *What's New*, p. 17](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=17)

The sources do not define event ordering, sample accuracy, host-loop behavior, transport/chase semantics, MIDI protocol/version, or how generated MIDI is exposed in each plug-in format.

### 10. Presets, content, editing workflow, and visualization

- Xfer states the product includes **over 626 presets and 288 wavetables**. [Xfer, “Specs”](https://xferrecords.com/products/serum-2#system-requirements)
- The browser covers factory content, artist packs, and user patches; folder hierarchy, search, categories/tags, ratings/filtering, editable metadata, pack creation from a folder, database rescan, preset hybridization, auto-play, custom preview clips, and macro adjustment during preview are shown. [Xfer, *What's New*, p. 19](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=19)
- Oscillators can be copied/pasted with or without modulation, and Serum 2 exposes comprehensive undo/redo. [Xfer, *What's New*, pp. 2, 6](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=6)
- Visual interaction is pervasive: direct filter-curve and effect-display manipulation, graphical mixer/filter controls, dynamic matrix activity, wavetable/sample views, LFO drawing/editing, piano roll, mapping editor, and browser preview workflow are all explicitly pictured and labeled. [Xfer, *What's New*, pp. 6-19](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=6)

The content counts are stated as “Serum” counts on the Serum 2 page; the page does not disclose count methodology or versioned content-manifest identity. Preset file format, state portability, missing-asset behavior, asset embedding, undo granularity, and content licensing details are not established here.

### 11. Oversampling and quality modes

The product page calls out a “truly high-quality sound,” and the effects overview labels the effects high quality; Delay has an HQ mode that is now the default. [Xfer, “Why Serum?”](https://xferrecords.com/products/serum-2) [Xfer, *What's New*, pp. 10-11](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=10)

Those statements do **not** establish oscillator oversampling factors, global draft/render quality modes, per-module oversampling, sample-rate dependence, anti-aliasing strategy, latency, or offline-versus-realtime behavior. No Serum 2 oversampling architecture can be responsibly specified from this evidence.

### 12. Host and plug-in delivery

Serum 2 is an instrument plug-in for Microsoft Windows and Apple macOS, delivered as **64-bit VST3, Audio Unit, and AAX**, and requires compatible host software. Xfer lists Windows 10+; on macOS it lists High Sierra or later for Intel and macOS 11 or later for Apple Silicon. [Xfer, *What's New*, p. 1](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf#page=1) [Xfer, “Specs / System Requirements”](https://xferrecords.com/products/serum-2#system-requirements)

No Linux build, standalone application, CLAP build, LV2 build, or 32-bit build is listed. This is an observation about Xfer's stated delivery matrix, not proof that other environments can never run it. The available source set includes no official VST3, AU, or AAX specification, so ABI, process, state, automation, note-expression, and MIDI-output details remain outside this report.

## Source gaps

The following requested details could not be answered confidently from the available official material:

- Exact unison voice-count limit and detune/blend/width/phase/randomization semantics.
- Whether envelopes have a hold stage and all envelope modes/curves.
- Complete LFO modes, sync divisions, trigger/scope, and execution rate.
- Exhaustive modulation source/destination list, route limits, scope, order, curves, and cycle handling.
- Full velocity/key-tracking implementation and any MPE, note-expression, MIDI 2.0, pressure, or timbre support.
- Complete filter and effect inventories and parameter details.
- Wavetable editor limits, import/export formats, morph operations, and full warp inventory.
- SFZ conformance subset and multisample asset/path behavior.
- Oversampling factors, quality modes, latency, anti-aliasing, and offline/realtime differences.
- Preset/state format, asset portability, content manifests, and missing-content behavior.
- Plug-in state, automation, generated-MIDI, note-expression, bus, and host compatibility semantics.
- Change history after the version 2.0 / March 17, 2025 What's New document; the retrieved product page exposed a 2.1.5 demo URL, but a URL slug alone is not release-note evidence and is not used to infer features.

## Sources

### Kept

- [Xfer Records — Serum 2 product page](https://xferrecords.com/products/serum-2) — current official overview, oscillator descriptions, content counts, formats, and system requirements; retrieved 2026-08-21.
- [Xfer Records — *What's New in Serum 2*](https://static.xferrecords.com/Serum%202%20What%27s%20New.pdf) — official feature overview, Serum 2 v2.0 / manual v1.0.0 / March 17, 2025; retrieved 2026-08-21.

### Dropped or unavailable

- Secondary reviews, videos, retailer pages, community posts, preset-vendor summaries, and search snippets — excluded by the primary-source-only requirement.
- Serum 1 manuals and unofficial Serum 2 manual mirrors — not needed for the few explicit Serum 1 comparisons and not used to fill Serum 2 gaps.
- Full Serum 2 reference manual and official release-note/support corpus — not present in the supplied source set; the available PDF is explicitly a highlight document.
- Official VST3, Audio Unit, and AAX specifications — not present in the supplied source set, so no format-level architectural claims are made.
