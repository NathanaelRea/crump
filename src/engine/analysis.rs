//! Offline analysis of finalized stereo float-WAV artifacts.
//!
//! Analysis deliberately reopens the completed file. Render-time bus telemetry is
//! not accepted as evidence for delivery metrics.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    EngineError,
    oversampling::{OVERSAMPLING_FACTOR, OVERSAMPLING_FILTER_TAPS, OversamplingInterpolator},
};

pub(super) const ANALYSIS_SCHEMA: u32 = 2;
const SUPPORTED_SAMPLE_RATE: u32 = 48_000;
const LOUDNESS_STEP_FRAMES: usize = 4_800;
const MOMENTARY_CHUNKS: usize = 4;
const SHORT_TERM_CHUNKS: usize = 30;
const SHORT_TERM_STEP_CHUNKS: usize = 1;
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
const LOUDNESS_OFFSET: f64 = -0.691;

/// Delivery metrics computed from the exact finalized WAV bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FinalizedAudioAnalysis {
    /// Analysis schema version, independent of the render-manifest schema.
    pub schema_version: u32,
    /// Exact decoded stereo frame count.
    pub analyzed_frames: u64,
    /// Largest absolute finite sample value.
    pub sample_peak: f64,
    /// Sample peak in dBFS, or `null` for digital silence.
    pub sample_peak_dbfs: Option<f64>,
    /// Largest reconstructed absolute sample value.
    pub true_peak: f64,
    /// Reconstructed true peak in dBTP, or `null` for digital silence.
    pub true_peak_dbtp: Option<f64>,
    /// Fixed reconstruction ratio used for true-peak analysis.
    pub true_peak_oversampling_factor: u8,
    /// Fixed reconstruction-kernel identity.
    pub true_peak_kernel: String,
    /// K-weighting, windowing, and gating method identity.
    pub loudness_method: String,
    /// K-weighted, absolute/relative-gated integrated loudness.
    pub integrated_lufs: Option<f64>,
    /// Maximum ungated three-second short-term loudness.
    pub maximum_short_term_lufs: Option<f64>,
    /// Gated 10th-to-95th-percentile short-term loudness range.
    pub loudness_range_lu: Option<f64>,
    /// Arithmetic mean of finite left and right samples.
    pub dc_offset: [f64; 2],
    /// Interleaved finite samples whose absolute value is at least full scale.
    pub clipped_sample_count: u64,
    /// Frames containing at least one clipped finite sample.
    pub clipped_frame_count: u64,
    /// Non-finite decoded samples. Published Crump renders require zero.
    pub non_finite_sample_count: u64,
    /// First frame containing a non-finite sample, when present.
    pub first_non_finite_frame: Option<u64>,
}

