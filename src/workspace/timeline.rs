//! Terminal-independent arrangement projection for the Timeline Production View.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{
    engine::{
        sample::decode_wav,
        schedule::{ClipEnvelope, RuntimeEvent, compile_schedules},
    },
    media::{AssetInventory, LoadedAsset},
    model::{Clip, Project, SampleDirection, SampleLoopRegion},
    time::{MusicalTime, parse_musical_time},
};

const PAN_STEPS_PER_WINDOW: u32 = 4;
const MAX_TIMELINE_ZOOM: u8 = 6;
const TIMELINE_GRIDS: [&str; 8] = [
    "1/16beat", "1/8beat", "1/4beat", "1/2beat", "1beat", "1bar", "2bar", "4bar",
];
const DEFAULT_TIMELINE_GRID: usize = 4;
const MAX_TIMELINE_JUMPS: usize = 256;
const WAVEFORM_BASE_BLOCK_FRAMES: u64 = 256;
const WAVEFORM_BASE_BLOCK_SIZE: usize = 256;
const WAVEFORM_ALGORITHM: &str = "crump-timeline-waveform-v1";

/// One transient location in the two-dimensional Timeline workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TimelinePosition {
    pub(super) at: MusicalTime,
    pub(super) track: usize,
}

impl Default for TimelinePosition {
    fn default() -> Self {
        Self {
            at: MusicalTime::ZERO,
            track: 0,
        }
    }
}

/// One normalized, exact half-open Timeline range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TimelineRange {
    pub(super) start: MusicalTime,
    pub(super) end: MusicalTime,
}

/// Searchable authored object resolved to a Timeline position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimelineSearchHit {
    pub(super) label: String,
    pub(super) position: TimelinePosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineSearch {
    query: String,
    hits: Vec<TimelineSearchHit>,
    selected: usize,
}

/// Transient Vim-like navigation and range-selection state.
#[derive(Debug, Clone)]
pub(super) struct TimelineState {
    position: TimelinePosition,
    grid: usize,
    visual_anchor: Option<MusicalTime>,
    last_visual: Option<TimelineRange>,
    marks: BTreeMap<char, TimelinePosition>,
    jumps: Vec<TimelinePosition>,
    jump_index: Option<usize>,
    search: Option<TimelineSearch>,
    follow_playhead: bool,
    cursor_armed: bool,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            position: TimelinePosition::default(),
            grid: DEFAULT_TIMELINE_GRID,
            visual_anchor: None,
            last_visual: None,
            marks: BTreeMap::new(),
            jumps: Vec::new(),
            jump_index: None,
            search: None,
            follow_playhead: false,
            cursor_armed: false,
        }
    }
}

impl TimelineState {
    pub(super) const fn position(&self) -> TimelinePosition {
        self.position
    }

    pub(super) fn set_track(&mut self, track: usize) {
        self.position.track = track;
    }

    pub(super) fn set_position(&mut self, position: TimelinePosition) {
        self.position = position;
    }

    pub(super) fn jump_to(&mut self, position: TimelinePosition) {
        let current = self.position;
        if self
            .jump_index
            .is_some_and(|index| index + 1 < self.jumps.len())
        {
            self.jumps.truncate(self.jump_index.unwrap_or(0) + 1);
        }
        if self.jumps.last().copied() != Some(current) {
            self.jumps.push(current);
        }
        if self.jumps.last().copied() != Some(position) {
            self.jumps.push(position);
        }
        if self.jumps.len() > MAX_TIMELINE_JUMPS {
            self.jumps.drain(..self.jumps.len() - MAX_TIMELINE_JUMPS);
        }
        self.jump_index = self.jumps.len().checked_sub(1);
        self.position = position;
    }

    pub(super) fn remember_for_return(&mut self) {
        if self.jumps.last().copied() != Some(self.position) {
            self.jumps.push(self.position);
        }
        if self.jumps.len() > MAX_TIMELINE_JUMPS {
            self.jumps.drain(..self.jumps.len() - MAX_TIMELINE_JUMPS);
        }
        self.jump_index = Some(self.jumps.len());
    }

    pub(super) fn jump_back(&mut self) -> Option<TimelinePosition> {
        let index = self.jump_index?;
        let previous = index.checked_sub(1)?;
        self.jump_index = Some(previous);
        self.position = self.jumps[previous];
        Some(self.position)
    }

    pub(super) fn jump_forward(&mut self) -> Option<TimelinePosition> {
        let index = self.jump_index?;
        let next = index.checked_add(1)?;
        let position = *self.jumps.get(next)?;
        self.jump_index = Some(next);
        self.position = position;
        Some(position)
    }

    pub(super) fn grid(&self, project: &Project) -> MusicalTime {
        parse_musical_time(TIMELINE_GRIDS[self.grid], project.song.meter)
            .unwrap_or_else(|_| MusicalTime::whole(1))
    }

    pub(super) fn grid_label(&self) -> &'static str {
        TIMELINE_GRIDS[self.grid]
    }

    pub(super) fn change_grid(&mut self, direction: isize) -> bool {
        let previous = self.grid;
        self.grid = self
            .grid
            .saturating_add_signed(direction.signum())
            .min(TIMELINE_GRIDS.len().saturating_sub(1));
        self.grid != previous
    }

    pub(super) const fn visual(&self) -> bool {
        self.visual_anchor.is_some()
    }

    pub(super) fn begin_visual(&mut self) {
        self.visual_anchor = Some(self.position.at);
    }

    pub(super) fn cancel_visual(&mut self) {
        self.visual_anchor = None;
    }

    pub(super) fn swap_visual_edge(&mut self) -> bool {
        let Some(anchor) = self.visual_anchor.as_mut() else {
            return false;
        };
        std::mem::swap(anchor, &mut self.position.at);
        true
    }

    pub(super) fn visual_range(&self) -> Option<TimelineRange> {
        let anchor = self.visual_anchor?;
        (anchor != self.position.at).then_some(TimelineRange {
            start: anchor.min(self.position.at),
            end: anchor.max(self.position.at),
        })
    }

    pub(super) fn commit_visual(&mut self) -> Option<TimelineRange> {
        let range = self.visual_range()?;
        self.last_visual = Some(range);
        self.visual_anchor = None;
        Some(range)
    }

    pub(super) fn restore_visual(&mut self) -> bool {
        let Some(range) = self.last_visual else {
            return false;
        };
        self.visual_anchor = Some(range.start);
        self.position.at = range.end;
        true
    }

    pub(super) fn set_mark(&mut self, mark: char) {
        self.marks.insert(mark.to_ascii_lowercase(), self.position);
    }

    pub(super) fn mark(&self, mark: char) -> Option<TimelinePosition> {
        self.marks.get(&mark.to_ascii_lowercase()).copied()
    }

    pub(super) fn set_search(
        &mut self,
        query: String,
        hits: Vec<TimelineSearchHit>,
    ) -> Option<TimelineSearchHit> {
        let Some(first) = hits.first().cloned() else {
            self.search = None;
            return None;
        };
        self.search = Some(TimelineSearch {
            query,
            hits,
            selected: 0,
        });
        Some(first)
    }

    pub(super) fn repeat_search(&mut self, direction: isize) -> Option<TimelineSearchHit> {
        let search = self.search.as_mut()?;
        if search.hits.is_empty() {
            return None;
        }
        search.selected = if direction < 0 {
            search
                .selected
                .checked_sub(1)
                .unwrap_or(search.hits.len().saturating_sub(1))
        } else {
            (search.selected + 1) % search.hits.len()
        };
        search.hits.get(search.selected).cloned()
    }

    pub(super) fn search_summary(&self) -> Option<(&str, usize, usize)> {
        self.search.as_ref().map(|search| {
            (
                search.query.as_str(),
                search.selected.saturating_add(1),
                search.hits.len(),
            )
        })
    }

    pub(super) const fn follow_playhead(&self) -> bool {
        self.follow_playhead
    }

    pub(super) fn toggle_follow_playhead(&mut self) -> bool {
        self.follow_playhead = !self.follow_playhead;
        self.follow_playhead
    }

    pub(super) const fn cursor_armed(&self) -> bool {
        self.cursor_armed
    }

    pub(super) fn arm_cursor(&mut self) {
        self.cursor_armed = true;
    }

    pub(super) fn disarm_cursor(&mut self) {
        self.cursor_armed = false;
    }

    pub(super) fn clamp(&mut self, project: &Project) {
        self.position.at = self.position.at.clamp(MusicalTime::ZERO, project.song.end);
        self.position.track = self
            .position
            .track
            .min(project.song.tracks.len().saturating_sub(1));
        if let Some(anchor) = self.visual_anchor.as_mut() {
            *anchor = (*anchor).clamp(MusicalTime::ZERO, project.song.end);
        }
        self.last_visual = self.last_visual.and_then(|range| {
            let range = TimelineRange {
                start: range.start.clamp(MusicalTime::ZERO, project.song.end),
                end: range.end.clamp(MusicalTime::ZERO, project.song.end),
            };
            (range.start < range.end).then_some(range)
        });
        self.search = None;
        self.marks.retain(|_, position| {
            position.at <= project.song.end && position.track < project.song.tracks.len()
        });
        self.jumps.retain(|position| {
            position.at <= project.song.end && position.track < project.song.tracks.len()
        });
        self.jump_index = self
            .jump_index
            .and_then(|index| (!self.jumps.is_empty()).then_some(index.min(self.jumps.len() - 1)));
    }
}

