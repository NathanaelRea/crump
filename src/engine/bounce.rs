//! Exact-range source-backed bounce preparation.
//!
//! This module renders one immutable Project Revision into staged bytes only. Publication remains a
//! checked source-and-media History transaction owned by the edit layer.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    media::MAX_MEDIA_ASSET_BYTES,
    model::{ChannelRef, GeneratedAssetProvenance, TapPoint},
    project::{ProjectBundle, RevisionId},
};

use super::{
    CompiledProject, EngineError, ProcessingMode,
    generated_audio::{
        GENERATED_AUDIO_FORMAT, GeneratedAudioArtifact, build_generated_audio_artifact,
        valid_source_id,
    },
};

/// Sealed staged artifact returned by [`bounce_revision`].
pub type BounceArtifact = GeneratedAudioArtifact;

/// Versioned deterministic bounce algorithm identity retained in generated-asset provenance.
pub const BOUNCE_ALGORITHM: &str = "crump-bounce-stereo-f32-v1";
/// Exact portable format currently admitted for source-backed bounce.
pub const BOUNCE_FORMAT: &str = GENERATED_AUDIO_FORMAT;

/// Exact source rendered into a generated asset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum BounceSource {
    /// Final Master output after its ordered processor chain and level.
    Master,
    /// One stable channel at its pre- or post-fader tap.
    Channel {
        /// Stable Track, Group, or Return identity.
        owner: ChannelRef,
        /// Exact capture point.
        tap: TapPoint,
    },
}

/// Explicit isolated-tail policy after the source range.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "policy", content = "frames", rename_all = "snake_case")]
pub enum BounceTailPolicy {
    /// End the generated asset exactly at the range boundary.
    None,
    /// Stop future source events and append this many processor/release-tail frames.
    Frames(u64),
}

impl BounceTailPolicy {
    const fn frames(self) -> u64 {
        match self {
            Self::None => 0,
            Self::Frames(frames) => frames,
        }
    }
}

/// Explicit output format policy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BounceFormatPolicy {
    /// Portable stereo IEEE 32-bit float WAV at the Project sample rate.
    StereoFloat32Wav,
}

/// One exact source-backed bounce request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BounceRequest {
    /// Exact Active Revision expected by the caller.
    pub active_revision: RevisionId,
    /// Master or one stable channel tap.
    pub source: BounceSource,
    /// Inclusive absolute Song frame after deterministic preroll.
    pub start_frame: u64,
    /// Exclusive absolute Song frame.
    pub end_frame: u64,
    /// Explicit post-range behavior.
    pub tail: BounceTailPolicy,
    /// Exact portable encoding policy.
    pub format: BounceFormatPolicy,
    /// New durable asset identity and destination filename stem.
    pub asset_id: String,
    /// Human-facing generated asset name.
    pub asset_name: String,
}

