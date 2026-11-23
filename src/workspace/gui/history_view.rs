//! Project History read as a musician's log: working sessions, each a list of the things changed,
//! with back-to-back edits to one thing folded into a run that expands. A side inspector explains
//! the selected change in words and offers Compare and Restore.

use std::sync::OnceLock;

use super::theme::{self, with_alpha};
use super::widgets;
use super::*;
use crate::history::{HistoryEntry, OriginKind};
use crate::model::Project;

/// Silence that starts a new working session.
const SESSION_GAP_SECS: i64 = 30 * 60;

/// What a History entry changed, for its tag and colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Song,
    Instrument,
    Pattern,
    Arrangement,
    Mix,
    Fx,
    Automation,
    Restore,
    Other,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Song => "SONG",
            Kind::Instrument => "SYNTH",
            Kind::Pattern => "NOTES",
            Kind::Arrangement => "ARRANGE",
            Kind::Mix => "MIX",
            Kind::Fx => "FX",
            Kind::Automation => "AUTO",
            Kind::Restore => "RESTORE",
            Kind::Other => "EDIT",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Kind::Song | Kind::Other => theme::TEXT_SECONDARY,
            Kind::Instrument => theme::ACCENT_VIOLET,
            Kind::Pattern => Color32::from_rgb(236, 122, 160),
            Kind::Arrangement => theme::ACCENT_CYAN,
            Kind::Mix => Color32::from_rgb(164, 204, 96),
            Kind::Fx => Color32::from_rgb(91, 155, 248),
            Kind::Automation => theme::SIG_NOMINAL,
            Kind::Restore => theme::TEXT_PRIMARY,
        }
    }
}

/// One History entry read as plain words.
struct Item<'a> {
    index: usize,
    entry: &'a HistoryEntry,
    /// Local time, in seconds since the epoch shifted by the local UTC offset.
    at: i64,
    kind: Kind,
    subject: String,
    action: String,
}

impl Item<'_> {
    /// Back-to-back changes to one thing fold into a run; restores always stand alone.
    fn same_subject(&self, other: &Item<'_>) -> bool {
        self.kind != Kind::Restore && self.kind == other.kind && self.subject == other.subject
    }
}

fn items(app: &Workspace) -> Vec<Item<'_>> {
    let offset = local_offset_secs();
    app.history
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            // A restore is known by its link, whatever its summary says.
            let (kind, subject, action) = if entry.restored_from.is_some() {
                (
                    Kind::Restore,
                    "Restore".to_owned(),
                    "Went back to an earlier version".to_owned(),
                )
            } else {
                describe(app, &entry.summary)
            };
            Item {
                index,
                entry,
                at: unix_seconds(&entry.created_at) + offset,
                kind,
                subject,
                action,
            }
        })
        .collect()
}

/// Newest-first items split into working sessions, newest session first.
fn sessions<'a, 'b>(items: &'b [Item<'a>]) -> Vec<&'b [Item<'a>]> {
    split_where(items, |newer, older| newer.at - older.at > SESSION_GAP_SECS)
}

fn split_where<'a, 'b>(
    items: &'b [Item<'a>],
    boundary: impl Fn(&Item<'a>, &Item<'a>) -> bool,
) -> Vec<&'b [Item<'a>]> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..items.len() {
        if boundary(&items[i - 1], &items[i]) {
            out.push(&items[start..i]);
            start = i;
        }
    }
    if !items.is_empty() {
        out.push(&items[start..]);
    }
    out
}

