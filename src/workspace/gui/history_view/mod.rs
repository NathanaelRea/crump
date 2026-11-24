//! Project History read as a musician's log: working sessions, each a list of the things changed,
//! with back-to-back edits to one thing folded into a run that expands. A side inspector explains
//! the selected change in words and offers Compare and Restore.

use std::sync::OnceLock;

use super::theme::{self, with_alpha};
use super::widgets;
use super::*;
use crate::history::{HistoryEntry, OriginKind};
use crate::model::Project;

mod console;
mod fx_rack;
mod song;
mod synth;

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
                .map(|(_, v)| format!(" to {}", readable_literal(v)))
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
/// A scrub rail across the top holds every version; below it, every part of the project that
/// version changed, as it was: instrument dials, channel strips, effect chains and patterns, each
/// changed control showing where it came from. Versions are parsed on background threads and
/// not at all while the rail is being dragged, so scrubbing stays smooth. Play auditions that
/// version in place of the current one, Then / Now swaps view and sound between them, and
/// Restore brings it back for good.
pub(super) fn history(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let items = items(app);
    let selected = clamp_index(app.selected_history, items.len());
    let item = &items[selected];
    let dragging = cx.state.history_dragging;
    // While the rail is dragged only versions already parsed are shown; the rest load on release.
    let load = |id: &str| {
        if dragging {
            app.history_project_peek(id)
        } else {
            app.history_project(id)
        }
    };
    let this = load(&item.entry.id);
    let previous = item.entry.parent.as_deref().and_then(load);
    let now = app.active_entry.as_deref().and_then(load);
    if app.history_projects_loading() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
    let mut pick: Option<usize> = None;

    // While playing, what you hear follows the page once the playhead settles: Then plays the
    // version at the playhead, Now the current one.
    let playing = app.player.is_some() || app.playback_requested;
    let heard = app.history_preview.as_ref().map(|(id, _)| id);
    if playing && !dragging {
        if cx.state.history_show_now || is_current(app, item.entry) {
            if heard.is_some() {
                cx.send(Action::PreviewHistoryVersion(None));
            }
        } else if heard != Some(&item.entry.id) && this.is_some() {
            cx.send(Action::PreviewHistoryVersion(Some(item.entry.id.clone())));
        }
    }

    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(22, 14))
        .show(ui, |ui| {
            scrub_rail(ui, cx, &items, selected, &mut pick);
            ui.add_space(10.0);
            transport(ui, cx, &items, item);
            ui.add_space(12.0);
            // The rail and transport stay put; the version below scrolls.
            egui::ScrollArea::vertical()
                .id_salt("history-version")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let Some(this) = this.as_deref() else {
                        header(ui, &items, item, None, items.len() - selected, items.len());
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new(if dragging {
                                "Release to show this version"
                            } else {
                                "Loading this version…"
                            })
                            .color(theme::TEXT_MUTED),
                        );
                        return;
                    };
                    let this_edit = cached_changes(
                        cx,
                        item.entry.parent.as_deref(),
                        previous.as_deref(),
                        &item.entry.id,
                        this,
                    );
                    header(
                        ui,
                        &items,
                        item,
                        this_edit.first(),
                        items.len() - selected,
                        items.len(),
                    );
                    ui.add_space(12.0);
                    let since = match (&app.active_entry, now.as_deref()) {
                        (Some(active), Some(now)) => {
                            cached_changes(cx, Some(&item.entry.id), Some(this), active, now)
                        }
                        _ => Vec::new(),
                    };
                    // Narrow windows stack the lists under the sound instead of beside it.
                    let stacked = ui.available_width() < 980.0;
                    let showing_now = cx.state.history_show_now;
                    let view = SoundView {
                        shown: if showing_now {
                            now.as_deref().unwrap_or(this)
                        } else {
                            this
                        },
                        previous: if showing_now {
                            None
                        } else {
                            previous.as_deref()
                        },
                        other: if showing_now {
                            Some(this)
                        } else {
                            now.as_deref()
                        },
                        other_word: if showing_now { "then" } else { "now" },
                    };
                    let mut areas = touched_areas(&this_edit);
                    if areas.is_empty() {
                        areas = named_areas(this, item);
                    }
                    let panel = |ui: &mut Ui| sound_panel(ui, &view, &areas, &this_edit);
                    let lists = |ui: &mut Ui| {
                        let waiting = item.entry.parent.is_some() && previous.is_none();
                        change_list(
                            ui,
                            "THIS EDIT",
                            &this_edit,
                            if waiting {
                                "Loading…"
                            } else {
                                "No values moved."
                            },
                        );
                        ui.add_space(14.0);
                        let title = if since.is_empty() {
                            "SINCE THIS VERSION".to_owned()
                        } else {
                            format!("SINCE THIS VERSION · {}", plural(since.len(), "difference"))
                        };
                        let now_loading = app.active_entry.is_some() && now.is_none();
                        change_list(
                            ui,
                            &title,
                            &since,
                            if now_loading {
                                "Loading…"
                            } else {
                                "This is the version you hear now."
                            },
                        );
                    };
                    if stacked {
                        panel(ui);
                        ui.add_space(14.0);
                        lists(ui);
                    } else {
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width() - SIDE_WIDTH - 16.0);
                                panel(ui);
                            });
                            ui.vertical(|ui| {
                                ui.set_width(SIDE_WIDTH);
                                lists(ui);
                            });
                        });
                    }
                });
        });

    if let Some(index) = pick {
        // Moving the playhead looks at that version.
        cx.state.history_show_now = false;
        cx.select(selected, index);
    }
}