/// Reopens and analyzes one finalized Crump delivery WAV.
pub(crate) fn analyze_finalized_wav(path: &Path) -> Result<FinalizedAudioAnalysis, EngineError> {
    let mut reader = hound::WavReader::open(path)?;
    let specification = reader.spec();
    if specification.channels != 2
        || specification.sample_rate != SUPPORTED_SAMPLE_RATE
        || specification.bits_per_sample != 32
        || specification.sample_format != hound::SampleFormat::Float
    {
        return Err(EngineError::Compile(
            "finalized WAV analysis requires stereo 48 kHz IEEE 32-bit float Audio".to_owned(),
        ));
    }

    let analyzed_frames = u64::from(reader.duration());
    let mut samples = reader.samples::<f32>();
    let mut sample_peak = 0.0_f64;
    let mut true_peak = 0.0_f64;
    let mut interpolator = OversamplingInterpolator::default();
    let mut dc_sum = [0.0_f64; 2];
    let mut dc_count = [0_u64; 2];
    let mut clipped_sample_count = 0_u64;
    let mut clipped_frame_count = 0_u64;
    let mut non_finite_sample_count = 0_u64;
    let mut first_non_finite_frame = None;
    let mut k_weighting = [KWeighting::default(), KWeighting::default()];
    let mut chunk_energy = 0.0_f64;
    let mut chunk_frames = 0_usize;
    let mut loudness_chunks = Vec::new();

    for frame in 0..analyzed_frames {
        let mut decoded = [0.0_f32; 2];
        let mut clipped_frame = false;
        for channel in 0..2 {
            let sample = samples.next().ok_or_else(|| {
                EngineError::Compile(
                    "finalized WAV ended before its declared frame count".to_owned(),
                )
            })??;
            if sample.is_finite() {
                let magnitude = f64::from(sample.abs());
                sample_peak = sample_peak.max(magnitude);
                dc_sum[channel] += f64::from(sample);
                dc_count[channel] = dc_count[channel].saturating_add(1);
                if magnitude >= 1.0 {
                    clipped_sample_count = clipped_sample_count.saturating_add(1);
                    clipped_frame = true;
                }
                decoded[channel] = sample;
            } else {
                non_finite_sample_count = non_finite_sample_count.saturating_add(1);
                first_non_finite_frame.get_or_insert(frame);
            }
        }
        if clipped_frame {
            clipped_frame_count = clipped_frame_count.saturating_add(1);
        }

        for reconstructed in interpolator.process(decoded) {
            true_peak = true_peak
                .max(f64::from(reconstructed[0].abs()))
                .max(f64::from(reconstructed[1].abs()));
        }
        let weighted_left = k_weighting[0].process(f64::from(decoded[0]));
        let weighted_right = k_weighting[1].process(f64::from(decoded[1]));
        chunk_energy += weighted_left.mul_add(weighted_left, weighted_right * weighted_right);
        chunk_frames += 1;
        if chunk_frames == LOUDNESS_STEP_FRAMES {
            loudness_chunks.push(chunk_energy);
            chunk_energy = 0.0;
            chunk_frames = 0;
        }
    }

    if let Some(extra) = samples.next() {
        extra?;
        return Err(EngineError::Compile(
            "finalized WAV contains samples beyond its declared frame count".to_owned(),
        ));
    }

    let reconstruction_flush_frames =
        usize::from(OVERSAMPLING_FILTER_TAPS).div_ceil(usize::from(OVERSAMPLING_FACTOR));
    for _ in 0..reconstruction_flush_frames {
        for reconstructed in interpolator.process([0.0; 2]) {
            true_peak = true_peak
                .max(f64::from(reconstructed[0].abs()))
                .max(f64::from(reconstructed[1].abs()));
        }
    }

    let momentary_energies = window_energies(&loudness_chunks, MOMENTARY_CHUNKS, 1);
    let integrated_energy = gated_integrated_energy(&momentary_energies);
    let integrated_lufs = integrated_energy.and_then(energy_to_lufs);
    let short_term_energies =
        window_energies(&loudness_chunks, SHORT_TERM_CHUNKS, SHORT_TERM_STEP_CHUNKS);
    let maximum_short_term_lufs = short_term_energies
        .iter()
        .filter_map(|energy| energy_to_lufs(*energy))
        .max_by(f64::total_cmp);
    let loudness_range_lu = loudness_range(&short_term_energies);

    Ok(FinalizedAudioAnalysis {
        schema_version: ANALYSIS_SCHEMA,
        analyzed_frames,
        sample_peak,
        sample_peak_dbfs: amplitude_to_db(sample_peak),
        true_peak,
        true_peak_dbtp: amplitude_to_db(true_peak),
        true_peak_oversampling_factor: OVERSAMPLING_FACTOR,
        true_peak_kernel: format!(
            "crump-linear-phase-fir-{}x-{}tap-v1",
            OVERSAMPLING_FACTOR, OVERSAMPLING_FILTER_TAPS
        ),
        loudness_method: "crump-k-weighted-400ms-gated-3s-100ms-lra-v2".to_owned(),
        integrated_lufs,
        maximum_short_term_lufs,
        loudness_range_lu,
        dc_offset: [
            mean_or_zero(dc_sum[0], dc_count[0]),
            mean_or_zero(dc_sum[1], dc_count[1]),
        ],
        clipped_sample_count,
        clipped_frame_count,
        non_finite_sample_count,
        first_non_finite_frame,
    })
}

