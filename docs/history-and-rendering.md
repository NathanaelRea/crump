# Project History and Deterministic Rendering

Crump treats persistence as a correctness boundary. Project source remains authoritative, while declared media is explicit exact-byte Project content. `.crump/` stores append-only actions, exact source snapshots, content-addressed media blobs, committed markers, the Active pointer, and one lock file. Playback and rendering use committed snapshots rather than assuming current disk bytes are valid.

## Revision identity

For an asset-free Project, a Revision ID remains the original `crump-project-source-v1` SHA-256 over the sorted set of normalized Project-relative source paths plus each file's exact bytes. This preserves every pre-media revision. When media is declared, the `crump-project-content-v1` identity additionally includes each sorted normalized asset path, exact byte length, and lowercase SHA-256. Path and length boundaries make concatenation ambiguity impossible. Formatting, comments, declarations, and media bytes therefore all participate in Candidate/Active identity.

Imports are canonicalized beneath their allowed root before hashing: ordinary imports beneath the Project, and `library:name` imports beneath the personal Instrument library (or an exact Project-local materialization). Personal-library use is never implicit: it must appear as a readable import, and publication snapshots the exact imported bytes. History Restore materializes those bytes inside the Project so the restored state no longer depends on later library changes.

## Metadata layout

```text
project/
├── project.crump
├── instruments/*.crump
└── .crump/
    ├── lock
    ├── state.json
    └── history/
        ├── entries/000000000001.json
        ├── snapshots/<revision>.json
        ├── assets/<sha256>
        └── committed/000000000001
```

Do not edit `.crump` by hand. It is intentionally readable for inspection and recovery tooling, not a second authoring surface. Back up the entire Project directory, including `.crump`, if history must travel with source.

## Publication transaction

A publication holds an exclusive Project lock and follows this order:

1. recover or remove any incomplete prior staging state;
2. verify the Candidate is fully parsed and valid;
3. validate provenance (including Agent explanation);
4. assign the next fixed-width monotonic sequence ID;
5. verify or persist immutable exact media blobs by content hash, then write and fsync the exact manifest snapshot and History Entry through sibling temporary files;
6. atomically replace and fsync `state.json` with the provisional new Active pointer;
7. create and fsync the committed marker as the final commit point; and
8. release the lock only after those atomic writes have synchronized their containing directories.

The state-before-marker order is deliberate: if interruption leaves a provisional pointer, startup recovery rolls it back to the newest complete committed entry and discards uncommitted metadata. The visible post-recovery result is the old complete Active state or the new complete state, never a mixture. Temporary source-edit backups are restored or cleaned idempotently.

An invalid Candidate leaves the previous Active pointer unchanged. A publication of a revision that is already Active is a conflict rather than a duplicate action.

## History entries

Each entry records:

- schema and monotonic local sequence;
- exact revision and parent identity;
- RFC 3339 UTC timestamp;
- Human, Agent, or System actor;
- concise summary and explanation;
- optional restore source; and
- changed source or media paths, added/removed/modified kind, before/after hashes, and changed-line count (`0` for binary media).

`history show` and `history diff` derive readable exact source changes. History is an action log, so multiple entries may intentionally point to the same content revision (for example, restore).

## Restore

```bash
crump history restore 000000000003 \
  --actor "$USER" \
  --explanation "Return to the sparse middle section"
```

Restore copies the selected exact source set and content-addressed media bytes into one journaled transaction, atomically removes declared files not present in that snapshot, validates the reconstructed Project from those bytes, and publishes it as a **new** entry with `restored_from`. Existing entries, snapshots, and immutable blobs remain unchanged. Startup recovery resolves an interrupted source-and-media transaction to the complete pre-commit or post-commit state.

This is durable Project History, not session-only undo/redo. Routine Production View changes are already separate entries, so selecting and restoring an earlier action provides the release's reversible workflow.

## Render range

The offline Render Range is explicit and finite:

```text
[Song zero, Song end + render_tail)
```

`Song end` is the arrangement boundary. `render_tail` is authored Project data, converted deterministically to frames, and capped by validation. Events cannot begin at or after Song end. Release/envelope/effect state may continue through the tail; output stops exactly at the resulting frame count.

## Output bundle

```bash
crump render --output render/master --plan
crump render --output render/master
```

The non-mutating `--plan` form reports the exact Active start, arrangement end, tail, exclusive end, sample rate, duration, destination state, and replacement/conflict consequence without creating directories or artifacts.

A complete bundle contains:

```text
render/master/
├── master.wav
├── stem-track-lead-post_fader.wav   # only when requested
├── stem-group-music-post_fader.wav  # only when requested
└── manifest.json
```

The WAV is stereo 48 kHz IEEE 32-bit little-endian float. It contains authored output without the monitoring limiter. Any non-finite sample aborts publication and identifies the frame. After finalization and `fsync`, Crump reopens the WAV and derives delivery analysis from the decoded written artifact rather than trusting the in-memory bus.

The manifest includes:

