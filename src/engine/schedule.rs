//! Deterministic expansion of finite Patterns and Clips into runtime Events.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use crate::{
    model::{
        ClipMode, GrooveTemplate, InstrumentKind, ParameterLock, ParameterTarget, Pattern,
        PatternEvent, Project, SampleDirection, SampleLoopRegion,
    },
    time::MusicalTime,
};

use super::EngineError;

pub(crate) const MAX_RUNTIME_TRACKS: usize = 32;
pub(crate) const MAX_TRACK_SCHEDULE_EVENTS: usize = 32_768;
pub(crate) const MAX_PROJECT_SCHEDULE_EVENTS: usize = 65_536;
pub(crate) const MAX_EVENTS_AT_ONE_FRAME: usize = 256;

/// Runtime events for one Track, sorted by deterministic sample-time order.
#[derive(Debug, Clone)]
pub(crate) struct TrackSchedule {
    pub(crate) instrument_id: String,
    pub(crate) track_id: String,
    pub(crate) events: Vec<ScheduledEvent>,
}

impl TrackSchedule {
    /// Owned allocation retained by one Arc-shared compiled schedule.
    pub(crate) fn allocation_bytes(&self) -> u64 {
        let word = std::mem::size_of::<usize>() as u64;
        (std::mem::size_of::<Self>() as u64)
            .saturating_add(word.saturating_mul(2))
            .saturating_add(self.instrument_id.capacity() as u64)
            .saturating_add(self.track_id.capacity() as u64)
            .saturating_add(
                (self.events.capacity() as u64)
                    .saturating_mul(std::mem::size_of::<ScheduledEvent>() as u64),
            )
            .saturating_add(
                self.events
                    .iter()
                    .map(|event| {
                        (event.order.track_id.capacity() as u64)
                            .saturating_add(event.order.clip_id.capacity() as u64)
                    })
                    .fold(0_u64, u64::saturating_add),
            )
    }
}

/// One exact runtime Event.
#[derive(Debug, Clone)]
pub(crate) struct ScheduledEvent {
    pub(crate) frame: u64,
    pub(crate) order: EventOrder,
    pub(crate) kind: RuntimeEvent,
}

impl ScheduledEvent {
    /// Durable Clip that produced this materialized Event.
    pub(crate) fn clip_id(&self) -> &str {
        &self.order.clip_id
    }
}

/// Tie-break identity independent of authored Track presentation order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct EventOrder {
    // Releases sort before attacks at the same sample so voice availability and
    // overlap behavior do not depend on Clip or authored Event ordering.
    phase: u8,
    track_id: String,
    clip_id: String,
    iteration: u64,
    semantic_key: [u8; 32],
    duplicate_ordinal: usize,
    pitch_or_voice: u16,
}

/// One per-event copy of the enclosing Clip's gain/fade envelope.
///
/// Voices advance this envelope from the Event's position in the Clip, so overlapping Clips remain
/// additive and retain independent gain/fade semantics without callback-time timeline searches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ClipEnvelope {
    pub(crate) gain: f32,
    pub(crate) position_frames: u64,
    pub(crate) duration_frames: u64,
    pub(crate) fade_in_frames: u64,
    pub(crate) fade_out_frames: u64,
}

impl ClipEnvelope {
    pub(crate) const TRANSPARENT: Self = Self {
        gain: 1.0,
        position_frames: 0,
        duration_frames: 0,
        fade_in_frames: 0,
        fade_out_frames: 0,
    };

    pub(crate) fn gain_at(self, age_frames: u64) -> f32 {
        let position = self.position_frames.saturating_add(age_frames);
        let fade_in = if self.fade_in_frames == 0 {
            1.0
        } else {
            (position.saturating_add(1) as f64 / self.fade_in_frames as f64).clamp(0.0, 1.0) as f32
        };
        let fade_out = if self.fade_out_frames == 0 || self.duration_frames == 0 {
            1.0
        } else {
            (self.duration_frames.saturating_sub(position) as f64 / self.fade_out_frames as f64)
                .clamp(0.0, 1.0) as f32
        };
        self.gain * fade_in * fade_out
    }
}

/// Runtime-consumable concrete Event.
#[derive(Debug, Clone)]
pub(crate) enum RuntimeEvent {
    NoteOn {
        note_id: u64,
        pitch: u8,
        velocity: f32,
        seed: u64,
        clip_envelope: ClipEnvelope,
    },
    NoteOff {
        note_id: u64,
    },
    Hit {
        voice: crate::model::DrumVoice,
        velocity: f32,
        seed: u64,
        clip_envelope: ClipEnvelope,
    },
    SampleOn {
        note_id: u64,
        mapping_index: u16,
        start_frame: u64,
        end_frame: u64,
        gain_db: f32,
        pan: f32,
        fade_in_frames: u32,
        fade_out_frames: u32,
        direction: Option<SampleDirection>,
        loop_region: Option<SampleLoopRegion>,
        velocity: f32,
        clip_envelope: ClipEnvelope,
    },
    SampleOff {
        note_id: u64,
    },
}

