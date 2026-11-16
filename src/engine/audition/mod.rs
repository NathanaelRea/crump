//! Unified non-authoritative audition preparation and inspection.
//!
//! This module owns target filtering, exact source-time range resolution, deterministic
//! pre-roll, monitoring-path selection, and the inspection contract shared by CLI and
//! workspace callers. It never mutates authoritative Project source.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    media::AssetInventory,
    model::{Clip, ClipMode, ClipSites, PatternEvent, Project, SampleMapping, TapPoint, Track},
    project::RevisionId,
    time::{MusicalTime, parse_musical_time},
};

/// Position of `frame` inside a resolved audition range, used to carry transport across publication.
///
/// A looping range wraps the frame to its loop phase; a one-shot range clamps it into
/// `[start, end)`. Both the controller's preparation and the callback's crossfade use
/// this rule so a moved range keeps the listener at the same relative point.
#[must_use]
pub(crate) fn range_offset(frame: u64, start: u64, end: u64, looping: bool) -> u64 {
    if looping {
        let length = end.saturating_sub(start).max(1);
        frame.saturating_sub(start) % length
    } else {
        frame
            .clamp(start, end.saturating_sub(1))
            .saturating_sub(start)
    }
}

use super::{
    CompiledProject, EngineError, ProcessorMeter, RuntimeTelemetry, SongRuntime,
    control::CompiledControlProgram,
    mix_graph::{CompiledProcessorInspection, MixGraphInspection},
    sample::{CompiledSampleLibrary, CompiledSampleSource},
};

/// Stable source-backed material selected for audition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[derive(Default)]
pub enum AuditionTarget {
    /// The complete authored Song.
    #[default]
    Song,
    /// One timeline Track with every authored Clip on that Track.
    Track {
        /// Durable Track ID.
        track: String,
    },
    /// One arranged Clip, found globally or constrained to one Track.
    Clip {
        /// Durable Clip ID.
        clip: String,
        /// Optional owning Track disambiguation.
        #[serde(skip_serializing_if = "Option::is_none")]
        track: Option<String>,
    },
    /// One reusable Pattern on its arranged Track(s), or on an explicitly selected Track.
    Pattern {
        /// Durable Pattern ID.
        pattern: String,
        /// Explicit consuming Track for unarranged or channel-path audition.
        #[serde(skip_serializing_if = "Option::is_none")]
        track: Option<String>,
    },
    /// One exact Project media asset region in original source frames.
    Asset {
        /// Durable media asset ID.
        asset: String,
        /// Optional inclusive original-source-frame start.
        #[serde(skip_serializing_if = "Option::is_none")]
        start_frame: Option<u64>,
        /// Optional exclusive original-source-frame end.
        #[serde(skip_serializing_if = "Option::is_none")]
        end_frame: Option<u64>,
    },
    /// One sampler mapping on a compatible Track.
    Mapping {
        /// Owning sampler Instrument ID.
        instrument: String,
        /// Instrument-local mapping ID.
        mapping: String,
        /// Explicit Track when multiple Tracks use the Instrument.
        #[serde(skip_serializing_if = "Option::is_none")]
        track: Option<String>,
    },
    /// One durably identified sample occurrence in a Pattern.
    Occurrence {
        /// Owning Pattern ID.
        pattern: String,
        /// Pattern-local durable sample occurrence ID.
        occurrence: String,
        /// Explicit consuming Track when needed.
        #[serde(skip_serializing_if = "Option::is_none")]
        track: Option<String>,
    },
}

/// Explicit monitoring point for the selected material.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[derive(Default)]
pub enum AuditionPath {
    /// Decoded, project-rate-converted sample frames with no sampler, channel, or Master DSP.
    Raw,
    /// Sampler output plus the Instrument post-sum chain, before Track inserts.
    Mapping,
    /// Selected Track channel before or after its fader/panner.
    Channel {
        /// Pre- or post-fader Track channel tap.
        tap: TapPoint,
    },
    /// Complete authored routing through Master.
    #[default]
    Master,
}

/// Exact half-open source range and loop policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditionRange {
    /// Locator ID or exact musical-time literal. Defaults to the target's natural start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Locator ID or exact musical-time literal. Defaults to the target's natural end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Rewind the selected half-open range until explicitly stopped.
    pub looping: bool,
}

/// One non-authoritative cue tap retained with the audition request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditionCue {
    /// Stable compiled channel identity such as `track/drums` or `return/verb`.
    pub channel: String,
    /// Pre- or post-fader tap selected for cue monitoring.
    pub tap: TapPoint,
}

/// One reusable audition request. Transient overrides are deliberately not source state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditionPlan {
    /// Selected authored material.
    pub target: AuditionTarget,
    /// Explicit monitoring path.
    pub path: AuditionPath,
    /// Exact seek/range/loop policy.
    pub range: AuditionRange,
    /// Monitoring-only bypass overrides by song-wide processor ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub transient_bypass: BTreeMap<String, bool>,
    /// Monitoring-only Solo selections by compiled channel identity.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub soloed_channels: BTreeSet<String>,
    /// Optional monitoring-only cue selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cue: Option<AuditionCue>,
}

/// Controller-thread preparation options.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuditionPrepareOptions {
    /// Resume at this already-resolved absolute Project frame, or at the range start.
    pub initial_frame: Option<u64>,
    /// Resume this many frames after the newly resolved range start.
    ///
    /// This is mutually exclusive with `initial_frame` and lets compatible publication
    /// retain transport position when a Locator or target boundary moves.
    pub initial_range_offset: Option<u64>,
    /// Preserve the prepared pre-roll state through realtime publication as an explicit seek.
    pub explicit_seek: bool,
}

/// Resolved state shown by CLI and workspace inspection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditionStatus {
    /// Exact Project revision from which the runtime was compiled.
    pub active_revision: RevisionId,
    /// Requested stable target.
    pub target: AuditionTarget,
    /// Effective monitoring path.
    pub path: AuditionPath,
    /// Tracks retained by target filtering, in presentation order.
    pub tracks: Vec<String>,
    /// Inclusive half-open range start.
    pub start_frame: u64,
    /// Exclusive half-open range end.
    pub end_frame: u64,
    /// First frame that will be heard after preparation.
    pub transport_frame: u64,
    /// Whether the range rewinds at its exclusive end.
    pub looping: bool,
    /// Monitoring-only Solo selections active in the prepared runtime.
    pub soloed_channels: BTreeSet<String>,
    /// Monitoring-only cue selection active in the prepared runtime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cue: Option<AuditionCue>,
    /// Frames deterministically evaluated from Project time zero before playback.
    pub preroll_frames: u64,
    /// Project sample rate used by all frame fields.
    pub sample_rate: u32,
}