/// What a session touched, oldest first, without repeats: "Deep House Bass, kick-four, Mix".
fn session_subjects(session: &[Item<'_>]) -> String {
    let mut subjects: Vec<&str> = Vec::new();
    for item in session.iter().rev() {
        if item.kind != Kind::Restore && !subjects.contains(&item.subject.as_str()) {
            subjects.push(&item.subject);
        }
    }
    subjects.join(", ")
}

/// Reads a History summary such as `Set dh-bass.cutoff` as (what kind of thing changed, its name,
/// what was done), using the Project's names for Instruments, Tracks and Parameters.
pub(super) fn describe(app: &Workspace, summary: &str) -> (Kind, String, String) {
    let project = app.bundle.project.as_ref();
    let instrument =
        |id: &str| project.and_then(|p| p.song.instruments.iter().find(|i| i.id == id));
    let track_name = |id: &str| {
        project
            .and_then(|p| p.song.tracks.iter().find(|t| t.id == id))
            .map(|t| t.name.clone())
    };
    let is_lane = |id: &str| project.is_some_and(|p| p.song.automations.iter().any(|l| l.id == id));
    let is_pattern =
        |id: &str| project.is_some_and(|p| p.song.patterns.iter().any(|pat| pat.id == id));
    let words = summary.split_whitespace().collect::<Vec<_>>();
    let rest = |from: usize| words.get(from..).map(|w| w.join(" ")).unwrap_or_default();

    if summary.starts_with("Restore history entry") || summary.starts_with("Restored") {
        return (
            Kind::Restore,
            "Restore".into(),
            "Went back to an earlier version".into(),
        );
    }
    if let Some(bpm) = summary.strip_prefix("Set tempo ") {
        return (Kind::Song, "Tempo".into(), bpm.to_owned());
    }
    if let Some(id) = summary.strip_prefix("Create Locator ") {
        return (Kind::Song, "Locators".into(), format!("Added {id}"));
    }
    match words.as_slice() {
        ["Add", "Clip", clip, "on", track] => {
            let name = track_name(track).unwrap_or_else(|| (*track).to_owned());
            return (Kind::Arrangement, name, format!("Added clip {clip}"));
        }
        ["Move", "Clip", clip, "to", at] => {
            return (
                Kind::Arrangement,
                "Arrangement".into(),
                format!("Moved {clip} to {at}"),
            );
        }
        ["Delete", "Clip", clip] => {
            return (
                Kind::Arrangement,
                "Arrangement".into(),
                format!("Deleted {clip}"),
            );
        }
        ["Set", track, param @ ("level" | "pan")] if track_name(track).is_some() => {
            return (
                Kind::Mix,
                track_name(track).unwrap_or_default(),
                title(param),
            );
        }
        ["Set", "Processor", processor, ..] | ["Move", "Processor", processor, ..] => {
            let detail = if words[0] == "Move" {
                format!("Moved {}", rest(3).to_lowercase())
            } else if summary.ends_with("bypass to true") {
                "Bypassed".to_owned()
            } else if summary.ends_with("bypass to false") {
                "Switched back on".to_owned()
            } else {
                rest(3)
            };
            let name = processor_name(app, processor).unwrap_or_else(|| (*processor).to_owned());
            return (Kind::Fx, name, detail);
        }
        ["Create", "Automation", "lane", lane] => {
            let name = lane_name(app, lane).unwrap_or_else(|| (*lane).to_owned());
            return (Kind::Automation, name, "Created lane".into());
        }
        [verb @ ("Add" | "Delete"), "point", point, "in", lane] => {
            let verb = if *verb == "Add" { "Added" } else { "Deleted" };
            return (
                Kind::Automation,
                lane_name(app, lane).unwrap_or_else(|| (*lane).to_owned()),
                format!("{verb} point {point}"),
            );
        }
        ["Add", what @ ("hit" | "note"), value, "in", pattern] => {
            return (
                Kind::Pattern,
                (*pattern).to_owned(),
                format!("Added {what} {value}"),
            );
        }
        [verb @ ("Change" | "Move" | "Delete"), .., "in", pattern] if is_pattern(pattern) => {
            let verb = match *verb {
                "Change" => "Changed",
                "Move" => "Moved",
                _ => "Deleted",
            };
            return (
                Kind::Pattern,
                (*pattern).to_owned(),
                format!("{verb} an event"),
            );
        }
        _ => {}
    }
    // `Set A.B …` (Instrument Parameter, Automation point, processor setting) and `Set P[e] …`.
    if words.first() == Some(&"Set")
        && let Some(target) = words.get(1)
    {
        if let Some((pattern, _)) = target.split_once('[') {
            return (
                Kind::Pattern,
                pattern.to_owned(),
                title(&format!("event {}", rest(2))),
            );
        }
        if let Some((owner, key)) = target.split_once('.') {
            if let Some(instrument) = instrument(owner) {
                let param = instrument
                    .parameters
                    .iter()
                    .find(|p| p.key == key)
                    .map_or_else(|| title(key), |p| p.name.clone());
                return (Kind::Instrument, instrument.name.clone(), param);
            }
            if is_lane(owner) {
                return (
                    Kind::Automation,
                    lane_name(app, owner).unwrap_or_else(|| owner.to_owned()),
                    format!("Point {}", rest(1).split_once('.').map_or("", |(_, r)| r))
                        .replace(" = ", " to "),
                );
            }
            let value = summary
                .split_once(" = ")
                .map(|(_, v)| format!(" to {v}"))
                .unwrap_or_default();
            let name = processor_name(app, owner).unwrap_or_else(|| owner.to_owned());
            return (Kind::Fx, name, format!("{}{value}", title(key)));
        }
    }
    (Kind::Other, "Project".into(), summary.to_owned())
}

/// An Automation lane named by what it moves: "Deep House Bass Cutoff", "Granular Cloud Level".
fn lane_name(app: &Workspace, lane: &str) -> Option<String> {
    use crate::model::ParameterTarget;
    let song = &app.bundle.project.as_ref()?.song;
    let target = &song.automations.iter().find(|l| l.id == lane)?.target;
    let channel = |name: Option<&String>, parameter: &crate::model::TrackParameter| {
        name.map(|name| format!("{name} {}", title(&format!("{parameter:?}"))))
    };
    match target {
        ParameterTarget::Instrument {
            instrument,
            parameter,
        } => {
            let instrument = song.instruments.iter().find(|i| i.id == *instrument)?;
            let parameter = instrument.parameters.iter().find(|p| p.key == *parameter)?;
            Some(format!("{} {}", instrument.name, parameter.name))
        }
        ParameterTarget::Track { track, parameter } => channel(
            song.tracks.iter().find(|t| t.id == *track).map(|t| &t.name),
            parameter,
        ),
        ParameterTarget::Group { group, parameter } => channel(
            song.groups.iter().find(|g| g.id == *group).map(|g| &g.name),
            parameter,
        ),
        ParameterTarget::Return {
            r#return,
            parameter,
        } => channel(
            song.returns
                .iter()
                .find(|r| r.id == *r#return)
                .map(|r| &r.name),
            parameter,
        ),
        ParameterTarget::Master => Some("Master Level".to_owned()),
        _ => None,
    }
}

/// A processor named by kind and where it sits: "Saturator on Music".
fn processor_name(app: &Workspace, id: &str) -> Option<String> {
    let song = &app.bundle.project.as_ref()?.song;
    let hosts = song
        .tracks
        .iter()
        .map(|t| (t.name.as_str(), &t.channel.inserts))
        .chain(
            song.groups
                .iter()
                .map(|g| (g.name.as_str(), &g.channel.inserts)),
        )
        .chain(
            song.returns
                .iter()
                .map(|r| (r.name.as_str(), &r.channel.inserts)),
        )
        .chain(song.master.iter().map(|m| ("Master", &m.inserts)))
        .chain(
            song.instruments
                .iter()
                .map(|i| (i.name.as_str(), &i.post_processors)),
        );
    for (host, chain) in hosts {
        if let Some(processor) = chain.iter().find(|p| p.id == id) {
            return Some(format!(
                "{} on {host}",
                theme::processor_title(processor.kind)
            ));
        }
    }
    None
}

fn title(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// Seconds since the Unix epoch for an RFC 3339 UTC timestamp (`2026-11-23T14:05:00Z`).
pub(super) fn unix_seconds(text: &str) -> i64 {
    let num = |range: std::ops::Range<usize>| {
        text.get(range)
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    };
    let (y, m, d) = (num(0..4), num(5..7), num(8..10));
    let (hh, mm, ss) = (num(11..13), num(14..16), num(17..19));
    // Days from the civil calendar (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400 + hh * 3600 + mm * 60 + ss
}

/// The local UTC offset in seconds, read once from `date +%z` (the clock library cannot ask the
/// OS safely once threads run). Falls back to UTC.
fn local_offset_secs() -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .and_then(|text| parse_offset(text.trim()))
            .unwrap_or(0)
    })
}

/// `+0130` / `-0800` as seconds.
fn parse_offset(text: &str) -> Option<i64> {
    let sign = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hours: i64 = text.get(1..3)?.parse().ok()?;
    let minutes: i64 = text.get(3..5)?.parse().ok()?;
    Some(sign * (hours * 3600 + minutes * 60))
}

fn now_local() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
        + local_offset_secs()
}

fn clock(at: i64) -> String {
    let secs = at.rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60)
}

fn day_label(at: i64) -> String {
    match now_local().div_euclid(86_400) - at.div_euclid(86_400) {
        0 => "Today".to_owned(),
        1 => "Yesterday".to_owned(),
        n if n < 7 => format!("{n} days ago"),
        _ => {
            let days = at.div_euclid(86_400);
            // Civil date from days (inverse of `unix_seconds`).
            let z = days + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z - era * 146_097;
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            format!("{d} {}", MONTHS[(m - 1) as usize])
        }
    }
}