const SIDE_WIDTH: f32 = 340.0;

/// Differences between two versions, computed once per pair.
fn cached_changes(
    cx: &mut Cx<'_>,
    from_id: Option<&str>,
    from: Option<&Project>,
    to_id: &str,
    to: &Project,
) -> Vec<ValueChange> {
    let key = (from_id.unwrap_or("").to_owned(), to_id.to_owned());
    if let Some(changes) = cx.state.history_diffs.get(&key) {
        return changes.clone();
    }
    let changes = value_changes(from, to);
    // Cache only a real comparison: an earlier version still loading is not "no change".
    if from_id.is_some() && from.is_none() {
        return changes;
    }
    if cx.state.history_diffs.len() > 256 {
        cx.state.history_diffs.clear();
    }
    cx.state.history_diffs.insert(key, changes.clone());
    changes
}

/// What part of the project a change landed in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Area {
    Tempo,
    Instrument(String),
    Track(String),
    Group(String),
    Return(String),
    Master,
    Pattern(String),
    /// A track's clips on the timeline.
    Arrangement(String),
    Other(String),
}

/// One value that differs between two versions.
#[derive(Clone, Debug)]
pub(super) struct ValueChange {
    area: Area,
    owner: String,
    label: String,
    before: String,
    after: String,
    /// The changed control's path inside its area (`parameters/drive/value`), to light it.
    path: Vec<String>,
}

/// Every value that differs from `before` to `after`, in plain words. The comparison walks the
/// whole Project, so it covers anything a version can change; source positions are ignored.
fn value_changes(before: Option<&Project>, after: &Project) -> Vec<ValueChange> {
    let Some(before) = before else {
        return Vec::new();
    };
    let (Ok(a), Ok(b)) = (serde_json::to_value(before), serde_json::to_value(after)) else {
        return Vec::new();
    };
    let mut raw = Vec::new();
    diff_json(&mut Vec::new(), &a, &b, &mut raw);
    raw.into_iter()
        .map(|(path, old, new)| describe_change(before, after, &path, old, new))
        .collect()
}

type RawChange = (
    Vec<String>,
    Option<serde_json::Value>,
    Option<serde_json::Value>,
);

/// Source bookkeeping that moves whenever a file is edited, never a musical value.
fn is_bookkeeping(key: &str) -> bool {
    key == "location" || key == "locations" || key.ends_with("site") || key.ends_with("sites")
}

/// Whether a serialized object is a `MusicalTime` (a fraction of quarter notes).
fn is_time(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    object.len() == 2 && object.contains_key("numerator") && object.contains_key("denominator")
}

fn element_id(value: &serde_json::Value) -> Option<&str> {
    value
        .get("id")
        .or_else(|| value.get("key"))
        .and_then(serde_json::Value::as_str)
}