/// Compiles every Track's event schedule.
pub(crate) fn compile_schedules(project: &Project) -> Result<Vec<TrackSchedule>, EngineError> {
    if project.song.tracks.len() > MAX_RUNTIME_TRACKS {
        return Err(EngineError::Compile(format!(
            "Project declares {} Tracks; runtime maximum is {MAX_RUNTIME_TRACKS}",
            project.song.tracks.len()
        )));
    }
    let patterns = materialize_patterns(project)?;
    let grooves: BTreeMap<_, _> = project
        .song
        .grooves
        .iter()
        .map(|groove| (groove.id.as_str(), groove))
        .collect();
    let mut schedules = Vec::with_capacity(project.song.tracks.len());
    for track in &project.song.tracks {
        let instrument = project
            .song
            .instruments
            .iter()
            .find(|instrument| instrument.id == track.instrument)
            .ok_or_else(|| {
                EngineError::Compile(format!(
                    "Track `{}` references missing Instrument",
                    track.id
                ))
            })?;
        let mapping_indexes = if instrument.kind == InstrumentKind::Sampler {
            instrument
                .sampler
                .as_ref()
                .map(|sampler| {
                    sampler
                        .mappings
                        .iter()
                        .enumerate()
                        .map(|(index, mapping)| {
                            u16::try_from(index)
                                .map(|index| (mapping.id.as_str(), index))
                                .map_err(|_| {
                                    EngineError::Compile(
                                        "sampler mapping index exceeds u16".to_owned(),
                                    )
                                })
                        })
                        .collect::<Result<BTreeMap<_, _>, _>>()
                })
                .transpose()?
                .unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        let mut events = Vec::new();
        for clip in &track.clips {
            let pattern = patterns.get(clip.pattern.as_str()).ok_or_else(|| {
                EngineError::Compile(format!("Clip `{}` references missing Pattern", clip.id))
            })?;
            let groove = clip
                .groove
                .as_deref()
                .map(|groove| {
                    grooves.get(groove).copied().ok_or_else(|| {
                        EngineError::Compile(format!(
                            "Clip `{}` references missing Groove Template `{groove}`",
                            clip.id
                        ))
                    })
                })
                .transpose()?;
            let traversal_end = clip.source_offset.checked_add(clip.duration)?;
            let iterations = match clip.mode {
                ClipMode::Once => 1,
                ClipMode::Loop => ceil_ratio(traversal_end, pattern.span)?,
            };
            let event_bound = usize::try_from(iterations)
                .ok()
                .and_then(|count| count.checked_mul(pattern.events.len()))
                .and_then(|count| count.checked_mul(2))
                .ok_or_else(|| {
                    EngineError::Compile(format!("Clip `{}` schedule bound overflows", clip.id))
                })?;
            if event_bound > MAX_TRACK_SCHEDULE_EVENTS.saturating_sub(events.len()) {
                return Err(EngineError::Compile(format!(
                    "Track `{}` exceeds the {MAX_TRACK_SCHEDULE_EVENTS}-Event compiled schedule maximum",
                    track.id
                )));
            }
            let clip_end = clip.start.checked_add(clip.duration)?;
            let bounded_clip_end = clip_end.min(project.song.end);
            let clip_start_frame = clip
                .start
                .to_frames(project.song.tempo_bpm, project.sample_rate)?;
            let clip_end_frame =
                bounded_clip_end.to_frames(project.song.tempo_bpm, project.sample_rate)?;
            let clip_duration_frames = clip_end_frame.saturating_sub(clip_start_frame);
            let fade_in_frames = clip
                .fade_in
                .to_frames(project.song.tempo_bpm, project.sample_rate)?;
            let fade_out_frames = clip
                .fade_out
                .to_frames(project.song.tempo_bpm, project.sample_rate)?;
            let clip_gain = 10.0_f64.powf(clip.gain_db / 20.0) as f32;
            for iteration in 0..iterations {
                let pattern_offset = pattern.span.checked_mul(iteration)?;
                for canonical in canonical_events(&pattern.events) {
                    let event = canonical.event;
                    let source_position = pattern_offset.checked_add(event.at())?;
                    if source_position < clip.source_offset {
                        continue;
                    }
                    let relative = source_position.checked_sub(clip.source_offset)?;
                    if relative >= clip.duration {
                        continue;
                    }
                    let stable = event_instance_id(
                        project.seed,
                        &project.id,
                        &track.id,
                        &clip.id,
                        iteration,
                        &canonical.semantic_key,
                        canonical.duplicate_ordinal,
                    );
                    if groove.is_some_and(|groove| !groove_event_is_included(groove, stable)) {
                        continue;
                    }
                    let grooved_relative =
                        apply_groove_timing(relative, source_position, groove, stable)?
                            .max(MusicalTime::ZERO);
                    let relative = if grooved_relative < clip.duration {
                        grooved_relative
                    } else {
                        relative
                    };
                    let onset = clip.start.checked_add(relative)?;
                    if onset >= clip_end || onset >= project.song.end {
                        continue;
                    }
                    let frame = onset.to_frames(project.song.tempo_bpm, project.sample_rate)?;
                    let clip_envelope = ClipEnvelope {
                        gain: clip_gain,
                        position_frames: frame.saturating_sub(clip_start_frame),
                        duration_frames: clip_duration_frames,
                        fade_in_frames,
                        fade_out_frames,
                    };
                    let order = |phase, pitch_or_voice: u16| EventOrder {
                        phase,
                        track_id: track.id.clone(),
                        clip_id: clip.id.clone(),
                        iteration,
                        semantic_key: canonical.semantic_key,
                        duplicate_ordinal: canonical.duplicate_ordinal,
                        pitch_or_voice,
                    };
                    match event {
                        PatternEvent::Note {
                            pitch,
                            duration,
                            velocity,
                            ..
                        } => {
                            let transformed_pitch = u8::try_from(
                                i16::from(*pitch).saturating_add(clip.transpose_semitones),
                            )
                            .map_err(|_| {
                                EngineError::Compile(format!(
                                    "Clip `{}` transposes a note outside the MIDI pitch domain",
                                    clip.id
                                ))
                            })?;
                            let authored_off = onset.checked_add(*duration)?;
                            let off = authored_off.min(clip_end).min(project.song.end);
                            let mut off_frame =
                                off.to_frames(project.song.tempo_bpm, project.sample_rate)?;
                            let boundary_frame = clip_end_frame;
                            if off_frame <= frame && boundary_frame > frame {
                                off_frame = frame + 1;
                            }
                            if off_frame <= frame {
                                continue;
                            }
                            let note_id = stable;
                            events.push(ScheduledEvent {
                                frame,
                                order: order(1, u16::from(transformed_pitch)),
                                kind: RuntimeEvent::NoteOn {
                                    note_id,
                                    pitch: transformed_pitch,
                                    velocity: groove_velocity(*velocity, groove, stable),
                                    seed: stable_u64(stable, b"voice"),
                                    clip_envelope,
                                },
                            });
                            events.push(ScheduledEvent {
                                frame: off_frame,
                                order: order(0, u16::from(transformed_pitch)),
                                kind: RuntimeEvent::NoteOff { note_id },
                            });
                        }
                        PatternEvent::Hit {
                            voice, velocity, ..
                        } => {
                            events.push(ScheduledEvent {
                                frame,
                                order: order(1, u16::from(*voice as u8)),
                                kind: RuntimeEvent::Hit {
                                    voice: *voice,
                                    velocity: groove_velocity(*velocity, groove, stable),
                                    seed: stable_u64(stable, b"drum"),
                                    clip_envelope,
                                },
                            });
                        }
                        PatternEvent::Sample {
                            mapping,
                            duration,
                            start_frame,
                            end_frame,
                            gain_db,
                            pan,
                            fade_in_frames,
                            fade_out_frames,
                            velocity,
                            direction,
                            loop_region,
                            ..
                        } => {
                            let mapping_index =
                                *mapping_indexes.get(mapping.as_str()).ok_or_else(|| {
                                    EngineError::Compile(format!(
                                        "sample Event references missing mapping `{mapping}`"
                                    ))
                                })?;
                            let authored_off = onset.checked_add(*duration)?;
                            let off = authored_off.min(clip_end).min(project.song.end);
                            let mut off_frame =
                                off.to_frames(project.song.tempo_bpm, project.sample_rate)?;
                            let boundary_frame = clip_end_frame;
                            if off_frame <= frame && boundary_frame > frame {
                                off_frame = frame + 1;
                            }
                            if off_frame <= frame {
                                continue;
                            }
                            events.push(ScheduledEvent {
                                frame,
                                order: order(1, mapping_index),
                                kind: RuntimeEvent::SampleOn {
                                    note_id: stable,
                                    mapping_index,
                                    start_frame: *start_frame,
                                    end_frame: *end_frame,
                                    gain_db: *gain_db as f32,
                                    pan: *pan as f32,
                                    fade_in_frames: *fade_in_frames,
                                    fade_out_frames: *fade_out_frames,
                                    direction: *direction,
                                    loop_region: *loop_region,
                                    velocity: groove_velocity(*velocity, groove, stable),
                                    clip_envelope,
                                },
                            });
                            events.push(ScheduledEvent {
                                frame: off_frame,
                                order: order(0, mapping_index),
                                kind: RuntimeEvent::SampleOff { note_id: stable },
                            });
                        }
                    }
                }
            }
        }
        events.sort_by(|left, right| (left.frame, &left.order).cmp(&(right.frame, &right.order)));
        schedules.push(TrackSchedule {
            instrument_id: track.instrument.clone(),
            track_id: track.id.clone(),
            events,
        });
    }
    schedules.sort_by(|left, right| left.track_id.cmp(&right.track_id));
    let total_events = schedules
        .iter()
        .try_fold(0_usize, |total, schedule| {
            total.checked_add(schedule.events.len())
        })
        .ok_or_else(|| EngineError::Compile("Project schedule Event count overflows".to_owned()))?;
    if total_events > MAX_PROJECT_SCHEDULE_EVENTS {
        return Err(EngineError::Compile(format!(
            "Project exceeds the {MAX_PROJECT_SCHEDULE_EVENTS}-Event compiled schedule maximum"
        )));
    }
    let mut events_per_frame = BTreeMap::<u64, usize>::new();
    for event in schedules.iter().flat_map(|schedule| &schedule.events) {
        let count = events_per_frame.entry(event.frame).or_default();
        *count = count.saturating_add(1);
        if *count > MAX_EVENTS_AT_ONE_FRAME {
            return Err(EngineError::Compile(format!(
                "frame {} exceeds the {MAX_EVENTS_AT_ONE_FRAME}-Event dispatch maximum",
                event.frame
            )));
        }
    }
    Ok(schedules)
}

/// One compiled step change for an Instrument Parameter lock lane.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScheduledParameterLockPoint {
    /// Absolute authored sample frame.
    pub(crate) frame: u64,
    /// Physical lock value, or `None` to restore Automation/base at Clip end.
    pub(crate) value: Option<String>,
}