/// Runtime processor activity joined to source-backed processor facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditionProcessorInspection {
    /// Static compiled processor contract.
    #[serde(flatten)]
    pub processor: CompiledProcessorInspection,
    /// Monitoring-only override active in this plan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transient_bypass: Option<bool>,
    /// Effective monitoring target after authored and transient state are combined.
    pub effective_bypass: bool,
    /// Current callback-cheap processor activity.
    pub activity: ProcessorMeter,
}

/// Serializable unified audition inspection payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditionInspection {
    /// Target, path, revision, range, loop, seek, and pre-roll state.
    pub status: AuditionStatus,
    /// Ordered compiled processors with bypass and activity state.
    pub processors: Vec<AuditionProcessorInspection>,
}

/// Fully compiled and pre-rolled audition runtime.
pub struct PreparedAudition {
    runtime: SongRuntime,
    status: AuditionStatus,
    mix_graph: MixGraphInspection,
    control_program: CompiledControlProgram,
    transient_bypass: BTreeMap<String, bool>,
}

impl PreparedAudition {
    /// Runtime ready for PipeWire or direct monitoring.
    #[must_use]
    pub fn into_runtime(self) -> SongRuntime {
        self.runtime
    }

    /// Resolved source/revision/transport state.
    #[must_use]
    pub const fn status(&self) -> &AuditionStatus {
        &self.status
    }

    /// Static processing graph aligned with runtime processor telemetry indexes.
    #[must_use]
    pub const fn mix_graph_inspection(&self) -> &MixGraphInspection {
        &self.mix_graph
    }

    /// Compiled control inspection aligned with this exact prepared revision.
    #[must_use]
    pub const fn control_program(&self) -> &CompiledControlProgram {
        &self.control_program
    }

    /// Unified inspection at the prepared frame.
    #[must_use]
    pub fn inspection(&self) -> AuditionInspection {
        inspection_from_parts(
            self.status.clone(),
            &self.mix_graph,
            self.runtime.telemetry(),
            &self.transient_bypass,
        )
    }
}

/// Joins a retained audition status and graph with fresh realtime telemetry.
#[must_use]
pub fn inspection_from_parts(
    mut status: AuditionStatus,
    mix_graph: &MixGraphInspection,
    telemetry: RuntimeTelemetry,
    transient_bypass: &BTreeMap<String, bool>,
) -> AuditionInspection {
    status.transport_frame = telemetry.frame;
    let processors = mix_graph
        .processors
        .iter()
        .cloned()
        .map(|processor| {
            let activity_available = processor.index < telemetry.processor_count;
            let mut activity = telemetry
                .processor_activity
                .get(processor.index)
                .copied()
                .unwrap_or_default();
            let transient = if activity_available {
                activity.transient_bypass
            } else {
                transient_bypass.get(&processor.id).copied()
            };
            let effective = if activity_available {
                activity.effective_bypass
            } else {
                transient.unwrap_or(processor.authored_bypass)
            };
            if !activity_available {
                activity.authored_bypass = processor.authored_bypass;
                activity.transient_bypass = transient;
                activity.effective_bypass = effective;
            }
            AuditionProcessorInspection {
                processor,
                transient_bypass: transient,
                effective_bypass: effective,
                activity,
            }
        })
        .collect();
    AuditionInspection { status, processors }
}

impl AuditionPlan {
    /// Compiles and pre-rolls from one exact Active Project revision.
    pub fn prepare(
        &self,
        revision: &RevisionId,
        project: &Project,
        inventory: &AssetInventory,
    ) -> Result<PreparedAudition, EngineError> {
        self.prepare_with_options(
            revision,
            project,
            inventory,
            AuditionPrepareOptions::default(),
        )
    }