- render schema version (currently 2) and Project ID;
- exact rendered Active Revision;
- Crump package version and running executable SHA-256;
- target OS/architecture and reference-environment flag;
- sample rate, channel count, and sample format;
- arrangement, tail, and total frame counts;
- exact duration and backward-compatible linear sample peak;
- finalized-file sample peak/dBFS and fixed 4× reconstructed true peak/dBTP;
- K-weighted gated integrated loudness, maximum three-second short-term loudness, and loudness range;
- per-channel DC offset, clipped sample/frame counts, non-finite count, and analyzed frame count;
- each selected stem's durable Track/Group owner, pre/post tap, deterministic filename, Master-alignment delay, inclusion policy, format/layout/frame count, finalized-file analysis, and exact WAV hash; and
- SHA-256 of the complete Master WAV bytes.

Silence or an artifact too short for a required loudness window is represented by `null`, not a fabricated finite LUFS value. Integrated loudness uses 400 ms windows at 100 ms steps with the BS.1770-derived absolute and relative gates. Short-term loudness uses three-second windows at 100 ms steps. Loudness Range first applies its `−70 LUFS` absolute gate, derives a separate `−20 LU` relative gate from the absolute-gated short-term mean, and reports the nearest 10th-to-95th-percentile span. The manifest names analysis schema 2 and the custom reconstruction kernel. The implementation passes the official EBU steady-tone loudness tolerance and true-peak cases 15–19, but the complete official signal set and transient true-peak cases remain open; see [the primary-source review](research/finalized-audio-analysis.md). These are measurements only: Crump applies no hidden normalization and does not reject creative output against a universal loudness target. Full-scale sample crossings are reported as clipping rather than silently changed; non-finite output remains a publication failure.

A requested Stem uses `--stem track/ID:pre_fader|post_fader` or `--stem group/ID:pre_fader|post_fader`. Both taps include upstream main routing and processing through the selected owner's inserts. The post-fader tap additionally includes its gain, panner, and Mute. Outgoing Send branches and downstream Group, Return, and Master processing are excluded. Every Stem receives deterministic initial zero padding equal to Master output latency minus selected-tap latency, and every file has the exact Master frame count. Fixed-size capture blocks are written to raw staging shards containing at most 16 Stems each, then each shard is streamed into at most 16 aligned WAV writers. This bounds open Stem descriptors and avoids multiplying long latency by the selected-Stem count in memory. A maximum admitted 192-Stem fixture exercises all Track/Group taps, and an injected post-spool failure proves preservation of the prior destination plus staging cleanup. These semantics make stems sample-aligned without claiming that an arbitrary incomplete selection reconstructs the processed Master.

The CLI response additionally returns SHA-256 of the exact manifest bytes and reports whether current Candidate and rendered Active identities match.

## Atomic render publication

Crump first rejects destinations that overlap Project source/history, symbolic-link destinations, non-directories, arbitrary non-render directories, and existing bundles whose manifest/artifact set, WAV hashes, formats, frame counts, or recomputed finalized-file analyses are inconsistent. It renders Master, every requested Stem, and the manifest into a unique sibling staging directory; finalizes, analyzes, hashes, and fsyncs every WAV; writes and fsyncs the manifest; and fsyncs the staging directory. Immediately before publication it reopens, hashes, and reanalyzes every artifact in any existing bundle. Publication then uses Linux `renameat2`:

- `NOREPLACE` publishes staging only if no destination won the race; or
- `EXCHANGE` atomically swaps complete staging and destination directories, after which the retired bundle is removed.

A reader therefore sees the old complete bundle or the new complete bundle. It does not see a new WAV paired with an old manifest, and Crump never recursively deletes an unrecognized destination. Staging remains on the same filesystem so directory exchange remains atomic. `--no-replace` fails if a destination already exists and leaves it unchanged.

A crash after an exchange can leave an unreferenced sibling `.rendering-*` directory containing the old complete bundle. This is safe residue, not a partial published master, and can be removed after checking the named destination.

## Determinism domain

Playback and render use the same semantic schedule and DSP implementation. Stable Event identities and random seeds derive from semantic Project/Event content; authored Track presentation order does not alter those identities. Event sorting, graph topology, voice stealing, drum/noise generation, sampler decode/resampling, finalized-file analysis, and WAV/manifest serialization are deterministic. Durable processor order, settings, and bypass are Authoritative Source and therefore affect both Active playback and render after publication. Audition target/path/range/loop, Solo, cue, pause, and transient processor bypass remain session-only monitoring state and are ignored by offline processing; repeated render tests prove that these overrides do not enter artifact identity. Portable media identity/history and sampler playback are implemented; Media Tracks and time stretching remain deferred.

The byte-identity promise is intentionally bounded. Repeated unchanged renders are expected to produce byte-identical WAV and manifest files only for the same:

- exact Project revision;
- render settings and range;
- Crump executable/build;
- 48 kHz sample rate; and
- documented Linux `amd64` comparison environment.

The manifest contains the executable hash and target facts needed to decide whether two renders are in the same comparison domain. Crump makes no bit-identity promise across versions, architectures, compiler outputs, or unsupported hosts.

## Verification

```bash
sha256sum render/master/master.wav render/master/manifest.json
crump inspect --json
```

For a release comparison, render twice to separate destination names with the same executable, compare exact bytes with `cmp`, and archive both command responses plus host facts. See [Release evidence](release-evidence.md).
