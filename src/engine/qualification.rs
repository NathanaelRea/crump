//! Offline, range-bounded callback workload qualification.
//!
//! This harness records reproducible Project/build/range facts and wall-clock block
//! timing without claiming PipeWire xrun, named-hardware, or realtime-safety evidence.

use std::{
    fs,
    fs::File,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::project::{ProjectBundle, RevisionId};

use super::mix_graph::RuntimeResourceEstimate;
use super::{CompiledProject, EngineError, ProcessingMode, SongRuntime};

const QUALIFICATION_SCHEMA: u32 = 1;
const UTILIZATION_BUCKET_WIDTH: f64 = 0.005;
const UTILIZATION_BUCKETS: usize = 4_096;

/// One explicit offline workload-profile request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualificationOptions {
    /// First frame included in the timed range.
    pub start_frame: u64,
    /// First frame excluded from the timed range.
    pub end_frame: u64,
    /// Full callback block size to exercise; the exact range may end with one short block.
    pub quantum_frames: usize,
    /// Largest accepted approximate p99.9 deadline utilization.
    pub maximum_p999_utilization: f64,
}

/// Host facts captured with a qualification report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualificationHost {
    /// Rust target operating system.
    pub target_os: String,
    /// Rust target architecture.
    pub target_arch: String,
    /// Running kernel release when exposed by the host.
    pub kernel_release: Option<String>,
    /// Operating-system release identity when exposed by the host.
    pub os_release: Option<String>,
    /// First CPU model identity reported by Linux when available.
    pub cpu_model: Option<String>,
}

/// Versioned machine-readable offline workload evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationReport {
    /// Qualification-report schema version.
    pub schema_version: u32,
    /// Exact timed method identity.
    pub measurement_method: String,
    /// Explicitly offline backend identity.
    pub backend: String,
    /// Stable Project identity.
    pub project_id: String,
    /// Exact qualified Project Revision.
    pub project_revision: RevisionId,
    /// Crump package version.
    pub crump_version: String,
    /// SHA-256 of the running executable.
    pub executable_sha256: String,
    /// Host identity facts.
    pub host: QualificationHost,
    /// Fixed Project sample rate.
    pub sample_rate: u32,
    /// Requested block size.
    pub requested_quantum_frames: usize,
    /// Full block size used by the offline harness.
    pub actual_quantum_frames: usize,
    /// Frames processed by the final timed block; equal to the quantum when range-aligned.
    pub final_block_frames: usize,
    /// Inclusive timed range start.
    pub start_frame: u64,
    /// Exclusive timed range end.
    pub end_frame: u64,
    /// Exact number of profiled Audio frames.
    pub profiled_frames: u64,
    /// Audio duration represented by the range.
    pub audio_duration_seconds: f64,
    /// Number of timed block invocations.
    pub callback_blocks: u64,
    /// Sum of measured block wall times.
    pub wall_time_seconds: f64,
    /// Audio duration divided by measured wall time, when measurable.
    pub realtime_factor: Option<f64>,
    /// Mean wall time divided by the available block deadlines.
    pub mean_callback_utilization: f64,
    /// Largest measured block deadline utilization.
    pub peak_callback_utilization: f64,
    /// Conservative fixed-histogram p99.9 deadline utilization.
    pub p999_callback_utilization: f64,
    /// Required p99.9 ceiling supplied by the operator.
    pub maximum_p999_utilization: f64,
    /// Blocks whose wall time exceeded the deadline for their exact processed frames.
    pub missed_deadlines: u64,
    /// Largest finite output sample magnitude.
    pub sample_peak: f64,
    /// Non-finite output samples observed by the harness.
    pub non_finite_sample_count: u64,
    /// First frame containing non-finite output, if any.
    pub first_non_finite_frame: Option<u64>,
    /// Compile-time structural resource/admission facts.
    pub resources: RuntimeResourceEstimate,
    /// Whether every offline machine-checkable threshold passed.
    pub passed: bool,
    /// Evidence this offline harness deliberately cannot provide.
    pub external_evidence_required: Vec<String>,
    /// PipeWire xruns are intentionally absent from offline profiling.
    pub pipewire_xruns: Option<u64>,
}

/// Profiles one exact Project range with the running wall clock.
pub fn qualify_project(
    bundle: &ProjectBundle,
    options: QualificationOptions,
) -> Result<QualificationReport, EngineError> {
    let project = bundle
        .publishable()
        .ok_or_else(|| EngineError::Validation(bundle.diagnostics.as_slice().to_vec()))?;
    let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)?;
    qualify_compiled_project(bundle, &compiled, options)
}

