# Credible Evidence for Unmet Needs in Modern Professional DAWs

Accessed: 2026-07-26

## Executive conclusion

The best-supported opportunity is not "a DAW with every feature, but in text." It is a narrower production environment whose authored state is portable, inspectable, deterministic, versionable, automatable, keyboard-complete, and recoverable. The evidence is strongest for project portability, accessibility gaps, reproducible save/load and render behavior, and reliability at plug-in and file boundaries. It is moderate for workflow scripting/customization and meaningful version history. It is weaker for broad claims that all professional DAWs are intrinsically too complex or that real-time cloud collaboration itself is a must-have.

A Linux-first entrant could plausibly address these needs without matching a mature DAW feature-for-feature by making them properties of the native project model: stable text serialization, explicit assets and dependencies, deterministic evaluation, command-addressable operations, semantic history/diffs, strong diagnostics, and a complete keyboard path. It should not promise universal DAW interchange, arbitrary plug-in reproducibility, automatic merge of simultaneous musical edits, or accessibility merely because it runs in a terminal.

## Scope and method

This report asks whether the named needs are evidenced broadly enough to matter and are plausibly addressable by a new Linux-first production tool. It uses:

- First-party vendor documentation and release notes where they explicitly disclose limitations, recovery procedures, or unsupported workflows.
- A vendor-maintained open interchange specification and its implementation list.
- Public issue records in the official LMMS and Helio repositories, favoring reproducible reports, maintainer/contributor-authored proposals, long-lived requests, and related records over isolated opinions.
- First-party descriptions of working counterexamples, such as REAPER scripting and Helio version control, as evidence that an approach is technically and product-wise plausible, not as proof that every user wants it.

Forum anecdotes, review sites, SEO surveys, and feature absence inferred from marketing were excluded. Tracker search counts are reported only as snapshots, not prevalence estimates: issue labels and titles are incomplete, duplicate policy varies, and one issue can represent one person or many. "Confidence" below evaluates the existence and breadth of the need, not the commercial size of a market.

## Evidence summary

| Need | Confidence | What the primary evidence establishes | Plausible first-release wedge |
| --- | --- | --- | --- |
| Interoperability and project portability | High | The DAWproject maintainers identify no prior purpose-built music-production exchange format; MIDI, AAF, presets, and stems each lose important structure. Six named DAW products now implement DAWproject 1.0. | Self-contained open project directories; explicit asset/dependency report; documented export boundary. |
| Accessibility and keyboard completeness | High | Ableton explicitly lists automation, modulation, MPE, Max for Live, some devices, and some third-party plug-ins as not screen-reader accessible; open Linux sequencers have unresolved visual and keyboard-only issues. | Keyboard-complete semantic UI, scalable text, high contrast, screen-reader testing, no mouse-only core operation. |
| Reproducibility and durable state | High for native state; medium for third-party plug-ins | Official manuals require collection/relinking of external files; LMMS has reproducible reports of changed plug-in sound after reload and nondeterministic XML serialization. | Canonical text, content-addressed assets, dependency manifests, deterministic native render tests, explicit degraded loading. |
| Reliability and recovery | High that the need persists; medium on comparative incidence | Current Ableton notes and LMMS records contain crashes and incorrect restore/render/playback classes, while Ableton documents dropout detection and restart for real-time renders. | Transactional saves, last-known-good playback, crash isolation, support bundles, offline validation/rendering. |
| Repetitive workflow automation | Medium-high | REAPER exposes actions and most of its API to text scripts; LMMS has an open scripting request for setup, sidechain, and riff workflows dating from 2015. | Stable command model plus batchable, undoable project transforms; avoid embedding a second UI toolkit initially. |
| Customization and user-authored processing | Medium | REAPER demonstrates scripts from macros through extensions; LMMS has separate long-lived requests for workflow scripting and per-song scriptable effects/controllers. | One composable native graph and a safe source-level extension seam, rather than unrestricted in-process code in release one. |
| Collaboration and versioning | Medium | Live's persistent version workflow is separate files and its undo history disappears on close; LMMS requests useful diffs/history; Helio ships integrated version control; Splice discontinued automatic session/revision sync after failing to sustain the desired quality. | Local semantic history, branches/checkpoints, diffable source, collect/validate bundle; remote sync can remain external. |
| Complexity and discoverability | Medium | A Helio tool maker explicitly cites DAW bloat as its product motivation; an LMMS contributor documents a hidden automation mode as undiscoverable; Ableton added a structured Learn View in 2026 to explain core concepts. | Progressive disclosure, searchable commands, inspectable relationships, diagnostics that name the next action. Do not equate minimal chrome with low complexity. |