/// Transient, non-authoritative horizontal Timeline viewport.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TimelineViewport {
    zoom: u8,
    offset_steps: u32,
}

impl TimelineViewport {
    pub(super) fn fit(&mut self) {
        *self = Self::default();
    }

    pub(super) fn zoom_in(&mut self) -> bool {
        if self.zoom >= MAX_TIMELINE_ZOOM {
            return false;
        }
        self.offset_steps = self.offset_steps.saturating_mul(2).saturating_add(2);
        self.zoom += 1;
        self.clamp();
        true
    }

    pub(super) fn zoom_out(&mut self) -> bool {
        if self.zoom == 0 {
            return false;
        }
        self.offset_steps = self.offset_steps.saturating_add(2) / 2;
        self.offset_steps = self.offset_steps.saturating_sub(2);
        self.zoom -= 1;
        self.clamp();
        true
    }

    pub(super) fn pan(&mut self, direction: isize) -> bool {
        self.pan_steps(direction.signum())
    }

    pub(super) fn pan_window(&mut self, direction: isize) -> bool {
        self.pan_steps(
            direction
                .signum()
                .saturating_mul(PAN_STEPS_PER_WINDOW as isize),
        )
    }

    fn pan_steps(&mut self, steps: isize) -> bool {
        let previous = self.offset_steps;
        self.offset_steps =
            self.offset_steps
                .saturating_add_signed(i32::try_from(steps).unwrap_or(if steps < 0 {
                    i32::MIN
                } else {
                    i32::MAX
                }));
        self.clamp();
        self.offset_steps != previous
    }

    pub(super) fn center_on(&mut self, time: MusicalTime, song_end: MusicalTime) {
        self.align_on(time, song_end, PAN_STEPS_PER_WINDOW / 2);
    }

    pub(super) fn align_start(&mut self, time: MusicalTime, song_end: MusicalTime) {
        self.align_on(time, song_end, 0);
    }

    pub(super) fn align_end(&mut self, time: MusicalTime, song_end: MusicalTime) {
        self.align_on(time, song_end, PAN_STEPS_PER_WINDOW);
    }

    pub(super) fn reveal(&mut self, time: MusicalTime, song_end: MusicalTime) {
        let window = self.window(song_end);
        if time < window.start {
            self.align_start(time, song_end);
        } else if time > window.end || time == song_end {
            self.align_end(time, song_end);
        }
    }

    pub(super) fn center_frame(&mut self, frame: u64, song_end_frame: u64) {
        if song_end_frame == 0 {
            return;
        }
        let total_steps = PAN_STEPS_PER_WINDOW.saturating_mul(self.zoom_factor());
        let step = u32::try_from(
            u128::from(frame.min(song_end_frame)).saturating_mul(u128::from(total_steps))
                / u128::from(song_end_frame),
        )
        .unwrap_or(total_steps);
        self.offset_steps = step.saturating_sub(PAN_STEPS_PER_WINDOW / 2);
        self.clamp();
    }

    pub(super) fn fit_range(&mut self, range: TimelineRange, song_end: MusicalTime) {
        if range.end <= range.start || song_end <= MusicalTime::ZERO {
            return;
        }
        for zoom in (0..=MAX_TIMELINE_ZOOM).rev() {
            self.zoom = zoom;
            self.center_on(
                midpoint(range.start, range.end).unwrap_or(range.start),
                song_end,
            );
            let window = self.window(song_end);
            if window.start <= range.start && window.end >= range.end {
                return;
            }
        }
        self.fit();
    }

    fn align_on(&mut self, time: MusicalTime, song_end: MusicalTime, anchor_steps: u32) {
        let total_steps = PAN_STEPS_PER_WINDOW.saturating_mul(self.zoom_factor());
        let step = time_fraction_steps(time, song_end, total_steps);
        self.offset_steps = step.saturating_sub(anchor_steps);
        self.clamp();
    }

    pub(super) fn zoom_factor(self) -> u32 {
        1_u32 << self.zoom
    }

    pub(super) fn is_fit(self) -> bool {
        self.zoom == 0
    }

    pub(super) fn window(self, song_end: MusicalTime) -> TimelineWindow {
        let denominator = PAN_STEPS_PER_WINDOW.saturating_mul(self.zoom_factor());
        let start =
            scale_time(song_end, self.offset_steps, denominator).unwrap_or(MusicalTime::ZERO);
        let end = scale_time(
            song_end,
            self.offset_steps.saturating_add(PAN_STEPS_PER_WINDOW),
            denominator,
        )
        .unwrap_or(song_end)
        .min(song_end);
        TimelineWindow { start, end }
    }

    fn clamp(&mut self) {
        self.zoom = self.zoom.min(MAX_TIMELINE_ZOOM);
        let total_steps = PAN_STEPS_PER_WINDOW.saturating_mul(self.zoom_factor());
        self.offset_steps = self
            .offset_steps
            .min(total_steps.saturating_sub(PAN_STEPS_PER_WINDOW));
    }
}

fn time_fraction_steps(time: MusicalTime, end: MusicalTime, total_steps: u32) -> u32 {
    if time <= MusicalTime::ZERO || end <= MusicalTime::ZERO {
        return 0;
    }
    let numerator = i128::from(time.numerator())
        .saturating_mul(i128::from(end.denominator()))
        .saturating_mul(i128::from(total_steps));
    let denominator = i128::from(time.denominator()).saturating_mul(i128::from(end.numerator()));
    if denominator <= 0 {
        return 0;
    }
    u32::try_from((numerator / denominator).max(0)).unwrap_or(total_steps)
}

fn midpoint(start: MusicalTime, end: MusicalTime) -> Option<MusicalTime> {
    let sum = start.checked_add(end).ok()?;
    MusicalTime::new(sum.numerator(), sum.denominator().saturating_mul(2)).ok()
}

fn multiply_time(left: MusicalTime, right: MusicalTime) -> Option<MusicalTime> {
    let numerator = i128::from(left.numerator()).checked_mul(i128::from(right.numerator()))?;
    let denominator =
        i128::from(left.denominator()).checked_mul(i128::from(right.denominator()))?;
    MusicalTime::new(
        i64::try_from(numerator).ok()?,
        i64::try_from(denominator).ok()?,
    )
    .ok()
}

