//! Focused, non-production workload probe for the qualification-max-v1 contract.

use std::{f64::consts::TAU, fs, hint::black_box, mem::size_of, time::Instant};

const SAMPLE_RATE: f64 = 48_000.0;
const FRAME_DEADLINE_NS: f64 = 1_000_000_000.0 / SAMPLE_RATE;
const VOICES: usize = 8;
const WAVETABLE_LANES_PER_VOICE: usize = 16;
const GRAINS_PER_VOICE: usize = 64;
const SPECTRAL_PEAKS_PER_VOICE: usize = 128;
const FILTERS_PER_VOICE: usize = 8;
const ROUTES_PER_VOICE: usize = 64;
const TABLE_LENGTH: usize = 2_048;
const WINDOW_LENGTH: usize = 1_024;

#[derive(Clone, Copy)]
struct GrainState {
    position: f64,
    increment: f64,
    window_position: f64,
    window_increment: f64,
    pan: f32,
    gain: f32,
}

#[derive(Clone, Copy)]
struct SpectralPeakState {
    real: f64,
    imaginary: f64,
    rotation_real: f64,
    rotation_imaginary: f64,
    gain: f64,
}

#[derive(Clone, Copy)]
struct FilterState {
    low_left: f64,
    band_left: f64,
    low_right: f64,
    band_right: f64,
}

fn elapsed_ns_per_frame(frames: usize, operation: impl FnOnce() -> f64) -> (f64, f64) {
    let started = Instant::now();
    let checksum = operation();
    let elapsed = started.elapsed().as_secs_f64() * 1_000_000_000.0;
    (elapsed / frames as f64, checksum)
}

fn wavetable_probe(frames: usize, table: &[f32]) -> (f64, f64) {
    let mut phases = vec![0.0_f64; VOICES * WAVETABLE_LANES_PER_VOICE];
    elapsed_ns_per_frame(frames, || {
        let mut checksum = 0.0_f64;
        for frame in 0..frames {
            for (lane, phase) in phases.iter_mut().enumerate() {
                let increment = (55.0 + (lane % 32) as f64 * 7.0) / SAMPLE_RATE;
                *phase = (*phase + increment).fract();
                let read = *phase * TABLE_LENGTH as f64;
                let left = read.floor() as usize & (TABLE_LENGTH - 1);
                let right = (left + 1) & (TABLE_LENGTH - 1);
                let fraction = read - read.floor();
                checksum +=
                    f64::from(table[left]) * (1.0 - fraction) + f64::from(table[right]) * fraction;
            }
            checksum *= 0.999_999 + (frame & 1) as f64 * 0.000_001;
        }
        black_box(checksum)
    })
}

fn granular_probe(frames: usize, sample: &[f32], window: &[f32]) -> (f64, f64) {
    let mut grains = (0..VOICES * GRAINS_PER_VOICE)
        .map(|index| GrainState {
            position: (index * 37 % sample.len()) as f64,
            increment: 0.5 + (index % 17) as f64 / 16.0,
            window_position: (index * 13 % window.len()) as f64,
            window_increment: 0.7 + (index % 5) as f64 * 0.1,
            pan: (index % 31) as f32 / 15.0 - 1.0,
            gain: 1.0 / GRAINS_PER_VOICE as f32,
        })
        .collect::<Vec<_>>();
    elapsed_ns_per_frame(frames, || {
        let mut checksum = 0.0_f64;
        for _ in 0..frames {
            for grain in &mut grains {
                let source_index = grain.position as usize % (sample.len() - 1);
                let source_fraction = grain.position - grain.position.floor();
                let source = f64::from(sample[source_index]) * (1.0 - source_fraction)
                    + f64::from(sample[source_index + 1]) * source_fraction;
                let window_index = grain.window_position as usize % window.len();
                let pan_gain = 1.0 - f64::from(grain.pan.abs()) * 0.25;
                checksum +=
                    source * f64::from(window[window_index]) * f64::from(grain.gain) * pan_gain;
                grain.position += grain.increment;
                if grain.position >= sample.len() as f64 {
                    grain.position -= sample.len() as f64;
                }
                grain.window_position += grain.window_increment;
                if grain.window_position >= window.len() as f64 {
                    grain.window_position -= window.len() as f64;
                }
            }
        }
        black_box(checksum)
    })
}

