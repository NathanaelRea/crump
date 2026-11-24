//! The History mixer, in track, group, return and Master signal order.

use super::*;
use crate::model::{ChannelStrip, OutputTarget, Panner, Processor, Send, Song, TapPoint, Unit};
use fx_rack::{Knob, difference, line_text, outline, readout, same_chain, same_device, slot};

/// Mixer state common to an authored channel and the implicit Master.
#[derive(Clone, Copy)]
struct Channel<'a> {
    name: &'a str,
    strip: Option<&'a ChannelStrip>,
    level: f64,
    inserts: &'a [Processor],
}

impl<'a> Channel<'a> {
    fn strip(name: &'a str, strip: &'a ChannelStrip) -> Self {
        Self {
            name,
            strip: Some(strip),
            level: strip.level_db,
            inserts: &strip.inserts,
        }
    }

    fn master(song: &'a Song) -> Self {
        Self {
            name: "Master",
            strip: None,
            level: song.master.as_ref().map_or(0.0, |m| m.level_db),
            inserts: song.master.as_ref().map_or(&[], |m| &m.inserts),
        }
    }

    fn sends(self) -> &'a [Send] {
        self.strip.map_or(&[], |c| &c.sends)
    }
}

/// Draws the complete console and expands the racks changed on the touched channels.
pub(super) fn draw(ui: &mut Ui, view: &SoundView<'_>, areas: &[Area], changes: &[&ValueChange]) {
    let song = &view.shown.song;
    let mut channels =
        Vec::with_capacity(song.tracks.len() + song.groups.len() + song.returns.len() + 1);
    channels.extend(song.tracks.iter().enumerate().map(|(i, t)| {
        (
            Area::Track(t.id.clone()),
            Channel::strip(&t.name, &t.channel),
            theme::track_color(i),
        )
    }));
    channels.extend(song.groups.iter().map(|g| {
        (
            Area::Group(g.id.clone()),
            Channel::strip(&g.name, &g.channel),
            theme::ACCENT_CYAN,
        )
    }));
    channels.extend(song.returns.iter().map(|r| {
        (
            Area::Return(r.id.clone()),
            Channel::strip(&r.name, &r.channel),
            theme::ACCENT_VIOLET,
        )
    }));
    channels.push((Area::Master, Channel::master(song), theme::TEXT_SECONDARY));

    section(ui, Kind::Mix, "Console", |ui| {
        const FOCUS: f32 = 112.0;
        const SPACING: f32 = 3.0;
        let width = ui.available_width();
        let touched = channels
            .iter()
            .filter(|(a, _, _)| areas.contains(a))
            .count();
        let context =
            ((width - 1.0 - FOCUS * touched as f32 - SPACING * (channels.len() - 1) as f32)
                / (channels.len() - touched).max(1) as f32)
                .clamp(49.0, 78.0);
        let strip_width = |area| if areas.contains(area) { FOCUS } else { context };
        let first_touched = channels.iter().position(|(a, _, _)| areas.contains(a));
        let scroll_id = ui.id().with("history-console-focus");
        let mut scroll = egui::ScrollArea::horizontal()
            .id_salt("history-console")
            .auto_shrink([false, true]);
        // On a new selection, expose the touched strip without overriding manual scrolling.
        let focus = first_touched.map(|i| (channels[i].1.name.to_owned(), i));
        let focus_changed = ui.data_mut(|data| {
            let previous = data.get_temp::<Option<(String, usize)>>(scroll_id);
            if previous.as_ref() == Some(&focus) {
                false
            } else {
                data.insert_temp(scroll_id, focus);
                true
            }
        });
        if focus_changed {
            let offset = first_touched.map_or(0.0, |i| {
                let end = channels[..=i]
                    .iter()
                    .map(|(a, _, _)| strip_width(a))
                    .sum::<f32>()
                    + i as f32 * SPACING;
                (end - width).max(0.0)
            });
            scroll = scroll.horizontal_scroll_offset(offset);
        }
        scroll.show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = SPACING;
            ui.horizontal_top(|ui| {
                for (area, channel, accent) in &channels {
                    let previous = lookup(view.previous, area);
                    let other = lookup(view.other, area);
                    let touched = areas.contains(area);
                    let edited = touched && view.previous.is_some();
                    let differs = other
                        .as_ref()
                        .is_some_and(|c| !same_chain(channel.inserts, c.channel().inserts))
                        || (touched && view.previous.is_none() && view.other.is_some());
                    let role = match area {
                        Area::Track(_) => "TRACK",
                        Area::Group(_) => "GROUP",
                        Area::Return(_) => "RETURN",
                        _ => "MAIN",
                    };
                    strip(
                        ui,
                        Strip {
                            channel: *channel,
                            previous: previous.as_ref().map(NamedChannel::channel),
                            other: other.as_ref().map(NamedChannel::channel),
                            role,
                            compact_name: match area {
                                Area::Track(id) | Area::Group(id) | Area::Return(id) => id,
                                _ => "Master",
                            },
                            accent: *accent,
                            touched,
                            edited,
                            differs,
                            width: strip_width(area),
                        },
                        view,
                    );
                }
            });
        });

        for area in areas {
            let previous = lookup(view.previous, area);
            let other = lookup(view.other, area);
            let shown = channels
                .iter()
                .find(|(a, _, _)| a == area)
                .map(|(_, c, _)| *c);
            let inserts = shown.map_or(&[][..], |c| c.inserts);
            let before = view
                .previous
                .map(|_| previous.as_ref().map_or(&[][..], |c| c.channel().inserts));
            let comparison = view
                .other
                .map(|_| other.as_ref().map_or(&[][..], |c| c.channel().inserts));
            let in_area = || changes.iter().copied().filter(|c| c.area == *area);
            let fx = in_area()
                .filter(|c| control_path(c).first().is_some_and(|p| p == "inserts"))
                .collect::<Vec<_>>();
            let name = shown
                .map(|c| c.name)
                .or_else(|| {
                    previous
                        .as_ref()
                        .or(other.as_ref())
                        .map(|c| c.channel().name)
                })
                .unwrap_or("Channel");
            if !fx.is_empty() || before.is_some_and(|old| !same_chain(inserts, old)) {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("{name} · Inserts"))
                        .size(11.5)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                fx_rack::chain(ui, view, inserts, before, comparison, &fx);
            }
            let unpictured = in_area()
                .filter(|c| {
                    let path = control_path(c);
                    if shown.is_none() || path.is_empty() {
                        return true;
                    }
                    !matches!(
                        path[0].as_str(),
                        "name" | "inserts" | "output" | "sends" | "level_db" | "mute" | "panner"
                    )
                })
                .collect::<Vec<_>>();
            change_chips(ui, &unpictured);
        }
    });
}

