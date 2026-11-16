# Portable deep-house synthesis reference

This is an original, self-contained structural reference at 122 BPM and 4/4. Its 112-bar half-open arrangement is 220.328 seconds (3:40.328) before the explicit four-second render tail. It is intended to prove editable complete-track integration, not producer credibility or similarity to a commercial recording.

## Section and source coverage

| Locator | Bar | Arrangement role |
| --- | ---: | --- |
| `intro` | 0 | pad and evolving texture establish harmony |
| `groove-in` | 16 | kick, percussion, bass, and hybrid keys enter |
| `theme-a` | 32 | wavetable hook enters |
| `break` | 48 | granular and spectral layers take focus |
| `drop` | 64 | full groove returns |
| `theme-b` | 80 | hook and transformed textures answer |
| `outro` | 96 | foreground parts leave |
| `end` | 112 | render boundary |

Every runnable source family is assigned to a Track: virtual analog (`bass`, `pad`, `transition`), wavetable (`lead`, `evolving`), `sample_source` plus multisample (`keys`), granular (`granular`), and spectral (`spectral`). Stock synthesis supplies `kick-drums` and `percussion`. The generated section table above is authored metadata for verification; all actual musical events remain reusable Patterns and Clips.

The production includes Pattern Variations, a deterministic swing Groove, bass cutoff and keys sample-start/decay Parameter Locks, absolute Automation, per-voice/Instrument/global modulation, an Audio-derived kick follower, explicit kick-to-bass compressor sidechain, Music/Drums Groups, shared Room/Echo Returns, post processing, and a Master limiter.

## Verification workflow

```bash
crump --project fixtures/deep-house-reference --json status
crump --project fixtures/deep-house-reference --json validate
crump --project fixtures/deep-house-reference --json inspect --at 64bar
crump --project fixtures/deep-house-reference --json publish \
  --origin agent --actor "$AGENT_NAME" \
  --summary "Add portable deep-house synthesis reference" \
  --explanation "Milestone 7 integrated source-family reference"
crump --project fixtures/deep-house-reference --json history list
crump --project fixtures/deep-house-reference --json render \
  --output render/delivery-final \
  --stem group/music:post_fader --stem group/drums:post_fader \
  --stem track/bass:post_fader --stem track/lead:post_fader --plan
crump --project fixtures/deep-house-reference --json render \
  --output render/delivery-final \
  --stem group/music:post_fader --stem group/drums:post_fader \
  --stem track/bass:post_fader --stem track/lead:post_fader
crump --project fixtures/deep-house-reference --json render \
  --output render/delivery-final-repeat \
  --stem group/music:post_fader --stem group/drums:post_fader \
  --stem track/bass:post_fader --stem track/lead:post_fader
```

`check` is intentionally not a completion gate for this fixture: that command enforces the separate 30–60 second Musical Sketch contract and rejects both this required 3–5 minute duration and declared media. `validate` and render compilation are the applicable objective gates.

Requested stems are aligned delivery taps but are not claimed to reconstruct the processed Master. Render manifests disclose independent hashes and analysis. Repeated render hashes prove determinism only when the executable/build, Active Revision, request, and deterministic domain are identical.

## Human evaluation still required

No machine check establishes groove quality, mix translation, expressive usefulness, or level-matched comparison to lawful reference instruments. Those listening/taste gates remain pending and this fixture makes no commercial-equivalence claim.