    /// Compiles and pre-rolls with explicit resume/publication behavior.
    pub fn prepare_with_options(
        &self,
        revision: &RevisionId,
        project: &Project,
        inventory: &AssetInventory,
        options: AuditionPrepareOptions,
    ) -> Result<PreparedAudition, EngineError> {
        let selection = resolve_target(project, &self.target)?;
        if matches!(self.target, AuditionTarget::Asset { .. }) && self.path != AuditionPath::Raw {
            return Err(compile_error(
                "an Asset target supports only the raw audition path; select a Mapping or Occurrence for routed playback"
                    .to_owned(),
            ));
        }
        let compiled = CompiledProject::compile_with_assets(&selection.project, inventory)?;
        let default_start = selection
            .default_start
            .to_frames(project.song.tempo_bpm, project.sample_rate)?;
        let mut default_end = selection
            .default_end
            .to_frames(project.song.tempo_bpm, project.sample_rate)?;
        let mut maximum_end = project
            .song
            .end
            .to_frames(project.song.tempo_bpm, project.sample_rate)?;
        let prepared_raw = if self.path == AuditionPath::Raw {
            let raw = selection.raw_source.as_ref().ok_or_else(|| {
                compile_error(
                    "raw path requires an Asset, Mapping, or sample Occurrence target".to_owned(),
                )
            })?;
            let library = CompiledSampleLibrary::compile(inventory, project.sample_rate)
                .map_err(|error| EngineError::Compile(error.to_string()))?;
            let source = library.get(&raw.asset).cloned().ok_or_else(|| {
                compile_error(format!(
                    "raw audition references unavailable asset `{}`",
                    raw.asset
                ))
            })?;
            let (source_start, source_end) = compiled_region(&source, raw.start, raw.end)?;
            let source_frames = u64::try_from(source_end - source_start)
                .map_err(|_| compile_error("raw audition source region is too large".to_owned()))?;
            let natural_end = default_start.checked_add(source_frames).ok_or_else(|| {
                compile_error("raw audition range exceeds the frame domain".to_owned())
            })?;
            default_end = natural_end;
            maximum_end = natural_end;
            Some((source, source_start, source_end))
        } else {
            None
        };
        let (start, end) = resolve_range(
            project,
            &self.range,
            default_start,
            default_end,
            maximum_end,
        )?;
        if options.initial_frame.is_some() && options.initial_range_offset.is_some() {
            return Err(compile_error(
                "audition preparation accepts either an absolute frame or a range offset, not both"
                    .to_owned(),
            ));
        }
        let initial = if let Some(offset) = options.initial_range_offset {
            start.saturating_add(offset.min(end.saturating_sub(start).saturating_sub(1)))
        } else {
            options.initial_frame.unwrap_or(start)
        };
        if initial < start || initial >= end {
            return Err(EngineError::Compile(format!(
                "audition transport frame {initial} lies outside [{start}, {end})"
            )));
        }

        let mut runtime = compiled.runtime();
        if let Some((source, source_start, source_end)) = prepared_raw {
            let range_offset =
                usize::try_from(start.saturating_sub(default_start)).unwrap_or(usize::MAX);
            let cursor = source_start.saturating_add(range_offset).min(source_end);
            runtime.set_raw_audition(source, cursor, source_end, cursor, end)?;
        }
        runtime.advance_to(start)?;
        runtime.set_audition_range(start, end)?;
        for (processor, bypass) in &self.transient_bypass {
            if runtime.set_processor_bypass(processor, Some(*bypass)) == 0 {
                return Err(EngineError::Compile(format!(
                    "audition bypass references missing processor `{processor}`"
                )));
            }
        }
        for channel in &self.soloed_channels {
            if !runtime.set_channel_solo(channel, true) {
                return Err(EngineError::Compile(format!(
                    "audition Solo references missing channel `{channel}`"
                )));
            }
        }
        if let Some(cue) = &self.cue
            && !runtime.set_channel_cue(Some(&cue.channel), cue.tap)
        {
            return Err(EngineError::Compile(format!(
                "audition cue references missing channel `{}`",
                cue.channel
            )));
        }

        let primary_track = selection.primary_track.as_deref();
        if let Some((track, mapping)) = &selection.mapping_trigger {
            let track_index = compiled_track_index(&compiled, track).ok_or_else(|| {
                EngineError::Compile(format!("audition Track `{track}` was not compiled"))
            })?;
            if !runtime.audition_mapping_on_track(track_index, mapping, 1.0) {
                return Err(EngineError::Compile(format!(
                    "audition mapping `{mapping}` is unavailable on Track `{track}`"
                )));
            }
        }

        match self.path {
            AuditionPath::Master => runtime.set_audition_master_output(),
            AuditionPath::Channel { tap } => {
                let track = require_primary_track(primary_track, "channel")?;
                let identity = format!("track/{track}");
                if !runtime.set_audition_channel_output(&identity, tap) {
                    return Err(EngineError::Compile(format!(
                        "audition channel `{identity}` is unavailable"
                    )));
                }
            }
            AuditionPath::Mapping => {
                let track = require_primary_track(primary_track, "mapping")?;
                if selection.mapping_source.is_none() {
                    return Err(EngineError::Compile(
                        "mapping path requires a Mapping or sample Occurrence target".to_owned(),
                    ));
                }
                let identity = format!("track/{track}");
                if !runtime.set_audition_source_output(&identity) {
                    return Err(EngineError::Compile(format!(
                        "audition mapping channel `{identity}` is unavailable"
                    )));
                }
            }
            AuditionPath::Raw => {}
        }

        if initial > start {
            runtime.advance_audition_to(initial)?;
        }
        if self.range.looping {
            runtime.enable_audition_loop(start, end)?;
        }
        if options.explicit_seek {
            runtime.request_audition_seek();
        }

        let status = AuditionStatus {
            active_revision: revision.clone(),
            target: self.target.clone(),
            path: self.path,
            tracks: selection.tracks,
            start_frame: start,
            end_frame: end,
            transport_frame: initial,
            looping: self.range.looping,
            soloed_channels: self.soloed_channels.clone(),
            cue: self.cue.clone(),
            preroll_frames: initial,
            sample_rate: project.sample_rate,
        };
        Ok(PreparedAudition {
            runtime,
            status,
            mix_graph: compiled.mix_graph_inspection().clone(),
            control_program: compiled.control_program().clone(),
            transient_bypass: self.transient_bypass.clone(),
        })
    }
}

#[derive(Debug, Clone)]
struct RawSourceSelection {
    asset: String,
    start: u64,
    end: u64,
}

struct ResolvedSelection {
    project: Project,
    tracks: Vec<String>,
    primary_track: Option<String>,
    mapping_trigger: Option<(String, String)>,
    mapping_source: Option<RawSourceSelection>,
    raw_source: Option<RawSourceSelection>,
    default_start: MusicalTime,
    default_end: MusicalTime,
}

