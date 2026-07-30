# Freeze the Native Musical-Sketch Capability Floor

Type: grilling
Mode: HITL
Status: open
Blocked by: [07 - Define the Routing, Modulation, and Parameter Semantic Model](07-define-routing-model.md), [10 - Freeze the Minimal DSL and Project-Source Contract](10-freeze-dsl-contract.md), [11 - Freeze the Compiled-Project and Runtime-Transition Contract](11-freeze-runtime-transition-contract.md)

## Question

What minimum graph primitives, polyphony and voice behavior, stock synthesized percussion, filtering, gain/pan, simple delay, simple reverb, saturation, Track balance, metering, Graph Validation, and monitoring safety are necessary and sufficient for the no-sample Musical Sketch?

## Research Baseline

Use native DSP and safety primary sources where they constrain correctness; do not derive a device inventory from full DAWs. Exclude samplers, compressors, detailed EQ, mastering, sidechains, Sends/Returns, plugins, and waveform generation. Human listening judges musical quality; machine checks enforce graph validity, declared bounds, deterministic contracts, finite authored values, and monitoring containment.
