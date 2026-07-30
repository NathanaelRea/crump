# Assess CLAP Hosting Against Crump's Release Constraints

Type: research
Mode: AFK
Status: resolved
Blocked by: none

## Question

Can current Rust CLAP-hosting libraries on the supported Linux baseline satisfy parameter discovery, automation, state restoration, latency reporting, scanning, GUI handling, failure containment, and reproducibility requirements, and where can they not?

## Answer

Use `clack-host` and `clack-extensions` as the only currently credible Rust foundation to evaluate, isolated behind a Crump-owned adapter and pinned with their `clap-sys` dependency. They expose CLAP's low-level mechanisms for parameter metadata and rescans, sample-offset automation events, opaque state streams, latency reporting, GUI negotiation, render mode, timers, and POSIX file descriptors, but they do not provide a production host or satisfy Crump's release contract by themselves.

Crump must still provide source-level plugin and parameter identity, bounded event transport, plugin-delay compensation, scanner policy, opaque-state storage and compatibility diagnostics, a desktop event-loop/GUI policy, and per-plugin qualification. Descriptor scanning executes third-party native code, so it must run in a disposable bounded process. A terminal host has no native parent window; CLAP supports X11 embedding, while standard Wayland behavior is limited to floating plugin windows.

Neither CLAP nor safe Rust wrappers contain plugin crashes, hangs, or undefined behavior. Meeting the playback-continuity requirement needs active DSP in a supervised worker process with preallocated bounded IPC, deadline fallback, latency accounting, state replay, and quarantine behavior. Likewise, exact state bytes and offline render mode cannot guarantee deterministic DSP; reproducibility can only be measured and promised for named plugin binaries and environments, with explicit exceptions.

The [primary-source research report](../assets/05-rust-clap-hosting-research.md) contains the capability matrix, library comparison, citations, uncertainties, and recommended evidence gate. It supports a focused architecture prototype before [Make the First-Release CLAP Inclusion Decision](19-decide-clap-inclusion.md), but does not decide inclusion.

## Comments

- Third-party plugin hosting is explicitly deferred beyond the First Public Release. This answer remains post-release technical evidence; issues 19 and 23 are no longer first-release successors or blockers.
