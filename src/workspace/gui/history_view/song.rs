//! Song modules of the History time machine: tempo & meter, patterns (piano roll / drum grid +
//! velocity lane), a track's clips on a mini timeline, automation curve lanes with previous curves
//! ghosted, song-level modulations, envelope followers, sidechains, locators, grooves and song end.

use std::collections::HashSet;

use super::*;
use crate::model::{
    AutomationInterpolation, AutomationLane, AutomationPoint, ChannelRef, Clip, ClipMode,
    ControlPolarity, DrumVoice, EnvelopeDetector, EnvelopeFollowerRoute, GrooveTemplate, Locator,
    ModulationRate, ModulationRoute, ModulationScope, ParameterLock, ParameterTarget, Pattern,
    PatternEvent, PatternVariation, SidechainRoute, Song, TapPoint, Track, TrackParameter, Unit,
};

/// Draws `area` (Tempo, a Pattern, a track's Arrangement, or any Other Song-level collection
/// such as automations, modulations, locators, grooves, envelope followers, sidechains, meter or
/// song end) as it is in `view.shown`. `changes` are the measured changes in that area.
pub(super) fn draw(ui: &mut Ui, view: &SoundView<'_>, area: &Area, changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);

    match area {
        Area::Tempo => draw_tempo_and_meter(ui, view, song, prev_song, other_song),
        Area::Pattern(id) => draw_pattern_section(ui, view, id, changes),
        Area::Arrangement(id) => draw_arrangement_section(ui, view, id, changes),
        Area::Other(collection) => match collection.as_str() {
            "automations" => draw_automations_section(ui, view, changes),
            "modulations" => draw_song_modulations_section(ui, view, changes),
            "envelope_followers" => draw_envelope_followers_section(ui, view, changes),
            "sidechains" => draw_sidechains_section(ui, view, changes),
            "locators" => draw_locators_section(ui, view, changes),
            "grooves" => draw_grooves_section(ui, view, changes),
            "meter" => draw_tempo_and_meter(ui, view, song, prev_song, other_song),
            "end" => draw_song_end_section(ui, view, song, prev_song, other_song),
            _ => {
                let owner = changes.first().map_or("Project", |c| c.owner.as_str());
                section(ui, Kind::Other, owner, |ui| change_chips(ui, changes));
            }
        },
        _ => {
            let owner = changes.first().map_or("Project", |c| c.owner.as_str());
            section(ui, Kind::Other, owner, |ui| change_chips(ui, changes));
        }
    }
}

// ── Tempo & Meter ────────────────────────────────────────────────────────────

fn draw_tempo_and_meter(
    ui: &mut Ui,
    view: &SoundView<'_>,
    song: &Song,
    prev_song: Option<&Song>,
    other_song: Option<&Song>,
) {
    section(ui, Kind::Song, "Song", |ui| {
        let min_bpm = song
            .tempo_bpm
            .min(prev_song.map_or(60.0, |s| s.tempo_bpm))
            .min(other_song.map_or(60.0, |s| s.tempo_bpm))
            .min(60.0);
        let max_bpm = song
            .tempo_bpm
            .max(prev_song.map_or(180.0, |s| s.tempo_bpm))
            .max(other_song.map_or(180.0, |s| s.tempo_bpm))
            .max(180.0);

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(14.0, 8.0);
            dial(
                ui,
                "Tempo",
                song.tempo_bpm,
                min_bpm,
                max_bpm,
                Unit::Ratio,
                &|bpm| format!("{bpm:.1} BPM"),
                mark(
                    song.tempo_bpm,
                    prev_song.map(|s| s.tempo_bpm),
                    other_song.map(|s| s.tempo_bpm),
                ),
                view.other_word,
                theme::TEXT_PRIMARY,
            );
            draw_meter_tile(
                ui,
                song.meter,
                prev_song.map(|s| s.meter),
                other_song.map(|s| s.meter),
                view.other_word,
            );
            draw_length_tile(ui, song, prev_song, other_song, view.other_word);
        });
    });
}

fn draw_meter_tile(
    ui: &mut Ui,
    meter: crate::time::Meter,
    prev: Option<crate::time::Meter>,
    other: Option<crate::time::Meter>,
    other_word: &str,
) {
    let edited = prev.is_some_and(|p| p != meter);
    let differs = !edited && other.is_some_and(|o| o != meter);
    ui.vertical(|ui| {
        ui.set_width(94.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(94.0, 72.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 6.0, theme::BG_CARD);
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(
                if edited { 1.5_f32 } else { 1.0_f32 },
                if edited {
                    theme::SELECTION
                } else if differs {
                    theme::BORDER_STRONG
                } else {
                    theme::BORDER_SUBTLE
                },
            ),
            StrokeKind::Inside,
        );
        painter.text(
            Pos2::new(rect.center().x, rect.center().y - 12.0),
            Align2::CENTER_CENTER,
            format!("{}", meter.numerator),
            FontId::monospace(18.0),
            theme::TEXT_PRIMARY,
        );
        painter.hline(
            (rect.center().x - 16.0)..=(rect.center().x + 16.0),
            rect.center().y,
            Stroke::new(1.2_f32, theme::BORDER_STRONG),
        );
        painter.text(
            Pos2::new(rect.center().x, rect.center().y + 13.0),
            Align2::CENTER_CENTER,
            format!("{}", meter.denominator),
            FontId::monospace(18.0),
            theme::TEXT_PRIMARY,
        );
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("METER")
                    .size(9.5)
                    .color(theme::TEXT_SECONDARY),
            );
            ui.label(
                RichText::new(format!("{meter}"))
                    .monospace()
                    .size(11.5)
                    .color(theme::TEXT_PRIMARY),
            );
            let note = if let Some(p) = prev
                && edited
            {
                format!("previous {p}")
            } else if let Some(o) = other
                && differs
            {
                format!("{other_word} {o}")
            } else {
                String::new()
            };
            if note.is_empty() {
                ui.add_space(ui.text_style_height(&egui::TextStyle::Body));
            } else {
                ui.label(RichText::new(note).size(10.0).color(theme::TEXT_MUTED));
            }
        });
    });
}

fn draw_length_tile(
    ui: &mut Ui,
    song: &Song,
    prev_song: Option<&Song>,
    other_song: Option<&Song>,
    other_word: &str,
) {
    let bars_of = |s: &Song| {
        let bl = s.meter.bar_length().map_or(4.0, |t| t.as_f64());
        s.end.as_f64() / bl.max(1e-6)
    };
    let bars = bars_of(song);
    let prev_bars = prev_song.map(bars_of);
    let other_bars = other_song.map(bars_of);
    let edited = prev_bars.is_some_and(|p| (p - bars).abs() > 1e-4);
    let differs = !edited && other_bars.is_some_and(|o| (o - bars).abs() > 1e-4);

    ui.vertical(|ui| {
        ui.set_width(94.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(94.0, 72.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 6.0, theme::BG_CARD);
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(
                if edited { 1.5_f32 } else { 1.0_f32 },
                if edited {
                    theme::SELECTION
                } else if differs {
                    theme::BORDER_STRONG
                } else {
                    theme::BORDER_SUBTLE
                },
            ),
            StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            format!("{bars:.0}"),
            FontId::monospace(18.0),
            theme::TEXT_PRIMARY,
        );
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("LENGTH")
                    .size(9.5)
                    .color(theme::TEXT_SECONDARY),
            );
            ui.label(
                RichText::new(format!("{bars:.0} bars"))
                    .monospace()
                    .size(11.5)
                    .color(theme::TEXT_PRIMARY),
            );
            let note = if let Some(p) = prev_bars
                && edited
            {
                format!("previous {p:.0} bars")
            } else if let Some(o) = other_bars
                && differs
            {
                format!("{other_word} {o:.0} bars")
            } else {
                String::new()
            };
            if note.is_empty() {
                ui.add_space(ui.text_style_height(&egui::TextStyle::Body));
            } else {
                ui.label(RichText::new(note).size(10.0).color(theme::TEXT_MUTED));
            }
        });
    });
}

// ── Pattern: Piano Roll / Drum Grid + Velocity Lane ──────────────────────────

#[derive(Clone, Debug)]
struct MatchedEvent<'a> {
    shown: &'a PatternEvent,
    prev: Option<&'a PatternEvent>,
    other: Option<&'a PatternEvent>,
    added: bool,
}

#[derive(Clone, Debug)]
struct PatternEventDiff<'a> {
    entries: Vec<MatchedEvent<'a>>,
    removed: Vec<&'a PatternEvent>,
}

fn event_velocity(ev: &PatternEvent) -> f64 {
    match ev {
        PatternEvent::Note { velocity, .. }
        | PatternEvent::Hit { velocity, .. }
        | PatternEvent::Sample { velocity, .. } => *velocity,
    }
}

fn event_duration(ev: &PatternEvent) -> f64 {
    match ev {
        PatternEvent::Note { duration, .. } | PatternEvent::Sample { duration, .. } => {
            duration.as_f64()
        }
        PatternEvent::Hit { .. } => 0.2,
    }
}

fn event_same_kind(a: &PatternEvent, b: &PatternEvent) -> bool {
    matches!(
        (a, b),
        (PatternEvent::Note { .. }, PatternEvent::Note { .. })
            | (PatternEvent::Hit { .. }, PatternEvent::Hit { .. })
            | (PatternEvent::Sample { .. }, PatternEvent::Sample { .. })
    )
}

fn event_exact_eq(a: &PatternEvent, b: &PatternEvent) -> bool {
    match (a, b) {
        (
            PatternEvent::Note {
                at: a1,
                pitch: p1,
                duration: d1,
                velocity: v1,
                ..
            },
            PatternEvent::Note {
                at: a2,
                pitch: p2,
                duration: d2,
                velocity: v2,
                ..
            },
        ) => a1 == a2 && p1 == p2 && d1 == d2 && (v1 - v2).abs() < 1e-6,
        (
            PatternEvent::Hit {
                at: a1,
                voice: vo1,
                velocity: v1,
                ..
            },
            PatternEvent::Hit {
                at: a2,
                voice: vo2,
                velocity: v2,
                ..
            },
        ) => a1 == a2 && vo1 == vo2 && (v1 - v2).abs() < 1e-6,
        (
            PatternEvent::Sample {
                id: id1,
                at: a1,
                mapping: m1,
                duration: d1,
                velocity: v1,
                ..
            },
            PatternEvent::Sample {
                id: id2,
                at: a2,
                mapping: m2,
                duration: d2,
                velocity: v2,
                ..
            },
        ) => id1 == id2 && a1 == a2 && m1 == m2 && d1 == d2 && (v1 - v2).abs() < 1e-6,
        _ => false,
    }
}

fn event_position_changed(a: &PatternEvent, b: &PatternEvent) -> bool {
    match (a, b) {
        (
            PatternEvent::Note {
                at: a1,
                pitch: p1,
                duration: d1,
                ..
            },
            PatternEvent::Note {
                at: a2,
                pitch: p2,
                duration: d2,
                ..
            },
        ) => a1 != a2 || p1 != p2 || d1 != d2,
        (
            PatternEvent::Hit {
                at: a1, voice: vo1, ..
            },
            PatternEvent::Hit {
                at: a2, voice: vo2, ..
            },
        ) => a1 != a2 || vo1 != vo2,
        (
            PatternEvent::Sample {
                at: a1,
                mapping: m1,
                duration: d1,
                ..
            },
            PatternEvent::Sample {
                at: a2,
                mapping: m2,
                duration: d2,
                ..
            },
        ) => a1 != a2 || m1 != m2 || d1 != d2,
        _ => true,
    }
}

fn event_distance(a: &PatternEvent, b: &PatternEvent) -> f64 {
    if !event_same_kind(a, b) {
        return f64::INFINITY;
    }
    let dt = (a.at().as_f64() - b.at().as_f64()).abs() * 4.0;
    let dv = (event_velocity(a) - event_velocity(b)).abs();
    match (a, b) {
        (PatternEvent::Note { pitch: p1, .. }, PatternEvent::Note { pitch: p2, .. }) => {
            dt + f64::from((*p1 as i32 - *p2 as i32).abs()) * 0.25 + dv
        }
        (PatternEvent::Hit { voice: v1, .. }, PatternEvent::Hit { voice: v2, .. }) => {
            dt + if v1 == v2 { 0.0 } else { 1.5 } + dv
        }
        (PatternEvent::Sample { id: i1, .. }, PatternEvent::Sample { id: i2, .. }) => {
            if i1 == i2 {
                0.0
            } else {
                dt + 10.0 + dv
            }
        }
        _ => f64::INFINITY,
    }
}

/// Pairs events in `shown` with their counterparts in `prev` (and `other`).
/// When lengths and kinds match (standard move / voice / pitch / velocity edits), pairs
/// positionally to stay 1:1 with `mod.rs`'s diff; otherwise matches exact events first and
/// nearest remaining events second, reporting unmatched `prev` events as removed.
fn diff_pattern_events<'a>(
    shown: &'a [PatternEvent],
    prev: Option<&'a [PatternEvent]>,
    other: Option<&'a [PatternEvent]>,
) -> PatternEventDiff<'a> {
    let pair_against = |target: &'a [PatternEvent]| -> (Vec<Option<&'a PatternEvent>>, Vec<bool>) {
        let mut matched: Vec<Option<&'a PatternEvent>> = vec![None; shown.len()];
        let mut used = vec![false; target.len()];
        if shown.len() == target.len()
            && shown
                .iter()
                .zip(target.iter())
                .all(|(a, b)| event_same_kind(a, b))
        {
            for i in 0..shown.len() {
                matched[i] = Some(&target[i]);
                used[i] = true;
            }
            return (matched, used);
        }
        // Pass 1: exact matches (or same Sample ID).
        for (si, se) in shown.iter().enumerate() {
            if let Some((ti, te)) = target.iter().enumerate().find(|(ti, te)| {
                !used[*ti]
                    && (event_exact_eq(se, te)
                        || matches!(
                            (se, *te),
                            (PatternEvent::Sample { id: a, .. }, PatternEvent::Sample { id: b, .. })
                                if a == b
                        ))
            }) {
                matched[si] = Some(te);
                used[ti] = true;
            }
        }
        // Pass 2: nearest remaining event of the same kind.
        for (si, se) in shown.iter().enumerate() {
            if matched[si].is_some() {
                continue;
            }
            if let Some((ti, te)) = target
                .iter()
                .enumerate()
                .filter(|(ti, te)| !used[*ti] && event_same_kind(se, te))
                .min_by(|(_, a), (_, b)| event_distance(se, a).total_cmp(&event_distance(se, b)))
            {
                matched[si] = Some(te);
                used[ti] = true;
            }
        }
        (matched, used)
    };

    let (prev_matches, prev_used) =
        prev.map_or_else(|| (vec![None; shown.len()], Vec::new()), pair_against);
    let (other_matches, _) =
        other.map_or_else(|| (vec![None; shown.len()], Vec::new()), pair_against);

    let entries = shown
        .iter()
        .enumerate()
        .map(|(i, ev)| MatchedEvent {
            shown: ev,
            prev: prev_matches[i],
            other: other_matches[i],
            added: prev.is_some() && prev_matches[i].is_none(),
        })
        .collect();

    let removed = prev
        .map(|p| {
            p.iter()
                .enumerate()
                .filter(|(i, _)| !prev_used.get(*i).copied().unwrap_or(false))
                .map(|(_, e)| e)
                .collect()
        })
        .unwrap_or_default();

    PatternEventDiff { entries, removed }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RollLane {
    Pitch(u8),
    Drum(DrumVoice),
    Sample(String),
}