/// Normalizes channel paths so Master and regular strips can share insert handling.
fn control_path(change: &ValueChange) -> &[String] {
    let path = &change.path;
    if path.first().is_some_and(|p| p == "channel") {
        &path[1..]
    } else {
        path
    }
}

/// Names from the shared History lookup are owned.
enum NamedChannel<'a> {
    Regular(String, &'a ChannelStrip),
    Master(Channel<'a>),
}

impl NamedChannel<'_> {
    fn channel(&self) -> Channel<'_> {
        match self {
            Self::Regular(name, strip) => Channel::strip(name, strip),
            Self::Master(master) => *master,
        }
    }
}

/// Finds a matching channel in a comparison version, including an implicit Master.
fn lookup<'a>(project: Option<&'a Project>, area: &Area) -> Option<NamedChannel<'a>> {
    let song = &project?.song;
    if *area == Area::Master {
        Some(NamedChannel::Master(Channel::master(song)))
    } else {
        channel_strip(song, area).map(|(name, strip)| NamedChannel::Regular(name, strip))
    }
}

/// Layout and comparison state for a single channel in the console.
struct Strip<'a> {
    channel: Channel<'a>,
    previous: Option<Channel<'a>>,
    other: Option<Channel<'a>>,
    role: &'static str,
    compact_name: &'a str,
    accent: Color32,
    touched: bool,
    edited: bool,
    differs: bool,
    width: f32,
}