/// Resolves one Timeline command destination without changing authored state.
pub(super) fn parse_destination(
    value: &str,
    current: MusicalTime,
    project: &Project,
) -> Result<MusicalTime, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Enter a bar, bar:beat, Locator ID, or relative musical duration".to_owned());
    }
    if let Some(locator) = project
        .song
        .locators
        .iter()
        .find(|locator| locator.id == value)
    {
        return Ok(locator.at);
    }
    if let Some(relative) = value.strip_prefix('+') {
        let amount = parse_musical_time(relative, project.song.meter)
            .map_err(|error| format!("Invalid relative Timeline amount: {error}"))?;
        return current
            .checked_add(amount)
            .map(|time| time.min(project.song.end))
            .map_err(|error| format!("Timeline destination overflowed: {error}"));
    }
    if let Some(relative) = value.strip_prefix('-') {
        let amount = parse_musical_time(relative, project.song.meter)
            .map_err(|error| format!("Invalid relative Timeline amount: {error}"))?;
        return Ok(current
            .checked_sub(amount)
            .unwrap_or(MusicalTime::ZERO)
            .max(MusicalTime::ZERO));
    }
    if let Some((bar, beat)) = value.split_once(':') {
        let bar = bar
            .parse::<u64>()
            .map_err(|_| "Timeline bar must be a positive integer".to_owned())?;
        if bar == 0 {
            return Err("Displayed Timeline bars begin at 1".to_owned());
        }
        let beat = parse_musical_time(beat, project.song.meter)
            .map_err(|error| format!("Invalid Timeline beat: {error}"))?;
        let one = MusicalTime::whole(1);
        if beat < one || beat > MusicalTime::whole(i64::from(project.song.meter.numerator)) {
            return Err(format!(
                "Timeline beat must lie in 1..={}",
                project.song.meter.numerator
            ));
        }
        let bar_start = project
            .song
            .meter
            .bar_length()
            .map_err(|error| error.to_string())?
            .checked_mul(bar - 1)
            .map_err(|error| error.to_string())?;
        let beat_index = beat.checked_sub(one).map_err(|error| error.to_string())?;
        let beat_length = MusicalTime::new(4, i64::from(project.song.meter.denominator))
            .map_err(|error| error.to_string())?;
        let beat_offset = multiply_time(beat_index, beat_length)
            .ok_or_else(|| "Timeline beat overflowed".to_owned())?;
        return bar_start
            .checked_add(beat_offset)
            .map(|time| time.min(project.song.end))
            .map_err(|error| error.to_string());
    }
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        let bar = value
            .parse::<u64>()
            .map_err(|_| "Timeline bar must be a positive integer".to_owned())?;
        if bar == 0 {
            return Err("Displayed Timeline bars begin at 1".to_owned());
        }
        return project
            .song
            .meter
            .bar_length()
            .map_err(|error| error.to_string())?
            .checked_mul(bar - 1)
            .map(|time| time.min(project.song.end))
            .map_err(|error| error.to_string());
    }
    parse_musical_time(value, project.song.meter)
        .map(|time| time.clamp(MusicalTime::ZERO, project.song.end))
        .map_err(|error| format!("Unknown Locator or invalid musical time `{value}`: {error}"))
}

pub(super) fn clip_at(project: &Project, position: TimelinePosition) -> Option<(usize, &Clip)> {
    let track = project.song.tracks.get(position.track)?;
    track
        .clips
        .iter()
        .find(|clip| {
            clip.start <= position.at
                && clip
                    .start
                    .checked_add(clip.duration)
                    .is_ok_and(|end| position.at < end)
        })
        .or_else(|| {
            track.clips.iter().find(|clip| {
                clip.start == position.at
                    || clip
                        .start
                        .checked_add(clip.duration)
                        .is_ok_and(|end| end == position.at)
            })
        })
        .map(|clip| (position.track, clip))
}

pub(super) fn adjacent_boundary(
    project: &Project,
    position: TimelinePosition,
    direction: isize,
) -> Option<MusicalTime> {
    let track = project.song.tracks.get(position.track)?;
    let mut boundaries = track
        .clips
        .iter()
        .flat_map(|clip| {
            std::iter::once(clip.start).chain(clip.start.checked_add(clip.duration).ok())
        })
        .chain(project.song.locators.iter().map(|locator| locator.at))
        .collect::<Vec<_>>();
    boundaries.sort_unstable();
    boundaries.dedup();
    if direction < 0 {
        boundaries
            .into_iter()
            .rev()
            .find(|boundary| *boundary < position.at)
    } else {
        boundaries
            .into_iter()
            .find(|boundary| *boundary > position.at)
    }
}

pub(super) fn adjacent_locator(
    project: &Project,
    position: TimelinePosition,
    direction: isize,
) -> Option<MusicalTime> {
    let mut locators = project
        .song
        .locators
        .iter()
        .map(|locator| locator.at)
        .collect::<Vec<_>>();
    locators.sort_unstable();
    locators.dedup();
    if direction < 0 {
        locators
            .into_iter()
            .rev()
            .find(|locator| *locator < position.at)
    } else {
        locators.into_iter().find(|locator| *locator > position.at)
    }
}

pub(super) fn track_material_edge(
    project: &Project,
    track_index: usize,
    end: bool,
) -> Option<MusicalTime> {
    let track = project.song.tracks.get(track_index)?;
    if end {
        track
            .clips
            .iter()
            .filter_map(|clip| clip.start.checked_add(clip.duration).ok())
            .max()
    } else {
        track.clips.iter().map(|clip| clip.start).min()
    }
}

pub(super) fn matching_pattern_clip(
    project: &Project,
    position: TimelinePosition,
    direction: isize,
) -> Option<TimelinePosition> {
    let (_, selected) = clip_at(project, position)?;
    let mut clips = project
        .song
        .tracks
        .iter()
        .enumerate()
        .flat_map(|(track, lane)| {
            lane.clips
                .iter()
                .filter(|clip| clip.pattern == selected.pattern)
                .map(move |clip| TimelinePosition {
                    at: clip.start,
                    track,
                })
        })
        .collect::<Vec<_>>();
    clips.sort_by(|left, right| left.at.cmp(&right.at).then(left.track.cmp(&right.track)));
    if direction < 0 {
        clips.into_iter().rev().find(|candidate| {
            candidate.at < position.at
                || (candidate.at == position.at && candidate.track < position.track)
        })
    } else {
        clips.into_iter().find(|candidate| {
            candidate.at > position.at
                || (candidate.at == position.at && candidate.track > position.track)
        })
    }
}

pub(super) fn search_project(project: &Project, query: &str) -> Vec<TimelineSearchHit> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let matches = |value: &str| value.to_lowercase().contains(&query);
    let mut hits = Vec::new();
    for (track_index, track) in project.song.tracks.iter().enumerate() {
        let track_position = TimelinePosition {
            at: track
                .clips
                .iter()
                .map(|clip| clip.start)
                .min()
                .unwrap_or(MusicalTime::ZERO),
            track: track_index,
        };
        if matches(&track.id) || matches(&track.name) {
            hits.push(TimelineSearchHit {
                label: format!("Track {} ({})", track.id, track.name),
                position: track_position,
            });
        }
        if let Some(instrument) = project
            .song
            .instruments
            .iter()
            .find(|instrument| instrument.id == track.instrument)
            && (matches(&instrument.id) || matches(&instrument.name))
        {
            hits.push(TimelineSearchHit {
                label: format!("Instrument {} on {}", instrument.id, track.id),
                position: track_position,
            });
        }
        for clip in &track.clips {
            if matches(&clip.id) || matches(&clip.pattern) {
                hits.push(TimelineSearchHit {
                    label: format!(
                        "Clip {} · Pattern {} · Track {}",
                        clip.id, clip.pattern, track.id
                    ),
                    position: TimelinePosition {
                        at: clip.start,
                        track: track_index,
                    },
                });
            }
        }
    }
    for locator in &project.song.locators {
        if matches(&locator.id) || matches(&locator.name) {
            hits.push(TimelineSearchHit {
                label: format!("Locator {} ({})", locator.id, locator.name),
                position: TimelinePosition {
                    at: locator.at,
                    track: 0,
                },
            });
        }
    }
    hits.sort_by(|left, right| {
        left.position
            .at
            .cmp(&right.position.at)
            .then(left.position.track.cmp(&right.position.track))
            .then(left.label.cmp(&right.label))
    });
    hits.dedup_by(|left, right| left.position == right.position && left.label == right.label);
    hits
}

fn scale_time(value: MusicalTime, numerator: u32, denominator: u32) -> Option<MusicalTime> {
    let numerator = i128::from(value.numerator()).checked_mul(i128::from(numerator))?;
    let denominator = i128::from(value.denominator()).checked_mul(i128::from(denominator))?;
    MusicalTime::new(
        i64::try_from(numerator).ok()?,
        i64::try_from(denominator).ok()?,
    )
    .ok()
}

/// One exact half-open viewport in authored Musical Time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct TimelineWindow {
    pub(super) start: MusicalTime,
    pub(super) end: MusicalTime,
}

impl TimelineWindow {
    fn is_valid(self) -> bool {
        self.end > self.start
    }
}

/// Semantic grid line projected into one terminal column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum TimelineGridMark {
    #[default]
    None,
    Beat,
    Bar,
}

/// Aggregated visual facts for one Track and terminal column.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct TimelineCell {
    pub(super) clip_depth: u16,
    pub(super) clip_start: bool,
    pub(super) clip_end: bool,
    pub(super) activity: f32,
    pub(super) sustained: f32,
    pub(super) sample_active: bool,
    pub(super) sample_peak: f32,
}