## Findings by need

### 1. Complexity and discoverability

**Evidence**

- Helio's official repository explains the product's existence by saying most DAW interfaces "seem overcomplicated" and become more bloated, then positions a clean UI and integrated version control as the alternative. This is a first-party product thesis rather than independent market measurement, but it shows the problem has motivated a maintained cross-platform sequencer with about 3,500 GitHub stars as of access. [H1]
- LMMS issue #403 was opened by a contributor, assigned to project contributors, and remained open after eleven comments and implementation work. It calls global automation a "hardly-discoverable secret feature": after its editor closes, no timeline object indicates that automation exists, and it is reachable only from the controlled parameter's context menu. The proposed migration work converts hidden global automation to represented automation tracks. This is unusually concrete evidence that state can exist yet be operationally undiscoverable. [L1]
- Ableton's Live 12.4 release notes describe a new Learn View that replaces Help View with structured modules combining video and text, topic filters, completion tracking, and picture-in-picture tutorials specifically "to make it easier to understand and explore Live's core concepts." This is not an admission that Live as a whole is unusable, but it is direct vendor investment in discoverability after twelve major versions. [A1]

**Confidence: Medium.** Three first-party sources converge on discoverability as continuing work, but none is a representative survey and Helio's statement is partly positioning. The evidence supports hidden state and learning cost as real failure modes; it does not support "professionals want a simpler DAW" as a universal claim.

**Implications**

- Every durable musical relationship should be listable and searchable: automation targets, routes, assets, devices, mappings, and generated material must not exist only behind context menus.
- A command/search layer can expose breadth without permanent chrome, but source syntax alone is not discoverability. The tool needs contextual help, completion, validation, and named navigation targets.
- First-release scope should be reduced semantically, not merely visually. Fewer concepts with regular composition rules are more defensible than a sparse UI over hidden modes.

### 2. Repetitive workflow automation

**Evidence**

- Cockos documents ReaScript as text scripts that can invoke any REAPER action and most extension API functions, ranging from advanced macros to full-featured extensions. Scripts enter the Action List and can be bound to keys, MIDI/OSC, mouse input, or toolbar buttons. The API also provides project persistence and explicit undo blocks. This is strong proof that command-addressable, undo-aware automation belongs in a professional production workflow. [R1]
- LMMS enhancement #2221 has remained open since 2015 and had 15 comments plus activity in 2026. Its concrete examples are automating repeated setup, standard sidechaining, and a riff generator. It is a request in an official tracker, not proof of mass demand, but its longevity and specificity corroborate the same job in a Linux DAW. [L2]
- Live's manual shows templates can preconfigure I/O, devices, key mappings, and MIDI mappings. That reduces repeated setup, while the REAPER and LMMS evidence covers operations that vary per project and need programmatic composition rather than a fixed template. [A2]

**Confidence: Medium-high.** The capability is proven in a professional DAW and requested over many years in an open-source DAW. No source quantifies how many users write scripts themselves; a tool may serve the need through shareable recipes or agent-generated commands rather than expecting every musician to program.

**Implications**