/// A channel strip in mixer order, with a common pan and fader datum across the bank.
fn strip(ui: &mut Ui, desc: Strip<'_>, view: &SoundView<'_>) {
    let Strip {
        channel,
        previous,
        other,
        role,
        compact_name,
        accent,
        touched,
        edited,
        differs,
        width,
    } = desc;
    let width = width - 6.0;
    let word = view.other_word;
    let before = previous.and_then(|c| c.strip);
    let compare = other.and_then(|c| c.strip);
    egui::Frame::new()
        .inner_margin(egui::Margin::same(2))
        .fill(if touched {
            theme::BG_CARD
        } else {
            theme::BG_SURFACE
        })
        .stroke(outline(edited, differs))
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                if !touched {
                    ui.multiply_opacity(0.80);
                }
                let (header, response) =
                    ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::hover());
                ui.painter()
                    .rect_filled(header.with_max_y(header.top() + 2.0), 1.0, accent);
                line_text(
                    ui,
                    header.translate(Vec2::new(0.0, 4.0)),
                    role,
                    FontId::monospace(8.0),
                    accent,
                    Align::Center,
                );
                let name_change = difference(
                    channel.name,
                    previous.map(|c| c.name),
                    other.map(|c| c.name),
                    word,
                    str::to_owned,
                );
                let shortened = compact_name.replace(['-', '_'], " ");
                let caption = if touched { channel.name } else { &shortened };
                line_text(
                    ui,
                    header.translate(Vec2::new(0.0, 14.0)),
                    caption,
                    FontId::proportional(if touched { 11.0 } else { 10.0 }),
                    if name_change.as_ref().is_some_and(|(edited, _)| *edited) {
                        theme::SELECTION
                    } else {
                        theme::TEXT_PRIMARY
                    },
                    Align::Center,
                );
                if let Some((_, note)) = name_change {
                    readout(ui, width, &note, theme::TEXT_SECONDARY, None);
                    response.on_hover_text(format!("{}\n{note}", channel.name));
                } else {
                    response.on_hover_text(channel.name);
                }
                let pan_height = 51.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(width, pan_height),
                    Layout::top_down(Align::Center),
                    |ui| {
                        ui.set_min_height(pan_height);
                        if let Some(channel) = channel.strip {
                            pan(ui, channel, before, compare, width, accent, word);
                        } else {
                            ui.label(RichText::new("STEREO").size(9.0).color(theme::TEXT_MUTED));
                        }
                    },
                );
                let level = mark(
                    channel.level,
                    previous.map(|c| c.level),
                    other.map(|c| c.level),
                );
                let mute = channel.strip.map(|c| {
                    let changed = difference(
                        c.mute,
                        before.map(|c| c.mute),
                        compare.map(|c| c.mute),
                        word,
                        |muted| if muted { "muted" } else { "mute off" }.to_owned(),
                    );
                    (c.mute, changed)
                });
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if touched && matches!(level, Some(Mark::Edited(_))) {
                        fader(ui, channel.level, level, word);
                        if let Some((muted, old)) = &mute {
                            mute_readout(ui, width, *muted, old.clone());
                        }
                    } else {
                        compact_fader(
                            ui,
                            width,
                            channel.level,
                            level,
                            word,
                            touched,
                            mute.as_ref().map(|(muted, prior)| {
                                (*muted, prior.as_ref().map(|(edited, _)| *edited))
                            }),
                        );
                        if let Some((muted, Some(old))) = mute {
                            mute_readout(ui, width, muted, Some(old));
                        }
                    }
                });
                let (output, output_change) = if let Some(channel) = channel.strip {
                    let output = output_name(&view.shown.song, &channel.output);
                    let route_at = |strip: Option<&ChannelStrip>, project: Option<&Project>| {
                        strip
                            .zip(project)
                            .map(|(c, p)| output_name(&p.song, &c.output))
                    };
                    let prior = difference(
                        output.as_str(),
                        route_at(before, view.previous).as_deref(),
                        route_at(compare, view.other).as_deref(),
                        word,
                        str::to_owned,
                    );
                    (output, prior)
                } else {
                    (
                        if touched { "Stereo out" } else { "Stereo" }.to_owned(),
                        None,
                    )
                };
                let route = if touched {
                    format!("Out  {output}")
                } else {
                    output
                };
                readout(ui, width, &route, accent, output_change);
                send_knobs(ui, channel, previous, other, width, accent, word);
                insert_slots(ui, channel, previous, other, width, word);
            })
        });
}