/// One presentation-ordered Track lane.
#[derive(Debug, Clone)]
pub(super) struct TimelineLane {
    pub(super) track_id: String,
    pub(super) cells: Vec<TimelineCell>,
}

/// Cached projection of one viewport and terminal width.
#[derive(Debug, Clone)]
pub(super) struct TimelineProjection {
    pub(super) window: TimelineWindow,
    pub(super) ruler: String,
    pub(super) grid: Vec<TimelineGridMark>,
    pub(super) locator_columns: BTreeSet<usize>,
    pub(super) lanes: Vec<TimelineLane>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProjectionKey {
    window: TimelineWindow,
    columns: usize,
}

#[derive(Debug, Clone)]
struct CachedProjection {
    key: ProjectionKey,
    projection: Arc<TimelineProjection>,
}

/// Revision-derived Timeline material and exact-asset waveform envelopes.
///
/// WAV decoding and schedule materialization happen during Candidate refresh. The
/// 40 ms draw path only bins immutable facts, and repeats of the same viewport
/// reuse the last projection.
#[derive(Debug, Default)]
pub(super) struct TimelineCache {
    tracks: BTreeMap<String, Vec<TimelineEvent>>,
    waveforms: BTreeMap<String, WaveformPyramid>,
    warning: Option<String>,
    projection: RefCell<Option<CachedProjection>>,
}

impl TimelineCache {
    pub(super) fn rebuild(&mut self, project: Option<&Project>, assets: &AssetInventory) {
        self.tracks.clear();
        self.warning = None;
        self.projection.replace(None);
        let Some(project) = project else {
            self.waveforms.clear();
            self.warning = Some("activity unavailable for invalid Candidate".to_owned());
            return;
        };

        match timeline_events(project, assets) {
            Ok(material) => {
                self.tracks = material.tracks;
                let required_assets = material.required_assets;
                self.waveforms
                    .retain(|sha256, _| required_assets.contains_key(sha256));
                let mut errors = Vec::new();
                for (sha256, asset) in required_assets {
                    if self.waveforms.contains_key(&sha256) {
                        continue;
                    }
                    match WaveformPyramid::build(asset) {
                        Ok(waveform) => {
                            self.waveforms.insert(sha256, waveform);
                        }
                        Err(error) => errors.push(error),
                    }
                }
                if !errors.is_empty() {
                    self.warning = Some(format!(
                        "{} sample waveform{} unavailable",
                        errors.len(),
                        if errors.len() == 1 { " is" } else { "s are" }
                    ));
                }
            }
            Err(error) => {
                self.waveforms.clear();
                self.warning = Some(format!("activity unavailable: {error}"));
            }
        }
    }

    pub(super) fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub(super) fn project(
        &self,
        project: &Project,
        window: TimelineWindow,
        columns: usize,
    ) -> Arc<TimelineProjection> {
        let key = ProjectionKey { window, columns };
        if let Some(cached) = self.projection.borrow().as_ref()
            && cached.key == key
        {
            return Arc::clone(&cached.projection);
        }
        let projection = Arc::new(project_timeline(
            project,
            &self.tracks,
            &self.waveforms,
            window,
            columns,
        ));
        self.projection.replace(Some(CachedProjection {
            key,
            projection: Arc::clone(&projection),
        }));
        projection
    }
}

#[derive(Debug, Clone)]
struct TimelineEvent {
    clip_id: String,
    start_frame: u64,
    end_frame: u64,
    kind: TimelineEventKind,
}

#[derive(Debug, Clone)]
enum TimelineEventKind {
    Note {
        strength: f32,
    },
    Hit {
        strength: f32,
    },
    Sample {
        waveform_key: Option<String>,
        source_rate: u32,
        source_start: u64,
        source_end: u64,
        direction: SampleDirection,
        loop_region: Option<SampleLoopRegion>,
        fade_in_frames: u32,
        fade_out_frames: u32,
        clip_envelope: ClipEnvelope,
    },
}

struct TimelineMaterial<'a> {
    tracks: BTreeMap<String, Vec<TimelineEvent>>,
    required_assets: BTreeMap<String, &'a LoadedAsset>,
}

fn timeline_events<'a>(
    project: &Project,
    assets: &'a AssetInventory,
) -> Result<TimelineMaterial<'a>, String> {
    let schedules = compile_schedules(project).map_err(|error| error.to_string())?;
    let mut tracks = BTreeMap::new();
    let mut required_assets = BTreeMap::new();
    for schedule in schedules {
        let track = project
            .song
            .tracks
            .iter()
            .find(|track| track.id == schedule.track_id)
            .ok_or_else(|| format!("schedule references missing Track `{}`", schedule.track_id))?;
        let instrument = project
            .song
            .instruments
            .iter()
            .find(|instrument| instrument.id == track.instrument)
            .ok_or_else(|| {
                format!(
                    "Track `{}` references missing Instrument `{}`",
                    track.id, track.instrument
                )
            })?;
        let mappings = instrument
            .sampler
            .as_ref()
            .map(|sampler| sampler.mappings.as_slice())
            .unwrap_or_default();
        let mut note_ends = BTreeMap::new();
        let mut sample_ends = BTreeMap::new();
        for event in &schedule.events {
            match event.kind {
                RuntimeEvent::NoteOff { note_id } => {
                    note_ends.insert(note_id, event.frame);
                }
                RuntimeEvent::SampleOff { note_id } => {
                    sample_ends.insert(note_id, event.frame);
                }
                RuntimeEvent::NoteOn { .. }
                | RuntimeEvent::Hit { .. }
                | RuntimeEvent::SampleOn { .. } => {}
            }
        }

        let mut projected = Vec::new();
        for event in &schedule.events {
            let clip_id = event.clip_id().to_owned();
            match &event.kind {
                RuntimeEvent::NoteOn {
                    note_id,
                    velocity,
                    clip_envelope,
                    ..
                } => projected.push(TimelineEvent {
                    clip_id,
                    start_frame: event.frame,
                    end_frame: note_ends
                        .get(note_id)
                        .copied()
                        .unwrap_or_else(|| event.frame.saturating_add(1)),
                    kind: TimelineEventKind::Note {
                        strength: (*velocity * clip_envelope.gain_at(0)).max(0.05),
                    },
                }),
                RuntimeEvent::Hit {
                    velocity,
                    clip_envelope,
                    ..
                } => projected.push(TimelineEvent {
                    clip_id,
                    start_frame: event.frame,
                    end_frame: event.frame.saturating_add(1),
                    kind: TimelineEventKind::Hit {
                        strength: (*velocity * clip_envelope.gain_at(0)).max(0.05),
                    },
                }),
                RuntimeEvent::SampleOn {
                    note_id,
                    mapping_index,
                    start_frame,
                    end_frame,
                    fade_in_frames,
                    fade_out_frames,
                    direction,
                    loop_region,
                    clip_envelope,
                    ..
                } => {
                    let mapping = mappings.get(usize::from(*mapping_index));
                    let asset = mapping.and_then(|mapping| assets.get(&mapping.asset));
                    if let Some(asset) = asset {
                        required_assets.insert(waveform_cache_key(asset), asset);
                    }
                    // Pattern Sample Events start occurrence voices; runtime
                    // `sample_off` releases them regardless of a mapping's
                    // note-trigger playback mode, so this scheduled off is the
                    // canonical visible end as well.
                    projected.push(TimelineEvent {
                        clip_id,
                        start_frame: event.frame,
                        end_frame: sample_ends
                            .get(note_id)
                            .copied()
                            .unwrap_or_else(|| event.frame.saturating_add(1)),
                        kind: TimelineEventKind::Sample {
                            waveform_key: asset.map(waveform_cache_key),
                            source_rate: asset
                                .map_or(project.sample_rate, |asset| asset.metadata().sample_rate),
                            source_start: *start_frame,
                            source_end: *end_frame,
                            direction: direction
                                .or_else(|| mapping.map(|mapping| mapping.direction))
                                .unwrap_or(SampleDirection::Forward),
                            loop_region: loop_region
                                .or_else(|| mapping.and_then(|mapping| mapping.loop_region)),
                            fade_in_frames: *fade_in_frames,
                            fade_out_frames: *fade_out_frames,
                            clip_envelope: *clip_envelope,
                        },
                    });
                }
                RuntimeEvent::NoteOff { .. } | RuntimeEvent::SampleOff { .. } => {}
            }
        }
        tracks.insert(track.id.clone(), projected);
    }
    Ok(TimelineMaterial {
        tracks,
        required_assets,
    })
}

