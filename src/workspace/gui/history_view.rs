//! Project History read as a musician's log: working sessions, each a list of the things changed,
//! with back-to-back edits to one thing folded into a run that expands. A side inspector explains
//! the selected change in words and offers Compare and Restore.

use std::sync::OnceLock;

use super::theme::{self, with_alpha};
use super::widgets::{self, led_button};
use super::*;
use crate::history::{HistoryEntry, OriginKind};

/// Silence that starts a new working session.
const SESSION_GAP_SECS: i64 = 30 * 60;
const INSPECTOR_WIDTH: f32 = 380.0;

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
    fn same_subject(&self, other: &Item<'_>) -> bool {
        self.kind == other.kind && self.subject == other.subject
    }
}

fn items(app: &Workspace) -> Vec<Item<'_>> {
    let offset = local_offset_secs();
    app.history
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let (kind, subject, action) = describe(app, &entry.summary);
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

/// A session split into runs of back-to-back edits to the same thing.
fn runs<'a, 'b>(session: &'b [Item<'a>]) -> Vec<&'b [Item<'a>]> {
    split_where(session, |newer, older| !newer.same_subject(older))
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

/// What a run did, oldest first: distinct actions with repeat counts.
fn run_actions(run: &[Item<'_>]) -> String {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for item in run.iter().rev() {
        match seen.iter_mut().find(|(action, _)| *action == item.action) {
            Some((_, count)) => *count += 1,
            None => seen.push((&item.action, 1)),
        }
    }
    seen.iter()
        .map(|(action, count)| {
            if *count > 1 {
                format!("{action} ×{count}")
            } else {
                (*action).to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
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

    if summary.starts_with("Restored") {
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

/// Everyone who made changes in a session, oldest first: "You, Claude (agent)".
fn authors(session: &[Item<'_>]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for item in session.iter().rev() {
        let name = who(item.entry);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
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

/// A small "Current" marker: the version you hear now.
fn current_marker(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(62.0, 18.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 9.0, theme::BG_ACTIVE);
    painter.circle_filled(
        Pos2::new(rect.left() + 9.0, rect.center().y),
        3.0,
        theme::TEXT_PRIMARY,
    );
    painter.text(
        Pos2::new(rect.left() + 16.0, rect.center().y),
        Align2::LEFT_CENTER,
        "Current",
        FontId::proportional(10.0),
        theme::TEXT_PRIMARY,
    );
}

/// Expand/collapse triangle.
fn disclosure(painter: &egui::Painter, center: Pos2, open: bool, color: Color32) {
    let points = if open {
        vec![
            Pos2::new(center.x - 4.0, center.y - 2.0),
            Pos2::new(center.x + 4.0, center.y - 2.0),
            Pos2::new(center.x, center.y + 3.0),
        ]
    } else {
        vec![
            Pos2::new(center.x - 2.0, center.y - 4.0),
            Pos2::new(center.x + 3.0, center.y),
            Pos2::new(center.x - 2.0, center.y + 4.0),
        ]
    };
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

/// Draws History once it has entries; the caller draws the empty state.
pub(super) fn history(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let items = items(app);
    let selected = clamp_index(app.selected_history, app.history.len());
    let mut pick: Option<usize> = None;
    let mut restore = false;
    let mut compare = false;

    egui::SidePanel::right("crump-history-inspector")
        .exact_width(INSPECTOR_WIDTH)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(theme::BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
                .inner_margin(egui::Margin::same(18)),
        )
        .show_inside(ui, |ui| {
            if let Some(item) = items.get(selected) {
                inspector(ui, app, &items, item, &mut pick, &mut restore, &mut compare);
            }
        });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(22, 14))
                .show(ui, |ui| {
                    for (number, session) in sessions(&items).into_iter().enumerate() {
                        session_block(ui, cx, &items, session, number, selected, &mut pick);
                    }
                });
        });

    if let Some(index) = pick {
        cx.select(selected, index);
    }
    if compare {
        cx.send(Action::OpenHistoryDiff);
    }
    if restore {
        cx.send(Action::OpenRestore);
    }
}

fn session_block(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    items: &[Item<'_>],
    session: &[Item<'_>],
    number: usize,
    selected: usize,
    pick: &mut Option<usize>,
) {
    let first = &session[session.len() - 1];
    let last = &session[0];
    // Sessions are keyed by their oldest entry, which never changes as the session grows.
    let key = first.entry.id.clone();
    let holds_selection = session.iter().any(|item| item.index == selected);
    // The latest session and the one holding the selection start open; the rest are folded.
    let open = match cx.state.history_sessions.get(&key) {
        Some(open) => *open,
        None => number == 0 || holds_selection,
    };

    let header = ui.horizontal(|ui| {
        let (tri, _) = ui.allocate_exact_size(Vec2::new(14.0, 18.0), Sense::hover());
        disclosure(ui.painter(), tri.center(), open, theme::TEXT_SECONDARY);
        ui.label(
            RichText::new(format!(
                "{} · {}–{}",
                day_label(first.at),
                clock(first.at),
                clock(last.at)
            ))
            .strong()
            .size(12.0)
            .color(theme::TEXT_PRIMARY),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!(
                    "{} · {} min · {}",
                    plural(session.len(), "change"),
                    ((last.at - first.at) / 60).max(1),
                    authors(session).join(", ")
                ))
                .size(10.5)
                .color(theme::TEXT_MUTED),
            );
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.add(
            egui::Label::new(
                RichText::new(session_subjects(session))
                    .size(11.0)
                    .color(theme::TEXT_SECONDARY),
            )
            .truncate(),
        );
    });
    if header
        .response
        .interact(Sense::click())
        .on_hover_text(if open {
            "Fold this session"
        } else {
            "Show this session"
        })
        .clicked()
    {
        cx.state.history_sessions.insert(key, !open);
    }
    let (rule, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rule, 0.0, theme::SEAM_DARK);
    ui.add_space(6.0);
    if open {
        // Name who made a run only when the session had more than one author.
        let mixed = authors(session).len() > 1;
        for run in runs(session) {
            run_rows(ui, cx, items, run, selected, mixed, pick);
        }
    }
    ui.add_space(18.0);
}

fn run_rows(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    items: &[Item<'_>],
    run: &[Item<'_>],
    selected: usize,
    mixed_authors: bool,
    pick: &mut Option<usize>,
) {
    let app = cx.app;
    let head = &run[0];
    let oldest = &run[run.len() - 1];
    let folds = run.len() > 1;
    let expanded = folds && cx.state.history_runs.contains(&oldest.entry.id);
    let holds_selection = run.iter().any(|item| item.index == selected);
    let highlight = holds_selection && !expanded;
    let current = run.iter().any(|item| is_current(app, item.entry));
    // A restore reads as where it went back to.
    let target = (head.kind == Kind::Restore)
        .then(|| restore_target(items, head))
        .flatten();
    let subject = target.map_or_else(
        || head.subject.clone(),
        |target| format!("Back to {} — {}", target.subject, target.action),
    );
    let detail = if let Some(target) = target {
        format!("version from {}", clock(target.at))
    } else if folds {
        format!("{} · {}", plural(run.len(), "change"), run_actions(run))
    } else {
        head.action.clone()
    };
    let mut triangle = Rect::NOTHING;
    let row = egui::Frame::new()
        .fill(if highlight {
            theme::BG_CARD
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::new(
            1.0_f32,
            if highlight {
                theme::SELECTION
            } else {
                Color32::TRANSPARENT
            },
        ))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (tri, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                triangle = tri.expand(6.0);
                if folds {
                    disclosure(ui.painter(), tri.center(), expanded, theme::TEXT_SECONDARY);
                }
                kind_chip(ui, head.kind);
                ui.label(
                    RichText::new(subject)
                        .strong()
                        .size(12.5)
                        .color(theme::TEXT_PRIMARY),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(if folds {
                            format!("{}–{}", clock(oldest.at), clock(head.at))
                        } else {
                            clock(head.at)
                        })
                        .monospace()
                        .size(10.5)
                        .color(theme::TEXT_MUTED),
                    );
                    if current {
                        current_marker(ui);
                    }
                    if mixed_authors {
                        ui.label(
                            RichText::new(who(head.entry))
                                .size(10.5)
                                .color(theme::TEXT_MUTED),
                        );
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(detail)
                                    .size(11.5)
                                    .color(theme::TEXT_SECONDARY),
                            )
                            .truncate(),
                        );
                    });
                });
            });
        });
    // The row selects the run's latest change and opens a run; only the triangle folds it.
    let response = row.response.interact(Sense::click());
    let response = if folds {
        response.on_hover_text("Click to open · the triangle folds it again")
    } else {
        response
    };
    if response.clicked() {
        let on_triangle = response
            .interact_pointer_pos()
            .is_some_and(|pos| triangle.contains(pos));
        if folds && on_triangle {
            if expanded {
                cx.state.history_runs.remove(&oldest.entry.id);
            } else {
                cx.state.history_runs.insert(oldest.entry.id.clone());
            }
        } else {
            *pick = Some(head.index);
            if folds {
                cx.state.history_runs.insert(oldest.entry.id.clone());
            }
        }
    }
    if expanded {
        for item in run {
            child_row(ui, app, item, selected, pick);
        }
    }
    ui.add_space(2.0);
}

fn child_row(
    ui: &mut Ui,
    app: &Workspace,
    item: &Item<'_>,
    selected: usize,
    pick: &mut Option<usize>,
) {
    let is_selected = item.index == selected;
    let row = egui::Frame::new()
        .fill(if is_selected {
            theme::BG_CARD
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::new(
            1.0_f32,
            if is_selected {
                theme::SELECTION
            } else {
                Color32::TRANSPARENT
            },
        ))
        .corner_radius(4.0)
        .inner_margin(egui::Margin {
            left: 100,
            right: 8,
            top: 3,
            bottom: 3,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), 3.0, theme::BORDER_STRONG);
                ui.label(
                    RichText::new(&item.action)
                        .size(11.5)
                        .color(theme::TEXT_PRIMARY),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(clock(item.at))
                            .monospace()
                            .size(10.0)
                            .color(theme::TEXT_MUTED),
                    );
                    if is_current(app, item.entry) {
                        current_marker(ui);
                    }
                });
            });
        });
    if row.response.interact(Sense::click()).clicked() {
        *pick = Some(item.index);
    }
}

