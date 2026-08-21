# Open-source and open-weight text-to-singing review

**Accessed:** 2026-08-21  
**Target:** original, offline-generated vocal assets for Crump on a Ryzen 5 7640U with 30 GiB RAM

## Conclusion

Turning ordinary TTS into a stylized melodic hook is practical, especially for short, processed electronic vocals. Turning a spoken phrase into a convincing exposed lead singer is not reliably solved by pitch shifting alone. Natural results require note-aware phoneme timing, a continuous pitch contour, formant-aware time/pitch processing, singing-like excitation, and musical transitions such as portamento and delayed vibrato.

For this project, the strongest current options are:

1. **Vevo2** for a non-commercial experiment with explicit melody control from a synthetic piano or humming guide.
2. **ACE-Step 1.5** for permissively licensed lyrics-to-music generation, multitrack generation, and vocal extraction, accepting weaker note-level determinism.
3. **OpenVPI DiffSinger** for precise score-conditioned synthesis if a suitable English voicebank with explicit distribution/commercial terms can be found or trained.
4. **TTS plus Rubber Band/WORLD/Praat-style processing** for deliberately synthetic vocal chops and backing layers.

YuE and other long-form song language models are not sensible CPU-first dependencies on this machine.

## Speech-to-singing by signal processing

A dry spoken TTS recording can be made melodic by:

1. forced-aligning words into phonemes or at least consonant/vowel regions;
2. mapping syllables to authored notes and extending primarily the vowel nuclei;
3. imposing a continuous F0 trajectory with note centers, slides, overshoot, and vibrato;
4. changing duration independently of pitch;
5. preserving or deliberately moving vocal formants;
6. retaining unvoiced consonants rather than forcing them onto pitched harmonics;
7. adding dynamics, breath/noise, doubles, harmonies, compression, saturation, delay, and reverb.

A constant pitch shift over a complete spoken phrase leaves speech timing and intonation underneath the new pitch. Uniform time stretching smears consonants, breaths, and transients. Large pitch or duration changes expose phasey sustained vowels, metallic excitation, duplicated consonants, formant mismatch, and robotic note transitions.

This machine already has FFmpeg's Rubber Band filter and `/usr/bin/rubberband`. The local TTS experiment at `../tts/generate_chatterbox_expression_reel.py` uses independent tempo and pitch controls with `formant=preserved`. That is enough for a first stylized-hook prototype, but not a complete singing renderer. A stronger offline implementation would use phoneme alignment plus time-varying pitch and duration curves, or a singing-aware neural model.

Primary technical sources:

- Rubber Band, [official site and documentation](https://breakfastquay.com/rubberband/) — independent time stretching, pitch shifting, and formant options.
- Praat, [Manipulation manual](https://www.fon.hum.uva.nl/praat/manual/Manipulation.html) — pitch and duration tiers with overlap-add resynthesis.
- Morise et al., [WORLD vocoder paper](https://www.isca-archive.org/interspeech_2016/morise16_interspeech.html) — F0, spectral-envelope, and aperiodicity analysis/resynthesis.

## Model categories

- **Lyrics-to-song:** lyrics and tags are enough; the model invents melody, singer, arrangement, and accompaniment. Output is usually a complete mix.
- **Text-to-singing / melody-controlled generation:** lyrics plus a melody/prosody guide produce a sung performance, often as isolated voice.
- **Score-conditioned singing voice synthesis (SVS):** the caller supplies lyrics or phonemes, note pitches, and note/phoneme durations. This is the most deterministic category.
- **Singing voice conversion (SVC):** changes an already sung performance's timbre/style; it does not remove the need for a guide performance.

## Candidates

### Vevo2

Official sources: [Hugging Face model](https://huggingface.co/RMSnow/Vevo2) and [Amphion recipe](https://github.com/open-mmlab/Amphion/tree/main/models/svc/vevo2).

Vevo2 explicitly supports zero-shot text-to-singing, SVS, singing editing, singing style conversion, and melody control. Its released examples accept a target text plus a prosody WAV; the prosody input may be humming or piano. Separate style and timbre references are optional in the melody-control interface. This makes it technically well matched to a workflow where Crump or a small script renders an original guide melody and Vevo2 turns it into an original vocal asset.

The released stack includes a 0.5B autoregressive model, a 350M flow-matching acoustic model, tokenizers, and a 250M vocoder. The official code selects CPU when CUDA is unavailable, but publishes no CPU performance figure. The Hugging Face repository is about 9.2 GB including duplicate/pretraining and optimizer artifacts; the inference subset is smaller but still substantial.

**Critical restriction:** the model card licenses the released weights under **CC BY-NC-ND 4.0**. This is open-weight, not OSI open source, and is unsuitable as a commercial production dependency without separate permission.

### ACE-Step 1.5

Official sources: [Hugging Face model](https://huggingface.co/ACE-Step/Ace-Step1.5) and [repository](https://github.com/ACE-Step/ACE-Step-1.5).

ACE-Step 1.5 is a lyrics-to-music foundation model rather than a deterministic score renderer. It supports lyrics, duration, BPM, key/scale, time signature, reference audio, cover/repaint operations, multitrack generation, track extraction, and `lego`-style layer generation. The official inference documentation lists vocals and backing vocals among extractable/generatable tracks.

A plausible Crump workflow is:

1. render an original instrumental or guide track from Crump;
2. ask ACE-Step to add a vocal layer with original lyrics;
3. extract the vocal track;
4. import the resulting WAV as a declared Crump asset;
5. do final arrangement, effects, automation, and mixing in Crump.

The model/repository license is **MIT**. The standard Hugging Face snapshot is about 10.1 GB. CPU inference is officially supported, but the installation and benchmark guides describe CPU as significantly slower and for testing; published fast timings are GPU results. It is therefore a practical candidate for an offline trial, not yet a measured CPU workflow on this machine. Its main creative limitation is stochastic melody, pronunciation, and singer identity rather than exact note/phoneme control.

### OpenVPI DiffSinger

Official source: [OpenVPI DiffSinger](https://github.com/openvpi/DiffSinger).

DiffSinger is score-conditioned SVS. It provides explicit control over pitch, energy, breathiness, and related variance curves and targets 44.1 kHz production workflows. It is the strongest architectural match when the composition owns exact notes and durations.

The maintained framework is **Apache 2.0**, but it is not itself a commercially cleared English singer. Acoustic models, vocoders, dictionaries, and community voicebanks have independent licenses and consent/provenance requirements. A suitable English checkpoint must be selected and reviewed separately. This is the primary adoption risk, not the engine interface.

### ESPnet VISinger2 checkpoints

Official example: [ACESinger/OpenCPOP VISinger2 44.1 kHz](https://huggingface.co/espnet/acesinger_opencpop_visinger2_44khz).

ESPnet publishes score-conditioned SVS checkpoints. The cited model consumes text/phonemes, duration labels, a musical score, and pitch features; its model card is **CC BY 4.0**, and the checkpoint is about 448 MB. This is much smaller than full-song models and is a plausible CPU benchmark target.

The cited checkpoint is Mandarin Chinese and does not solve the desired English-vocal workflow. Other ESPnet checkpoints likewise need language, singer, and model-card review. It remains useful as evidence that a compact, explicit-score integration is feasible.

### YuE

Official sources: [Hugging Face model](https://huggingface.co/m-a-p/YuE-s1-7B-anneal-en-cot) and [repository](https://github.com/multimodal-art-projection/YuE).

YuE transforms sectioned lyrics and genre/style prompts into multi-minute songs containing vocals and accompaniment. The code and released weights are **Apache 2.0**. Its ordinary interface does not provide deterministic score-level melody control, and the project still lists stem generation as unfinished.

The English stage-one checkpoint is about 12.45 GB and stage two about 3.93 GB. Official guidance recommends at least 24 GB GPU memory for short sessions and 80 GB for multi-session full songs; published generation time for 30 seconds is about 360 seconds on an RTX 4090. It is not a realistic CPU-first dependency here.

### Other full-song models

DiffRhythm 2 and HeartMuLa publish lyrics-conditioned full-song weights under Apache 2.0, but they remain less suitable than ACE-Step for this particular integration: they target complete mixed songs rather than exact isolated score-driven vocals, and their official local workflows are GPU-oriented. They may be useful for ideation but should not displace Crump's authored composition.

Sources:

- [DiffRhythm 2 official model](https://huggingface.co/ASLP-lab/DiffRhythm2) and [repository](https://github.com/ASLP-lab/DiffRhythm2).
- [HeartMuLa official model](https://huggingface.co/HeartMuLa/HeartMuLa-oss-3B) and [repository](https://github.com/HeartMuLa/heartlib).

## Recommendation for `already-here`

The reference vocal is a short, recurring, heavily produced pitched motif rather than an exposed conventional lead performance. That makes a hybrid approach realistic:

1. author the melody and syllable timing explicitly;
2. render an original piano/humming guide;
3. test **Vevo2** only if non-commercial licensing is acceptable, or test **ACE-Step 1.5** as the permissive alternative;
4. in parallel, make a TTS-plus-Rubber-Band version from synthetic voices already available under `../tts`;
5. import only newly generated vocal WAVs into Crump;
6. layer doubles/octaves/harmonies and finish them with Crump's sampler, automation, modulation, routing, and effects;
7. compare both routes under human listening before adding Crump DSP.

If model-rendered vocals are accepted as build-time assets, Crump does not need neural singing inference. Its most valuable native addition would instead be a deep offline warped-sample module: independent time/pitch curves, formant policy, phoneme/warp anchors, and deterministic generated-asset provenance.

## Remaining checks

Before adopting any checkpoint:

- record its exact revision/hash and model-card license;
- review voice/singer consent and generated-audio terms independently of the framework license;
- confirm whether output is isolated vocal or a separated mixture;
- benchmark a 10–20 second phrase for cold start, peak RAM, runtime, and repeatability;
- listen for lyric alignment, note accuracy, sustained-vowel artifacts, consonant timing, breaths, and stem bleed.