fn diff_json(
    path: &mut Vec<String>,
    a: &serde_json::Value,
    b: &serde_json::Value,
    out: &mut Vec<RawChange>,
) {
    use serde_json::Value;
    const MAX: usize = 400;
    if a == b || out.len() >= MAX {
        return;
    }
    match (a, b) {
        // A musical time is one value, not a numerator and a denominator.
        (Value::Object(x), Value::Object(y)) if !is_time(x) || !is_time(y) => {
            let keys = y.keys().chain(x.keys().filter(|k| !y.contains_key(*k)));
            for key in keys.cloned().collect::<Vec<_>>() {
                if is_bookkeeping(&key) {
                    continue;
                }
                path.push(key.clone());
                match (x.get(&key), y.get(&key)) {
                    (Some(l), Some(r)) => diff_json(path, l, r, out),
                    (l, r) => out.push((path.clone(), l.cloned(), r.cloned())),
                }
                path.pop();
            }
        }
        (Value::Array(x), Value::Array(y))
            if x.iter().chain(y).all(|v| element_id(v).is_some()) =>
        {
            for r in y {
                let id = element_id(r).unwrap_or_default().to_owned();
                path.push(id.clone());
                match x.iter().find(|l| element_id(l) == Some(&id)) {
                    Some(l) => diff_json(path, l, r, out),
                    None => out.push((path.clone(), None, Some(r.clone()))),
                }
                path.pop();
            }
            for l in x {
                let id = element_id(l).unwrap_or_default();
                if !y.iter().any(|r| element_id(r) == Some(id)) {
                    path.push(id.to_owned());
                    out.push((path.clone(), Some(l.clone()), None));
                    path.pop();
                }
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (index, (l, r)) in x.iter().zip(y).enumerate() {
                path.push(format!("#{}", index + 1));
                diff_json(path, l, r, out);
                path.pop();
            }
        }
        _ => out.push((path.clone(), Some(a.clone()), Some(b.clone()))),
    }
}

/// A raw difference read as owner, control and values in the Project's own words and units.
fn describe_change(
    before: &Project,
    after: &Project,
    path: &[String],
    old: Option<serde_json::Value>,
    new: Option<serde_json::Value>,
) -> ValueChange {
    use crate::model::Unit;
    let song = &after.song;
    let seg = |i: usize| path.get(i).map(String::as_str).unwrap_or("");
    let rest = |from: usize| path.get(from..).map(<[String]>::to_vec).unwrap_or_default();
    let fmt = |value: &Option<serde_json::Value>| match value {
        None => String::new(),
        Some(v) => json_text(v),
    };
    let (mut before_text, mut after_text) = match (&old, &new) {
        (None, Some(_)) => (String::new(), "added".to_owned()),
        (Some(_), None) => (String::new(), "removed".to_owned()),
        _ => (fmt(&old), fmt(&new)),
    };
    let unit_text = |unit: Unit, v: &Option<serde_json::Value>| {
        v.as_ref()
            .and_then(serde_json::Value::as_f64)
            .map(|v| theme::format_compact_value(v, unit))
    };
    let mut retext = |unit: Unit| {
        if let (Some(a), Some(b)) = (unit_text(unit, &old), unit_text(unit, &new)) {
            before_text = a;
            after_text = b;
        }
    };
    let (area, owner, inner) = match (seg(0), seg(1)) {
        ("song", "tempo_bpm") => {
            let bpm = |v: &Option<serde_json::Value>| {
                v.as_ref()
                    .and_then(serde_json::Value::as_f64)
                    .map(|v| format!("{v:.1} BPM"))
            };
            if let (Some(a), Some(b)) = (bpm(&old), bpm(&new)) {
                before_text = a;
                after_text = b;
            }
            (Area::Tempo, "Song".to_owned(), vec!["Tempo".to_owned()])
        }
        ("song", "instruments") => {
            let id = seg(2).to_owned();
            let inst = song
                .instruments
                .iter()
                .find(|i| i.id == id)
                .or_else(|| before.song.instruments.iter().find(|i| i.id == id));
            let inner = rest(3);
            if inner.len() == 3
                && inner[0] == "parameters"
                && inner[2] == "value"
                && let Some(param) =
                    inst.and_then(|i| i.parameters.iter().find(|p| p.key == inner[1]))
            {
                retext(param.unit);
            }
            let owner = inst.map_or_else(|| id.clone(), |i| i.name.clone());
            (Area::Instrument(id), owner, inner)
        }
        (scope @ ("tracks" | "groups" | "returns"), _)
        | ("song", scope @ ("tracks" | "groups" | "returns")) => {
            let id = seg(2).to_owned();
            let name = match scope {
                "tracks" => song
                    .tracks
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.name.clone()),
                "groups" => song
                    .groups
                    .iter()
                    .find(|g| g.id == id)
                    .map(|g| g.name.clone()),
                _ => song
                    .returns
                    .iter()
                    .find(|r| r.id == id)
                    .map(|r| r.name.clone()),
            };
            let inner = rest(3);
            if inner.last().is_some_and(|k| k == "level_db") {
                retext(Unit::Decibels);
            } else if inner.first().is_some_and(|k| k == "channel")
                && inner.get(1).is_some_and(|k| k == "panner")
            {
                retext(Unit::Pan);
            }
            let area = match scope {
                "tracks" if inner.first().is_some_and(|k| k == "clips") => {
                    Area::Arrangement(id.clone())
                }
                "tracks" => Area::Track(id.clone()),
                "groups" => Area::Group(id.clone()),
                _ => Area::Return(id.clone()),
            };
            (area, name.unwrap_or(id), inner)
        }
        ("song", "master") => {
            let inner = rest(2);
            if inner.last().is_some_and(|k| k == "level_db") {
                retext(Unit::Decibels);
            }
            (Area::Master, "Master".to_owned(), inner)
        }
        ("song", "patterns") => (Area::Pattern(seg(2).to_owned()), seg(2).to_owned(), rest(3)),
        ("song", collection) => (
            Area::Other(collection.to_owned()),
            title(&collection.replace('_', " ")),
            rest(2),
        ),
        (top, _) => (Area::Other(top.to_owned()), title(top), rest(1)),
    };
    ValueChange {
        label: control_label(after, &area, &inner),
        area,
        owner,
        before: before_text,
        after: after_text,
        path: inner,
    }
}