fn inspector(
    ui: &mut Ui,
    app: &Workspace,
    items: &[Item<'_>],
    item: &Item<'_>,
    pick: &mut Option<usize>,
    restore: &mut bool,
    compare: &mut bool,
) {
    ui.horizontal(|ui| {
        kind_chip(ui, item.kind);
        ui.label(
            RichText::new(&item.subject)
                .strong()
                .size(13.5)
                .color(theme::TEXT_PRIMARY),
        );
    });
    ui.add_space(8.0);
    let headline = match restore_target(items, item) {
        Some(target) => format!(
            "Went back to {} — {} ({})",
            target.subject,
            target.action,
            clock(target.at)
        ),
        None => item.action.clone(),
    };
    ui.add(
        egui::Label::new(
            RichText::new(headline)
                .size(18.0)
                .color(theme::TEXT_PRIMARY),
        )
        .wrap(),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new(format!(
            "{} · {} at {} · {}",
            who(item.entry),
            day_label(item.at),
            clock(item.at),
            ago(item.at)
        ))
        .size(11.0)
        .color(theme::TEXT_SECONDARY),
    );
    if is_current(app, item.entry) {
        ui.add_space(8.0);
        current_marker(ui);
    }
    if !item.entry.explanation.is_empty() {
        ui.add_space(10.0);
        ui.add(
            egui::Label::new(
                RichText::new(format!("“{}”", item.entry.explanation))
                    .italics()
                    .size(11.5)
                    .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
    }
    if let Some(target) = restore_target(items, item) {
        ui.add_space(8.0);
        if ui
            .add(egui::Button::new("Show that version").fill(theme::BG_CARD))
            .clicked()
        {
            *pick = Some(target.index);
        }
    }
    // The other direction: this version was brought back later.
    for later in items
        .iter()
        .filter(|other| other.entry.restored_from.as_ref() == Some(&item.entry.id))
    {
        ui.add_space(6.0);
        if ui
            .add(egui::Button::new(format!("Restored at {}", clock(later.at))).fill(theme::BG_CARD))
            .on_hover_text("Show the restore")
            .clicked()
        {
            *pick = Some(later.index);
        }
    }

    // What a restore would undo: every later change (History is newest first).
    ui.add_space(16.0);
    let later = &items[..item.index];
    let impact = if later.is_empty() {
        "This is the latest change.".to_owned()
    } else {
        format!(
            "Restoring undoes the {} made after it ({}). They stay in History, and the restore \
             is added as a new change.",
            plural(later.len(), "change"),
            session_subjects(later)
        )
    };
    ui.add(
        egui::Label::new(
            RichText::new(impact)
                .size(10.5)
                .color(theme::TEXT_SECONDARY),
        )
        .wrap(),
    );
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new("Compare with now").fill(theme::BG_CARD))
            .on_hover_text("Show what differs between this version and the current one · d")
            .clicked()
        {
            *compare = true;
        }
        let can_restore = !is_current(app, item.entry);
        if ui
            .add_enabled(
                can_restore,
                egui::Button::new("Restore this version").fill(theme::BG_CARD),
            )
            .on_hover_text("Bring back the whole project as it was after this change · R")
            .clicked()
        {
            *restore = true;
        }
    });

    // Every change to the same thing, newest first, to jump between versions of it.
    let others = items
        .iter()
        .filter(|other| other.same_subject(item) && other.kind != Kind::Restore)
        .collect::<Vec<_>>();
    if others.len() > 1 {
        ui.add_space(18.0);
        ui.label(
            RichText::new(format!("ALL CHANGES TO {}", item.subject.to_uppercase()))
                .strong()
                .size(9.5)
                .color(theme::TEXT_MUTED),
        );
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            for other in others {
                let text = format!("{} {}", clock(other.at), other.action);
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
                if led_button(
                    ui,
                    &text,
                    other.index == item.index,
                    theme::SELECTION,
                    Vec2::new(width, 22.0),
                    &format!("{} · {}", day_label(other.at), ago(other.at)),
                )
                .clicked()
                {
                    *pick = Some(other.index);
                }
            }
        });
    }

    // File-level detail stays available, folded away.
    ui.add_space(18.0);
    egui::CollapsingHeader::new(
        RichText::new("Technical details")
            .size(10.5)
            .color(theme::TEXT_MUTED),
    )
    .id_salt("history-technical")
    .show(ui, |ui| {
        ui.label(
            RichText::new(format!(
                "Change {} · {}",
                item.entry.id,
                plural(item.entry.changes.len(), "file")
            ))
            .monospace()
            .size(10.0)
            .color(theme::TEXT_MUTED),
        );
        for change in &item.entry.changes {
            ui.label(
                RichText::new(widgets::legible(&change.path.display().to_string()))
                    .monospace()
                    .size(10.5)
                    .color(theme::TEXT_SECONDARY),
            );
        }
    });
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Sessions and the runs inside the newest one, as (subject, edits).
    pub(in crate::workspace::gui) fn shape(app: &Workspace) -> (usize, Vec<(String, usize)>) {
        let items = items(app);
        let sessions = sessions(&items);
        let runs = runs(sessions[0])
            .iter()
            .map(|run| (run[0].subject.clone(), run.len()))
            .collect();
        (sessions.len(), runs)
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