/// Formats a main output as a named destination.
fn output_name(song: &Song, output: &OutputTarget) -> String {
    match output {
        OutputTarget::Master => "Master".to_owned(),
        OutputTarget::Group(id) => song
            .groups
            .iter()
            .find(|g| g.id == *id)
            .map_or(id, |g| &g.name)
            .clone(),
        OutputTarget::Return(id) => song
            .returns
            .iter()
            .find(|r| r.id == *id)
            .map_or(id, |r| &r.name)
            .clone(),
    }
}

/// A one- or two-dial panner with the mode visible, including stereo balance.
fn pan(
    ui: &mut Ui,
    channel: &ChannelStrip,
    previous: Option<&ChannelStrip>,
    other: Option<&ChannelStrip>,
    width: f32,
    accent: Color32,
    word: &str,
) {
    let mode = |panner: &Panner| match panner {
        Panner::MonoPan { .. } => "Pan",
        Panner::Balance { .. } => "Balance",
        Panner::StereoPan { .. } => "Stereo pan",
    };
    let current_mode = mode(&channel.panner);
    let mode_change = difference(
        current_mode,
        previous.map(|c| mode(&c.panner)),
        other.map(|c| mode(&c.panner)),
        word,
        str::to_owned,
    );
    if let Some((_, note)) = mode_change {
        let response = ui.label(RichText::new(note).size(8.0).color(theme::TEXT_SECONDARY));
        response.on_hover_text(current_mode);
    }
    let dials = pan_dials(&channel.panner);
    let prev_dials = previous.map(|c| pan_dials(&c.panner));
    let other_dials = other.map(|c| pan_dials(&c.panner));
    let dial_width = (width - 2.0 * (dials.len() - 1) as f32) / dials.len() as f32;
    ui.horizontal(|ui| {
        for (label, value) in dials {
            let value_of =
                |dials: &Vec<(&str, f64)>| dials.iter().find(|(l, _)| *l == label).map(|(_, v)| *v);
            let comparison = mark(
                value,
                prev_dials.as_ref().and_then(value_of),
                other_dials.as_ref().and_then(value_of),
            );
            let label = match label {
                "Pan left" => "L",
                "Pan right" => "R",
                _ => current_mode,
            };
            Knob {
                label,
                value,
                range: (Unit::Pan, -1.0, 1.0),
                comparison,
                presence: None,
                word,
                accent,
            }
            .draw(ui, dial_width, 7.0, |v| {
                theme::format_compact_value(v, Unit::Pan)
            });
        }
    });
}

