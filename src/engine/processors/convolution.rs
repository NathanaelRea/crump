use std::sync::Arc;

use super::{ReverbPreDelay, Scalar};
use crate::engine::{
    EngineError,
    fft::{Complex, fft, multiply},
    sample::{CompiledSampleSource, converted_frame_count},
};
use crate::media::WavMetadata;

const PARTITION: usize = 256;
const FFT_SIZE: usize = 2 * PARTITION;
// Only the unique real-signal bins are retained in the frequency delay line.
const BINS: usize = PARTITION + 1;
pub(crate) const MAX_IR_SECONDS: u32 = 10;

pub(crate) fn ir_frames(metadata: &WavMetadata, rate: u32) -> Result<usize, String> {
    if !matches!(metadata.channels, 1 | 2) || metadata.frames == 0 {
        return Err("impulse response must be a nonempty mono or stereo WAV".into());
    }
    let frames = usize::try_from(metadata.frames)
        .ok()
        .and_then(|frames| converted_frame_count(frames, metadata.sample_rate, rate).ok())
        .filter(|&frames| frames > 0 && frames <= rate as usize * MAX_IR_SECONDS as usize);
    frames
        .ok_or_else(|| format!("impulse response exceeds {MAX_IR_SECONDS} seconds at Project rate"))
}

fn partitions(frames: usize) -> usize {
    frames.div_ceil(PARTITION).saturating_sub(1)
}

pub(super) fn tail_frames(frames: usize, rate: u32) -> u32 {
    // Include the final overlap block, which can retain FFT roundoff in the
    // zero-padded region, and the full automatable pre-delay range.
    let convolution = if partitions(frames) == 0 {
        frames - 1
    } else {
        (frames.div_ceil(PARTITION) + 1) * PARTITION - 1
    };
    convolution as u32 + rate
}

pub(super) fn estimate(frames: usize, rate: u32) -> (u64, u64) {
    let count = partitions(frames) as u64;
    let spectra = 4 * count * BINS as u64 * size_of::<Complex>() as u64;
    let fixed = (3 * PARTITION * size_of::<[f64; 2]>()
        + (FFT_SIZE + 2 * BINS) * size_of::<Complex>()
        + (rate as usize + 2) * size_of::<[f32; 2]>()
        + size_of::<Kernel>()
        + size_of::<ConvolutionState>()
        + 4 * size_of::<usize>()) as u64;
    // Two direct dot products, four transforms per partition, and stereo complex MACs.
    (spectra + fixed, 4 * PARTITION as u64 + 640 + 16 * count)
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ConvolutionProgram {
    pub(super) mix: Scalar,
    pub(super) pre_delay: Scalar,
    pub(super) level: Scalar,
    pub(super) width: Scalar,
    pub(super) frames: usize,
    pub(super) kernel: Option<Arc<Kernel>>,
}

#[derive(Debug)]
pub(super) struct Kernel {
    identity: [u8; 32],
    cuts: [f64; 2],
    head: Vec<[f64; 2]>,
    spectra: [Vec<Complex>; 2],
}

impl PartialEq for Kernel {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity && self.cuts == other.cuts
    }
}

impl Kernel {
    pub(super) fn compile(
        source: &CompiledSampleSource,
        rate: u32,
        cuts: [f64; 2],
    ) -> Result<(Arc<Self>, usize), EngineError> {
        if source.is_empty() || source.len() > rate as usize * MAX_IR_SECONDS as usize {
            return Err(EngineError::Compile(format!(
                "impulse response `{}` must contain 1 frame through {MAX_IR_SECONDS} seconds at Project rate",
                source.asset_id()
            )));
        }
        let frames = source.len() + filter_padding(cuts, rate);
        let mut head = vec![[0.0; 2]; frames.min(PARTITION)];
        let mut spectra = std::array::from_fn(|_| Vec::with_capacity(partitions(frames) * BINS));
        let mut scratch = vec![Complex::default(); FFT_SIZE];
        for channel in 0..2 {
            let [hp, lp] = cuts.map(|hz| {
                -(-std::f64::consts::TAU * hz.min(f64::from(rate) * 0.49) / f64::from(rate))
                    .exp_m1()
            });
            let mut low = 0.0;
            let mut high = 0.0;
            for partition in 0..frames.div_ceil(PARTITION) {
                scratch.fill(Complex::default());
                let start = partition * PARTITION;
                for (offset, bin) in scratch[..PARTITION]
                    .iter_mut()
                    .take(frames - start)
                    .enumerate()
                {
                    let mut value =
                        f64::from(source.frame(start + offset).unwrap_or_default()[channel]);
                    if hp > 0.0 {
                        low += hp * (value - low);
                        value -= low;
                    }
                    if lp > 0.0 {
                        high += lp * (value - high);
                        value = high;
                    }
                    if partition == 0 {
                        head[offset][channel] = value;
                    } else {
                        bin.re = value;
                    }
                }
                if partition != 0 {
                    fft(&mut scratch, false);
                    spectra[channel].extend_from_slice(&scratch[..BINS]);
                }
            }
        }
        Ok((
            Arc::new(Self {
                identity: source.cache_identity(),
                cuts,
                head,
                spectra,
            }),
            frames,
        ))
    }
}