/// A path inside an area as words: `channel/inserts/bass-color/settings/drive/value` reads
/// "Saturator · Drive".
fn control_label(project: &Project, area: &Area, path: &[String]) -> String {
    const NOISE: [&str; 8] = [
        "points",
        "channel",
        "settings",
        "value",
        "inserts",
        "post_processors",
        "panner",
        "graph",
    ];
    // Instrument parameters by their names.
    if let (Area::Instrument(id), ["parameters", key, ..]) = (
        area,
        path.iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice(),
    ) && let Some(param) = project
        .song
        .instruments
        .iter()
        .find(|i| i.id == *id)
        .and_then(|i| i.parameters.iter().find(|p| p.key == *key))
    {
        return param.name.clone();
    }
    let mut words: Vec<String> = Vec::new();
    for (i, segment) in path.iter().enumerate() {
        if NOISE.contains(&segment.as_str()) {
            continue;
        }
        // An effect by its kind name.
        if i > 0
            && matches!(path[i - 1].as_str(), "inserts" | "post_processors")
            && let Some((_, processor)) = processors(project)
                .into_iter()
                .find(|(_, p)| p.id == *segment)
        {
            words.push(theme::processor_title(processor.kind).to_owned());
            continue;
        }
        let word = match segment.as_str() {
            "level_db" => "Level".to_owned(),
            "at" => "Position".to_owned(),
            "position" => "Pan".to_owned(),
            "left" => "Pan left".to_owned(),
            "right" => "Pan right".to_owned(),
            other => title(&other.replace('_', " ")),
        };
        if words.last() != Some(&word) {
            words.push(word);
        }
    }
    if words.is_empty() {
        "Value".to_owned()
    } else {
        words.join(" · ")
    }
}

fn json_text(value: &serde_json::Value) -> String {
    use serde_json::Value;
    match value {
        Value::Number(n) => n
            .as_f64()
            .map(|f| {
                let text = format!("{f:.3}");
                text.trim_end_matches('0').trim_end_matches('.').to_owned()
            })
            .unwrap_or_else(|| n.to_string()),
        Value::String(s) => readable_literal(s),
        Value::Bool(b) => if *b { "on" } else { "off" }.to_owned(),
        Value::Null => "none".to_owned(),
        Value::Array(items) => plural(items.len(), "item"),
        Value::Object(object) if is_time(object) => {
            let part = |key: &str| object.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            let quarters = part("numerator") / part("denominator").max(1.0);
            let text = format!("{quarters:.3}");
            format!("{} qn", text.trim_end_matches('0').trim_end_matches('.'))
        }
        Value::Object(_) => "changed".to_owned(),
    }
}

