# PROTOTYPE: Safe Graph Replacement

## Question

Can edited source be compiled into a replacement audio graph while invalid edits leave the active graph alone, and can valid replacements reach the audio callback without locks, allocation, deallocation, dangerous output, or an avoidable replacement discontinuity?

This is throwaway planning evidence, not the start of Crump's engine. It simulates an audio callback rather than opening an audio device. The graph is deliberately tiny so the experiment isolates publication, ownership, retirement, and transition behavior.

## Run

From this directory, launch the version picker:

```sh
bash run.sh
```

- `crossfade` blends old and new graphs over 192 samples (`4 ms` at 48 kHz), the accepted default fallback.
- `zero-crossing` keeps the old graph until its output is near zero, with a bounded 50 ms wait.
- `hard-swap` changes graphs at the next callback boundary. It is both the intentionally naive control case and the observable result of disabling the fallback preference when no edit-specific transition applies.

Enter source revisions such as:

```text
gain 0.25 freq 220
gain 1.8 freq 880
gain nope freq 440
```

Use `stress` to publish a burst of valid and invalid revisions, and `quit` to stop. The display reports the candidate and active revision, rejected edits, graph swaps, callback-side allocation/deallocation attempts, peak output, and worst adjacent-sample delta.

See enlarged waveform plots and listen to all three versions under the same transition workload:

```sh
bash run.sh compare
```

The comparison writes `demo-output/crossfade.wav`, `demo-output/zero-crossing.wav`, and `demo-output/hard-swap.wav`, then plays them with PipeWire. The vertical line in each terminal plot marks the requested replacement. These are waveform plots; the "audio graph" being replaced is the processing structure that generates them.

You can also launch one directly with `bash run.sh crossfade`, `bash run.sh zero-crossing`, or `bash run.sh hard-swap`.

## Deliberate Limits

- This proves bounded handoff for one compiler/control producer and one audio consumer, not the final graph representation or thread topology.
- Graphs are boxed off-thread. Superseded graphs return through a second bounded queue and are dropped off-thread.
- If the publication queue is full, source compilation waits on the control side; the callback never waits.
- The three versions compare only transition policy. The `4 ms` fallback and disable switch are an application-level authoring preference, not durable project or render semantics. Edit-specific ramps and safe or musical boundaries remain a later contract decision.
- The output clamp is only a last-resort safety guard, not a mastering limiter design.