fn resolve_target(
    project: &Project,
    target: &AuditionTarget,
) -> Result<ResolvedSelection, EngineError> {
    let mut filtered = project.clone();
    let song_end = project.song.end;
    let mut tracks = Vec::new();
    let mut primary_track = None;
    let mut mapping_trigger = None;
    let mut mapping_source = None;
    let mut raw_source = None;
    let mut default_start = MusicalTime::ZERO;
    let mut default_end = song_end;

    match target {
        AuditionTarget::Song => {
            tracks.extend(project.song.tracks.iter().map(|track| track.id.clone()));
        }
        AuditionTarget::Track { track } => {
            require_track(project, track)?;
            retain_clips(&mut filtered, |candidate, _| candidate.id == *track);
            tracks.push(track.clone());
            primary_track = Some(track.clone());
        }
        AuditionTarget::Clip { clip, track } => {
            let (owner, selected) = find_clip(project, clip, track.as_deref())?;
            let owner_id = owner.id.clone();
            let clip_id = selected.id.clone();
            default_start = selected.start;
            default_end = selected
                .start
                .checked_add(selected.duration)
                .map_err(EngineError::Time)?;
            retain_clips(&mut filtered, |candidate, candidate_clip| {
                candidate.id == owner_id && candidate_clip.id == clip_id
            });
            tracks.push(owner.id.clone());
            primary_track = Some(owner.id.clone());
        }
        AuditionTarget::Pattern { pattern, track } => {
            let pattern_model = project
                .song
                .patterns
                .iter()
                .find(|candidate| candidate.id == *pattern)
                .ok_or_else(|| compile_error(format!("unknown audition Pattern `{pattern}`")))?;
            let selected_tracks = pattern_tracks(project, pattern, track.as_deref())?;
            let selected_ids = selected_tracks
                .iter()
                .map(|track| track.id.as_str())
                .collect::<BTreeSet<_>>();
            retain_clips(&mut filtered, |candidate, clip| {
                selected_ids.contains(candidate.id.as_str()) && clip.pattern == *pattern
            });
            if selected_tracks
                .iter()
                .all(|track| !track.clips.iter().any(|clip| clip.pattern == *pattern))
            {
                let track_id = selected_tracks[0].id.clone();
                let duration = pattern_model.span.min(song_end);
                inject_pattern_clip(&mut filtered, &track_id, pattern_model, duration)?;
                default_end = duration;
            } else if let Some((start, end)) = clip_bounds(&selected_tracks, pattern)? {
                default_start = start;
                default_end = end;
            }
            tracks.extend(selected_tracks.iter().map(|track| track.id.clone()));
            primary_track = (selected_tracks.len() == 1).then(|| selected_tracks[0].id.clone());
        }
        AuditionTarget::Asset {
            asset,
            start_frame,
            end_frame,
        } => {
            let declared = project
                .assets
                .iter()
                .find(|candidate| candidate.id == *asset)
                .ok_or_else(|| compile_error(format!("unknown audition Asset `{asset}`")))?;
            let inventory_asset = project
                .assets
                .iter()
                .find(|candidate| candidate.id == declared.id)
                .expect("declared asset");
            let _ = inventory_asset;
            retain_clips(&mut filtered, |_, _| false);
            raw_source = Some(RawSourceSelection {
                asset: asset.clone(),
                start: start_frame.unwrap_or(0),
                end: end_frame.unwrap_or(u64::MAX),
            });
        }
        AuditionTarget::Mapping {
            instrument,
            mapping,
            track,
        } => {
            let (mapping_model, selected_track) =
                resolve_mapping_track(project, instrument, mapping, track.as_deref())?;
            retain_clips(&mut filtered, |_, _| false);
            tracks.push(selected_track.id.clone());
            primary_track = Some(selected_track.id.clone());
            mapping_trigger = Some((selected_track.id.clone(), mapping.clone()));
            let source = mapping_source_selection(mapping_model);
            mapping_source = Some(source.clone());
            raw_source = Some(source);
        }
        AuditionTarget::Occurrence {
            pattern,
            occurrence,
            track,
        } => {
            let pattern_model = project
                .song
                .patterns
                .iter()
                .find(|candidate| candidate.id == *pattern)
                .ok_or_else(|| compile_error(format!("unknown audition Pattern `{pattern}`")))?;
            let event = pattern_model
                .events
                .iter()
                .find(|event| matches!(event, PatternEvent::Sample { id, .. } if id == occurrence))
                .ok_or_else(|| {
                    compile_error(format!(
                        "unknown sample occurrence `{occurrence}` in Pattern `{pattern}`"
                    ))
                })?;
            let PatternEvent::Sample {
                mapping,
                start_frame,
                end_frame,
                ..
            } = event
            else {
                unreachable!("matched sample occurrence")
            };
            let selected_tracks = occurrence_tracks(project, pattern, mapping, track.as_deref())?;
            let selected_ids = selected_tracks
                .iter()
                .map(|track| track.id.as_str())
                .collect::<BTreeSet<_>>();
            retain_clips(&mut filtered, |candidate, clip| {
                selected_ids.contains(candidate.id.as_str()) && clip.pattern == *pattern
            });
            if selected_tracks
                .iter()
                .all(|track| !track.clips.iter().any(|clip| clip.pattern == *pattern))
            {
                let duration = pattern_model.span.min(song_end);
                inject_pattern_clip(
                    &mut filtered,
                    &selected_tracks[0].id,
                    pattern_model,
                    duration,
                )?;
                default_end = duration;
            } else if let Some((start, end)) = clip_bounds(&selected_tracks, pattern)? {
                default_start = start;
                default_end = end;
            }
            let filtered_pattern = filtered
                .song
                .patterns
                .iter_mut()
                .find(|candidate| candidate.id == *pattern)
                .expect("selected Pattern clone");
            filtered_pattern
                .events
                .retain(|candidate| matches!(candidate, PatternEvent::Sample { id, .. } if id == occurrence));
            let selected_track = &selected_tracks[0];
            let instrument = project
                .song
                .instruments
                .iter()
                .find(|instrument| instrument.id == selected_track.instrument)
                .expect("validated Track Instrument");
            let mapping_model = instrument
                .sampler
                .as_ref()
                .and_then(|sampler| {
                    sampler
                        .mappings
                        .iter()
                        .find(|candidate| candidate.id == *mapping)
                })
                .expect("occurrence mapping checked by track selection");
            let source = RawSourceSelection {
                asset: mapping_model.asset.clone(),
                start: *start_frame,
                end: *end_frame,
            };
            mapping_source = Some(source.clone());
            raw_source = Some(source);
            tracks.extend(selected_tracks.iter().map(|track| track.id.clone()));
            primary_track = (selected_tracks.len() == 1).then(|| selected_tracks[0].id.clone());
        }
    }

    Ok(ResolvedSelection {
        project: filtered,
        tracks,
        primary_track,
        mapping_trigger,
        mapping_source,
        raw_source,
        default_start,
        default_end,
    })
}

fn retain_clips(project: &mut Project, mut keep: impl FnMut(&Track, &Clip) -> bool) {
    for track in &mut project.song.tracks {
        let owner = track.clone();
        track.clips.retain(|clip| keep(&owner, clip));
    }
}

fn require_track<'a>(project: &'a Project, track: &str) -> Result<&'a Track, EngineError> {
    project
        .song
        .tracks
        .iter()
        .find(|candidate| candidate.id == track)
        .ok_or_else(|| compile_error(format!("unknown audition Track `{track}`")))
}

fn find_clip<'a>(
    project: &'a Project,
    clip: &str,
    track: Option<&str>,
) -> Result<(&'a Track, &'a Clip), EngineError> {
    let matches = project
        .song
        .tracks
        .iter()
        .filter(|candidate| track.is_none_or(|track| candidate.id == track))
        .flat_map(|track| {
            track
                .clips
                .iter()
                .filter(move |candidate| candidate.id == clip)
                .map(move |clip| (track, clip))
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [selected] => Ok(*selected),
        [] => Err(compile_error(format!("unknown audition Clip `{clip}`"))),
        _ => Err(compile_error(format!(
            "audition Clip `{clip}` is ambiguous; select its Track"
        ))),
    }
}

fn pattern_tracks<'a>(
    project: &'a Project,
    pattern: &str,
    explicit: Option<&str>,
) -> Result<Vec<&'a Track>, EngineError> {
    if let Some(track) = explicit {
        return Ok(vec![require_track(project, track)?]);
    }
    let arranged = project
        .song
        .tracks
        .iter()
        .filter(|track| track.clips.iter().any(|clip| clip.pattern == pattern))
        .collect::<Vec<_>>();
    if !arranged.is_empty() {
        return Ok(arranged);
    }
    match project.song.tracks.as_slice() {
        [track] => Ok(vec![track]),
        [] => Err(compile_error(
            "Pattern audition requires at least one Track".to_owned(),
        )),
        _ => Err(compile_error(format!(
            "unarranged Pattern `{pattern}` requires an explicit Track"
        ))),
    }
}

