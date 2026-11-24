//! Deterministic stereo float-WAV rendering with failure-atomic bundle publication.

use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};

use rustix::fs::{RenameFlags, renameat_with};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    model::{ChannelRef, RenderTailPolicy, TapPoint},
    project::{ProjectBundle, RevisionId},
};

use super::{
    CompiledProject, EngineError, FinalizedAudioAnalysis, ProcessingMode,
    analysis::{ACTIVE_RMS_METHOD, ANALYSIS_SCHEMA, analyze_finalized_wav},
    mix_graph::{ChannelRole, MAX_MIX_CHANNELS},
};

const RENDER_SCHEMA: u32 = 3;
const WAV_FILE: &str = "master.wav";
const MASTER_SPOOL_FILE: &str = ".master-spool.raw";
const MANIFEST_FILE: &str = "manifest.json";
const MAX_RENDER_STEMS: usize = MAX_MIX_CHANNELS * 2;
const MAX_RENDER_MANIFEST_BYTES: u64 = 1 << 20;
const STEMS_PER_SPOOL: usize = 16;
const RENDER_BLOCK_FRAMES: usize = 1_024;
const WRAP_CHUNK_FRAMES: usize = RENDER_BLOCK_FRAMES * 8;
const MAX_IN_MEMORY_WRAP_FRAMES: u64 = 65_536;
static RENDER_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Offline render publication settings.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Destination bundle directory containing `master.wav` and `manifest.json`.
    pub output: PathBuf,
    /// Atomically replace an existing complete bundle.
    pub replace: bool,
    /// Selected aligned Track/Group channel taps.
    pub stems: Vec<RenderStemRequest>,
    /// Optional override for the Project's authored `render_tail` policy.
    pub tail_policy_override: Option<RenderTailPolicy>,
    #[cfg(test)]
    fail_after_stem_spool: bool,
}

impl RenderOptions {
    /// Creates options that atomically replace an earlier bundle.
    #[must_use]
    pub fn new(output: impl Into<PathBuf>) -> Self {
        Self {
            output: output.into(),
            replace: true,
            stems: Vec::new(),
            tail_policy_override: None,
            #[cfg(test)]
            fail_after_stem_spool: false,
        }
    }
}

/// One selected Track/Group stem and explicit channel tap.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RenderStemRequest {
    /// Stable authored Track or Group identity.
    pub owner: ChannelRef,
    /// Capture before or after the owner's fader/panner/mute stage.
    pub tap: TapPoint,
}

impl FromStr for RenderStemRequest {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (owner, tap) = value.rsplit_once(':').ok_or_else(|| {
            "stem must use OWNER:TAP, for example track/lead:post_fader".to_owned()
        })?;
        let owner = ChannelRef::from_str(owner).map_err(|error| error.to_string())?;
        if !matches!(owner, ChannelRef::Track(_) | ChannelRef::Group(_)) {
            return Err("stem owner must be track/ID or group/ID".to_owned());
        }
        let tap = TapPoint::from_str(tap).map_err(|error| error.to_string())?;
        Ok(Self { owner, tap })
    }
}

/// Explicit inclusion policy for one exported stem.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenderStemProcessingPolicy {
    /// Upstream main-route material reaching the owner is included.
    pub includes_upstream_main_routing: bool,
    /// Instrument post-sum and owner insert processing before the tap are included.
    pub includes_owner_processing: bool,
    /// Owner fader, panner, and Mute are included only for post-fader taps.
    pub includes_owner_fader_panner_mute: bool,
    /// Outgoing parallel Send branches are not folded into the owner stem.
    pub includes_outgoing_sends: bool,
    /// Downstream Group/Return processing is not included.
    pub includes_downstream_processing: bool,
    /// Master processing is not included.
    pub includes_master_processing: bool,
}

/// Planned aligned stem artifact before Audio is written.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenderStemPlan {
    /// Stable authored Track or Group identity.
    pub owner: ChannelRef,
    /// Explicit owner-channel tap.
    pub tap: TapPoint,
    /// Safe deterministic bundle-relative WAV name.
    pub file: String,
    /// Zero padding needed to align this tap with Master output latency.
    pub alignment_delay_frames: u32,
    /// Exact capture and processing policy.
    pub processing_policy: RenderStemProcessingPolicy,
}

/// One finalized, analyzed, and hashed stem artifact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderStemManifest {
    /// Planned owner, tap, filename, alignment, and processing policy.
    #[serde(flatten)]
    pub plan: RenderStemPlan,
    /// PCM sample rate shared with Master.
    pub sample_rate: u32,
    /// Stereo channel count.
    pub channels: u16,
    /// Explicit stereo layout.
    pub channel_layout: String,
    /// IEEE stereo 32-bit float WAV.
    pub sample_format: String,
    /// Frame count shared exactly with Master.
    pub frame_count: u64,
    /// Analysis of the finalized stem WAV.
    pub analysis: FinalizedAudioAnalysis,
    /// SHA-256 of exact stem WAV bytes.
    pub wav_sha256: String,
}

/// Deterministic machine-readable render manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Stable Project ID.
    pub project_id: String,
    /// Exact Active Revision rendered.
    pub project_revision: RevisionId,
    /// Crump package version.
    pub crump_version: String,
    /// SHA-256 of the exact running executable.
    pub executable_sha256: String,
    /// Compilation target OS.
    pub target_os: String,
    /// Compilation target architecture.
    pub target_arch: String,
    /// Distribution identity from `/etc/os-release`, when available.
    pub host_os_release: Option<String>,
    /// Running kernel release, when available.
    pub host_kernel_release: Option<String>,
    /// True only on the promised Ubuntu 26.04 Linux amd64 comparison environment.
    pub reference_environment: bool,
    /// PCM sample rate.
    pub sample_rate: u32,
    /// Always two for the First Public Release.
    pub channels: u16,
    /// Always IEEE stereo 32-bit float WAV.
    pub sample_format: String,
    /// Frames from Song zero through arrangement end.
    pub arrangement_frames: u64,
    /// Explicit authored Render Tail frames.
    pub render_tail_frames: u64,
    /// Policy governing how the Render Tail is combined with the arrangement (`append` or `wrap`).
    pub tail_policy: RenderTailPolicy,
    /// Complete output frame count (`arrangement + tail` for `append`, `arrangement` for `wrap`).
    pub frame_count: u64,
    /// Complete output duration in seconds.
    pub duration_seconds: f64,
    /// Maximum absolute authored output sample; retained for manifest compatibility.
    pub peak: f32,
    /// Maximum absolute Master sample before tail wrapping (`null` / omitted for `append`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_wrap_peak: Option<f64>,
    /// True when wrapping raised the finalized Master sample peak above `pre_wrap_peak`.
    #[serde(default)]
    pub wrap_peak_exceeded: bool,
    /// Delivery metrics computed by reopening the exact finalized WAV.
    pub analysis: FinalizedAudioAnalysis,
    /// Hash of exact WAV bytes.
    pub wav_sha256: String,
    /// Selected aligned Track/Group stem artifacts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stems: Vec<RenderStemManifest>,
    /// Feedback graph voices or shared runtimes muted for non-finite or runaway output.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub feedback_mutes: u64,
}

const fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// Published render artifact paths and manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderResult {
    /// Complete bundle directory.
    pub output: PathBuf,
    /// Stereo float WAV path.
    pub wav: PathBuf,
    /// Deterministic manifest path.
    pub manifest_path: PathBuf,
    /// Selected stem WAV paths in manifest order.
    pub stems: Vec<PathBuf>,
    /// Parsed manifest.
    pub manifest: RenderManifest,
    /// Hash of exact manifest bytes.
    pub manifest_sha256: String,
}

#[derive(Debug, Clone)]
struct PreparedStem {
    plan: RenderStemPlan,
    channel_index: usize,
}

/// Resolves selected stem owners, taps, filenames, and Master alignment.
pub fn plan_render_stems(
    compiled: &CompiledProject,
    requests: &[RenderStemRequest],
) -> Result<Vec<RenderStemPlan>, EngineError> {
    Ok(prepare_render_stems(compiled, requests)?
        .into_iter()
        .map(|stem| stem.plan)
        .collect())
}

fn prepare_render_stems(
    compiled: &CompiledProject,
    requests: &[RenderStemRequest],
) -> Result<Vec<PreparedStem>, EngineError> {
    if requests.len() > MAX_RENDER_STEMS {
        return Err(EngineError::Compile(format!(
            "at most {MAX_RENDER_STEMS} stems may be selected"
        )));
    }
    let inspection = compiled.mix_graph_inspection();
    let master_latency = inspection
        .channels
        .iter()
        .find(|channel| channel.role == ChannelRole::Master)
        .ok_or_else(|| EngineError::Compile("compiled mix has no Master channel".to_owned()))?
        .output_latency_frames;
    let mut unique = BTreeSet::new();
    for request in requests {
        if !unique.insert(request.clone()) {
            return Err(EngineError::Compile(format!(
                "stem `{}` at `{}` was selected more than once",
                request.owner,
                tap_name(request.tap)
            )));
        }
    }

    unique
        .into_iter()
        .map(|request| {
            let identity = request.owner.to_string();
            let channel = inspection
                .channels
                .iter()
                .find(|channel| channel.identity == identity)
                .ok_or_else(|| {
                    EngineError::Compile(format!("stem owner `{identity}` does not exist"))
                })?;
            let expected_role = match request.owner {
                ChannelRef::Track(_) => ChannelRole::Track,
                ChannelRef::Group(_) => ChannelRole::Group,
                ChannelRef::Return(_) => {
                    return Err(EngineError::Compile(
                        "stem owner must be a Track or Group".to_owned(),
                    ));
                }
            };
            if channel.role != expected_role {
                return Err(EngineError::Compile(format!(
                    "stem owner `{identity}` has an inconsistent compiled role"
                )));
            }
            let alignment_delay_frames = master_latency
                .checked_sub(channel.output_latency_frames)
                .ok_or_else(|| {
                    EngineError::Compile(format!(
                        "stem `{identity}` latency exceeds Master output latency"
                    ))
                })?;
            let file = stem_file_name(&request.owner, request.tap)?;
            Ok(PreparedStem {
                channel_index: channel.index,
                plan: RenderStemPlan {
                    owner: request.owner,
                    tap: request.tap,
                    file,
                    alignment_delay_frames,
                    processing_policy: stem_processing_policy(request.tap),
                },
            })
        })
        .collect()
}