/// Each area a set of changes touched, in order, once.
fn touched_areas(changes: &[ValueChange]) -> Vec<Area> {
    let mut areas: Vec<Area> = Vec::new();
    for change in changes {
        if !areas.contains(&change.area) {
            areas.push(change.area.clone());
        }
    }
    areas
}

/// The area a change names, for edits that moved no value (structure, arrangement).
fn named_areas(project: &Project, item: &Item<'_>) -> Vec<Area> {
    let song = &project.song;
    let area = match item.kind {
        Kind::Instrument => song
            .instruments
            .iter()
            .find(|i| i.name == item.subject)
            .map(|i| Area::Instrument(i.id.clone())),
        Kind::Mix => song
            .tracks
            .iter()
            .find(|t| t.name == item.subject)
            .map(|t| Area::Track(t.id.clone())),
        Kind::Arrangement => song
            .tracks
            .iter()
            .find(|t| t.name == item.subject)
            .map(|t| Area::Arrangement(t.id.clone())),
        Kind::Pattern => Some(Area::Pattern(item.subject.clone())),
        Kind::Song => Some(Area::Tempo),
        _ => None,
    };
    area.into_iter().collect()
}

/// Every effect chain with the name of what it sits on: tracks, groups, returns, the Master and
/// instruments' own effects.
fn chains(project: &Project) -> Vec<(String, &[crate::model::Processor])> {
    let song = &project.song;
    let mut out: Vec<(String, &[crate::model::Processor])> = Vec::new();
    out.extend(
        song.tracks
            .iter()
            .map(|t| (t.name.clone(), t.channel.inserts.as_slice())),
    );
    out.extend(
        song.groups
            .iter()
            .map(|g| (g.name.clone(), g.channel.inserts.as_slice())),
    );
    out.extend(
        song.returns
            .iter()
            .map(|r| (r.name.clone(), r.channel.inserts.as_slice())),
    );
    out.extend(
        song.master
            .iter()
            .map(|m| ("Master".to_owned(), m.inserts.as_slice())),
    );
    out.extend(
        song.instruments
            .iter()
            .map(|i| (i.name.clone(), i.post_processors.as_slice())),
    );
    out
}

/// Every effect with the name of what it sits on.
fn processors(project: &Project) -> Vec<(String, &crate::model::Processor)> {
    chains(project)
        .into_iter()
        .flat_map(|(host, chain)| chain.iter().map(move |p| (host.clone(), p)))
        .collect()
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
    text: &dyn Fn(f64) -> String,
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
        // An arc with no length (a centred pan, an unmoved value) draws nothing: a line whose
        // points all coincide has no direction and tessellates into garbage.
        let arc = |from: f32, to: f32| {
            if (to - from).abs() < 1e-4 {
                return Vec::new();
            }
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
        // Pan and other centred ranges fill out from the middle, as the mixer's do.
        let origin = if widgets::fills_from_centre(unit, min, max) {
            theme::value_to_norm(0.0, min, max, unit)
        } else {
            0.0
        };
        painter.add(egui::Shape::line(
            arc(origin.min(norm), origin.max(norm)),
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
                RichText::new(text(value))
                    .monospace()
                    .size(11.5)
                    .color(theme::TEXT_PRIMARY),
            );
            let note = match mark {
                Some(Mark::Edited(prev)) => {
                    format!("previous {}", text(prev))
                }
                Some(Mark::Differs(other)) => {
                    format!("{other_word} {}", text(other))
                }
                None => String::new(),
            };
            // An unmarked control keeps the note's line, empty, so rows stay aligned.
            if note.is_empty() {
                ui.add_space(ui.text_style_height(&egui::TextStyle::Body));
            } else {
                ui.label(RichText::new(note).size(10.0).color(theme::TEXT_MUTED));
            }
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
        if let Some(Mark::Differs(_)) = mark {
            painter.rect_stroke(
                cap.expand(3.0),
                4.0,
                Stroke::new(1.0_f32, theme::BORDER_STRONG),
                StrokeKind::Outside,
            );
        }
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
                None => String::new(),
            };
            // An unmarked control keeps the note's line, empty, so rows stay aligned.
            if note.is_empty() {
                ui.add_space(ui.text_style_height(&egui::TextStyle::Body));
            } else {
                ui.label(RichText::new(note).size(10.0).color(theme::TEXT_MUTED));
            }
        });
    });
}