/// Compact gain fader with a ghosted cap when the level differs.
fn compact_fader(
    ui: &mut Ui,
    width: f32,
    db: f64,
    level: Option<Mark>,
    word: &str,
    focused: bool,
    mute: Option<(bool, Option<bool>)>,
) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 48.0), Sense::hover());
    let painter = ui.painter();
    let rail = Rect::from_center_size(rect.center(), Vec2::new(4.0, 40.0));
    painter.rect_filled(rail, 2.0, theme::BG_WELL);
    let at = |db| rail.bottom() - widgets::fader_db_to_norm(db, -60.0, 12.0) * rail.height();
    let tick = |db, width, color| {
        painter.hline(
            (rail.center().x - width)..=(rail.center().x + width),
            at(db),
            Stroke::new(1.0_f32, color),
        );
    };
    for tick_db in [-48.0, -24.0, -12.0, 0.0, 6.0] {
        tick(
            tick_db,
            if tick_db == 0.0 { 8.0 } else { 5.0 },
            theme::BORDER_STRONG,
        );
    }
    let note = match level {
        Some(Mark::Edited(was)) => Some((
            was,
            format!("Was {}", theme::format_compact_value(was, Unit::Decibels)),
        )),
        Some(Mark::Differs(was)) => Some((
            was,
            format!(
                "{word} {}",
                theme::format_compact_value(was, Unit::Decibels)
            ),
        )),
        None => None,
    };
    if let Some((was, _)) = &note {
        tick(*was, 12.0, theme::TEXT_SECONDARY);
    }
    let cap = Rect::from_center_size(
        Pos2::new(rail.center().x, at(db)),
        Vec2::new(if focused { 26.0 } else { 17.0 }, 8.0),
    );
    painter.rect_filled(cap, 2.0, theme::TEXT_SECONDARY);
    if note.is_some() {
        let edited = matches!(level, Some(Mark::Edited(_)));
        painter.rect_stroke(
            cap.expand(2.0),
            3.0,
            outline(edited, true),
            StrokeKind::Outside,
        );
    }
    if let Some((muted, prior)) = mute {
        let button =
            Rect::from_min_size(rect.left_top() + Vec2::new(0.0, 1.0), Vec2::new(13.0, 13.0));
        painter.rect_filled(
            button,
            2.0,
            if muted {
                theme::MUTE_LIT
            } else {
                theme::BG_WELL
            },
        );
        if prior.is_some() {
            painter.rect_stroke(
                button,
                2.0,
                outline(prior == Some(true), true),
                StrokeKind::Inside,
            );
        }
        painter.text(
            button.center(),
            Align2::CENTER_CENTER,
            "M",
            FontId::monospace(8.5),
            if muted {
                theme::BG_WELL
            } else {
                theme::TEXT_SECONDARY
            },
        );
        ui.interact(button, ui.id().with("mute"), Sense::hover())
            .on_hover_text(if muted { "Muted" } else { "Mute off" });
    }
    let value = theme::format_compact_value(db, Unit::Decibels);
    let height = if focused && note.is_some() {
        23.0
    } else {
        12.0
    };
    let (value_rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    line_text(
        ui,
        value_rect,
        &value,
        FontId::monospace(if width < 48.0 { 8.5 } else { 10.0 }),
        theme::TEXT_PRIMARY,
        Align::Center,
    );
    if focused && let Some((_, note)) = &note {
        line_text(
            ui,
            value_rect.translate(Vec2::new(0.0, 11.0)),
            note,
            FontId::proportional(9.0),
            theme::TEXT_SECONDARY,
            Align::Center,
        );
    }
    response.on_hover_text(note.map_or(value.clone(), |(_, note)| format!("{value}\n{note}")));
}

/// Names a mute change below its switch so the previous state remains readable.
fn mute_readout(ui: &mut Ui, width: f32, muted: bool, prior: Option<(bool, String)>) {
    readout(
        ui,
        width,
        if muted { "Muted" } else { "Mute off" },
        if muted {
            theme::MUTE_LIT
        } else {
            theme::TEXT_SECONDARY
        },
        prior,
    );
}