fn stem_file_name(owner: &ChannelRef, tap: TapPoint) -> Result<String, EngineError> {
    match owner {
        ChannelRef::Track(id) if !id.is_empty() => {
            Ok(format!("stem-track-{id}-{}.wav", tap_name(tap)))
        }
        ChannelRef::Group(id) if !id.is_empty() => {
            Ok(format!("stem-group-{id}-{}.wav", tap_name(tap)))
        }
        ChannelRef::Track(_) | ChannelRef::Group(_) => Err(EngineError::Compile(
            "stem owner ID cannot be empty".to_owned(),
        )),
        ChannelRef::Return(_) => Err(EngineError::Compile(
            "stem owner must be a Track or Group".to_owned(),
        )),
    }
}

const fn stem_processing_policy(tap: TapPoint) -> RenderStemProcessingPolicy {
    RenderStemProcessingPolicy {
        includes_upstream_main_routing: true,
        includes_owner_processing: true,
        includes_owner_fader_panner_mute: matches!(tap, TapPoint::PostFader),
        includes_outgoing_sends: false,
        includes_downstream_processing: false,
        includes_master_processing: false,
    }
}

const fn tap_name(tap: TapPoint) -> &'static str {
    match tap {
        TapPoint::PreFader => "pre_fader",
        TapPoint::PostFader => "post_fader",
    }
}

fn stem_spool_path(staging: &Path, index: usize) -> PathBuf {
    staging.join(format!(".stem-spool-{index}.raw"))
}

fn encode_stem_frames(frames: &[[f32; 2]], encoded: &mut Vec<u8>) {
    encoded.clear();
    encoded.reserve(frames.len() * 2 * size_of::<f32>());
    for frame in frames {
        encoded.extend_from_slice(&frame[0].to_le_bytes());
        encoded.extend_from_slice(&frame[1].to_le_bytes());
    }
}

fn materialize_stem_spools(
    staging: &Path,
    prepared_stems: &[PreparedStem],
    spool_paths: &[PathBuf],
    specification: hound::WavSpec,
    frame_count: u64,
) -> Result<(), EngineError> {
    let expected_spools = prepared_stems.len().div_ceil(STEMS_PER_SPOOL);
    if spool_paths.len() != expected_spools {
        return Err(EngineError::Compile(
            "temporary stem spool set changed during render".to_owned(),
        ));
    }

    let mut encoded = vec![0_u8; RENDER_BLOCK_FRAMES * 2 * size_of::<f32>()];
    for (spool_index, spool_path) in spool_paths.iter().enumerate() {
        let first_stem = spool_index * STEMS_PER_SPOOL;
        let stems =
            &prepared_stems[first_stem..(first_stem + STEMS_PER_SPOOL).min(prepared_stems.len())];
        let mut reader = BufReader::new(File::open(spool_path)?);
        let mut writers = stems
            .iter()
            .map(|stem| hound::WavWriter::create(staging.join(&stem.plan.file), specification))
            .collect::<Result<Vec<_>, _>>()?;
        let zero_frames = stems
            .iter()
            .map(|stem| u64::from(stem.plan.alignment_delay_frames).min(frame_count))
            .collect::<Vec<_>>();
        for (writer, zero_frames) in writers.iter_mut().zip(&zero_frames) {
            for _ in 0..*zero_frames {
                writer.write_sample(0.0_f32)?;
                writer.write_sample(0.0_f32)?;
            }
        }

        let mut block_start = 0_u64;
        while block_start < frame_count {
            let block_frames = (frame_count - block_start).min(RENDER_BLOCK_FRAMES as u64) as usize;
            let encoded_bytes = block_frames * 2 * size_of::<f32>();
            for (writer, zero_frames) in writers.iter_mut().zip(&zero_frames) {
                reader.read_exact(&mut encoded[..encoded_bytes])?;
                let source_frames = frame_count - *zero_frames;
                for (offset, frame) in encoded[..encoded_bytes]
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .enumerate()
                {
                    if block_start + offset as u64 >= source_frames {
                        continue;
                    }
                    let left = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
                    let right = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
                    writer.write_sample(left)?;
                    writer.write_sample(right)?;
                }
            }
            block_start += block_frames as u64;
        }
        let mut trailing = [0_u8; 1];
        if reader.read(&mut trailing)? != 0 {
            return Err(EngineError::Compile(
                "temporary stem spool contains data beyond the render range".to_owned(),
            ));
        }
        for writer in writers {
            writer.finalize()?;
        }
        for stem in stems {
            File::open(staging.join(&stem.plan.file))?.sync_all()?;
        }
        drop(reader);
        fs::remove_file(spool_path)?;
    }
    Ok(())
}

const FRAME_BYTES: usize = 2 * size_of::<f32>();

#[derive(Clone, Copy)]
struct ShardStemLayout {
    total_frames: u64,
    arrangement_frames: u64,
    stems_in_shard: usize,
    stem_index: usize,
    alignment_delay_frames: u32,
}

fn add_shard_stem_range(
    file: &mut File,
    layout: ShardStemLayout,
    src_start: u64,
    count: usize,
    scratch: &mut [u8],
    accum: &mut [[f64; 2]],
) -> Result<(), EngineError> {
    let mut cursor = src_start;
    let mut written = 0_usize;
    while written < count {
        let block_index = cursor / RENDER_BLOCK_FRAMES as u64;
        let block_start = block_index * RENDER_BLOCK_FRAMES as u64;
        let block_len = (layout.total_frames - block_start).min(RENDER_BLOCK_FRAMES as u64);
        let offset_in_block = cursor - block_start;
        let take = ((block_len - offset_in_block) as usize).min(count - written);
        let frame_offset = block_index * RENDER_BLOCK_FRAMES as u64 * layout.stems_in_shard as u64
            + layout.stem_index as u64 * block_len
            + offset_in_block;
        file.seek(SeekFrom::Start(frame_offset * FRAME_BYTES as u64))?;
        let byte_len = take * FRAME_BYTES;
        file.read_exact(&mut scratch[..byte_len])?;
        for (offset, frame) in scratch[..byte_len].as_chunks::<8>().0.iter().enumerate() {
            let left = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
            let right = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
            accum[written + offset][0] += f64::from(left);
            accum[written + offset][1] += f64::from(right);
        }
        cursor += take as u64;
        written += take;
    }
    Ok(())
}

