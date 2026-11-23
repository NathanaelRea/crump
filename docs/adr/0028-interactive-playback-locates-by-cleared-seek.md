# ADR 0028: Interactive Playback Locates by Cleared Seek

Status: Accepted
Date: 2026-11-22
Amends: the seek clauses of [docs/audio-and-midi.md](../audio-and-midi.md) and [docs/architecture/overview.md](../architecture/overview.md)

Workspace playback (terminal and desktop) now positions a freshly compiled runtime at its start frame with a cleared seek instead of deterministic preroll from Song zero. A cleared seek moves every Track's schedule cursor to the target frame and clears voices, mix-graph processor state, and Envelope Follower state, the same operation compatible publication already used to prime a replacement runtime. It still runs on the controller thread and is published at a block boundary; the callback never does the seek.

Preroll renders every frame before the start one sample at a time. On the deep-house reference it ran at about 1.3× realtime, so pressing Play at the Drop (bar 64) took about 147 s, and every edit made during mid-Song playback prerolled a replacement to the playhead before throwing that state away in `prime_for_publication`. A cleared seek prepares the same start in about 40 ms.

## Consequences

- Notes and effect tails that began before the start frame are not heard after a mid-Song Play, as with a DAW locate. Raw audition stays sample-identical because it does not flow through the mix graph.
- Offline render, bounce, qualification, and `crump play --from` keep deterministic preroll, so rendered output is unchanged and still the reference for what a Song sounds like at a given frame.
- `AuditionPrepareOptions::cleared_seek` selects the behaviour; it defaults to preroll.

## Alternatives

- Faster preroll (block processing, parallel Tracks) would shrink the wait but keep it proportional to the start position.
- Chasing notes that overlap the start frame is compatible with this decision and can be added later without reintroducing preroll.