fn waveform_cache_key(asset: &LoadedAsset) -> String {
    format!("{WAVEFORM_ALGORITHM}:stereo-minmax:{}", asset.sha256())
}

#[derive(Debug, Clone, Copy)]
struct Peak {
    minimum: f32,
    maximum: f32,
}

impl Default for Peak {
    fn default() -> Self {
        Self {
            minimum: f32::INFINITY,
            maximum: f32::NEG_INFINITY,
        }
    }
}

impl Peak {
    fn include(&mut self, value: f32) {
        self.minimum = self.minimum.min(value);
        self.maximum = self.maximum.max(value);
    }

    fn merge(self, other: Self) -> Self {
        Self {
            minimum: self.minimum.min(other.minimum),
            maximum: self.maximum.max(other.maximum),
        }
    }

    fn magnitude(self) -> f32 {
        if self.minimum.is_finite() && self.maximum.is_finite() {
            self.minimum.abs().max(self.maximum.abs()).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone)]
struct WaveformPyramid {
    source_frames: u64,
    levels: Vec<Vec<Peak>>,
}

impl WaveformPyramid {
    fn build(asset: &LoadedAsset) -> Result<Self, String> {
        let samples = decode_wav(asset).map_err(|error| error.to_string())?;
        let source_frames = u64::try_from(samples.len())
            .map_err(|_| format!("asset `{}` has too many frames", asset.id()))?;
        let mut base = Vec::with_capacity(samples.len().div_ceil(WAVEFORM_BASE_BLOCK_SIZE));
        for frames in samples.chunks(WAVEFORM_BASE_BLOCK_SIZE) {
            let mut peak = Peak::default();
            for frame in frames {
                peak.include(frame[0]);
                peak.include(frame[1]);
            }
            base.push(peak);
        }
        let mut levels = vec![base];
        while levels.last().is_some_and(|level| level.len() > 1) {
            let Some(previous) = levels.last() else {
                break;
            };
            let mut next = Vec::with_capacity(previous.len().div_ceil(2));
            for pair in previous.chunks(2) {
                next.push(if pair.len() == 2 {
                    pair[0].merge(pair[1])
                } else {
                    pair[0]
                });
            }
            levels.push(next);
        }
        Ok(Self {
            source_frames,
            levels,
        })
    }