impl RollLane {
    fn matches(&self, ev: &PatternEvent) -> bool {
        match (self, ev) {
            (Self::Pitch(p), PatternEvent::Note { pitch, .. }) => p == pitch,
            (Self::Drum(v), PatternEvent::Hit { voice, .. }) => v == voice,
            (Self::Sample(m), PatternEvent::Sample { mapping, .. }) => m == mapping,
            _ => false,
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Pitch(p) => crate::model::format_pitch(*p),
            Self::Drum(v) => drum_voice_name(*v).to_owned(),
            Self::Sample(m) => m.clone(),
        }
    }

    fn is_black_key(&self) -> bool {
        matches!(self, Self::Pitch(p) if matches!(p % 12, 1 | 3 | 6 | 8 | 10))
    }
}

fn drum_voice_name(voice: DrumVoice) -> &'static str {
    match voice {
        DrumVoice::Kick => "KICK",
        DrumVoice::Snare => "SNARE",
        DrumVoice::Hat => "HAT",
        DrumVoice::Clap => "CLAP",
    }
}

fn build_lanes<'a>(
    events: impl Iterator<Item = &'a PatternEvent> + Clone,
) -> (Vec<RollLane>, bool) {
    let has_notes = events
        .clone()
        .any(|e| matches!(e, PatternEvent::Note { .. }));
    let has_hits = events
        .clone()
        .any(|e| matches!(e, PatternEvent::Hit { .. }));

    if has_notes {
        let mut lo = 127u8;
        let mut hi = 0u8;
        for ev in events {
            if let PatternEvent::Note { pitch, .. } = ev {
                lo = lo.min(*pitch);
                hi = hi.max(*pitch);
            }
        }
        lo = lo.saturating_sub(1);
        hi = hi.saturating_add(1).min(127);
        while hi.saturating_sub(lo) < 5 && (lo > 0 || hi < 127) {
            lo = lo.saturating_sub(1);
            if hi < 127 {
                hi += 1;
            }
        }
        // Cap lane count so very wide chords stay legible; always include active pitches if huge.
        if hi.saturating_sub(lo) > 16 {
            let mut pitches: Vec<u8> = Vec::new();
            for p in (lo..=hi).rev() {
                pitches.push(p);
            }
            return (pitches.into_iter().map(RollLane::Pitch).collect(), false);
        }
        ((lo..=hi).rev().map(RollLane::Pitch).collect(), false)
    } else if has_hits {
        (
            vec![
                RollLane::Drum(DrumVoice::Kick),
                RollLane::Drum(DrumVoice::Snare),
                RollLane::Drum(DrumVoice::Hat),
                RollLane::Drum(DrumVoice::Clap),
            ],
            true,
        )
    } else {
        let mut mappings = Vec::new();
        for ev in events {
            if let PatternEvent::Sample { mapping, .. } = ev
                && !mappings.contains(mapping)
            {
                mappings.push(mapping.clone());
            }
        }
        if mappings.is_empty() {
            ((57..=64).rev().map(RollLane::Pitch).collect(), false)
        } else {
            (mappings.into_iter().map(RollLane::Sample).collect(), false)
        }
    }
}

fn draw_pattern_section(ui: &mut Ui, view: &SoundView<'_>, id: &str, changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_pat = view
        .previous
        .and_then(|p| p.song.patterns.iter().find(|pat| pat.id == id));
    let other_pat = view
        .other
        .and_then(|p| p.song.patterns.iter().find(|pat| pat.id == id));

    let Some(pattern) = song.patterns.iter().find(|p| p.id == id) else {
        section(ui, Kind::Pattern, id, |ui| {
            setting_chip(ui, "Pattern removed", true, false);
            change_chips(ui, changes);
        });
        return;
    };

    // Match the color of the track that uses this pattern when possible, else Kind::Pattern.
    let pat_color = song
        .tracks
        .iter()
        .enumerate()
        .find(|(_, t)| t.clips.iter().any(|c| c.pattern == pattern.id))
        .map_or(Kind::Pattern.color(), |(idx, _)| theme::track_color(idx));

    section(ui, Kind::Pattern, &pattern.id, |ui| {
        let meter = song.meter;
        let bar_len = meter.bar_length().map_or(4.0, |t| t.as_f64());

        // Header metadata row: span (lit if changed), event count, mode.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
            let span_bars = pattern.span.as_f64() / bar_len.max(1e-6);
            let prev_span = prev_pat.map(|p| p.span).filter(|s| *s != pattern.span);
            let other_span = other_pat.map(|o| o.span).filter(|s| *s != pattern.span);
            let span_text = match (prev_span, other_span) {
                (Some(ps), _) => format!(
                    "Span {:.2} bars (prev {:.2})",
                    span_bars,
                    ps.as_f64() / bar_len.max(1e-6)
                ),
                (None, Some(os)) => format!(
                    "Span {:.2} bars ({} {:.2})",
                    span_bars,
                    view.other_word,
                    os.as_f64() / bar_len.max(1e-6)
                ),
                _ => format!("Span {} bars", trim_num(span_bars)),
            };
            setting_chip(ui, &span_text, prev_span.is_some(), other_span.is_some());
            setting_chip(
                ui,
                &format!("{} events", pattern.events.len()),
                prev_pat.is_some_and(|p| p.events.len() != pattern.events.len()),
                false,
            );
        });
        ui.add_space(6.0);

        // Variation card if this pattern is a procedural variation.
        if let Some(var) = &pattern.variation {
            draw_pattern_variation(
                ui,
                var,
                prev_pat.and_then(|p| p.variation.as_ref()),
                other_pat.and_then(|o| o.variation.as_ref()),
                view.other_word,
            );
            ui.add_space(6.0);
        }

        // Piano roll / drum grid + synced velocity lane.
        let show_events =
            !pattern.events.is_empty() || prev_pat.is_some_and(|p| !p.events.is_empty());
        if show_events {
            let diff = diff_pattern_events(
                &pattern.events,
                prev_pat.map(|p| p.events.as_slice()),
                other_pat.map(|o| o.events.as_slice()),
            );
            let width = ui.available_width().max(260.0);
            let span_q = pattern
                .span
                .as_f64()
                .max(prev_pat.map_or(0.0, |p| p.span.as_f64()))
                .max(0.25);

            paint_pattern_grid(ui, &diff, span_q, meter, pat_color, width);
            ui.add_space(4.0);
            paint_velocity_lane(ui, &diff, span_q, meter, pat_color, width, view.other_word);

            // Concise plain-English caption of what moved on this pattern.
            let notes = summarize_pattern_diff(&diff, meter);
            if !notes.is_empty() {
                ui.add_space(5.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                    for note in notes {
                        setting_chip(ui, &note, true, false);
                    }
                });
            }
        }

        // Parameter locks strip if present or edited.
        if !pattern.locks.is_empty() || prev_pat.is_some_and(|p| !p.locks.is_empty()) {
            ui.add_space(6.0);
            draw_pattern_locks(
                ui,
                &pattern.locks,
                prev_pat.map(|p| p.locks.as_slice()),
                other_pat.map(|o| o.locks.as_slice()),
                meter,
                view.other_word,
            );
        }

        // Fallback chips only for unknown pattern fields not covered above.
        let remaining: Vec<&ValueChange> = changes
            .iter()
            .filter(|c| {
                c.path.first().is_none_or(|r| {
                    !matches!(r.as_str(), "events" | "span" | "locks" | "variation")
                })
            })
            .copied()
            .collect();
        change_chips(ui, &remaining);
    });
}

const PATTERN_GUTTER_W: f32 = 66.0;
const PATTERN_RULER_H: f32 = 18.0;