#[derive(Debug, Clone)]
struct AuthoredParameterLockPoint {
    frame: u64,
    phase: u8,
    order: String,
    value: Option<String>,
}

/// Materializes Pattern Parameter locks into bounded deterministic Song-time lanes.
pub(crate) fn compile_track_parameter_lock_lanes(
    project: &Project,
) -> Result<BTreeMap<(String, ParameterTarget), Vec<ScheduledParameterLockPoint>>, EngineError> {
    const MAX_PARAMETER_LOCK_POINTS: usize = 8_192;

    let patterns = materialize_patterns(project)?;
    let mut lanes = BTreeMap::<(String, ParameterTarget), Vec<AuthoredParameterLockPoint>>::new();
    let mut point_count = 0usize;
    for track in &project.song.tracks {
        for clip in &track.clips {
            let pattern = patterns.get(clip.pattern.as_str()).ok_or_else(|| {
                EngineError::Compile(format!("Clip `{}` references missing Pattern", clip.id))
            })?;
            if pattern.locks.is_empty() {
                continue;
            }
            let traversal_end = clip.source_offset.checked_add(clip.duration)?;
            let iterations = match clip.mode {
                ClipMode::Once => 1,
                ClipMode::Loop => ceil_ratio(traversal_end, pattern.span)?,
            };
            let clip_start_frame = clip
                .start
                .to_frames(project.song.tempo_bpm, project.sample_rate)?;
            let clip_end_frame = clip
                .start
                .checked_add(clip.duration)?
                .min(project.song.end)
                .to_frames(project.song.tempo_bpm, project.sample_rate)?;
            let mut locks = pattern.locks.iter().collect::<Vec<_>>();
            locks.sort_by(|left, right| {
                (&left.at, &left.parameter, &left.id).cmp(&(&right.at, &right.parameter, &right.id))
            });
            let mut clip_targets = BTreeSet::new();
            let mut prior = BTreeMap::<ParameterTarget, (MusicalTime, String, String)>::new();
            for iteration in 0..iterations {
                let pattern_offset = pattern.span.checked_mul(iteration)?;
                for lock in &locks {
                    let source_position = pattern_offset.checked_add(lock.at)?;
                    if source_position >= traversal_end {
                        continue;
                    }
                    let target = ParameterTarget::Instrument {
                        instrument: track.instrument.clone(),
                        parameter: lock.parameter.clone(),
                    };
                    if source_position < clip.source_offset {
                        let candidate = (source_position, lock.id.clone(), lock.value.clone());
                        if prior.get(&target).is_none_or(|current| {
                            (candidate.0, &candidate.1) > (current.0, &current.1)
                        }) {
                            prior.insert(target, candidate);
                        }
                        continue;
                    }
                    let relative = source_position.checked_sub(clip.source_offset)?;
                    if relative >= clip.duration {
                        continue;
                    }
                    let frame = clip
                        .start
                        .checked_add(relative)?
                        .to_frames(project.song.tempo_bpm, project.sample_rate)?;
                    clip_targets.insert(target.clone());
                    if point_count >= MAX_PARAMETER_LOCK_POINTS {
                        return Err(EngineError::Compile(format!(
                            "Project exceeds the {MAX_PARAMETER_LOCK_POINTS}-point Parameter lock maximum"
                        )));
                    }
                    lanes.entry((track.id.clone(), target)).or_default().push(
                        AuthoredParameterLockPoint {
                            frame,
                            phase: 1,
                            order: format!("{}:{iteration}:{}", clip.id, lock.id),
                            value: Some(lock.value.clone()),
                        },
                    );
                    point_count += 1;
                }
            }
            for (target, (_, lock_id, value)) in prior {
                if point_count >= MAX_PARAMETER_LOCK_POINTS {
                    return Err(EngineError::Compile(format!(
                        "Project exceeds the {MAX_PARAMETER_LOCK_POINTS}-point Parameter lock maximum"
                    )));
                }
                clip_targets.insert(target.clone());
                lanes.entry((track.id.clone(), target)).or_default().push(
                    AuthoredParameterLockPoint {
                        frame: clip_start_frame,
                        phase: 1,
                        order: format!("{}:prior:{lock_id}", clip.id),
                        value: Some(value),
                    },
                );
                point_count += 1;
            }
            for target in clip_targets {
                if point_count >= MAX_PARAMETER_LOCK_POINTS {
                    return Err(EngineError::Compile(format!(
                        "Project exceeds the {MAX_PARAMETER_LOCK_POINTS}-point Parameter lock maximum"
                    )));
                }
                lanes.entry((track.id.clone(), target)).or_default().push(
                    AuthoredParameterLockPoint {
                        frame: clip_end_frame,
                        phase: 0,
                        order: clip.id.clone(),
                        value: None,
                    },
                );
                point_count += 1;
            }
        }
    }

    Ok(lanes
        .into_iter()
        .map(|(owner, mut points)| {
            points.sort_by(|left, right| {
                (left.frame, left.phase, &left.order).cmp(&(right.frame, right.phase, &right.order))
            });
            let points = points
                .into_iter()
                .map(|point| ScheduledParameterLockPoint {
                    frame: point.frame,
                    value: point.value,
                })
                .collect();
            (owner, points)
        })
        .collect())
}

