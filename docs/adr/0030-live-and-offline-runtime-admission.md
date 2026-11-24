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

A future Track Freeze can use an authored Track setting and a disposable
pre-fader cache beneath `.crump/`. The flag would enter History as source,
and the cached bytes would stay outside Media Assets and Revision identity.
To preserve routing, a live replacement must keep the source tap available to
Sends, sidechains, envelope followers, and applicable audition paths. Cache
invalidation must cover upstream signal and control dependencies, timing, and
the executing build. Offline delivery continues from the full authored graph.