fn paint_pattern_grid(
    ui: &mut Ui,
    diff: &PatternEventDiff<'_>,
    span_q: f64,
    meter: crate::time::Meter,
    pat_color: Color32,
    width: f32,
) {
    let all_events = diff
        .entries
        .iter()
        .flat_map(|e| [Some(e.shown), e.prev, e.other].into_iter().flatten())
        .chain(diff.removed.iter().copied());
    let (lanes, is_drum) = build_lanes(all_events);

    let row_h = if is_drum {
        28.0_f32
    } else {
        (144.0 / lanes.len().max(4) as f32).clamp(11.0, 20.0)
    };
    let grid_h = (lanes.len() as f32 * row_h).clamp(96.0, 196.0);
    let total_h = PATTERN_RULER_H + grid_h + 2.0;

    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, total_h), Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 4.0, theme::BG_CANVAS);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    let grid = Rect::from_min_max(
        Pos2::new(rect.left() + PATTERN_GUTTER_W, rect.top() + PATTERN_RULER_H),
        Pos2::new(rect.right() - 1.0, rect.bottom() - 1.0),
    );
    let ruler = Rect::from_min_max(
        Pos2::new(grid.left(), rect.top() + 1.0),
        Pos2::new(grid.right(), grid.top()),
    );
    let corner = Rect::from_min_max(
        Pos2::new(rect.left() + 1.0, rect.top() + 1.0),
        Pos2::new(grid.left(), grid.top()),
    );

    painter.rect_filled(corner, 0.0, theme::BG_SURFACE);
    painter.rect_filled(ruler, 0.0, theme::BG_SURFACE);
    painter.hline(
        rect.x_range(),
        grid.top(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );
    painter.vline(
        grid.left(),
        rect.y_range(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );
    painter.text(
        corner.center(),
        Align2::CENTER_CENTER,
        if is_drum { "DRUM KIT" } else { "NOTES" },
        FontId::monospace(8.5),
        theme::TEXT_MUTED,
    );

    // Lanes and gutter labels.
    let actual_row_h = grid.height() / lanes.len().max(1) as f32;
    for (i, lane) in lanes.iter().enumerate() {
        let top = grid.top() + i as f32 * actual_row_h;
        let bot = (top + actual_row_h).min(grid.bottom());
        let lane_rect =
            Rect::from_min_max(Pos2::new(grid.left(), top), Pos2::new(grid.right(), bot));
        let gutter_rect = Rect::from_min_max(
            Pos2::new(rect.left() + 1.0, top),
            Pos2::new(grid.left(), bot),
        );

        let fill = if lane.is_black_key() {
            theme::BG_WELL
        } else if i % 2 == 1 {
            theme::BG_LANE_ALT
        } else {
            theme::BG_CANVAS
        };
        painter.rect_filled(lane_rect, 0.0, fill);
        painter.hline(
            lane_rect.x_range(),
            bot,
            Stroke::new(1.0_f32, with_alpha(theme::BORDER_SUBTLE, 0.6)),
        );

        match lane {
            RollLane::Drum(voice) => {
                let vc = theme::drum_voice_color(*voice);
                painter.rect_filled(gutter_rect, 0.0, theme::BG_SURFACE);
                painter.rect_filled(
                    Rect::from_min_max(
                        gutter_rect.left_top(),
                        Pos2::new(gutter_rect.left() + 3.0, gutter_rect.bottom()),
                    ),
                    0.0,
                    vc,
                );
                painter.text(
                    Pos2::new(gutter_rect.left() + 8.0, gutter_rect.center().y),
                    Align2::LEFT_CENTER,
                    lane.label(),
                    FontId::monospace(9.5),
                    vc,
                );
            }
            RollLane::Pitch(p) => {
                let is_c = p % 12 == 0;
                painter.rect_filled(
                    gutter_rect,
                    0.0,
                    if lane.is_black_key() {
                        Color32::from_rgb(20, 23, 30)
                    } else {
                        theme::BG_SURFACE
                    },
                );
                if is_c || actual_row_h >= 13.0 {
                    painter.text(
                        Pos2::new(gutter_rect.right() - 5.0, gutter_rect.center().y),
                        Align2::RIGHT_CENTER,
                        lane.label(),
                        FontId::monospace(8.5),
                        if is_c {
                            theme::TEXT_PRIMARY
                        } else {
                            theme::TEXT_MUTED
                        },
                    );
                }
            }
            RollLane::Sample(name) => {
                painter.rect_filled(gutter_rect, 0.0, theme::BG_SURFACE);
                let txt = widgets::ellipsize(
                    &painter,
                    name,
                    &FontId::monospace(8.5),
                    gutter_rect.width() - 8.0,
                );
                painter.text(
                    Pos2::new(gutter_rect.left() + 5.0, gutter_rect.center().y),
                    Align2::LEFT_CENTER,
                    txt,
                    FontId::monospace(8.5),
                    theme::TEXT_SECONDARY,
                );
            }
        }
    }

    // Beat / bar / subdivision gridlines + ruler ticks.
    let x_of = |q: f64| grid.left() + (q / span_q).clamp(0.0, 1.0) as f32 * grid.width();
    let bar_len = meter.bar_length().map_or(4.0, |t| t.as_f64());
    let beat_q = 4.0 / f64::from(meter.denominator.max(1));
    let sub_steps = ((span_q / (beat_q * 0.25)).round() as usize).min(128);
    let px_per_sub = grid.width() / sub_steps.max(1) as f32;
    if px_per_sub >= 10.0 {
        for s in 1..sub_steps {
            let q = s as f64 * beat_q * 0.25;
            if q < span_q - 1e-4 {
                painter.vline(
                    x_of(q),
                    grid.y_range(),
                    Stroke::new(1.0_f32, theme::GRID_SUB),
                );
            }
        }
    }
    let total_beats = (span_q / beat_q).ceil() as u32;
    for b in 0..=total_beats {
        let q = f64::from(b) * beat_q;
        if q > span_q + 1e-4 {
            break;
        }
        let x = x_of(q);
        let is_bar = (q % bar_len).abs() < 1e-4;
        painter.vline(
            x,
            grid.y_range(),
            Stroke::new(
                1.0_f32,
                if is_bar {
                    theme::GRID_BAR
                } else {
                    theme::GRID_BEAT
                },
            ),
        );
        if b < total_beats {
            let bar_num = (q / bar_len).floor() as u32 + 1;
            let beat_in_bar = ((q % bar_len) / beat_q).round() as u32 + 1;
            let lbl = if is_bar {
                format!("{bar_num}")
            } else {
                format!("{bar_num}.{beat_in_bar}")
            };
            if is_bar || grid.width() / total_beats.max(1) as f32 >= 26.0 {
                painter.text(
                    Pos2::new(x + 3.0, ruler.center().y),
                    Align2::LEFT_CENTER,
                    lbl,
                    FontId::monospace(8.5),
                    if is_bar {
                        theme::TEXT_PRIMARY
                    } else {
                        theme::TEXT_MUTED
                    },
                );
            }
        }
    }

    let event_rect = |ev: &PatternEvent| -> Option<(Rect, Color32)> {
        let lane_idx = lanes.iter().position(|l| l.matches(ev))?;
        let top = grid.top() + lane_idx as f32 * actual_row_h;
        let pad_y = (actual_row_h * 0.14).clamp(1.5, 3.5);
        let x0 = x_of(ev.at().as_f64());
        let (x1, col) = match ev {
            PatternEvent::Hit { voice, .. } => {
                let w = (grid.width() / (span_q as f32 * 4.0)).clamp(10.0, 20.0);
                (
                    (x0 + w).min(grid.right() - 1.0),
                    theme::drum_voice_color(*voice),
                )
            }
            PatternEvent::Note { .. } | PatternEvent::Sample { .. } => {
                let end = x_of(ev.at().as_f64() + event_duration(ev));
                ((end - 1.0).max(x0 + 5.0).min(grid.right() - 1.0), pat_color)
            }
        };
        Some((
            Rect::from_min_max(
                Pos2::new(x0, top + pad_y),
                Pos2::new(x1, top + actual_row_h - pad_y),
            ),
            col,
        ))
    };

    // Pass 1: Ghost previous positions of moved/revoiced events, plus removed events.
    for entry in &diff.entries {
        if let Some(prev) = entry.prev
            && event_position_changed(entry.shown, prev)
            && let Some((gr, gcol)) = event_rect(prev)
        {
            painter.rect_filled(gr, 3.0, with_alpha(gcol, 0.16));
            painter.rect_stroke(
                gr,
                3.0,
                Stroke::new(1.2_f32, with_alpha(theme::TEXT_SECONDARY, 0.78)),
                StrokeKind::Inside,
            );
            if let Some((cr, _)) = event_rect(entry.shown) {
                synth::paint_arrow(
                    &painter,
                    gr.center(),
                    cr.center(),
                    with_alpha(theme::SELECTION, 0.85),
                );
            }
        } else if let Some(other) = entry.other
            && entry.prev.is_none_or(|p| event_exact_eq(entry.shown, p))
            && event_position_changed(entry.shown, other)
            && let Some((or, _)) = event_rect(other)
        {
            painter.rect_stroke(
                or,
                3.0,
                Stroke::new(1.0_f32, with_alpha(theme::BORDER_STRONG, 0.85)),
                StrokeKind::Inside,
            );
        }
    }
    for rem in &diff.removed {
        if let Some((rr, _)) = event_rect(rem) {
            painter.rect_filled(rr, 3.0, with_alpha(theme::SIG_CLIP, 0.16));
            painter.rect_stroke(
                rr,
                3.0,
                Stroke::new(1.2_f32, with_alpha(theme::SIG_CLIP, 0.75)),
                StrokeKind::Inside,
            );
        }
    }

    // Pass 2: Current version's notes / hits.
    for entry in &diff.entries {
        let Some((nr, col)) = event_rect(entry.shown) else {
            continue;
        };
        let edited = entry.added || entry.prev.is_some_and(|p| !event_exact_eq(entry.shown, p));
        let differs = !edited && entry.other.is_some_and(|o| !event_exact_eq(entry.shown, o));
        let vel = event_velocity(entry.shown) as f32;
        let alpha = if edited {
            (0.75 + vel * 0.25).clamp(0.75, 1.0)
        } else {
            (0.45 + vel * 0.45).clamp(0.45, 0.92)
        };
        painter.rect_filled(nr, 3.0, with_alpha(col, alpha));
        if edited {
            painter.rect_stroke(
                nr.expand(1.0),
                3.5,
                Stroke::new(1.8_f32, theme::SELECTION),
                StrokeKind::Outside,
            );
        } else if differs {
            painter.rect_stroke(
                nr.expand(0.5),
                3.5,
                Stroke::new(1.1_f32, theme::BORDER_STRONG),
                StrokeKind::Outside,
            );
        } else {
            painter.rect_stroke(
                nr,
                3.0,
                Stroke::new(1.0_f32, with_alpha(col, 0.9)),
                StrokeKind::Inside,
            );
        }
    }
}

fn paint_velocity_lane(
    ui: &mut Ui,
    diff: &PatternEventDiff<'_>,
    span_q: f64,
    meter: crate::time::Meter,
    pat_color: Color32,
    width: f32,
    other_word: &str,
) {
    let height = 64.0_f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 4.0, theme::BG_CANVAS);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    let gutter = Rect::from_min_max(
        Pos2::new(rect.left() + 1.0, rect.top() + 1.0),
        Pos2::new(rect.left() + PATTERN_GUTTER_W, rect.bottom() - 1.0),
    );
    let lane = Rect::from_min_max(
        Pos2::new(gutter.right(), rect.top() + 1.0),
        Pos2::new(rect.right() - 1.0, rect.bottom() - 1.0),
    );
    painter.rect_filled(gutter, 0.0, theme::BG_SURFACE);
    painter.vline(
        gutter.right(),
        rect.y_range(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );
    painter.text(
        Pos2::new(gutter.left() + 6.0, gutter.top() + 7.0),
        Align2::LEFT_TOP,
        "VELOCITY",
        FontId::monospace(8.5),
        theme::TEXT_SECONDARY,
    );
    painter.text(
        Pos2::new(gutter.right() - 5.0, gutter.top() + 22.0),
        Align2::RIGHT_CENTER,
        "100%",
        FontId::monospace(8.0),
        theme::TEXT_MUTED,
    );
    painter.text(
        Pos2::new(gutter.right() - 5.0, gutter.bottom() - 12.0),
        Align2::RIGHT_CENTER,
        "0%",
        FontId::monospace(8.0),
        theme::TEXT_MUTED,
    );

    let pad_top = 8.0_f32;
    let pad_bot = 6.0_f32;
    let stem_h = (lane.height() - pad_top - pad_bot).max(12.0);
    let y_of = |v: f64| lane.bottom() - pad_bot - (v.clamp(0.0, 1.0) as f32) * stem_h;
    let x_of = |q: f64| lane.left() + (q / span_q).clamp(0.0, 1.0) as f32 * lane.width();

    for frac in [0.25_f64, 0.5, 0.75] {
        painter.hline(
            lane.x_range(),
            y_of(frac),
            Stroke::new(1.0_f32, theme::GRID_SUB),
        );
    }
    let bar_len = meter.bar_length().map_or(4.0, |t| t.as_f64());
    let beat_q = 4.0 / f64::from(meter.denominator.max(1));
    let total_beats = (span_q / beat_q).ceil() as u32;
    for b in 0..=total_beats {
        let q = f64::from(b) * beat_q;
        if q > span_q + 1e-4 {
            break;
        }
        let is_bar = (q % bar_len).abs() < 1e-4;
        painter.vline(
            x_of(q),
            lane.y_range(),
            Stroke::new(
                1.0_f32,
                if is_bar {
                    theme::GRID_BAR
                } else {
                    theme::GRID_BEAT
                },
            ),
        );
    }

    // Fan simultaneous events slightly horizontally so chords / layered drums stay distinct.
    let mut xs: Vec<f32> = Vec::with_capacity(diff.entries.len());
    for (idx, entry) in diff.entries.iter().enumerate() {
        let at_q = entry.shown.at().as_f64();
        let same_before = diff.entries[..idx]
            .iter()
            .filter(|o| (o.shown.at().as_f64() - at_q).abs() < 0.005)
            .count();
        let x = (x_of(at_q) + 5.0 + same_before as f32 * 5.0).min(lane.right() - 5.0);
        xs.push(x);
    }

    // Removed event stems (muted red ghost).
    for rem in &diff.removed {
        let rx = (x_of(rem.at().as_f64()) + 5.0).min(lane.right() - 5.0);
        let ry = y_of(event_velocity(rem));
        painter.line_segment(
            [Pos2::new(rx, lane.bottom() - pad_bot), Pos2::new(rx, ry)],
            Stroke::new(2.0_f32, with_alpha(theme::SIG_CLIP, 0.4)),
        );
        painter.circle_stroke(
            Pos2::new(rx, ry),
            3.0,
            Stroke::new(1.2_f32, with_alpha(theme::SIG_CLIP, 0.7)),
        );
    }

    for (idx, entry) in diff.entries.iter().enumerate() {
        let x = xs[idx];
        let v = event_velocity(entry.shown);
        let y = y_of(v);
        let prev_v = entry.prev.map(event_velocity);
        let other_v = entry.other.map(event_velocity);
        let vel_edited = prev_v.is_some_and(|pv| (pv - v).abs() > 1e-4);
        let vel_differs = !vel_edited && other_v.is_some_and(|ov| (ov - v).abs() > 1e-4);
        let moved = entry
            .prev
            .is_some_and(|p| event_position_changed(entry.shown, p));

        let col = match entry.shown {
            PatternEvent::Hit { voice, .. } => theme::drum_voice_color(*voice),
            _ => pat_color,
        };

        // Ghost previous velocity height on the same stem (or at old x if moved).
        if let Some(pv) = prev_v
            && (vel_edited || moved)
        {
            let px = if moved {
                (x_of(entry.prev.unwrap().at().as_f64()) + 5.0).min(lane.right() - 5.0)
            } else {
                x
            };
            let py = y_of(pv);
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(px - 3.0, py),
                    Pos2::new(px + 3.0, lane.bottom() - pad_bot),
                ),
                1.5,
                with_alpha(theme::TEXT_SECONDARY, 0.22),
            );
            painter.hline(
                (px - 6.0)..=(px + 6.0),
                py,
                Stroke::new(1.6_f32, theme::TEXT_SECONDARY),
            );
        } else if let Some(ov) = other_v
            && vel_differs
        {
            let oy = y_of(ov);
            painter.hline(
                (x - 5.0)..=(x + 5.0),
                oy,
                Stroke::new(1.3_f32, theme::BORDER_STRONG),
            );
        }

        let bar_col = if vel_edited || entry.added {
            theme::SELECTION
        } else {
            col
        };
        let bar_half = if vel_edited || entry.added { 3.5 } else { 2.5 };
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x - bar_half, y),
                Pos2::new(x + bar_half, lane.bottom() - pad_bot),
            ),
            1.5,
            with_alpha(bar_col, if vel_edited { 0.85 } else { 0.62 }),
        );
        painter.circle_filled(Pos2::new(x, y), if vel_edited { 4.0 } else { 3.2 }, bar_col);
        if vel_edited {
            painter.circle_stroke(
                Pos2::new(x, y),
                4.0,
                Stroke::new(1.2_f32, theme::TEXT_PRIMARY),
            );
            // Compact percentage readout beside the edited lollipop.
            let label = format!("{:.0}%", v * 100.0);
            let fits_right = x + 32.0 < lane.right();
            painter.text(
                Pos2::new(
                    if fits_right { x + 7.0 } else { x - 7.0 },
                    y.clamp(lane.top() + 8.0, lane.bottom() - 10.0),
                ),
                if fits_right {
                    Align2::LEFT_CENTER
                } else {
                    Align2::RIGHT_CENTER
                },
                label,
                FontId::monospace(9.0),
                theme::TEXT_PRIMARY,
            );
        } else if moved {
            painter.circle_stroke(Pos2::new(x, y), 4.0, Stroke::new(1.3_f32, theme::SELECTION));
        } else if vel_differs && let Some(ov) = other_v {
            let _ = (other_word, ov);
            painter.circle_stroke(
                Pos2::new(x, y),
                3.5,
                Stroke::new(1.0_f32, theme::BORDER_STRONG),
            );
        }
    }
}

fn summarize_pattern_diff(diff: &PatternEventDiff<'_>, meter: crate::time::Meter) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, entry) in diff.entries.iter().enumerate() {
        let num = idx + 1;
        if entry.added {
            out.push(format!(
                "Added #{} at {}",
                num,
                theme::format_bar_beat(entry.shown.at().as_f64(), meter)
            ));
            continue;
        }
        let Some(prev) = entry.prev else {
            continue;
        };
        if event_exact_eq(entry.shown, prev) {
            continue;
        }
        let mut parts = Vec::new();
        if entry.shown.at() != prev.at() {
            parts.push(format!(
                "moved {} → {}",
                theme::format_bar_beat(prev.at().as_f64(), meter),
                theme::format_bar_beat(entry.shown.at().as_f64(), meter)
            ));
        }
        match (prev, entry.shown) {
            (PatternEvent::Hit { voice: v0, .. }, PatternEvent::Hit { voice: v1, .. })
                if v0 != v1 =>
            {
                parts.push(format!(
                    "voice {} → {}",
                    drum_voice_name(*v0).to_lowercase(),
                    drum_voice_name(*v1).to_lowercase()
                ));
            }
            (
                PatternEvent::Note {
                    pitch: p0,
                    duration: d0,
                    ..
                },
                PatternEvent::Note {
                    pitch: p1,
                    duration: d1,
                    ..
                },
            ) => {
                if p0 != p1 {
                    parts.push(format!(
                        "pitch {} → {}",
                        crate::model::format_pitch(*p0),
                        crate::model::format_pitch(*p1)
                    ));
                }
                if d0 != d1 {
                    parts.push(format!("len {d0} → {d1}"));
                }
            }
            _ => {}
        }
        let v0 = event_velocity(prev);
        let v1 = event_velocity(entry.shown);
        if (v0 - v1).abs() > 1e-4 {
            parts.push(format!("vel {:.0}% → {:.0}%", v0 * 100.0, v1 * 100.0));
        }
        if !parts.is_empty() {
            out.push(format!("#{num} {}", parts.join(" · ")));
        }
        if out.len() >= 6 {
            break;
        }
    }
    if !diff.removed.is_empty() {
        out.push(format!("{} removed", diff.removed.len()));
    }
    out
}