fn ago(at: i64) -> String {
    let secs = (now_local() - at).max(0);
    match secs {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

fn who(entry: &HistoryEntry) -> String {
    match entry.origin.kind {
        OriginKind::Human => "You".to_owned(),
        OriginKind::Agent => format!("{} (agent)", entry.origin.name),
        OriginKind::System => "Crump".to_owned(),
    }
}

fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// One History entry in plain words with its local time: "Deep House Bass — Drive · Today at
/// 14:19", for confirmations outside the History view.
pub(super) fn plain_label(app: &Workspace, entry: &HistoryEntry) -> String {
    let (_, subject, action) = describe(app, &entry.summary);
    let at = unix_seconds(&entry.created_at) + local_offset_secs();
    format!("{subject} — {action} · {} at {}", day_label(at), clock(at))
}

fn is_current(app: &Workspace, entry: &HistoryEntry) -> bool {
    app.active_entry.as_ref() == Some(&entry.id)
}

/// The words for the entry a restore went back to: "Deep House Bass — Drive at 22:05".
fn restore_target<'a>(items: &'a [Item<'a>], item: &Item<'_>) -> Option<&'a Item<'a>> {
    let target = item.entry.restored_from.as_ref()?;
    items.iter().find(|candidate| candidate.entry.id == *target)
}

fn kind_chip(ui: &mut Ui, kind: Kind) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(64.0, 18.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, with_alpha(kind.color(), 0.14));
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        kind.label(),
        FontId::monospace(9.0),
        kind.color(),
    );
}

/// Draws History once it has entries; the caller draws the empty state.
///
/// A scrub rail across the top holds every version; the page below shows the sound as it was at
/// the playhead, rebuilt from that version's snapshot: the instrument's dials, its track and its
/// effects, each changed control showing where it came from. Play auditions that version in
/// place of the current one, Then / Now swaps view and sound between them, and Restore brings it
/// back for good.
pub(super) fn history(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let items = items(app);
    let selected = clamp_index(app.selected_history, items.len());
    let item = &items[selected];
    let this = version_project(cx, &item.entry.id);
    let previous = item
        .entry
        .parent
        .as_ref()
        .and_then(|parent| version_project(cx, parent));
    let now = app
        .active_entry
        .clone()
        .and_then(|id| version_project(cx, &id));
    let mut pick: Option<usize> = None;

    // A version being heard follows the playhead as it moves.
    if let Some((heard, _)) = &app.history_preview
        && !cx.state.history_show_now
        && *heard != item.entry.id
    {
        cx.send(Action::PreviewHistoryVersion(Some(item.entry.id.clone())));
    }

    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(22, 14))
        .show(ui, |ui| {
            scrub_rail(ui, &items, selected, &mut pick);
            ui.add_space(10.0);
            transport(ui, cx, &items, item);
            ui.add_space(14.0);
            header(ui, &items, item, items.len() - selected, items.len());
            ui.add_space(14.0);

            let Some(this) = this.as_ref().and_then(|bundle| bundle.project.as_ref()) else {
                ui.label(
                    RichText::new("This version's snapshot could not be read.")
                        .color(theme::TEXT_MUTED),
                );
                return;
            };
            let previous = previous.as_ref().and_then(|bundle| bundle.project.as_ref());
            let now = now.as_ref().and_then(|bundle| bundle.project.as_ref());
            let this_edit = value_changes(previous, this);
            let since = now
                .map(|now| value_changes(Some(this), now))
                .unwrap_or_default();
            let focus = focus_of(this, &this_edit).or_else(|| focus_of(this, &since));

            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - SIDE_WIDTH - 16.0).max(320.0));
                    let showing_now = cx.state.history_show_now;
                    let view = SoundView {
                        shown: if showing_now {
                            now.unwrap_or(this)
                        } else {
                            this
                        },
                        previous: if showing_now { None } else { previous },
                        other: if showing_now { Some(this) } else { now },
                        other_word: if showing_now { "then" } else { "now" },
                    };
                    match &focus {
                        Some(focus) => sound_panel(ui, &view, focus),
                        None => {
                            ui.label(
                                RichText::new("This change has no dials to show.")
                                    .color(theme::TEXT_MUTED),
                            );
                        }
                    }
                });
                ui.vertical(|ui| {
                    ui.set_width(SIDE_WIDTH);
                    change_list(ui, "THIS EDIT", &this_edit, "No values moved.");
                    ui.add_space(14.0);
                    let title = if is_current(app, item.entry) {
                        "SINCE THIS VERSION".to_owned()
                    } else {
                        format!("SINCE THIS VERSION · {}", plural(selected, "change"))
                    };
                    change_list(ui, &title, &since, "This is the version you hear now.");
                });
            });
        });

    if let Some(index) = pick {
        cx.select(selected, index);
    }
}

const SIDE_WIDTH: f32 = 340.0;

/// The Project a History entry published, rebuilt once from its snapshot and cached.
fn version_project(
    cx: &mut Cx<'_>,
    id: &str,
) -> Option<std::sync::Arc<crate::project::ProjectBundle>> {
    if let Some(cached) = cx.state.history_versions.get(id) {
        return cached.clone();
    }
    let bundle = crate::history::HistoryStore::open(&cx.app.root)
        .and_then(|store| store.bundle_at(id))
        .ok()
        .filter(|bundle| bundle.project.is_some())
        .map(std::sync::Arc::new);
    cx.state
        .history_versions
        .insert(id.to_owned(), bundle.clone());
    bundle
}

/// Where a value change landed.
#[derive(Clone, PartialEq)]
enum Area {
    Tempo,
    Instrument(String),
    Track(String),
    Processor(String),
    Pattern(String),
}

/// One value that differs between two versions.
#[derive(Clone)]
struct ValueChange {
    area: Area,
    owner: String,
    label: String,
    before: String,
    after: String,
}