    fn magnitude(&self, start: u64, end: u64) -> f32 {
        let start = start.min(self.source_frames);
        let end = end.min(self.source_frames);
        if start >= end || self.levels.is_empty() {
            return 0.0;
        }
        let span = end - start;
        let mut level_index = 0_usize;
        let mut block_frames = WAVEFORM_BASE_BLOCK_FRAMES;
        while level_index + 1 < self.levels.len() && block_frames.saturating_mul(2) <= span {
            level_index += 1;
            block_frames = block_frames.saturating_mul(2);
        }
        let level = &self.levels[level_index];
        let first = usize::try_from(start / block_frames)
            .unwrap_or(usize::MAX)
            .min(level.len());
        let last = usize::try_from(end.saturating_sub(1) / block_frames)
            .unwrap_or(usize::MAX)
            .saturating_add(1)
            .min(level.len());
        level[first..last]
            .iter()
            .copied()
            .fold(Peak::default(), Peak::merge)
            .magnitude()
    }
}

fn project_timeline(
    project: &Project,
    events: &BTreeMap<String, Vec<TimelineEvent>>,
    waveforms: &BTreeMap<String, WaveformPyramid>,
    window: TimelineWindow,
    columns: usize,
) -> TimelineProjection {
    let (grid, ruler) = project_ruler(project, window, columns);
    let locator_columns = project
        .song
        .locators
        .iter()
        .filter_map(|locator| time_column(locator.at, window, columns))
        .collect();
    if columns == 0 || !window.is_valid() {
        return TimelineProjection {
            window,
            ruler,
            grid,
            locator_columns,
            lanes: project
                .song
                .tracks
                .iter()
                .map(|track| TimelineLane {
                    track_id: track.id.clone(),
                    cells: Vec::new(),
                })
                .collect(),
        };
    }

    let viewport_start = window
        .start
        .to_frames(project.song.tempo_bpm, project.sample_rate)
        .unwrap_or(0);
    let viewport_end = window
        .end
        .to_frames(project.song.tempo_bpm, project.sample_rate)
        .unwrap_or_else(|_| viewport_start.saturating_add(1))
        .max(viewport_start.saturating_add(1));
    let mut lanes = Vec::with_capacity(project.song.tracks.len());
    for track in &project.song.tracks {
        let mut cells = vec![TimelineCell::default(); columns];
        for clip in &track.clips {
            let clip_end = clip
                .start
                .checked_add(clip.duration)
                .unwrap_or(project.song.end)
                .min(project.song.end);
            if let Some((first, last)) = time_span_columns(clip.start, clip_end, window, columns) {
                for cell in &mut cells[first..last] {
                    cell.clip_depth = cell.clip_depth.saturating_add(1);
                }
                if clip.start >= window.start && clip.start < window.end {
                    cells[first].clip_start = true;
                }
                if clip_end > window.start && clip_end <= window.end {
                    cells[last - 1].clip_end = true;
                }
            }
        }

        let clip_ids = track
            .clips
            .iter()
            .map(|clip| clip.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut sustain_delta = vec![0.0_f32; columns.saturating_add(1)];
        for event in events.get(&track.id).into_iter().flatten() {
            if !clip_ids.contains(event.clip_id.as_str()) {
                continue;
            }
            match &event.kind {
                TimelineEventKind::Hit { strength } => {
                    if let Some(column) =
                        frame_column(event.start_frame, viewport_start, viewport_end, columns)
                    {
                        cells[column].activity += strength;
                    }
                }
                TimelineEventKind::Note { strength } => {
                    if let Some(column) =
                        frame_column(event.start_frame, viewport_start, viewport_end, columns)
                    {
                        cells[column].activity += strength;
                    }
                    if let Some((first, last)) = span_columns(
                        event.start_frame,
                        event.end_frame,
                        viewport_start,
                        viewport_end,
                        columns,
                    ) {
                        sustain_delta[first] += strength * 0.3;
                        sustain_delta[last] -= strength * 0.3;
                    }
                }
                TimelineEventKind::Sample {
                    waveform_key,
                    source_rate,
                    source_start,
                    source_end,
                    direction,
                    loop_region,
                    fade_in_frames,
                    fade_out_frames,
                    clip_envelope,
                } => {
                    let Some((first, last)) = span_columns(
                        event.start_frame,
                        event.end_frame,
                        viewport_start,
                        viewport_end,
                        columns,
                    ) else {
                        continue;
                    };
                    let waveform = waveform_key.as_ref().and_then(|key| waveforms.get(key));
                    for (column, cell) in cells.iter_mut().enumerate().take(last).skip(first) {
                        let (column_start, column_end) =
                            column_frame_span(column, viewport_start, viewport_end, columns);
                        let overlap_start = column_start.max(event.start_frame);
                        let overlap_end = column_end.min(event.end_frame);
                        if overlap_start >= overlap_end {
                            continue;
                        }
                        let age_start = overlap_start.saturating_sub(event.start_frame);
                        let age_end = overlap_end.saturating_sub(event.start_frame);
                        let Some(waveform) = waveform else {
                            cell.sample_active = true;
                            continue;
                        };
                        let ranges = sample_source_ranges(
                            *source_start,
                            *source_end,
                            *direction,
                            *loop_region,
                            source_progress(age_start, *source_rate, project.sample_rate),
                            source_progress_ceil(age_end, *source_rate, project.sample_rate),
                        );
                        if ranges.is_empty() {
                            continue;
                        }
                        cell.sample_active = true;
                        let peak = ranges
                            .iter()
                            .map(|(start, end)| waveform.magnitude(*start, *end))
                            .fold(0.0_f32, f32::max);
                        let center_age = age_start.saturating_add(age_end) / 2;
                        let fade = sample_fade(
                            source_progress(center_age, *source_rate, project.sample_rate),
                            *source_start,
                            *source_end,
                            *direction,
                            *loop_region,
                            *fade_in_frames,
                            *fade_out_frames,
                        );
                        cell.sample_peak = cell
                            .sample_peak
                            .max(peak * fade * clip_envelope.gain_at(center_age).max(0.0));
                    }
                }
            }
        }
        let mut sustained = 0.0_f32;
        for (cell, delta) in cells.iter_mut().zip(sustain_delta) {
            sustained += delta;
            cell.sustained = sustained.max(0.0);
        }
        lanes.push(TimelineLane {
            track_id: track.id.clone(),
            cells,
        });
    }

    TimelineProjection {
        window,
        ruler,
        grid,
        locator_columns,
        lanes,
    }
}

fn source_progress(age: u64, source_rate: u32, project_rate: u32) -> u64 {
    u64::try_from(
        u128::from(age).saturating_mul(u128::from(source_rate)) / u128::from(project_rate.max(1)),
    )
    .unwrap_or(u64::MAX)
}

fn source_progress_ceil(age: u64, source_rate: u32, project_rate: u32) -> u64 {
    let numerator = u128::from(age).saturating_mul(u128::from(source_rate));
    let denominator = u128::from(project_rate.max(1));
    u64::try_from(numerator.div_ceil(denominator)).unwrap_or(u64::MAX)
}

fn sample_fade(
    progress: u64,
    source_start: u64,
    source_end: u64,
    direction: SampleDirection,
    loop_region: Option<SampleLoopRegion>,
    fade_in_frames: u32,
    fade_out_frames: u32,
) -> f32 {
    let travelled = sample_travelled(progress, source_start, source_end, direction, loop_region);
    let fade_in = if fade_in_frames == 0 {
        1.0
    } else {
        travelled as f32 / fade_in_frames as f32
    };
    let fade_out = if loop_region.is_some() || fade_out_frames == 0 {
        1.0
    } else {
        source_end
            .saturating_sub(source_start)
            .saturating_sub(travelled) as f32
            / fade_out_frames as f32
    };
    fade_in.clamp(0.0, 1.0) * fade_out.clamp(0.0, 1.0)
}

fn sample_travelled(
    progress: u64,
    source_start: u64,
    source_end: u64,
    direction: SampleDirection,
    loop_region: Option<SampleLoopRegion>,
) -> u64 {
    let source_length = source_end.saturating_sub(source_start);
    let Some(loop_region) = loop_region else {
        return progress.min(source_length);
    };
    let loop_start = loop_region.start_frame.max(source_start);
    let loop_end = loop_region.end_frame.min(source_end);
    let loop_length = loop_end.saturating_sub(loop_start);
    let crossfade = u64::from(loop_region.crossfade_frames);
    if loop_length == 0 || crossfade >= loop_length {
        return progress.min(source_length);
    }
    let initial_length = match direction {
        SampleDirection::Forward => loop_end.saturating_sub(source_start),
        SampleDirection::Reverse => source_end.saturating_sub(loop_start),
    };
    if progress < initial_length {
        return progress;
    }
    let cycle_length = loop_length - crossfade;
    let cycle_position = progress.saturating_sub(initial_length) % cycle_length;
    match direction {
        SampleDirection::Forward => loop_start
            .saturating_add(crossfade)
            .saturating_add(cycle_position)
            .saturating_sub(source_start),
        SampleDirection::Reverse => source_end
            .saturating_sub(loop_end.saturating_sub(crossfade))
            .saturating_add(cycle_position),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SourceRanges {
    values: [(u64, u64); 6],
    len: usize,
}

impl SourceRanges {
    fn one(range: (u64, u64)) -> Self {
        let mut ranges = Self::default();
        ranges.push(range);
        ranges
    }

    fn push(&mut self, range: (u64, u64)) {
        debug_assert!(self.len < self.values.len());
        if let Some(slot) = self.values.get_mut(self.len) {
            *slot = range;
            self.len += 1;
        }
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn iter(&self) -> std::slice::Iter<'_, (u64, u64)> {
        self.values[..self.len].iter()
    }

    #[cfg(test)]
    fn as_slice(&self) -> &[(u64, u64)] {
        &self.values[..self.len]
    }
}

fn sample_source_ranges(
    source_start: u64,
    source_end: u64,
    direction: SampleDirection,
    loop_region: Option<SampleLoopRegion>,
    progress_start: u64,
    progress_end: u64,
) -> SourceRanges {
    if source_start >= source_end || progress_start >= progress_end {
        return SourceRanges::default();
    }
    let source_length = source_end - source_start;
    let Some(loop_region) = loop_region else {
        let start = progress_start.min(source_length);
        let end = progress_end.min(source_length);
        if start >= end {
            return SourceRanges::default();
        }
        let range = match direction {
            SampleDirection::Forward => (source_start + start, source_start + end),
            SampleDirection::Reverse => (source_end - end, source_end - start),
        };
        return SourceRanges::one(range);
    };
    let loop_start = loop_region.start_frame.max(source_start);
    let loop_end = loop_region.end_frame.min(source_end);
    if loop_start >= loop_end {
        return SourceRanges::default();
    }
    let loop_length = loop_end - loop_start;
    let crossfade = u64::from(loop_region.crossfade_frames);
    if crossfade >= loop_length {
        return SourceRanges::default();
    }
    let cycle_length = loop_length - crossfade;
    let initial_length = match direction {
        SampleDirection::Forward => loop_end - source_start,
        SampleDirection::Reverse => source_end - loop_start,
    };
    let mut ranges = SourceRanges::default();
    let initial_start = progress_start.min(initial_length);
    let initial_end = progress_end.min(initial_length);
    if initial_start < initial_end {
        let primary = match direction {
            SampleDirection::Forward => (source_start + initial_start, source_start + initial_end),
            SampleDirection::Reverse => (source_end - initial_end, source_end - initial_start),
        };
        ranges.push(primary);
        push_crossfade_alternate(
            &mut ranges,
            primary,
            direction,
            loop_start,
            loop_end,
            crossfade,
        );
    }
    if progress_end <= initial_length {
        return ranges;
    }

    let repeated_start = progress_start.saturating_sub(initial_length);
    let repeated_end = progress_end.saturating_sub(initial_length);
    if repeated_start >= repeated_end {
        return ranges;
    }
    if repeated_end - repeated_start >= cycle_length {
        push_loop_cycle_range(
            &mut ranges,
            direction,
            loop_start,
            loop_end,
            crossfade,
            0,
            cycle_length,
        );
        return ranges;
    }
    let first_cycle = repeated_start / cycle_length;
    let last_cycle = repeated_end.saturating_sub(1) / cycle_length;
    let first_position = repeated_start % cycle_length;
    let last_position = repeated_end.saturating_sub(1) % cycle_length + 1;
    if first_cycle == last_cycle {
        push_loop_cycle_range(
            &mut ranges,
            direction,
            loop_start,
            loop_end,
            crossfade,
            first_position,
            last_position,
        );
    } else {
        push_loop_cycle_range(
            &mut ranges,
            direction,
            loop_start,
            loop_end,
            crossfade,
            first_position,
            cycle_length,
        );
        push_loop_cycle_range(
            &mut ranges,
            direction,
            loop_start,
            loop_end,
            crossfade,
            0,
            last_position,
        );
    }
    ranges
}

fn push_loop_cycle_range(
    ranges: &mut SourceRanges,
    direction: SampleDirection,
    loop_start: u64,
    loop_end: u64,
    crossfade: u64,
    cycle_start: u64,
    cycle_end: u64,
) {
    if cycle_start >= cycle_end {
        return;
    }
    let primary = match direction {
        SampleDirection::Forward => (
            loop_start + crossfade + cycle_start,
            loop_start + crossfade + cycle_end,
        ),
        SampleDirection::Reverse => (
            loop_end - crossfade - cycle_end,
            loop_end - crossfade - cycle_start,
        ),
    };
    ranges.push(primary);
    push_crossfade_alternate(ranges, primary, direction, loop_start, loop_end, crossfade);
}

fn push_crossfade_alternate(
    ranges: &mut SourceRanges,
    primary: (u64, u64),
    direction: SampleDirection,
    loop_start: u64,
    loop_end: u64,
    crossfade: u64,
) {
    if crossfade == 0 {
        return;
    }
    let (zone_start, zone_end) = match direction {
        SampleDirection::Forward => (loop_end - crossfade, loop_end),
        SampleDirection::Reverse => (loop_start, loop_start + crossfade),
    };
    let overlap_start = primary.0.max(zone_start);
    let overlap_end = primary.1.min(zone_end);
    if overlap_start >= overlap_end {
        return;
    }
    ranges.push(match direction {
        SampleDirection::Forward => (
            loop_start + overlap_start - zone_start,
            loop_start + overlap_end - zone_start,
        ),
        SampleDirection::Reverse => (
            loop_end - crossfade + overlap_start - loop_start,
            loop_end - crossfade + overlap_end - loop_start,
        ),
    });
}

fn project_ruler(
    project: &Project,
    window: TimelineWindow,
    columns: usize,
) -> (Vec<TimelineGridMark>, String) {
    if columns == 0 || !window.is_valid() {
        return (Vec::new(), String::new());
    }
    let bar_length = project
        .song
        .meter
        .bar_length()
        .unwrap_or_else(|_| MusicalTime::whole(4));
    let beat_length = MusicalTime::new(4, i64::from(project.song.meter.denominator))
        .unwrap_or_else(|_| MusicalTime::whole(1));
    let mut grid = vec![TimelineGridMark::None; columns];
    let mut labels = Vec::<Option<u64>>::with_capacity(columns);
    for (column, mark) in grid.iter_mut().enumerate() {
        let Some((left, right)) = column_time_span(window, column, columns) else {
            labels.push(None);
            continue;
        };
        let bar = unit_boundary_at_or_after(left, bar_length)
            .filter(|(_, boundary)| *boundary < right)
            .map(|(index, _)| index.saturating_add(1));
        let beat = unit_boundary_at_or_after(left, beat_length)
            .is_some_and(|(_, boundary)| boundary < right);
        *mark = if bar.is_some() {
            TimelineGridMark::Bar
        } else if beat {
            TimelineGridMark::Beat
        } else {
            TimelineGridMark::None
        };
        labels.push(bar);
    }
    // Labels stay ASCII and glyph-free so the terminal adapter can choose a
    // Unicode or ASCII grid without invalidating semantic projection data.
    let mut ruler = vec![' '; columns];
    let mut occupied_until = 0_usize;
    for (column, bar) in labels.into_iter().enumerate() {
        let Some(bar) = bar else {
            continue;
        };
        if column < occupied_until {
            continue;
        }
        let label = bar.to_string();
        if column + label.chars().count() > ruler.len() {
            continue;
        }
        for (offset, character) in label.chars().enumerate() {
            ruler[column + offset] = character;
        }
        occupied_until = column + label.len() + 1;
    }
    (grid, ruler.into_iter().collect())
}

pub(super) fn column_time_span(
    window: TimelineWindow,
    column: usize,
    columns: usize,
) -> Option<(MusicalTime, MusicalTime)> {
    let span = window.end.checked_sub(window.start).ok()?;
    let denominator = u32::try_from(columns).ok()?;
    let left = window
        .start
        .checked_add(scale_time(span, u32::try_from(column).ok()?, denominator)?)
        .ok()?;
    let right = window
        .start
        .checked_add(scale_time(
            span,
            u32::try_from(column.checked_add(1)?).ok()?,
            denominator,
        )?)
        .ok()?;
    Some((left, right))
}

fn unit_boundary_at_or_after(value: MusicalTime, unit: MusicalTime) -> Option<(u64, MusicalTime)> {
    if value < MusicalTime::ZERO || !unit.is_positive() {
        return None;
    }
    let numerator = u128::try_from(value.numerator())
        .ok()?
        .checked_mul(u128::try_from(unit.denominator()).ok()?)?;
    let denominator = u128::try_from(value.denominator())
        .ok()?
        .checked_mul(u128::try_from(unit.numerator()).ok()?)?;
    let index = u64::try_from(numerator.div_ceil(denominator)).ok()?;
    Some((index, unit.checked_mul(index).ok()?))
}

pub(super) fn time_column(
    time: MusicalTime,
    window: TimelineWindow,
    columns: usize,
) -> Option<usize> {
    if columns == 0 || !window.is_valid() || time < window.start || time >= window.end {
        return None;
    }
    let delta = time.checked_sub(window.start).ok()?;
    let span = window.end.checked_sub(window.start).ok()?;
    let numerator = i128::from(delta.numerator())
        .checked_mul(i128::from(span.denominator()))?
        .checked_mul(i128::try_from(columns).ok()?)?;
    let denominator = i128::from(delta.denominator()).checked_mul(i128::from(span.numerator()))?;
    usize::try_from(numerator / denominator)
        .ok()
        .map(|column| column.min(columns - 1))
}

pub(super) fn time_span_columns(
    start: MusicalTime,
    end: MusicalTime,
    window: TimelineWindow,
    columns: usize,
) -> Option<(usize, usize)> {
    if columns == 0
        || !window.is_valid()
        || start >= end
        || end <= window.start
        || start >= window.end
    {
        return None;
    }
    let start = start.max(window.start);
    let end = end.min(window.end);
    let first = time_column(start, window, columns)?;
    let delta = end.checked_sub(window.start).ok()?;
    let span = window.end.checked_sub(window.start).ok()?;
    let numerator = u128::try_from(delta.numerator())
        .ok()?
        .checked_mul(u128::try_from(span.denominator()).ok()?)?
        .checked_mul(u128::try_from(columns).ok()?)?;
    let denominator = u128::try_from(delta.denominator())
        .ok()?
        .checked_mul(u128::try_from(span.numerator()).ok()?)?;
    let last = usize::try_from(numerator.div_ceil(denominator))
        .ok()?
        .clamp(first + 1, columns);
    Some((first, last))
}

pub(super) fn frame_column(
    frame: u64,
    viewport_start: u64,
    viewport_end: u64,
    columns: usize,
) -> Option<usize> {
    if columns == 0 || frame < viewport_start || frame >= viewport_end {
        return None;
    }
    let span = viewport_end - viewport_start;
    let offset = frame - viewport_start;
    let column = u128::from(offset).saturating_mul(columns as u128) / u128::from(span);
    Some(
        usize::try_from(column)
            .unwrap_or(columns - 1)
            .min(columns - 1),
    )
}

fn span_columns(
    start: u64,
    end: u64,
    viewport_start: u64,
    viewport_end: u64,
    columns: usize,
) -> Option<(usize, usize)> {
    if columns == 0 || start >= end || end <= viewport_start || start >= viewport_end {
        return None;
    }
    let start = start.max(viewport_start);
    let end = end.min(viewport_end);
    let span = viewport_end - viewport_start;
    let first = usize::try_from(
        u128::from(start - viewport_start).saturating_mul(columns as u128) / u128::from(span),
    )
    .unwrap_or(columns - 1)
    .min(columns - 1);
    let last_frame_numerator =
        u128::from(end.saturating_sub(1) - viewport_start).saturating_mul(columns as u128);
    let last = usize::try_from(last_frame_numerator / u128::from(span))
        .unwrap_or(columns - 1)
        .saturating_add(1)
        .clamp(first + 1, columns);
    Some((first, last))
}

fn column_frame_span(
    column: usize,
    viewport_start: u64,
    viewport_end: u64,
    columns: usize,
) -> (u64, u64) {
    let span = viewport_end - viewport_start;
    let denominator = columns.max(1) as u128;
    let start = viewport_start.saturating_add(
        u64::try_from(
            u128::from(span)
                .saturating_mul(column as u128)
                .div_ceil(denominator),
        )
        .unwrap_or(span),
    );
    let end = viewport_start.saturating_add(
        u64::try_from(
            u128::from(span)
                .saturating_mul(column.saturating_add(1) as u128)
                .div_ceil(denominator),
        )
        .unwrap_or(span),
    );
    (start.min(viewport_end), end.min(viewport_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_fit_zoom_and_pan_remain_exact_song_fractions() {
        let end = MusicalTime::whole(16);
        let mut viewport = TimelineViewport::default();
        assert_eq!(
            viewport.window(end),
            TimelineWindow {
                start: MusicalTime::ZERO,
                end
            }
        );
        assert!(viewport.zoom_in());
        assert_eq!(viewport.window(end).start, MusicalTime::whole(4));
        assert_eq!(viewport.window(end).end, MusicalTime::whole(12));
        assert!(viewport.pan(-1));
        assert_eq!(viewport.window(end).start, MusicalTime::whole(2));
        assert_eq!(viewport.window(end).end, MusicalTime::whole(10));
        assert!(viewport.zoom_out());
        assert!(viewport.is_fit());
        assert_eq!(viewport.window(end).start, MusicalTime::ZERO);
        assert_eq!(viewport.window(end).end, end);
    }

    #[test]
    fn goto_parser_supports_bars_beats_locators_and_relative_durations() {
        let source = include_str!("../../tests/fixtures/minimal.crump").replace(
            "    track lead",
            "    locator chorus \"Chorus\" at 2beat\n    track lead",
        );
        let parsed = crate::dsl::parse_project(std::path::Path::new("minimal.crump"), &source);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        let mut project = parsed.project.expect("Project");
        project.song.end = MusicalTime::whole(32);
        let cursor = MusicalTime::whole(2);

        assert_eq!(
            parse_destination("8", cursor, &project).expect("bar"),
            MusicalTime::whole(28)
        );
        assert_eq!(
            parse_destination("8:3", cursor, &project).expect("bar and beat"),
            MusicalTime::whole(30)
        );
        assert_eq!(
            parse_destination("chorus", cursor, &project).expect("Locator"),
            MusicalTime::whole(2)
        );
        assert_eq!(
            parse_destination("+2bar", cursor, &project).expect("forward relative"),
            MusicalTime::whole(10)
        );
        assert_eq!(
            parse_destination("-1beat", cursor, &project).expect("back relative"),
            MusicalTime::whole(1)
        );
    }

    #[test]
    fn navigation_state_normalizes_ranges_marks_and_jump_returns() {
        let mut state = TimelineState::default();
        state.begin_visual();
        state.set_position(TimelinePosition {
            at: MusicalTime::whole(2),
            track: 0,
        });
        assert_eq!(
            state.visual_range(),
            Some(TimelineRange {
                start: MusicalTime::ZERO,
                end: MusicalTime::whole(2),
            })
        );
        assert!(state.swap_visual_edge());
        state.set_position(TimelinePosition {
            at: MusicalTime::whole(1),
            track: 0,
        });
        assert_eq!(
            state.visual_range(),
            Some(TimelineRange {
                start: MusicalTime::whole(1),
                end: MusicalTime::whole(2),
            })
        );

        state.set_mark('A');
        assert_eq!(state.mark('a'), Some(state.position()));
        state.remember_for_return();
        assert_eq!(state.jump_back(), Some(state.position()));
    }

    #[test]
    fn frame_projection_is_half_open_and_keeps_short_spans_visible() {
        assert_eq!(frame_column(0, 0, 100, 10), Some(0));
        assert_eq!(frame_column(99, 0, 100, 10), Some(9));
        assert_eq!(frame_column(100, 0, 100, 10), None);
        let thirds = TimelineWindow {
            start: MusicalTime::ZERO,
            end: MusicalTime::whole(1),
        };
        assert_eq!(
            time_span_columns(
                MusicalTime::new(1, 3).expect("one third"),
                MusicalTime::new(2, 3).expect("two thirds"),
                thirds,
                3,
            ),
            Some((1, 2))
        );
        assert_eq!(span_columns(10, 11, 0, 100, 10), Some((1, 2)));
        assert_eq!(span_columns(33, 34, 0, 100, 3), Some((0, 1)));
        assert_eq!(span_columns(100, 110, 0, 100, 10), None);
        assert_eq!(column_frame_span(0, 0, 100, 3), (0, 34));
        assert_eq!(column_frame_span(1, 0, 100, 3), (34, 67));
        assert_eq!(column_frame_span(2, 0, 100, 3), (67, 100));
    }

    #[test]
    fn source_ranges_follow_reverse_and_bounded_loop_traversal() {
        assert_eq!(
            sample_source_ranges(0, 100, SampleDirection::Forward, None, 10, 20).as_slice(),
            &[(10, 20)]
        );
        assert_eq!(
            sample_source_ranges(0, 100, SampleDirection::Reverse, None, 10, 20).as_slice(),
            &[(80, 90)]
        );
        let loop_region = Some(SampleLoopRegion {
            start_frame: 20,
            end_frame: 40,
            crossfade_frames: 0,
        });
        assert_eq!(
            sample_source_ranges(0, 100, SampleDirection::Forward, loop_region, 40, 50).as_slice(),
            &[(20, 30)]
        );
        assert_eq!(
            sample_source_ranges(0, 100, SampleDirection::Reverse, loop_region, 80, 90).as_slice(),
            &[(30, 40)]
        );

        let crossfaded_loop = Some(SampleLoopRegion {
            start_frame: 20,
            end_frame: 40,
            crossfade_frames: 4,
        });
        assert_eq!(
            sample_source_ranges(0, 100, SampleDirection::Forward, crossfaded_loop, 52, 56,)
                .as_slice(),
            &[(36, 40), (20, 24)]
        );
        assert_eq!(
            sample_source_ranges(0, 100, SampleDirection::Reverse, crossfaded_loop, 92, 96,)
                .as_slice(),
            &[(20, 24), (36, 40)]
        );
        assert_eq!(
            sample_travelled(40, 0, 100, SampleDirection::Forward, crossfaded_loop),
            24
        );
        assert_eq!(
            sample_travelled(80, 0, 100, SampleDirection::Reverse, crossfaded_loop),
            64
        );
    }

    #[test]
    fn ruler_uses_exact_meter_beat_and_bar_boundaries() {
        let parsed = crate::dsl::parse_project(
            std::path::Path::new("minimal.crump"),
            include_str!("../../tests/fixtures/minimal.crump"),
        );
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        let mut project = parsed.project.expect("Project");
        project.song.meter = crate::time::Meter::new(6, 8).expect("6/8 meter");
        let (grid, ruler) = project_ruler(
            &project,
            TimelineWindow {
                start: MusicalTime::ZERO,
                end: MusicalTime::whole(6),
            },
            12,
        );
        assert_eq!(grid[0], TimelineGridMark::Bar);
        assert_eq!(grid[1], TimelineGridMark::Beat);
        assert_eq!(grid[6], TimelineGridMark::Bar);
        assert_eq!(ruler.chars().next(), Some('1'));
        assert_eq!(ruler.chars().nth(6), Some('2'));
    }

    #[test]
    fn canonical_schedule_projects_clip_ranges_and_pattern_activity() {
        let parsed = crate::dsl::parse_project(
            std::path::Path::new("minimal.crump"),
            include_str!("../../tests/fixtures/minimal.crump"),
        );
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        let project = parsed.project.expect("Project");
        let mut cache = TimelineCache::default();
        cache.rebuild(Some(&project), &AssetInventory::default());
        let projection = cache.project(
            &project,
            TimelineViewport::default().window(project.song.end),
            16,
        );
        assert_eq!(projection.lanes.len(), 2);
        let lead = &projection.lanes[0];
        assert!(lead.cells.iter().all(|cell| cell.clip_depth == 1));
        assert_eq!(
            lead.cells.iter().filter(|cell| cell.activity > 0.0).count(),
            4
        );
        let drums = &projection.lanes[1];
        assert_eq!(
            drums
                .cells
                .iter()
                .filter(|cell| cell.activity > 0.0)
                .count(),
            8
        );
    }

    #[test]
    fn sampler_occurrences_project_verified_source_waveforms() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/cookbook/35-hat-roll");
        let bundle = crate::project::load_project(&root).expect("sample Project");
        assert!(!bundle.diagnostics.has_errors(), "{:?}", bundle.diagnostics);
        let project = bundle.publishable().expect("publishable Project");
        let mut cache = TimelineCache::default();
        cache.rebuild(Some(project), &bundle.assets);
        let projection = cache.project(
            project,
            TimelineViewport::default().window(project.song.end),
            64,
        );
        assert!(
            projection.lanes[0]
                .cells
                .iter()
                .any(|cell| cell.sample_active && cell.sample_peak > 0.0)
        );
        assert!(cache.warning().is_none());

        // An invalid Candidate must clear Candidate-derived media rather than
        // retaining a waveform that could be mistaken for Active material.
        cache.rebuild(None, &bundle.assets);
        let invalid_projection = cache.project(
            project,
            TimelineViewport::default().window(project.song.end),
            64,
        );
        assert!(
            invalid_projection
                .lanes
                .iter()
                .flat_map(|lane| &lane.cells)
                .all(|cell| !cell.sample_active)
        );
        assert_eq!(
            cache.warning(),
            Some("activity unavailable for invalid Candidate")
        );
        assert_eq!(WAVEFORM_ALGORITHM, "crump-timeline-waveform-v1");
    }
}