fn draw_pattern_locks(
    ui: &mut Ui,
    locks: &[ParameterLock],
    prev: Option<&[ParameterLock]>,
    other: Option<&[ParameterLock]>,
    meter: crate::time::Meter,
    other_word: &str,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
        ui.label(
            RichText::new("P-LOCKS")
                .strong()
                .size(9.0)
                .color(theme::ACCENT_VIOLET),
        );
        for lock in locks {
            let pl = prev.and_then(|list| list.iter().find(|l| l.id == lock.id));
            let ol = other.and_then(|list| list.iter().find(|l| l.id == lock.id));
            let edited = prev.is_some()
                && pl.is_none_or(|p| {
                    p.at != lock.at || p.parameter != lock.parameter || p.value != lock.value
                });
            let differs = !edited
                && ol.is_some_and(|o| {
                    o.at != lock.at || o.parameter != lock.parameter || o.value != lock.value
                });
            let pos = theme::format_bar_beat(lock.at.as_f64(), meter);
            let val = readable_literal(&lock.value);
            let mut text = format!("@{pos} {} = {val}", lock.parameter);
            if let Some(p) = pl
                && edited
            {
                text.push_str(&format!(" (prev {})", readable_literal(&p.value)));
            } else if let Some(o) = ol
                && differs
            {
                text.push_str(&format!(" ({other_word} {})", readable_literal(&o.value)));
            }
            setting_chip(ui, &text, edited, differs);
        }
    });
}

fn draw_pattern_variation(
    ui: &mut Ui,
    var: &PatternVariation,
    prev: Option<&PatternVariation>,
    other: Option<&PatternVariation>,
    other_word: &str,
) {
    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_header(
                ui,
                &format!("VARIATION FROM {}", var.source.to_uppercase()),
                theme::ACCENT_CYAN,
            );
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                dial(
                    ui,
                    "Velocity",
                    var.velocity_scale,
                    0.0,
                    2.0,
                    Unit::Ratio,
                    &|v| format!("{:.0}%", v * 100.0),
                    mark(
                        var.velocity_scale,
                        prev.map(|p| p.velocity_scale),
                        other.map(|o| o.velocity_scale),
                    ),
                    other_word,
                    theme::ACCENT_CYAN,
                );
                dial(
                    ui,
                    "Prob",
                    var.probability,
                    0.0,
                    1.0,
                    Unit::Ratio,
                    &|v| format!("{:.0}%", v * 100.0),
                    mark(
                        var.probability,
                        prev.map(|p| p.probability),
                        other.map(|o| o.probability),
                    ),
                    other_word,
                    theme::ACCENT_CYAN,
                );
                ui.vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                        diff_chip(
                            ui,
                            "Rotate",
                            &format!("{}", var.rotate),
                            prev.map(|p| format!("{}", p.rotate)),
                            other.map(|o| format!("{}", o.rotate)),
                            other_word,
                        );
                        diff_chip(
                            ui,
                            "Transpose",
                            &format!("{:+} st", var.transpose_semitones),
                            prev.map(|p| format!("{:+} st", p.transpose_semitones)),
                            other.map(|o| format!("{:+} st", o.transpose_semitones)),
                            other_word,
                        );
                        diff_chip(
                            ui,
                            "Reverse",
                            if var.reverse { "on" } else { "off" },
                            prev.map(|p| if p.reverse { "on" } else { "off" }.to_owned()),
                            other.map(|o| if o.reverse { "on" } else { "off" }.to_owned()),
                            other_word,
                        );
                        diff_chip(
                            ui,
                            "Seed",
                            &var.seed.to_string(),
                            prev.map(|p| p.seed.to_string()),
                            other.map(|o| o.seed.to_string()),
                            other_word,
                        );
                    });
                });
            });
        });
}

fn diff_chip(
    ui: &mut Ui,
    label: &str,
    curr: &str,
    prev: Option<String>,
    other: Option<String>,
    other_word: &str,
) {
    let p_diff = prev.filter(|p| p != curr);
    let o_diff = other.filter(|o| o != curr);
    let (text, lit, quiet) = match (p_diff, o_diff) {
        (Some(p), _) => (format!("{label} {curr} (prev {p})"), true, false),
        (None, Some(o)) => (format!("{label} {curr} ({other_word} {o})"), false, true),
        _ => (format!("{label} {curr}"), false, false),
    };
    setting_chip(ui, &text, lit, quiet);
}

// ── Arrangement: Track Clips on Mini Timeline ────────────────────────────────

fn draw_arrangement_section(
    ui: &mut Ui,
    view: &SoundView<'_>,
    track_id: &str,
    changes: &[&ValueChange],
) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);

    let Some((track_idx, track)) = song
        .tracks
        .iter()
        .enumerate()
        .find(|(_, t)| t.id == track_id)
    else {
        section(ui, Kind::Arrangement, track_id, |ui| {
            setting_chip(ui, "Track removed", true, false);
            change_chips(ui, changes);
        });
        return;
    };

    let prev_track = prev_song.and_then(|s| s.tracks.iter().find(|t| t.id == track_id));
    let other_track = other_song.and_then(|s| s.tracks.iter().find(|t| t.id == track_id));
    let color = theme::track_color(track_idx);

    section(ui, Kind::Arrangement, &track.name, |ui| {
        let width = ui.available_width().max(260.0);
        paint_track_clips_timeline(ui, song, track, prev_track, other_track, color, width);

        // Concise summary of clip edits (moves, resizes, adds, deletes, gain/transpose/mode).
        let summaries =
            summarize_clip_edits(track, prev_track, other_track, song.meter, view.other_word);
        if !summaries.is_empty() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                for (text, lit, quiet) in summaries {
                    setting_chip(ui, &text, lit, quiet);
                }
            });
        }
    });
}

fn clip_changed(a: &Clip, b: &Clip) -> bool {
    a.pattern != b.pattern
        || a.start != b.start
        || a.duration != b.duration
        || a.mode != b.mode
        || a.source_offset != b.source_offset
        || a.transpose_semitones != b.transpose_semitones
        || (a.gain_db - b.gain_db).abs() > 1e-6
        || a.fade_in != b.fade_in
        || a.fade_out != b.fade_out
        || a.groove != b.groove
}

fn paint_track_clips_timeline(
    ui: &mut Ui,
    song: &Song,
    track: &Track,
    prev_track: Option<&Track>,
    other_track: Option<&Track>,
    color: Color32,
    width: f32,
) {
    let max_clip_end = track
        .clips
        .iter()
        .chain(prev_track.into_iter().flat_map(|t| t.clips.iter()))
        .map(|c| c.start.as_f64() + c.duration.as_f64())
        .fold(0.0_f64, f64::max);
    let end_q = song.end.as_f64().max(max_clip_end).max(4.0);
    let bar_len = song.meter.bar_length().map_or(4.0, |t| t.as_f64());

    let has_locators = !song.locators.is_empty();
    let loc_h = if has_locators { 15.0 } else { 0.0 };
    let ruler_h = 16.0_f32;
    let lane_h = 68.0_f32;
    let total_h = loc_h + ruler_h + lane_h + 2.0;

    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, total_h), Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 4.0, theme::BG_CANVAS);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    let loc_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 1.0, rect.top() + 1.0),
        Pos2::new(rect.right() - 1.0, rect.top() + 1.0 + loc_h),
    );
    let ruler_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 1.0, loc_rect.bottom()),
        Pos2::new(rect.right() - 1.0, loc_rect.bottom() + ruler_h),
    );
    let lane_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 1.0, ruler_rect.bottom()),
        Pos2::new(rect.right() - 1.0, rect.bottom() - 1.0),
    );

    if has_locators {
        painter.rect_filled(loc_rect, 0.0, Color32::from_rgb(20, 24, 31));
    }
    painter.rect_filled(ruler_rect, 0.0, theme::BG_SURFACE);
    painter.hline(
        rect.x_range(),
        lane_rect.top(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );

    let x_of = |q: f64| lane_rect.left() + (q / end_q).clamp(0.0, 1.0) as f32 * lane_rect.width();

    // Bar gridlines & bar numbers.
    let bar_px = (bar_len / end_q) as f32 * lane_rect.width();
    let label_every = theme::bar_label_interval(46.0 / bar_px.max(1.0));
    let total_bars = (end_q / bar_len).ceil() as u32;
    for bar in 0..=total_bars {
        let q = f64::from(bar) * bar_len;
        if q > end_q + 1e-4 {
            break;
        }
        let x = x_of(q);
        let major = bar.is_multiple_of(label_every) || bar_px >= 20.0;
        if major || bar_px >= 8.0 {
            painter.vline(
                x,
                lane_rect.y_range(),
                Stroke::new(
                    1.0_f32,
                    if major {
                        theme::GRID_BAR
                    } else {
                        theme::GRID_SUB
                    },
                ),
            );
        }
        if bar.is_multiple_of(label_every) {
            let fits_right = x + 28.0 <= ruler_rect.right();
            painter.text(
                Pos2::new(
                    if fits_right { x + 3.0 } else { x - 3.0 },
                    ruler_rect.center().y,
                ),
                if fits_right {
                    Align2::LEFT_CENTER
                } else {
                    Align2::RIGHT_CENTER
                },
                format!("{}", bar + 1),
                FontId::monospace(8.5),
                theme::TEXT_SECONDARY,
            );
        }
    }

    // Locator flags in top band.
    if has_locators {
        paint_locator_band(&painter, loc_rect, lane_rect, &song.locators, end_q);
    }

    let clip_rect = |clip: &Clip| -> Rect {
        let x0 = x_of(clip.start.as_f64());
        let x1 = x_of(clip.start.as_f64() + clip.duration.as_f64()).max(x0 + 4.0);
        Rect::from_min_max(
            Pos2::new(x0, lane_rect.top() + 5.0),
            Pos2::new(x1, lane_rect.bottom() - 5.0),
        )
    };

    // Pass 1: Ghost previous clip bounds (for moved/resized clips) and removed clips.
    if let Some(pt) = prev_track {
        for prev_clip in &pt.clips {
            let curr = track.clips.iter().find(|c| c.id == prev_clip.id);
            match curr {
                None => {
                    let gr = clip_rect(prev_clip);
                    painter.rect_filled(gr, 4.0, with_alpha(theme::SIG_CLIP, 0.14));
                    painter.rect_stroke(
                        gr,
                        4.0,
                        Stroke::new(1.3_f32, with_alpha(theme::SIG_CLIP, 0.72)),
                        StrokeKind::Inside,
                    );
                }
                Some(c) if c.start != prev_clip.start || c.duration != prev_clip.duration => {
                    let gr = clip_rect(prev_clip);
                    let cr = clip_rect(c);
                    painter.rect_filled(gr, 4.0, with_alpha(color, 0.10));
                    painter.rect_stroke(
                        gr,
                        4.0,
                        Stroke::new(1.3_f32, with_alpha(theme::TEXT_SECONDARY, 0.78)),
                        StrokeKind::Inside,
                    );
                    synth::paint_arrow(
                        &painter,
                        gr.center(),
                        cr.center(),
                        with_alpha(theme::SELECTION, 0.85),
                    );
                }
                _ => {}
            }
        }
    }
    if let Some(ot) = other_track {
        for oc in &ot.clips {
            if let Some(c) = track.clips.iter().find(|c| c.id == oc.id)
                && prev_track
                    .and_then(|pt| pt.clips.iter().find(|p| p.id == c.id))
                    .is_none_or(|p| !clip_changed(p, c))
                && (c.start != oc.start || c.duration != oc.duration)
            {
                let or = clip_rect(oc);
                painter.rect_stroke(
                    or,
                    4.0,
                    Stroke::new(1.0_f32, with_alpha(theme::BORDER_STRONG, 0.8)),
                    StrokeKind::Inside,
                );
            }
        }
    }

    // Pass 2: Current clips with header bar, mini MIDI preview, and fades.
    for clip in &track.clips {
        let body = clip_rect(clip);
        let prev_clip = prev_track.and_then(|pt| pt.clips.iter().find(|p| p.id == clip.id));
        let other_clip = other_track.and_then(|ot| ot.clips.iter().find(|o| o.id == clip.id));
        let added = prev_track.is_some() && prev_clip.is_none();
        let edited = added || prev_clip.is_some_and(|p| clip_changed(p, clip));
        let differs = !edited && other_clip.is_some_and(|o| clip_changed(o, clip));

        let header_h = 14.0_f32;
        let header = Rect::from_min_max(
            body.left_top(),
            Pos2::new(body.right(), (body.top() + header_h).min(body.bottom())),
        );
        painter.rect_filled(
            body,
            4.0,
            with_alpha(color, if edited { 0.28 } else { 0.18 }),
        );
        painter.rect_filled(
            header,
            egui::CornerRadius {
                nw: 4,
                ne: 4,
                sw: 0,
                se: 0,
            },
            with_alpha(color, if edited { 0.94 } else { 0.80 }),
        );

        // Mini note/hit preview inside the clip body.
        let content = Rect::from_min_max(
            Pos2::new(body.left() + 2.0, header.bottom() + 2.0),
            Pos2::new(body.right() - 2.0, body.bottom() - 2.0),
        );
        if content.width() >= 6.0
            && content.height() >= 8.0
            && let Some(pat) = song.patterns.iter().find(|p| p.id == clip.pattern)
            && !pat.events.is_empty()
        {
            paint_mini_clip_events(&painter, content, clip, pat, color, end_q, lane_rect);
        }

        // Fade wedges when present.
        if clip.fade_in.as_f64() > 0.0 {
            let fx = x_of(clip.start.as_f64() + clip.fade_in.as_f64()).min(body.right());
            painter.line_segment(
                [
                    Pos2::new(body.left(), body.bottom()),
                    Pos2::new(fx, header.bottom()),
                ],
                Stroke::new(1.0_f32, with_alpha(color, 0.85)),
            );
        }
        if clip.fade_out.as_f64() > 0.0 {
            let fx = x_of(clip.start.as_f64() + clip.duration.as_f64() - clip.fade_out.as_f64())
                .max(body.left());
            painter.line_segment(
                [
                    Pos2::new(fx, header.bottom()),
                    Pos2::new(body.right(), body.bottom()),
                ],
                Stroke::new(1.0_f32, with_alpha(color, 0.85)),
            );
        }

        if edited {
            painter.rect_stroke(
                body.expand(1.0),
                4.5,
                Stroke::new(2.0_f32, theme::SELECTION),
                StrokeKind::Outside,
            );
        } else if differs {
            painter.rect_stroke(
                body,
                4.0,
                Stroke::new(1.3_f32, theme::BORDER_STRONG),
                StrokeKind::Inside,
            );
        } else {
            painter.rect_stroke(
                body,
                4.0,
                Stroke::new(1.0_f32, with_alpha(color, 0.72)),
                StrokeKind::Inside,
            );
        }

        if body.width() >= 24.0 {
            let label = widgets::ellipsize(
                &painter,
                &clip.pattern,
                &FontId::proportional(9.5),
                header.width() - 8.0,
            );
            painter
                .with_clip_rect(header.shrink2(Vec2::new(2.0, 0.0)))
                .text(
                    Pos2::new(header.left() + 4.0, header.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(9.5),
                    theme::BG_WELL,
                );
        }
    }
}

fn paint_mini_clip_events(
    painter: &egui::Painter,
    content: Rect,
    clip: &Clip,
    pattern: &Pattern,
    color: Color32,
    end_q: f64,
    lane_rect: Rect,
) {
    let p = painter.with_clip_rect(content);
    let x_of = |q: f64| lane_rect.left() + (q / end_q).clamp(0.0, 1.0) as f32 * lane_rect.width();
    let clip_start = clip.start.as_f64();
    let clip_dur = clip.duration.as_f64();
    let pat_len = pattern.span.as_f64().max(0.25);
    let repeats = match clip.mode {
        ClipMode::Once => 1,
        ClipMode::Loop => ((clip_dur / pat_len).ceil().max(1.0) as usize).min(64),
    };
    let bright = Color32::from_rgb(
        color.r().saturating_add(50),
        color.g().saturating_add(50),
        color.b().saturating_add(50),
    );
    let (min_p, max_p) = pattern.events.iter().fold((127u8, 0u8), |(lo, hi), ev| {
        if let PatternEvent::Note { pitch, .. } = ev {
            (lo.min(*pitch), hi.max(*pitch))
        } else {
            (lo, hi)
        }
    });
    let pitch_span = f32::from(max_p.saturating_sub(min_p).max(6));

    for rep in 0..repeats {
        let offset = clip_start + rep as f64 * pat_len;
        if offset >= clip_start + clip_dur {
            break;
        }
        for ev in &pattern.events {
            match ev {
                PatternEvent::Note {
                    at,
                    pitch,
                    duration,
                    ..
                } => {
                    let s = offset + at.as_f64();
                    let e = (s + duration.as_f64()).min(clip_start + clip_dur);
                    if s >= clip_start + clip_dur || e <= s {
                        continue;
                    }
                    let nx0 = x_of(s);
                    let nx1 = x_of(e).max(nx0 + 2.0);
                    let frac = (f32::from(*pitch - min_p) / pitch_span).clamp(0.0, 1.0);
                    let ny = content.bottom() - 3.5 - frac * (content.height() - 4.0).max(0.0);
                    p.rect_filled(
                        Rect::from_min_max(Pos2::new(nx0, ny), Pos2::new(nx1, ny + 3.0)),
                        1.0,
                        with_alpha(bright, 0.78),
                    );
                }
                PatternEvent::Hit { at, voice, .. } => {
                    let s = offset + at.as_f64();
                    if s >= clip_start + clip_dur {
                        continue;
                    }
                    let row = match voice {
                        DrumVoice::Hat => 0,
                        DrumVoice::Clap => 1,
                        DrumVoice::Snare => 2,
                        DrumVoice::Kick => 3,
                    };
                    let rh = (content.height() / 4.0).max(3.0);
                    let ny = content.top() + row as f32 * rh + 1.0;
                    let nx0 = x_of(s);
                    p.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(nx0, ny),
                            Pos2::new(nx0 + 2.5, ny + (rh - 2.0).max(2.0)),
                        ),
                        1.0,
                        with_alpha(bright, 0.78),
                    );
                }
                PatternEvent::Sample { at, duration, .. } => {
                    let s = offset + at.as_f64();
                    let e = (s + duration.as_f64()).min(clip_start + clip_dur);
                    if s >= clip_start + clip_dur || e <= s {
                        continue;
                    }
                    p.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(x_of(s), content.center().y - 4.0),
                            Pos2::new(x_of(e).max(x_of(s) + 2.5), content.center().y + 4.0),
                        ),
                        1.0,
                        with_alpha(bright, 0.72),
                    );
                }
            }
        }
    }
}