fn spectral_probe(frames: usize) -> (f64, f64) {
    let mut peaks = (0..VOICES * SPECTRAL_PEAKS_PER_VOICE)
        .map(|index| {
            let frequency = 40.0 + (index % SPECTRAL_PEAKS_PER_VOICE) as f64 * 37.0;
            let rotation = TAU * frequency / SAMPLE_RATE;
            SpectralPeakState {
                real: 1.0,
                imaginary: 0.0,
                rotation_real: rotation.cos(),
                rotation_imaginary: rotation.sin(),
                gain: 1.0 / SPECTRAL_PEAKS_PER_VOICE as f64,
            }
        })
        .collect::<Vec<_>>();
    elapsed_ns_per_frame(frames, || {
        let mut checksum = 0.0_f64;
        for _ in 0..frames {
            for peak in &mut peaks {
                let next_real =
                    peak.real * peak.rotation_real - peak.imaginary * peak.rotation_imaginary;
                let next_imaginary =
                    peak.real * peak.rotation_imaginary + peak.imaginary * peak.rotation_real;
                peak.real = next_real;
                peak.imaginary = next_imaginary;
                checksum += peak.real * peak.gain;
            }
        }
        black_box(checksum)
    })
}

fn filter_and_route_probe(frames: usize) -> (f64, f64) {
    let mut filters = vec![
        FilterState {
            low_left: 0.0,
            band_left: 0.0,
            low_right: 0.0,
            band_right: 0.0,
        };
        VOICES * FILTERS_PER_VOICE
    ];
    let mut route_values = vec![0.0_f64; VOICES * ROUTES_PER_VOICE];
    elapsed_ns_per_frame(frames, || {
        let mut checksum = 0.0_f64;
        for frame in 0..frames {
            for (index, route) in route_values.iter_mut().enumerate() {
                let source = ((frame + index) & 2_047) as f64 / 1_023.5 - 1.0;
                let curved = source.signum() * source.abs().powi(2);
                *route = (*route + curved * 0.001).clamp(-1.0, 1.0);
                checksum += *route;
            }
            let input = ((frame & 255) as f64 / 127.5 - 1.0) * 0.1;
            for filter in &mut filters {
                let high_left = input - filter.low_left - 0.2 * filter.band_left;
                filter.band_left += 0.08 * high_left;
                filter.low_left += 0.08 * filter.band_left;
                let high_right = -input - filter.low_right - 0.2 * filter.band_right;
                filter.band_right += 0.08 * high_right;
                filter.low_right += 0.08 * filter.band_right;
                checksum += filter.low_left + filter.low_right;
            }
        }
        black_box(checksum)
    })
}

fn combined_probe(frames: usize, table: &[f32], sample: &[f32], window: &[f32]) -> (f64, f64) {
    let (wavetable, checksum_a) = wavetable_probe(frames, table);
    let (granular, checksum_b) = granular_probe(frames, sample, window);
    let (spectral, checksum_c) = spectral_probe(frames);
    let (filter_route, checksum_d) = filter_and_route_probe(frames);
    (
        wavetable + granular + spectral + filter_route,
        checksum_a + checksum_b + checksum_c + checksum_d,
    )
}

fn cpu_model() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("model name\t: ")
                    .map(str::trim)
                    .map(str::to_owned)
            })
        })
        .unwrap_or_else(|| "unavailable".to_owned())
}

