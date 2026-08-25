# Native synthesis alpha resource probe

`resource_probe.rs` is a focused, non-production workload prototype used to choose Milestone 0's `qualification-max-v1` profile. It preallocates the measured workload state, then approximates per-frame work for:

- eight polyphonic voices;
- 16 virtual-analog/wavetable unison lanes per voice;
- 64 active grains per voice;
- 128 recursively advanced spectral peaks per voice;
- eight stereo state-variable filters per voice; and
- 64 Instrument routes per voice.

Run it with:

```bash
prototypes/synth-alpha/run.sh
```

The checked-in `measurement-2026-08-22.json` records the median of five consecutive release-optimized process observations on the disclosed host and retains all five combined-workload observations. The exploratory 16-voice profile was materially heavier, so this historical prototype measurement used eight voices. The runnable integrated qualification fixture now admits seven resident voices because an eighth crosses the implemented two-runtime transition-work budget; the prototype remains design evidence rather than current runtime qualification. The probe records its exact Rust flags and source hash.

This is ceiling-selection evidence only. It does not execute the eventual algorithms, measure p99.9 callback time, observe PipeWire xruns, prove no callback allocation, qualify another CPU, or establish sound quality. Runtime admission must remain conservative, and the final named-hardware endurance gate supersedes this prototype.
