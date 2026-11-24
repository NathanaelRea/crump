//! Loudness-targeted mix fader adjustments written as checked Structured Source Edits.
//!
//! Crump has no hidden render-time normalization: setting a channel or Master to a loudness
//! target renders the Project offline, measures the post-fader signal, computes the required
//! linear fader delta, and publishes the updated `level` in Authoritative Source through History.

use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    engine::{
        AdmissionProfile, CompiledProject, EngineError, ProcessingMode,
        analysis::{AudioAnalyzer, FinalizedAudioAnalysis},
        mix_graph::ChannelRole,
    },
    history::{ChangeOrigin, HistoryEntry, HistoryStore, Publication},
    model::{ChannelRef, ParameterTarget, Project, RenderTailPolicy, TapPoint, TrackParameter},
    project::ProjectBundle,
};

use super::{EditError, apply_structured_edit, set_parameter_target};

/// Tolerance in dB below which `level-to` treats the owner as already on target.
pub const LEVEL_TO_TOLERANCE_DB: f64 = 0.05;
const MAX_IN_MEMORY_WRAP_FRAMES: u64 = 1_048_576;
const WRAP_CHUNK_FRAMES: usize = 65_536;
const FRAME_BYTES: usize = 8;

/// Mix owner whose `level` fader is measured and updated by `level-to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixLevelOwner {
    /// Master output bus (`master`).
    Master,
    /// Named Track (`track/<id>`).
    Track(String),
    /// Named Group (`group/<id>`).
    Group(String),
}

impl MixLevelOwner {
    /// Returns the continuous stable Parameter target for this owner's `level`.
    #[must_use]
    pub fn parameter_target(&self) -> ParameterTarget {
        match self {
            Self::Master => ParameterTarget::Master,
            Self::Track(track) => ParameterTarget::Track {
                track: track.clone(),
                parameter: TrackParameter::Level,
            },
            Self::Group(group) => ParameterTarget::Group {
                group: group.clone(),
                parameter: TrackParameter::Level,
            },
        }
    }

    /// Returns the current authored `level` in dB for this owner, or `None` if unknown.
    #[must_use]
    pub fn current_level_db(&self, project: &Project) -> Option<f64> {
        match self {
            Self::Master => Some(
                project
                    .song
                    .master
                    .as_ref()
                    .map_or(0.0, |master| master.level_db),
            ),
            Self::Track(id) => project
                .song
                .tracks
                .iter()
                .find(|track| &track.id == id)
                .map(|track| track.channel.level_db),
            Self::Group(id) => project
                .song
                .groups
                .iter()
                .find(|group| &group.id == id)
                .map(|group| group.channel.level_db),
        }
    }

    fn channel_ref(&self) -> Option<ChannelRef> {
        match self {
            Self::Master => None,
            Self::Track(id) => Some(ChannelRef::Track(id.clone())),
            Self::Group(id) => Some(ChannelRef::Group(id.clone())),
        }
    }
}

impl fmt::Display for MixLevelOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Master => f.write_str("master"),
            Self::Track(id) => write!(f, "track/{id}"),
            Self::Group(id) => write!(f, "group/{id}"),
        }
    }
}

impl FromStr for MixLevelOwner {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed == "master" {
            return Ok(Self::Master);
        }
        if let Some(id) = trimmed.strip_prefix("track/")
            && !id.is_empty()
            && !id.contains('/')
        {
            return Ok(Self::Track(id.to_owned()));
        }
        if let Some(id) = trimmed.strip_prefix("group/")
            && !id.is_empty()
            && !id.contains('/')
        {
            return Ok(Self::Group(id.to_owned()));
        }
        Err(format!(
            "mix owner `{value}` must be `master`, `track/<id>`, or `group/<id>`"
        ))
    }
}

impl Serialize for MixLevelOwner {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for MixLevelOwner {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// Loudness metric targeted by `level-to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoudnessMeasure {
    /// K-weighted, ITU-R BS.1770 / EBU R128 gated integrated loudness (LUFS).
    IntegratedLufs,
    /// Unweighted stereo RMS across the entire finalized signal (dBFS).
    Rms,
    /// Unweighted stereo RMS across active 400 ms blocks (100 ms step) at or above -60 dBFS.
    ActiveRms,
}

impl LoudnessMeasure {
    /// Returns the measured value in dB/LUFS from a completed analysis, if measurable.
    #[must_use]
    pub const fn extract(self, analysis: &FinalizedAudioAnalysis) -> Option<f64> {
        match self {
            Self::IntegratedLufs => analysis.integrated_lufs,
            Self::Rms => analysis.rms_dbfs,
            Self::ActiveRms => analysis.active_rms_dbfs,
        }
    }