- Build one stable, documented command/transaction model used by keyboard commands, structured UI, scripts, and agents. Each transform should support preview, validation, one-step undo, and non-interactive execution.
- Prioritize high-frequency structural work: create/rename/reorder tracks, connect routing, instantiate a reusable graph, transform patterns, set or generate automation, collect assets, validate, and render.
- Do not begin with unrestricted UI scripting or background daemons. REAPER's documentation warns scripts can crash the host and that deferred scripts must manage undo explicitly; those costs are avoidable in a constrained first release. [R1]

### 3. Interoperability and project portability

**Evidence**

- DAWproject's Bitwig-maintained specification states that no existing format was purpose-built to transfer music-production projects. It identifies concrete losses: Standard MIDI lacks audio, plug-in state, and high-level automation such as ramps; AAF lacks notes and musical time; shared plug-in presets require per-instance handling; stems flatten editable structure. Its goal is one vendor-neutral file containing audio, notes, automation, plug-in state, and surrounding project structure. [D1]
- DAWproject 1.0 is stable and, as of access, lists support in Bitwig Studio, Studio One, Cubase, Cubasis, VST Live, and n-Track Studio. Adoption by competing vendors is stronger evidence than a proposal alone that structured interchange addresses a cross-product need. [D1]
- LMMS issue #6909 requests DAWproject import/export and is assigned to a contributor. Separate open LMMS reports show ordinary MIDI round trips merging tracks, losing instrument/channel meaning, or omitting tempo changes. These records illustrate exactly the lower-level losses DAWproject's rationale predicts. [L3][L4][L5]
- Live's own file-management manual explains that sets refer to external samples, can go offline when those files move, require explicit Collect All and Save for transfer, and provide repair workflows for missing references. It also says moving files outside Live can disorganize a project. Portability therefore includes assets and references, not only the main document. [A2]

**Confidence: High.** A stable open specification, multi-vendor implementation, official DAW file-management procedures, and independent open tracker failures all converge. The remaining uncertainty is how much fidelity users require across unlike feature models, not whether portability is a real problem.

**Implications**

- Make Crump's native project an ordinary directory with human-readable authored files, relative or content-addressed media references, and a machine-readable dependency report. A one-command collect/verify/archive operation should fail loudly on missing or changed assets.
- Treat DAWproject as a future interoperability target and source of vocabulary, not as the native schema: its own non-goals exclude being a DAW-native format and session/view preferences. [D1]
- First release can honestly promise portable Crump-to-Crump projects and stems/MIDI where semantics permit. Full DAWproject import/export and third-party device translation should wait for conformance fixtures and explicit loss reports.

### 4. Reproducibility

**Evidence**

- Live's manual documents dependencies outside the set, missing-file silence, explicit collection, repair ambiguity when several candidate files match, and real-time rendering for external hardware. These are legitimate workflows, but they show that "the project file" alone is not sufficient input to reproduce output. [A2]
- LMMS bug #7104 gives reproducible steps where a Carla-hosted Vital instance sounds materially different after save, close, and reload because parameters are changed. It remains an open bug with six comments and is labeled as a regression. This directly violates the minimum expectation that persisted authored state survives a round trip. [L6]
- LMMS enhancement #8439 reports that reopening and saving an unchanged uncompressed XML project randomly reorders attributes, creating noisy Git diffs. The report identifies Qt's randomized hash ordering as the cause. Even when audio is unchanged, nondeterministic serialization obstructs review, provenance, and meaningful version history. [L7]
- DAWproject embeds plug-in state and permits embedded or referenced audio, but it cannot guarantee that another host has the same plug-in implementation or that a plug-in renders deterministically. The format improves capture and transport; it does not by itself prove sonic reproducibility. [D1]

**Confidence: High for native authored state and assets; medium for arbitrary plug-ins and hardware.** The failure modes are direct and reproducible. No reviewed source establishes bit-identical output as a broad DAW user requirement, so the practical claim should be "same authored musical result under a declared environment," with stronger tests for native devices.

**Implications**