fn json_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn main() {
    let table = (0..TABLE_LENGTH)
        .map(|index| (TAU * index as f64 / TABLE_LENGTH as f64).sin() as f32)
        .collect::<Vec<_>>();
    let sample = (0..48_000)
        .map(|index| {
            ((TAU * 173.0 * index as f64 / SAMPLE_RATE).sin()
                + (TAU * 521.0 * index as f64 / SAMPLE_RATE).sin() * 0.3) as f32
        })
        .collect::<Vec<_>>();
    let window = (0..WINDOW_LENGTH)
        .map(|index| (0.5 - 0.5 * (TAU * index as f64 / (WINDOW_LENGTH - 1) as f64).cos()) as f32)
        .collect::<Vec<_>>();

    let frames = 24_000;
    let (wavetable_ns, checksum_a) = wavetable_probe(frames, &table);
    let (granular_ns, checksum_b) = granular_probe(frames, &sample, &window);
    let (spectral_ns, checksum_c) = spectral_probe(frames);
    let (filter_route_ns, checksum_d) = filter_and_route_probe(frames);
    let (combined_ns, checksum_e) = combined_probe(frames, &table, &sample, &window);

    let mutable_voice_bytes = (WAVETABLE_LANES_PER_VOICE * size_of::<f64>())
        + (GRAINS_PER_VOICE * size_of::<GrainState>())
        + (SPECTRAL_PEAKS_PER_VOICE * size_of::<SpectralPeakState>())
        + (FILTERS_PER_VOICE * size_of::<FilterState>())
        + (ROUTES_PER_VOICE * size_of::<f64>());
    let mutable_runtime_bytes = mutable_voice_bytes * VOICES * 2;
    let immutable_program_bytes = 8_388_608_usize;
    let shared_state_bytes = 4_194_304_usize;
    let publication_peak_bytes =
        (mutable_runtime_bytes + immutable_program_bytes + shared_state_bytes) * 2;
    let work_units_per_frame = 36_864_u64;
    let utilization = combined_ns / FRAME_DEADLINE_NS;
    let checksum = checksum_a + checksum_b + checksum_c + checksum_d + checksum_e;

    let rustc = option_env!("RUSTC_VERSION").unwrap_or("unavailable");
    let rustc_flags = option_env!("RUSTC_FLAGS").unwrap_or("unavailable");
    let probe_source_sha256 = option_env!("PROBE_SOURCE_SHA256").unwrap_or("unavailable");
    let host = option_env!("HOST_IDENTITY").unwrap_or("unavailable");
    println!(
        concat!(
            "{{\n",
            "  \"schema_version\": 1,\n",
            "  \"profile\": \"qualification-max-v1\",\n",
            "  \"sample_rate\": 48000,\n",
            "  \"measured_frames\": {frames},\n",
            "  \"cpu_model\": \"{cpu}\",\n",
            "  \"host_identity\": \"{host}\",\n",
            "  \"rustc\": \"{rustc}\",\n",
            "  \"rustc_flags\": \"{rustc_flags}\",\n",
            "  \"probe_source_sha256\": \"{probe_source_sha256}\",\n",
            "  \"wavetable_ns_per_frame\": {wavetable:.3},\n",
            "  \"granular_ns_per_frame\": {granular:.3},\n",
            "  \"spectral_ns_per_frame\": {spectral:.3},\n",
            "  \"filter_route_ns_per_frame\": {filter_route:.3},\n",
            "  \"combined_ns_per_frame\": {combined:.3},\n",
            "  \"frame_deadline_ns\": {deadline:.3},\n",
            "  \"prototype_utilization\": {utilization:.6},\n",
            "  \"mutable_voice_bytes\": {mutable_voice_bytes},\n",
            "  \"mutable_runtime_bytes\": {mutable_runtime_bytes},\n",
            "  \"immutable_program_bytes\": {immutable_program_bytes},\n",
            "  \"shared_state_bytes\": {shared_state_bytes},\n",
            "  \"publication_peak_bytes\": {publication_peak_bytes},\n",
            "  \"work_units_per_frame\": {work_units_per_frame},\n",
            "  \"checksum\": {checksum:.9}\n",
            "}}"
        ),
        frames = frames,
        cpu = json_string(&cpu_model()),
        host = json_string(host),
        rustc = json_string(rustc),
        rustc_flags = json_string(rustc_flags),
        probe_source_sha256 = json_string(probe_source_sha256),
        wavetable = wavetable_ns,
        granular = granular_ns,
        spectral = spectral_ns,
        filter_route = filter_route_ns,
        combined = combined_ns,
        deadline = FRAME_DEADLINE_NS,
        utilization = utilization,
        mutable_voice_bytes = mutable_voice_bytes,
        mutable_runtime_bytes = mutable_runtime_bytes,
        immutable_program_bytes = immutable_program_bytes,
        shared_state_bytes = shared_state_bytes,
        publication_peak_bytes = publication_peak_bytes,
        work_units_per_frame = work_units_per_frame,
        checksum = checksum,
    );
}
