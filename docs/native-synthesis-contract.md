# Native Synthesis Compatibility

Crump accepts the native Instrument behavior documented by the current `crump-module 1` language. This page records the compatibility boundary for graph-authored synthesis without defining another source mode.

## Supported behavior

A graph-authored Instrument contains:

- stable Parameters with physical units and bounded domains;
- a Faceplate mapped to those Parameters;
- one typed per-voice Composable DSP Graph;
- deterministic polyphonic voice allocation and replacement;
- explicit Event, Audio, and Control Ports and Buses;
- oscillators, noise, envelopes, filters, amplification, panning, and supported modulation paths;
- an ordered Instrument processor chain after voice summing; and
- exact source locations, resource estimates, inspection, History, playback, and render behavior.

Graph validation rejects missing or duplicate IDs, invalid topology, incompatible port types or units, implicit channel conversion, cycles, disconnected required inputs, non-finite values, unsupported settings, and resource excess. Compilation and immutable runtime construction occur away from the audio callback.

The Project media model accepts exact-hash Project-owned WAV assets. Sampler Instruments decode and resample those assets off-callback and provide the bounded regions, loops, direction, pitch, envelope, and voice behavior documented in [Project Language 1](language.md).

## Source compatibility

`crump-module 1` is the accepted module header. Other module versions and unknown declarations are parse or validation errors. Crump does not accept a Node or setting unless parsing, validation, compilation, inspection, resource accounting, playback, rendering, and failure behavior are defined for it. Unsupported declarations never compile as silent no-ops.

An invalid Candidate leaves the prior Active Revision unchanged and audible. Playback and rendering continue to consume the exact Active snapshot rather than unchecked source or media.

## Unsupported synthesis declarations

The current grammar has no:

- shared post-voice graph declaration;
- Instrument-owned modulation-source or modulation-route matrix;
- macro Parameter dependency graph;
- graph wavetable, granular, multisample, or spectral source Node;
- implicit mono/stereo conversion or implicit graph fan-in;
- opaque native preset, dynamically loaded plug-in, or arbitrary Rust extension; or
- external authoritative sample-library path.

These names are not reserved syntax and must not be emitted by an Authoring Agent.

## Current ownership rules

Song Automation, Pattern Parameter Locks, routed LFO Modulation, and the Envelope Follower use the precedence and ownership documented in the language guide. Project-owned media bytes and `.crump` source are Authoritative Source; decoded samples and compiled programs are disposable runtime representations. Generated views project existing declarations and do not own sound state.