/// Every value that differs from `before` to `after`, in plain words.
fn value_changes(before: Option<&Project>, after: &Project) -> Vec<ValueChange> {
    use crate::model::Unit;
    let mut out = Vec::new();
    let Some(before) = before else {
        return out;
    };
    let mut push = |area: Area, owner: &str, label: &str, before: String, after: String| {
        if before != after {
            out.push(ValueChange {
                area,
                owner: owner.to_owned(),
                label: label.to_owned(),
                before,
                after,
            });
        }
    };
    push(
        Area::Tempo,
        "Song",
        "Tempo",
        format!("{:.1} BPM", before.song.tempo_bpm),
        format!("{:.1} BPM", after.song.tempo_bpm),
    );
    for inst in &after.song.instruments {
        let Some(old) = before.song.instruments.iter().find(|i| i.id == inst.id) else {
            continue;
        };
        for param in &inst.parameters {
            if let Some(prev) = old.parameters.iter().find(|p| p.key == param.key) {
                push(
                    Area::Instrument(inst.id.clone()),
                    &inst.name,
                    &param.name,
                    theme::format_compact_value(prev.value, param.unit),
                    theme::format_compact_value(param.value, param.unit),
                );
            }
        }
    }
    for track in &after.song.tracks {
        let Some(old) = before.song.tracks.iter().find(|t| t.id == track.id) else {
            continue;
        };
        push(
            Area::Track(track.id.clone()),
            &track.name,
            "Level",
            theme::format_compact_value(old.channel.level_db, Unit::Decibels),
            theme::format_compact_value(track.channel.level_db, Unit::Decibels),
        );
        push(
            Area::Track(track.id.clone()),
            &track.name,
            "Pan",
            theme::format_compact_value(old.channel.panner.primary_position(), Unit::Pan),
            theme::format_compact_value(track.channel.panner.primary_position(), Unit::Pan),
        );
    }
    let old_processors = processors(before);
    for (host, processor) in processors(after) {
        let Some((_, old)) = old_processors.iter().find(|(_, p)| p.id == processor.id) else {
            continue;
        };
        let owner = format!("{} on {host}", theme::processor_title(processor.kind));
        for setting in &processor.settings {
            if let Some(prev) = old.settings.iter().find(|s| s.key == setting.key) {
                push(
                    Area::Processor(processor.id.clone()),
                    &owner,
                    &title(&setting.key),
                    prev.value.clone(),
                    setting.value.clone(),
                );
            }
        }
        push(
            Area::Processor(processor.id.clone()),
            &owner,
            "Bypass",
            if old.bypass { "on" } else { "off" }.to_owned(),
            if processor.bypass { "on" } else { "off" }.to_owned(),
        );
    }
    for pattern in &after.song.patterns {
        if let Some(old) = before.song.patterns.iter().find(|p| p.id == pattern.id)
            && musical_events(old) != musical_events(pattern)
        {
            let (was, is) = (musical_events(old), musical_events(pattern));
            // Same number of events: say how many changed rather than "4 to 4".
            let (before, after) = if was.len() == is.len() {
                let changed = was.iter().zip(&is).filter(|(a, b)| a != b).count();
                (
                    String::new(),
                    format!("{changed} of {} changed", plural(is.len(), "event")),
                )
            } else {
                (plural(was.len(), "event"), plural(is.len(), "event"))
            };
            out.push(ValueChange {
                area: Area::Pattern(pattern.id.clone()),
                owner: pattern.id.clone(),
                label: "Notes".to_owned(),
                before,
                after,
            });
        }
    }
    out
}

/// A pattern's events as music only (time, pitch or voice, length, velocity), ignoring where
/// they sit in the source, which shifts whenever a file is edited.
fn musical_events(pattern: &crate::model::Pattern) -> Vec<String> {
    use crate::model::PatternEvent;
    pattern
        .events
        .iter()
        .map(|event| match event {
            PatternEvent::Note {
                at,
                pitch,
                duration,
                velocity,
                ..
            } => format!("note {at} {pitch} {duration} {velocity}"),
            PatternEvent::Hit {
                at,
                voice,
                velocity,
                ..
            } => format!("hit {at} {voice:?} {velocity}"),
            PatternEvent::Sample {
                at,
                mapping,
                duration,
                velocity,
                ..
            } => format!("sample {at} {mapping} {duration} {velocity}"),
        })
        .collect()
}

/// Every track, group, return and Master insert with the name of the channel it sits on.
fn processors(project: &Project) -> Vec<(String, &crate::model::Processor)> {
    let song = &project.song;
    let mut out = Vec::new();
    for t in &song.tracks {
        out.extend(t.channel.inserts.iter().map(|p| (t.name.clone(), p)));
    }
    for g in &song.groups {
        out.extend(g.channel.inserts.iter().map(|p| (g.name.clone(), p)));
    }
    for r in &song.returns {
        out.extend(r.channel.inserts.iter().map(|p| (r.name.clone(), p)));
    }
    if let Some(m) = &song.master {
        out.extend(m.inserts.iter().map(|p| ("Master".to_owned(), p)));
    }
    out
}

/// The sound a set of changes belongs to: an instrument and the track playing it, or a pattern.
#[derive(Default, Clone)]
struct Focus {
    instrument: Option<String>,
    track: Option<String>,
    pattern: Option<String>,
    tempo: bool,
}

fn focus_of(project: &Project, changes: &[ValueChange]) -> Option<Focus> {
    let song = &project.song;
    let track_playing = |instrument: &str| {
        song.tracks
            .iter()
            .find(|t| t.instrument == instrument)
            .map(|t| t.id.clone())
    };
    let change = changes.first()?;
    let mut focus = Focus::default();
    match &change.area {
        Area::Tempo => focus.tempo = true,
        Area::Instrument(id) => {
            focus.instrument = Some(id.clone());
            focus.track = track_playing(id);
        }
        Area::Track(track) => {
            focus.track = Some(track.clone());
            focus.instrument = song
                .tracks
                .iter()
                .find(|t| t.id == *track)
                .map(|t| t.instrument.clone());
        }
        Area::Pattern(pattern) => {
            focus.pattern = Some(pattern.clone());
            if let Some(track) = song
                .tracks
                .iter()
                .find(|t| t.clips.iter().any(|c| c.pattern == *pattern))
            {
                focus.track = Some(track.id.clone());
                focus.instrument = Some(track.instrument.clone());
            }
        }
        Area::Processor(id) => {
            if let Some(track) = song
                .tracks
                .iter()
                .find(|t| t.channel.inserts.iter().any(|p| p.id == *id))
            {
                focus.track = Some(track.id.clone());
                focus.instrument = Some(track.instrument.clone());
            } else {
                // A group, return or Master effect: show just the effects.
                focus.track = None;
            }
        }
    }
    Some(focus)
}

/// Which version the sound panel shows and what it compares against.
struct SoundView<'a> {
    /// The version drawn.
    shown: &'a Project,
    /// The version before it, whose values ghost behind this edit's controls.
    previous: Option<&'a Project>,
    /// The other side of Then / Now, whose differing values are marked more quietly.
    other: Option<&'a Project>,
    other_word: &'a str,
}