/// Materializes the singular inspection view of Track-owned lock lanes.
pub(crate) fn compile_parameter_lock_lanes(
    project: &Project,
) -> Result<BTreeMap<ParameterTarget, Vec<ScheduledParameterLockPoint>>, EngineError> {
    let mut lanes = BTreeMap::<ParameterTarget, Vec<ScheduledParameterLockPoint>>::new();
    for ((_track, target), points) in compile_track_parameter_lock_lanes(project)? {
        lanes.entry(target).or_default().extend(points);
    }
    for points in lanes.values_mut() {
        points.sort_by(|left, right| {
            (left.frame, u8::from(left.value.is_some()))
                .cmp(&(right.frame, u8::from(right.value.is_some())))
        });
    }
    Ok(lanes)
}

#[derive(Debug, Clone)]
struct MaterializedPattern {
    span: MusicalTime,
    events: Vec<PatternEvent>,
    locks: Vec<ParameterLock>,
}

fn materialize_patterns(
    project: &Project,
) -> Result<BTreeMap<String, MaterializedPattern>, EngineError> {
    let authored = project
        .song
        .patterns
        .iter()
        .map(|pattern| (pattern.id.as_str(), pattern))
        .collect::<BTreeMap<_, _>>();
    let mut materialized = BTreeMap::new();
    for pattern in &project.song.patterns {
        let mut visiting = BTreeSet::new();
        let _ = materialize_pattern(&pattern.id, &authored, &mut materialized, &mut visiting)?;
    }
    Ok(materialized)
}

fn materialize_pattern(
    id: &str,
    authored: &BTreeMap<&str, &Pattern>,
    materialized: &mut BTreeMap<String, MaterializedPattern>,
    visiting: &mut BTreeSet<String>,
) -> Result<MaterializedPattern, EngineError> {
    if let Some(pattern) = materialized.get(id) {
        return Ok(pattern.clone());
    }
    if !visiting.insert(id.to_owned()) {
        return Err(EngineError::Compile(format!(
            "Pattern Variation lineage contains a cycle at `{id}`"
        )));
    }
    let pattern = authored
        .get(id)
        .copied()
        .ok_or_else(|| EngineError::Compile(format!("missing Pattern `{id}`")))?;
    let result = if let Some(variation) = &pattern.variation {
        let source = materialize_pattern(&variation.source, authored, materialized, visiting)?;
        if source.span != pattern.span {
            return Err(EngineError::Compile(format!(
                "Pattern Variation `{id}` span differs from source `{}`",
                variation.source
            )));
        }
        let mut events = Vec::with_capacity(source.events.len());
        for canonical in canonical_events(&source.events) {
            let event_seed = variation_event_seed(
                variation.seed,
                id,
                &canonical.semantic_key,
                canonical.duplicate_ordinal,
            );
            if unit_random(event_seed) >= variation.probability {
                continue;
            }
            let mut event = canonical.event.clone();
            transform_variation_event(&mut event, pattern.span, variation)?;
            events.push(event);
        }
        let mut locks = source.locks;
        for lock in &mut locks {
            lock.at = transform_variation_time(lock.at, pattern.span, variation)?;
        }
        MaterializedPattern {
            span: pattern.span,
            events,
            locks,
        }
    } else {
        MaterializedPattern {
            span: pattern.span,
            events: pattern.events.clone(),
            locks: pattern.locks.clone(),
        }
    };
    visiting.remove(id);
    materialized.insert(id.to_owned(), result.clone());
    Ok(result)
}