fn occurrence_tracks<'a>(
    project: &'a Project,
    pattern: &str,
    mapping: &str,
    explicit: Option<&str>,
) -> Result<Vec<&'a Track>, EngineError> {
    let compatible = |track: &&Track| {
        project
            .song
            .instruments
            .iter()
            .find(|instrument| instrument.id == track.instrument)
            .and_then(|instrument| instrument.sampler.as_ref())
            .is_some_and(|sampler| {
                sampler
                    .mappings
                    .iter()
                    .any(|candidate| candidate.id == mapping)
            })
    };
    if let Some(track) = explicit {
        let selected = require_track(project, track)?;
        if !compatible(&selected) {
            return Err(compile_error(format!(
                "Track `{track}` does not provide sample mapping `{mapping}`"
            )));
        }
        return Ok(vec![selected]);
    }
    let arranged = project
        .song
        .tracks
        .iter()
        .filter(|track| track.clips.iter().any(|clip| clip.pattern == pattern))
        .filter(compatible)
        .collect::<Vec<_>>();
    if !arranged.is_empty() {
        return Ok(arranged);
    }
    let compatible_tracks = project
        .song
        .tracks
        .iter()
        .filter(compatible)
        .collect::<Vec<_>>();
    match compatible_tracks.as_slice() {
        [track] => Ok(vec![*track]),
        [] => Err(compile_error(format!(
            "no Track provides sample mapping `{mapping}`"
        ))),
        _ => Err(compile_error(format!(
            "unarranged sample occurrence requires an explicit Track for mapping `{mapping}`"
        ))),
    }
}

fn resolve_mapping_track<'a>(
    project: &'a Project,
    instrument_id: &str,
    mapping_id: &str,
    explicit_track: Option<&str>,
) -> Result<(&'a SampleMapping, &'a Track), EngineError> {
    let instrument = project
        .song
        .instruments
        .iter()
        .find(|candidate| candidate.id == instrument_id)
        .ok_or_else(|| compile_error(format!("unknown sampler Instrument `{instrument_id}`")))?;
    let mapping = instrument
        .sampler
        .as_ref()
        .and_then(|sampler| {
            sampler
                .mappings
                .iter()
                .find(|candidate| candidate.id == mapping_id)
        })
        .ok_or_else(|| {
            compile_error(format!(
                "unknown sample mapping `{mapping_id}` on Instrument `{instrument_id}`"
            ))
        })?;
    let tracks = project
        .song
        .tracks
        .iter()
        .filter(|track| track.instrument == instrument_id)
        .filter(|track| explicit_track.is_none_or(|selected| track.id == selected))
        .collect::<Vec<_>>();
    match tracks.as_slice() {
        [track] => Ok((mapping, *track)),
        [] => Err(compile_error(format!(
            "no selected Track uses sampler Instrument `{instrument_id}`"
        ))),
        _ => Err(compile_error(format!(
            "mapping `{instrument_id}/{mapping_id}` is used by multiple Tracks; select one"
        ))),
    }
}

fn mapping_source_selection(mapping: &SampleMapping) -> RawSourceSelection {
    RawSourceSelection {
        asset: mapping.asset.clone(),
        start: mapping.start_frame,
        end: mapping.end_frame,
    }
}

fn inject_pattern_clip(
    project: &mut Project,
    track_id: &str,
    pattern: &crate::model::Pattern,
    duration: MusicalTime,
) -> Result<(), EngineError> {
    let site = pattern.span_site.clone();
    let clip = Clip {
        id: "audition-pattern".to_owned(),
        pattern: pattern.id.clone(),
        start: MusicalTime::ZERO,
        duration,
        mode: ClipMode::Once,
        source_offset: MusicalTime::ZERO,
        transpose_semitones: 0,
        gain_db: 0.0,
        fade_in: MusicalTime::ZERO,
        fade_out: MusicalTime::ZERO,
        groove: None,
        sites: ClipSites {
            site: site.clone(),
            pattern: site.clone(),
            start: site.clone(),
            duration: site.clone(),
            mode: site,
            source_offset: None,
            transpose: None,
            gain: None,
            fade_in: None,
            fade_out: None,
            groove: None,
        },
        location: pattern.location.clone(),
    };
    require_track(project, track_id)?;
    project
        .song
        .tracks
        .iter_mut()
        .find(|track| track.id == track_id)
        .expect("required Track")
        .clips
        .push(clip);
    Ok(())
}

fn clip_bounds(
    tracks: &[&Track],
    pattern: &str,
) -> Result<Option<(MusicalTime, MusicalTime)>, EngineError> {
    let mut bounds: Option<(MusicalTime, MusicalTime)> = None;
    for clip in tracks
        .iter()
        .flat_map(|track| &track.clips)
        .filter(|clip| clip.pattern == pattern)
    {
        let end = clip
            .start
            .checked_add(clip.duration)
            .map_err(EngineError::Time)?;
        bounds = Some(match bounds {
            None => (clip.start, end),
            Some((start, current_end)) => (start.min(clip.start), current_end.max(end)),
        });
    }
    Ok(bounds)
}

fn resolve_range(
    project: &Project,
    range: &AuditionRange,
    default_start: u64,
    default_end: u64,
    maximum_end: u64,
) -> Result<(u64, u64), EngineError> {
    let start_frame = if let Some(value) = range.from.as_deref() {
        resolve_position(project, value)?.to_frames(project.song.tempo_bpm, project.sample_rate)?
    } else {
        default_start
    };
    let end_frame = if let Some(value) = range.to.as_deref() {
        resolve_position(project, value)?.to_frames(project.song.tempo_bpm, project.sample_rate)?
    } else {
        default_end
    };
    if start_frame >= end_frame || end_frame > maximum_end {
        return Err(compile_error(format!(
            "audition range [{start_frame}, {end_frame}) must be positive and lie within target frame {maximum_end}"
        )));
    }
    Ok((start_frame, end_frame))
}