/// The pan dials a panner shows: one, or one per side for a stereo panner.
fn pan_dials(panner: &crate::model::Panner) -> Vec<(&'static str, f64)> {
    match panner {
        crate::model::Panner::StereoPan { left, right, .. } => {
            vec![("Pan left", *left), ("Pan right", *right)]
        }
        other => vec![("Pan", other.primary_position())],
    }
}

fn section(ui: &mut Ui, kind: Kind, name: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            // One card per row, as wide as the column: wrapping inside, never past the edge.
            ui.set_width(ui.available_width());
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

/// Every area a version changed, as the modules that own it: the synth for an instrument, the
/// console (with each channel's FX rack) for tracks, groups, returns and the Master, and the
/// song modules for tempo, patterns, arrangement, automation and the rest. Each draws the
/// version in `view.shown` and marks what this edit moved and what differs from the other side.
fn sound_panel(ui: &mut Ui, view: &SoundView<'_>, areas: &[Area], changes: &[ValueChange]) {
    if areas.is_empty() {
        ui.label(
            RichText::new("This change has nothing to show as controls.").color(theme::TEXT_MUTED),
        );
        return;
    }
    let in_area = |area: &Area| {
        changes
            .iter()
            .filter(|c| c.area == *area)
            .collect::<Vec<_>>()
    };
    // The console draws every channel at once, so its areas go to it together.
    let console_areas = areas
        .iter()
        .filter(|a| {
            matches!(
                a,
                Area::Track(_) | Area::Group(_) | Area::Return(_) | Area::Master
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut console_drawn = false;
    for area in areas {
        match area {
            Area::Instrument(id) => synth::draw(ui, view, id, &in_area(area)),
            Area::Track(_) | Area::Group(_) | Area::Return(_) | Area::Master => {
                if console_drawn {
                    continue;
                }
                console_drawn = true;
                let console_changes = changes
                    .iter()
                    .filter(|c| console_areas.contains(&c.area))
                    .collect::<Vec<_>>();
                console::draw(ui, view, &console_areas, &console_changes);
            }
            Area::Tempo | Area::Pattern(_) | Area::Arrangement(_) | Area::Other(_) => {
                song::draw(ui, view, area, &in_area(area));
            }
        }
        ui.add_space(12.0);
    }
}

/// A track, group or return's name and channel strip in a version.
fn channel_strip<'a>(
    song: &'a crate::model::Song,
    area: &Area,
) -> Option<(String, &'a crate::model::ChannelStrip)> {
    match area {
        Area::Track(id) => song
            .tracks
            .iter()
            .find(|t| t.id == *id)
            .map(|t| (t.name.clone(), &t.channel)),
        Area::Group(id) => song
            .groups
            .iter()
            .find(|g| g.id == *id)
            .map(|g| (g.name.clone(), &g.channel)),
        Area::Return(id) => song
            .returns
            .iter()
            .find(|r| r.id == *id)
            .map(|r| (r.name.clone(), &r.channel)),
        _ => None,
    }
}

/// Changes with no control of their own, as lit chips: "Filter · Cutoff 420 Hz → 520 Hz".
fn change_chips(ui: &mut Ui, changes: &[&ValueChange]) {
    if changes.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        for change in changes {
            let text = if change.before.is_empty() {
                format!("{} {}", change.label, change.after)
            } else {
                format!(
                    "{} {} (previous {})",
                    change.label, change.after, change.before
                )
            };
            setting_chip(ui, &text, true, false);
        }
    });
}

/// A setting literal as people read it: `-25db` as `-25 dB`, `105ms` as `105 ms`.
fn readable_literal(value: &str) -> String {
    // Edits from dragging carry every digit (0.6625022888183594); three decimals read fine.
    let short = |number: &str| {
        number.parse::<f64>().ok().map(|n| {
            let text = format!("{n:.3}");
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        })
    };
    for (suffix, unit) in [
        ("db", " dB"),
        ("khz", " kHz"),
        ("hz", " Hz"),
        ("ms", " ms"),
        ("st", " st"),
        ("s", " s"),
    ] {
        if let Some(number) = value.strip_suffix(suffix)
            && let Some(number) = short(number)
        {
            return format!("{number}{unit}");
        }
    }
    short(value).unwrap_or_else(|| value.replace('_', " "))
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
        // Bypass, when it is on or moved, as its own chip.
        let bypass_word = |on: bool| if on { "bypassed" } else { "on" };
        let bypass_prev = previous
            .map(|p| p.bypass)
            .filter(|b| *b != processor.bypass);
        let bypass_other = other.map(|p| p.bypass).filter(|b| *b != processor.bypass);
        if processor.bypass || bypass_prev.is_some() || bypass_other.is_some() {
            let text = match (bypass_prev, bypass_other) {
                (Some(prev), _) => format!(
                    "{} (previous {})",
                    bypass_word(processor.bypass),
                    bypass_word(prev)
                ),
                (None, Some(other)) => format!(
                    "{} ({other_word} {})",
                    bypass_word(processor.bypass),
                    bypass_word(other)
                ),
                _ => bypass_word(processor.bypass).to_owned(),
            };
            setting_chip(ui, &text, bypass_prev.is_some(), bypass_other.is_some());
        }
        for setting in &processor.settings {
            let value_in = |p: Option<&crate::model::Processor>| {
                p.and_then(|p| p.settings.iter().find(|s| s.key == setting.key))
                    .map(|s| s.value.clone())
                    .filter(|v| *v != setting.value)
            };
            let value = readable_literal(&setting.value);
            let (text, lit, quiet) = match (value_in(previous), value_in(other)) {
                (Some(prev), _) => (
                    format!(
                        "{} {value} (previous {})",
                        setting.key,
                        readable_literal(&prev)
                    ),
                    true,
                    false,
                ),
                (None, Some(other)) => (
                    format!(
                        "{} {value} ({other_word} {})",
                        setting.key,
                        readable_literal(&other)
                    ),
                    false,
                    true,
                ),
                _ => (format!("{} {value}", setting.key), false, false),
            };
            setting_chip(ui, &text, lit, quiet);
        }
    });
    ui.add_space(6.0);
}