fn summarize_clip_edits(
    track: &Track,
    prev_track: Option<&Track>,
    other_track: Option<&Track>,
    meter: crate::time::Meter,
    other_word: &str,
) -> Vec<(String, bool, bool)> {
    let mut out = Vec::new();
    let bar_len = meter.bar_length().map_or(4.0, |t| t.as_f64());
    let fmt_bar = |q: f64| format!("bar {}", trim_num(q / bar_len.max(1e-6) + 1.0));
    let fmt_dur = |q: f64| format!("{} bars", trim_num(q / bar_len.max(1e-6)));

    for clip in &track.clips {
        let pc = prev_track.and_then(|pt| pt.clips.iter().find(|p| p.id == clip.id));
        let oc = other_track.and_then(|ot| ot.clips.iter().find(|o| o.id == clip.id));
        if prev_track.is_some() && pc.is_none() {
            out.push((
                format!(
                    "+ {} ({}) at {}, {}",
                    clip.id,
                    clip.pattern,
                    fmt_bar(clip.start.as_f64()),
                    fmt_dur(clip.duration.as_f64())
                ),
                true,
                false,
            ));
            continue;
        }
        if let Some(p) = pc
            && clip_changed(p, clip)
        {
            let mut parts = Vec::new();
            if p.start != clip.start {
                parts.push(format!(
                    "{} → {}",
                    fmt_bar(p.start.as_f64()),
                    fmt_bar(clip.start.as_f64())
                ));
            }
            if p.duration != clip.duration {
                parts.push(format!(
                    "len {} → {}",
                    fmt_dur(p.duration.as_f64()),
                    fmt_dur(clip.duration.as_f64())
                ));
            }
            if p.pattern != clip.pattern {
                parts.push(format!("pattern {} → {}", p.pattern, clip.pattern));
            }
            if (p.gain_db - clip.gain_db).abs() > 1e-4 {
                parts.push(format!("gain {:.1} → {:.1} dB", p.gain_db, clip.gain_db));
            }
            if p.transpose_semitones != clip.transpose_semitones {
                parts.push(format!(
                    "transpose {:+} → {:+} st",
                    p.transpose_semitones, clip.transpose_semitones
                ));
            }
            if p.mode != clip.mode {
                parts.push(format!("mode {:?} → {:?}", p.mode, clip.mode));
            }
            if p.groove != clip.groove {
                parts.push(format!(
                    "groove {} → {}",
                    p.groove.as_deref().unwrap_or("none"),
                    clip.groove.as_deref().unwrap_or("none")
                ));
            }
            if parts.is_empty() {
                parts.push("updated".to_owned());
            }
            out.push((format!("{}: {}", clip.id, parts.join(" · ")), true, false));
        } else if let Some(o) = oc
            && clip_changed(o, clip)
        {
            out.push((
                format!(
                    "{}: at {} ({other_word} {})",
                    clip.id,
                    fmt_bar(clip.start.as_f64()),
                    fmt_bar(o.start.as_f64())
                ),
                false,
                true,
            ));
        }
    }
    if let Some(pt) = prev_track {
        for p in &pt.clips {
            if !track.clips.iter().any(|c| c.id == p.id) {
                out.push((
                    format!("− {} removed (was {})", p.id, fmt_bar(p.start.as_f64())),
                    true,
                    false,
                ));
            }
        }
    }
    out
}

fn paint_locator_band(
    painter: &egui::Painter,
    band: Rect,
    plot: Rect,
    locators: &[Locator],
    end_q: f64,
) {
    let x_of = |q: f64| plot.left() + (q / end_q).clamp(0.0, 1.0) as f32 * plot.width();
    let mut last_right = f32::NEG_INFINITY;
    for loc in locators {
        let x = x_of(loc.at.as_f64());
        painter.vline(
            x,
            plot.y_range(),
            Stroke::new(1.0_f32, with_alpha(theme::TEXT_MUTED, 0.14)),
        );
        let fw = (loc.name.len() as f32 * 5.2 + 10.0).clamp(26.0, 74.0);
        let flag_x = if x + fw > band.right() - 2.0 {
            x - fw
        } else {
            x
        };
        if flag_x < last_right + 3.0 {
            continue;
        }
        let flag = Rect::from_min_size(
            Pos2::new(flag_x, band.top() + 1.5),
            Vec2::new(fw, (band.height() - 3.0).max(8.0)),
        );
        painter.rect_filled(flag, 2.0, theme::BG_CARD);
        painter.rect_filled(
            Rect::from_min_size(flag.left_top(), Vec2::new(2.0, flag.height())),
            1.0,
            theme::TEXT_MUTED,
        );
        painter.text(
            Pos2::new(flag.left() + 5.0, flag.center().y),
            Align2::LEFT_CENTER,
            &loc.name,
            FontId::proportional(8.5),
            theme::TEXT_SECONDARY,
        );
        last_right = flag.right();
    }
}

// ── Automation Lanes as Curves (with Previous Ghosted) ───────────────────────

struct ResolvedTarget {
    title: String,
    unit: Unit,
    min: f64,
    max: f64,
}