fn fold_shard_stem_to_wav(
    file: &mut File,
    wav_path: &Path,
    specification: hound::WavSpec,
    layout: ShardStemLayout,
) -> Result<(), EngineError> {
    if layout.arrangement_frames == 0 {
        return Err(EngineError::Compile(
            "wrap tail policy requires a positive arrangement length".to_owned(),
        ));
    }
    let expected_spool_bytes = layout
        .total_frames
        .checked_mul(layout.stems_in_shard as u64)
        .and_then(|frames| frames.checked_mul(FRAME_BYTES as u64))
        .ok_or_else(|| EngineError::Compile("temporary render spool size overflowed".to_owned()))?;
    if file.metadata()?.len() != expected_spool_bytes {
        return Err(EngineError::Compile(
            "temporary render spool size does not match the render range".to_owned(),
        ));
    }

    let mut writer = hound::WavWriter::create(wav_path, specification)?;
    let mut scratch = vec![0_u8; RENDER_BLOCK_FRAMES * FRAME_BYTES];
    let delay = u64::from(layout.alignment_delay_frames);

    if layout.arrangement_frames <= MAX_IN_MEMORY_WRAP_FRAMES {
        let mut accum = vec![[0.0_f64; 2]; layout.arrangement_frames as usize];
        let mut block_start = 0_u64;
        while block_start < layout.total_frames {
            let block_len = (layout.total_frames - block_start).min(RENDER_BLOCK_FRAMES as u64);
            let block_index = block_start / RENDER_BLOCK_FRAMES as u64;
            let frame_offset =
                block_index * RENDER_BLOCK_FRAMES as u64 * layout.stems_in_shard as u64
                    + layout.stem_index as u64 * block_len;
            file.seek(SeekFrom::Start(frame_offset * FRAME_BYTES as u64))?;
            let byte_len = block_len as usize * FRAME_BYTES;
            file.read_exact(&mut scratch[..byte_len])?;
            for (offset, frame) in scratch[..byte_len].as_chunks::<8>().0.iter().enumerate() {
                let aligned = block_start + offset as u64 + delay;
                if aligned >= layout.total_frames {
                    break;
                }
                let dst = (aligned % layout.arrangement_frames) as usize;
                let left = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
                let right = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
                accum[dst][0] += f64::from(left);
                accum[dst][1] += f64::from(right);
            }
            block_start += block_len;
        }
        for (index, frame) in accum.iter().enumerate() {
            let left = frame[0] as f32;
            let right = frame[1] as f32;
            if !left.is_finite() || !right.is_finite() {
                return Err(EngineError::NonFinite {
                    frame: index as u64,
                });
            }
            writer.write_sample(left)?;
            writer.write_sample(right)?;
        }
    } else {
        let mut accum = vec![[0.0_f64; 2]; WRAP_CHUNK_FRAMES];
        let mut out_start = 0_u64;
        while out_start < layout.arrangement_frames {
            let chunk_len =
                (layout.arrangement_frames - out_start).min(WRAP_CHUNK_FRAMES as u64) as usize;
            accum[..chunk_len].fill([0.0, 0.0]);
            let mut fold_base = 0_u64;
            while fold_base < layout.total_frames {
                let aligned_start = fold_base + out_start;
                let aligned_end =
                    (fold_base + out_start + chunk_len as u64).min(layout.total_frames);
                if aligned_start < aligned_end {
                    let active_start = aligned_start.max(delay);
                    if active_start < aligned_end {
                        let dst_offset = (active_start - aligned_start) as usize;
                        let src_start = active_start - delay;
                        let count = (aligned_end - active_start) as usize;
                        add_shard_stem_range(
                            file,
                            layout,
                            src_start,
                            count,
                            &mut scratch,
                            &mut accum[dst_offset..dst_offset + count],
                        )?;
                    }
                }
                fold_base = fold_base.saturating_add(layout.arrangement_frames);
            }
            for (offset, frame) in accum[..chunk_len].iter().enumerate() {
                let left = frame[0] as f32;
                let right = frame[1] as f32;
                if !left.is_finite() || !right.is_finite() {
                    return Err(EngineError::NonFinite {
                        frame: out_start + offset as u64,
                    });
                }
                writer.write_sample(left)?;
                writer.write_sample(right)?;
            }
            out_start += chunk_len as u64;
        }
    }

    writer.finalize()?;
    File::open(wav_path)?.sync_all()?;
    Ok(())
}

fn materialize_wrapped_stem_spools(
    staging: &Path,
    prepared_stems: &[PreparedStem],
    spool_paths: &[PathBuf],
    specification: hound::WavSpec,
    total_frames: u64,
    arrangement_frames: u64,
) -> Result<(), EngineError> {
    let expected_spools = prepared_stems.len().div_ceil(STEMS_PER_SPOOL);
    if spool_paths.len() != expected_spools {
        return Err(EngineError::Compile(
            "temporary stem spool set changed during render".to_owned(),
        ));
    }
    for (spool_index, spool_path) in spool_paths.iter().enumerate() {
        let first_stem = spool_index * STEMS_PER_SPOOL;
        let stems =
            &prepared_stems[first_stem..(first_stem + STEMS_PER_SPOOL).min(prepared_stems.len())];
        let mut file = File::open(spool_path)?;
        for (stem_index, stem) in stems.iter().enumerate() {
            fold_shard_stem_to_wav(
                &mut file,
                &staging.join(&stem.plan.file),
                specification,
                ShardStemLayout {
                    total_frames,
                    arrangement_frames,
                    stems_in_shard: stems.len(),
                    stem_index,
                    alignment_delay_frames: stem.plan.alignment_delay_frames,
                },
            )?;
        }
        drop(file);
        fs::remove_file(spool_path)?;
    }
    Ok(())
}

fn validate_finalized_analysis(
    analysis: &FinalizedAudioAnalysis,
    expected_frames: u64,
) -> Result<(), EngineError> {
    if let Some(frame) = analysis.first_non_finite_frame {
        return Err(EngineError::NonFinite { frame });
    }
    if analysis.analyzed_frames != expected_frames {
        return Err(EngineError::Compile(format!(
            "finalized WAV contains {} frames; expected {expected_frames}",
            analysis.analyzed_frames
        )));
    }
    Ok(())
}

/// Renders a valid Active Revision into an atomic artifact bundle.
pub fn render_project(
    bundle: &ProjectBundle,
    options: &RenderOptions,
) -> Result<RenderResult, EngineError> {
    let project = bundle
        .publishable()
        .ok_or_else(|| EngineError::Validation(bundle.diagnostics.as_slice().to_vec()))?;
    let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)?;
    let tail_policy = options
        .tail_policy_override
        .unwrap_or(project.render_tail_policy);
    let destination = resolved_destination(&options.output)?;
    validate_render_destination(bundle, &destination)?;
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "render output has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "render output {} exists and is not a real directory",
                        destination.display()
                    ),
                )
                .into());
            }
            if !options.replace {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("render bundle {} already exists", destination.display()),
                )
                .into());
            }
            validate_existing_render_bundle(&destination)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let staging = staging_path(&destination)?;
    fs::create_dir(&staging)?;
    #[cfg(test)]
    let fail_after_stem_spool = options.fail_after_stem_spool;
    #[cfg(not(test))]
    let fail_after_stem_spool = false;
    let result = render_staging(
        bundle,
        &compiled,
        &staging,
        &options.stems,
        tail_policy,
        fail_after_stem_spool,
    )
    .and_then(|manifest| {
        publish_directory(&staging, &destination, options.replace)?;
        let manifest_bytes = fs::read(destination.join(MANIFEST_FILE))?;
        let stems = manifest
            .stems
            .iter()
            .map(|stem| destination.join(&stem.plan.file))
            .collect();
        Ok(RenderResult {
            output: destination.clone(),
            wav: destination.join(WAV_FILE),
            manifest_path: destination.join(MANIFEST_FILE),
            stems,
            manifest,
            manifest_sha256: hash_bytes(&manifest_bytes),
        })
    });
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn render_staging(
    bundle: &ProjectBundle,
    compiled: &CompiledProject,
    staging: &Path,
    requests: &[RenderStemRequest],
    tail_policy: RenderTailPolicy,
    fail_after_stem_spool: bool,
) -> Result<RenderManifest, EngineError> {
    let prepared_stems = prepare_render_stems(compiled, requests)?;
    let wav_path = staging.join(WAV_FILE);
    let specification = hound::WavSpec {
        channels: 2,
        sample_rate: compiled.sample_rate(),
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let master_spool_path = staging.join(MASTER_SPOOL_FILE);
    let mut direct_writer = match tail_policy {
        RenderTailPolicy::Append => Some(hound::WavWriter::create(&wav_path, specification)?),
        RenderTailPolicy::Wrap => None,
    };
    let mut master_spool = match tail_policy {
        RenderTailPolicy::Append => None,
        RenderTailPolicy::Wrap => Some(BufWriter::new(File::create(&master_spool_path)?)),
    };
    let stem_spool_paths = (0..prepared_stems.len().div_ceil(STEMS_PER_SPOOL))
        .map(|index| stem_spool_path(staging, index))
        .collect::<Vec<_>>();
    let mut stem_spools = stem_spool_paths
        .iter()
        .map(|path| File::create(path).map(BufWriter::new))
        .collect::<Result<Vec<_>, _>>()?;
    let mut runtime = compiled.runtime();
    let mut block = vec![[0.0_f32, 0.0_f32]; RENDER_BLOCK_FRAMES];
    let mut stem_blocks = vec![vec![[0.0_f32, 0.0_f32]; block.len()]; prepared_stems.len()];
    let mut encoded_frames = Vec::with_capacity(RENDER_BLOCK_FRAMES * FRAME_BYTES);
    let mut pre_wrap_peak = 0.0_f64;
    while !runtime.finished() {
        let mut count = 0_usize;
        while count < block.len() && !runtime.finished() {
            let sample = runtime.process_frame(ProcessingMode::Offline)?;
            pre_wrap_peak = pre_wrap_peak
                .max(f64::from(sample[0].abs()))
                .max(f64::from(sample[1].abs()));
            block[count] = sample;
            for (prepared, stem_block) in prepared_stems.iter().zip(&mut stem_blocks) {
                let sample = runtime
                    .channel_tap_sample(prepared.channel_index, prepared.plan.tap)
                    .ok_or_else(|| {
                        EngineError::Compile(format!(
                            "compiled stem `{}` has no runtime channel tap",
                            prepared.plan.owner
                        ))
                    })?;
                stem_block[count] = sample;
            }
            count += 1;
        }
        if let Some(writer) = direct_writer.as_mut() {
            for frame in &block[..count] {
                writer.write_sample(frame[0])?;
                writer.write_sample(frame[1])?;
            }
        }
        if let Some(spool) = master_spool.as_mut() {
            encode_stem_frames(&block[..count], &mut encoded_frames);
            spool.write_all(&encoded_frames)?;
        }
        for (spool, stem_chunk) in stem_spools
            .iter_mut()
            .zip(stem_blocks.chunks(STEMS_PER_SPOOL))
        {
            for stem_block in stem_chunk {
                encode_stem_frames(&stem_block[..count], &mut encoded_frames);
                spool.write_all(&encoded_frames)?;
            }
        }
    }
    let feedback_mutes = runtime.telemetry().feedback_mutes;
    if let Some(writer) = direct_writer {
        writer.finalize()?;
        File::open(&wav_path)?.sync_all()?;
    }
    if let Some(mut spool) = master_spool {
        spool.flush()?;
        spool.get_ref().sync_all()?;
    }
    for mut spool in stem_spools {
        spool.flush()?;
        spool.get_ref().sync_all()?;
    }
    if fail_after_stem_spool {
        return Err(EngineError::Compile(
            "injected failure after stem spool finalization".to_owned(),
        ));
    }
    let total_frames = compiled.total_frames();
    let arrangement_frames = compiled.arrangement_frames();
    let output_frames = match tail_policy {
        RenderTailPolicy::Append => {
            materialize_stem_spools(
                staging,
                &prepared_stems,
                &stem_spool_paths,
                specification,
                total_frames,
            )?;
            total_frames
        }
        RenderTailPolicy::Wrap => {
            let mut master_file = File::open(&master_spool_path)?;
            fold_shard_stem_to_wav(
                &mut master_file,
                &wav_path,
                specification,
                ShardStemLayout {
                    total_frames,
                    arrangement_frames,
                    stems_in_shard: 1,
                    stem_index: 0,
                    alignment_delay_frames: 0,
                },
            )?;
            drop(master_file);
            fs::remove_file(&master_spool_path)?;
            materialize_wrapped_stem_spools(
                staging,
                &prepared_stems,
                &stem_spool_paths,
                specification,
                total_frames,
                arrangement_frames,
            )?;
            arrangement_frames
        }
    };
    let analysis = analyze_finalized_wav(&wav_path)?;
    validate_finalized_analysis(&analysis, output_frames)?;
    let peak = analysis.sample_peak as f32;
    let pre_wrap_peak = (tail_policy == RenderTailPolicy::Wrap).then_some(pre_wrap_peak);
    let wrap_peak_exceeded = pre_wrap_peak.is_some_and(|pre| analysis.sample_peak > pre);
    let wav_sha256 = hash_file(&wav_path)?;
    let mut stems = Vec::with_capacity(prepared_stems.len());
    for prepared in prepared_stems {
        let path = staging.join(&prepared.plan.file);
        let stem_analysis = analyze_finalized_wav(&path)?;
        validate_finalized_analysis(&stem_analysis, output_frames)?;
        stems.push(RenderStemManifest {
            plan: prepared.plan,
            sample_rate: compiled.sample_rate(),
            channels: 2,
            channel_layout: "stereo".to_owned(),
            sample_format: "ieee_float_32_le".to_owned(),
            frame_count: output_frames,
            analysis: stem_analysis,
            wav_sha256: hash_file(&path)?,
        });
    }
    let executable_path = std::env::current_exe()?;
    let executable_sha256 = hash_file(&executable_path)?;
    let host_os_release = host_os_release();
    let host_kernel_release = fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|release| release.trim().to_owned())
        .filter(|release| !release.is_empty());
    let reference_environment = std::env::consts::OS == "linux"
        && std::env::consts::ARCH == "x86_64"
        && host_os_release.as_deref() == Some("ubuntu/26.04");
    let project_id = bundle
        .project
        .as_ref()
        .ok_or_else(|| EngineError::Compile("render bundle has no parsed Project".to_owned()))?
        .id
        .clone();
    let manifest = RenderManifest {
        schema_version: RENDER_SCHEMA,
        project_id,
        project_revision: bundle.revision.clone(),
        crump_version: env!("CARGO_PKG_VERSION").to_owned(),
        executable_sha256,
        target_os: std::env::consts::OS.to_owned(),
        target_arch: std::env::consts::ARCH.to_owned(),
        host_os_release,
        host_kernel_release,
        reference_environment,
        sample_rate: compiled.sample_rate(),
        channels: 2,
        sample_format: "ieee_float_32_le".to_owned(),
        arrangement_frames,
        render_tail_frames: total_frames - arrangement_frames,
        tail_policy,
        frame_count: output_frames,
        duration_seconds: output_frames as f64 / f64::from(compiled.sample_rate()),
        peak,
        pre_wrap_peak,
        wrap_peak_exceeded,
        analysis,
        wav_sha256,
        stems,
        feedback_mutes,
    };
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    let manifest_path = staging.join(MANIFEST_FILE);
    let mut manifest_file = File::create(&manifest_path)?;
    manifest_file.write_all(&manifest_bytes)?;
    manifest_file.sync_all()?;
    File::open(staging)?.sync_all()?;
    Ok(manifest)
}