pub(super) fn filter_padding(cuts: [f64; 2], rate: u32) -> usize {
    if cuts == [0.0; 2] { 0 } else { rate as usize }
}

#[derive(Debug, Clone)]
pub(super) struct ConvolutionState {
    head_history: Vec<[f64; 2]>,
    input_spectra: [Vec<Complex>; 2],
    scratch: Vec<Complex>,
    sums: [Vec<Complex>; 2],
    next: Vec<[f64; 2]>,
    overlap: Vec<[f64; 2]>,
    offset: usize,
    block: usize,
    delay: ReverbPreDelay,
}

impl ConvolutionState {
    pub(super) fn new(frames: usize, rate: u32) -> Self {
        Self {
            head_history: vec![[0.0; 2]; PARTITION],
            input_spectra: std::array::from_fn(|_| {
                vec![Complex::default(); partitions(frames) * BINS]
            }),
            scratch: vec![Complex::default(); FFT_SIZE],
            sums: std::array::from_fn(|_| vec![Complex::default(); BINS]),
            next: vec![[0.0; 2]; PARTITION],
            overlap: vec![[0.0; 2]; PARTITION],
            offset: 0,
            block: 0,
            delay: ReverbPreDelay::new(rate),
        }
    }

    #[cfg(test)]
    pub(super) fn storage(&self) -> impl Iterator<Item = (*const (), usize)> {
        [
            (
                self.head_history.as_ptr().cast(),
                self.head_history.capacity(),
            ),
            (self.scratch.as_ptr().cast(), self.scratch.capacity()),
            (self.next.as_ptr().cast(), self.next.capacity()),
            (self.overlap.as_ptr().cast(), self.overlap.capacity()),
            (
                self.delay.buffer.as_ptr().cast(),
                self.delay.buffer.capacity(),
            ),
        ]
        .into_iter()
        .chain(
            self.input_spectra
                .iter()
                .chain(&self.sums)
                .map(|v| (v.as_ptr().cast(), v.capacity())),
        )
    }

    pub(super) fn reset(&mut self) {
        self.head_history.fill([0.0; 2]);
        for spectrum in &mut self.input_spectra {
            spectrum.fill(Complex::default());
        }
        for sum in &mut self.sums {
            sum.fill(Complex::default());
        }
        self.next.fill([0.0; 2]);
        self.overlap.fill([0.0; 2]);
        self.offset = 0;
        self.block = 0;
        self.delay.reset();
    }

    pub(super) fn process(
        &mut self,
        input: [f32; 2],
        program: &ConvolutionProgram,
        controls: &[f64],
        rate: u32,
    ) -> [f32; 2] {
        let kernel = program.kernel.as_ref().expect("runtime convolution kernel");
        self.head_history[self.offset] = input.map(f64::from);
        let mut wet = self.next[self.offset];
        for (tap, coefficient) in kernel.head.iter().enumerate() {
            let history = self.head_history[(self.offset + PARTITION - tap) % PARTITION];
            wet[0] += coefficient[0] * history[0];
            wet[1] += coefficient[1] * history[1];
        }
        self.accumulate_history(kernel);
        self.offset += 1;
        if self.offset == PARTITION {
            self.offset = 0;
            self.finish_partition(kernel);
        }
        let wet = self.delay.process(
            wet.map(|value| value as f32),
            program.pre_delay.value(controls),
            rate,
        );
        let mid = (wet[0] + wet[1]) * 0.5;
        let side = (wet[0] - wet[1]) * 0.5 * program.width.value(controls) as f32;
        let mix = program.mix.value(controls) as f32;
        let gain = 10.0_f32.powf(program.level.value(controls) as f32 / 20.0);
        let widened = if program.width.value(controls) == 1.0 {
            wet
        } else {
            [mid + side, mid - side]
        };
        std::array::from_fn(|channel| {
            gain * (input[channel] * (1.0 - mix) + widened[channel] * mix)
        })
    }