/// Renders an exact immutable Revision to staged bytes without changing source or media.
pub fn bounce_revision(
    bundle: &ProjectBundle,
    request: &BounceRequest,
) -> Result<BounceArtifact, EngineError> {
    if bundle.revision != request.active_revision {
        return Err(EngineError::Compile(format!(
            "bounce expected Active Revision {}, but received {}",
            request.active_revision, bundle.revision
        )));
    }
    match request.format {
        BounceFormatPolicy::StereoFloat32Wav => {}
    }
    if !valid_source_id(&request.asset_id) {
        return Err(EngineError::Compile(
            "bounce asset ID must start with an ASCII letter and contain only letters, digits, `_`, or `-`"
                .to_owned(),
        ));
    }
    if request.asset_name.trim().is_empty()
        || request
            .asset_name
            .chars()
            .any(|character| character.is_control())
    {
        return Err(EngineError::Compile(
            "bounce asset name must be non-empty and contain no control characters".to_owned(),
        ));
    }
    let project = bundle
        .publishable()
        .ok_or_else(|| EngineError::Validation(bundle.diagnostics.as_slice().to_vec()))?;
    let path = PathBuf::from(format!("media/{}.wav", request.asset_id));
    if project
        .assets
        .iter()
        .any(|asset| asset.id == request.asset_id)
    {
        return Err(EngineError::Compile(format!(
            "media asset ID `{}` already exists",
            request.asset_id
        )));
    }
    if project.assets.iter().any(|asset| asset.path == path) {
        return Err(EngineError::Compile(format!(
            "media destination `{}` already exists",
            path.display()
        )));
    }
    let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)?;
    if request.start_frame >= request.end_frame || request.end_frame > compiled.arrangement_frames()
    {
        return Err(EngineError::Compile(format!(
            "bounce range [{}, {}) must be non-empty and lie within arrangement [0, {})",
            request.start_frame,
            request.end_frame,
            compiled.arrangement_frames()
        )));
    }
    let tail_frames = request.tail.frames();
    let frame_count = request
        .end_frame
        .saturating_sub(request.start_frame)
        .checked_add(tail_frames)
        .ok_or_else(|| EngineError::Compile("bounce frame count overflowed".to_owned()))?;
    let byte_len = frame_count
        .checked_mul(8)
        .and_then(|bytes| bytes.checked_add(44))
        .ok_or_else(|| EngineError::Compile("bounce byte count overflowed".to_owned()))?;
    if byte_len > MAX_MEDIA_ASSET_BYTES || byte_len > u64::from(u32::MAX) {
        return Err(EngineError::Compile(format!(
            "bounce would produce {byte_len} bytes; maximum is {MAX_MEDIA_ASSET_BYTES}"
        )));
    }

    let channel_index = match &request.source {
        BounceSource::Master => None,
        BounceSource::Channel { owner, .. } => Some(
            compiled
                .mix_graph_inspection()
                .channels
                .iter()
                .find(|channel| channel.identity == owner.to_string())
                .map(|channel| channel.index)
                .ok_or_else(|| {
                    EngineError::Compile(format!("bounce source `{owner}` does not exist"))
                })?,
        ),
    };
    let mut runtime = compiled.runtime();
    runtime.advance_to(request.start_frame)?;
    let capacity = usize::try_from(frame_count).map_err(|_| {
        EngineError::Compile("bounce frame count does not fit this host".to_owned())
    })?;
    let mut frames = Vec::with_capacity(capacity);
    for _ in request.start_frame..request.end_frame {
        let master = runtime.process_frame(ProcessingMode::Offline)?;
        frames.push(capture_source(
            &runtime,
            master,
            &request.source,
            channel_index,
        )?);
    }
    if tail_frames > 0 {
        runtime.begin_isolated_tail(tail_frames);
        for _ in 0..tail_frames {
            let master = runtime.process_frame(ProcessingMode::Offline)?;
            frames.push(capture_source(
                &runtime,
                master,
                &request.source,
                channel_index,
            )?);
        }
    }
    debug_assert_eq!(frames.len() as u64, frame_count);
    let (owner, tap) = match &request.source {
        BounceSource::Master => ("master".to_owned(), "master_output".to_owned()),
        BounceSource::Channel { owner, tap } => (
            owner.to_string(),
            match tap {
                TapPoint::PreFader => "pre_fader",
                TapPoint::PostFader => "post_fader",
            }
            .to_owned(),
        ),
    };
    let provenance = GeneratedAssetProvenance::Bounce {
        source_revision: request.active_revision.to_string(),
        owner,
        tap,
        start_frame: request.start_frame,
        end_frame: request.end_frame,
        tail_frames,
        format: BOUNCE_FORMAT.to_owned(),
        algorithm: BOUNCE_ALGORITHM.to_owned(),
        build: env!("CARGO_PKG_VERSION").to_owned(),
    };
    build_generated_audio_artifact(
        &request.asset_id,
        &request.asset_name,
        &frames,
        project.sample_rate,
        provenance,
    )
}