- Canonical serialization is product behavior: stable ordering, explicit units/defaults, no timestamps or random IDs introduced on no-op save, and clean semantic diffs.
- Record engine version, sample hashes, project seed, device identities/versions, render settings, and unresolved external inputs. Offer `validate` and `render` without opening an interactive session.
- Gate native devices on save/reload equivalence and deterministic offline-render fixtures. Label external hardware and opaque plug-ins as non-hermetic dependencies rather than pretending to control them.

### 5. Collaboration and versioning

**Evidence**

- Live 12 now exposes an action-by-action Undo History, but the official manual says it is not saved when the set closes. For persistent versions, the documented workflow is to save differently named Live Sets in one project. Merging versions means importing complete tracks or clips, not reviewing and merging semantic edits. [A2]
- LMMS issue #8230 says its existing Save as New Version operation appends a running number and requests history, diffs, and a non-developer-friendly subset of Git. LMMS #8439 independently seeks deterministic serialization specifically to make external version control meaningful. [L8][L7]
- Helio's official README lists integrated version control as a core feature, demonstrating that local musical history can be made part of a sequencer rather than left entirely to filename conventions. Its open issue #13 asks to map that internal history to Git so a collaborator can synchronize via GitHub, exposing the next unmet seam: local history does not automatically provide exchange and merging. [H1][H2]
- Splice's CEO stated that its Studio collaboration feature automatically synchronized sessions and revisions, but Splice shut it down in 2023 after saying it had not been a focus since 2017 and could not provide a quality experience it was proud of. Splice called collaboration vital while directing users to generic file sharing. This is strong evidence that collaboration infrastructure is operationally difficult and service-dependent, but weak evidence about current demand magnitude. [S1]

**Confidence: Medium.** Multiple tools recognize versions and collaboration, but the evidence does not rank real-time co-editing above reliable handoff, review, and rollback. The shutdown cautions against making a cloud service the durable medium.

**Implications**

- Prioritize local-first history: named checkpoints, branchable experiments, semantic diffs, provenance, and exact rollback. Keep project files independently usable with standard Git and file-sync tools.
- A first collaboration feature should be a verifiable handoff bundle plus readable diff/review, not simultaneous editing. Audio assets need hash-aware transfer; authored text needs stable IDs and deterministic formatting.
- Do not claim automatic merge until conflicts in timeline placement, routing, IDs, and procedural expressions have explicit semantics and adversarial tests.

### 6. Accessibility

**Evidence**