fn resolve_target(song: &Song, target: &ParameterTarget) -> ResolvedTarget {
    match target {
        ParameterTarget::Instrument {
            instrument,
            parameter,
        } => {
            if let Some(inst) = song.instruments.iter().find(|i| i.id == *instrument)
                && let Some(param) = inst.parameters.iter().find(|p| p.key == *parameter)
            {
                return ResolvedTarget {
                    title: format!("{} · {}", inst.name, param.name),
                    unit: param.unit,
                    min: param.minimum,
                    max: param.maximum,
                };
            }
            ResolvedTarget {
                title: format!("{instrument} · {parameter}"),
                unit: Unit::Ratio,
                min: 0.0,
                max: 1.0,
            }
        }
        ParameterTarget::Track { track, parameter } => {
            let name = song
                .tracks
                .iter()
                .find(|t| t.id == *track)
                .map_or(track.as_str(), |t| t.name.as_str());
            track_param_target(name, *parameter)
        }
        ParameterTarget::Group { group, parameter } => {
            let name = song
                .groups
                .iter()
                .find(|g| g.id == *group)
                .map_or(group.as_str(), |g| g.name.as_str());
            track_param_target(name, *parameter)
        }
        ParameterTarget::Return {
            r#return,
            parameter,
        } => {
            let name = song
                .returns
                .iter()
                .find(|r| r.id == *r#return)
                .map_or(r#return.as_str(), |r| r.name.as_str());
            track_param_target(name, *parameter)
        }
        ParameterTarget::Send { send } => ResolvedTarget {
            title: format!("Send {send} · Level"),
            unit: Unit::Decibels,
            min: -60.0,
            max: 12.0,
        },
        ParameterTarget::Processor {
            processor,
            parameter,
        } => {
            let proc = find_processor(song, processor);
            let title = proc.map_or_else(
                || format!("{processor} · {parameter}"),
                |(host, p)| format!("{} on {host} · {parameter}", theme::processor_title(p.kind)),
            );
            let (unit, min, max) = proc
                .and_then(|(_, p)| crate::model::processor_setting_contract(p.kind, parameter))
                .unwrap_or((Unit::Ratio, 0.0, 1.0));
            ResolvedTarget {
                title,
                unit,
                min,
                max,
            }
        }
        ParameterTarget::Master => ResolvedTarget {
            title: "Master · Level".to_owned(),
            unit: Unit::Decibels,
            min: -60.0,
            max: 12.0,
        },
    }
}

fn track_param_target(name: &str, parameter: TrackParameter) -> ResolvedTarget {
    match parameter {
        TrackParameter::Level => ResolvedTarget {
            title: format!("{name} · Level"),
            unit: Unit::Decibels,
            min: -60.0,
            max: 12.0,
        },
        TrackParameter::Pan
        | TrackParameter::Balance
        | TrackParameter::LeftPan
        | TrackParameter::RightPan => ResolvedTarget {
            title: format!("{name} · {parameter:?}"),
            unit: Unit::Pan,
            min: -1.0,
            max: 1.0,
        },
        TrackParameter::Mute => ResolvedTarget {
            title: format!("{name} · Mute"),
            unit: Unit::Ratio,
            min: 0.0,
            max: 1.0,
        },
    }
}

fn find_processor<'a>(song: &'a Song, id: &str) -> Option<(&'a str, &'a crate::model::Processor)> {
    song.tracks
        .iter()
        .map(|t| (t.name.as_str(), t.channel.inserts.as_slice()))
        .chain(
            song.groups
                .iter()
                .map(|g| (g.name.as_str(), g.channel.inserts.as_slice())),
        )
        .chain(
            song.returns
                .iter()
                .map(|r| (r.name.as_str(), r.channel.inserts.as_slice())),
        )
        .chain(song.master.iter().map(|m| ("Master", m.inserts.as_slice())))
        .chain(
            song.instruments
                .iter()
                .map(|i| (i.name.as_str(), i.post_processors.as_slice())),
        )
        .find_map(|(host, list)| list.iter().find(|p| p.id == id).map(|p| (host, p)))
}

fn draw_automations_section(ui: &mut Ui, view: &SoundView<'_>, changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);

    let mut lane_ids: Vec<String> = Vec::new();
    for change in changes {
        if let Some(id) = change.path.first()
            && !lane_ids.contains(id)
        {
            lane_ids.push(id.clone());
        }
    }
    if lane_ids.is_empty() {
        lane_ids = song.automations.iter().map(|l| l.id.clone()).collect();
    }

    section(ui, Kind::Automation, "Automations", |ui| {
        let width = ui.available_width().max(260.0);
        for (idx, lane_id) in lane_ids.iter().enumerate() {
            if idx > 0 {
                ui.add_space(10.0);
            }
            let curr = song.automations.iter().find(|l| l.id == *lane_id);
            let prev = prev_song.and_then(|s| s.automations.iter().find(|l| l.id == *lane_id));
            let other = other_song.and_then(|s| s.automations.iter().find(|l| l.id == *lane_id));
            match (curr, prev) {
                (Some(lane), _) => {
                    draw_automation_lane_card(ui, song, lane, prev, other, view.other_word, width);
                }
                (None, Some(removed)) => {
                    let info = resolve_target(prev_song.unwrap_or(song), &removed.target);
                    setting_chip(
                        ui,
                        &format!("− {} ({}) removed", removed.id, info.title),
                        true,
                        false,
                    );
                }
                (None, None) => {}
            }
        }
    });
}

fn sample_automation_curve(
    lane: &AutomationLane,
    unit: Unit,
    min_val: f64,
    song_end: f64,
    x_of: &dyn Fn(f64) -> f32,
    y_of: &dyn Fn(f64) -> f32,
) -> Vec<Pos2> {
    let resolved: Vec<(f64, f64, AutomationInterpolation)> = lane
        .points
        .iter()
        .map(|pt| {
            (
                pt.at.as_f64(),
                unit.parse_value(&pt.value).unwrap_or(min_val),
                pt.interpolation,
            )
        })
        .collect();

    let mut pts = Vec::new();
    for seg in resolved.windows(2) {
        let (q0, v0, interp) = seg[0];
        let (q1, v1, _) = seg[1];
        match interp {
            AutomationInterpolation::Step => {
                pts.push(Pos2::new(x_of(q0), y_of(v0)));
                pts.push(Pos2::new(x_of(q1), y_of(v0)));
                pts.push(Pos2::new(x_of(q1), y_of(v1)));
            }
            AutomationInterpolation::Linear => {
                let steps = 20usize;
                for s in 0..=steps {
                    let t = s as f64 / steps as f64;
                    pts.push(Pos2::new(
                        x_of(q0 + t * (q1 - q0)),
                        y_of(v0 + t * (v1 - v0)),
                    ));
                }
            }
            AutomationInterpolation::Power(exp) => {
                let steps = 20usize;
                for s in 0..=steps {
                    let t = s as f64 / steps as f64;
                    let v = v0 + t.powf(exp) * (v1 - v0);
                    pts.push(Pos2::new(x_of(q0 + t * (q1 - q0)), y_of(v)));
                }
            }
        }
    }
    if let Some(&(last_q, last_v, _)) = resolved.last() {
        if pts.is_empty() {
            pts.push(Pos2::new(x_of(last_q), y_of(last_v)));
        }
        if last_q < song_end {
            pts.push(Pos2::new(x_of(song_end), y_of(last_v)));
        }
    }
    pts
}

fn draw_automation_lane_card(
    ui: &mut Ui,
    song: &Song,
    lane: &AutomationLane,
    prev: Option<&AutomationLane>,
    other: Option<&AutomationLane>,
    other_word: &str,
    width: f32,
) {
    let target = resolve_target(song, &lane.target);
    let unit = target.unit;
    let min_val = target.min;
    let max_val = target.max;

    let lane_edited = prev.is_some_and(|p| automation_lane_changed(p, lane))
        || (prev.is_none() && other.is_some());
    let lane_differs = !lane_edited && other.is_some_and(|o| automation_lane_changed(o, lane));

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&target.title)
                .strong()
                .size(11.5)
                .color(theme::TEXT_PRIMARY),
        );
        ui.label(
            RichText::new(format!("{} · {} pts", lane.id, lane.points.len()))
                .monospace()
                .size(9.5)
                .color(theme::TEXT_MUTED),
        );
    });
    ui.add_space(4.0);

    let axis_w = 56.0_f32;
    let ruler_h = 18.0_f32;
    let total_h = 132.0_f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, total_h), Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 4.0, theme::BG_CANVAS);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(
            if lane_edited { 1.3_f32 } else { 1.0_f32 },
            if lane_edited {
                theme::SELECTION
            } else if lane_differs {
                theme::BORDER_STRONG
            } else {
                theme::BORDER_SUBTLE
            },
        ),
        StrokeKind::Inside,
    );

    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + axis_w, rect.top() + ruler_h),
        Pos2::new(rect.right() - 8.0, rect.bottom() - 8.0),
    );
    let ruler = Rect::from_min_max(
        Pos2::new(plot.left(), rect.top() + 1.0),
        Pos2::new(rect.right() - 1.0, plot.top()),
    );
    let axis = Rect::from_min_max(
        Pos2::new(rect.left() + 1.0, plot.top()),
        Pos2::new(plot.left(), rect.bottom() - 1.0),
    );
    painter.rect_filled(
        Rect::from_min_max(
            rect.left_top() + Vec2::splat(1.0),
            Pos2::new(plot.left(), plot.top()),
        ),
        0.0,
        theme::BG_SURFACE,
    );
    painter.rect_filled(ruler, 0.0, theme::BG_SURFACE);
    painter.rect_filled(axis, 0.0, theme::BG_SURFACE);
    painter.hline(
        rect.x_range(),
        plot.top(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );
    painter.vline(
        plot.left(),
        rect.y_range(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );
    painter.text(
        Pos2::new(rect.left() + axis_w * 0.5, rect.top() + ruler_h * 0.5),
        Align2::CENTER_CENTER,
        format!("{unit}").to_uppercase(),
        FontId::monospace(8.5),
        theme::TEXT_MUTED,
    );

    let song_end = song.end.as_f64().max(16.0);
    let bar_len = song.meter.bar_length().map_or(4.0, |t| t.as_f64());
    let x_of = |q: f64| plot.left() + (q / song_end).clamp(0.0, 1.0) as f32 * plot.width();
    let y_top = plot.top() + 6.0;
    let y_bot = plot.bottom() - 6.0;
    let y_span = (y_bot - y_top).max(16.0);
    let y_of = |v: f64| y_bot - theme::value_to_norm(v, min_val, max_val, unit) * y_span;

    // Ghost target track clips behind the automation curve.
    let ghost_track = song
        .tracks
        .iter()
        .enumerate()
        .find(|(_, tr)| match &lane.target {
            ParameterTarget::Track { track, .. } => *track == tr.id,
            ParameterTarget::Instrument { instrument, .. } => *instrument == tr.instrument,
            _ => false,
        });
    if let Some((tr_idx, track)) = ghost_track {
        let tr_col = theme::track_color(tr_idx);
        for clip in &track.clips {
            let g = Rect::from_min_max(
                Pos2::new(x_of(clip.start.as_f64()), plot.top() + 1.0),
                Pos2::new(
                    x_of(clip.start.as_f64() + clip.duration.as_f64()),
                    plot.bottom() - 1.0,
                ),
            );
            painter.rect_filled(g, 2.0, with_alpha(theme::TEXT_PRIMARY, 0.025));
            painter.rect_stroke(
                g,
                2.0,
                Stroke::new(1.0_f32, with_alpha(tr_col, 0.12)),
                StrokeKind::Inside,
            );
        }
    }

    // Y-axis ticks (min, mid, max).
    let mid_val = theme::norm_to_value(0.5, min_val, max_val, unit);
    for v in [min_val, mid_val, max_val] {
        let y = y_of(v);
        painter.hline(plot.x_range(), y, Stroke::new(1.0_f32, theme::GRID_BEAT));
        painter.text(
            Pos2::new(plot.left() - 5.0, y),
            Align2::RIGHT_CENTER,
            theme::format_compact_value(v, unit),
            FontId::monospace(8.5),
            theme::TEXT_SECONDARY,
        );
    }

    // Vertical bar lines + ruler numerals.
    let bar_px = (bar_len / song_end) as f32 * plot.width();
    let label_every = theme::bar_label_interval(46.0 / bar_px.max(1.0));
    let total_bars = (song_end / bar_len).ceil() as u32;
    for bar in 0..=total_bars {
        let q = f64::from(bar) * bar_len;
        if q > song_end + 1e-4 {
            break;
        }
        let x = x_of(q);
        if bar.is_multiple_of(label_every) {
            painter.vline(x, plot.y_range(), Stroke::new(1.0_f32, theme::GRID_BAR));
            let fits_right = x + 28.0 <= ruler.right();
            painter.text(
                Pos2::new(if fits_right { x + 3.0 } else { x - 3.0 }, ruler.center().y),
                if fits_right {
                    Align2::LEFT_CENTER
                } else {
                    Align2::RIGHT_CENTER
                },
                format!("{}", bar + 1),
                FontId::monospace(8.5),
                theme::TEXT_SECONDARY,
            );
        }
    }

    // Ghost previous (or quiet other) curve.
    if let Some(p) = prev
        && automation_lane_changed(p, lane)
    {
        let gpts = sample_automation_curve(p, unit, min_val, song_end, &x_of, &y_of);
        if gpts.len() >= 2 {
            painter.add(egui::Shape::line(
                gpts,
                Stroke::new(1.5_f32, with_alpha(theme::TEXT_SECONDARY, 0.65)),
            ));
        }
        for ppt in &p.points {
            let cur_same = lane.points.iter().find(|cp| cp.id == ppt.id);
            let px = x_of(ppt.at.as_f64());
            let py = y_of(unit.parse_value(&ppt.value).unwrap_or(min_val));
            match cur_same {
                None => {
                    painter.circle_stroke(
                        Pos2::new(px, py),
                        3.5,
                        Stroke::new(1.3_f32, with_alpha(theme::SIG_CLIP, 0.8)),
                    );
                }
                Some(cp) if point_changed(ppt, cp) => {
                    painter.circle_stroke(
                        Pos2::new(px, py),
                        3.2,
                        Stroke::new(1.3_f32, with_alpha(theme::TEXT_SECONDARY, 0.85)),
                    );
                    let cx = x_of(cp.at.as_f64());
                    let cy = y_of(unit.parse_value(&cp.value).unwrap_or(min_val));
                    synth::paint_arrow(
                        &painter,
                        Pos2::new(px, py),
                        Pos2::new(cx, cy),
                        with_alpha(theme::SELECTION, 0.8),
                    );
                }
                _ => {}
            }
        }
    } else if let Some(o) = other
        && automation_lane_changed(o, lane)
    {
        let opts = sample_automation_curve(o, unit, min_val, song_end, &x_of, &y_of);
        if opts.len() >= 2 {
            painter.add(egui::Shape::line(
                opts,
                Stroke::new(1.2_f32, with_alpha(theme::BORDER_STRONG, 0.85)),
            ));
        }
    }

    // Current curve + area fill.
    let curve_col = Kind::Automation.color();
    let pts = sample_automation_curve(lane, unit, min_val, song_end, &x_of, &y_of);
    if pts.len() >= 2 {
        let fill = with_alpha(curve_col, 0.14);
        for w in pts.windows(2) {
            if (w[1].x - w[0].x).abs() > 0.2 {
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        w[0],
                        w[1],
                        Pos2::new(w[1].x, plot.bottom()),
                        Pos2::new(w[0].x, plot.bottom()),
                    ],
                    fill,
                    Stroke::NONE,
                ));
            }
        }
        painter.add(egui::Shape::line(pts, Stroke::new(2.0_f32, curve_col)));
    }

    // Point nodes.
    let mut point_notes = Vec::new();
    for pt in &lane.points {
        let v = unit.parse_value(&pt.value).unwrap_or(min_val);
        let pos = Pos2::new(x_of(pt.at.as_f64()), y_of(v));
        let pp = prev.and_then(|p| p.points.iter().find(|q| q.id == pt.id));
        let op = other.and_then(|o| o.points.iter().find(|q| q.id == pt.id));
        let added = prev.is_some() && pp.is_none();
        let edited = added || pp.is_some_and(|p| point_changed(p, pt));
        let differs = !edited && op.is_some_and(|o| point_changed(o, pt));

        let r = if edited { 4.2 } else { 3.2 };
        painter.circle_filled(
            pos,
            r,
            if edited {
                theme::SELECTION
            } else {
                theme::TEXT_PRIMARY
            },
        );
        painter.circle_stroke(
            pos,
            r,
            Stroke::new(
                1.3_f32,
                if edited {
                    theme::TEXT_PRIMARY
                } else if differs {
                    theme::BORDER_STRONG
                } else {
                    curve_col
                },
            ),
        );

        if added {
            point_notes.push((
                format!(
                    "+ {} {} at {}",
                    pt.id,
                    theme::format_compact_value(v, unit),
                    theme::format_bar_beat(pt.at.as_f64(), song.meter)
                ),
                true,
                false,
            ));
        } else if let Some(p) = pp
            && edited
        {
            let pv = unit.parse_value(&p.value).unwrap_or(min_val);
            point_notes.push((
                format!(
                    "{}: {} (prev {})",
                    pt.id,
                    theme::format_compact_value(v, unit),
                    theme::format_compact_value(pv, unit)
                ),
                true,
                false,
            ));
        } else if let Some(o) = op
            && differs
        {
            let ov = unit.parse_value(&o.value).unwrap_or(min_val);
            point_notes.push((
                format!(
                    "{}: {} ({other_word} {})",
                    pt.id,
                    theme::format_compact_value(v, unit),
                    theme::format_compact_value(ov, unit)
                ),
                false,
                true,
            ));
        }
    }

    if !point_notes.is_empty() {
        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
            for (text, lit, quiet) in point_notes {
                setting_chip(ui, &text, lit, quiet);
            }
        });
    }
}

fn point_changed(a: &AutomationPoint, b: &AutomationPoint) -> bool {
    a.at != b.at || a.value != b.value || a.interpolation != b.interpolation
}

fn automation_lane_changed(a: &AutomationLane, b: &AutomationLane) -> bool {
    a.target != b.target
        || a.points.len() != b.points.len()
        || a.points
            .iter()
            .zip(b.points.iter())
            .any(|(x, y)| x.id != y.id || point_changed(x, y))
}

// ── Song-Level Modulations ───────────────────────────────────────────────────

fn draw_song_modulations_section(ui: &mut Ui, view: &SoundView<'_>, _changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);

    section(ui, Kind::Other, "Modulations", |ui| {
        for route in &song.modulations {
            let pr = prev_song.and_then(|s| s.modulations.iter().find(|r| r.id == route.id));
            let or = other_song.and_then(|s| s.modulations.iter().find(|r| r.id == route.id));
            let added = prev_song.is_some() && pr.is_none();
            draw_song_mod_row(ui, song, route, pr, or, view.other_word, added);
            ui.add_space(4.0);
        }
        if let Some(ps) = prev_song {
            for rem in &ps.modulations {
                if !song.modulations.iter().any(|r| r.id == rem.id) {
                    setting_chip(ui, &format!("− {} removed", rem.id), true, false);
                }
            }
        }
    });
}

