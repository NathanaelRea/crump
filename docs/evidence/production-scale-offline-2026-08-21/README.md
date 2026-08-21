# Production-scale offline characterization — not realtime qualification

> **Status:** unsupported developer-host evidence only. These reports do not close the PipeWire, named-hardware, callback-safety, endurance, listening, or Slice 7 acceptance gates.

Captured 2026-08-21 from the exact Active Revision of [`fixtures/production/`](../../../fixtures/production/) after:

```bash
cargo build --release --all-features
crump --project "$PROJECT" --json qualify \
  --from drop --to break --quantum QUANTUM \
  --maximum-p999-utilization 0.7 \
  --output REPORT.json
```

Identity shared by every archived report:

- Project Revision: `5d1920663bb5fae1d2cefc6c0688825f7efb9c5357d84e00a1947f919166e42b`
- Executable SHA-256: `046bc689df0fdf1db5da21357a7fce45e9a3c65bbca3d9fcc58586d34f14da56`
- Host: unsupported Omarchy 4.0.0 / Linux 7.1.8 `x86_64`, AMD Ryzen 5 7640U
- Backend: `offline`; `pipewire_xruns` is `null`
- Peak range: `drop`→`break`, frames `[3,072,000, 4,608,000)`, 32 seconds
- Control range: `intro`→`build`, frames `[0, 1,536,000)`, 32 seconds

The peak range is selected from the fixture's authored maximum-activity region: the baseline pad, bass, drums, and texture layers continue while the dense pluck Clip and sample-rush Clip are additionally active from `drop` through `return`. It is a range choice, not an assertion that offline timing predicts PipeWire behavior.

| Report | Quantum | Mean | p99.9 | Peak | Missed | Finite | Result |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| `intro-build-q64.json` | 64 | 0.234 | 0.390 | 0.661 | 0 | yes | pass |
| `drop-break-q64-run1.json` | 64 | 0.360 | 0.545 | 1.075 | 1 | yes | **fail** |
| `drop-break-q64-run2.json` | 64 | 0.355 | 0.490 | 0.933 | 0 | yes | pass |
| `drop-break-q64-run3.json` | 64 | 0.361 | 0.580 | 0.905 | 0 | yes | pass |
| `drop-break-q128.json` | 128 | 0.361 | 0.580 | 0.833 | 0 | yes | pass |
| `drop-break-q256.json` | 256 | 0.362 | 0.435 | 0.524 | 0 | yes | pass |

All p99.9 values are below the operator-supplied `0.7` ceiling, and all decoded output samples were finite. The first 64-frame run nevertheless observed one wall-clock block above its offline deadline, so its report correctly has `passed: false`. The failed report is retained rather than replaced by the two passing repetitions.

The reports establish that the exact authored peak range was exercised at every promised full-block quantum and that the profiler preserves failures. They do **not** establish zero target-graph xruns, realtime scheduling behavior, a 30-minute zero-miss run, callback allocation/blocking/I/O safety, or supported-hardware capacity. Those gates remain open.