fn transform_variation_event(
    event: &mut PatternEvent,
    span: MusicalTime,
    variation: &crate::model::PatternVariation,
) -> Result<(), EngineError> {
    let rotated = transform_variation_time(event.at(), span, variation)?;
    match event {
        PatternEvent::Note {
            at,
            pitch,
            velocity,
            ..
        } => {
            *at = rotated;
            *pitch =
                u8::try_from(i16::from(*pitch) + variation.transpose_semitones).map_err(|_| {
                    EngineError::Compile(
                        "Pattern Variation transposes a note outside the MIDI pitch domain"
                            .to_owned(),
                    )
                })?;
            *velocity = (*velocity * variation.velocity_scale).clamp(0.0, 1.0);
        }
        PatternEvent::Hit { at, velocity, .. } => {
            *at = rotated;
            *velocity = (*velocity * variation.velocity_scale).clamp(0.0, 1.0);
        }
        PatternEvent::Sample { at, velocity, .. } => {
            *at = rotated;
            *velocity = (*velocity * variation.velocity_scale).clamp(0.0, 1.0);
        }
    }
    Ok(())
}

fn transform_variation_time(
    at: MusicalTime,
    span: MusicalTime,
    variation: &crate::model::PatternVariation,
) -> Result<MusicalTime, EngineError> {
    let reversed = if variation.reverse && at > MusicalTime::ZERO {
        span.checked_sub(at)?
    } else {
        at
    };
    wrap_pattern_time(reversed.checked_add(variation.rotate)?, span)
}

fn wrap_pattern_time(value: MusicalTime, span: MusicalTime) -> Result<MusicalTime, EngineError> {
    if value < MusicalTime::ZERO || !span.is_positive() {
        return Err(EngineError::Compile(
            "Pattern Variation wrapping requires non-negative time and positive span".to_owned(),
        ));
    }
    if value >= span {
        Ok(value.checked_sub(span)?)
    } else {
        Ok(value)
    }
}

fn variation_event_seed(
    seed: u64,
    pattern_id: &str,
    semantic_key: &[u8; 32],
    duplicate_ordinal: usize,
) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(b"crump-pattern-variation-v1\0");
    hasher.update(seed.to_le_bytes());
    hasher.update((pattern_id.len() as u64).to_le_bytes());
    hasher.update(pattern_id.as_bytes());
    hasher.update(semantic_key);
    hasher.update((duplicate_ordinal as u64).to_le_bytes());
    first_u64_le(&hasher.finalize())
}

struct CanonicalEvent<'a> {
    event: &'a PatternEvent,
    semantic_key: [u8; 32],
    duplicate_ordinal: usize,
}

fn canonical_events(events: &[PatternEvent]) -> Vec<CanonicalEvent<'_>> {
    let mut canonical = events
        .iter()
        .map(|event| CanonicalEvent {
            event,
            semantic_key: event_semantic_key(event),
            duplicate_ordinal: 0,
        })
        .collect::<Vec<_>>();
    canonical.sort_by_key(|event| event.semantic_key);
    let mut previous = None;
    let mut duplicate_ordinal = 0;
    for event in &mut canonical {
        if previous == Some(event.semantic_key) {
            duplicate_ordinal += 1;
        } else {
            previous = Some(event.semantic_key);
            duplicate_ordinal = 0;
        }
        event.duplicate_ordinal = duplicate_ordinal;
    }
    canonical
}

fn event_semantic_key(event: &PatternEvent) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"crump-pattern-event-v1\0");
    let at = event.at();
    hasher.update(at.numerator().to_le_bytes());
    hasher.update(at.denominator().to_le_bytes());
    match event {
        PatternEvent::Note {
            pitch,
            duration,
            velocity,
            ..
        } => {
            hasher.update([0, *pitch]);
            hasher.update(duration.numerator().to_le_bytes());
            hasher.update(duration.denominator().to_le_bytes());
            hasher.update(velocity.to_bits().to_le_bytes());
        }
        PatternEvent::Hit {
            voice, velocity, ..
        } => {
            hasher.update([1, *voice as u8]);
            hasher.update(velocity.to_bits().to_le_bytes());
        }
        PatternEvent::Sample {
            id,
            mapping,
            duration,
            start_frame,
            end_frame,
            gain_db,
            pan,
            fade_in_frames,
            fade_out_frames,
            velocity,
            ..
        } => {
            hasher.update([2]);
            for value in [id, mapping] {
                hasher.update((value.len() as u64).to_le_bytes());
                hasher.update(value.as_bytes());
            }
            hasher.update(duration.numerator().to_le_bytes());
            hasher.update(duration.denominator().to_le_bytes());
            hasher.update(start_frame.to_le_bytes());
            hasher.update(end_frame.to_le_bytes());
            hasher.update(gain_db.to_bits().to_le_bytes());
            hasher.update(pan.to_bits().to_le_bytes());
            hasher.update(fade_in_frames.to_le_bytes());
            hasher.update(fade_out_frames.to_le_bytes());
            hasher.update(velocity.to_bits().to_le_bytes());
        }
    }
    hasher.finalize().into()
}

fn event_instance_id(
    seed: u64,
    project_id: &str,
    track_id: &str,
    clip_id: &str,
    iteration: u64,
    semantic_key: &[u8; 32],
    duplicate_ordinal: usize,
) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(b"crump-event-instance-v1\0");
    hasher.update(seed.to_le_bytes());
    for id in [project_id, track_id, clip_id] {
        hasher.update((id.len() as u64).to_le_bytes());
        hasher.update(id.as_bytes());
    }
    hasher.update(iteration.to_le_bytes());
    hasher.update(semantic_key);
    hasher.update((duplicate_ordinal as u64).to_le_bytes());
    first_u64_le(&hasher.finalize())
}

fn first_u64_le(bytes: &[u8]) -> u64 {
    let mut prefix = [0_u8; 8];
    for (target, source) in prefix.iter_mut().zip(bytes) {
        *target = *source;
    }
    u64::from_le_bytes(prefix)
}

