# Freeze the First-Release Reference-Project Acceptance Contract

Type: grilling
Mode: HITL
Status: resolved
Blocked by: none

## Question

What exact reference project, user workflow, observable outcomes, and exclusion rules will prove that Crump can produce a complete track in its first public release?

## Answer

The first release is proved by one fixed, versioned **Reference Project** and a **Reference Project Acceptance Gate** that combines an exploratory human checklist with machine-verifiable checks on the same fixture.

### Reference Project

- A finished 3-5 minute, genre-neutral electronic track with at least three clearly different sections, an intentional transition, and simultaneous rhythmic, bass, melodic or harmonic, and texture roles.
- Its arrangement exercises reusable patterns or clips, timeline placement, repetition, transformation, pattern composition, notes, chords, rhythm, rests, velocity, duration, probability, deterministic randomness, and at least one tempo or meter change.
- Its native sound sources include a pitched sample instrument, a one-shot or loop sample role, a stock native synthesizer, and a reusable instrument authored as a composable DSP graph.
- Its routing exercises a group, a send and return, an explicit bus or submix, insert effects, automated gain or pan, mute and solo, metering, and protected master output. Sidechaining and resampling are not required by this contract.
- Filtering or EQ, dynamics, delay, reverb, saturation or distortion, and utility gain, pan, or routing each make an audible or measurable contribution; bypassed instances do not count.
- It contains point or lane automation, procedural automation, and modulation through an envelope, LFO, or equivalent source. Procedural automation remains identifiable and is not flattened by structured editing.
- It is a rich, multi-file, version-controlled source fixture containing comments, imports, reusable definitions, deterministic seeds, and at least one nontrivial user expression that unrelated structured edits must preserve.
- It renders a master in WAV and FLAC plus explicitly selected individual-track, group, and bus or return stems. Exact stem semantics, naming, effect-tail rules, and digital comparison thresholds are owned by downstream decisions.

### Human Checklist

At release-candidate milestones, at least one electronic musician who understands production concepts, is comfortable with text-based tools, did not author the Reference Project, and did not implement Crump performs an untimed exploratory check. They start from a clean copy of the completed project and use only shipped documentation; they need not recreate the track.

The checklist asks the musician to:

- Open, navigate, play, and self-report whether they understand the project and its source.
- Use the source editor, timeline, mixer and routing, instrument and parameter, automation, sample browser, waveform, meter or spectrum, diagnostics, and engine-status views for a meaningful inspection or edit; the oscilloscope need only be observed.
- Make one meaningful direct DSL edit and one meaningful structured TUI edit, hear each valid change, inspect the resulting source, use undo and redo, and confirm unrelated comments and expressions survive.
- Introduce both a syntax error and a semantically invalid routing or parameter edit during playback. The active revision remains audible, diagnostics identify the candidate revision and source location, no partial change applies, and correction publishes without restarting transport.
- Use a real supported MIDI controller to play a selected native instrument, capture a short phrase into human-readable source, map one continuous control and one pad or button, observe input and mapping feedback, and confirm notes and mappings survive reopen.
- Inspect and alter point automation, procedural automation, and modulation without flattening the procedural representation.
- Navigate and modify the project's group, send and return, bus, inserts, mixer automation, mute or solo, metering, and master-safety path.
- Save and collect or copy the project, reopen it in a clean supported machine or user environment, and confirm its authored state and audible result. On disposable copies, remove and alter assets and verify clear missing or changed-asset diagnostics without project corruption.
- Render the WAV and FLAC masters and selected track, group, and bus or return stems, then confirm no output is missing, clipped, truncated, or missing intended routing, automation, or effect tails.
- Judge whether the result sounds like a finished, intentionally arranged and mixed release demo rather than a feature montage.

The checklist is deliberately exploratory rather than a rigid task script or timed usability test. A promised capability that the musician cannot complete without developer intervention or an undisclosed workaround fails the gate. Data loss, project corruption, crashes, hangs, dangerous audio, and inability to determine which project revision is active also fail. Non-blocking confusion or friction is recorded for prioritization but does not alone fail the release.

### Machine-Verifiable Checks

Instrumented or automated checks on the same project establish repeated-render determinism, invalid-edit atomicity, save and reopen equivalence, callback and underrun behavior, update latency, and output safety. The governing runtime, persistence, rendering, Linux-baseline, and performance tickets define the exact procedures and thresholds; subjective listening does not substitute for them.

### Evidence And Exclusions

- Retain a dated checklist identifying the Crump build, Reference Project revision, supported system, audio and MIDI hardware, tester, pass or fail result for each area, observed workarounds and friction, and linked renders and logs. A session recording is optional.
- All sequencing, synthesis, effects, routing, mixing, mastering, source generation, and post-render correction used to satisfy the gate occur in Crump.
- Pre-existing sample assets and MIDI-controller firmware are allowed. External tools may inspect exported files but may not repair or transform them for acceptance.
- CLAP plugins and Rust extensions may be demonstrated separately but cannot satisfy any native acceptance item. The native gate remains valid if CLAP is excluded from the release.

## Comments

- Superseded on 2026-07-28 by the canonical product contract and First Public Release interpretation. The former sample-using 3-5 minute professional-track gate is historical; current acceptance is the 30-60 second no-sample Musical Sketch, external Authoring Agent workflow, Beginner Musician evaluation, Experienced Producer evaluation, and objective checks frozen by issue 25.