    fn accumulate_history(&mut self, kernel: &Kernel) {
        let count = self.input_spectra[0].len() / BINS;
        let older = count.saturating_sub(1);
        // All but the newest input spectrum are ready at the start of the block.
        // Spread their multiply-accumulates over the block in increasing IR order.
        let start = 1 + older * self.offset / PARTITION;
        let end = 1 + older * (self.offset + 1) / PARTITION;
        for partition in start..end {
            let index = (self.block + count - partition) % count;
            for channel in 0..2 {
                accumulate(
                    &mut self.sums[channel],
                    &self.input_spectra[channel][index * BINS..(index + 1) * BINS],
                    &kernel.spectra[channel][partition * BINS..(partition + 1) * BINS],
                );
            }
        }
    }

    fn finish_partition(&mut self, kernel: &Kernel) {
        let count = self.input_spectra[0].len() / BINS;
        if count == 0 {
            return;
        }
        // IR partition one starts on the next sample. The just-completed input
        // partition therefore supplies its spectrum in time for zero latency.
        for channel in 0..2 {
            self.scratch.fill(Complex::default());
            for (bin, frame) in self.scratch.iter_mut().zip(&self.head_history) {
                bin.re = frame[channel];
            }
            fft(&mut self.scratch, false);
            let history = &mut self.input_spectra[channel];
            history[self.block * BINS..(self.block + 1) * BINS]
                .copy_from_slice(&self.scratch[..BINS]);
            let sum = &mut self.sums[channel];
            accumulate(sum, &self.scratch[..BINS], &kernel.spectra[channel][..BINS]);
            self.scratch[..BINS].copy_from_slice(sum);
            sum.fill(Complex::default());
            for bin in 1..PARTITION {
                let value = self.scratch[bin];
                self.scratch[FFT_SIZE - bin] = Complex {
                    re: value.re,
                    im: -value.im,
                };
            }
            fft(&mut self.scratch, true);
            for frame in 0..PARTITION {
                self.next[frame][channel] = self.scratch[frame].re + self.overlap[frame][channel];
                self.overlap[frame][channel] = self.scratch[frame + PARTITION].re;
            }
        }
        self.block = (self.block + 1) % count;
    }
}

