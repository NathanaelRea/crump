# ADR 0031 — Zero-latency convolution

Status: Accepted

A Project can reproduce a captured or externally generated linear reverberation response through a native `convolution` processor. The source is an exact-byte Media Asset with `usage impulse_response`. The Project sample cache decodes and resamples it before runtime preparation. Mono and dual-mono stereo mappings are explicit. The converted response is capped at 10 seconds.

The first 256 taps run directly so the dry and wet paths share zero intrinsic latency. Later taps use uniformly partitioned overlap-add convolution with 512-point transforms from the native radix-2 implementation. Real-signal symmetry halves the retained spectrum. The runtime spreads older partition multiply-accumulates across the block and processes the newest spectrum and transforms at the boundary. This bounds the burst without adding buffering latency to the direct path. The internal phase advances per sample, independently of host block grouping.

The prepared kernel is immutable. The runtime owns preallocated buffers for input spectra, overlap, scratch, and wet pre-delay. Admission derives bytes and amortized work from Project-rate frame counts before preparing kernels. Exact sample identity and structural filter settings govern state compatibility. Optional wet cuts are baked into the kernel with one second of filter decay so the effect has a finite tail. Bypass uses the shared latency-aligned policy and keeps history running.

A fully FFT-buffered implementation would need to delay dry audio for alignment. Direct convolution of every tap would increase per-sample work with the full response length. This design trades bounded direct-head work and fixed transform bursts for immediate output and predictable admission. The work estimate is abstract; live performance depends on the host and Project.