fn resolve_position(project: &Project, value: &str) -> Result<MusicalTime, EngineError> {
    if let Some(locator) = project
        .song
        .locators
        .iter()
        .find(|locator| locator.id == value)
    {
        return Ok(locator.at);
    }
    parse_musical_time(value, project.song.meter).map_err(EngineError::Time)
}

fn compiled_track_index(compiled: &CompiledProject, track_id: &str) -> Option<usize> {
    compiled
        .tracks
        .iter()
        .position(|track| track.schedule.track_id == track_id)
}

fn require_primary_track<'a>(track: Option<&'a str>, path: &str) -> Result<&'a str, EngineError> {
    track.ok_or_else(|| {
        compile_error(format!(
            "{path} audition path requires one explicit target Track"
        ))
    })
}

fn compiled_region(
    source: &CompiledSampleSource,
    original_start: u64,
    original_end: u64,
) -> Result<(usize, usize), EngineError> {
    let source_end = if original_end == u64::MAX {
        source.len()
    } else {
        let scale = f64::from(source.sample_rate()) / f64::from(source.source_sample_rate());
        (original_end as f64 * scale).round() as usize
    };
    let scale = f64::from(source.sample_rate()) / f64::from(source.source_sample_rate());
    let source_start = (original_start as f64 * scale).round() as usize;
    if source_start >= source_end || source_end > source.len() {
        return Err(compile_error(format!(
            "raw audition region [{original_start}, {original_end}) is outside asset `{}`",
            source.asset_id()
        )));
    }
    Ok((source_start, source_end))
}