pub(crate) fn qualify_compiled_project(
    bundle: &ProjectBundle,
    compiled: &CompiledProject,
    options: QualificationOptions,
) -> Result<QualificationReport, EngineError> {
    qualify_compiled_with_timer(bundle, compiled, options, |runtime, output| {
        let started = Instant::now();
        let processed = runtime.process_block(output, ProcessingMode::Offline)?;
        Ok((processed, started.elapsed()))
    })
}

#[cfg(test)]
fn qualify_project_with_timer(
    bundle: &ProjectBundle,
    options: QualificationOptions,
    process_block: impl FnMut(
        &mut SongRuntime,
        &mut [[f32; 2]],
    ) -> Result<(usize, Duration), EngineError>,
) -> Result<QualificationReport, EngineError> {
    let project = bundle
        .publishable()
        .ok_or_else(|| EngineError::Validation(bundle.diagnostics.as_slice().to_vec()))?;
    let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)?;
    qualify_compiled_with_timer(bundle, &compiled, options, process_block)
}

fn qualify_compiled_with_timer(
    bundle: &ProjectBundle,
    compiled: &CompiledProject,
    options: QualificationOptions,
    mut process_block: impl FnMut(
        &mut SongRuntime,
        &mut [[f32; 2]],
    ) -> Result<(usize, Duration), EngineError>,
) -> Result<QualificationReport, EngineError> {
    let resources = compiled.mix_graph_inspection().resources.clone();
    validate_options(compiled, options, &resources)?;

    let mut runtime = compiled.runtime();
    runtime.advance_to(options.start_frame)?;
    let mut output = vec![[0.0_f32; 2]; options.quantum_frames];
    let mut histogram = [0_u64; UTILIZATION_BUCKETS];
    let mut callback_blocks = 0_u64;
    let mut total_elapsed = Duration::ZERO;
    let mut peak_utilization = 0.0_f64;
    let mut missed_deadlines = 0_u64;
    let mut sample_peak = 0.0_f64;
    let mut non_finite_sample_count = 0_u64;
    let mut first_non_finite_frame = None;
    let mut final_block_frames = options.quantum_frames;
    let mut frame = options.start_frame;

    while frame < options.end_frame {
        let requested =
            usize::try_from((options.end_frame - frame).min(options.quantum_frames as u64))
                .unwrap_or(options.quantum_frames);
        final_block_frames = requested;
        let (processed, elapsed) = process_block(&mut runtime, &mut output[..requested])?;
        if processed != requested {
            return Err(EngineError::Compile(format!(
                "qualification runtime processed {processed} frames for a {requested}-frame block"
            )));
        }
        let block_budget_seconds = requested as f64 / f64::from(compiled.sample_rate());
        let utilization = elapsed.as_secs_f64() / block_budget_seconds;
        let bucket = utilization_bucket(utilization);
        histogram[bucket] = histogram[bucket].saturating_add(1);
        callback_blocks = callback_blocks.saturating_add(1);
        total_elapsed = total_elapsed.saturating_add(elapsed);
        peak_utilization = peak_utilization.max(utilization);
        if utilization > 1.0 {
            missed_deadlines = missed_deadlines.saturating_add(1);
        }

        for (offset, stereo) in output[..processed].iter().enumerate() {
            let mut frame_is_non_finite = false;
            for sample in stereo {
                if sample.is_finite() {
                    sample_peak = sample_peak.max(f64::from(sample.abs()));
                } else {
                    non_finite_sample_count = non_finite_sample_count.saturating_add(1);
                    frame_is_non_finite = true;
                }
            }
            if frame_is_non_finite && first_non_finite_frame.is_none() {
                first_non_finite_frame = Some(frame + offset as u64);
            }
        }
        frame = frame.saturating_add(processed as u64);
    }

    let p999_callback_utilization =
        histogram_quantile(&histogram, callback_blocks, 999, 1_000, peak_utilization);
    let wall_time_seconds = total_elapsed.as_secs_f64();
    let profiled_frames = options.end_frame - options.start_frame;
    let audio_duration_seconds = profiled_frames as f64 / f64::from(compiled.sample_rate());
    let total_budget_seconds = audio_duration_seconds;
    let realtime_factor =
        (wall_time_seconds > 0.0).then_some(audio_duration_seconds / wall_time_seconds);
    let mean_callback_utilization = if total_budget_seconds > 0.0 {
        wall_time_seconds / total_budget_seconds
    } else {
        0.0
    };
    let passed = missed_deadlines == 0
        && non_finite_sample_count == 0
        && p999_callback_utilization <= options.maximum_p999_utilization;

    Ok(QualificationReport {
        schema_version: QUALIFICATION_SCHEMA,
        measurement_method: "crump-offline-wall-clock-block-profile-v1".to_owned(),
        backend: "offline".to_owned(),
        project_id: runtime.project_id().to_owned(),
        project_revision: bundle.revision.clone(),
        crump_version: env!("CARGO_PKG_VERSION").to_owned(),
        executable_sha256: hash_file(&std::env::current_exe()?)?,
        host: qualification_host(),
        sample_rate: compiled.sample_rate(),
        requested_quantum_frames: options.quantum_frames,
        actual_quantum_frames: options.quantum_frames,
        final_block_frames,
        start_frame: options.start_frame,
        end_frame: options.end_frame,
        profiled_frames,
        audio_duration_seconds,
        callback_blocks,
        wall_time_seconds,
        realtime_factor,
        mean_callback_utilization,
        peak_callback_utilization: peak_utilization,
        p999_callback_utilization,
        maximum_p999_utilization: options.maximum_p999_utilization,
        missed_deadlines,
        sample_peak,
        non_finite_sample_count,
        first_non_finite_frame,
        resources,
        passed,
        external_evidence_required: vec![
            "PipeWire xrun count from the target graph".to_owned(),
            "named-hardware 30-minute realtime endurance".to_owned(),
            "callback allocation/blocking/I/O audit".to_owned(),
        ],
        pipewire_xruns: None,
    })
}