- Ableton states that screen-reader support arrived in Live 12 and lists current exclusions: browser tagging, automation editing, Clip View modulation, MPE, Max for Live, some third-party plug-ins, and advanced parts of some native devices. Its detailed overview adds that Tab navigation is not yet universal, integrated MIDI controllers are not well integrated with screen readers, and automation and fades are unsupported in Arrangement View. These are first-party, current, feature-level limitations in a major professional DAW. [A3][A4]
- Ableton asks accessibility users to email feedback or join testing, indicating an active direct-user research channel. Its release notes continue to fix screen-reader announcements, focus, and keyboard navigation, showing accessibility is an ongoing engineering surface rather than a one-time checkbox. [A1][A3]
- LMMS has four open issues carrying its `accessibility` label in the GitHub API snapshot. One contributor-filed bug (#6047) describes insufficient selected/unselected contrast and remained open with 13 comments; #7092 reports unreadable, non-scaling fonts from a sight-impaired user's perspective. Helio issue #340 requests keyboard-only note selection because editing still requires a mouse. [Q2][L9][L10][H3]

**Confidence: High.** The strongest source explicitly enumerates inaccessible professional workflows, and two Linux sequencers corroborate visual scaling, contrast, and mouse-dependence gaps. Tracker label counts understate unlabelled issues and are not population estimates.

**Implications**

- Make the complete reference workflow keyboard-operable, including selection, automation, routing, graph editing, diagnostics, and rendering. Test it without a pointer, not merely with keyboard shortcuts added to common actions.
- A terminal/TUI can inherit semantic text and OS accessibility benefits, but custom-drawn terminal widgets can still be opaque. Provide stable labels, focus order, non-color state cues, scalable text, high contrast, and screen-reader acceptance tests on the supported Linux stack.
- Third-party plug-in GUIs are outside full control. Native instruments/effects should have host-provided semantic controls so the complete native workflow remains accessible even when a plug-in editor is not.

### 7. Reliability and recovery

**Evidence**

- The LMMS GitHub API snapshot returned 444 open issues labeled `bug` and 38 open issues with `crash` in the title. These are triage/workload indicators, not rates: they include old and development-version reports, and title search misses crashes described only in bodies. [Q1][Q3]
- Specific current records establish consequential classes rather than mere volume: LMMS #7104 changes sound after reload; #8014 plays automation correctly in the editor but omits it from exports above 48 kHz; Helio #352 reports a VST crashing the host while the VST3 exposes no audio output. [L6][L11][H4]
- Ableton Live 12.4.3 release notes include fixes for crashes during plug-in scanning, set recovery, undo, time-signature editing, and plug-in/API interactions, plus incorrect warp-marker restoration and automation/playback behavior. The same notes document corruption repair and a case where a screen-reader announcement fix was reverted because it introduced a crash. Release notes prove these failure classes reach shipped software; they do not establish comparative defect rates. [A1]
- Live's manual detects dropouts during real-time external-device renders and can restart from the beginning. This is useful recovery, but it also establishes that render correctness must be monitored rather than assumed. [A2]

**Confidence: High that reliability remains an unmet need; medium on breadth and relative severity.** Official fixes and reproducible open bugs are direct evidence. Raw issue counts cannot compare products or show how often typical users encounter a defect.

**Implications**

- Keep the audio callback isolated from parsing, persistence, UI, and extension failure. Preserve a last-known-good active revision when an edit is invalid.
- Use atomic saves, checksummed assets, autosave/recovery journals, bounded crash isolation for optional plug-ins, and diagnostics that preserve the user's project. Rendering should detect non-finite output, dropouts where applicable, missing dependencies, and divergence from the active revision.
- Publish a narrow Linux reference configuration and support bundle. Reliability is more credible as measured behavior on named hardware/software than as a generic "stable on Linux" claim.

### 8. Customization and user-authored behavior

**Evidence**

- REAPER's official scripting surface supports three languages, most host API functions, custom/dockable UI, background scripts, persistent project data, and explicit undo. It demonstrates a very high customization ceiling, while its own documentation also describes dependency, API-version, error-handling, undo, and crash risks. [R1]
- LMMS has distinct long-lived open requests for workflow scripting (#2221) and script-based per-song effects/controllers (#551, opened in 2014 with 33 comments). The latter's motivation is avoiding the compile/register/release/cross-platform burden of creating a native plug-in for a one-song idea. [L2][L12]
- Helio's product thesis seeks clean UI while offering integrated history and cross-platform portability. Combined with REAPER's breadth, this frames the credible opportunity: deep customization need not require exposing every mechanism as permanent interface chrome, but the evidence does not prove that source code is the preferred interface for most musicians. [H1][R1]

**Confidence: Medium.** The need is persistent and a professional implementation exists, but the sources skew toward technically inclined users. "Users want to author DSP in text" remains a hypothesis requiring musician tests.

**Implications**

- Separate safe musical composition from advanced DSP extension. First release can provide reusable typed native graphs and deterministic procedural control without promising arbitrary native code.
- Keep extensions inspectable and project-local where possible. Declare inputs, outputs, parameters, state, latency, and determinism; validate before publication to the audio engine.
- Test whether target musicians can modify and reuse a graph with structured assistance. Do not use REAPER's API size as a parity target.

## Cross-cutting product implications

1. **Lead with durable state, not code as identity.** Human-readable canonical source is valuable because it enables inspection, automation, portability, and history. It is not evidence that musicians want a general-purpose programming environment.
2. **Make native completeness the wedge.** A small set of native instruments, effects, routing, automation, and rendering can satisfy stronger accessibility and reproducibility claims than an early dependency on arbitrary plug-ins.
3. **Use one semantic command layer.** Keyboard UI, scripts, agents, history, and tests should invoke the same validated transactions. This addresses repetitive work without creating separate automation semantics.
4. **Treat dependencies as authored facts.** Samples, engine version, seeds, device state, and render settings belong in validation and provenance. Missing or changed inputs should never silently become a different song.
5. **Prefer local-first collaboration.** Canonical text, stable IDs, semantic diffs, checkpoints, and collect/verify bundles are tractable. Hosted sync and concurrent merge are separate products with substantial operational and semantic risk.
6. **Define accessibility by task completion.** The reference project must be editable, mixed, automated, validated, and rendered by keyboard with meaningful screen-reader output; a text UI alone is insufficient evidence.
7. **Make reliability visible.** Last-known-good audio, atomic persistence, explicit degraded states, deterministic offline checks, and support telemetry are product capabilities, not only implementation quality.

## What the evidence does not justify

- It does not show that a terminal DAW has broad demand, that users prefer a DSL to direct manipulation, or that agent-assisted editing is trusted for musical work.
- It does not show that real-time multi-user editing belongs in a first release.
- It does not make full-fidelity interchange between unlike DAWs possible; DAWproject explicitly leaves flattening and conversion to importers.
- It does not make arbitrary plug-ins deterministic, accessible, portable, or safe merely by storing their opaque state.
- It does not support comparing DAW reliability by public issue counts; commercial trackers are often private and open-source projects differ in age, release policy, and triage.
- It does not show that deep customization should be enabled by unrestricted in-process scripting. A typed graph or constrained transaction language may satisfy the evidenced jobs with a smaller failure surface.

## Recommended validation before product commitment

1. Give 6-10 target electronic musicians the same relocation, alternate-version, repetitive-edit, and recovery tasks in their current DAW; measure completion, workarounds, and lost information rather than asking abstract preference questions.
2. Prototype one canonical text project through edit, semantic diff, branch, merge conflict, asset relocation, validation, save/reload, and deterministic native render.
3. Have at least one blind or low-vision producer complete the reference workflow on the supported Linux stack using the actual screen reader and keyboard path.
4. Compare a command recipe, agent-generated transaction, and structured UI for two repeated tasks. Measure correction cost and trust, not only initial speed.
5. Round-trip a deliberately bounded project through DAWproject and publish a machine-readable loss report before promising interchange.
6. Fault-inject parser errors, missing samples, changed hashes, plug-in crashes, audio-device loss, save interruption, and render non-finite output; verify last-known-good playback and recoverable authored state.

## Sources

All sources accessed 2026-07-26.

- **[A1] Ableton, Live 12 Release Notes.** Current first-party release history, including Live 12.4 Learn View, accessibility fixes, crash fixes, recovery, state restoration, and render/playback corrections. https://www.ableton.com/en/release-notes/live-12/
- **[A2] Ableton, Live 12 Manual, "Managing Files and Sets."** External references, collection, missing-file repair, versions, transient undo history, templates, merging, and real-time render dropout recovery. https://www.ableton.com/en/live-manual/12/managing-files-and-sets/
- **[A3] Ableton, "Accessibility in Ableton Live FAQ."** Explicit screen-reader exclusions and tested readers. https://help.ableton.com/hc/en-us/articles/11762577169948-Accessibility-in-Ableton-Live-FAQ
- **[A4] Ableton, "Accessibility in Live Overview."** Keyboard workflow, incomplete Tab coverage, controller limitations, and task-level screen-reader support. https://help.ableton.com/hc/en-us/articles/11550373507868-Accessibility-in-Live-Overview
- **[D1] Bitwig, DAWproject README/specification, version 1.0.** Motivation, goals/non-goals, format, comparison with MIDI/AAF/stems, and implementation list. https://github.com/bitwig/dawproject/blob/main/README.md
- **[H1] Helio, official repository README, "Why another sequencer?"** Product motivation, clean UI, and integrated version control. https://github.com/helio-fm/helio-sequencer/blob/develop/README.md
- **[H2] Helio issue #13, "Integrating with other version control systems."** Open enhancement for mapping internal history to Git collaboration. https://github.com/helio-fm/helio-sequencer/issues/13
- **[H3] Helio issue #340, "Keyboard-only editing."** Open request documenting mouse-required note selection. https://github.com/helio-fm/helio-sequencer/issues/340
- **[H4] Helio issue #352, "Vital VST (not VST3) crashes Helio on launch."** Reproducible Linux plug-in failure report. https://github.com/helio-fm/helio-sequencer/issues/352
- **[L1] LMMS issue #403, "Remove global automations."** Contributor-authored discoverability analysis and migration checklist. https://github.com/LMMS/lmms/issues/403
- **[L2] LMMS issue #2221, "Scripting System."** Open workflow-automation enhancement. https://github.com/LMMS/lmms/issues/2221
- **[L3] LMMS issue #6909, "Implement Import/Export of the dawproject open format."** Assigned open enhancement. https://github.com/LMMS/lmms/issues/6909
- **[L4] LMMS issue #8415, "MIDI export merges all tracks into one track."** Reproducible current portability loss. https://github.com/LMMS/lmms/issues/8415
- **[L5] LMMS issue #4765, "Export MIDI: tempo change events."** Open report of tempo automation omitted from MIDI export. https://github.com/LMMS/lmms/issues/4765
- **[L6] LMMS issue #7104, "Carla instruments do not save/load properly; sound completely different after reload."** Open reproducibility regression. https://github.com/LMMS/lmms/issues/7104
- **[L7] LMMS issue #8439, "Make order of attributes in .mmp consistent."** Open deterministic-serialization/version-control enhancement. https://github.com/LMMS/lmms/issues/8439
- **[L8] LMMS issue #8230, "Useful Versioning Based on git."** Open semantic-history/diff enhancement. https://github.com/LMMS/lmms/issues/8230
- **[L9] LMMS issue #6047, "Increase GUI contrast in active/inactive buttons."** Open accessibility-labelled bug. https://github.com/LMMS/lmms/issues/6047
- **[L10] LMMS issue #7092, "Accessibility for Sight impaired: A 'fix' for fonts not scaling."** Open sight-impaired user report. https://github.com/LMMS/lmms/issues/7092
- **[L11] LMMS issue #8014, "Automation tracks are ignored during export."** Reproducible editor/export divergence at higher sample rates. https://github.com/LMMS/lmms/issues/8014
- **[L12] LMMS issue #551, "Script-based plugins (effects, controllers, etc.)."** Long-lived per-song processing customization request. https://github.com/LMMS/lmms/issues/551
- **[Q1] GitHub Search API snapshot, open LMMS issues labelled `bug` (444 results).** https://api.github.com/search/issues?q=repo%3ALMMS%2Flmms+is%3Aissue+is%3Aopen+label%3Abug&per_page=1
- **[Q2] GitHub Search API snapshot, open LMMS issues labelled `accessibility` (4 results).** https://api.github.com/search/issues?q=repo%3ALMMS%2Flmms+is%3Aissue+is%3Aopen+label%3Aaccessibility&per_page=1
- **[Q3] GitHub Search API snapshot, open LMMS issues with `crash` in the title (38 results).** https://api.github.com/search/issues?q=repo%3ALMMS%2Flmms+is%3Aissue+is%3Aopen+crash+in%3Atitle&per_page=1
- **[R1] Cockos, "ReaScript."** Official scripting, action binding, persistence, undo, background execution, and failure documentation. https://www.reaper.fm/sdk/reascript/reascript.php
- **[S1] Splice CEO Kakul Srivastava, "Our journey to make Splice better: A letter from our CEO."** First-party Studio collaboration shutdown and session/revision-sync timeline, 2023-03-09. https://splice.com/blog/studio-shutdown/