/// How a control differs: this edit moved it, or it differs from the other side.
#[derive(Clone, Copy)]
enum Mark {
    Edited(f64),
    Differs(f64),
}

fn mark(value: f64, previous: Option<f64>, other: Option<f64>) -> Option<Mark> {
    let moved = |v: f64| (v - value).abs() > 1e-9;
    match (previous.filter(|v| moved(*v)), other.filter(|v| moved(*v))) {
        (Some(prev), _) => Some(Mark::Edited(prev)),
        (None, Some(other)) => Some(Mark::Differs(other)),
        _ => None,
    }
}

/// A read-only dial. An edited control lights its arc, ghosts its previous position on the arc
/// and names the previous value; a control that only differs from the other side gets a quiet
/// outline and names the other value.
#[allow(clippy::too_many_arguments)]
fn dial(
    ui: &mut Ui,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    unit: crate::model::Unit,
    mark: Option<Mark>,
    other_word: &str,
    accent: Color32,
) {
    const SIZE: f32 = 64.0;
    ui.vertical(|ui| {
        ui.set_width(SIZE + 30.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(SIZE + 30.0, SIZE + 8.0), Sense::hover());
        let painter = ui.painter();
        let center = rect.center();
        let radius = SIZE * 0.42;
        let start = std::f32::consts::PI * 0.75;
        let sweep = std::f32::consts::PI * 1.5;
        let point = |t: f32, r: f32| {
            let a = start + sweep * t;
            Pos2::new(center.x + r * a.cos(), center.y + r * a.sin())
        };
        let arc = |from: f32, to: f32| {
            (0..=32)
                .map(|i| point(from + (to - from) * i as f32 / 32.0, radius))
                .collect::<Vec<_>>()
        };
        let norm = theme::value_to_norm(value, min, max, unit);
        painter.circle_filled(center, radius * 0.78, theme::BG_CARD);
        painter.add(egui::Shape::line(
            arc(0.0, 1.0),
            Stroke::new(4.0_f32, theme::BG_WELL),
        ));
        let edited = matches!(mark, Some(Mark::Edited(_)));
        if let Some(Mark::Edited(prev)) = mark {
            let was = theme::value_to_norm(prev, min, max, unit);
            painter.add(egui::Shape::line(
                arc(was.min(norm), was.max(norm)),
                Stroke::new(4.0_f32, with_alpha(accent, 0.28)),
            ));
            painter.line_segment(
                [point(was, radius - 7.0), point(was, radius + 6.0)],
                Stroke::new(2.0_f32, theme::TEXT_SECONDARY),
            );
        }
        painter.add(egui::Shape::line(
            arc(0.0, norm),
            Stroke::new(
                4.0_f32,
                if edited {
                    accent
                } else {
                    with_alpha(accent, 0.5)
                },
            ),
        ));
        painter.line_segment(
            [center, point(norm, radius * 0.7)],
            Stroke::new(2.0_f32, theme::TEXT_PRIMARY),
        );
        match mark {
            Some(Mark::Edited(_)) => {
                painter.circle_stroke(
                    center,
                    radius + 10.0,
                    Stroke::new(1.5_f32, theme::SELECTION),
                );
            }
            Some(Mark::Differs(_)) => {
                painter.circle_stroke(
                    center,
                    radius + 10.0,
                    Stroke::new(1.0_f32, theme::BORDER_STRONG),
                );
            }
            None => {}
        }
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(label.to_uppercase())
                    .size(9.5)
                    .color(theme::TEXT_SECONDARY),
            );
            ui.label(
                RichText::new(theme::format_compact_value(value, unit))
                    .monospace()
                    .size(11.5)
                    .color(theme::TEXT_PRIMARY),
            );
            let note = match mark {
                Some(Mark::Edited(prev)) => {
                    format!("previous {}", theme::format_compact_value(prev, unit))
                }
                Some(Mark::Differs(other)) => {
                    format!("{other_word} {}", theme::format_compact_value(other, unit))
                }
                None => " ".to_owned(),
            };
            ui.label(RichText::new(note).size(10.0).color(theme::TEXT_MUTED));
        });
    });
}

/// A small vertical fader, its marked level ghosted.
fn fader(ui: &mut Ui, db: f64, mark: Option<Mark>, other_word: &str) {
    use crate::model::Unit;
    ui.vertical(|ui| {
        ui.set_width(64.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(64.0, 116.0), Sense::hover());
        let painter = ui.painter();
        let slot = Rect::from_center_size(rect.center(), Vec2::new(6.0, rect.height() - 10.0));
        painter.rect_filled(slot, 3.0, theme::BG_WELL);
        let y_of =
            |db: f64| slot.bottom() - widgets::fader_db_to_norm(db, -60.0, 12.0) * slot.height();
        painter.hline(
            (slot.left() - 8.0)..=(slot.right() + 8.0),
            y_of(0.0),
            Stroke::new(1.0_f32, theme::BORDER_STRONG),
        );
        if let Some(Mark::Edited(prev) | Mark::Differs(prev)) = mark {
            painter.rect_filled(
                Rect::from_center_size(
                    Pos2::new(slot.center().x, y_of(prev)),
                    Vec2::new(30.0, 4.0),
                ),
                2.0,
                with_alpha(theme::TEXT_SECONDARY, 0.6),
            );
        }
        let cap =
            Rect::from_center_size(Pos2::new(slot.center().x, y_of(db)), Vec2::new(34.0, 12.0));
        painter.rect_filled(cap, 3.0, theme::TEXT_PRIMARY);
        if matches!(mark, Some(Mark::Edited(_))) {
            painter.rect_stroke(
                cap.expand(3.0),
                4.0,
                Stroke::new(1.5_f32, theme::SELECTION),
                StrokeKind::Outside,
            );
        }
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("LEVEL")
                    .size(9.5)
                    .color(theme::TEXT_SECONDARY),
            );
            ui.label(
                RichText::new(theme::format_compact_value(db, Unit::Decibels))
                    .monospace()
                    .size(11.5)
                    .color(theme::TEXT_PRIMARY),
            );
            let note = match mark {
                Some(Mark::Edited(prev)) => {
                    format!(
                        "previous {}",
                        theme::format_compact_value(prev, Unit::Decibels)
                    )
                }
                Some(Mark::Differs(other)) => {
                    format!(
                        "{other_word} {}",
                        theme::format_compact_value(other, Unit::Decibels)
                    )
                }
                None => " ".to_owned(),
            };
            ui.label(RichText::new(note).size(10.0).color(theme::TEXT_MUTED));
        });
    });
}