fn amplitude_to_db(amplitude: f64) -> Option<f64> {
    (amplitude > 0.0).then(|| 20.0 * amplitude.log10())
}

fn mean_or_zero(sum: f64, count: u64) -> f64 {
    if count == 0 { 0.0 } else { sum / count as f64 }
}

fn window_energies(chunks: &[f64], width: usize, step: usize) -> Vec<f64> {
    if chunks.len() < width {
        return Vec::new();
    }
    (0..=chunks.len() - width)
        .step_by(step)
        .map(|start| {
            chunks[start..start + width].iter().sum::<f64>() / (width * LOUDNESS_STEP_FRAMES) as f64
        })
        .collect()
}

fn gated_integrated_energy(energies: &[f64]) -> Option<f64> {
    let absolute_gate_energy = lufs_to_energy(ABSOLUTE_GATE_LUFS);
    let absolute = energies
        .iter()
        .copied()
        .filter(|energy| *energy >= absolute_gate_energy)
        .collect::<Vec<_>>();
    let absolute_mean = mean(&absolute)?;
    let relative_gate_energy = absolute_mean * 0.1;
    mean(
        &absolute
            .into_iter()
            .filter(|energy| *energy >= relative_gate_energy)
            .collect::<Vec<_>>(),
    )
}

fn loudness_range(short_term_energies: &[f64]) -> Option<f64> {
    let absolute_gate_energy = lufs_to_energy(ABSOLUTE_GATE_LUFS);
    let absolute = short_term_energies
        .iter()
        .copied()
        .filter(|energy| *energy >= absolute_gate_energy)
        .collect::<Vec<_>>();
    let absolute_mean = mean(&absolute)?;
    let relative_gate_energy = absolute_mean * 0.01;
    let mut loudness = absolute
        .into_iter()
        .filter(|energy| *energy >= relative_gate_energy)
        .filter_map(energy_to_lufs)
        .collect::<Vec<_>>();
    if loudness.is_empty() {
        return None;
    }
    loudness.sort_by(f64::total_cmp);
    Some(nearest_percentile(&loudness, 0.95) - nearest_percentile(&loudness, 0.10))
}

fn nearest_percentile(sorted: &[f64], quantile: f64) -> f64 {
    let index = ((sorted.len() - 1) as f64 * quantile).round() as usize;
    sorted[index]
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn energy_to_lufs(energy: f64) -> Option<f64> {
    (energy > 0.0).then(|| LOUDNESS_OFFSET + 10.0 * energy.log10())
}

fn lufs_to_energy(loudness: f64) -> f64 {
    10.0_f64.powf((loudness - LOUDNESS_OFFSET) / 10.0)
}

#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    const fn new(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let output = self.b0.mul_add(
            input,
            self.b1.mul_add(
                self.x1,
                self.b2
                    .mul_add(self.x2, -self.a1 * self.y1 - self.a2 * self.y2),
            ),
        );
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

#[derive(Debug, Clone, Copy)]
struct KWeighting {
    shelf: Biquad,
    high_pass: Biquad,
}

impl Default for KWeighting {
    fn default() -> Self {
        // ITU-R BS.1770 K-weighting coefficients specialized for 48 kHz.
        Self {
            shelf: Biquad::new(
                1.535_124_859_586_97,
                -2.691_696_189_406_38,
                1.198_392_810_852_85,
                -1.690_659_293_182_41,
                0.732_480_774_215_85,
            ),
            high_pass: Biquad::new(1.0, -2.0, 1.0, -1.990_047_454_833_98, 0.990_072_250_366_21),
        }
    }
}

impl KWeighting {
    fn process(&mut self, sample: f64) -> f64 {
        self.high_pass.process(self.shelf.process(sample))
    }
}

#[cfg(test)]
mod tests {
    use std::{f64::consts::PI, fs};

    use tempfile::tempdir;

    use super::*;

    fn write_wav(path: &Path, frames: &[[f32; 2]]) {
        let specification = hound::WavSpec {
            channels: 2,
            sample_rate: SUPPORTED_SAMPLE_RATE,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, specification).expect("create WAV");
        for frame in frames {
            writer.write_sample(frame[0]).expect("left sample");
            writer.write_sample(frame[1]).expect("right sample");
        }
        writer.finalize().expect("finalize WAV");
    }

