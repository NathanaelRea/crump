# Sample transform workflows

## Reverse-wet-print-reverse

1. Select the destination syllable, chord, or phrase.
2. Reverse traversal.
3. Apply a fully wet delay/reverb/spectral chain.
4. Bounce with a deliberate tail.
5. Reverse the generated asset.
6. Align its endpoint to the dry onset and fade the boundary.

Use for reverse reverb, reverse delay, haunting layers, and pre-echoes.

## Exact-duration texture

1. Select and prepare a source region to an exact output frame count.
2. Set independent pitch and formant policy.
3. Apply texture/grain/warp behavior.
4. Bounce only if downstream slicing needs stable bytes.
5. Preserve the original and compare at matched level.

## Vocal chop instrument

1. Detect candidate boundaries, then review words/consonants manually.
2. Build exact non-overlapping slices.
3. Map slices to notes with one stable root policy.
4. Set attack/release, fades, choke, and voice replacement.
5. Author or seed the chop Pattern.
6. Add pitch layers, filtering, dynamics, delay, and reverb after timing works dry.

## Playable effect tail

1. Process one source hit or phrase fully wet.
2. Bounce enough tail for the intended instrument release.
3. Trim recognizable dry attack when desired.
4. Map the stable tail region to a sampler or granular Instrument.
5. Add a separate envelope and post effects.

## Drum extraction

1. Run transient analysis over the declared loop.
2. Review and correct every slice boundary.
3. Map hits by role and audition each raw/mapped.
4. Normalize only by explicit policy; otherwise retain source level.
5. Add choke/round-robin/velocity layers.
6. Write a new deterministic drum Pattern.

## Iterative resampling

1. Bounce generation N with exact provenance.
2. Apply one coherent edit family: reverse, micro-repeat, pitch, grain, or filtering.
3. Bounce generation N+1 only when stable bytes are useful.
4. Alternate source and derivative explicitly in arrangement.
5. Retain every parent identity; never overwrite the lineage root.

## Convolution texture

1. Import and verify source and impulse assets.
2. Choose normalization, trim, channel, latency, and tail policy explicitly.
3. Render or process through convolution.
4. Filter and envelope the result for its musical role.
5. Record impulse hash and algorithm identity in provenance.