fn validate_options(
    compiled: &CompiledProject,
    options: QualificationOptions,
    resources: &RuntimeResourceEstimate,
) -> Result<(), EngineError> {
    if options.start_frame >= options.end_frame || options.end_frame > compiled.total_frames() {
        return Err(EngineError::Compile(format!(
            "qualification range must satisfy start < end <= {} frames",
            compiled.total_frames()
        )));
    }
    if !resources.supported_quanta.contains(&options.quantum_frames) {
        return Err(EngineError::Compile(format!(
            "qualification quantum {} is unsupported; choose one of {:?}",
            options.quantum_frames, resources.supported_quanta
        )));
    }
    if !options.maximum_p999_utilization.is_finite()
        || options.maximum_p999_utilization <= 0.0
        || options.maximum_p999_utilization > 1.0
    {
        return Err(EngineError::Compile(
            "qualification p99.9 utilization ceiling must be finite and in (0, 1]".to_owned(),
        ));
    }
    Ok(())
}

fn utilization_bucket(utilization: f64) -> usize {
    if !utilization.is_finite() || utilization < 0.0 {
        return UTILIZATION_BUCKETS - 1;
    }
    ((utilization / UTILIZATION_BUCKET_WIDTH).ceil() as usize).min(UTILIZATION_BUCKETS - 1)
}

fn histogram_quantile(
    histogram: &[u64; UTILIZATION_BUCKETS],
    total: u64,
    numerator: u64,
    denominator: u64,
    observed_peak: f64,
) -> f64 {
    if total == 0 || denominator == 0 {
        return 0.0;
    }
    let target =
        ((u128::from(total) * u128::from(numerator)).div_ceil(u128::from(denominator))) as u64;
    let mut cumulative = 0_u64;
    for (index, count) in histogram.iter().enumerate() {
        cumulative = cumulative.saturating_add(*count);
        if cumulative >= target {
            let bucket_ceiling = index as f64 * UTILIZATION_BUCKET_WIDTH;
            return if index == UTILIZATION_BUCKETS - 1 {
                observed_peak.max(bucket_ceiling)
            } else {
                bucket_ceiling
            };
        }
    }
    (UTILIZATION_BUCKETS - 1) as f64 * UTILIZATION_BUCKET_WIDTH
}