/// A pattern drawn small: notes or hits over its span.
fn pattern_thumb(
    painter: &egui::Painter,
    rect: Rect,
    pattern: &crate::model::Pattern,
    color: Color32,
) {
    use crate::model::PatternEvent;
    painter.rect_filled(rect, 4.0, theme::BG_WELL);
    let span = pattern.span.as_f64().max(0.25);
    let pitches = pattern
        .events
        .iter()
        .filter_map(|e| match e {
            PatternEvent::Note { pitch, .. } => Some(*pitch),
            _ => None,
        })
        .collect::<Vec<_>>();
    let hi = pitches.iter().max().copied().unwrap_or(60);
    let lo = pitches.iter().min().copied().unwrap_or(48);
    let rows = f32::from(hi.saturating_sub(lo).max(4)) + 1.0;
    for beat in 0..(span.ceil() as usize) {
        painter.vline(
            rect.left() + rect.width() * (beat as f64 / span) as f32,
            rect.y_range(),
            Stroke::new(1.0_f32, with_alpha(theme::BORDER_SUBTLE, 0.6)),
        );
    }
    let h = ((rect.height() - 8.0) / rows).max(3.0);
    for event in &pattern.events {
        let (at, len, row) = match event {
            PatternEvent::Note {
                at,
                pitch,
                duration,
                ..
            } => (at.as_f64(), duration.as_f64(), f32::from(hi - pitch)),
            PatternEvent::Hit { at, voice, .. } => (at.as_f64(), 0.12, *voice as u8 as f32),
            PatternEvent::Sample { at, duration, .. } => (at.as_f64(), duration.as_f64(), 0.0),
        };
        let x0 = rect.left() + rect.width() * (at / span) as f32;
        let x1 = rect.left() + rect.width() * ((at + len) / span) as f32;
        let y = rect.top() + 4.0 + row.min(rows - 1.0) * h;
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1.max(x0 + 3.0), y + h)),
            1.5,
            color,
        );
    }
}

fn section(ui: &mut Ui, kind: Kind, name: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    kind_chip(ui, kind);
                    ui.label(
                        RichText::new(name)
                            .strong()
                            .size(12.5)
                            .color(theme::TEXT_PRIMARY),
                    );
                });
                ui.add_space(8.0);
                body(ui);
            });
        });
}

/// The sound a version belongs to: synth dials, its track's level and pan, its effects and its
/// pattern, as `view.shown` has them.
fn sound_panel(ui: &mut Ui, view: &SoundView<'_>, focus: &Focus) {
    use crate::model::Unit;
    let song = &view.shown.song;
    let prev_song = view.previous.map(|p| &p.song);
    let other_song = view.other.map(|p| &p.song);
    let word = view.other_word;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(16.0, 16.0);
        if focus.tempo {
            section(ui, Kind::Song, "Song", |ui| {
                dial(
                    ui,
                    "Tempo",
                    song.tempo_bpm,
                    60.0,
                    180.0,
                    Unit::Ratio,
                    mark(
                        song.tempo_bpm,
                        prev_song.map(|s| s.tempo_bpm),
                        other_song.map(|s| s.tempo_bpm),
                    ),
                    word,
                    theme::TEXT_PRIMARY,
                );
            });
        }
        let track = focus
            .track
            .as_ref()
            .and_then(|id| song.tracks.iter().find(|t| t.id == *id));
        if let Some(inst) = focus
            .instrument
            .as_ref()
            .and_then(|id| song.instruments.iter().find(|i| i.id == *id))
        {
            let find = |s: Option<&crate::model::Song>, key: &str| {
                s.and_then(|s| s.instruments.iter().find(|i| i.id == inst.id))
                    .and_then(|i| i.parameters.iter().find(|p| p.key == key))
                    .map(|p| p.value)
            };
            let name = match track {
                Some(track) => format!("{} on {}", inst.name, track.name),
                None => inst.name.clone(),
            };
            section(ui, Kind::Instrument, &name, |ui| {
                ui.horizontal_top(|ui| {
                    for param in &inst.parameters {
                        dial(
                            ui,
                            &param.name,
                            param.value,
                            param.minimum,
                            param.maximum,
                            param.unit,
                            mark(
                                param.value,
                                find(prev_song, &param.key),
                                find(other_song, &param.key),
                            ),
                            word,
                            Kind::Instrument.color(),
                        );
                    }
                });
            });
        }
        if let Some(track) = track {
            let prev = prev_song.and_then(|s| s.tracks.iter().find(|t| t.id == track.id));
            let other = other_song.and_then(|s| s.tracks.iter().find(|t| t.id == track.id));
            section(ui, Kind::Mix, &track.name, |ui| {
                ui.horizontal_top(|ui| {
                    fader(
                        ui,
                        track.channel.level_db,
                        mark(
                            track.channel.level_db,
                            prev.map(|t| t.channel.level_db),
                            other.map(|t| t.channel.level_db),
                        ),
                        word,
                    );
                    let pan = track.channel.panner.primary_position();
                    dial(
                        ui,
                        "Pan",
                        pan,
                        -1.0,
                        1.0,
                        Unit::Pan,
                        mark(
                            pan,
                            prev.map(|t| t.channel.panner.primary_position()),
                            other.map(|t| t.channel.panner.primary_position()),
                        ),
                        word,
                        Kind::Mix.color(),
                    );
                });
            });
            if !track.channel.inserts.is_empty() {
                section(ui, Kind::Fx, &format!("Effects on {}", track.name), |ui| {
                    for processor in &track.channel.inserts {
                        let prev_p = prev
                            .and_then(|t| t.channel.inserts.iter().find(|p| p.id == processor.id));
                        let other_p = other
                            .and_then(|t| t.channel.inserts.iter().find(|p| p.id == processor.id));
                        effect_settings(ui, processor, prev_p, other_p, word);
                    }
                });
            }
        }
        if let Some(pattern) = focus
            .pattern
            .as_ref()
            .and_then(|id| song.patterns.iter().find(|p| p.id == *id))
        {
            section(ui, Kind::Pattern, &pattern.id, |ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(300.0, 96.0), Sense::hover());
                pattern_thumb(ui.painter(), rect, pattern, Kind::Pattern.color());
            });
        }
    });
}