/// A small send knob per parallel branch, with distinct routing and tap changes.
fn send_knobs(
    ui: &mut Ui,
    channel: Channel<'_>,
    previous: Option<Channel<'_>>,
    other: Option<Channel<'_>>,
    width: f32,
    accent: Color32,
    word: &str,
) {
    let shown = channel.sends();
    let before = previous.map_or(&[][..], Channel::sends);
    let comparison = other.map_or(&[][..], Channel::sends);
    ui.label(RichText::new("SENDS").size(8.5).color(theme::TEXT_MUTED));
    if shown.is_empty() {
        readout(ui, width, "—", theme::TEXT_MUTED, None);
        ui.add_space(20.0);
    }
    for pair in shown.chunks(2) {
        let mut routing = [None, None];
        ui.horizontal_top(|ui| {
            for (index, send) in pair.iter().enumerate() {
                let was = before.iter().find(|s| s.id == send.id);
                let compared = comparison.iter().find(|s| s.id == send.id);
                let tap = |tap| {
                    if tap == TapPoint::PreFader {
                        "Pre"
                    } else {
                        "Post"
                    }
                };
                let target = send.target.trim_start_matches("return/");
                let label = match send.tap {
                    TapPoint::PreFader => format!("{target} · Pre"),
                    TapPoint::PostFader => target.to_owned(),
                };
                let prior = difference(
                    (send.target.as_str(), send.tap),
                    was.map(|s| (s.target.as_str(), s.tap)),
                    compared.map(|s| (s.target.as_str(), s.tap)),
                    word,
                    |(target, at)| {
                        format!("{} · {}", target.trim_start_matches("return/"), tap(at))
                    },
                )
                .or_else(|| {
                    if previous.is_some() && was.is_none() {
                        Some((true, "Was absent".to_owned()))
                    } else if other.is_some() && compared.is_none() {
                        Some((false, format!("{word} absent")))
                    } else {
                        None
                    }
                });
                routing[index] =
                    prior.map(|prior| (format!("{target} · {}", tap(send.tap)), prior));
                Knob {
                    label: &label,
                    value: send.level_db,
                    range: (Unit::Decibels, -60.0, 6.0),
                    comparison: mark(
                        send.level_db,
                        was.map(|s| s.level_db),
                        compared.map(|s| s.level_db),
                    ),
                    presence: None,
                    word,
                    accent,
                }
                .draw(ui, (width - 2.0) * 0.5, 6.0, |v| {
                    if width < 90.0 {
                        format!("{v:.0}")
                    } else {
                        format!("{v:.1} dB")
                    }
                });
            }
        });
        for (label, prior) in routing.into_iter().flatten() {
            readout(ui, width, &label, accent, Some(prior));
        }
    }
    let removed = before.iter().map(|s| (s, true));
    let compared = comparison
        .iter()
        .filter(|s| !before.iter().any(|old| old.id == s.id))
        .map(|s| (s, false));
    for (send, edited) in removed
        .chain(compared)
        .filter(|(s, _)| !shown.iter().any(|cur| cur.id == s.id))
    {
        let who = if edited { "Was" } else { word };
        readout(
            ui,
            width,
            "Empty send",
            theme::TEXT_MUTED,
            Some((
                edited,
                format!(
                    "{who} {} · {:.1} dB",
                    send.target.trim_start_matches("return/"),
                    send.level_db
                ),
            )),
        );
    }
}

/// The insert slot list follows the chain order and lights the devices this edit moved.
fn insert_slots(
    ui: &mut Ui,
    channel: Channel<'_>,
    previous: Option<Channel<'_>>,
    other: Option<Channel<'_>>,
    width: f32,
    word: &str,
) {
    ui.label(RichText::new("INSERTS").size(8.5).color(theme::TEXT_MUTED));
    if channel.inserts.is_empty() {
        readout(ui, width, "—", theme::TEXT_MUTED, None);
    }
    for (i, proc) in channel.inserts.iter().enumerate() {
        let before = slot(previous.map(|c| c.inserts), &proc.id);
        let compare = slot(other.map(|c| c.inserts), &proc.id);
        let changed = |(pos, p)| pos != i || !same_device(proc, p);
        let edited = previous.is_some() && before.is_none_or(changed);
        let differs = other.is_some() && compare.is_none_or(changed);
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 15.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, theme::BG_WELL);
        painter.rect_stroke(rect, 3.0, outline(edited, differs), StrokeKind::Inside);
        let color = if proc.bypass {
            theme::TEXT_MUTED
        } else {
            theme::processor_color(proc.kind)
        };
        painter.circle_filled(Pos2::new(rect.left() + 5.0, rect.center().y), 2.1, color);
        let text = rect
            .with_min_x(rect.left() + 10.0)
            .translate(Vec2::new(0.0, 1.0));
        let title = theme::processor_title(proc.kind);
        line_text(
            ui,
            text,
            title,
            FontId::proportional(9.5),
            color,
            Align::Min,
        );
        let mut tip = format!(
            "{title} · {}\n{}",
            proc.id,
            if proc.bypass { "Bypassed" } else { "On" }
        );
        if edited || differs {
            let (version, old) = if edited {
                ("Was", before)
            } else {
                (word, compare)
            };
            if let Some((pos, p)) = old {
                tip.push_str(&format!(
                    "\n{version} {} · slot {}",
                    if p.bypass { "bypassed" } else { "on" },
                    pos + 1
                ));
            } else {
                tip.push_str(&format!("\n{version} absent"));
            }
        }
        response.on_hover_text(tip);
    }
}