fn compile_error(message: String) -> EngineError {
    EngineError::Compile(message)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    use crate::{
        dsl::parse_project,
        engine::ProcessingMode,
        project::{PROJECT_FILE, ProjectBundle, load_project},
    };

    use super::*;

    fn fixture() -> Project {
        let document = parse_project(
            Path::new("project.crump"),
            include_str!("../../../tests/fixtures/minimal.crump"),
        );
        assert!(!document.diagnostics.has_errors());
        document.project.expect("fixture Project")
    }

    fn sampler_fixture() -> ProjectBundle {
        let root = tempdir().expect("Project root").keep();
        fs::create_dir(root.join("media")).expect("media directory");
        let wav = root.join("media/source.wav");
        let mut writer = hound::WavWriter::create(
            &wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .expect("WAV writer");
        for index in 0..1_024 {
            let sample = if (index / 16) % 2 == 0 {
                16_000_i16
            } else {
                -16_000_i16
            };
            writer.write_sample(sample).expect("WAV sample");
        }
        writer.finalize().expect("finalize WAV");
        let digest = Sha256::digest(fs::read(&wav).expect("WAV bytes"));
        let sha256 = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let source = format!(
            r#"crump 1
project "Audition Publication" {{
  id audition-publication
  seed 17
  sample_rate 48000
  render_tail 10ms
  asset source "Source" wav "media/source.wav" sha256 "{sha256}"
  song {{
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument samples "Samples" sampler {{
      polyphony 2
      parameter gain "Gain" db -96db 24db 0db 0db
      parameter tune "Tune" semitones -24st 24st 0st 0st
      parameter release "Release" seconds 0s 1s 5ms 5ms
      faceplate {{
        control gain "Gain" "Sampler level" maps gain
        control tune "Tune" "Sampler repitch" maps tune
        control release "Release" "Sampler release" maps release
      }}
      processor sample_color utility {{
        setting gain -2db
        setting width 1
      }}
      mapping zone asset source keys C4 C4 root C4 {{
        region 0 1024
        playback one_shot
        gain -3db
        pan 0.25
        envelope 0s 1ms
        fade 4 4
      }}
    }}
    pattern phrase 1bar {{
    }}
    track samples "Samples" instrument samples level -4db pan -0.2 {{
      processor track_color utility {{
        setting gain -3db
        setting width 1
      }}
      clip phrase pattern phrase start 0bar duration 1bar mode once
    }}
    master level -1db {{
      processor master_color utility {{
        setting gain -2db
        setting width 1
      }}
    }}
  }}
}}
"#
        );
        fs::write(root.join(PROJECT_FILE), source).expect("Project source");
        load_project(&root).expect("load sampler Project")
    }

    #[test]
    fn plan_prepares_and_reports_solo_and_cue_through_the_shared_seam() {
        let project = fixture();
        let plan = AuditionPlan {
            soloed_channels: BTreeSet::from(["track/lead".to_owned()]),
            cue: Some(AuditionCue {
                channel: "track/drums".to_owned(),
                tap: TapPoint::PreFader,
            }),
            ..AuditionPlan::default()
        };

        let prepared = plan
            .prepare(
                &RevisionId("solo-cue".to_owned()),
                &project,
                &AssetInventory::default(),
            )
            .expect("prepare Solo/cue audition");
        assert_eq!(prepared.status.soloed_channels, plan.soloed_channels);
        assert_eq!(prepared.status.cue, plan.cue);
    }

    #[test]
    fn range_relative_resume_maps_into_newly_resolved_boundaries() {
        let project = fixture();
        let offset = 6_000;
        let original = AuditionPlan {
            range: AuditionRange {
                from: Some("0beat".to_owned()),
                to: Some("2beat".to_owned()),
                looping: true,
            },
            ..AuditionPlan::default()
        }
        .prepare_with_options(
            &RevisionId("relative-original".to_owned()),
            &project,
            &AssetInventory::default(),
            AuditionPrepareOptions {
                initial_frame: None,
                initial_range_offset: Some(offset),
                explicit_seek: false,
            },
        )
        .expect("prepare original relative position");
        let moved = AuditionPlan {
            range: AuditionRange {
                from: Some("1beat".to_owned()),
                to: Some("3beat".to_owned()),
                looping: true,
            },
            ..AuditionPlan::default()
        }
        .prepare_with_options(
            &RevisionId("relative-moved".to_owned()),
            &project,
            &AssetInventory::default(),
            AuditionPrepareOptions {
                initial_frame: None,
                initial_range_offset: Some(offset),
                explicit_seek: false,
            },
        )
        .expect("prepare moved relative position");

        assert_eq!(
            original.status.transport_frame - original.status.start_frame,
            offset
        );
        assert_eq!(
            moved.status.transport_frame - moved.status.start_frame,
            offset
        );
        assert_ne!(
            original.status.transport_frame,
            moved.status.transport_frame
        );

        let original_start = original.status.start_frame;
        let original_end = original.status.end_frame;
        let moved_start = moved.status.start_frame;
        let moved_end = moved.status.end_frame;
        let stale_prepared_frame = moved.status.transport_frame;
        let mut current = original.into_runtime();
        for _ in 0..137 {
            current
                .process_frame(ProcessingMode::Monitoring)
                .expect("advance current transport after controller preparation");
        }
        let callback_offset = current.frame().saturating_sub(original_start)
            % original_end.saturating_sub(original_start);
        let expected_frame = moved_start
            + callback_offset.min(moved_end.saturating_sub(moved_start).saturating_sub(1));
        let mut next = moved.into_runtime();
        next.prime_for_publication();
        current.prepare_transition(&mut next);
        assert_ne!(
            next.frame(),
            stale_prepared_frame,
            "controller-time telemetry must not decide the eventual publication phase"
        );
        assert_eq!(
            next.frame(),
            expected_frame,
            "callback preparation must map its exact current range offset into the moved range"
        );

        current.complete_transition(&mut next);
        current.seek_transport(original_end);
        next.seek_transport(moved_start + 123);
        next.prime_for_publication();
        current.prepare_transition(&mut next);
        assert_eq!(
            next.frame(),
            moved_start,
            "the exclusive loop end is the next cycle's zero offset, not the last frame"
        );
    }

    #[test]
    fn exact_seek_matches_uninterrupted_offline_state() {
        let project = fixture();
        let revision = RevisionId("test".to_owned());
        let plan = AuditionPlan {
            range: AuditionRange {
                from: Some("1beat".to_owned()),
                to: Some("3beat".to_owned()),
                looping: false,
            },
            ..AuditionPlan::default()
        };
        let prepared = plan
            .prepare(&revision, &project, &AssetInventory::default())
            .expect("prepare exact seek");
        let mut sought = prepared.into_runtime();
        let compiled = CompiledProject::compile(&project).expect("compile baseline");
        let mut baseline = compiled.runtime();
        baseline
            .advance_to(sought.frame())
            .expect("advance baseline");
        for _ in 0..256 {
            assert_eq!(
                sought
                    .process_frame(super::super::ProcessingMode::Offline)
                    .unwrap(),
                baseline
                    .process_frame(super::super::ProcessingMode::Offline)
                    .unwrap()
            );
        }
    }

    #[test]
    fn sample_mapping_is_audible_at_every_supported_path_depth() {
        let bundle = sampler_fixture();
        let project = bundle.publishable().expect("sampler Project");
        let target = AuditionTarget::Mapping {
            instrument: "samples".to_owned(),
            mapping: "zone".to_owned(),
            track: Some("samples".to_owned()),
        };
        let paths = [
            AuditionPath::Raw,
            AuditionPath::Mapping,
            AuditionPath::Channel {
                tap: TapPoint::PreFader,
            },
            AuditionPath::Channel {
                tap: TapPoint::PostFader,
            },
            AuditionPath::Master,
        ];
        let mut rendered = Vec::new();
        for path in paths {
            let plan = AuditionPlan {
                target: target.clone(),
                path,
                ..AuditionPlan::default()
            };
            let prepare = || {
                plan.prepare(
                    &RevisionId(format!("path-{path:?}")),
                    project,
                    &bundle.assets,
                )
                .unwrap_or_else(|error| panic!("prepare {path:?}: {error}"))
                .into_runtime()
            };
            let mut actual_runtime = prepare();
            let actual = (0..256)
                .map(|_| {
                    actual_runtime
                        .process_frame(ProcessingMode::Monitoring)
                        .unwrap_or_else(|error| panic!("process {path:?}: {error}"))
                })
                .collect::<Vec<_>>();
            let expected = if path == AuditionPath::Raw {
                let mut reference = prepare();
                let raw = reference
                    .raw_audition
                    .as_mut()
                    .expect("prepared raw source");
                (0..256)
                    .map(|_| raw.process().map(super::super::monitor_sample))
                    .collect::<Vec<_>>()
            } else {
                let mut reference = prepare();
                let channel = reference
                    .mix
                    .channel_index("track/samples")
                    .expect("sample Track channel");
                (0..256)
                    .map(|_| {
                        let master = reference
                            .process_frame(ProcessingMode::Offline)
                            .expect("reference routed frame");
                        let selected = match path {
                            AuditionPath::Mapping => reference
                                .mix
                                .source_tap(channel)
                                .expect("Mapping source tap"),
                            AuditionPath::Channel { tap } => reference
                                .channel_tap_sample(channel, tap)
                                .expect("Track channel tap"),
                            AuditionPath::Master => master,
                            AuditionPath::Raw => unreachable!("handled above"),
                        };
                        selected.map(super::super::monitor_sample)
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(actual, expected, "exact {path:?} monitoring path");
            assert!(
                actual.iter().flatten().any(|sample| sample.abs() > 1.0e-6),
                "sample Mapping was silent at {path:?} depth"
            );
            rendered.push((path, actual));
        }
        for pair in rendered.windows(2) {
            assert_ne!(
                pair[0].1, pair[1].1,
                "fixture treatment must distinguish {:?} from {:?}",
                pair[0].0, pair[1].0
            );
        }
    }

    #[test]
    fn raw_asset_natural_range_can_extend_beyond_song_end() {
        let bundle = sampler_fixture();
        let mut project = bundle.publishable().expect("sampler Project").clone();
        project.song.end =
            parse_musical_time("1/100beat", project.song.meter).expect("short exact Song end");
        let plan = AuditionPlan {
            target: AuditionTarget::Asset {
                asset: "source".to_owned(),
                start_frame: None,
                end_frame: None,
            },
            path: AuditionPath::Raw,
            ..AuditionPlan::default()
        };

        let prepared = plan
            .prepare(&RevisionId("long-raw".to_owned()), &project, &bundle.assets)
            .expect("prepare raw asset longer than Song");
        assert_eq!(prepared.status.start_frame, 0);
        assert_eq!(prepared.status.end_frame, 1_024);
        assert!(
            prepared.status.end_frame
                > project
                    .song
                    .end
                    .to_frames(project.song.tempo_bpm, project.sample_rate)
                    .expect("Song frames")
        );
        let mut runtime = prepared.into_runtime();
        let mut heard_last_frame = false;
        for frame in 0..1_024 {
            let output = runtime
                .process_frame(ProcessingMode::Monitoring)
                .expect("raw frame");
            if frame == 1_023 {
                heard_last_frame = output.into_iter().any(|sample| sample.abs() > 1.0e-6);
            }
        }
        assert!(heard_last_frame);
        assert!(runtime.finished());
    }

    #[test]
    fn raw_publication_realigns_and_advances_the_replacement_cursor() {
        let bundle = sampler_fixture();
        let project = bundle.publishable().expect("sampler Project");
        let revision = RevisionId("raw-publication".to_owned());
        let plan = AuditionPlan {
            target: AuditionTarget::Asset {
                asset: "source".to_owned(),
                start_frame: None,
                end_frame: None,
            },
            path: AuditionPath::Raw,
            ..AuditionPlan::default()
        };
        let at_frame = |frame, explicit_seek| AuditionPrepareOptions {
            initial_frame: Some(frame),
            initial_range_offset: None,
            explicit_seek,
        };
        let mut baseline = plan
            .prepare_with_options(&revision, project, &bundle.assets, at_frame(300, true))
            .expect("prepare baseline raw audition")
            .into_runtime();
        let mut current = plan
            .prepare_with_options(&revision, project, &bundle.assets, at_frame(300, true))
            .expect("prepare current raw audition")
            .into_runtime();
        let mut next = plan
            .prepare_with_options(&revision, project, &bundle.assets, at_frame(80, false))
            .expect("prepare stale replacement raw audition")
            .into_runtime();
        next.prime_for_publication();
        current.prepare_transition(&mut next);

        for position in 1..=64_u64 {
            baseline
                .process_frame(ProcessingMode::Monitoring)
                .expect("baseline transition frame");
            let phase = position as f64 / 64.0;
            current
                .process_transition_frame(
                    &mut next,
                    (phase * std::f64::consts::FRAC_PI_2).cos() as f32,
                    (phase * std::f64::consts::FRAC_PI_2).sin() as f32,
                )
                .expect("raw transition frame");
        }
        current.complete_transition(&mut next);

        for _ in 0..128 {
            assert_eq!(
                next.process_frame(ProcessingMode::Monitoring)
                    .expect("replacement raw frame"),
                baseline
                    .process_frame(ProcessingMode::Monitoring)
                    .expect("baseline raw frame")
            );
        }
    }

    #[test]
    fn incompatible_mapping_publication_refires_the_selected_mapping() {
        let bundle = sampler_fixture();
        let project = bundle.publishable().expect("sampler Project");
        let mut replacement_project = project.clone();
        replacement_project.song.instruments[0]
            .sampler
            .as_mut()
            .expect("sampler definition")
            .mappings[0]
            .gain_db = -6.0;
        let plan = AuditionPlan {
            target: AuditionTarget::Mapping {
                instrument: "samples".to_owned(),
                mapping: "zone".to_owned(),
                track: Some("samples".to_owned()),
            },
            path: AuditionPath::Mapping,
            ..AuditionPlan::default()
        };
        let revision = RevisionId("mapping-publication".to_owned());
        let mut current = plan
            .prepare_with_options(
                &revision,
                project,
                &bundle.assets,
                AuditionPrepareOptions {
                    initial_frame: Some(300),
                    initial_range_offset: None,
                    explicit_seek: true,
                },
            )
            .expect("prepare current Mapping audition")
            .into_runtime();
        let mut next = plan
            .prepare_with_options(
                &revision,
                &replacement_project,
                &bundle.assets,
                AuditionPrepareOptions {
                    initial_frame: Some(80),
                    initial_range_offset: None,
                    explicit_seek: false,
                },
            )
            .expect("prepare replacement Mapping audition")
            .into_runtime();
        next.prime_for_publication();
        current.prepare_transition(&mut next);
        assert!(
            current
                .tracks
                .iter()
                .any(|track| !track.transition_unchanged),
            "the changed sampler must restart behind the publication guard"
        );

        let mut heard_replacement = false;
        for _ in 0..128 {
            let output = current
                .process_transition_frame(&mut next, 0.0, 1.0)
                .expect("Mapping transition frame");
            heard_replacement |= output.into_iter().any(|sample| sample.abs() > 1.0e-6);
        }
        assert!(
            heard_replacement,
            "an incompatible replacement must re-fire the armed Mapping instead of remaining silent"
        );
    }

    #[test]
    fn live_processor_bypass_telemetry_is_not_replaced_by_static_plan_state() {
        let document = parse_project(
            Path::new("production_mix.crump"),
            include_str!("../../../tests/fixtures/production_mix.crump"),
        );
        assert!(!document.diagnostics.has_errors());
        let project = document.project.expect("production fixture");
        let prepared = AuditionPlan::default()
            .prepare(
                &RevisionId("telemetry".to_owned()),
                &project,
                &AssetInventory::default(),
            )
            .expect("prepare production audition");
        let processor = prepared
            .mix_graph
            .processors
            .first()
            .expect("fixture processor")
            .clone();
        let mut telemetry = RuntimeTelemetry {
            processor_count: prepared.mix_graph.processors.len(),
            ..RuntimeTelemetry::default()
        };
        telemetry.processor_activity[processor.index] = ProcessorMeter {
            authored_bypass: true,
            transient_bypass: Some(false),
            effective_bypass: false,
            bypass_mix: 0.25,
            ..ProcessorMeter::default()
        };
        let overrides = BTreeMap::from([(processor.id.clone(), true)]);

        let inspection =
            inspection_from_parts(prepared.status, &prepared.mix_graph, telemetry, &overrides);
        let actual = inspection
            .processors
            .into_iter()
            .find(|candidate| candidate.processor.id == processor.id)
            .expect("inspected processor");
        assert!(actual.activity.authored_bypass);
        assert_eq!(actual.transient_bypass, Some(false));
        assert!(!actual.effective_bypass);
        assert_eq!(actual.activity.bypass_mix, 0.25);
    }

    #[test]
    fn pattern_target_removes_unselected_clips_without_mutating_source_project() {
        let project = fixture();
        let original_clip_count = project
            .song
            .tracks
            .iter()
            .map(|track| track.clips.len())
            .sum::<usize>();
        let target = AuditionTarget::Pattern {
            pattern: project.song.patterns[0].id.clone(),
            track: None,
        };
        let selection = resolve_target(&project, &target).expect("resolve Pattern");
        assert_eq!(
            project
                .song
                .tracks
                .iter()
                .map(|track| track.clips.len())
                .sum::<usize>(),
            original_clip_count
        );
        assert!(selection.project.song.tracks.iter().all(|track| {
            track
                .clips
                .iter()
                .all(|clip| clip.pattern == project.song.patterns[0].id)
        }));
    }
}