fn song_mod_changed(a: &ModulationRoute, b: &ModulationRoute) -> bool {
    a.scope != b.scope
        || a.polarity != b.polarity
        || a.waveform != b.waveform
        || a.rate != b.rate
        || a.target != b.target
        || a.depth != b.depth
        || a.operators != b.operators
}

fn draw_song_mod_row(
    ui: &mut Ui,
    song: &Song,
    route: &ModulationRoute,
    prev: Option<&ModulationRoute>,
    other: Option<&ModulationRoute>,
    other_word: &str,
    added: bool,
) {
    let target = resolve_target(song, &route.target);
    let max_depth = (target.max - target.min).abs().max(1.0);
    let curr_d = target.unit.parse_value(&route.depth).unwrap_or(0.0);
    let prev_d = prev.and_then(|r| target.unit.parse_value(&r.depth).ok());
    let other_d = other.and_then(|r| target.unit.parse_value(&r.depth).ok());
    let dm = mark(curr_d, prev_d, other_d);

    let edited = added || prev.is_some_and(|p| song_mod_changed(p, route));
    let differs = !edited && other.is_some_and(|o| song_mod_changed(o, route));

    let target_param = match &route.target {
        ParameterTarget::Instrument {
            instrument,
            parameter,
        } => song
            .instruments
            .iter()
            .find(|i| i.id == *instrument)
            .and_then(|i| i.parameters.iter().find(|p| p.key == *parameter)),
        _ => None,
    };

    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .stroke(Stroke::new(
            if edited { 1.3_f32 } else { 1.0_f32 },
            if edited {
                theme::SELECTION
            } else if differs {
                theme::BORDER_STRONG
            } else {
                theme::BORDER_SUBTLE
            },
        ))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                let rate_str = match &route.rate {
                    ModulationRate::FreeHertz(hz) => theme::format_compact_value(*hz, Unit::Hertz),
                    ModulationRate::Synchronized(t) => format!("{t}"),
                };
                let scope_str = match &route.scope {
                    ModulationScope::Global => "global".to_owned(),
                    ModulationScope::Instrument(i) => format!("inst:{i}"),
                    ModulationScope::PerVoice(i) => format!("voice:{i}"),
                };
                setting_chip(
                    ui,
                    &format!(
                        "{} {} · {scope_str}",
                        synth::lfo_waveform_name(route.waveform),
                        rate_str
                    ),
                    prev.is_some_and(|p| p.waveform != route.waveform || p.rate != route.rate),
                    false,
                );
                ui.label(RichText::new("→").color(theme::TEXT_MUTED));
                setting_chip(
                    ui,
                    &target.title,
                    prev.is_some_and(|p| p.target != route.target),
                    false,
                );
                let bar_w = ui.available_width().clamp(110.0, 260.0);
                let depth_lit = added || matches!(dm, Some(Mark::Edited(_)));
                synth::paint_bipolar_depth_bar(
                    ui,
                    curr_d,
                    max_depth,
                    target_param,
                    target.unit,
                    dm,
                    if depth_lit {
                        theme::SELECTION
                    } else {
                        theme::ACCENT_VIOLET
                    },
                    bar_w,
                    20.0,
                );
                let pol = match route.polarity {
                    ControlPolarity::Unipolar => "UNI",
                    ControlPolarity::Bipolar => "BI",
                };
                setting_chip(
                    ui,
                    pol,
                    prev.is_some_and(|p| p.polarity != route.polarity),
                    false,
                );
            });
            match dm {
                Some(Mark::Edited(p)) => {
                    ui.label(
                        RichText::new(format!(
                            "{} · depth previous {}",
                            route.id,
                            theme::format_compact_value(p, target.unit)
                        ))
                        .monospace()
                        .size(9.5)
                        .color(theme::SELECTION),
                    );
                }
                Some(Mark::Differs(o)) => {
                    ui.label(
                        RichText::new(format!(
                            "{} · depth {other_word} {}",
                            route.id,
                            theme::format_compact_value(o, target.unit)
                        ))
                        .monospace()
                        .size(9.5)
                        .color(theme::TEXT_MUTED),
                    );
                }
                None => {}
            }
        });
}

// ── Envelope Followers ───────────────────────────────────────────────────────

fn draw_envelope_followers_section(ui: &mut Ui, view: &SoundView<'_>, _changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);

    section(ui, Kind::Other, "Envelope Followers", |ui| {
        for ef in &song.envelope_followers {
            let pef = prev_song.and_then(|s| s.envelope_followers.iter().find(|r| r.id == ef.id));
            let oef = other_song.and_then(|s| s.envelope_followers.iter().find(|r| r.id == ef.id));
            let added = prev_song.is_some() && pef.is_none();
            draw_env_follower_row(ui, song, ef, pef, oef, view.other_word, added);
            ui.add_space(4.0);
        }
        if let Some(ps) = prev_song {
            for rem in &ps.envelope_followers {
                if !song.envelope_followers.iter().any(|r| r.id == rem.id) {
                    setting_chip(ui, &format!("− {} removed", rem.id), true, false);
                }
            }
        }
    });
}

fn ef_changed(a: &EnvelopeFollowerRoute, b: &EnvelopeFollowerRoute) -> bool {
    a.source != b.source
        || a.tap != b.tap
        || a.detector != b.detector
        || (a.attack_seconds - b.attack_seconds).abs() > 1e-6
        || (a.release_seconds - b.release_seconds).abs() > 1e-6
        || (a.floor_db - b.floor_db).abs() > 1e-6
        || (a.ceiling_db - b.ceiling_db).abs() > 1e-6
        || a.target != b.target
        || a.depth != b.depth
}

fn draw_env_follower_row(
    ui: &mut Ui,
    song: &Song,
    ef: &EnvelopeFollowerRoute,
    prev: Option<&EnvelopeFollowerRoute>,
    other: Option<&EnvelopeFollowerRoute>,
    other_word: &str,
    added: bool,
) {
    let target = resolve_target(song, &ef.target);
    let max_depth = (target.max - target.min).abs().max(1.0);
    let curr_d = target.unit.parse_value(&ef.depth).unwrap_or(0.0);
    let prev_d = prev.and_then(|r| target.unit.parse_value(&r.depth).ok());
    let other_d = other.and_then(|r| target.unit.parse_value(&r.depth).ok());
    let dm = mark(curr_d, prev_d, other_d);

    let edited = added || prev.is_some_and(|p| ef_changed(p, ef));
    let differs = !edited && other.is_some_and(|o| ef_changed(o, ef));

    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .stroke(Stroke::new(
            if edited { 1.3_f32 } else { 1.0_f32 },
            if edited {
                theme::SELECTION
            } else if differs {
                theme::BORDER_STRONG
            } else {
                theme::BORDER_SUBTLE
            },
        ))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                let src_name = channel_label(song, &ef.source);
                let tap = match ef.tap {
                    TapPoint::PreFader => "PRE",
                    TapPoint::PostFader => "POST",
                };
                let det = match ef.detector {
                    EnvelopeDetector::Peak => "PEAK",
                    EnvelopeDetector::Rms => "RMS",
                };
                setting_chip(
                    ui,
                    &format!("{src_name} · {tap} · {det}"),
                    prev.is_some_and(|p| {
                        p.source != ef.source || p.tap != ef.tap || p.detector != ef.detector
                    }),
                    false,
                );
                setting_chip(
                    ui,
                    &format!(
                        "A {:.0}ms R {:.0}ms · {:.0}..{:.0} dB",
                        ef.attack_seconds * 1000.0,
                        ef.release_seconds * 1000.0,
                        ef.floor_db,
                        ef.ceiling_db
                    ),
                    prev.is_some_and(|p| {
                        (p.attack_seconds - ef.attack_seconds).abs() > 1e-5
                            || (p.release_seconds - ef.release_seconds).abs() > 1e-5
                            || (p.floor_db - ef.floor_db).abs() > 1e-4
                            || (p.ceiling_db - ef.ceiling_db).abs() > 1e-4
                    }),
                    false,
                );
                ui.label(RichText::new("→").color(theme::TEXT_MUTED));
                setting_chip(
                    ui,
                    &target.title,
                    prev.is_some_and(|p| p.target != ef.target),
                    false,
                );
                let bar_w = ui.available_width().clamp(100.0, 220.0);
                let depth_lit = added || matches!(dm, Some(Mark::Edited(_)));
                synth::paint_bipolar_depth_bar(
                    ui,
                    curr_d,
                    max_depth,
                    None,
                    target.unit,
                    dm,
                    if depth_lit {
                        theme::SELECTION
                    } else {
                        theme::ACCENT_CYAN
                    },
                    bar_w,
                    20.0,
                );
            });
            if let Some(Mark::Edited(p)) = dm {
                ui.label(
                    RichText::new(format!(
                        "{} · depth previous {}",
                        ef.id,
                        theme::format_compact_value(p, target.unit)
                    ))
                    .monospace()
                    .size(9.5)
                    .color(theme::SELECTION),
                );
            } else if let Some(Mark::Differs(o)) = dm {
                ui.label(
                    RichText::new(format!(
                        "{} · depth {other_word} {}",
                        ef.id,
                        theme::format_compact_value(o, target.unit)
                    ))
                    .monospace()
                    .size(9.5)
                    .color(theme::TEXT_MUTED),
                );
            }
        });
}

// ── Sidechains ───────────────────────────────────────────────────────────────

fn channel_label(song: &Song, ch: &ChannelRef) -> String {
    match ch {
        ChannelRef::Track(id) => song
            .tracks
            .iter()
            .find(|t| t.id == *id)
            .map_or_else(|| id.clone(), |t| t.name.clone()),
        ChannelRef::Group(id) => song
            .groups
            .iter()
            .find(|g| g.id == *id)
            .map_or_else(|| id.clone(), |g| g.name.clone()),
        ChannelRef::Return(id) => song
            .returns
            .iter()
            .find(|r| r.id == *id)
            .map_or_else(|| id.clone(), |r| r.name.clone()),
    }
}

fn draw_sidechains_section(ui: &mut Ui, view: &SoundView<'_>, _changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);

    section(ui, Kind::Other, "Sidechains", |ui| {
        for sc in &song.sidechains {
            let psc = prev_song.and_then(|s| s.sidechains.iter().find(|r| r.id == sc.id));
            let osc = other_song.and_then(|s| s.sidechains.iter().find(|r| r.id == sc.id));
            let added = prev_song.is_some() && psc.is_none();
            let edited = added || psc.is_some_and(|p| sidechain_changed(p, sc));
            let differs = !edited && osc.is_some_and(|o| sidechain_changed(o, sc));

            let proc_title = find_processor(song, &sc.processor).map_or_else(
                || sc.processor.clone(),
                |(host, p)| format!("{} on {host}", theme::processor_title(p.kind)),
            );
            let tap = match sc.tap {
                TapPoint::PreFader => "PRE",
                TapPoint::PostFader => "POST",
            };
            egui::Frame::new()
                .fill(theme::BG_CANVAS)
                .stroke(Stroke::new(
                    if edited { 1.3_f32 } else { 1.0_f32 },
                    if edited {
                        theme::SELECTION
                    } else if differs {
                        theme::BORDER_STRONG
                    } else {
                        theme::BORDER_SUBTLE
                    },
                ))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(8, 5))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                        setting_chip(
                            ui,
                            &format!("{} · {tap}", channel_label(song, &sc.from)),
                            psc.is_some_and(|p| p.from != sc.from || p.tap != sc.tap) || added,
                            false,
                        );
                        ui.label(RichText::new("→").color(theme::ACCENT_CYAN));
                        setting_chip(
                            ui,
                            &format!("{proc_title} ({})", sc.input),
                            psc.is_some_and(|p| p.processor != sc.processor || p.input != sc.input)
                                || added,
                            false,
                        );
                    });
                });
            ui.add_space(4.0);
        }
        if let Some(ps) = prev_song {
            for rem in &ps.sidechains {
                if !song.sidechains.iter().any(|r| r.id == rem.id) {
                    setting_chip(ui, &format!("− {} removed", rem.id), true, false);
                }
            }
        }
    });
}

fn sidechain_changed(a: &SidechainRoute, b: &SidechainRoute) -> bool {
    a.from != b.from || a.tap != b.tap || a.processor != b.processor || a.input != b.input
}

// ── Locators Timeline Strip ──────────────────────────────────────────────────