fn publish_directory(staging: &Path, destination: &Path, replace: bool) -> Result<(), EngineError> {
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "render destination has no parent",
        )
    })?;
    let directory = File::open(parent)?;
    let staging_name = staging.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "staging path has no name")
    })?;
    let destination_name = destination.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "output path has no name")
    })?;
    if destination.exists() {
        if !replace {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "render bundle exists",
            )
            .into());
        }
        validate_existing_render_bundle(destination)?;
        renameat_with(
            &directory,
            staging_name,
            &directory,
            destination_name,
            RenameFlags::EXCHANGE,
        )
        .map_err(std::io::Error::from)?;
        directory.sync_all()?;
        fs::remove_dir_all(staging)?;
        directory.sync_all()?;
    } else {
        renameat_with(
            &directory,
            staging_name,
            &directory,
            destination_name,
            RenameFlags::NOREPLACE,
        )
        .map_err(std::io::Error::from)?;
        directory.sync_all()?;
    }
    Ok(())
}

fn resolved_destination(output: &Path) -> Result<PathBuf, std::io::Error> {
    let absolute = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()?.join(output)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    let destination_name = normalized.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "render output requires a directory name",
        )
    })?;
    let mut cursor = normalized.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "render output has no parent",
        )
    })?;
    let mut missing = Vec::new();
    while !cursor.exists() {
        let name = cursor.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "render output has no existing ancestor",
            )
        })?;
        missing.push(name.to_os_string());
        cursor = cursor.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "render output has no existing ancestor",
            )
        })?;
    }
    let mut resolved = fs::canonicalize(cursor)?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    resolved.push(destination_name);
    Ok(resolved)
}

pub(crate) fn inspect_render_destination(
    bundle: &ProjectBundle,
    output: &Path,
) -> Result<(PathBuf, bool), EngineError> {
    let destination = resolved_destination(output)?;
    validate_render_destination(bundle, &destination)?;
    match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "render output {} exists and is not a real directory",
                        destination.display()
                    ),
                )
                .into());
            }
            validate_existing_render_bundle(&destination)?;
            Ok((destination, true))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((destination, false)),
        Err(error) => Err(error.into()),
    }
}

fn validate_render_destination(
    bundle: &ProjectBundle,
    destination: &Path,
) -> Result<(), std::io::Error> {
    let root = fs::canonicalize(&bundle.root)?;
    let metadata = root.join(".crump");
    let contains_project = root.starts_with(destination);
    let contains_source = bundle
        .sources
        .keys()
        .map(|path| root.join(path))
        .any(|path| path.starts_with(destination));
    if contains_project || contains_source || destination.starts_with(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "render destination {} overlaps Project source or history",
                destination.display()
            ),
        ));
    }
    Ok(())
}