fn capture_source(
    runtime: &super::SongRuntime,
    master: [f32; 2],
    source: &BounceSource,
    channel_index: Option<usize>,
) -> Result<[f32; 2], EngineError> {
    match source {
        BounceSource::Master => Ok(master),
        BounceSource::Channel { owner, tap } => runtime
            .channel_tap_sample(channel_index.expect("channel source has an index"), *tap)
            .ok_or_else(|| EngineError::Compile(format!("bounce source `{owner}` has no tap"))),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor};

    use tempfile::tempdir;

    use super::*;
    use crate::{
        history::{ChangeOrigin, HistoryStore, Publication},
        project::{PROJECT_FILE, load_project},
    };

    #[test]
    fn exact_revision_bounce_is_deterministic_and_declares_complete_provenance() {
        let root = tempdir().expect("Project root");
        fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/minimal.crump"),
            root.path().join(PROJECT_FILE),
        )
        .expect("Project source");
        let candidate = load_project(root.path()).expect("candidate");
        let store = HistoryStore::open(root.path()).expect("History");
        store
            .publish(
                &candidate,
                Publication::new(ChangeOrigin::human("test"), "Initial", "Bounce base"),
            )
            .expect("publish");
        let active = store
            .active_bundle()
            .expect("active bundle")
            .expect("Active");
        let project = active.publishable().expect("valid Active");
        let end_frame = project
            .song
            .end
            .to_frames(project.song.tempo_bpm, project.sample_rate)
            .expect("end")
            .min(2_048);
        let request = BounceRequest {
            active_revision: active.revision.clone(),
            source: BounceSource::Master,
            start_frame: 0,
            end_frame,
            tail: BounceTailPolicy::Frames(64),
            format: BounceFormatPolicy::StereoFloat32Wav,
            asset_id: "resampled".to_owned(),
            asset_name: "Resampled".to_owned(),
        };
        let first = bounce_revision(&active, &request).expect("bounce");
        let second = bounce_revision(&active, &request).expect("repeat bounce");
        assert_eq!(first.bytes(), second.bytes());
        assert_eq!(first.sha256(), second.sha256());
        assert_eq!(first.frame_count(), end_frame + 64);
        assert!(
            first
                .source_declaration()
                .contains(&active.revision.to_string())
        );
        assert!(first.source_declaration().contains(BOUNCE_ALGORITHM));
        assert!(matches!(
            first.provenance(),
            GeneratedAssetProvenance::Bounce {
                owner,
                tail_frames: 64,
                ..
            } if owner == "master"
        ));

        drop(store);
        let entry = crate::edit::publish_bounce_asset(
            root.path(),
            &active.revision,
            &first,
            Publication::new(
                ChangeOrigin::human("test"),
                "Publish bounce",
                "Atomic generated source and bytes",
            ),
        )
        .expect("atomic bounce publication");
        let reloaded = load_project(root.path()).expect("published bounce Project");
        assert_eq!(reloaded.revision, entry.revision);
        let asset = reloaded
            .publishable()
            .expect("valid published Project")
            .assets
            .iter()
            .find(|asset| asset.id == "resampled")
            .expect("generated asset declaration");
        assert_eq!(asset.sha256, first.sha256());
        assert_eq!(asset.generated.as_ref(), Some(first.provenance()));
        assert_eq!(
            fs::read(root.path().join(first.path())).expect("published bytes"),
            first.bytes()
        );
    }

    #[test]
    fn channel_taps_preroll_and_invalid_ranges_are_explicit() {
        let root = tempdir().expect("Project root");
        fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/minimal.crump"),
            root.path().join(PROJECT_FILE),
        )
        .expect("Project source");
        let candidate = load_project(root.path()).expect("candidate");
        let store = HistoryStore::open(root.path()).expect("History");
        store
            .publish(
                &candidate,
                Publication::new(ChangeOrigin::human("test"), "Initial", "Tap bounce base"),
            )
            .expect("publish");
        let active = store
            .active_bundle()
            .expect("active bundle")
            .expect("Active");
        let project = active.publishable().expect("valid Active");
        let start_frame = crate::time::parse_musical_time("1beat", project.song.meter)
            .expect("one beat")
            .to_frames(project.song.tempo_bpm, project.sample_rate)
            .expect("one-beat frame");
        let end_frame = start_frame * 2;
        let channel = "track/lead".parse::<ChannelRef>().expect("channel");
        let request = |tap, id: &str| BounceRequest {
            active_revision: active.revision.clone(),
            source: BounceSource::Channel {
                owner: channel.clone(),
                tap,
            },
            start_frame,
            end_frame,
            tail: BounceTailPolicy::None,
            format: BounceFormatPolicy::StereoFloat32Wav,
            asset_id: id.to_owned(),
            asset_name: id.to_owned(),
        };
        let pre = bounce_revision(&active, &request(TapPoint::PreFader, "lead-pre"))
            .expect("pre-fader bounce");
        let post = bounce_revision(&active, &request(TapPoint::PostFader, "lead-post"))
            .expect("post-fader bounce");
        assert_eq!(pre.frame_count(), end_frame - start_frame);
        assert_eq!(post.frame_count(), end_frame - start_frame);
        assert_ne!(pre.bytes(), post.bytes());
        assert!(matches!(
            pre.provenance(),
            GeneratedAssetProvenance::Bounce { owner, tap, .. }
                if owner == "track/lead" && tap == "pre_fader"
        ));
        assert!(matches!(
            post.provenance(),
            GeneratedAssetProvenance::Bounce { tap, .. } if tap == "post_fader"
        ));
        assert!(
            hound::WavReader::new(Cursor::new(pre.bytes()))
                .expect("pre WAV")
                .samples::<f32>()
                .any(|sample| sample.expect("sample").abs() > 1.0e-6)
        );

        let mut invalid = request(TapPoint::PreFader, "invalid");
        invalid.end_frame = invalid.start_frame;
        assert!(bounce_revision(&active, &invalid).is_err());
        invalid.end_frame = u64::MAX;
        assert!(bounce_revision(&active, &invalid).is_err());
        invalid.end_frame = end_frame;
        invalid.source = BounceSource::Channel {
            owner: "track/missing"
                .parse()
                .expect("source-safe missing channel"),
            tap: TapPoint::PreFader,
        };
        assert!(bounce_revision(&active, &invalid).is_err());
    }
}