/// One effect's settings as chips; edited ones lit with their previous value, ones that differ
/// from the other side outlined quietly.
fn effect_settings(
    ui: &mut Ui,
    processor: &crate::model::Processor,
    previous: Option<&crate::model::Processor>,
    other: Option<&crate::model::Processor>,
    other_word: &str,
) {
    ui.label(
        RichText::new(theme::processor_title(processor.kind))
            .strong()
            .size(11.5)
            .color(if processor.bypass {
                theme::TEXT_MUTED
            } else {
                theme::TEXT_PRIMARY
            }),
    );
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        for setting in &processor.settings {
            let value_in = |p: Option<&crate::model::Processor>| {
                p.and_then(|p| p.settings.iter().find(|s| s.key == setting.key))
                    .map(|s| s.value.clone())
                    .filter(|v| *v != setting.value)
            };
            let (text, lit, quiet) = match (value_in(previous), value_in(other)) {
                (Some(prev), _) => (
                    format!("{} {} (previous {prev})", setting.key, setting.value),
                    true,
                    false,
                ),
                (None, Some(other)) => (
                    format!("{} {} ({other_word} {other})", setting.key, setting.value),
                    false,
                    true,
                ),
                _ => (format!("{} {}", setting.key, setting.value), false, false),
            };
            let width = ui
                .painter()
                .layout_no_wrap(
                    text.clone(),
                    FontId::proportional(10.5),
                    theme::TEXT_PRIMARY,
                )
                .size()
                .x
                + 16.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 3.0, theme::BG_CARD);
            painter.rect_stroke(
                rect,
                3.0,
                Stroke::new(
                    1.0_f32,
                    if lit {
                        theme::SELECTION
                    } else if quiet {
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
                text,
                FontId::proportional(10.5),
                if lit || quiet {
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_SECONDARY
                },
            );
        }
    });
    ui.add_space(6.0);
}

/// A titled list of value changes: owner and control, the old value dimmed, the new one strong.
fn change_list(ui: &mut Ui, title: &str, changes: &[ValueChange], empty: &str) {
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title)
                    .strong()
                    .size(9.5)
                    .color(theme::TEXT_MUTED),
            );
            ui.add_space(6.0);
            if changes.is_empty() {
                ui.label(RichText::new(empty).size(11.0).color(theme::TEXT_MUTED));
            }
            for change in changes {
                ui.label(
                    RichText::new(format!("{} · {}", change.owner, change.label))
                        .size(11.0)
                        .color(theme::TEXT_SECONDARY),
                );
                ui.horizontal(|ui| {
                    if !change.before.is_empty() {
                        ui.label(
                            RichText::new(&change.before)
                                .monospace()
                                .size(12.0)
                                .color(theme::TEXT_MUTED),
                        );
                        ui.label(RichText::new("to").size(10.5).color(theme::TEXT_MUTED));
                    }
                    ui.label(
                        RichText::new(&change.after)
                            .monospace()
                            .strong()
                            .size(14.0)
                            .color(theme::TEXT_PRIMARY),
                    );
                });
                ui.add_space(4.0);
            }
        });
}

/// Every version as a tick, oldest on the left and Now at the right end, sessions as bands.
/// Ticks for the selected sound stand out; restores are diamonds. Drag anywhere to scrub.
fn scrub_rail(ui: &mut Ui, items: &[Item<'_>], selected: usize, pick: &mut Option<usize>) {
    let n = items.len().max(1);
    let height = 58.0;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        Sense::click_and_drag(),
    );
    let painter = ui.painter();
    let rail = Rect::from_min_max(
        Pos2::new(rect.left() + 14.0, rect.top()),
        Pos2::new(rect.right() - 56.0, rect.bottom()),
    );
    let slot = rail.width() / n as f32;
    let x_of = |index: usize| rail.right() - slot * (index as f32 + 0.5);
    painter.rect_filled(rect, 6.0, theme::BG_WELL);
    for (band, session) in sessions(items).iter().enumerate() {
        let oldest = &session[session.len() - 1];
        let x0 = x_of(oldest.index) - slot * 0.5;
        let x1 = x_of(session[0].index) + slot * 0.5;
        if band % 2 == 1 {
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(x0, rect.top()), Pos2::new(x1, rect.bottom())),
                0.0,
                with_alpha(theme::BG_SURFACE, 0.8),
            );
        }
        painter.text(
            Pos2::new(x0 + 6.0, rect.top() + 5.0),
            Align2::LEFT_TOP,
            format!("{} {}", day_label(oldest.at), clock(oldest.at)),
            FontId::proportional(9.5),
            theme::TEXT_MUTED,
        );
    }
    let y = rect.bottom() - 18.0;
    painter.hline(
        rail.x_range(),
        y,
        Stroke::new(2.0_f32, theme::BORDER_SUBTLE),
    );
    let focus = &items[selected];
    for item in items {
        let x = x_of(item.index);
        let related = item.same_subject(focus) || item.index == selected;
        let color = if related {
            item.kind.color()
        } else {
            with_alpha(item.kind.color(), 0.4)
        };
        if item.kind == Kind::Restore {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x, y - 7.0),
                    Pos2::new(x + 5.0, y),
                    Pos2::new(x, y + 7.0),
                    Pos2::new(x - 5.0, y),
                ],
                color,
                Stroke::NONE,
            ));
        } else {
            let tall = if related { 12.0 } else { 7.0 };
            painter.line_segment(
                [Pos2::new(x, y - tall), Pos2::new(x, y + 4.0)],
                Stroke::new(3.0_f32, color),
            );
        }
    }
    // Now: the right end of time.
    let now_x = rect.right() - 30.0;
    painter.text(
        Pos2::new(now_x, y),
        Align2::CENTER_CENTER,
        "Now",
        FontId::proportional(10.5),
        theme::TEXT_SECONDARY,
    );
    let px = x_of(selected);
    painter.vline(
        px,
        (rect.top() + 18.0)..=rect.bottom(),
        Stroke::new(2.0_f32, theme::SELECTION),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(px - 6.0, rect.top() + 18.0),
            Pos2::new(px + 6.0, rect.top() + 18.0),
            Pos2::new(px, rect.top() + 26.0),
        ],
        theme::SELECTION,
        Stroke::NONE,
    ));
    let index_at = |x: f32| (((rail.right() - x) / slot).floor().max(0.0) as usize).min(n - 1);
    if let Some(pos) = response.hover_pos() {
        let hovered = &items[index_at(pos.x)];
        response.clone().on_hover_text(format!(
            "{} — {}\n{} at {}",
            hovered.subject,
            hovered.action,
            day_label(hovered.at),
            clock(hovered.at)
        ));
    }
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        *pick = Some(index_at(pos.x));
    }
}