/// One effect setting as a chip: lit when this edit moved it, outlined when it differs from the
/// other side of Then / Now.
fn setting_chip(ui: &mut Ui, text: &str, lit: bool, quiet: bool) {
    let width = ui
        .painter()
        .layout_no_wrap(
            text.to_owned(),
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
                    // Values in mono; a sentence ("1 of 4 events changed") in the text font.
                    let after = RichText::new(&change.after)
                        .strong()
                        .color(theme::TEXT_PRIMARY);
                    ui.label(if change.before.is_empty() {
                        after.size(12.5)
                    } else {
                        after.monospace().size(14.0)
                    });
                });
                ui.add_space(4.0);
            }
        });
}

/// Every version as a tick, oldest on the left and Now at the right end, sessions as bands.
/// Ticks for the selected sound stand out; restores are diamonds. Drag anywhere to scrub.
fn scrub_rail(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    items: &[Item<'_>],
    selected: usize,
    pick: &mut Option<usize>,
) {
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
    let mut label_end = f32::NEG_INFINITY;
    for (band, session) in sessions(items).iter().enumerate().rev() {
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
        // Labels left to right, each skipped if it would run into the one before.
        if x0 + 6.0 > label_end {
            let label = painter.text(
                Pos2::new(x0 + 6.0, rect.top() + 5.0),
                Align2::LEFT_TOP,
                format!("{} {}", day_label(oldest.at), clock(oldest.at)),
                FontId::proportional(9.5),
                theme::TEXT_MUTED,
            );
            label_end = label.right() + 8.0;
        }
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
                Stroke::new(3.0_f32.min(slot * 0.6).max(1.0), color),
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
    // While dragging, versions not yet parsed wait for the release (see `history`).
    cx.state.history_dragging = response.dragged();
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
    let playing = app.player.is_some() || app.playback_requested;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        // Newest first: older is a higher index.
        if ui
            .add_enabled(selected + 1 < items.len(), egui::Button::new("Older"))
            .on_hover_text("Step to the version before this one")
            .clicked()
        {
            cx.state.history_show_now = false;
            cx.select(selected, selected + 1);
        }
        if ui
            .add_enabled(selected > 0, egui::Button::new("Newer"))
            .on_hover_text("Step to the version after this one")
            .clicked()
        {
            cx.state.history_show_now = false;
            cx.select(selected, selected - 1);
        }
        ui.add_space(8.0);
        // Play starts from the cursor; what it plays follows Then / Now.
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
        if playing {
            theme::paint_stop_icon(painter, icon, theme::TEXT_PRIMARY);
        } else {
            theme::paint_play_icon(painter, icon, theme::SIG_NOMINAL);
        }
        let show_now = cx.state.history_show_now;
        painter.text(
            Pos2::new(rect.left() + 34.0, rect.center().y),
            Align2::LEFT_CENTER,
            match (playing, show_now) {
                (true, _) => "Stop",
                (false, false) => "Play this version",
                (false, true) => "Play now",
            },
            FontId::proportional(12.0),
            theme::TEXT_PRIMARY,
        );
        if response
            .on_hover_text("Hear the project as it was at this version, from the cursor")
            .clicked()
        {
            if playing {
                cx.send(Action::StopPlayback);
            } else if show_now || is_current(app, item.entry) {
                // The current version plays as itself.
                cx.send(Action::TogglePlayback);
            } else {
                cx.send(Action::PlayHistoryVersion(item.entry.id.clone()));
            }
        }
        // Then / Now swaps what the panel shows and, while playing, what you hear. On the
        // current version both are the same, so the switch steps aside.
        let current = is_current(app, item.entry);
        for (label, now) in [("Then", false), ("Now", true)] {
            if ui
                .add_enabled_ui(!current, |ui| {
                    widgets::pill_toggle(
                        ui,
                        label,
                        show_now == now,
                        if now {
                            "Show and hear the current version"
                        } else {
                            "Show and hear this version"
                        },
                    )
                })
                .inner
                .clicked()
            {
                cx.state.history_show_now = now;
            }
        }
        if playing {
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

fn header(
    ui: &mut Ui,
    items: &[Item<'_>],
    item: &Item<'_>,
    measured: Option<&ValueChange>,
    number: usize,
    total: usize,
) {
    // A restore reads as where it went back to; a summary not recognised reads as what it
    // measurably changed.
    let named = match (item.kind, measured) {
        (Kind::Other, Some(change)) => format!("{} — {}", change.owner, change.label),
        _ => format!("{} — {}", item.subject, item.action),
    };
    let headline = restore_target(items, item).map_or_else(
        || named.clone(),
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
        let store = crate::history::HistoryStore::open(&app.root).unwrap();
        let read = |summary: &str| {
            let entry = app
                .history
                .iter()
                .find(|entry| entry.summary.contains(summary))
                .expect("the edit")
                .clone();
            let after = store.project_at(&entry.id).unwrap().unwrap();
            let before = store.project_at(entry.parent.as_ref().unwrap()).unwrap();
            value_changes(before.as_ref(), &after)
                .iter()
                .map(|c| format!("{} · {}: {} to {}", c.owner, c.label, c.before, c.after))
                .collect::<Vec<_>>()
        };
        // The values each edit set; what they replaced depends on the copied Project.
        let set = |summary: &str| {
            read(summary)
                .into_iter()
                .map(|line| {
                    let (what, rest) = line.split_once(": ").unwrap_or((&line, ""));
                    let to = rest.rsplit_once(" to ").map_or(rest, |(_, to)| to);
                    format!("{what} to {to}")
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(set("dh-bass.drive"), ["Deep House Bass · Drive to 42%"]);
        assert_eq!(
            set("bass-color.drive"),
            ["Sub and Analog Bass · Saturator · Drive to 1.4"],
            "an effect on a track"
        );
        assert_eq!(
            set("track/bass/level"),
            ["Sub and Analog Bass · Level to -9.0 dB"]
        );
        app.shutdown();
    }

    #[test]
    fn a_change_anywhere_lands_in_its_area() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/deep-house-reference");
        let before = crate::project::load_project(&root)
            .unwrap()
            .project
            .unwrap();
        let mut after = before.clone();
        after.song.groups[0].channel.level_db -= 2.0;
        let processor = &mut after.song.groups[0].channel.inserts[0];
        processor.bypass = !processor.bypass;
        let changes = value_changes(Some(&before), &after);
        let group = Area::Group(after.song.groups[0].id.clone());
        assert!(changes.len() >= 2, "{changes:?}");
        assert!(changes.iter().all(|c| c.area == group), "{changes:?}");
        assert_eq!(touched_areas(&changes), [group]);
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