fn accumulate(sum: &mut [Complex], input: &[Complex], coefficients: &[Complex]) {
    for ((sum, &input), &coefficient) in sum.iter_mut().zip(input).zip(coefficients) {
        let product = multiply(input, coefficient);
        sum.re += product.re;
        sum.im += product.im;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        ir: &[[f32; 2]],
        cuts: [f64; 2],
        mut program: ConvolutionProgram,
        block: usize,
    ) -> Vec<[f32; 2]> {
        let source = CompiledSampleSource::from_test_frames(ir.to_vec());
        let (kernel, frames) = Kernel::compile(&source, 48_000, cuts).unwrap();
        program.kernel = Some(kernel);
        program.frames = frames;
        let mut state = ConvolutionState::new(frames, 48_000);
        let mut audio = vec![[0.0; 2]; frames + 2 * PARTITION];
        audio[0] = [1.0; 2];
        for chunk in audio.chunks_mut(block) {
            for sample in chunk {
                *sample = state.process(*sample, &program, &[], 48_000);
            }
        }
        audio
    }

    fn fully_wet() -> ConvolutionProgram {
        ConvolutionProgram {
            mix: Scalar::Constant(1.0),
            pre_delay: Scalar::Constant(0.0),
            level: Scalar::Constant(0.0),
            width: Scalar::Constant(1.0),
            frames: 0,
            kernel: None,
        }
    }

    #[test]
    fn impulse_reproduces_stereo_ir_across_partition_boundaries() {
        let ir: Vec<_> = (0..(5 * PARTITION + 17))
            .map(|index| {
                [
                    (index as f32 * 0.43).sin() * 0.2,
                    (index as f32 * 0.14).cos() * 0.3,
                ]
            })
            .collect();
        let reference = run(&ir, [0.0; 2], fully_wet(), 64);
        for (index, frame) in reference.iter().enumerate() {
            let expected = ir.get(index).copied().unwrap_or_default();
            for channel in 0..2 {
                assert!(
                    (frame[channel] - expected[channel]).abs() < 1.0e-6,
                    "frame {index}"
                );
            }
        }
        for block in [128, 256] {
            let output = run(&ir, [0.0; 2], fully_wet(), block);
            assert!(
                output
                    .iter()
                    .zip(&reference)
                    .all(|(a, b)| a.map(f32::to_bits) == b.map(f32::to_bits))
            );
        }
    }

    #[test]
    fn parallel_mix_predelay_output_gain_and_wet_cuts() {
        let ir = [[0.75, -0.5]];
        let program = ConvolutionProgram {
            mix: Scalar::Constant(0.5),
            pre_delay: Scalar::Constant(0.022),
            level: Scalar::Constant(-6.020_599_913),
            ..fully_wet()
        };
        let mut extended_ir = vec![[0.0; 2]; PARTITION * 5];
        extended_ir[0] = ir[0];
        let output = run(&extended_ir, [0.0; 2], program, 128);
        assert_eq!(output[0], [0.25; 2]);
        assert!(output[1..1056].iter().all(|&frame| frame == [0.0; 2]));
        assert_eq!(output[1056], [0.1875, -0.125]);

        let hp = run(&ir, [200.0, 0.0], fully_wet(), 64);
        assert!(hp[0][0] > 0.7 && hp[1][0] < 0.0);
        assert!(
            hp.iter()
                .map(|frame| f64::from(frame[0]))
                .sum::<f64>()
                .abs()
                < 1.0e-6
        );
        let lp = run(&ir, [0.0, 5_000.0], fully_wet(), 64);
        assert!(lp[0][0] > 0.0 && lp[0][0] < 0.5 && lp[1][0] > 0.0);
        assert!((lp.iter().map(|frame| f64::from(frame[0])).sum::<f64>() - 0.75).abs() < 1.0e-6);
    }

    #[test]
    fn project_admission_and_inspection_use_the_converted_wav_length() {
        use crate::{dsl::parse_project, engine::CompiledProject, media::load_asset_inventory};
        use sha2::{Digest, Sha256};

        let root = tempfile::tempdir().unwrap();
        let wav = root.path().join("hall.wav");
        let write_wav = |frames| {
            let mut writer = hound::WavWriter::create(
                &wav,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 24_000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for index in 0..frames {
                let sample = if index == 0 || index == frames - 1 {
                    16_384_i16
                } else {
                    0
                };
                writer.write_sample(sample).unwrap();
            }
            writer.finalize().unwrap();
        };
        let parse = || {
            let digest: String = Sha256::digest(std::fs::read(&wav).unwrap())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            let source = include_str!("../../../tests/fixtures/production_mix.crump")
                .replace("crump 1", "crump 2")
                .replace("  song {", &format!("  asset hall \"Hall\" wav \"hall.wav\" sha256 \"{digest}\" {{\n    usage impulse_response\n  }}\n  song {{"))
                .replace(
                    "processor room_reverb reverb {\n        setting pre_delay 0.01s\n        setting diffusion 0.7\n        setting decay 0.9\n        setting damping 0.35\n        setting modulation 0.1",
                    "processor room_reverb convolution {\n        setting ir hall",
                );
            let parsed = parse_project(&root.path().join("project.crump"), &source);
            assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
            let project = parsed.project.unwrap();
            let (inventory, diagnostics) = load_asset_inventory(root.path(), &project.assets);
            assert!(!diagnostics.has_errors(), "{diagnostics:?}");
            (project, inventory)
        };

        for frames in [300, 24_000] {
            write_wav(frames);
            let (project, inventory) = parse();
            let compiled = CompiledProject::compile_with_assets(&project, &inventory).unwrap();
            let inspection = compiled
                .mix_graph_inspection()
                .processors
                .iter()
                .find(|processor| processor.kind == crate::model::ProcessorKind::Convolution)
                .unwrap();
            let converted = frames as usize * 2;
            assert_eq!(inspection.intrinsic_latency_frames, 0);
            assert!(inspection.tail_frames_upper_bound.unwrap() >= 48_000 + converted as u32 - 1);
            let (bytes, work) = estimate(converted, 48_000);
            assert!(inspection.state_bytes > bytes);
            assert_eq!(inspection.work_units_per_frame, work);

            if frames == 300 {
                let process = |quantum| {
                    let mut runtime = compiled.runtime();
                    let mut output = vec![[0.0; 2]; 4096];
                    let allocations = allocation_counter::measure(|| {
                        for block in output.chunks_mut(quantum) {
                            runtime
                                .process_block(block, crate::engine::ProcessingMode::Offline)
                                .unwrap();
                        }
                    });
                    assert_eq!(allocations.count_total, 0);
                    output
                        .into_iter()
                        .map(|frame| frame.map(f32::to_bits))
                        .collect::<Vec<_>>()
                };
                let reference = process(64);
                assert_eq!(reference, process(128));
                assert_eq!(reference, process(256));
            }
        }

        write_wav(240_001);
        let (project, inventory) = parse();
        let diagnostics = crate::validate::validate_project_assets(&project, &inventory);
        assert!(diagnostics.has_errors());
        assert!(format!("{diagnostics:?}").contains("exceeds 10 seconds"));
    }
}