fn draw_locators_section(ui: &mut Ui, view: &SoundView<'_>, _changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);
    let bar_len = song.meter.bar_length().map_or(4.0, |t| t.as_f64());

    section(ui, Kind::Song, "Locators", |ui| {
        let end_q = song
            .end
            .as_f64()
            .max(
                song.locators
                    .iter()
                    .map(|l| l.at.as_f64())
                    .fold(0.0_f64, f64::max),
            )
            .max(4.0);

        let width = ui.available_width().max(260.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 62.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, theme::BG_CANVAS);
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
            StrokeKind::Inside,
        );

        let inner = rect.shrink2(Vec2::new(10.0, 6.0));
        let ruler_y = inner.bottom() - 14.0;
        painter.hline(
            inner.x_range(),
            ruler_y,
            Stroke::new(1.5_f32, theme::BORDER_STRONG),
        );
        let x_of = |q: f64| inner.left() + (q / end_q).clamp(0.0, 1.0) as f32 * inner.width();

        // Bar ruler ticks.
        let bar_px = (bar_len / end_q) as f32 * inner.width();
        let label_every = theme::bar_label_interval(46.0 / bar_px.max(1.0));
        let total_bars = (end_q / bar_len).ceil() as u32;
        for bar in (0..=total_bars).step_by(label_every.max(1) as usize) {
            let q = f64::from(bar) * bar_len;
            if q > end_q + 1e-4 {
                break;
            }
            let x = x_of(q);
            painter.vline(
                x,
                (ruler_y - 3.0)..=(ruler_y + 3.0),
                Stroke::new(1.0_f32, theme::TEXT_MUTED),
            );
            painter.text(
                Pos2::new(x, inner.bottom()),
                Align2::CENTER_BOTTOM,
                format!("{}", bar + 1),
                FontId::monospace(8.5),
                theme::TEXT_MUTED,
            );
        }

        // Removed locators ghosted in muted red.
        if let Some(ps) = prev_song {
            for pl in &ps.locators {
                if !song.locators.iter().any(|l| l.id == pl.id) {
                    let x = x_of(pl.at.as_f64());
                    painter.vline(
                        x,
                        (inner.top() + 4.0)..=ruler_y,
                        Stroke::new(1.2_f32, with_alpha(theme::SIG_CLIP, 0.7)),
                    );
                }
            }
        }

        let mut notes = Vec::new();
        for loc in &song.locators {
            let pl = prev_song.and_then(|s| s.locators.iter().find(|l| l.id == loc.id));
            let ol = other_song.and_then(|s| s.locators.iter().find(|l| l.id == loc.id));
            let added = prev_song.is_some() && pl.is_none();
            let edited = added || pl.is_some_and(|p| p.at != loc.at || p.name != loc.name);
            let differs = !edited && ol.is_some_and(|o| o.at != loc.at || o.name != loc.name);

            let x = x_of(loc.at.as_f64());
            if let Some(p) = pl
                && p.at != loc.at
            {
                let px = x_of(p.at.as_f64());
                painter.vline(
                    px,
                    (inner.top() + 8.0)..=ruler_y,
                    Stroke::new(1.2_f32, with_alpha(theme::TEXT_SECONDARY, 0.65)),
                );
                synth::paint_arrow(
                    &painter,
                    Pos2::new(px, ruler_y - 10.0),
                    Pos2::new(x, ruler_y - 10.0),
                    with_alpha(theme::SELECTION, 0.85),
                );
            }

            let pin_col = if edited {
                theme::SELECTION
            } else if differs {
                theme::BORDER_STRONG
            } else {
                theme::ACCENT_CYAN
            };
            painter.vline(
                x,
                (inner.top() + 2.0)..=ruler_y,
                Stroke::new(if edited { 2.0_f32 } else { 1.2_f32 }, pin_col),
            );
            let fw = (loc.name.len() as f32 * 5.2 + 10.0).clamp(28.0, 76.0);
            let fx = if x + fw > inner.right() { x - fw } else { x };
            let flag = Rect::from_min_size(Pos2::new(fx, inner.top() + 2.0), Vec2::new(fw, 15.0));
            painter.rect_filled(
                flag,
                2.5,
                if edited {
                    theme::BG_ACTIVE
                } else {
                    theme::BG_CARD
                },
            );
            painter.rect_stroke(flag, 2.5, Stroke::new(1.0_f32, pin_col), StrokeKind::Inside);
            painter.text(
                flag.center(),
                Align2::CENTER_CENTER,
                &loc.name,
                FontId::proportional(8.5),
                if edited {
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_SECONDARY
                },
            );

            let bar_num = loc.at.as_f64() / bar_len.max(1e-6) + 1.0;
            if added {
                notes.push((
                    format!("+ {} at bar {}", loc.name, trim_num(bar_num)),
                    true,
                    false,
                ));
            } else if let Some(p) = pl
                && edited
            {
                let p_bar = p.at.as_f64() / bar_len.max(1e-6) + 1.0;
                notes.push((
                    format!(
                        "{}: bar {} (prev {} bar {})",
                        loc.name,
                        trim_num(bar_num),
                        p.name,
                        trim_num(p_bar)
                    ),
                    true,
                    false,
                ));
            } else if let Some(o) = ol
                && differs
            {
                let o_bar = o.at.as_f64() / bar_len.max(1e-6) + 1.0;
                notes.push((
                    format!(
                        "{}: bar {} ({} bar {})",
                        loc.name,
                        trim_num(bar_num),
                        view.other_word,
                        trim_num(o_bar)
                    ),
                    false,
                    true,
                ));
            }
        }
        if !notes.is_empty() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                for (text, lit, quiet) in notes {
                    setting_chip(ui, &text, lit, quiet);
                }
            });
        }
    });
}

// ── Grooves (Swing Grid + Dials) ─────────────────────────────────────────────

fn draw_grooves_section(ui: &mut Ui, view: &SoundView<'_>, changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);

    let mut ids: HashSet<&str> = changes
        .iter()
        .filter_map(|c| c.path.first().map(String::as_str))
        .collect();
    if ids.is_empty() {
        ids = song.grooves.iter().map(|g| g.id.as_str()).collect();
    }

    section(ui, Kind::Other, "Grooves", |ui| {
        for groove in song.grooves.iter().filter(|g| ids.contains(g.id.as_str())) {
            let pg = prev_song.and_then(|s| s.grooves.iter().find(|g| g.id == groove.id));
            let og = other_song.and_then(|s| s.grooves.iter().find(|g| g.id == groove.id));
            draw_groove_card(ui, groove, pg, og, view.other_word);
            ui.add_space(6.0);
        }
    });
}

fn draw_groove_card(
    ui: &mut Ui,
    groove: &GrooveTemplate,
    prev: Option<&GrooveTemplate>,
    other: Option<&GrooveTemplate>,
    other_word: &str,
) {
    let swing_edited = prev.is_some_and(|p| (p.swing - groove.swing).abs() > 1e-5);
    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .stroke(Stroke::new(
            if swing_edited { 1.3_f32 } else { 1.0_f32 },
            if swing_edited {
                theme::SELECTION
            } else {
                theme::BORDER_SUBTLE
            },
        ))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_header(ui, &groove.id.to_uppercase(), theme::ACCENT_AMBER);
            ui.add_space(6.0);

            // Visual 8-step swing grid comparing straight ticks vs swung odd subdivisions.
            let w = ui.available_width().clamp(180.0, 520.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, theme::BG_WELL);
            let inner = rect.shrink2(Vec2::new(10.0, 4.0));
            let pairs = 4usize;
            for pair in 0..pairs {
                let p0 = inner.left() + (pair as f32 / pairs as f32) * inner.width();
                let pw = inner.width() / pairs as f32;
                // Even beat (straight).
                painter.vline(
                    p0,
                    inner.y_range(),
                    Stroke::new(1.5_f32, theme::TEXT_SECONDARY),
                );
                // Straight 50% midpoint guide.
                painter.vline(
                    p0 + pw * 0.5,
                    inner.y_range(),
                    Stroke::new(1.0_f32, theme::GRID_SUB),
                );
                // Previous swung odd-subdivision tick (ghosted).
                if let Some(p) = prev
                    && swing_edited
                {
                    let px = p0 + (p.swing.clamp(0.1, 0.9) as f32) * pw;
                    painter.vline(
                        px,
                        (inner.top() + 4.0)..=(inner.bottom() - 4.0),
                        Stroke::new(1.5_f32, with_alpha(theme::TEXT_SECONDARY, 0.7)),
                    );
                }
                // Current swung odd-subdivision tick.
                let cx = p0 + (groove.swing.clamp(0.1, 0.9) as f32) * pw;
                painter.vline(
                    cx,
                    (inner.top() + 2.0)..=(inner.bottom() - 2.0),
                    Stroke::new(
                        2.0_f32,
                        if swing_edited {
                            theme::SELECTION
                        } else {
                            theme::ACCENT_AMBER
                        },
                    ),
                );
            }
            ui.add_space(6.0);

            ui.horizontal_wrapped(|ui| {
                dial(
                    ui,
                    "Swing",
                    groove.swing,
                    0.5,
                    0.75,
                    Unit::Ratio,
                    &|v| format!("{:.0}%", v * 100.0),
                    mark(groove.swing, prev.map(|p| p.swing), other.map(|o| o.swing)),
                    other_word,
                    theme::ACCENT_AMBER,
                );
                dial(
                    ui,
                    "Velocity",
                    groove.velocity,
                    0.0,
                    1.0,
                    Unit::Ratio,
                    &|v| format!("{:.0}%", v * 100.0),
                    mark(
                        groove.velocity,
                        prev.map(|p| p.velocity),
                        other.map(|o| o.velocity),
                    ),
                    other_word,
                    theme::ACCENT_AMBER,
                );
                dial(
                    ui,
                    "Prob",
                    groove.probability,
                    0.0,
                    1.0,
                    Unit::Ratio,
                    &|v| format!("{:.0}%", v * 100.0),
                    mark(
                        groove.probability,
                        prev.map(|p| p.probability),
                        other.map(|o| o.probability),
                    ),
                    other_word,
                    theme::ACCENT_AMBER,
                );
                ui.vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                        diff_chip(
                            ui,
                            "Grid",
                            &format!("{}", groove.grid),
                            prev.map(|p| format!("{}", p.grid)),
                            other.map(|o| format!("{}", o.grid)),
                            other_word,
                        );
                        diff_chip(
                            ui,
                            "Timing",
                            &format!("{}", groove.timing),
                            prev.map(|p| format!("{}", p.timing)),
                            other.map(|o| format!("{}", o.timing)),
                            other_word,
                        );
                        diff_chip(
                            ui,
                            "Seed",
                            &groove.seed.to_string(),
                            prev.map(|p| p.seed.to_string()),
                            other.map(|o| o.seed.to_string()),
                            other_word,
                        );
                    });
                });
            });
        });
}

// ── Song End Length Bar ──────────────────────────────────────────────────────

fn draw_song_end_section(
    ui: &mut Ui,
    view: &SoundView<'_>,
    song: &Song,
    prev_song: Option<&Song>,
    other_song: Option<&Song>,
) {
    let bar_len = song.meter.bar_length().map_or(4.0, |t| t.as_f64());
    let curr_q = song.end.as_f64();
    let prev_q = prev_song.map(|s| s.end.as_f64());
    let other_q = other_song.map(|s| s.end.as_f64());
    let max_q = curr_q
        .max(prev_q.unwrap_or(0.0))
        .max(other_q.unwrap_or(0.0))
        .max(4.0);
    let edited = prev_q.is_some_and(|p| (p - curr_q).abs() > 1e-4);

    section(ui, Kind::Song, "Song Length", |ui| {
        let w = ui.available_width().max(240.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 42.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, theme::BG_WELL);
        let inner = rect.shrink2(Vec2::new(8.0, 6.0));
        let x_of = |q: f64| inner.left() + (q / max_q).clamp(0.0, 1.0) as f32 * inner.width();

        if let Some(pq) = prev_q
            && edited
        {
            let pr = Rect::from_min_max(inner.left_top(), Pos2::new(x_of(pq), inner.bottom()));
            painter.rect_filled(pr, 3.0, with_alpha(theme::TEXT_SECONDARY, 0.16));
            painter.vline(
                x_of(pq),
                inner.y_range(),
                Stroke::new(2.0_f32, theme::TEXT_SECONDARY),
            );
        }
        let cr = Rect::from_min_max(inner.left_top(), Pos2::new(x_of(curr_q), inner.bottom()));
        painter.rect_filled(cr, 3.0, with_alpha(theme::ACCENT_CYAN, 0.25));
        painter.vline(
            x_of(curr_q),
            inner.y_range(),
            Stroke::new(
                2.0_f32,
                if edited {
                    theme::SELECTION
                } else {
                    theme::ACCENT_CYAN
                },
            ),
        );
        let curr_bars = curr_q / bar_len.max(1e-6);
        let label = if let Some(pq) = prev_q
            && edited
        {
            format!(
                "{} bars (previous {} bars)",
                trim_num(curr_bars),
                trim_num(pq / bar_len.max(1e-6))
            )
        } else if let Some(oq) = other_q
            && (oq - curr_q).abs() > 1e-4
        {
            format!(
                "{} bars ({} {} bars)",
                trim_num(curr_bars),
                view.other_word,
                trim_num(oq / bar_len.max(1e-6))
            )
        } else {
            format!("{} bars", trim_num(curr_bars))
        };
        painter.text(
            inner.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(11.0),
            theme::TEXT_PRIMARY,
        );
    });
}

fn trim_num(v: f64) -> String {
    let text = format!("{v:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DrumVoice, ValueSite};
    use crate::time::MusicalTime;

    fn site() -> ValueSite {
        ValueSite {
            file: std::path::PathBuf::new(),
            start: 0,
            end: 0,
            expected: String::new(),
        }
    }

    fn hit(at_num: i64, at_den: i64, voice: DrumVoice, vel: f64) -> PatternEvent {
        PatternEvent::Hit {
            at: MusicalTime::new(at_num, at_den).unwrap(),
            voice,
            velocity: vel,
            site: site(),
            at_site: site(),
            voice_site: site(),
            velocity_site: site(),
        }
    }

    #[test]
    fn equal_length_pattern_diff_pairs_positionally_for_moves_and_voice_changes() {
        let prev = vec![
            hit(0, 1, DrumVoice::Kick, 0.9),
            hit(1, 1, DrumVoice::Kick, 0.86),
        ];
        let shown = vec![
            hit(1, 2, DrumVoice::Snare, 0.72),
            hit(1, 1, DrumVoice::Kick, 0.86),
        ];
        let diff = diff_pattern_events(&shown, Some(&prev), None);
        assert!(diff.removed.is_empty());
        assert_eq!(diff.entries.len(), 2);
        assert!(!diff.entries[0].added);
        assert!(event_position_changed(
            diff.entries[0].shown,
            diff.entries[0].prev.unwrap()
        ));
        assert!(event_exact_eq(
            diff.entries[1].shown,
            diff.entries[1].prev.unwrap()
        ));
    }

    #[test]
    fn unequal_length_pattern_diff_marks_added_and_removed_events() {
        let prev = vec![
            hit(0, 1, DrumVoice::Kick, 0.9),
            hit(2, 1, DrumVoice::Snare, 0.8),
        ];
        let shown = vec![
            hit(0, 1, DrumVoice::Kick, 0.9),
            hit(3, 1, DrumVoice::Hat, 0.5),
            hit(7, 2, DrumVoice::Clap, 0.6),
        ];
        let diff = diff_pattern_events(&shown, Some(&prev), None);
        // First hit matches exactly; second hit pairs with nearest remaining prev; third is added.
        assert!(event_exact_eq(
            diff.entries[0].shown,
            diff.entries[0].prev.unwrap()
        ));
        assert!(diff.entries[2].added);
        assert!(diff.removed.is_empty());

        let shrunk = vec![hit(0, 1, DrumVoice::Kick, 0.9)];
        let diff2 = diff_pattern_events(&shrunk, Some(&prev), None);
        assert_eq!(diff2.removed.len(), 1);
    }

    #[test]
    fn automation_curve_sampling_handles_step_linear_and_power() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/deep-house-reference/project.crump");
        let project = crate::project::load_project(&path)
            .expect("load reference")
            .project
            .expect("parsed project");
        let lane = &project.song.automations[0];
        let target = resolve_target(&project.song, &lane.target);
        let pts = sample_automation_curve(
            lane,
            target.unit,
            target.min,
            project.song.end.as_f64(),
            &|q| q as f32,
            &|v| v as f32,
        );
        assert!(pts.len() >= lane.points.len());
    }
}