    #[test]
    fn silence_has_explicit_null_loudness_and_zero_delivery_metrics() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("silence.wav");
        write_wav(&path, &vec![[0.0; 2]; 4 * SUPPORTED_SAMPLE_RATE as usize]);

        let analysis = analyze_finalized_wav(&path).expect("analyze silence");
        assert_eq!(analysis.analyzed_frames, 192_000);
        assert_eq!(analysis.sample_peak, 0.0);
        assert_eq!(analysis.true_peak, 0.0);
        assert_eq!(analysis.sample_peak_dbfs, None);
        assert_eq!(analysis.true_peak_dbtp, None);
        assert_eq!(analysis.integrated_lufs, None);
        assert_eq!(analysis.maximum_short_term_lufs, None);
        assert_eq!(analysis.loudness_range_lu, None);
        assert_eq!(analysis.dc_offset, [0.0; 2]);
        assert_eq!(analysis.clipped_sample_count, 0);
        assert_eq!(analysis.non_finite_sample_count, 0);
    }

    #[test]
    fn ebu_mode_stereo_tone_reports_the_published_loudness_tolerance() {
        // EBU Tech 3341 (2023), Table 1, test case 1.
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("ebu-mode-case-1.wav");
        let amplitude = 10.0_f64.powf(-23.0 / 20.0);
        let frames = (0..20 * SUPPORTED_SAMPLE_RATE as usize)
            .map(|frame| {
                let sample = (amplitude
                    * (2.0 * PI * 1_000.0 * frame as f64 / f64::from(SUPPORTED_SAMPLE_RATE)).sin())
                    as f32;
                [sample, sample]
            })
            .collect::<Vec<_>>();
        write_wav(&path, &frames);

        let analysis = analyze_finalized_wav(&path).expect("analyze EBU tone");
        assert_eq!(analysis.schema_version, ANALYSIS_SCHEMA);
        assert!((analysis.sample_peak - amplitude).abs() < 1.0e-6);
        assert!((analysis.true_peak - amplitude).abs() < 2.0e-3);
        let integrated = analysis.integrated_lufs.expect("integrated loudness");
        assert!((-23.1..=-22.9).contains(&integrated), "{integrated}");
        let short_term = analysis
            .maximum_short_term_lufs
            .expect("short-term loudness");
        assert!((-23.1..=-22.9).contains(&short_term), "{short_term}");
        assert!(analysis.loudness_range_lu.expect("loudness range") < 0.1);
        assert!(analysis.dc_offset[0].abs() < 1.0e-8);
        assert!(analysis.dc_offset[1].abs() < 1.0e-8);
        assert_eq!(analysis.clipped_sample_count, 0);
    }

    #[test]
    fn loudness_range_uses_the_absolute_gated_short_term_mean_and_nearest_percentiles() {
        // EBU Tech 3342 (2023), section 3.1 and Appendix 1 reference algorithm.
        let short_term_energies = [-80.0, -50.0, -40.0, -30.0, -20.0, -10.0].map(lufs_to_energy);
        let range = loudness_range(&short_term_energies).expect("Loudness Range");
        assert!((range - 20.0).abs() < 1.0e-12, "{range}");

        let chunks = vec![f64::from(LOUDNESS_STEP_FRAMES as u32); SHORT_TERM_CHUNKS + 1];
        assert_eq!(
            window_energies(&chunks, SHORT_TERM_CHUNKS, SHORT_TERM_STEP_CHUNKS).len(),
            2,
            "three-second Short-term windows must advance at the required 10 Hz"
        );
    }

    #[test]
    fn reconstructed_peak_passes_the_published_ebu_steady_tone_cases() {
        // EBU Tech 3341 (2023), Table 1, true-peak test cases 15-19.
        let directory = tempdir().expect("temporary directory");
        let cases = [
            (15, 12_000.0_f64, 0.0_f64, 0.50_f64, -6.0_f64),
            (16, 12_000.0, 45.0, 0.50, -6.0),
            (17, 8_000.0, 60.0, 0.50, -6.0),
            (18, 6_000.0, 67.5, 0.50, -6.0),
            (19, 12_000.0, 45.0, 1.41, 3.0),
        ];
        let frame_count = SUPPORTED_SAMPLE_RATE as usize / 5;
        let fade_frames = SUPPORTED_SAMPLE_RATE as usize / 100;
        for (case, frequency, phase_degrees, amplitude, expected_dbtp) in cases {
            let path = directory.path().join(format!("ebu-mode-case-{case}.wav"));
            let phase = phase_degrees.to_radians();
            let frames = (0..frame_count)
                .map(|frame| {
                    let fade_in = frame as f64 / fade_frames as f64;
                    let fade_out = (frame_count - 1 - frame) as f64 / fade_frames as f64;
                    let envelope = fade_in.min(fade_out).min(1.0);
                    let sample = (amplitude
                        * envelope
                        * (2.0 * PI * frequency * frame as f64 / f64::from(SUPPORTED_SAMPLE_RATE)
                            + phase)
                            .sin()) as f32;
                    [sample, sample]
                })
                .collect::<Vec<_>>();
            write_wav(&path, &frames);

            let analysis = analyze_finalized_wav(&path).expect("analyze EBU true-peak tone");
            let measured = analysis.true_peak_dbtp.expect("reconstructed peak");
            assert!(
                (expected_dbtp - 0.4..=expected_dbtp + 0.2).contains(&measured),
                "EBU case {case}: expected {expected_dbtp} +0.2/-0.4 dBTP, got {measured}"
            );
        }
    }

    #[test]
    fn oversampled_peak_finds_an_intersample_overload_without_sample_clipping() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("intersample.wav");
        let frames = (0..4 * SUPPORTED_SAMPLE_RATE as usize)
            .map(|frame| {
                let sample = (2.0 * PI * 12_000.0 * frame as f64 / f64::from(SUPPORTED_SAMPLE_RATE)
                    + std::f64::consts::FRAC_PI_4)
                    .sin() as f32
                    * 1.2;
                [sample, sample]
            })
            .collect::<Vec<_>>();
        write_wav(&path, &frames);

        let analysis = analyze_finalized_wav(&path).expect("analyze intersample vector");
        assert!(analysis.sample_peak < 0.9);
        assert!(analysis.true_peak > 1.1, "{}", analysis.true_peak);
        assert_eq!(analysis.clipped_sample_count, 0);
        assert_eq!(analysis.clipped_frame_count, 0);
    }

    #[test]
    fn dc_clipping_and_non_finite_samples_are_counted_from_decoded_bytes() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("counts.wav");
        write_wav(&path, &[[0.25, 0.25], [1.0, -1.2], [f32::NAN, 0.25]]);

        let analysis = analyze_finalized_wav(&path).expect("analyze count vector");
        assert_eq!(analysis.sample_peak, f64::from(1.2_f32));
        assert_eq!(analysis.clipped_sample_count, 2);
        assert_eq!(analysis.clipped_frame_count, 1);
        assert_eq!(analysis.non_finite_sample_count, 1);
        assert_eq!(analysis.first_non_finite_frame, Some(2));
        assert!((analysis.dc_offset[0] - 0.625).abs() < 1.0e-9);
        assert!((analysis.dc_offset[1] + 0.7 / 3.0).abs() < 1.0e-7);
    }

    #[test]
    fn truncated_and_corrupt_finalized_wavs_are_rejected() {
        let directory = tempdir().expect("temporary directory");
        let truncated = directory.path().join("truncated.wav");
        write_wav(&truncated, &vec![[0.1; 2]; 48_000]);
        let mut bytes = fs::read(&truncated).expect("read WAV");
        bytes.truncate(bytes.len() - 16);
        fs::write(&truncated, bytes).expect("truncate WAV");
        assert!(analyze_finalized_wav(&truncated).is_err());

        let corrupt = directory.path().join("corrupt.wav");
        fs::write(&corrupt, b"not a RIFF/WAVE file").expect("corrupt WAV");
        assert!(analyze_finalized_wav(&corrupt).is_err());
    }
}
