//! Deterministic expansion of finite Patterns and Clips into runtime Events.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::{
    model::{ClipMode, PatternEvent, Project},
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
    pub(crate) level_db: f64,
    pub(crate) pan: f64,
    pub(crate) events: Vec<ScheduledEvent>,
}

/// One exact runtime Event.
#[derive(Debug, Clone)]
pub(crate) struct ScheduledEvent {
    pub(crate) frame: u64,
    pub(crate) order: EventOrder,
    pub(crate) kind: RuntimeEvent,
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
    pitch_or_voice: u8,
}

/// Runtime-consumable concrete Event.
#[derive(Debug, Clone)]
pub(crate) enum RuntimeEvent {
    NoteOn {
        note_id: u64,
        pitch: u8,
        velocity: f32,
        seed: u64,
    },
    NoteOff {
        note_id: u64,
    },
    Hit {
        voice: crate::model::DrumVoice,
        velocity: f32,
        seed: u64,
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
    let patterns: BTreeMap<_, _> = project
        .song
        .patterns
        .iter()
        .map(|pattern| (pattern.id.as_str(), pattern))
        .collect();
    let mut schedules = Vec::with_capacity(project.song.tracks.len());
    for track in &project.song.tracks {
        let mut events = Vec::new();
        for clip in &track.clips {
            let pattern = patterns.get(clip.pattern.as_str()).ok_or_else(|| {
                EngineError::Compile(format!("Clip `{}` references missing Pattern", clip.id))
            })?;
            let iterations = match clip.mode {
                ClipMode::Once => 1,
                ClipMode::Loop => ceil_ratio(clip.duration, pattern.span)?,
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
            for iteration in 0..iterations {
                let offset = pattern.span.checked_mul(iteration)?;
                for canonical in canonical_events(&pattern.events) {
                    let event = canonical.event;
                    let relative = offset.checked_add(event.at())?;
                    if relative >= clip.duration {
                        continue;
                    }
                    let onset = clip.start.checked_add(relative)?;
                    if onset >= clip_end || onset >= project.song.end {
                        continue;
                    }
                    let frame = onset.to_frames(project.song.tempo_bpm, project.sample_rate)?;
                    let stable = event_instance_id(
                        project.seed,
                        &project.id,
                        &track.id,
                        &clip.id,
                        iteration,
                        &canonical.semantic_key,
                        canonical.duplicate_ordinal,
                    );
                    let order = |phase, pitch_or_voice| EventOrder {
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
                            let authored_off = onset.checked_add(*duration)?;
                            let off = authored_off.min(clip_end).min(project.song.end);
                            let mut off_frame =
                                off.to_frames(project.song.tempo_bpm, project.sample_rate)?;
                            let boundary_frame = clip_end
                                .min(project.song.end)
                                .to_frames(project.song.tempo_bpm, project.sample_rate)?;
                            if off_frame <= frame && boundary_frame > frame {
                                off_frame = frame + 1;
                            }
                            if off_frame <= frame {
                                continue;
                            }
                            let note_id = stable;
                            events.push(ScheduledEvent {
                                frame,
                                order: order(1, *pitch),
                                kind: RuntimeEvent::NoteOn {
                                    note_id,
                                    pitch: *pitch,
                                    velocity: *velocity as f32,
                                    seed: stable_u64(stable, b"voice"),
                                },
                            });
                            events.push(ScheduledEvent {
                                frame: off_frame,
                                order: order(0, *pitch),
                                kind: RuntimeEvent::NoteOff { note_id },
                            });
                        }
                        PatternEvent::Hit {
                            voice, velocity, ..
                        } => {
                            events.push(ScheduledEvent {
                                frame,
                                order: order(1, *voice as u8),
                                kind: RuntimeEvent::Hit {
                                    voice: *voice,
                                    velocity: *velocity as f32,
                                    seed: stable_u64(stable, b"drum"),
                                },
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
            level_db: track.level_db,
            pan: track.pan,
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::dsl::parse_project;

    use super::*;

    fn project(source: &str) -> Project {
        let document = parse_project(Path::new("project.crump"), source);
        assert!(!document.diagnostics.has_errors());
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
            } => (event.frame, 2, seed, voice as u8, velocity.to_bits()),
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