    /// Canonical unit string for human-readable reports and History summaries.
    #[must_use]
    pub const fn unit_label(self) -> &'static str {
        match self {
            Self::IntegratedLufs => "LUFS",
            Self::Rms | Self::ActiveRms => "dBFS",
        }
    }

    /// Canonical CLI/JSON token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IntegratedLufs => "integrated-lufs",
            Self::Rms => "rms",
            Self::ActiveRms => "active-rms",
        }
    }
}

impl fmt::Display for LoudnessMeasure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LoudnessMeasure {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "integrated-lufs" | "integrated_lufs" => Ok(Self::IntegratedLufs),
            "rms" => Ok(Self::Rms),
            "active-rms" | "active_rms" => Ok(Self::ActiveRms),
            _ => Err(format!(
                "loudness measure `{value}` must be `integrated-lufs`, `rms`, or `active-rms`"
            )),
        }
    }
}

/// Parses a CLI level target such as `-20db`, `-14.5lufs`, `-18dbfs`, or `-20`.
pub fn parse_level_target_db(input: &str) -> Result<f64, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("target level cannot be empty".to_owned());
    }
    let lowered = trimmed.to_ascii_lowercase();
    let numeric = lowered
        .strip_suffix("dbfs")
        .or_else(|| lowered.strip_suffix("lufs"))
        .or_else(|| lowered.strip_suffix("db"))
        .unwrap_or(&lowered)
        .trim();
    let value: f64 = numeric.parse().map_err(|_| {
        format!("invalid level target `{input}` (expected e.g. `-20db` or `-14lufs`)")
    })?;
    if !value.is_finite() {
        return Err(format!("level target `{input}` must be finite"));
    }
    Ok(value)
}

/// Parameters for one offline loudness-targeting operation.
#[derive(Debug, Clone)]
pub struct LevelToRequest {
    /// Master, Track, or Group whose `level` fader will be adjusted.
    pub owner: MixLevelOwner,
    /// Target loudness in dB/LUFS.
    pub target_db: f64,
    /// Loudness metric to measure against `target_db`.
    pub measure: LoudnessMeasure,
    /// Measure and report without editing Authoritative Source or publishing History.
    pub dry_run: bool,
    /// Origin recorded on the published History entry when a write occurs.
    pub origin: ChangeOrigin,
    /// Optional explicit History summary; a descriptive summary is generated when omitted.
    pub summary: Option<String>,
    /// Optional explicit History explanation; a descriptive rationale is generated when omitted.
    pub explanation: Option<String>,
}

/// Machine-readable report returned by [`level_to`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelToResult {
    /// Mix owner measured and targeted.
    pub owner: MixLevelOwner,
    /// Authoritative Parameter target (`master`, `track/<id>/level`, or `group/<id>/level`).
    pub target: String,
    /// Loudness measurement method used.
    pub measure: LoudnessMeasure,
    /// Requested target loudness in dB/LUFS.
    pub target_db: f64,
    /// Measured loudness of the owner's current post-fader signal.
    pub measured_db: f64,
    /// Owner's authored `level` before this operation (dB).
    pub previous_level_db: f64,
    /// Owner's resulting authored `level` (dB).
    pub new_level_db: f64,
    /// Applied (or planned, on `--dry-run`) fader change in dB.
    pub delta_db: f64,
    /// Exact unrounded `target_db - measured_db` gap in dB.
    pub raw_delta_db: f64,
    /// No-op tolerance threshold in dB.
    pub tolerance_db: f64,
    /// True when the signal was already within `tolerance_db` of `target_db`.
    pub within_tolerance: bool,
    /// True when `--dry-run` prevented writing Authoritative Source or History.
    pub dry_run: bool,
    /// True when Authoritative Source was updated and published to History.
    pub changed: bool,
    /// Effective Project tail policy applied during the measurement render.
    pub tail_policy: RenderTailPolicy,
    /// Frame count analyzed after applying `tail_policy`.
    pub analyzed_frames: u64,
    /// Sample peak of the measured signal before applying `delta_db` (dBFS).
    pub measured_sample_peak_dbfs: Option<f64>,
    /// True peak of the measured signal before applying `delta_db` (dBTP).
    pub measured_true_peak_dbtp: Option<f64>,
    /// Projected sample peak after applying `delta_db` (dBFS).
    pub resulting_sample_peak_dbfs: Option<f64>,
    /// Projected true peak after applying `delta_db` (dBTP).
    pub resulting_true_peak_dbtp: Option<f64>,
    /// True when the projected sample peak or true peak reaches or exceeds 0 dBFS / 0 dBTP.
    pub resulting_would_clip: bool,
    /// Published History entry when `changed` is true.
    pub history_entry: Option<HistoryEntry>,
}

