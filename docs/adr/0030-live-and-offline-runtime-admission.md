# 0030. Live and offline runtime admission

Accepted.

Live playback and realtime qualification use the Live admission profile. It
reserves up to two resident runtimes and accounts for their combined
per-frame work and transition bookkeeping. A guarded replacement can overlap
the prior runtime inside the audio callback.

Render, bounce, and Instrument Module preview use the Offline profile. They
keep the same structural capacities and cap one runtime at 1 GiB. They have no
callback work limit. Render planning and control inspection also use Offline
admission.

Project Publication requires Offline admission. An Offline budget failure
blocks publication with CRV092. A Project that fits Offline admission but
exceeds the Live budget publishes with CRV093. Live compilation still checks
admission before allocating the runtime. The workspace reports that playback
of the published revision is unavailable and retains any prior audible runtime.

Track Freeze is an authored Track option. Live preparation captures the
Track's stereo pre-fader tap through Offline admission from Song zero through
Song end and the render tail. The cache stores absolute Song frames,
including upstream routing compensation and insert latency. Live compilation
keeps the tap's latency and routing identity, replaces the Instrument and
inserts with a cached sample lookup, and budgets the resident audio and the
lookup. Fader, panner, Mute, Sends, and outgoing sidechain and Envelope
Follower taps continue to run live.

The cache is disposable and bounded by Track slots under `.crump/freeze/`.
Scoped auditions use a separate set of slots for their filtered schedules.
Each slot contains a semantic content key, little-endian stereo float samples,
and a checksum. Preparation checks the key, size, and checksum, then replaces
a missing or stale slot atomically after capture. The key combines the
executing binary hash, cache algorithm
version, Track schedule identity, and a conservative semantic Project
fingerprint including timing, assets, graphs, inserts, and control
dependencies. Source locations and freeze directives are ignored. A Track's
post-fader controls enter the key when they can feed the capture through
routing or potentially through an Envelope Follower.

Track Freeze enters History through Authoritative Source. Cache bytes stay
outside Media Assets and Revision identity. Offline render and bounce always
execute the full authored graph. Audio bytes are identical across freeze
edits, while delivery manifests identify their respective source revisions.