/// Older / Newer, Play this version, Then / Now and Restore.
fn transport(ui: &mut Ui, cx: &mut Cx<'_>, items: &[Item<'_>], item: &Item<'_>) {
    let app = cx.app;
    let selected = item.index;
    let hearing = app.history_preview.as_ref().map(|(id, _)| id.clone());
    let playing = app.player.is_some() || app.playback_requested;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        // Newest first: older is a higher index.
        if ui
            .add_enabled(selected + 1 < items.len(), egui::Button::new("Older"))
            .on_hover_text("Step to the version before this one")
            .clicked()
        {
            cx.select(selected, selected + 1);
        }
        if ui
            .add_enabled(selected > 0, egui::Button::new("Newer"))
            .on_hover_text("Step to the version after this one")
            .clicked()
        {
            cx.select(selected, selected - 1);
        }
        ui.add_space(8.0);
        let previewing = playing && hearing.is_some();
        let (rect, response) = ui.allocate_exact_size(Vec2::new(170.0, 30.0), Sense::click());
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            15.0,
            if response.hovered() {
                theme::BG_HOVER
            } else {
                theme::BG_ACTIVE
            },
        );
        let icon = Rect::from_center_size(
            Pos2::new(rect.left() + 18.0, rect.center().y),
            Vec2::splat(16.0),
        );
        if previewing {
            theme::paint_stop_icon(painter, icon, theme::TEXT_PRIMARY);
        } else {
            theme::paint_play_icon(painter, icon, theme::SIG_NOMINAL);
        }
        painter.text(
            Pos2::new(rect.left() + 34.0, rect.center().y),
            Align2::LEFT_CENTER,
            if previewing {
                "Stop"
            } else {
                "Play this version"
            },
            FontId::proportional(12.0),
            theme::TEXT_PRIMARY,
        );
        if response
            .on_hover_text("Hear the project as it was at this version, from the cursor")
            .clicked()
        {
            if previewing {
                cx.send(Action::StopPlayback);
            } else {
                cx.state.history_show_now = false;
                cx.send(Action::PreviewHistoryVersion(Some(item.entry.id.clone())));
                if !playing {
                    cx.send(Action::TogglePlayback);
                }
            }
        }
        // Then / Now swaps what the panel shows and, while a version plays, what you hear.
        let show_now = cx.state.history_show_now;
        for (label, now) in [("Then", false), ("Now", true)] {
            if widgets::pill_toggle(
                ui,
                label,
                show_now == now,
                if now {
                    "Show and hear the current version"
                } else {
                    "Show and hear this version"
                },
            )
            .clicked()
                && show_now != now
            {
                cx.state.history_show_now = now;
                if previewing || (playing && hearing.is_none() && !now) {
                    cx.send(Action::PreviewHistoryVersion(
                        (!now).then(|| item.entry.id.clone()),
                    ));
                }
            }
        }
        if previewing {
            ui.label(
                RichText::new(if show_now {
                    "Hearing now".to_owned()
                } else {
                    format!("Hearing version {}", items.len() - selected)
                })
                .size(11.0)
                .color(theme::TEXT_SECONDARY),
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let current = is_current(app, item.entry);
            let later = &items[..selected];
            let impact = if later.is_empty() {
                "This is the latest change.".to_owned()
            } else {
                format!(
                    "Restoring undoes the {} made after it ({}). They stay in History.",
                    plural(later.len(), "change"),
                    session_subjects(later)
                )
            };
            if ui
                .add_enabled(!current, egui::Button::new("Restore this version"))
                .on_hover_text(impact)
                .clicked()
            {
                cx.send(Action::OpenRestore);
            }
            if ui
                .add(egui::Button::new(RichText::new("Source diff").size(10.5)).frame(false))
                .on_hover_text("The source this change edited · d")
                .clicked()
            {
                cx.send(Action::OpenHistoryDiff);
            }
        });
    });
}

fn header(ui: &mut Ui, items: &[Item<'_>], item: &Item<'_>, number: usize, total: usize) {
    // A restore reads as where it went back to.
    let headline = restore_target(items, item).map_or_else(
        || format!("{} — {}", item.subject, item.action),
        |target| {
            format!(
                "Went back to {} — {} ({})",
                target.subject,
                target.action,
                clock(target.at)
            )
        },
    );
    ui.horizontal(|ui| {
        kind_chip(ui, item.kind);
        ui.label(
            RichText::new(headline)
                .strong()
                .size(17.0)
                .color(theme::TEXT_PRIMARY),
        );
    });
    ui.label(
        RichText::new(format!(
            "Version {number} of {total} · {} · {} at {} · {}",
            who(item.entry),
            day_label(item.at),
            clock(item.at),
            ago(item.at)
        ))
        .size(11.0)
        .color(theme::TEXT_SECONDARY),
    );
    // Only an agent's note says something; workspace edits carry a fixed one.
    if item.entry.origin.kind == OriginKind::Agent && !item.entry.explanation.is_empty() {
        ui.label(
            RichText::new(format!("“{}”", item.entry.explanation))
                .italics()
                .size(11.5)
                .color(theme::TEXT_SECONDARY),
        );
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    #[test]
    fn a_version_reads_as_what_it_moved() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/deep-house-reference");
        let (_dir, mut app) = super::super::tests::seed_real_history(&root);
        let drive = app
            .history
            .iter()
            .find(|entry| entry.summary.contains("dh-bass.drive"))
            .expect("the drive edit")
            .clone();
        let store = crate::history::HistoryStore::open(&app.root).unwrap();
        let after = store.bundle_at(&drive.id).unwrap();
        let before = store.bundle_at(drive.parent.as_ref().unwrap()).unwrap();
        let changes = value_changes(before.project.as_ref(), after.project.as_ref().unwrap());
        let read = changes
            .iter()
            .map(|c| format!("{} · {}: {} to {}", c.owner, c.label, c.before, c.after))
            .collect::<Vec<_>>();
        assert_eq!(read, ["Deep House Bass · Drive: 20% to 42%"]);
        let focus = focus_of(after.project.as_ref().unwrap(), &changes).unwrap();
        assert_eq!(focus.instrument.as_deref(), Some("dh-bass"));
        assert_eq!(focus.track.as_deref(), Some("bass"));
        app.shutdown();
    }

    #[test]
    fn timestamps_and_offsets_parse() {
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z"), 0);
        assert_eq!(unix_seconds("2026-11-23T14:05:09Z"), 1_795_442_709);
        assert_eq!(unix_seconds("2024-02-29T23:59:59Z"), 1_709_251_199);
        assert_eq!(parse_offset("-0800"), Some(-8 * 3600));
        assert_eq!(parse_offset("+0530"), Some(5 * 3600 + 30 * 60));
        assert_eq!(parse_offset("UTC"), None);
    }
}