fn qualification_host() -> QualificationHost {
    QualificationHost {
        target_os: std::env::consts::OS.to_owned(),
        target_arch: std::env::consts::ARCH.to_owned(),
        kernel_release: read_trimmed(Path::new("/proc/sys/kernel/osrelease")),
        os_release: fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|source| os_release_identity(&source)),
        cpu_model: fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|source| first_key_value(&source, "model name")),
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn first_key_value(source: &str, key: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        (candidate.trim() == key)
            .then(|| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn os_release_identity(source: &str) -> Option<String> {
    let value = |key: &str| {
        source.lines().find_map(|line| {
            let (candidate, value) = line.split_once('=')?;
            (candidate == key).then(|| value.trim_matches('"').to_owned())
        })
    };
    let id = value("ID")?;
    Some(
        match value("VERSION_ID").filter(|version| !version.is_empty()) {
            Some(version) => format!("{id}/{version}"),
            None => id,
        },
    )
}

fn hash_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1_024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use tempfile::tempdir;

    use crate::project::load_project;

    use super::*;

    #[test]
    fn fake_block_timer_proves_pass_and_deadline_failure_thresholds() {
        let directory = tempdir().expect("temporary Project root");
        fs::write(
            directory.path().join("project.crump"),
            include_str!("../../tests/fixtures/minimal.crump"),
        )
        .expect("Project source");
        let bundle = load_project(directory.path()).expect("load Project");
        let options = QualificationOptions {
            start_frame: 0,
            end_frame: 1_024,
            quantum_frames: 64,
            maximum_p999_utilization: 0.8,
        };

        let mut fast = VecDeque::from(vec![Duration::from_micros(100); 16]);
        let report = qualify_project_with_timer(&bundle, options, |runtime, output| {
            let processed = runtime.process_block(output, ProcessingMode::Offline)?;
            Ok((processed, fast.pop_front().expect("fast timing")))
        })
        .expect("fast qualification");
        assert!(report.passed);
        assert_eq!(report.callback_blocks, 16);
        assert_eq!(report.missed_deadlines, 0);
        assert!(report.p999_callback_utilization < 0.1);
        assert_eq!(report.non_finite_sample_count, 0);
        assert_eq!(report.pipewire_xruns, None);
        assert_eq!(report.external_evidence_required.len(), 3);

        let mut slow = VecDeque::from(vec![Duration::from_millis(2); 16]);
        let report = qualify_project_with_timer(&bundle, options, |runtime, output| {
            let processed = runtime.process_block(output, ProcessingMode::Offline)?;
            Ok((processed, slow.pop_front().expect("slow timing")))
        })
        .expect("slow qualification");
        assert!(!report.passed);
        assert_eq!(report.missed_deadlines, 16);
        assert!(report.p999_callback_utilization > 1.0);

        let mut overflow = VecDeque::from(vec![Duration::from_secs(1); 16]);
        let report = qualify_project_with_timer(&bundle, options, |runtime, output| {
            let processed = runtime.process_block(output, ProcessingMode::Offline)?;
            Ok((processed, overflow.pop_front().expect("overflow timing")))
        })
        .expect("overflow qualification");
        assert_eq!(report.peak_callback_utilization, 750.0);
        assert_eq!(report.p999_callback_utilization, 750.0);
    }

    #[test]
    fn every_supported_quantum_profiles_the_exact_requested_range() {
        let directory = tempdir().expect("temporary Project root");
        fs::write(
            directory.path().join("project.crump"),
            include_str!("../../tests/fixtures/minimal.crump"),
        )
        .expect("Project source");
        let bundle = load_project(directory.path()).expect("load Project");
        for quantum_frames in [64, 128, 256] {
            let options = QualificationOptions {
                start_frame: 512,
                end_frame: 2_051,
                quantum_frames,
                maximum_p999_utilization: 1.0,
            };
            let blocks = 1_539_usize.div_ceil(quantum_frames);
            let report = qualify_project_with_timer(&bundle, options, |runtime, output| {
                let processed = runtime.process_block(output, ProcessingMode::Offline)?;
                let elapsed = Duration::from_nanos(output.len() as u64 * 1_000);
                Ok((processed, elapsed))
            })
            .expect("qualification");
            assert_eq!(report.start_frame, 512);
            assert_eq!(report.end_frame, 2_051);
            assert_eq!(report.profiled_frames, 1_539);
            assert_eq!(report.callback_blocks, blocks as u64);
            assert_eq!(report.actual_quantum_frames, quantum_frames);
            assert_eq!(report.final_block_frames, 1_539 % quantum_frames);
            assert_eq!(report.missed_deadlines, 0);
            assert!((report.mean_callback_utilization - 0.048).abs() < 1.0e-12);
            assert!(report.passed);
        }
    }

    #[test]
    fn qualification_rejects_unbounded_ranges_quanta_and_margins() {
        let directory = tempdir().expect("temporary Project root");
        fs::write(
            directory.path().join("project.crump"),
            include_str!("../../tests/fixtures/minimal.crump"),
        )
        .expect("Project source");
        let bundle = load_project(directory.path()).expect("load Project");
        for options in [
            QualificationOptions {
                start_frame: 100,
                end_frame: 100,
                quantum_frames: 64,
                maximum_p999_utilization: 0.8,
            },
            QualificationOptions {
                start_frame: 0,
                end_frame: 100,
                quantum_frames: 512,
                maximum_p999_utilization: 0.8,
            },
            QualificationOptions {
                start_frame: 0,
                end_frame: 100,
                quantum_frames: 64,
                maximum_p999_utilization: 1.1,
            },
        ] {
            assert!(qualify_project(&bundle, options).is_err());
        }
    }
}