fn validate_existing_render_bundle(destination: &Path) -> Result<(), EngineError> {
    let manifest_path = destination.join(MANIFEST_FILE);
    let manifest_metadata = match fs::symlink_metadata(&manifest_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(unrecognized_render_bundle(destination));
        }
        Err(error) => return Err(error.into()),
    };
    if !manifest_metadata.file_type().is_file() {
        return Err(unrecognized_render_bundle(destination));
    }
    if manifest_metadata.len() > MAX_RENDER_MANIFEST_BYTES {
        return Err(invalid_render_bundle(format!(
            "existing render manifest exceeds {MAX_RENDER_MANIFEST_BYTES} bytes"
        )));
    }
    let mut manifest_bytes = Vec::new();
    File::open(&manifest_path)?
        .take(MAX_RENDER_MANIFEST_BYTES + 1)
        .read_to_end(&mut manifest_bytes)?;
    if manifest_bytes.len() as u64 > MAX_RENDER_MANIFEST_BYTES {
        return Err(invalid_render_bundle(format!(
            "existing render manifest exceeds {MAX_RENDER_MANIFEST_BYTES} bytes"
        )));
    }
    let manifest: RenderManifest = serde_json::from_slice(&manifest_bytes)?;
    let expected_frame_count = match manifest.tail_policy {
        RenderTailPolicy::Append => manifest
            .arrangement_frames
            .checked_add(manifest.render_tail_frames),
        RenderTailPolicy::Wrap => Some(manifest.arrangement_frames),
    };
    let wrap_metadata_consistent = match manifest.tail_policy {
        RenderTailPolicy::Append => {
            manifest.pre_wrap_peak.is_none() && !manifest.wrap_peak_exceeded
        }
        RenderTailPolicy::Wrap => manifest.pre_wrap_peak.is_some_and(|pre| {
            pre.is_finite()
                && pre >= 0.0
                && manifest.wrap_peak_exceeded == (manifest.analysis.sample_peak > pre)
        }),
    };
    if manifest.schema_version != RENDER_SCHEMA
        || manifest.channels != 2
        || manifest.sample_format != "ieee_float_32_le"
        || manifest.stems.len() > MAX_RENDER_STEMS
        || expected_frame_count != Some(manifest.frame_count)
        || !wrap_metadata_consistent
    {
        return Err(invalid_render_bundle(
            "existing render manifest has an unsupported format",
        ));
    }

    let mut expected_names = vec![MANIFEST_FILE.to_owned(), WAV_FILE.to_owned()];
    for stem in &manifest.stems {
        if stem.plan.file != stem_file_name(&stem.plan.owner, stem.plan.tap)?
            || stem.plan.processing_policy != stem_processing_policy(stem.plan.tap)
            || stem.sample_rate != manifest.sample_rate
            || stem.channels != manifest.channels
            || stem.channel_layout != "stereo"
            || stem.sample_format != manifest.sample_format
            || stem.frame_count != manifest.frame_count
            || !manifest_analysis_is_consistent(&stem.analysis, stem.frame_count)
        {
            return Err(invalid_render_bundle(
                "existing stem metadata is inconsistent with its manifest",
            ));
        }
        expected_names.push(stem.plan.file.clone());
    }
    expected_names.sort();
    if expected_names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(invalid_render_bundle(
            "existing render manifest contains duplicate artifact names",
        ));
    }

    let mut names = fs::read_dir(destination)?
        .map(|entry| {
            entry.and_then(|entry| {
                entry.file_name().into_string().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "render bundle contains a non-UTF-8 entry name",
                    )
                })
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    if names != expected_names {
        return Err(unrecognized_render_bundle(destination));
    }
    for name in &expected_names {
        if !fs::symlink_metadata(destination.join(name))?
            .file_type()
            .is_file()
        {
            return Err(invalid_render_bundle(format!(
                "render bundle artifact `{name}` is not a regular file"
            )));
        }
    }

    let analysis = &manifest.analysis;
    if !manifest_analysis_is_consistent(analysis, manifest.frame_count)
        || analysis.sample_peak as f32 != manifest.peak
    {
        return Err(invalid_render_bundle(
            "existing render analysis is inconsistent with its manifest",
        ));
    }
    validate_existing_wav(
        &destination.join(WAV_FILE),
        &manifest.wav_sha256,
        manifest.sample_rate,
        manifest.channels,
        manifest.frame_count,
        analysis,
    )?;
    for stem in &manifest.stems {
        validate_existing_wav(
            &destination.join(&stem.plan.file),
            &stem.wav_sha256,
            stem.sample_rate,
            stem.channels,
            stem.frame_count,
            &stem.analysis,
        )?;
    }
    Ok(())
}

fn validate_existing_wav(
    path: &Path,
    expected_sha256: &str,
    sample_rate: u32,
    channels: u16,
    frame_count: u64,
    expected_analysis: &FinalizedAudioAnalysis,
) -> Result<(), EngineError> {
    if hash_file(path)? != expected_sha256 {
        return Err(invalid_render_bundle(format!(
            "existing render WAV `{}` does not match its manifest hash",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<unknown>")
        )));
    }
    let reader = hound::WavReader::open(path)?;
    let specification = reader.spec();
    if specification.channels != channels
        || specification.sample_rate != sample_rate
        || specification.bits_per_sample != 32
        || specification.sample_format != hound::SampleFormat::Float
        || u64::from(reader.duration()) != frame_count
    {
        return Err(invalid_render_bundle(format!(
            "existing render WAV `{}` format or frame count does not match its manifest",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<unknown>")
        )));
    }
    let actual_analysis = analyze_finalized_wav(path)?;
    if !finalized_analysis_matches(expected_analysis, &actual_analysis) {
        return Err(invalid_render_bundle(format!(
            "existing render WAV `{}` analysis does not describe its exact bytes",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<unknown>")
        )));
    }
    Ok(())
}

fn finalized_analysis_matches(
    expected: &FinalizedAudioAnalysis,
    actual: &FinalizedAudioAnalysis,
) -> bool {
    expected.schema_version == actual.schema_version
        && expected.analyzed_frames == actual.analyzed_frames
        && close_analysis_value(expected.sample_peak, actual.sample_peak)
        && close_optional_analysis_value(expected.sample_peak_dbfs, actual.sample_peak_dbfs)
        && close_analysis_value(expected.true_peak, actual.true_peak)
        && close_optional_analysis_value(expected.true_peak_dbtp, actual.true_peak_dbtp)
        && expected.true_peak_oversampling_factor == actual.true_peak_oversampling_factor
        && expected.true_peak_kernel == actual.true_peak_kernel
        && expected.loudness_method == actual.loudness_method
        && close_optional_analysis_value(expected.integrated_lufs, actual.integrated_lufs)
        && close_optional_analysis_value(
            expected.maximum_short_term_lufs,
            actual.maximum_short_term_lufs,
        )
        && close_optional_analysis_value(expected.loudness_range_lu, actual.loudness_range_lu)
        && close_optional_analysis_value(expected.rms_dbfs, actual.rms_dbfs)
        && close_optional_analysis_value(expected.active_rms_dbfs, actual.active_rms_dbfs)
        && close_analysis_value(expected.active_rms_gate_dbfs, actual.active_rms_gate_dbfs)
        && expected.active_rms_method == actual.active_rms_method
        && expected
            .dc_offset
            .into_iter()
            .zip(actual.dc_offset)
            .all(|(expected, actual)| close_analysis_value(expected, actual))
        && expected.clipped_sample_count == actual.clipped_sample_count
        && expected.clipped_frame_count == actual.clipped_frame_count
        && expected.non_finite_sample_count == actual.non_finite_sample_count
        && expected.first_non_finite_frame == actual.first_non_finite_frame
}

fn close_optional_analysis_value(expected: Option<f64>, actual: Option<f64>) -> bool {
    match (expected, actual) {
        (Some(expected), Some(actual)) => close_analysis_value(expected, actual),
        (None, None) => true,
        _ => false,
    }
}

fn close_analysis_value(expected: f64, actual: f64) -> bool {
    let scale = expected.abs().max(actual.abs()).max(1.0);
    (expected - actual).abs() <= f64::EPSILON * 16.0 * scale
}

fn manifest_analysis_is_consistent(analysis: &FinalizedAudioAnalysis, frame_count: u64) -> bool {
    analysis.schema_version == ANALYSIS_SCHEMA
        && analysis.analyzed_frames == frame_count
        && analysis.sample_peak.is_finite()
        && analysis.sample_peak >= 0.0
        && analysis.true_peak.is_finite()
        && analysis.true_peak >= 0.0
        && analysis.active_rms_gate_dbfs.is_finite()
        && analysis.active_rms_method == ACTIVE_RMS_METHOD
        && analysis.dc_offset.into_iter().all(f64::is_finite)
        && analysis
            .sample_peak_dbfs
            .into_iter()
            .chain(analysis.true_peak_dbtp)
            .chain(analysis.integrated_lufs)
            .chain(analysis.maximum_short_term_lufs)
            .chain(analysis.loudness_range_lu)
            .chain(analysis.rms_dbfs)
            .chain(analysis.active_rms_dbfs)
            .all(f64::is_finite)
        && analysis.clipped_frame_count <= frame_count
        && analysis.clipped_sample_count <= frame_count.saturating_mul(2)
        && analysis.non_finite_sample_count == 0
        && analysis.first_non_finite_frame.is_none()
}

fn unrecognized_render_bundle(destination: &Path) -> EngineError {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "refusing to replace {} because it is not an exact complete Crump render bundle",
            destination.display()
        ),
    )
    .into()
}

fn invalid_render_bundle(message: impl Into<String>) -> EngineError {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}

fn staging_path(destination: &Path) -> Result<PathBuf, std::io::Error> {
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "render output has no parent",
        )
    })?;
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "render output name is not UTF-8",
            )
        })?;
    for _ in 0..100 {
        let counter = RENDER_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".{name}.rendering-{}-{counter}",
            std::process::id()
        ));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a render staging directory",
    ))
}

fn host_os_release() -> Option<String> {
    let source = fs::read_to_string("/etc/os-release").ok()?;
    let value = |key: &str| {
        source.lines().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name == key).then(|| value.trim().trim_matches('"').to_owned())
        })
    };
    let id = value("ID")?;
    let version = value("VERSION_ID")
        .or_else(|| value("BUILD_ID"))
        .unwrap_or_default();
    Some(format!("{id}/{version}"))
}

