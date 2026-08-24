# ADR 0011: Graph Transition Behavior

Status: Accepted
Date: 2026-07-28

A valid topology change restarts the affected Instrument's voices and effect state behind an aligned short equal-power fade. Transport and semantically unchanged Tracks continue, and the workspace reports the transition.

Topology-compatible publication retains compatible voice, processor, and compensation state. Only changed continuous effective targets receive a preallocated monitoring ramp; unchanged step and linear Automation remains sample-exact, and boolean controls remain discrete.

Crump does not migrate incompatible voice, envelope, delay, or reverb state across topology changes. It also does not restart the whole transport for one affected Instrument.