/// Errors produced by [`level_to`].
#[derive(Debug, Error)]
pub enum LevelToError {
    #[error("level-to requires a valid Project")]
    InvalidBase,
    #[error("{0}")]
    InvalidTargetValue(String),
    #[error("unknown mix owner `{0}`")]
    UnknownOwner(String),
    #[error(
        "cannot adjust `{0}` level because an Automation lane controls that parameter; edit or remove the Automation lane instead"
    )]
    AutomatedTarget(String),
    #[error(
        "owner `{owner}` produced no measurable `{measure}` loudness (signal is silent, below gate, or shorter than one analysis window)"
    )]
    UnmeasurableSignal {
        owner: String,
        measure: LoudnessMeasure,
    },
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Edit(#[from] EditError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Measures an owner's post-fader signal offline and writes its updated `level` through History.
pub fn level_to(
    bundle: &ProjectBundle,
    request: &LevelToRequest,
) -> Result<LevelToResult, LevelToError> {
    if !request.target_db.is_finite() {
        return Err(LevelToError::InvalidTargetValue(
            "target level must be finite".to_owned(),
        ));
    }
    if bundle.diagnostics.has_errors() {
        return Err(LevelToError::InvalidBase);
    }
    let project = bundle.project.as_ref().ok_or(LevelToError::InvalidBase)?;
    let previous_level_db = request
        .owner
        .current_level_db(project)
        .ok_or_else(|| LevelToError::UnknownOwner(request.owner.to_string()))?;
    let parameter_target = request.owner.parameter_target();
    if project
        .song
        .automations
        .iter()
        .any(|lane| lane.target == parameter_target)
    {
        return Err(LevelToError::AutomatedTarget(request.owner.to_string()));
    }

    // Fail fast on Candidate != Active before spending CPU on an offline measurement render.
    if !request.dry_run {
        let store = HistoryStore::open(&bundle.root).map_err(EditError::from)?;
        if let Some(active) = store.active_state().map_err(EditError::from)?
            && active.active_revision != bundle.revision
        {
            return Err(LevelToError::Edit(EditError::CandidateNotActive {
                active: active.active_revision,
                candidate: bundle.revision.clone(),
            }));
        }
    }

    let compiled =
        CompiledProject::compile_for(project, &bundle.assets, AdmissionProfile::Offline)?;
    let tail_policy = project.render_tail_policy;
    let analysis = measure_owner_offline(&compiled, &request.owner, tail_policy)?;
    let measured_db =
        request
            .measure
            .extract(&analysis)
            .ok_or_else(|| LevelToError::UnmeasurableSignal {
                owner: request.owner.to_string(),
                measure: request.measure,
            })?;

    let raw_delta_db = request.target_db - measured_db;
    let within_tolerance = raw_delta_db.abs() <= LEVEL_TO_TOLERANCE_DB;
    let (new_level_db, delta_db, replacement) = if within_tolerance {
        (previous_level_db, 0.0, None)
    } else {
        let candidate_level = round_to_centi_db(previous_level_db + raw_delta_db);
        // Validate the target fader range even on `--dry-run`.
        let replacement = set_parameter_target(bundle, &parameter_target, candidate_level)?;
        if replacement.replacement == replacement.expected {
            (previous_level_db, 0.0, None)
        } else {
            (
                candidate_level,
                round_to_centi_db(candidate_level - previous_level_db),
                Some(replacement),
            )
        }
    };

    // Channel strip faders (including Master's fader) are linear post-insert gains, so shifting
    // `level` by `delta_db` shifts post-fader sample peak and true peak by exact `delta_db`.
    let resulting_sample_peak_dbfs = analysis.sample_peak_dbfs.map(|peak| peak + delta_db);
    let resulting_true_peak_dbtp = analysis.true_peak_dbtp.map(|peak| peak + delta_db);
    let resulting_would_clip = resulting_sample_peak_dbfs.is_some_and(|peak| peak >= 0.0)
        || resulting_true_peak_dbtp.is_some_and(|peak| peak >= 0.0);

    let (changed, history_entry) = match (request.dry_run, replacement) {
        (false, Some(replacement)) => {
            let summary = request.summary.clone().unwrap_or_else(|| {
                format!(
                    "Set {} level to {:.2} dB ({} target {:.2} {})",
                    request.owner,
                    new_level_db,
                    request.measure,
                    request.target_db,
                    request.measure.unit_label()
                )
            });
            let explanation = request.explanation.clone().unwrap_or_else(|| {
                format!(
                    "Measured {} at {:.2} {} ({}); adjusted level from {:.2} dB to {:.2} dB (delta {:+.2} dB)",
                    request.owner,
                    measured_db,
                    request.measure.unit_label(),
                    request.measure,
                    previous_level_db,
                    new_level_db,
                    delta_db
                )
            });
            let entry = apply_structured_edit(
                &bundle.root,
                &bundle.revision,
                vec![replacement],
                Publication {
                    origin: request.origin.clone(),
                    summary,
                    explanation,
                    restored_from: None,
                },
            )?;
            (true, Some(entry))
        }
        _ => (false, None),
    };

    Ok(LevelToResult {
        owner: request.owner.clone(),
        target: parameter_target.to_string(),
        measure: request.measure,
        target_db: request.target_db,
        measured_db,
        previous_level_db,
        new_level_db,
        delta_db,
        raw_delta_db,
        tolerance_db: LEVEL_TO_TOLERANCE_DB,
        within_tolerance: within_tolerance || delta_db == 0.0,
        dry_run: request.dry_run,
        changed,
        tail_policy,
        analyzed_frames: analysis.analyzed_frames,
        measured_sample_peak_dbfs: analysis.sample_peak_dbfs,
        measured_true_peak_dbtp: analysis.true_peak_dbtp,
        resulting_sample_peak_dbfs,
        resulting_true_peak_dbtp,
        resulting_would_clip,
        history_entry,
    })
}

fn round_to_centi_db(value: f64) -> f64 {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == 0.0 { 0.0 } else { rounded }
}

fn resolve_owner_tap(
    compiled: &CompiledProject,
    owner: &MixLevelOwner,
) -> Result<(Option<usize>, u64), LevelToError> {
    let Some(channel_ref) = owner.channel_ref() else {
        return Ok((None, 0));
    };
    let inspection = compiled.mix_graph_inspection();
    let master_latency = inspection
        .channels
        .iter()
        .find(|channel| channel.role == ChannelRole::Master)
        .map(|channel| channel.output_latency_frames)
        .ok_or_else(|| {
            EngineError::Compile("compiled mix graph has no Master channel".to_owned())
        })?;
    let identity = channel_ref.to_string();
    let expected_role = match &channel_ref {
        ChannelRef::Track(_) => ChannelRole::Track,
        ChannelRef::Group(_) => ChannelRole::Group,
        ChannelRef::Return(_) => ChannelRole::Return,
    };
    let channel = inspection
        .channels
        .iter()
        .find(|channel| channel.identity == identity && channel.role == expected_role)
        .ok_or_else(|| LevelToError::UnknownOwner(identity.clone()))?;
    let alignment_delay = master_latency
        .checked_sub(channel.output_latency_frames)
        .ok_or_else(|| {
            EngineError::Compile(format!(
                "channel `{identity}` latency exceeds Master output latency"
            ))
        })?;
    Ok((Some(channel.index), u64::from(alignment_delay)))
}

fn measure_owner_offline(
    compiled: &CompiledProject,
    owner: &MixLevelOwner,
    tail_policy: RenderTailPolicy,
) -> Result<FinalizedAudioAnalysis, LevelToError> {
    if compiled.sample_rate() != 48_000 {
        return Err(EngineError::Compile(format!(
            "offline loudness analysis requires 48000 Hz audio, got {} Hz",
            compiled.sample_rate()
        ))
        .into());
    }
    let (channel_index, alignment_delay) = resolve_owner_tap(compiled, owner)?;
    let total_frames = compiled.total_frames();
    let arrangement_frames = compiled.arrangement_frames();
    let mut runtime = compiled.runtime();
    let mut analyzer = AudioAnalyzer::new();

    match tail_policy {
        RenderTailPolicy::Append => {
            for _ in 0..alignment_delay.min(total_frames) {
                analyzer.push_frame([0.0, 0.0]);
            }
            let mut frame_index = 0_u64;
            while !runtime.finished() {
                let master_sample = runtime.process_frame(ProcessingMode::Offline)?;
                let sample = match channel_index {
                    None => master_sample,
                    Some(index) => runtime
                        .channel_tap_sample(index, TapPoint::PostFader)
                        .ok_or_else(|| {
                            EngineError::Compile(format!(
                                "compiled channel `{owner}` has no post-fader tap"
                            ))
                        })?,
                };
                if frame_index.saturating_add(alignment_delay) < total_frames {
                    analyzer.push_frame(sample);
                }
                frame_index += 1;
            }
            Ok(analyzer.finish())
        }
        RenderTailPolicy::Wrap => {
            if arrangement_frames == 0 {
                return Err(EngineError::Compile(
                    "wrap tail policy requires a positive arrangement length".to_owned(),
                )
                .into());
            }
            if arrangement_frames <= MAX_IN_MEMORY_WRAP_FRAMES {
                let mut accum = vec![[0.0_f64; 2]; arrangement_frames as usize];
                let mut frame_index = 0_u64;
                while !runtime.finished() {
                    let master_sample = runtime.process_frame(ProcessingMode::Offline)?;
                    let sample = match channel_index {
                        None => master_sample,
                        Some(index) => runtime
                            .channel_tap_sample(index, TapPoint::PostFader)
                            .ok_or_else(|| {
                                EngineError::Compile(format!(
                                    "compiled channel `{owner}` has no post-fader tap"
                                ))
                            })?,
                    };
                    let aligned = frame_index.saturating_add(alignment_delay);
                    if aligned < total_frames {
                        let dst = (aligned % arrangement_frames) as usize;
                        accum[dst][0] += f64::from(sample[0]);
                        accum[dst][1] += f64::from(sample[1]);
                    }
                    frame_index += 1;
                }
                for (index, frame) in accum.iter().enumerate() {
                    let left = frame[0] as f32;
                    let right = frame[1] as f32;
                    if !left.is_finite() || !right.is_finite() {
                        return Err(EngineError::NonFinite {
                            frame: index as u64,
                        }
                        .into());
                    }
                    analyzer.push_frame([left, right]);
                }
                return Ok(analyzer.finish());
            }

            // Bounded-memory fallback for very long wrapped arrangements.
            let (spool_file, _spool_guard) = open_temp_spool()?;
            let mut spool = BufWriter::new(spool_file);
            let mut encoded = [0_u8; FRAME_BYTES];
            while !runtime.finished() {
                let master_sample = runtime.process_frame(ProcessingMode::Offline)?;
                let sample = match channel_index {
                    None => master_sample,
                    Some(index) => runtime
                        .channel_tap_sample(index, TapPoint::PostFader)
                        .ok_or_else(|| {
                            EngineError::Compile(format!(
                                "compiled channel `{owner}` has no post-fader tap"
                            ))
                        })?,
                };
                encoded[0..4].copy_from_slice(&sample[0].to_le_bytes());
                encoded[4..8].copy_from_slice(&sample[1].to_le_bytes());
                spool.write_all(&encoded)?;
            }
            let mut file = spool
                .into_inner()
                .map_err(std::io::IntoInnerError::into_error)?;
            let mut accum = vec![[0.0_f64; 2]; WRAP_CHUNK_FRAMES];
            let mut scratch = vec![0_u8; WRAP_CHUNK_FRAMES * FRAME_BYTES];
            let mut out_start = 0_u64;
            while out_start < arrangement_frames {
                let chunk_len =
                    (arrangement_frames - out_start).min(WRAP_CHUNK_FRAMES as u64) as usize;
                accum[..chunk_len].fill([0.0, 0.0]);
                let mut fold_base = 0_u64;
                while fold_base < total_frames {
                    let aligned_start = fold_base + out_start;
                    let aligned_end = (fold_base + out_start + chunk_len as u64).min(total_frames);
                    if aligned_start < aligned_end {
                        let active_start = aligned_start.max(alignment_delay);
                        if active_start < aligned_end {
                            let dst_offset = (active_start - aligned_start) as usize;
                            let src_start = active_start - alignment_delay;
                            let count = (aligned_end - active_start) as usize;
                            file.seek(SeekFrom::Start(src_start * FRAME_BYTES as u64))?;
                            let byte_len = count * FRAME_BYTES;
                            file.read_exact(&mut scratch[..byte_len])?;
                            for (offset, raw) in
                                scratch[..byte_len].as_chunks::<8>().0.iter().enumerate()
                            {
                                let left = f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
                                let right = f32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
                                accum[dst_offset + offset][0] += f64::from(left);
                                accum[dst_offset + offset][1] += f64::from(right);
                            }
                        }
                    }
                    fold_base = fold_base.saturating_add(arrangement_frames);
                }
                for (offset, frame) in accum[..chunk_len].iter().enumerate() {
                    let left = frame[0] as f32;
                    let right = frame[1] as f32;
                    if !left.is_finite() || !right.is_finite() {
                        return Err(EngineError::NonFinite {
                            frame: out_start + offset as u64,
                        }
                        .into());
                    }
                    analyzer.push_frame([left, right]);
                }
                out_start += chunk_len as u64;
            }
            Ok(analyzer.finish())
        }
    }
}

static LEVEL_TO_SPOOL_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempSpoolGuard(PathBuf);

impl Drop for TempSpoolGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn open_temp_spool() -> Result<(File, TempSpoolGuard), std::io::Error> {
    let dir = std::env::temp_dir();
    for _ in 0..100 {
        let counter = LEVEL_TO_SPOOL_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!(
            ".crump-level-to-{}-{counter}.spool",
            std::process::id()
        ));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((file, TempSpoolGuard(path))),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate temporary loudness measurement spool",
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::{
        engine::{RenderOptions, render_project},
        history::{ChangeOrigin, HistoryStore, Publication},
        project::load_project,
    };

    use super::*;

    fn mix_fixture(render_tail: &str, extra_song_lines: &str) -> String {
        format!(
            r#"crump 1
project "Level To Test" {{
  id level_to_test
  seed 42
  sample_rate 48000
  render_tail {render_tail}
  song {{
    tempo 120bpm
    meter 4/4
    end 4bars
    instrument lead "Lead" graph {{
      polyphony 4
      parameter tone "Tone" hz 100hz 10000hz 3000hz 3200hz
      faceplate {{
        control tone "Tone" "Low-pass cutoff" maps tone
      }}
      graph 8 {{
        node oscillator oscillator waveform saw level 0.5
        node envelope envelope attack 5ms decay 80ms sustain 0.6 release 150ms
        node filter lowpass cutoff $tone resonance 0.15
        node amplifier amplifier level 0.8
        node position pan position 0
        node output output
        connect note.pitch -> oscillator.pitch
        connect note.gate -> envelope.gate
        connect oscillator.audio -> filter.audio
        connect filter.audio -> amplifier.audio
        connect envelope.control -> amplifier.gain
        connect amplifier.audio -> position.audio
        connect position.audio -> output.audio
      }}
    }}
    pattern sparse 4bars {{
      note 0beat C4 2beats 0.9
    }}
    pattern steady 4bars {{
      note 0beat C4 16beats 0.9
    }}
    track lead "Lead" instrument lead level -12db pan 0 {{
      output group/bus
      clip c1 pattern sparse start 0beat duration 4bars mode once
    }}
    track pad "Pad" instrument lead level -15db pan 0 {{
      output group/bus
      clip c2 pattern steady start 0beat duration 4bars mode once
    }}
    group bus "Bus" level 0db {{
      output master
    }}
    master level 0db {{
    }}
{extra_song_lines}
  }}
}}
"#
        )
    }

    fn init_published_project(source: &str) -> tempfile::TempDir {
        let dir = tempdir().expect("temp project dir");
        fs::write(dir.path().join("project.crump"), source).expect("write source");
        let bundle = load_project(dir.path()).expect("load project");
        assert!(!bundle.diagnostics.has_errors(), "{:?}", bundle.diagnostics);
        let store = HistoryStore::open(dir.path()).expect("open history");
        store
            .publish(
                &bundle,
                Publication {
                    origin: ChangeOrigin::human("test"),
                    summary: "Initial project".to_owned(),
                    explanation: String::new(),
                    restored_from: None,
                },
            )
            .expect("publish initial");
        dir
    }

    #[test]
    fn parses_owners_measures_and_targets() {
        assert_eq!(
            "master".parse::<MixLevelOwner>().unwrap(),
            MixLevelOwner::Master
        );
        assert_eq!(
            "track/lead".parse::<MixLevelOwner>().unwrap(),
            MixLevelOwner::Track("lead".to_owned())
        );
        assert_eq!(
            "group/bus".parse::<MixLevelOwner>().unwrap(),
            MixLevelOwner::Group("bus".to_owned())
        );
        assert!("return/room".parse::<MixLevelOwner>().is_err());
        assert!("track/".parse::<MixLevelOwner>().is_err());
        assert!("track/a/b".parse::<MixLevelOwner>().is_err());

        assert_eq!(
            "integrated-lufs".parse::<LoudnessMeasure>().unwrap(),
            LoudnessMeasure::IntegratedLufs
        );
        assert_eq!(
            "rms".parse::<LoudnessMeasure>().unwrap(),
            LoudnessMeasure::Rms
        );
        assert_eq!(
            "active-rms".parse::<LoudnessMeasure>().unwrap(),
            LoudnessMeasure::ActiveRms
        );

        assert_eq!(parse_level_target_db("-20db").unwrap(), -20.0);
        assert_eq!(parse_level_target_db("-14.5LUFS").unwrap(), -14.5);
        assert_eq!(parse_level_target_db("-18dBFS").unwrap(), -18.0);
        assert_eq!(parse_level_target_db("-16").unwrap(), -16.0);
        assert!(parse_level_target_db("-20hz").is_err());
    }

    #[test]
    fn level_to_adjusts_track_active_rms_creates_history_and_second_run_is_noop() {
        let dir = init_published_project(&mix_fixture("0s", ""));
        let bundle = load_project(dir.path()).expect("load project");

        // Dry run measures without modifying source or History.
        let dry = level_to(
            &bundle,
            &LevelToRequest {
                owner: MixLevelOwner::Track("lead".to_owned()),
                target_db: -20.0,
                measure: LoudnessMeasure::ActiveRms,
                dry_run: true,
                origin: ChangeOrigin::human("test"),
                summary: None,
                explanation: None,
            },
        )
        .expect("dry run");
        assert!(dry.dry_run);
        assert!(!dry.changed);
        assert!(dry.history_entry.is_none());
        assert!(dry.delta_db.abs() > LEVEL_TO_TOLERANCE_DB);
        assert_eq!(
            HistoryStore::open(dir.path())
                .unwrap()
                .list()
                .unwrap()
                .len(),
            1
        );

        // Apply active-rms target to sparse track `lead` (2 beats sounding out of 16 beats).
        let applied = level_to(
            &bundle,
            &LevelToRequest {
                owner: MixLevelOwner::Track("lead".to_owned()),
                target_db: -20.0,
                measure: LoudnessMeasure::ActiveRms,
                dry_run: false,
                origin: ChangeOrigin::human("test"),
                summary: None,
                explanation: None,
            },
        )
        .expect("apply level-to");
        assert!(applied.changed);
        assert!(!applied.within_tolerance);
        assert!(applied.history_entry.is_some());
        assert_eq!(applied.new_level_db, dry.new_level_db);
        assert_eq!(
            HistoryStore::open(dir.path())
                .unwrap()
                .list()
                .unwrap()
                .len(),
            2
        );

        // Reload and verify Authoritative Source updated and a second run is a no-op.
        let updated = load_project(dir.path()).expect("reload project");
        assert_eq!(
            MixLevelOwner::Track("lead".to_owned())
                .current_level_db(updated.project.as_ref().unwrap()),
            Some(applied.new_level_db)
        );
        let second = level_to(
            &updated,
            &LevelToRequest {
                owner: MixLevelOwner::Track("lead".to_owned()),
                target_db: -20.0,
                measure: LoudnessMeasure::ActiveRms,
                dry_run: false,
                origin: ChangeOrigin::human("test"),
                summary: None,
                explanation: None,
            },
        )
        .expect("second run");
        assert!(!second.changed);
        assert!(second.within_tolerance);
        assert!(second.history_entry.is_none());
        assert!((second.measured_db - (-20.0)).abs() <= LEVEL_TO_TOLERANCE_DB);
        assert_eq!(
            HistoryStore::open(dir.path())
                .unwrap()
                .list()
                .unwrap()
                .len(),
            2
        );

        // Verify a real stem render matches the measured active_rms_dbfs and resulting peak.
        let mut render_options = RenderOptions::new(dir.path().join("render_check"));
        render_options.stems = vec!["track/lead:post_fader".parse().unwrap()];
        let rendered = render_project(&updated, &render_options).expect("render stem");
        let stem_analysis = &rendered.manifest.stems[0].analysis;
        let stem_active = stem_analysis.active_rms_dbfs.expect("stem active_rms_dbfs");
        assert!(
            (stem_active - (-20.0)).abs() <= LEVEL_TO_TOLERANCE_DB,
            "stem active RMS was {stem_active}, expected -20.0"
        );
        assert!(
            (stem_analysis.sample_peak_dbfs.unwrap() - applied.resulting_sample_peak_dbfs.unwrap())
                .abs()
                < 0.05
        );
    }

    #[test]
    fn level_to_adjusts_master_integrated_lufs_and_group_rms() {
        let dir = init_published_project(&mix_fixture("500ms wrap", ""));
        let bundle = load_project(dir.path()).expect("load project");

        let group_res = level_to(
            &bundle,
            &LevelToRequest {
                owner: MixLevelOwner::Group("bus".to_owned()),
                target_db: -24.0,
                measure: LoudnessMeasure::Rms,
                dry_run: false,
                origin: ChangeOrigin::human("test"),
                summary: Some("Set bus RMS to -24 dBFS".to_owned()),
                explanation: None,
            },
        )
        .expect("group level-to");
        assert!(group_res.changed);
        assert_eq!(group_res.tail_policy, RenderTailPolicy::Wrap);
        assert_eq!(
            group_res.history_entry.as_ref().unwrap().summary,
            "Set bus RMS to -24 dBFS"
        );

        let after_group = load_project(dir.path()).expect("reload after group");
        let master_res = level_to(
            &after_group,
            &LevelToRequest {
                owner: MixLevelOwner::Master,
                target_db: -18.0,
                measure: LoudnessMeasure::IntegratedLufs,
                dry_run: false,
                origin: ChangeOrigin::human("test"),
                summary: None,
                explanation: None,
            },
        )
        .expect("master level-to");
        assert!(master_res.changed);
        assert_eq!(master_res.tail_policy, RenderTailPolicy::Wrap);

        let after_master = load_project(dir.path()).expect("reload after master");
        let rendered = render_project(
            &after_master,
            &RenderOptions::new(dir.path().join("master_render")),
        )
        .expect("render master");
        let lufs = rendered
            .manifest
            .analysis
            .integrated_lufs
            .expect("integrated_lufs");
        assert!(
            (lufs - (-18.0)).abs() <= LEVEL_TO_TOLERANCE_DB,
            "master integrated LUFS was {lufs}, expected -18.0"
        );
        assert!(
            (rendered.manifest.analysis.sample_peak_dbfs.unwrap()
                - master_res.resulting_sample_peak_dbfs.unwrap())
            .abs()
                < 0.05
        );
    }

    #[test]
    fn level_to_rejects_automated_level_target() {
        let source = mix_fixture(
            "0s",
            "    automation lead_lvl target track/lead/level {\n      point p0 0beat -12db linear\n      point p1 4bars -6db linear\n    }",
        );
        let dir = init_published_project(&source);
        let bundle = load_project(dir.path()).expect("load project");
        let error = level_to(
            &bundle,
            &LevelToRequest {
                owner: MixLevelOwner::Track("lead".to_owned()),
                target_db: -20.0,
                measure: LoudnessMeasure::ActiveRms,
                dry_run: false,
                origin: ChangeOrigin::human("test"),
                summary: None,
                explanation: None,
            },
        )
        .expect_err("automated target must be rejected");
        assert!(matches!(error, LevelToError::AutomatedTarget(_)), "{error}");
    }
}