fn ceil_ratio(numerator: MusicalTime, denominator: MusicalTime) -> Result<u64, EngineError> {
    if !numerator.is_positive() || !denominator.is_positive() {
        return Err(EngineError::Compile(
            "loop spans must be positive".to_owned(),
        ));
    }
    let top = i128::from(numerator.numerator()) * i128::from(denominator.denominator());
    let bottom = i128::from(numerator.denominator()) * i128::from(denominator.numerator());
    let value = (top + bottom - 1) / bottom;
    u64::try_from(value)
        .map_err(|_| EngineError::Compile("loop iteration count exceeds bounds".to_owned()))
}

fn stable_u64(seed: u64, bytes: &[u8]) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(b"crump-event-v1\0");
    hasher.update(seed.to_le_bytes());
    hasher.update(bytes);
    first_u64_le(&hasher.finalize())
}

fn groove_event_is_included(groove: &GrooveTemplate, event_seed: u64) -> bool {
    unit_random(stable_u64(event_seed ^ groove.seed, b"groove-probability")) < groove.probability
}

fn groove_velocity(base: f64, groove: Option<&GrooveTemplate>, event_seed: u64) -> f32 {
    let Some(groove) = groove else {
        return base as f32;
    };
    let centered =
        unit_random(stable_u64(event_seed ^ groove.seed, b"groove-velocity")) * 2.0 - 1.0;
    (base + centered * groove.velocity).clamp(0.0, 1.0) as f32
}

fn apply_groove_timing(
    relative: MusicalTime,
    source_position: MusicalTime,
    groove: Option<&GrooveTemplate>,
    event_seed: u64,
) -> Result<MusicalTime, EngineError> {
    let Some(groove) = groove else {
        return Ok(relative);
    };
    let mut transformed = relative;
    if floor_ratio(source_position, groove.grid)? % 2 == 1 {
        let swing_parts = ((groove.swing.mul_add(2.0, -1.0) * 1_000_000.0).round()) as i64;
        transformed = transformed.checked_add(scale_time(groove.grid, swing_parts, 1_000_000)?)?;
    }
    if groove.timing > MusicalTime::ZERO {
        let random = stable_u64(event_seed ^ groove.seed, b"groove-timing");
        let centered = i64::try_from(random % 131_071)
            .map_err(|_| EngineError::Compile("Groove random value overflows".to_owned()))?
            - 65_535;
        transformed = transformed.checked_add(scale_time(groove.timing, centered, 65_535)?)?;
    }
    Ok(transformed)
}

fn floor_ratio(numerator: MusicalTime, denominator: MusicalTime) -> Result<u64, EngineError> {
    if numerator < MusicalTime::ZERO || !denominator.is_positive() {
        return Err(EngineError::Compile(
            "Groove positions require non-negative time and positive grid".to_owned(),
        ));
    }
    let top = i128::from(numerator.numerator()) * i128::from(denominator.denominator());
    let bottom = i128::from(numerator.denominator()) * i128::from(denominator.numerator());
    u64::try_from(top / bottom)
        .map_err(|_| EngineError::Compile("Groove subdivision index overflows".to_owned()))
}

fn scale_time(
    value: MusicalTime,
    numerator: i64,
    denominator: i64,
) -> Result<MusicalTime, EngineError> {
    let numerator = value
        .numerator()
        .checked_mul(numerator)
        .ok_or_else(|| EngineError::Compile("Groove timing scale overflows".to_owned()))?;
    let denominator = value
        .denominator()
        .checked_mul(denominator)
        .ok_or_else(|| EngineError::Compile("Groove timing scale overflows".to_owned()))?;
    MusicalTime::new(numerator, denominator).map_err(EngineError::from)
}