fn hash_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1_024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(hasher.finalize().as_slice()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    hex_digest(Sha256::digest(bytes).as_slice())
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use tempfile::tempdir;

    use crate::{engine::schedule::MAX_RUNTIME_TRACKS, project::load_project};

    use super::*;

    const MAX_STEM_GROUPS: usize = MAX_MIX_CHANNELS - MAX_RUNTIME_TRACKS - 1;

    fn maximum_stem_source() -> String {
        let mut source = r#"crump 1
project "Maximum Stems" {
  id maximum_stems
  seed 42
  sample_rate 48000
  render_tail 0s
  song {
    tempo 120bpm
    meter 4/4
    end 1/100beat
    instrument drums "Drums" stock_drums {
      polyphony 1
      parameter tone "Tone" ratio 0 1 0.5 0.5
      parameter punch "Punch" ratio 0 1 0.5 0.5
      faceplate {
        control tone "Tone" "Brightness" maps tone
        control punch "Punch" "Impact" maps punch
      }
    }
    pattern empty 1beat {
    }
"#
        .to_owned();
        for index in 0..MAX_RUNTIME_TRACKS {
            source.push_str(&format!(
                "    track t{index:02} \"Track {index}\" instrument drums level 0db pan 0 {{\n      output group/g{index:02}\n    }}\n"
            ));
        }
        for index in 0..MAX_STEM_GROUPS {
            source.push_str(&format!(
                "    group g{index:02} \"Group {index}\" level 0db {{\n      output master\n    }}\n"
            ));
        }
        source.push_str("    master level 0db {\n    }\n  }\n}\n");
        source
    }

    fn directory_bytes(path: &Path) -> BTreeMap<String, Vec<u8>> {
        fs::read_dir(path)
            .expect("read bundle")
            .map(|entry| {
                let entry = entry.expect("bundle entry");
                let name = entry.file_name().to_string_lossy().into_owned();
                let bytes = fs::read(entry.path()).expect("read bundle artifact");
                (name, bytes)
            })
            .collect()
    }

    #[test]
    fn replacement_is_complete_and_no_replace_preserves_the_old_bundle() {
        let directory = tempdir().unwrap();
        let source = include_str!("../../tests/fixtures/minimal.crump");
        let source_path = directory.path().join("project.crump");
        fs::write(&source_path, source).unwrap();
        let bundle = load_project(directory.path()).unwrap();
        let output = directory.path().join("master");
        let first = render_project(&bundle, &RenderOptions::new(&output)).unwrap();
        let first_wav = fs::read(&first.wav).unwrap();
        let first_manifest = fs::read(&first.manifest_path).unwrap();
        assert_eq!(first.manifest.schema_version, 3);
        assert_eq!(first.manifest.tail_policy, RenderTailPolicy::Append);
        assert!(first.manifest.pre_wrap_peak.is_none());
        assert!(!first.manifest.wrap_peak_exceeded);
        let analysis = &first.manifest.analysis;
        assert_eq!(analysis.analyzed_frames, first.manifest.frame_count);
        assert_eq!(analysis.non_finite_sample_count, 0);
        assert_eq!(analysis.sample_peak as f32, first.manifest.peak);
        assert!(analysis.integrated_lufs.is_some());
        assert!(analysis.rms_dbfs.is_some());
        assert!(analysis.active_rms_dbfs.is_some());

        let mut no_replace = RenderOptions::new(&output);
        no_replace.replace = false;
        assert!(render_project(&bundle, &no_replace).is_err());
        assert_eq!(fs::read(&first.wav).unwrap(), first_wav);
        assert_eq!(fs::read(&first.manifest_path).unwrap(), first_manifest);

        fs::write(&source_path, source.replacen("C4", "D4", 1)).unwrap();
        let changed = load_project(directory.path()).unwrap();
        let replaced = render_project(&changed, &RenderOptions::new(&output)).unwrap();
        assert_ne!(
            replaced.manifest.project_revision,
            first.manifest.project_revision
        );
        assert_ne!(fs::read(replaced.wav).unwrap(), first_wav);
        assert!(
            fs::read_dir(directory.path())
                .unwrap()
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().contains(".rendering-"))
        );
    }

    #[test]
    fn maximum_track_and_group_stem_set_uses_bounded_spool_shards() {
        let directory = tempdir().expect("Project root");
        fs::write(
            directory.path().join("project.crump"),
            maximum_stem_source(),
        )
        .expect("Project source");
        let bundle = load_project(directory.path()).expect("load maximum-stem Project");
        let output = directory.path().join("maximum-stems");
        let mut options = RenderOptions::new(&output);
        for index in 0..MAX_RUNTIME_TRACKS {
            for tap in [TapPoint::PreFader, TapPoint::PostFader] {
                options.stems.push(RenderStemRequest {
                    owner: ChannelRef::Track(format!("t{index:02}")),
                    tap,
                });
            }
        }
        for index in 0..MAX_STEM_GROUPS {
            for tap in [TapPoint::PreFader, TapPoint::PostFader] {
                options.stems.push(RenderStemRequest {
                    owner: ChannelRef::Group(format!("g{index:02}")),
                    tap,
                });
            }
        }
        let maximum_selectable_stems = (MAX_MIX_CHANNELS - 1) * 2;
        assert_eq!(options.stems.len(), maximum_selectable_stems);
        assert_eq!(
            options.stems.len().div_ceil(STEMS_PER_SPOOL),
            maximum_selectable_stems.div_ceil(STEMS_PER_SPOOL)
        );

        let result = render_project(&bundle, &options).expect("render maximum stem set");
        assert_eq!(result.manifest.stems.len(), maximum_selectable_stems);
        assert_eq!(directory_bytes(&output).len(), maximum_selectable_stems + 2);
        assert!(result.manifest.stems.iter().all(|stem| {
            stem.frame_count == result.manifest.frame_count
                && stem.analysis.analyzed_frames == result.manifest.frame_count
                && output.join(&stem.plan.file).is_file()
        }));
        assert!(
            fs::read_dir(directory.path())
                .expect("Project root entries")
                .all(|entry| {
                    let name = entry
                        .expect("Project root entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned();
                    !name.contains(".rendering-") && !name.contains(".stem-spool-")
                })
        );
    }

    #[test]
    fn injected_post_spool_failure_preserves_destination_and_cleans_staging() {
        let directory = tempdir().expect("Project root");
        fs::write(
            directory.path().join("project.crump"),
            maximum_stem_source(),
        )
        .expect("Project source");
        let bundle = load_project(directory.path()).expect("load stem Project");
        let output = directory.path().join("preserved");
        let request = RenderStemRequest {
            owner: ChannelRef::Track("t00".to_owned()),
            tap: TapPoint::PostFader,
        };
        let mut initial = RenderOptions::new(&output);
        initial.stems.push(request.clone());
        render_project(&bundle, &initial).expect("initial render");
        let before = directory_bytes(&output);

        let mut interrupted = RenderOptions::new(&output);
        interrupted.stems.push(request);
        interrupted.fail_after_stem_spool = true;
        let error = render_project(&bundle, &interrupted).expect_err("injected render failure");
        assert!(
            error
                .to_string()
                .contains("injected failure after stem spool finalization"),
            "{error}"
        );
        assert_eq!(directory_bytes(&output), before);
        assert!(
            fs::read_dir(directory.path())
                .expect("Project root entries")
                .all(|entry| {
                    let name = entry
                        .expect("Project root entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned();
                    !name.contains(".rendering-") && !name.contains(".stem-spool-")
                })
        );
    }

    #[test]
    fn selected_track_and_group_stems_are_aligned_analyzed_and_reconstruct_the_master() {
        let directory = tempdir().expect("Project root");
        let source = r#"crump 1
project "Stem Alignment" {
  id stem_alignment
  seed 42
  sample_rate 48000
  render_tail 100ms
  song {
    tempo 120bpm
    meter 4/4
    end 1beat
    instrument drums "Drums" stock_drums {
      polyphony 8
      parameter tone "Tone" ratio 0 1 0.5 0.5
      parameter punch "Punch" ratio 0 1 0.5 0.5
      faceplate {
        control tone "Tone" "Brightness" maps tone
        control punch "Punch" "Impact" maps punch
      }
    }
    pattern beat 1beat {
      hit 0beat kick 1
    }
    track a "A" instrument drums level -6db pan 0 {
      processor wait latency {
        setting frames 5
      }
      output group/bus
      clip a pattern beat start 0beat duration 1beat mode once
    }
    track b "B" instrument drums level -9db pan 0 {
      output group/bus
      clip b pattern beat start 0beat duration 1beat mode once
    }
    group bus "Bus" level 0db {
      output master
    }
    master level 0db {
    }
  }
}
"#;
        fs::write(directory.path().join("project.crump"), source).expect("Project source");
        let bundle = load_project(directory.path()).expect("Project");
        assert!(!bundle.diagnostics.has_errors());
        let output = directory.path().join("stems");
        let mut duplicate_options = RenderOptions::new(directory.path().join("duplicate"));
        duplicate_options.stems = ["track/a:pre_fader", "track/a:pre_fader"]
            .map(|value| value.parse().expect("duplicate request"))
            .to_vec();
        let duplicate = render_project(&bundle, &duplicate_options)
            .expect_err("duplicate stem selection is rejected");
        assert!(duplicate.to_string().contains("selected more than once"));
        assert!(!duplicate_options.output.exists());
        let mut unknown_options = RenderOptions::new(directory.path().join("unknown"));
        unknown_options.stems = vec!["track/missing:pre_fader".parse().expect("unknown request")];
        let unknown = render_project(&bundle, &unknown_options)
            .expect_err("unknown stem selection is rejected");
        assert!(unknown.to_string().contains("does not exist"));
        assert!(!unknown_options.output.exists());

        let stemless =
            render_project(&bundle, &RenderOptions::new(&output)).expect("initial stemless render");
        assert!(stemless.manifest.stems.is_empty());

        let mut options = RenderOptions::new(&output);
        options.stems = [
            "track/a:pre_fader",
            "track/a:post_fader",
            "track/b:post_fader",
            "group/bus:post_fader",
        ]
        .map(|value| value.parse().expect("stem request"))
        .to_vec();

        let rendered = render_project(&bundle, &options).expect("stem render");
        assert_eq!(rendered.stems.len(), 4);
        assert_eq!(rendered.manifest.stems.len(), 4);
        for stem in &rendered.manifest.stems {
            assert_eq!(stem.frame_count, rendered.manifest.frame_count);
            assert_eq!(stem.analysis.analyzed_frames, stem.frame_count);
            assert_eq!(stem.analysis.non_finite_sample_count, 0);
            assert!(output.join(&stem.plan.file).is_file());
        }
        let stem = |owner: &ChannelRef, tap: TapPoint| {
            rendered
                .manifest
                .stems
                .iter()
                .find(|stem| &stem.plan.owner == owner && stem.plan.tap == tap)
                .expect("selected stem")
        };
        assert_eq!(
            stem(&ChannelRef::Track("a".to_owned()), TapPoint::PostFader)
                .plan
                .alignment_delay_frames,
            0
        );
        assert_eq!(
            stem(&ChannelRef::Track("b".to_owned()), TapPoint::PostFader)
                .plan
                .alignment_delay_frames,
            5
        );
        assert!(
            !stem(&ChannelRef::Track("a".to_owned()), TapPoint::PreFader)
                .plan
                .processing_policy
                .includes_owner_fader_panner_mute
        );

        let read_frames = |path: &Path| {
            let mut reader = hound::WavReader::open(path).expect("open rendered WAV");
            let samples = reader
                .samples::<f32>()
                .collect::<Result<Vec<_>, _>>()
                .expect("decode rendered WAV");
            samples
                .as_chunks::<2>()
                .0
                .iter()
                .map(|frame| [frame[0], frame[1]])
                .collect::<Vec<_>>()
        };
        let a_pre = read_frames(
            &output.join(
                &stem(&ChannelRef::Track("a".to_owned()), TapPoint::PreFader)
                    .plan
                    .file,
            ),
        );
        let a_post = read_frames(
            &output.join(
                &stem(&ChannelRef::Track("a".to_owned()), TapPoint::PostFader)
                    .plan
                    .file,
            ),
        );
        let b_post = read_frames(
            &output.join(
                &stem(&ChannelRef::Track("b".to_owned()), TapPoint::PostFader)
                    .plan
                    .file,
            ),
        );
        let bus = read_frames(
            &output.join(
                &stem(&ChannelRef::Group("bus".to_owned()), TapPoint::PostFader)
                    .plan
                    .file,
            ),
        );
        let master = read_frames(&rendered.wav);
        assert_eq!(master.len(), bus.len());
        assert!(
            a_pre
                .iter()
                .flatten()
                .map(|sample| sample.abs())
                .fold(0.0_f32, f32::max)
                > a_post
                    .iter()
                    .flatten()
                    .map(|sample| sample.abs())
                    .fold(0.0_f32, f32::max)
        );
        for (((a, b), bus), master) in a_post.iter().zip(&b_post).zip(&bus).zip(&master) {
            for channel in 0..2 {
                assert!((a[channel] + b[channel] - bus[channel]).abs() < 1.0e-6);
                assert!((bus[channel] - master[channel]).abs() < 1.0e-6);
            }
        }

        let first_stem = fs::read(&rendered.stems[0]).expect("first stem bytes");
        let repeated = render_project(&bundle, &options).expect("repeat stem render");
        assert_eq!(
            fs::read(&repeated.stems[0]).expect("repeated stem bytes"),
            first_stem
        );

        let replaced_with_stemless = render_project(&bundle, &RenderOptions::new(&output))
            .expect("replace stems with a stemless bundle");
        assert!(replaced_with_stemless.manifest.stems.is_empty());
        assert!(repeated.stems.iter().all(|path| !path.exists()));
        let artifact_names = fs::read_dir(&output)
            .expect("stemless artifact set")
            .map(|entry| entry.expect("artifact").file_name())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            artifact_names,
            [MANIFEST_FILE, WAV_FILE]
                .map(std::ffi::OsString::from)
                .into_iter()
                .collect()
        );

        let repeated = render_project(&bundle, &options).expect("restore stem bundle");
        let manifest_bytes = fs::read(&repeated.manifest_path).expect("manifest bytes");
        let mut tampered_manifest: serde_json::Value =
            serde_json::from_slice(&manifest_bytes).expect("manifest JSON");
        tampered_manifest["stems"][0]["analysis"]["dc_offset"][0] = serde_json::json!(0.125);
        let tampered_manifest =
            serde_json::to_vec_pretty(&tampered_manifest).expect("tampered manifest JSON");
        fs::write(&repeated.manifest_path, &tampered_manifest).expect("tamper analysis");
        let analysis_error = render_project(&bundle, &options)
            .expect_err("manifest analysis tampering blocks replacement");
        assert!(
            analysis_error
                .to_string()
                .contains("analysis does not describe")
        );
        assert_eq!(
            fs::read(&repeated.manifest_path).expect("tampered manifest survives"),
            tampered_manifest
        );
        fs::write(&repeated.manifest_path, manifest_bytes).expect("restore manifest");

        let mut tampered_stem = first_stem;
        tampered_stem.push(0);
        fs::write(&repeated.stems[0], &tampered_stem).expect("tamper stem");
        let error =
            render_project(&bundle, &options).expect_err("tampered stem blocks replacement");
        assert!(
            error
                .to_string()
                .contains("does not match its manifest hash")
        );
        assert_eq!(
            fs::read(&repeated.stems[0]).expect("tampered stem survives"),
            tampered_stem
        );
    }

    #[test]
    fn render_refuses_to_replace_source_history_or_arbitrary_directories() {
        let directory = tempdir().unwrap();
        let source = include_str!("../../tests/fixtures/minimal.crump");
        let source_path = directory.path().join("project.crump");
        fs::write(&source_path, source).unwrap();
        let bundle = load_project(directory.path()).unwrap();

        let root_error =
            render_project(&bundle, &RenderOptions::new(directory.path())).unwrap_err();
        assert!(root_error.to_string().contains("overlaps Project source"));
        assert_eq!(fs::read_to_string(&source_path).unwrap(), source);

        let metadata = directory.path().join(".crump");
        fs::create_dir(&metadata).unwrap();
        fs::write(metadata.join("keep"), b"history").unwrap();
        let metadata_error = render_project(&bundle, &RenderOptions::new(&metadata)).unwrap_err();
        assert!(
            metadata_error
                .to_string()
                .contains("overlaps Project source")
        );
        assert_eq!(fs::read(metadata.join("keep")).unwrap(), b"history");

        let arbitrary = directory.path().join("documents");
        fs::create_dir(&arbitrary).unwrap();
        fs::write(arbitrary.join("keep"), b"user data").unwrap();
        let arbitrary_error = render_project(&bundle, &RenderOptions::new(&arbitrary)).unwrap_err();
        assert!(arbitrary_error.to_string().contains("refusing to replace"));
        assert_eq!(fs::read(arbitrary.join("keep")).unwrap(), b"user data");
    }

    #[test]
    fn render_refuses_to_replace_a_tampered_bundle() {
        let directory = tempdir().unwrap();
        let source = include_str!("../../tests/fixtures/minimal.crump");
        fs::write(directory.path().join("project.crump"), source).unwrap();
        let bundle = load_project(directory.path()).unwrap();
        let output = directory.path().join("master");
        let rendered = render_project(&bundle, &RenderOptions::new(&output)).unwrap();
        let mut tampered = fs::read(&rendered.wav).unwrap();
        tampered.push(0);
        fs::write(&rendered.wav, &tampered).unwrap();

        let error = render_project(&bundle, &RenderOptions::new(&output)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not match its manifest hash")
        );
        assert_eq!(fs::read(&rendered.wav).unwrap(), tampered);
    }

    #[test]
    fn unchanged_renders_are_byte_identical() {
        let directory = tempdir().unwrap();
        let source = include_str!("../../tests/fixtures/minimal.crump");
        fs::write(directory.path().join("project.crump"), source).unwrap();
        let bundle = load_project(directory.path()).unwrap();
        assert!(!bundle.diagnostics.has_errors());
        let first =
            render_project(&bundle, &RenderOptions::new(directory.path().join("first"))).unwrap();
        let second = render_project(
            &bundle,
            &RenderOptions::new(directory.path().join("second")),
        )
        .unwrap();
        assert_eq!(fs::read(first.wav).unwrap(), fs::read(second.wav).unwrap());
        assert_eq!(
            fs::read(first.manifest_path).unwrap(),
            fs::read(second.manifest_path).unwrap()
        );
    }

    fn read_stereo_frames(path: &Path) -> Vec<[f32; 2]> {
        let mut reader = hound::WavReader::open(path).expect("open rendered WAV");
        let samples = reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .expect("decode rendered WAV");
        samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|frame| [frame[0], frame[1]])
            .collect()
    }

    #[test]
    fn wrap_tail_folds_tail_onto_start_including_multi_wrap_and_reconstructs_stems() {
        let directory = tempdir().expect("Project root");
        // 1 beat at 120bpm = 0.5s (24,000 frames). render_tail 1500ms = 72,000 frames (3x arrangement)
        // so the tail wraps multiple times onto the start of the loop.
        let source = r#"crump 1
project "Seamless Loop" {
  id seamless_loop
  seed 42
  sample_rate 48000
  render_tail 1500ms wrap
  song {
    tempo 120bpm
    meter 4/4
    end 1beat
    instrument drums "Drums" stock_drums {
      polyphony 8
      parameter tone "Tone" ratio 0 1 0.5 0.5
      parameter punch "Punch" ratio 0 1 0.5 0.5
      faceplate {
        control tone "Tone" "Brightness" maps tone
        control punch "Punch" "Impact" maps punch
      }
    }
    pattern late_hit 1beat {
      hit 1/2beat kick 1
    }
    track a "A" instrument drums level -6db pan 0 {
      processor wait latency {
        setting frames 5
      }
      processor tail_delay delay {
        setting time 500ms
        setting feedback 0.55
        setting mix 0.5
      }
      output group/bus
      clip a pattern late_hit start 0beat duration 1beat mode once
    }
    track b "B" instrument drums level -9db pan 0 {
      processor tail_verb reverb {
        setting pre_delay 0s
        setting diffusion 0.7
        setting decay 0.85
        setting damping 0.2
        setting modulation 0.1
        setting width 1
        setting mix 0.5
      }
      output group/bus
      clip b pattern late_hit start 0beat duration 1beat mode once
    }
    group bus "Bus" level 0db {
      output master
    }
    master level 0db {
    }
  }
}
"#;
        fs::write(directory.path().join("project.crump"), source).expect("write source");
        let bundle = load_project(directory.path()).expect("load loop Project");
        assert!(!bundle.diagnostics.has_errors(), "{:?}", bundle.diagnostics);

        let stems = [
            "track/a:post_fader",
            "track/b:post_fader",
            "group/bus:post_fader",
        ]
        .map(|value| value.parse().expect("stem request"))
        .to_vec();

        // Render with explicit append override as reference.
        let append_out = directory.path().join("append");
        let mut append_options = RenderOptions::new(&append_out);
        append_options.stems = stems.clone();
        append_options.tail_policy_override = Some(RenderTailPolicy::Append);
        let appended = render_project(&bundle, &append_options).expect("append render");
        assert_eq!(appended.manifest.tail_policy, RenderTailPolicy::Append);
        assert_eq!(appended.manifest.arrangement_frames, 24_000);
        assert_eq!(appended.manifest.render_tail_frames, 72_000);
        assert_eq!(appended.manifest.frame_count, 96_000);
        assert!(appended.manifest.pre_wrap_peak.is_none());
        assert!(!appended.manifest.wrap_peak_exceeded);

        // Render with Project's wrap tail policy.
        let wrap_out = directory.path().join("wrap");
        let mut wrap_options = RenderOptions::new(&wrap_out);
        wrap_options.stems = stems;
        let wrapped = render_project(&bundle, &wrap_options).expect("wrap render");
        assert_eq!(wrapped.manifest.tail_policy, RenderTailPolicy::Wrap);
        assert_eq!(wrapped.manifest.arrangement_frames, 24_000);
        assert_eq!(wrapped.manifest.render_tail_frames, 72_000);
        assert_eq!(wrapped.manifest.frame_count, 24_000);
        assert_eq!(wrapped.manifest.analysis.analyzed_frames, 24_000);
        assert!(wrapped.manifest.pre_wrap_peak.is_some());
        assert!(wrapped.manifest.analysis.rms_dbfs.is_some());
        assert!(wrapped.manifest.analysis.active_rms_dbfs.is_some());

        let append_master = read_stereo_frames(&appended.wav);
        let wrap_master = read_stereo_frames(&wrapped.wav);
        assert_eq!(append_master.len(), 96_000);
        assert_eq!(wrap_master.len(), 24_000);

        // Tail past arrangement end must be non-silent so the fold actually adds audio onto frame 0..
        let tail_peak = append_master[24_000..]
            .iter()
            .flatten()
            .map(|sample| sample.abs())
            .fold(0.0_f32, f32::max);
        assert!(tail_peak > 1.0e-3, "expected audible tail, got {tail_peak}");

        // Exact multi-wrap fold equivalence with append reference.
        let mut expected_master = vec![[0.0_f64; 2]; 24_000];
        for (index, frame) in append_master.iter().enumerate() {
            let dst = index % 24_000;
            expected_master[dst][0] += f64::from(frame[0]);
            expected_master[dst][1] += f64::from(frame[1]);
        }
        let mut max_fold_error = 0.0_f32;
        for (actual, expected) in wrap_master.iter().zip(&expected_master) {
            for channel in 0..2 {
                let error = (actual[channel] - expected[channel] as f32).abs();
                max_fold_error = max_fold_error.max(error);
            }
        }
        assert!(
            max_fold_error < 1.0e-6,
            "wrapped master diverged from folded append master by {max_fold_error}"
        );

        // Stems wrap identically (aligned) and still reconstruct Master.
        assert_eq!(wrapped.manifest.stems.len(), 3);
        for stem in &wrapped.manifest.stems {
            assert_eq!(stem.frame_count, 24_000);
            assert_eq!(stem.analysis.analyzed_frames, 24_000);
            assert!(stem.analysis.active_rms_dbfs.is_some());
        }
        let stem_path = |owner: &ChannelRef| {
            let entry = wrapped
                .manifest
                .stems
                .iter()
                .find(|stem| &stem.plan.owner == owner && stem.plan.tap == TapPoint::PostFader)
                .expect("stem");
            wrap_out.join(&entry.plan.file)
        };
        let a_post = read_stereo_frames(&stem_path(&ChannelRef::Track("a".to_owned())));
        let b_post = read_stereo_frames(&stem_path(&ChannelRef::Track("b".to_owned())));
        let bus = read_stereo_frames(&stem_path(&ChannelRef::Group("bus".to_owned())));
        assert_eq!(a_post.len(), 24_000);
        assert_eq!(b_post.len(), 24_000);
        assert_eq!(bus.len(), 24_000);
        for (((a, b), bus), master) in a_post.iter().zip(&b_post).zip(&bus).zip(&wrap_master) {
            for channel in 0..2 {
                assert!((a[channel] + b[channel] - bus[channel]).abs() < 1.0e-5);
                assert!((bus[channel] - master[channel]).abs() < 1.0e-5);
            }
        }

        // Existing wrapped bundle passes validation and can be replaced cleanly.
        let replaced = render_project(&bundle, &wrap_options).expect("replace wrapped bundle");
        assert_eq!(
            fs::read(&replaced.wav).unwrap(),
            fs::read(&wrapped.wav).unwrap()
        );

        // Tampering with wrap metadata blocks replacement.
        let original_manifest = fs::read(&replaced.manifest_path).expect("manifest bytes");
        let mut tampered: serde_json::Value =
            serde_json::from_slice(&original_manifest).expect("parse manifest");
        tampered["pre_wrap_peak"] = serde_json::Value::Null;
        fs::write(
            &replaced.manifest_path,
            serde_json::to_vec_pretty(&tampered).unwrap(),
        )
        .unwrap();
        let error = render_project(&bundle, &wrap_options)
            .expect_err("tampered wrap metadata must block replacement");
        assert!(error.to_string().contains("unsupported format"), "{error}");
    }

    #[test]
    fn wrap_tail_flags_wrap_peak_exceeded_when_wrapped_tail_pushes_above_limiter_ceiling() {
        let directory = tempdir().expect("Project root");
        // Hit at beat 0 (limited at -12db ceiling) plus a 500ms delay (1 beat = exact loop length)
        // whose repeat lands back on beat 0 after wrapping, summing two limited peaks together.
        let source = r#"crump 1
project "Wrapped Limiter Peak" {
  id wrapped_limiter_peak
  seed 42
  sample_rate 48000
  render_tail 600ms wrap
  song {
    tempo 120bpm
    meter 4/4
    end 1beat
    instrument drums "Drums" stock_drums {
      polyphony 8
      parameter tone "Tone" ratio 0 1 0.5 0.5
      parameter punch "Punch" ratio 0 1 0.5 0.5
      faceplate {
        control tone "Tone" "Brightness" maps tone
        control punch "Punch" "Impact" maps punch
      }
    }
    pattern hit_start 1beat {
      hit 0beat kick 1
    }
    track drums "Drums" instrument drums level 0db pan 0 {
      processor echo delay {
        setting time 500ms
        setting feedback 0.0
        setting mix 0.5
      }
      output master
      clip c pattern hit_start start 0beat duration 1beat mode once
    }
    master level 0db {
      processor ceiling limiter {
        setting ceiling -12db
        setting release 0.05s
        setting lookahead 0.005s
      }
    }
  }
}
"#;
        fs::write(directory.path().join("project.crump"), source).expect("write source");
        let bundle = load_project(directory.path()).expect("load limiter wrap Project");
        assert!(!bundle.diagnostics.has_errors(), "{:?}", bundle.diagnostics);
        let output = directory.path().join("wrap_limiter");
        let rendered = render_project(&bundle, &RenderOptions::new(&output)).expect("wrap render");
        assert_eq!(rendered.manifest.tail_policy, RenderTailPolicy::Wrap);
        let pre_wrap = rendered
            .manifest
            .pre_wrap_peak
            .expect("wrap render records pre_wrap_peak");
        assert!(
            rendered.manifest.analysis.sample_peak > pre_wrap,
            "expected wrapped peak {} to exceed pre-wrap peak {pre_wrap}",
            rendered.manifest.analysis.sample_peak
        );
        assert!(rendered.manifest.wrap_peak_exceeded);
    }
}
