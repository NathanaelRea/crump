# 0032. Track Freeze never delays playback

Accepted. Supersedes the Track Freeze capture and cache-key paragraphs of
[ADR 0030](0030-live-and-offline-runtime-admission.md).

Interactive playback must start immediately after any edit. Capturing a frozen
Track during Live preparation made Play wait for a whole-Song render, about two
minutes for a three-minute Song, and any edit anywhere invalidated every
capture.

Live preparation in the workspace no longer captures. It uses each frozen
Track's cached audio when the key matches and returns the missing captures as
background work. Until a capture lands, its Track runs live. When running the
missing Tracks live would exceed the Live budget, they play their previous
capture from the same slot instead, or silence when there is none, and the
workspace swaps the new capture in with an Affected Instrument Restart as soon
as it is ready. The workspace states which stand-in is playing. A newer capture
cancels an older one. `crump play` still waits for missing captures.

A capture covers the Track's scope: the Track and every channel that can reach
its pre-fader tap through routing or an Envelope Follower targeting the scope,
with the Instruments, Patterns, Grooves, Sends, and controls those channels
use. Outputs leaving the scope go to Master, and the Master chain is cleared.
Each scope renders as its own Project, in parallel with the others. A scope
that fails to compile falls back to a whole-Project capture.

The key hashes the scope's model with names, presentation, source locations,
and the freeze directives removed. Faders, panners, and Mutes are left out
unless they reach the tap through a post-fader route or Follower. The key also
includes the Track schedule, the cache algorithm version, and an engine
fingerprint. A build script computes the fingerprint from every source file
except the client, CLI, editing, parsing, History, and Project-loading modules,
plus `Cargo.lock`, the compiler version, the target, and the profile. Those
modules only produce or display the model, which the key already hashes.

Verified captures stay in memory for the process and are reused while the slot
file still carries the same key.

The rejected alternative was to keep blocking with a faster renderer. Even at
four times realtime, a three-minute Song makes Play wait 45 seconds.