fn unit_random(value: u64) -> f64 {
    let mantissa = value >> 11;
    mantissa as f64 / (1_u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::dsl::parse_project;

    use super::*;

    fn project(source: &str) -> Project {
        let document = parse_project(Path::new("project.crump"), source);
        assert!(
            !document.diagnostics.has_errors(),
            "{:?}",
            document.diagnostics.as_slice()
        );
        document.project.expect("Project")
    }

    fn event_signature(event: &ScheduledEvent) -> (u64, u8, u64, u8, u32) {
        match event.kind {
            RuntimeEvent::NoteOn {
                note_id,
                pitch,
                velocity,
                ..
            } => (event.frame, 1, note_id, pitch, velocity.to_bits()),
            RuntimeEvent::NoteOff { note_id } => (event.frame, 0, note_id, 0, 0),
            RuntimeEvent::Hit {
                voice,
                velocity,
                seed,
                ..
            } => (event.frame, 2, seed, voice as u8, velocity.to_bits()),
            RuntimeEvent::SampleOn {
                note_id, velocity, ..
            } => (event.frame, 3, note_id, 0, velocity.to_bits()),
            RuntimeEvent::SampleOff { note_id } => (event.frame, 4, note_id, 0, 0),
        }
    }

    #[test]
    fn loop_expansion_is_half_open_and_deterministic() {
        let source = r#"crump 1
project "Schedule" {
  id schedule
  seed 7
  sample_rate 48000
  render_tail 0s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument drums "Drums" stock_drums {
      polyphony 8
      parameter tone "Tone" ratio 0 1 0.5 0.5
      faceplate {
        control tone "Tone" "Brightness" maps tone
      }
    }
    pattern beat 1beat {
      hit 0beat kick 1
    }
    track drums "Drums" instrument drums level 0db pan 0 {
      clip four pattern beat start 0bar duration 1bar mode loop
    }
  }
}
"#;
        let document = parse_project(Path::new("project.crump"), source);
        let project = document.project.unwrap();
        let first = compile_schedules(&project).unwrap();
        let second = compile_schedules(&project).unwrap();
        assert_eq!(first[0].events.len(), 4);
        assert_eq!(first[0].events[3].frame, 72_000);
        assert_eq!(first[0].events[0].order, second[0].events[0].order);
    }

    #[test]
    fn reusable_groove_applies_exact_swing_and_seeded_humanization_probability() {
        let source = r#"crump 1
project "Groove" {
  id groove
  seed 19
  sample_rate 48000
  render_tail 0s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument drums "Drums" stock_drums {
      polyphony 8
      parameter tone "Tone" ratio 0 1 0.5 0.5
      faceplate {
        control tone "Tone" "Brightness" maps tone
      }
    }
    groove shuffle grid 1/2beat swing 0.625 timing 0beat velocity 0 probability 1 seed 41
    pattern beat 2beat {
      hit 0beat kick 0.7
      hit 1/2beat hat 0.7
      hit 1beat snare 0.7
      hit 3/2beat hat 0.7
    }
    track drums "Drums" instrument drums level 0db pan 0 {
      clip grooved pattern beat start 0bar duration 2beat mode once {
        groove shuffle
      }
    }
  }
}
"#;
        let straight = compile_schedules(&project(source)).expect("swing schedule");
        let frames = straight[0]
            .events
            .iter()
            .map(|event| event.frame)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![0, 15_000, 24_000, 39_000]);

        let humanized_source = source
            .replace("timing 0beat", "timing 1/32beat")
            .replace("velocity 0 probability", "velocity 0.1 probability");
        let first = compile_schedules(&project(&humanized_source)).expect("humanized schedule");
        let second = compile_schedules(&project(&humanized_source)).expect("repeat schedule");
        let first_signatures = first[0]
            .events
            .iter()
            .map(event_signature)
            .collect::<Vec<_>>();
        let second_signatures = second[0]
            .events
            .iter()
            .map(event_signature)
            .collect::<Vec<_>>();
        assert_eq!(first_signatures, second_signatures);
        assert_eq!(first_signatures.len(), 4);
        assert_ne!(
            first_signatures,
            straight[0]
                .events
                .iter()
                .map(event_signature)
                .collect::<Vec<_>>()
        );

        let boundary = source.replace(
            r#"      hit 0beat kick 0.7
      hit 1/2beat hat 0.7
      hit 1beat snare 0.7
      hit 3/2beat hat 0.7"#,
            "      hit 15/8beat hat 0.7",
        );
        let boundary = compile_schedules(&project(&boundary)).expect("boundary schedule");
        assert_eq!(boundary[0].events.len(), 1);
        assert_eq!(boundary[0].events[0].frame, 45_000);

        let excluded = source.replace("probability 1", "probability 0");
        let excluded = compile_schedules(&project(&excluded)).expect("probability schedule");
        assert!(excluded[0].events.is_empty());
    }

    #[test]
    fn parameter_lock_limit_short_circuits_high_iteration_loops() {
        let source = include_str!("../../tests/fixtures/minimal.crump")
            .replace(
                "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz",
                "parameter tone \"Tone\" hz 100hz 10000hz 3000hz 3200hz automatable",
            )
            .replace(
                "    pattern melody 1bar {\n      note 0beat C4 1beat 0.8\n      note 1beat E4 1beat 0.75\n      note 2beat G4 1beat 0.8\n      note 3beat E4 1beat 0.75\n    }",
                "    pattern melody 1/100000000beat {\n      lock tiny-loop at 0beat parameter tone value 1000hz\n    }",
            )
            .replace(
                "clip melody pattern melody start 0bar duration 1bar mode once",
                "clip melody pattern melody start 0bar duration 1bar mode loop",
            );
        let project = project(&source);
        let error = compile_track_parameter_lock_lanes(&project)
            .expect_err("the bounded compiler must reject before traversing the full loop");
        assert!(error
            .to_string()
            .contains("8192-point Parameter lock maximum"));
    }

    #[test]
    fn pattern_variations_materialize_deterministic_transforms_off_callback() {
        let source = r#"crump 1
project "Variation" {
  id variation
  seed 19
  sample_rate 48000
  render_tail 0s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument lead "Lead" graph {
      polyphony 4
      parameter gain "Gain" ratio 0 1 0.5 0.5
      faceplate {
        control gain "Gain" "Gain" maps gain
      }
      graph 4 {
        node osc oscillator waveform sine gain 0.2
        node out output
        connect osc.out -> out.in
      }
    }
    pattern phrase 1bar {
      lock low at 0beat parameter gain value 0.2
      lock high at 2beat parameter gain value 0.8
      note 0beat C4 1/2beat 1
      note 1beat D4 1/2beat 1
      note 2beat E4 1/2beat 1
      note 3beat F4 1/2beat 1
    }
    pattern answer 1bar from phrase {
      rotate 1beat
      transpose 12st
      velocity 0.5
      reverse true
      probability 1
      seed 99
    }
    pattern echo 1bar from answer {
      rotate 1beat
      transpose -12st
      velocity 0.5
      reverse false
      probability 1
      seed 100
    }
    track lead "Lead" instrument lead level 0db pan 0 {
      clip answer pattern echo start 0bar duration 1bar mode once
    }
  }
}
"#;
        let parsed = project(source);
        let variation = parsed
            .song
            .patterns
            .iter()
            .find(|pattern| pattern.id == "answer")
            .expect("authored Variation");
        assert!(variation.events.is_empty());
        assert_eq!(
            variation
                .variation
                .as_ref()
                .expect("procedural lineage")
                .source,
            "phrase"
        );
        assert_eq!(
            parsed
                .song
                .patterns
                .iter()
                .find(|pattern| pattern.id == "echo")
                .and_then(|pattern| pattern.variation.as_ref())
                .expect("chained procedural lineage")
                .source,
            "answer"
        );

        let first = compile_schedules(&parsed).expect("Variation schedule");
        let second = compile_schedules(&parsed).expect("repeat Variation schedule");
        let note_ons = first[0]
            .events
            .iter()
            .filter_map(|event| match event.kind {
                RuntimeEvent::NoteOn {
                    pitch, velocity, ..
                } => Some((event.frame, pitch, velocity.to_bits())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            note_ons,
            vec![
                (0, 64, 0.25_f32.to_bits()),
                (24_000, 62, 0.25_f32.to_bits()),
                (48_000, 60, 0.25_f32.to_bits()),
                (72_000, 65, 0.25_f32.to_bits()),
            ]
        );
        assert_eq!(
            first[0]
                .events
                .iter()
                .map(event_signature)
                .collect::<Vec<_>>(),
            second[0]
                .events
                .iter()
                .map(event_signature)
                .collect::<Vec<_>>()
        );
        let lock_target = ParameterTarget::Instrument {
            instrument: "lead".to_owned(),
            parameter: "gain".to_owned(),
        };
        let lock_lanes = compile_parameter_lock_lanes(&parsed).expect("Variation lock lanes");
        assert_eq!(
            lock_lanes
                .get(&lock_target)
                .expect("inherited lock lane")
                .iter()
                .map(|point| (point.frame, point.value.as_deref()))
                .collect::<Vec<_>>(),
            vec![(0, Some("0.8")), (48_000, Some("0.2")), (96_000, None),]
        );

        let excluded = source.replace("probability 1", "probability 0");
        let excluded = compile_schedules(&project(&excluded)).expect("excluded Variation");
        assert!(excluded[0].events.is_empty());

        let probabilistic = source.replace("probability 1", "probability 0.5");
        let reordered = probabilistic.replace(
            r#"      note 0beat C4 1/2beat 1
      note 1beat D4 1/2beat 1
      note 2beat E4 1/2beat 1
      note 3beat F4 1/2beat 1"#,
            r#"      note 3beat F4 1/2beat 1
      note 1beat D4 1/2beat 1
      note 0beat C4 1/2beat 1
      note 2beat E4 1/2beat 1"#,
        );
        let probabilistic =
            compile_schedules(&project(&probabilistic)).expect("probabilistic Variation schedule");
        let reordered =
            compile_schedules(&project(&reordered)).expect("reordered Variation schedule");
        assert_eq!(
            probabilistic[0]
                .events
                .iter()
                .map(event_signature)
                .collect::<Vec<_>>(),
            reordered[0]
                .events
                .iter()
                .map(event_signature)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn source_offset_transpose_gain_and_fades_compile_per_clip() {
        let source = r#"crump 1
project "Clip transforms" {
  id clip-transforms
  seed 11
  sample_rate 48000
  render_tail 0s
  song {
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument lead "Lead" graph {
      polyphony 2
      parameter gain "Gain" ratio 0 1 0.5 0.5
      faceplate {
        control gain "Gain" "Gain" maps gain
      }
      graph 4 {
        node osc oscillator waveform sine gain 0.2
        node out output
        connect osc.out -> out.in
      }
    }
    pattern phrase 1bar {
      note 0beat C4 1beat 1
      note 1beat D4 1beat 0.8
    }
    track lead "Lead" instrument lead level 0db pan 0 {
      clip shifted pattern phrase start 0bar duration 1bar mode loop {
        offset 1beat
        transpose 12st
        gain -6db
        fade 1beat 1beat
      }
    }
  }
}
"#;
        let project = project(source);
        let schedule = compile_schedules(&project).expect("compiled transformed Clip");
        let note_ons = schedule[0]
            .events
            .iter()
            .filter_map(|event| match event.kind {
                RuntimeEvent::NoteOn {
                    pitch,
                    clip_envelope,
                    ..
                } => Some((event.frame, pitch, clip_envelope)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(note_ons.len(), 2);
        assert_eq!((note_ons[0].0, note_ons[0].1), (0, 74));
        assert_eq!((note_ons[1].0, note_ons[1].1), (72_000, 72));
        let first = note_ons[0].2;
        assert_eq!(first.position_frames, 0);
        assert_eq!(first.duration_frames, 96_000);
        assert_eq!(first.fade_in_frames, 24_000);
        assert_eq!(first.fade_out_frames, 24_000);
        assert!((first.gain - 10.0_f32.powf(-6.0 / 20.0)).abs() < 1.0e-6);
        assert!(first.gain_at(0) < first.gain_at(23_999));
        assert!((first.gain_at(23_999) - first.gain).abs() < 1.0e-6);
        assert!(note_ons[1].2.gain_at(23_999) < note_ons[1].2.gain_at(0));
    }

    #[test]
    fn authored_event_reordering_does_not_change_schedule_identity() {
        let source = include_str!("../../tests/fixtures/minimal.crump");
        let first = source.replace(
            "      note 0beat C4 1beat 0.8\n      note 1beat E4 1beat 0.75",
            "      note 0beat C4 1beat 0.8\n      note 0beat E4 1beat 0.75",
        );
        let second = source.replace(
            "      note 0beat C4 1beat 0.8\n      note 1beat E4 1beat 0.75",
            "      note 0beat E4 1beat 0.75\n      note 0beat C4 1beat 0.8",
        );
        let first = compile_schedules(&project(&first)).expect("first schedule");
        let second = compile_schedules(&project(&second)).expect("second schedule");
        let first = first
            .iter()
            .find(|schedule| schedule.track_id == "lead")
            .expect("lead schedule")
            .events
            .iter()
            .map(event_signature)
            .collect::<Vec<_>>();
        let second = second
            .iter()
            .find(|schedule| schedule.track_id == "lead")
            .expect("lead schedule")
            .events
            .iter()
            .map(event_signature)
            .collect::<Vec<_>>();
        assert_eq!(first, second);
    }

    #[test]
    fn same_frame_releases_sort_before_attacks() {
        let schedules =
            compile_schedules(&project(include_str!("../../tests/fixtures/minimal.crump")))
                .expect("schedule");
        let lead = schedules
            .iter()
            .find(|schedule| schedule.track_id == "lead")
            .expect("lead schedule");
        let at_one_beat = lead
            .events
            .iter()
            .filter(|event| event.frame == 24_000)
            .collect::<Vec<_>>();
        assert!(matches!(at_one_beat[0].kind, RuntimeEvent::NoteOff { .. }));
        assert!(matches!(at_one_beat[1].kind, RuntimeEvent::NoteOn { .. }));
    }

    #[test]
    fn positive_notes_receive_at_least_one_sample() {
        let source = include_str!("../../tests/fixtures/minimal.crump").replacen(
            "note 0beat C4 1beat 0.8",
            "note 0beat C4 1/100000beat 0.8",
            1,
        );
        let schedules = compile_schedules(&project(&source)).expect("schedule");
        let lead = schedules
            .iter()
            .find(|schedule| schedule.track_id == "lead")
            .expect("lead schedule");
        let note_id = lead
            .events
            .iter()
            .find_map(|event| match event.kind {
                RuntimeEvent::NoteOn {
                    note_id, pitch: 60, ..
                } => Some(note_id),
                _ => None,
            })
            .expect("C4 Note On");
        let off = lead
            .events
            .iter()
            .find(|event| {
                matches!(event.kind, RuntimeEvent::NoteOff { note_id: id } if id == note_id)
            })
            .expect("C4 Note Off");
        assert_eq!(off.frame, 1);
    }
}
