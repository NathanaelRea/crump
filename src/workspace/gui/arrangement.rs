//! Pro Timeline & Arrangement view: color-coded tracks with inline M/S/A + fader + pan + meter,
//! translucent colored clips with real MIDI note/drum previews, loop seams & fades, two-band
//! locator/ruler header, clamped loop brace, and direct clip manipulation with live drag ghosts.

use super::theme::{self, with_alpha};
use super::widgets::preview_key;
use super::widgets::{
    dbfs_meter_horizontal, horizontal_mini_slider, led_button, paint_dbfs_meter_vertical,
    pill_toggle, unavailable,
};
use super::*;
use crate::model::{Clip, DrumVoice, Pattern, PatternEvent, Track, Unit};
use crate::time::MusicalTime;

const HEADER_WIDTH: f32 = 236.0;
/// Two-band ruler: top 18px Locator flags + Loop Brace, bottom 24px Bar numbers + beat ticks.
pub(super) const RULER_HEIGHT: f32 = 42.0;
const LOCATOR_BAND_H: f32 = 18.0;
const BASE_LANE_HEIGHT: f32 = 64.0;
/// Smallest lane height the arrangement shrinks to so all tracks fit the view.
const MIN_FIT_LANE_HEIGHT: f32 = 52.0;
/// Width of one projected activity column.
const COLUMN_WIDTH: f32 = 2.0;
const MAX_COLUMNS: usize = 4096;
const PAN_SCROLL_STEP: f32 = 48.0;
const ZOOM_SCROLL_STEP: f32 = 0.25;

pub(super) fn timeline(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(
            ui,
            "Timeline unavailable while Project source has syntax errors",
        );
        return;
    };

    compact_toolbar(ui, cx, project);
    // Material that could not be read (an unreadable sample) is the one thing worth saying.
    if let Some(warning) = app.timeline_cache.warning() {
        ui.label(RichText::new(warning).size(11.0).color(theme::SIG_CLIP));
    }

    // Compact 1-line contextual selection bar at bottom
    egui::TopBottomPanel::bottom("crump-timeline-inspector")
        .frame(
            egui::Frame::new()
                .fill(theme::BG_SURFACE)
                .inner_margin(egui::Margin::symmetric(10, 4))
                .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK)),
        )
        .show_inside(ui, |ui| {
            contextual_bar(ui, cx, project);
        });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| lanes(ui, cx, project));
}

/// Slim 30px single-row arrangement toolbar.
fn compact_toolbar(ui: &mut Ui, cx: &mut Cx<'_>, project: &Project) {
    let app = cx.app;
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                // Grid stepper pill
                ui.label(
                    RichText::new("GRID")
                        .size(9.5)
                        .strong()
                        .color(theme::TEXT_MUTED),
                );
                if led_button(
                    ui,
                    "−",
                    false,
                    theme::ACCENT_CYAN,
                    Vec2::new(20.0, 20.0),
                    "Finer grid · [",
                )
                .clicked()
                {
                    cx.send(TimelineAction::ChangeGrid(-1));
                }
                // Grid and zoom readouts sit in fixed slots, so changing either never moves
                // the tools to their right.
                let (grid_rect, _) = ui.allocate_exact_size(Vec2::new(52.0, 14.0), Sense::hover());
                ui.painter().text(
                    grid_rect.center(),
                    Align2::CENTER_CENTER,
                    app.timeline_state.grid_label(),
                    FontId::monospace(11.5),
                    theme::TEXT_PRIMARY,
                );
                if led_button(
                    ui,
                    "+",
                    false,
                    theme::ACCENT_CYAN,
                    Vec2::new(20.0, 20.0),
                    "Coarser grid · ]",
                )
                .clicked()
                {
                    cx.send(TimelineAction::ChangeGrid(1));
                }

                ui.separator();

                // Zoom cluster
                ui.label(
                    RichText::new("ZOOM")
                        .size(9.5)
                        .strong()
                        .color(theme::TEXT_MUTED),
                );
                if led_button(
                    ui,
                    "−",
                    false,
                    theme::ACCENT_CYAN,
                    Vec2::new(20.0, 20.0),
                    "Zoom out · -",
                )
                .clicked()
                {
                    cx.send(TimelineAction::Zoom(-1));
                }
                let (zoom_rect, _) = ui.allocate_exact_size(Vec2::new(44.0, 14.0), Sense::hover());
                ui.painter().text(
                    zoom_rect.center(),
                    Align2::CENTER_CENTER,
                    if app.timeline_viewport.is_fit() {
                        "FIT".to_owned()
                    } else {
                        format!("{}×", app.timeline_viewport.zoom_factor())
                    },
                    FontId::monospace(11.0),
                    theme::TEXT_SECONDARY,
                );
                if led_button(
                    ui,
                    "+",
                    false,
                    theme::ACCENT_CYAN,
                    Vec2::new(20.0, 20.0),
                    "Zoom in · +",
                )
                .clicked()
                {
                    cx.send(TimelineAction::Zoom(1));
                }
                if pill_toggle(
                    ui,
                    "Fit",
                    app.timeline_viewport.is_fit(),
                    "Fit whole Song · Home",
                )
                .clicked()
                {
                    cx.send(TimelineAction::Fit);
                }

                if pill_toggle(
                    ui,
                    "Follow",
                    app.timeline_state.follow_playhead(),
                    "Follow playhead · f",
                )
                .clicked()
                {
                    cx.send(TimelineAction::ToggleFollow);
                }

                ui.separator();

                // Range / Loop tools (contextual when RANGE mode active). Both sets share one
                // slot sized for the wider Range-mode set, so entering Range mode never moves the
                // locator pills or drops some of them.
                let pill_w = |ui: &Ui, label: &str| {
                    (ui.painter()
                        .layout_no_wrap(
                            label.to_owned(),
                            FontId::proportional(12.0),
                            theme::TEXT_PRIMARY,
                        )
                        .size()
                        .x
                        + 18.0)
                        .max(46.0)
                };
                let slot_w = ["Loop Range", "Play Range", "Fit Range", "Cancel"]
                    .iter()
                    .map(|label| pill_w(ui, label) + ui.spacing().item_spacing.x)
                    .sum::<f32>();
                ui.allocate_ui_with_layout(
                    Vec2::new(slot_w, 24.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_width(slot_w);
                        if app.timeline_state.visual() {
                            if pill_toggle(ui, "Loop Range", true, "Loop selected range · L")
                                .clicked()
                            {
                                cx.send(TimelineAction::ApplyRange { looping: true });
                            }
                            if pill_toggle(
                                ui,
                                "Play Range",
                                false,
                                "Play selected range once · Enter",
                            )
                            .clicked()
                            {
                                cx.send(TimelineAction::ApplyRange { looping: false });
                            }
                            if pill_toggle(ui, "Fit Range", false, "Zoom to range · zf").clicked()
                            {
                                cx.send(TimelineAction::FitRange);
                            }
                            if pill_toggle(ui, "Cancel", false, "Exit RANGE mode · Esc").clicked()
                            {
                                cx.send(Action::CancelMode);
                            }
                        } else {
                            if pill_toggle(ui, "Range", false, "Select a range on the Timeline · V")
                                .clicked()
                            {
                                cx.send(TimelineAction::BeginRange);
                            }
                            if pill_toggle(ui, "+ Locator", false, "Create a Locator at cursor · M")
                                .clicked()
                            {
                                cx.send(TimelineAction::OpenLocator);
                            }
                            if pill_toggle(
                                ui,
                                "Search",
                                false,
                                "Search Tracks, Clips, Patterns, Locators · /",
                            )
                            .clicked()
                            {
                                cx.send(TimelineAction::OpenSearch);
                            }
                        }
                    },
                );

                // Right-aligned cursor readout & working locator jump pills
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let cursor_str = theme::format_bar_beat(
                        app.timeline_position().at.as_f64(),
                        project.song.meter,
                    );
                    // Fixed slot, so the locator pills hold still as the cursor crosses bar 10.
                    let (cur_rect, cur_resp) =
                        ui.allocate_exact_size(Vec2::new(92.0, 14.0), Sense::click());
                    ui.painter().text(
                        cur_rect.right_center(),
                        Align2::RIGHT_CENTER,
                        format!("CUR {cursor_str}"),
                        FontId::monospace(11.0),
                        if cur_resp.hovered() {
                            theme::TEXT_PRIMARY
                        } else {
                            theme::TEXT_SECONDARY
                        },
                    );
                    cur_resp
                        .on_hover_text("Cursor position · click to go to exact bar (:)")
                        .clicked()
                        .then(|| cx.send(TimelineAction::OpenGoto));

                    if !project.song.locators.is_empty() {
                        ui.separator();
                        let cur_at = app.timeline_position().at;
                        // As many locators as the toolbar has room for, rather than a fixed five.
                        let pill_w =
                            |name: &str| (name.len() as f32 * 6.5 + 14.0).clamp(42.0, 92.0);
                        let room = ui.available_width() - 8.0;
                        let fit = project
                            .song
                            .locators
                            .iter()
                            .scan(0.0, |used, locator| {
                                *used += pill_w(&locator.name) + ui.spacing().item_spacing.x;
                                Some(*used)
                            })
                            .take_while(|used| *used <= room)
                            .count();
                        for (idx, locator) in
                            project.song.locators.iter().enumerate().take(fit).rev()
                        {
                            let at_here = locator.at == cur_at;
                            if led_button(
                                ui,
                                &locator.name,
                                at_here,
                                theme::TEXT_SECONDARY,
                                Vec2::new(pill_w(&locator.name), 20.0),
                                &format!("Jump to locator {} ({})", locator.name, locator.at),
                            )
                            .clicked()
                            {
                                cx.send(TimelineAction::JumpToLocator(idx));
                            }
                        }
                    }
                });
            });
        });
}

/// Where the lanes sit on screen and how Musical Time maps onto them.
struct Geometry {
    lanes: Rect,
    full_canvas: Rect,
    lane_height: f32,
    columns: usize,
    start: f64,
    span: f64,
}

impl Geometry {
    fn x_of(&self, quarters: f64) -> f32 {
        self.lanes.left() + ((quarters - self.start) / self.span) as f32 * self.lanes.width()
    }

    fn quarters_of(&self, x: f32) -> f64 {
        let frac = f64::from(((x - self.lanes.left()) / self.lanes.width()).clamp(0.0, 1.0));
        self.start + frac * self.span
    }

    fn visible(&self, quarters: f64) -> bool {
        quarters >= self.start && quarters <= self.start + self.span
    }

    fn lane_top(&self, track: usize) -> f32 {
        self.lanes.top() + self.lane_height * track as f32
    }

    fn column_at(&self, x: f32) -> usize {
        let fraction = ((x - self.lanes.left()) / self.lanes.width()).clamp(0.0, 1.0);
        ((fraction * self.columns as f32) as usize).min(self.columns - 1)
    }

    fn track_at(&self, y: f32, tracks: usize) -> Option<usize> {
        if y < self.lanes.top() || tracks == 0 {
            return None;
        }
        let idx = ((y - self.lanes.top()) / self.lane_height) as usize;
        (idx < tracks).then_some(idx)
    }
}

fn lanes(ui: &mut Ui, cx: &mut Cx<'_>, project: &Project) {
    let app = cx.app;
    let tracks = &project.song.tracks;
    let avail_h = ui.available_height().max(320.0);
    // Lanes share the view: small arrangements get generous lanes, and larger ones shrink (to a
    // readable minimum) so every track is visible on load before scrolling is needed.
    let fit = (avail_h - RULER_HEIGHT - 8.0) / tracks.len().max(1) as f32;
    let lane_height = if fit >= MIN_FIT_LANE_HEIGHT {
        fit.min(92.0)
    } else {
        BASE_LANE_HEIGHT
    };
    let tracks_h = lane_height * tracks.len().max(1) as f32;
    let total_h = (RULER_HEIGHT + tracks_h).max(avail_h);
    let size = Vec2::new(ui.available_width().max(HEADER_WIDTH + 120.0), total_h);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());

    let lanes = Rect::from_min_max(
        Pos2::new(rect.left() + HEADER_WIDTH, rect.top() + RULER_HEIGHT),
        Pos2::new(rect.right(), rect.top() + RULER_HEIGHT + tracks_h),
    );
    let full_canvas = Rect::from_min_max(
        Pos2::new(rect.left() + HEADER_WIDTH, rect.top() + RULER_HEIGHT),
        Pos2::new(rect.right(), rect.bottom()),
    );
    let columns = ((lanes.width() / COLUMN_WIDTH) as usize).clamp(1, MAX_COLUMNS);
    let projection = app.timeline_cache.project(
        project,
        app.timeline_viewport.window(project.song.end),
        columns,
    );
    let start = projection.window.start.as_f64();
    let geometry = Geometry {
        lanes,
        full_canvas,
        lane_height,
        columns,
        start,
        span: (projection.window.end.as_f64() - start).max(f64::EPSILON),
    };
    let painter = ui.painter_at(rect);
    let timed = painter.with_clip_rect(Rect::from_min_max(
        Pos2::new(lanes.left(), rect.top()),
        rect.max,
    ));

    // Top-left corner header cell above track headers
    let corner = Rect::from_min_max(rect.left_top(), Pos2::new(lanes.left(), lanes.top()));
    painter.rect_filled(corner, 0.0, theme::BG_SURFACE);
    painter.hline(
        corner.x_range(),
        corner.bottom(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );
    painter.vline(
        corner.right(),
        rect.top()..=rect.bottom(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );
    painter.text(
        Pos2::new(corner.left() + 10.0, corner.top() + 11.0),
        Align2::LEFT_CENTER,
        format!("ARRANGEMENT · {} TRACKS", tracks.len()),
        FontId::proportional(9.5),
        theme::TEXT_MUTED,
    );
    painter.text(
        Pos2::new(corner.left() + 10.0, corner.bottom() - 10.0),
        Align2::LEFT_CENTER,
        format!(
            "{:.0} BPM · {} · END {}",
            project.song.tempo_bpm,
            project.song.meter,
            theme::format_bar_beat(project.song.end.as_f64(), project.song.meter,)
        ),
        FontId::monospace(10.0),
        theme::TEXT_SECONDARY,
    );

    // Render active track lanes + headers
    for (index, track) in tracks.iter().enumerate() {
        let top = geometry.lane_top(index);
        let lane = Rect::from_min_max(
            Pos2::new(lanes.left(), top),
            Pos2::new(lanes.right(), top + lane_height),
        );
        let lane_bg = if index == app.selected_track {
            Color32::from_rgb(26, 32, 42)
        } else if index % 2 == 0 {
            theme::BG_CANVAS
        } else {
            theme::BG_LANE_ALT
        };
        timed.rect_filled(lane, 0.0, lane_bg);
        timed.hline(
            lane.x_range(),
            lane.bottom(),
            Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        );

        let header = Rect::from_min_max(
            Pos2::new(rect.left(), top),
            Pos2::new(lanes.left() - 1.0, top + lane_height),
        );
        track_header(ui, cx, &painter, header, index, track);
    }

    // Continue faint studio arrangement grid rails below the last track so there's never an empty void
    if lanes.bottom() < rect.bottom() {
        let empty_hdr = Rect::from_min_max(
            Pos2::new(rect.left(), lanes.bottom()),
            Pos2::new(lanes.left() - 1.0, rect.bottom()),
        );
        painter.rect_filled(empty_hdr, 0.0, theme::BG_WELL);
        let mut y = lanes.bottom();
        let mut row_i = tracks.len();
        while y < rect.bottom() {
            let next_y = (y + lane_height).min(rect.bottom());
            let bg = if row_i.is_multiple_of(2) {
                with_alpha(theme::BG_CANVAS, 0.85)
            } else {
                with_alpha(theme::BG_LANE_ALT, 0.7)
            };
            timed.rect_filled(
                Rect::from_min_max(Pos2::new(lanes.left(), y), Pos2::new(lanes.right(), next_y)),
                0.0,
                bg,
            );
            timed.hline(
                lanes.x_range(),
                next_y,
                Stroke::new(1.0_f32, with_alpha(theme::BORDER_SUBTLE, 0.45)),
            );
            y = next_y;
            row_i += 1;
        }
        // Subtle studio hint in the empty lower header bay, only when there is room for it
        if empty_hdr.height() >= 28.0 {
            painter.text(
                Pos2::new(
                    empty_hdr.center().x,
                    (empty_hdr.top() + 26.0).min(empty_hdr.center().y),
                ),
                Align2::CENTER_CENTER,
                "Double-click lane to add Clip",
                FontId::proportional(10.5),
                with_alpha(theme::TEXT_MUTED, 0.65),
            );
        }
    }

    grid(&timed, &geometry, rect, app, project);
    for (index, track) in tracks.iter().enumerate() {
        let color = theme::track_color(index);
        let drew_midi = clips(&timed, &geometry, index, track, project, color);
        if !drew_midi
            && let Some(lane) = projection
                .lanes
                .iter()
                .find(|lane| lane.track_id == track.id)
        {
            activity(&timed, &geometry, index, &lane.cells, color);
        }
    }
    paint_clip_drag_ghost(&timed, &geometry, cx.state, project);
    markers(&timed, &geometry, rect, app, project);
    pointer(ui, cx, &geometry, rect, project);
}

/// Rich 236px studio track header with color stripe, M/S/A LEDs, inline level & pan sliders, and live meter.
fn track_header(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    painter: &egui::Painter,
    header: Rect,
    index: usize,
    track: &Track,
) {
    let app = cx.app;
    let selected = app.selected_track;
    let is_selected = index == selected;
    let color = theme::track_color(index);
    let identity = format!("track/{}", track.id);
    let mute_key = preview_key::channel_mute(&identity);
    let muted = cx.state.flag(&mute_key, track.channel.mute);
    let is_solo = app.audition_plan.soloed_channels.contains(&identity);
    let is_armed = matches!(
        &app.audition_plan.target,
        AuditionTarget::Track { track: t } if *t == track.id
    );

    let meter_info = app
        .mix_channel_indexes
        .get(&identity)
        .copied()
        .filter(|&i| i < app.telemetry.engine.channel_count)
        .map(|i| app.telemetry.engine.channel_activity[i]);

    let p = painter.with_clip_rect(header);
    let bg = if is_selected {
        Color32::from_rgb(36, 43, 56)
    } else {
        theme::BG_SURFACE
    };
    p.rect_filled(header, 0.0, bg);
    if is_selected {
        p.rect_filled(header, 0.0, with_alpha(color, 0.08));
    }
    // 4px left track color stripe
    p.rect_filled(
        Rect::from_min_max(
            header.left_top(),
            Pos2::new(header.left() + 4.0, header.bottom()),
        ),
        0.0,
        color,
    );
    // Bottom hairline
    p.hline(
        header.x_range(),
        header.bottom(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );

    // Live 9px vertical dBFS meter on the far right edge of the header
    let meter_rect = Rect::from_min_max(
        Pos2::new(header.right() - 11.0, header.top() + 5.0),
        Pos2::new(header.right() - 2.0, header.bottom() - 5.0),
    );
    let (peak, avg) = meter_info.map_or((0.0, 0.0), |m| (m.current_peak, m.average));
    paint_dbfs_meter_vertical(&p, meter_rect, peak, avg);

    let inner_rect = Rect::from_min_max(
        Pos2::new(header.left() + 9.0, header.top() + 5.0),
        Pos2::new(header.right() - 15.0, header.bottom() - 4.0),
    );

    // Background click selects track
    let bg_resp = ui.interact(
        header,
        ui.id().with(("track-header-bg", index)),
        Sense::click(),
    );
    if bg_resp.clicked() {
        cx.select(selected, index);
    }
    if bg_resp.double_clicked() {
        cx.select(selected, index);
        cx.send(TimelineAction::GoInstrument);
    }

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner_rect)
            .layout(Layout::top_down(Align::LEFT)),
    );
    child.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);

    // Row 1: Track index + Name + M / S / A micro-LEDs
    child.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        let name_w = (inner_rect.width() - 68.0).max(60.0);
        ui.allocate_ui_with_layout(
            Vec2::new(name_w, 18.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                // Full width whatever the name, so M / S / A line up in columns down the tracks.
                ui.set_min_width(name_w);
                ui.label(
                    RichText::new(format!("{:02}", index + 1))
                        .monospace()
                        .size(9.5)
                        .color(theme::TEXT_MUTED),
                );
                ui.add(
                    egui::Label::new(RichText::new(&track.name).strong().size(12.5).color(
                        if muted {
                            theme::TEXT_MUTED
                        } else {
                            theme::TEXT_PRIMARY
                        },
                    ))
                    .truncate(),
                );
            },
        );

        if led_button(
            ui,
            "M",
            muted,
            theme::MUTE_LIT,
            Vec2::new(19.0, 17.0),
            "Mute Track (authored)",
        )
        .clicked()
        {
            cx.state.hold_flag(&mute_key, !muted);
            cx.select(selected, index);
            cx.send(Action::ToggleChannelMute {
                identity: identity.clone(),
            });
        }
        if led_button(
            ui,
            "S",
            is_solo,
            theme::SIG_HOT,
            Vec2::new(19.0, 17.0),
            "Solo Track (transient)",
        )
        .clicked()
        {
            cx.select(selected, index);
            cx.send(Action::ToggleChannelSolo {
                identity: identity.clone(),
            });
        }
        if led_button(
            ui,
            "F",
            track.frozen,
            theme::SIG_NOMINAL,
            Vec2::new(19.0, 17.0),
            "Track Freeze (authored)",
        )
        .clicked()
        {
            cx.send(Action::ToggleTrackFreeze {
                track: track.id.clone(),
            });
        }
        if led_button(
            ui,
            "A",
            is_armed,
            theme::SIG_NOMINAL,
            Vec2::new(19.0, 17.0),
            "Arm Track audition target · A",
        )
        .clicked()
        {
            cx.select(selected, index);
            cx.send(Action::ArmSelectedAudition);
        }
    });

    // Row 2: Inline draggable mini level fader + inline draggable Pan slider
    child.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let pan_w = 56.0;
        let slider_w = (inner_rect.width() - pan_w - 4.0).max(70.0);
        let drag_key = preview_key::channel_level(&format!("track/{}", track.id));
        let out = horizontal_mini_slider(
            ui,
            cx.state,
            &drag_key,
            &track.instrument,
            track.channel.level_db,
            -48.0,
            6.0,
            0.0,
            Unit::Decibels,
            color,
            slider_w,
            18.0,
        );
        if out.clicked {
            cx.select(selected, index);
        }
        if let Some(db) = out.committed {
            cx.send(Action::SetParameterTargetValue {
                target: format!("track/{}/level", track.id),
                value: (db * 10.0).round() / 10.0,
            });
        }

        let pan_key = preview_key::channel_pan(&format!("track/{}", track.id));
        let pan_out = horizontal_mini_slider(
            ui,
            cx.state,
            &pan_key,
            "Pan",
            track.channel.panner.primary_position(),
            -1.0,
            1.0,
            0.0,
            Unit::Pan,
            theme::ACCENT_CYAN,
            pan_w,
            18.0,
        );
        if pan_out.clicked {
            cx.select(selected, index);
        }
        if pan_out.right_clicked {
            cx.select(selected, index);
            cx.send(Action::OpenTrackPan);
        }
        if let Some(pan) = pan_out.committed {
            cx.send(Action::SetParameterTargetValue {
                target: format!("track/{}/pan", track.id),
                value: (pan * 100.0).round() / 100.0,
            });
        }
    });

    // Row 3 (when tall lane): subtle routing/clip count metadata
    if header.height() >= 70.0 {
        child.horizontal(|ui| {
            let route = match &track.channel.output {
                crate::model::OutputTarget::Master => "Master".to_owned(),
                crate::model::OutputTarget::Group(g) => format!("Grp {g}"),
                crate::model::OutputTarget::Return(r) => format!("Ret {r}"),
            };
            ui.label(
                RichText::new(format!(
                    "{route} · {} clips · {} FX",
                    track.clips.len(),
                    track.channel.inserts.len()
                ))
                .size(9.5)
                .color(theme::TEXT_MUTED),
            );
        });
    }
}

/// Resolves an audition range endpoint string (locator ID/name or musical time literal) into quarters.
fn resolve_range_quarters(spec: Option<&str>, project: &Project, fallback: f64) -> f64 {
    let Some(text) = spec else {
        return fallback;
    };
    if let Some(loc) = project
        .song
        .locators
        .iter()
        .find(|loc| loc.id == text || loc.name == text)
    {
        return loc.at.as_f64();
    }
    crate::time::parse_musical_time(text, project.song.meter)
        .map(MusicalTime::as_f64)
        .unwrap_or(fallback)
}

/// Two-band studio ruler (Locator + Loop Brace band above Bar/Beat numbers) & full-height gridlines.
fn grid(
    painter: &egui::Painter,
    geometry: &Geometry,
    rect: Rect,
    app: &Workspace,
    project: &Project,
) {
    let meter = project.song.meter;
    let bar = meter.bar_length().map_or(4.0, MusicalTime::as_f64);
    let beat = 4.0 / f64::from(meter.denominator.max(1));
    let pixels_per_quarter = f64::from(geometry.lanes.width()) / geometry.span;
    let end = geometry.start + geometry.span;

    let ruler_rect = Rect::from_min_max(
        Pos2::new(geometry.lanes.left(), rect.top()),
        Pos2::new(rect.right(), geometry.lanes.top()),
    );
    let locator_band = Rect::from_min_max(
        ruler_rect.left_top(),
        Pos2::new(ruler_rect.right(), ruler_rect.top() + LOCATOR_BAND_H),
    );
    let time_band = Rect::from_min_max(
        Pos2::new(ruler_rect.left(), ruler_rect.top() + LOCATOR_BAND_H),
        ruler_rect.max,
    );

    // Two distinct subtle surfaces for locator band vs bar-number band
    painter.rect_filled(locator_band, 0.0, Color32::from_rgb(20, 24, 31));
    painter.rect_filled(time_band, 0.0, theme::BG_SURFACE);
    painter.hline(
        ruler_rect.x_range(),
        locator_band.bottom(),
        Stroke::new(1.0_f32, with_alpha(theme::BORDER_SUBTLE, 0.65)),
    );
    painter.hline(
        ruler_rect.x_range(),
        ruler_rect.bottom(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );

    // Clamped Loop Brace in top band (never overflows actual loop range or song end)
    let loop_bounds: Option<(f64, f64, bool)> =
        if let Some(range) = app.timeline_state.visual_range() {
            Some((range.start.as_f64(), range.end.as_f64(), true))
        } else if app.audition_plan.range.looping {
            let s = resolve_range_quarters(app.audition_plan.range.from.as_deref(), project, 0.0);
            let e = resolve_range_quarters(
                app.audition_plan.range.to.as_deref(),
                project,
                project.song.end.as_f64(),
            );
            (e > s).then_some((s, e, false))
        } else {
            None
        };
    if let Some((ls, le, is_selection)) = loop_bounds {
        let x0 = geometry
            .x_of(ls)
            .clamp(ruler_rect.left(), ruler_rect.right());
        let x1 = geometry
            .x_of(le)
            .clamp(ruler_rect.left(), ruler_rect.right());
        if x1 > x0 + 2.0 {
            let brace = Rect::from_min_max(
                Pos2::new(x0, locator_band.top() + 2.0),
                Pos2::new(x1, locator_band.top() + 7.0),
            );
            painter.rect_filled(
                brace,
                2.0,
                if is_selection {
                    theme::ACCENT_AMBER
                } else {
                    with_alpha(theme::ACCENT_AMBER, 0.72)
                },
            );
            // Left and right loop-brace end hooks
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x0, brace.top()), Vec2::new(3.0, 9.0)),
                1.0,
                theme::ACCENT_AMBER,
            );
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new((x1 - 3.0).max(x0), brace.top()),
                    Vec2::new(3.0, 9.0),
                ),
                1.0,
                theme::ACCENT_AMBER,
            );
        }
    }

    // Beat lines extending down full canvas height
    if beat * pixels_per_quarter >= 10.0 {
        let mut index = (geometry.start / beat).ceil();
        while index * beat < end {
            let x = geometry.x_of(index * beat);
            painter.vline(
                x,
                geometry.full_canvas.y_range(),
                Stroke::new(1.0_f32, theme::GRID_BEAT),
            );
            painter.vline(
                x,
                (time_band.bottom() - 4.0)..=time_band.bottom(),
                Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
            );
            index += 1.0;
        }
    }

    // Bar lines & clean unobstructed ruler numerals inside bottom band
    let bar_pixels = (bar * pixels_per_quarter).max(f64::EPSILON);
    let label_every = u64::from(theme::bar_label_interval(48.0 / bar_pixels as f32));
    let line_every = (5.0 / bar_pixels).ceil().max(1.0) as u64;
    let mut index = (geometry.start / bar).ceil() as u64;
    while (index as f64) * bar < end {
        let x = geometry.x_of(index as f64 * bar);
        if index.is_multiple_of(line_every) {
            // At overview zoom only the labelled bars keep full weight; the rest recede so the
            // grid reads as structure behind the clips rather than competing with them.
            let major = index.is_multiple_of(label_every) || bar_pixels >= 24.0;
            painter.vline(
                x,
                geometry.full_canvas.y_range(),
                Stroke::new(
                    1.0_f32,
                    if major {
                        theme::GRID_BAR
                    } else {
                        theme::GRID_BEAT
                    },
                ),
            );
        }
        if index.is_multiple_of(label_every) {
            painter.vline(
                x,
                (time_band.top() + 3.0)..=time_band.bottom(),
                Stroke::new(1.0_f32, theme::BORDER_STRONG),
            );
            painter.text(
                Pos2::new(x + 4.0, time_band.center().y),
                Align2::LEFT_CENTER,
                format!("{}", index + 1),
                FontId::monospace(10.5),
                theme::TEXT_SECONDARY,
            );
        }
        index += 1;
    }
}

/// Paints color-coded rounded studio clips with solid header bands, real mini-MIDI note previews,
/// loop-repeat seam dividers, and fade wedges. Returns `true` if MIDI previews were drawn.
fn clips(
    painter: &egui::Painter,
    geometry: &Geometry,
    index: usize,
    track: &Track,
    project: &Project,
    color: Color32,
) -> bool {
    let top = geometry.lane_top(index);
    let end = geometry.start + geometry.span;
    let mut drew_any_midi = false;

    for clip in &track.clips {
        let clip_start = clip.start.as_f64();
        let clip_end = clip_start + clip.duration.as_f64();
        if clip_end <= geometry.start || clip_start >= end {
            continue;
        }
        let x0 = geometry.x_of(clip_start);
        let x1 = geometry.x_of(clip_end).max(x0 + 4.0);
        let body = Rect::from_min_max(
            Pos2::new(x0, top + 3.0),
            Pos2::new(x1, top + geometry.lane_height - 3.0),
        );
        let header_h = 15.0;
        let header_rect = Rect::from_min_max(
            body.left_top(),
            Pos2::new(body.right(), (body.top() + header_h).min(body.bottom())),
        );

        // Translucent track-colored clip body wash
        painter.rect_filled(body, 4.0, with_alpha(color, 0.18));
        // Solid clip header bar
        painter.rect_filled(
            header_rect,
            CornerRadius {
                nw: 4,
                ne: 4,
                sw: 0,
                se: 0,
            },
            with_alpha(color, 0.86),
        );

        // Real MIDI mini-note / drum-hit preview inside clip body
        let content_rect = Rect::from_min_max(
            Pos2::new(body.left() + 2.0, header_rect.bottom() + 3.0),
            Pos2::new(body.right() - 2.0, body.bottom() - 3.0),
        );
        if content_rect.height() >= 8.0
            && content_rect.width() >= 6.0
            && let Some(pattern) = project.song.patterns.iter().find(|p| p.id == clip.pattern)
        {
            // Pattern Variations carry lineage rather than Events; preview what they will play.
            let materialized;
            let pattern = if pattern.variation.is_some() {
                materialized = crate::model::Pattern {
                    events: crate::engine::schedule::materialized_pattern_events(
                        project,
                        &pattern.id,
                    )
                    .unwrap_or_default(),
                    ..pattern.clone()
                };
                &materialized
            } else {
                pattern
            };
            if !pattern.events.is_empty() {
                paint_clip_midi_preview(painter, geometry, content_rect, clip, pattern, color);
                drew_any_midi = true;
            }
        }

        // Visual fade-in / fade-out wedges when authored
        let fade_in_q = clip.fade_in.as_f64();
        if fade_in_q > 0.0 {
            let fx = geometry.x_of(clip_start + fade_in_q).min(body.right());
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(body.left(), header_rect.bottom()),
                    Pos2::new(fx, header_rect.bottom()),
                    Pos2::new(body.left(), body.bottom()),
                ],
                with_alpha(theme::BG_WELL, 0.55),
                Stroke::NONE,
            ));
            painter.line_segment(
                [
                    Pos2::new(body.left(), body.bottom()),
                    Pos2::new(fx, header_rect.bottom()),
                ],
                // A fade is a gain curve in the clip's own color, never a white strike-through.
                Stroke::new(1.0_f32, with_alpha(color, 0.8)),
            );
        }
        let fade_out_q = clip.fade_out.as_f64();
        if fade_out_q > 0.0 {
            let fx = geometry.x_of(clip_end - fade_out_q).max(body.left());
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(fx, header_rect.bottom()),
                    Pos2::new(body.right(), header_rect.bottom()),
                    Pos2::new(body.right(), body.bottom()),
                ],
                with_alpha(theme::BG_WELL, 0.55),
                Stroke::NONE,
            ));
            painter.line_segment(
                [
                    Pos2::new(fx, header_rect.bottom()),
                    Pos2::new(body.right(), body.bottom()),
                ],
                // A fade is a gain curve in the clip's own color, never a white strike-through.
                Stroke::new(1.0_f32, with_alpha(color, 0.8)),
            );
        }

        // Subtle crisp outline + right-edge resize handle accent
        painter.rect_stroke(
            body,
            4.0,
            Stroke::new(1.0_f32, with_alpha(color, 0.78)),
            StrokeKind::Inside,
        );
        if body.width() >= 18.0 {
            painter.vline(
                body.right() - 2.0,
                (header_rect.bottom() + 4.0)..=(body.bottom() - 4.0),
                Stroke::new(1.2_f32, with_alpha(color, 0.42)),
            );
        }

        // Crisp dark label inside header band
        if body.width() >= 26.0 {
            painter
                .with_clip_rect(header_rect.shrink2(Vec2::new(3.0, 0.0)))
                .text(
                    Pos2::new(header_rect.left() + 5.0, header_rect.center().y),
                    Align2::LEFT_CENTER,
                    super::widgets::ellipsize(
                        painter,
                        &clip.pattern,
                        &FontId::proportional(10.0),
                        header_rect.width() - 10.0,
                    ),
                    FontId::proportional(10.0),
                    theme::BG_WELL,
                );
        }
    }
    drew_any_midi
}

/// Renders true pitch-mapped horizontal MIDI note bars or drum-lane hits tiled across loop repeats.
fn paint_clip_midi_preview(
    painter: &egui::Painter,
    geometry: &Geometry,
    content_rect: Rect,
    clip: &Clip,
    pattern: &Pattern,
    color: Color32,
) {
    let p = painter.with_clip_rect(content_rect);
    let clip_start = clip.start.as_f64();
    let clip_dur = clip.duration.as_f64();
    let pat_len = pattern.span.as_f64().max(0.25);
    let repeats = (clip_dur / pat_len).ceil().max(1.0) as usize;

    let bright = Color32::from_rgb(
        color.r().saturating_add(55),
        color.g().saturating_add(55),
        color.b().saturating_add(55),
    );

    // Only repeats that intersect the visible window are painted, so long clips preview to their
    // end at any zoom without unbounded work.
    let first_rep = ((geometry.start - clip_start) / pat_len).floor().max(0.0) as usize;
    let last_rep = (((geometry.start + geometry.span - clip_start) / pat_len)
        .ceil()
        .max(0.0) as usize)
        .min(repeats)
        .min(first_rep + 512);

    // Loop repeat seam dividers, quieter when repeats are packed tightly at overview zoom
    let seam_px = geometry.x_of(clip_start + pat_len) - geometry.x_of(clip_start);
    if seam_px >= 6.0 {
        let seam_alpha = if seam_px < 28.0 { 0.10 } else { 0.20 };
        for rep in first_rep.max(1)..last_rep {
            let seam_q = clip_start + rep as f64 * pat_len;
            if seam_q < clip_start + clip_dur - 0.01 {
                let sx = geometry.x_of(seam_q);
                p.vline(
                    sx,
                    content_rect.y_range(),
                    Stroke::new(1.0_f32, with_alpha(theme::TEXT_PRIMARY, seam_alpha)),
                );
            }
        }
    }

    // Determine melodic pitch range if pattern has notes
    let mut min_pitch = 127u8;
    let mut max_pitch = 0u8;
    for ev in &pattern.events {
        if let PatternEvent::Note { pitch, .. } = ev {
            min_pitch = min_pitch.min(*pitch);
            max_pitch = max_pitch.max(*pitch);
        }
    }
    let pitch_span = (max_pitch.saturating_sub(min_pitch)).max(6) as f32;

    for rep in first_rep..last_rep {
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
                    velocity,
                    ..
                } => {
                    let s = offset + at.as_f64();
                    let e = (s + duration.as_f64()).min(clip_start + clip_dur);
                    if s >= clip_start + clip_dur || e <= s {
                        continue;
                    }
                    let nx0 = geometry.x_of(s);
                    let nx1 = geometry.x_of(e).max(nx0 + 2.5);
                    let frac = ((*pitch - min_pitch) as f32 / pitch_span).clamp(0.0, 1.0);
                    let note_h = (content_rect.height() / (pitch_span + 2.0)).clamp(3.0, 7.0);
                    let ny = content_rect.bottom()
                        - note_h
                        - frac * (content_rect.height() - note_h).max(0.0);
                    let alpha = (0.55 + (*velocity as f32) * 0.42).clamp(0.55, 0.96);
                    p.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(nx0, ny),
                            Pos2::new((nx1 - 1.0).max(nx0 + 2.0), ny + note_h),
                        ),
                        1.5,
                        with_alpha(bright, alpha),
                    );
                }
                PatternEvent::Hit {
                    at,
                    voice,
                    velocity,
                    ..
                } => {
                    let s = offset + at.as_f64();
                    if s >= clip_start + clip_dur {
                        continue;
                    }
                    let lane_idx = match voice {
                        DrumVoice::Hat => 0,
                        DrumVoice::Clap => 1,
                        DrumVoice::Snare => 2,
                        DrumVoice::Kick => 3,
                    };
                    let row_h = (content_rect.height() / 4.0).max(3.5);
                    let ny = content_rect.top() + lane_idx as f32 * row_h + 1.0;
                    let nx0 = geometry.x_of(s);
                    let nx1 = geometry
                        .x_of((s + 0.22).min(clip_start + clip_dur))
                        .max(nx0 + 3.5);
                    let alpha = (0.52 + (*velocity as f32) * 0.45).clamp(0.52, 0.96);
                    p.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(nx0, ny),
                            Pos2::new(nx1, (ny + row_h - 2.0).max(ny + 2.5)),
                        ),
                        1.5,
                        with_alpha(bright, alpha),
                    );
                }
                PatternEvent::Sample {
                    at,
                    duration,
                    velocity,
                    ..
                } => {
                    let s = offset + at.as_f64();
                    let e = (s + duration.as_f64()).min(clip_start + clip_dur);
                    if s >= clip_start + clip_dur || e <= s {
                        continue;
                    }
                    let nx0 = geometry.x_of(s);
                    let nx1 = geometry.x_of(e).max(nx0 + 3.0);
                    let mid = content_rect.center().y;
                    let half = (content_rect.height() * 0.28 * (*velocity as f32)).max(2.5);
                    p.rect_filled(
                        Rect::from_min_max(Pos2::new(nx0, mid - half), Pos2::new(nx1, mid + half)),
                        1.5,
                        with_alpha(bright, 0.78),
                    );
                }
            }
        }
    }
}

/// Paints audio waveform previews inside sample/fallback clips.
fn activity(
    painter: &egui::Painter,
    geometry: &Geometry,
    index: usize,
    cells: &[timeline::TimelineCell],
    color: Color32,
) {
    let top = geometry.lane_top(index) + 21.0;
    let bottom = geometry.lane_top(index) + geometry.lane_height - 6.0;
    let height = (bottom - top).max(2.0);
    let width = geometry.lanes.width() / geometry.columns as f32;
    let bright = Color32::from_rgb(
        color.r().saturating_add(45),
        color.g().saturating_add(45),
        color.b().saturating_add(45),
    );
    for (column, cell) in cells.iter().enumerate() {
        let left = geometry.lanes.left() + width * column as f32;
        let x = left..=(left + width.max(1.2));
        if cell.sample_active {
            let half = cell.sample_peak.clamp(0.0, 1.0) * height * 0.48;
            let middle = (top + bottom) * 0.5;
            painter.rect_filled(
                Rect::from_x_y_ranges(x, (middle - half.max(0.8))..=(middle + half.max(0.8))),
                0.0,
                with_alpha(bright, 0.88),
            );
        } else if cell.activity > 0.0 {
            let level = (cell.activity / (1.0 + cell.activity)).sqrt();
            let h = (level * height).max(3.0);
            painter.rect_filled(
                Rect::from_x_y_ranges(x, (bottom - h)..=bottom),
                0.5,
                with_alpha(bright, 0.82),
            );
        }
    }
}

/// Paints a live translucent clip ghost, vertical cyan snap guide, and floating Bar:Beat HUD while dragging.
fn paint_clip_drag_ghost(
    painter: &egui::Painter,
    geometry: &Geometry,
    state: &UiState,
    project: &Project,
) {
    let Some((clip_id, is_resize, is_duplicate, orig_start, orig_dur, live_q)) = &state.clip_drag
    else {
        return;
    };
    let Some((track_idx, clip)) = project
        .song
        .tracks
        .iter()
        .enumerate()
        .find_map(|(i, t)| t.clips.iter().find(|c| c.id == *clip_id).map(|c| (i, c)))
    else {
        return;
    };
    let (ghost_start, ghost_dur, guide_q, hud_text) = if *is_resize {
        let dur = (*live_q).max(0.25);
        let mt = theme::quarters_to_musical_time(dur);
        (*orig_start, dur, *orig_start + dur, format!("LEN {mt}"))
    } else {
        let st = (*live_q).max(0.0);
        let label = timeline_time_label(theme::quarters_to_musical_time(st), project.song.meter);
        let prefix = if *is_duplicate { "COPY" } else { "MOVE" };
        (st, *orig_dur, st, format!("{prefix} {label}"))
    };

    let top = geometry.lane_top(track_idx);
    let x0 = geometry.x_of(ghost_start);
    let x1 = geometry.x_of(ghost_start + ghost_dur).max(x0 + 6.0);
    let ghost_rect = Rect::from_min_max(
        Pos2::new(x0, top + 3.0),
        Pos2::new(x1, top + geometry.lane_height - 3.0),
    );
    let ghost_color = if *is_duplicate {
        theme::SIG_NOMINAL
    } else {
        theme::ACCENT_CYAN
    };
    painter.rect_filled(ghost_rect, 4.0, with_alpha(ghost_color, 0.28));
    painter.rect_stroke(
        ghost_rect,
        4.0,
        Stroke::new(1.8_f32, ghost_color),
        StrokeKind::Inside,
    );
    let guide_x = geometry.x_of(guide_q);
    painter.vline(
        guide_x,
        geometry.full_canvas.y_range(),
        Stroke::new(1.2_f32, with_alpha(ghost_color, 0.8)),
    );
    let badge = Rect::from_min_size(
        Pos2::new(
            (guide_x + 6.0).min(geometry.lanes.right() - 88.0),
            (top - 16.0).max(geometry.lanes.top() + 2.0),
        ),
        Vec2::new(84.0, 17.0),
    );
    painter.rect_filled(badge, 3.0, theme::BG_WELL);
    painter.rect_stroke(
        badge,
        3.0,
        Stroke::new(1.0_f32, ghost_color),
        StrokeKind::Inside,
    );
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        format!("{hud_text} · {}", clip.pattern),
        FontId::monospace(9.5),
        ghost_color,
    );
}

/// Range selection wash, colored Locator flags in upper band, cursor, and playhead.
fn markers(
    painter: &egui::Painter,
    geometry: &Geometry,
    rect: Rect,
    app: &Workspace,
    project: &Project,
) {
    let full_height = rect.top()..=geometry.full_canvas.bottom();
    if let Some(range) = app.timeline_state.visual_range() {
        painter.rect_filled(
            Rect::from_x_y_ranges(
                geometry.x_of(range.start.as_f64())..=geometry.x_of(range.end.as_f64()),
                geometry.full_canvas.y_range(),
            ),
            0.0,
            with_alpha(theme::ACCENT_AMBER, 0.16),
        );
    }

    // Colored locator flag tags cleanly inside the dedicated top locator band (zero overlap with bar numbers!)
    // Each flag ends before the next locator's line, so close locators never paint over
    // one another; names that do not fit are ellipsized.
    let mut locator_xs = project
        .song
        .locators
        .iter()
        .map(|locator| geometry.x_of(locator.at.as_f64()))
        .collect::<Vec<_>>();
    locator_xs.sort_by(f32::total_cmp);
    for locator in &project.song.locators {
        let at = locator.at.as_f64();
        if !geometry.visible(at) {
            continue;
        }
        let x = geometry.x_of(at);
        let room = locator_xs
            .iter()
            .find(|next| **next > x + 0.5)
            .map_or(f32::INFINITY, |next| next - x - 3.0);
        painter.vline(
            x,
            geometry.full_canvas.y_range(),
            Stroke::new(1.0_f32, with_alpha(theme::TEXT_MUTED, 0.14)),
        );
        let font = FontId::proportional(9.5);
        let flag_w = (painter
            .layout_no_wrap(locator.name.clone(), font.clone(), theme::TEXT_SECONDARY)
            .size()
            .x
            + 12.0)
            .clamp(32.0, 120.0)
            .min(room.max(14.0));
        let name = widgets::ellipsize(painter, &locator.name, &font, flag_w - 9.0);
        // A flag that would run off the right edge hangs left of its line instead.
        let flag_x = if x + flag_w > rect.right() - 2.0 {
            x - flag_w
        } else {
            x
        };
        let flag = Rect::from_min_size(
            Pos2::new(flag_x, rect.top() + 2.0),
            Vec2::new(flag_w, LOCATOR_BAND_H - 4.0),
        );
        // Neutral chip and muted tick: resting locators stay quiet beside clips and curves.
        painter.rect_filled(flag, 2.5, theme::BG_CARD);
        painter.rect_filled(
            Rect::from_min_size(flag.left_top(), Vec2::new(2.5, flag.height())),
            1.0,
            theme::TEXT_MUTED,
        );
        painter.text(
            Pos2::new(flag.left() + 6.0, flag.center().y),
            Align2::LEFT_CENTER,
            name,
            font,
            theme::TEXT_SECONDARY,
        );
    }

    // Active edit cursor
    let position = app.timeline_position();
    let cursor = position.at.as_f64();
    if geometry.visible(cursor) {
        let x = geometry.x_of(cursor);
        painter.vline(
            x,
            full_height.clone(),
            Stroke::new(1.2_f32, with_alpha(theme::SIG_NOMINAL, 0.78)),
        );
        let top = geometry.lane_top(position.track);
        painter.vline(
            x,
            top..=(top + geometry.lane_height),
            Stroke::new(2.5_f32, theme::SIG_NOMINAL),
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x - 5.0, rect.top() + LOCATOR_BAND_H),
                Pos2::new(x + 5.0, rect.top() + LOCATOR_BAND_H),
                Pos2::new(x, rect.top() + LOCATOR_BAND_H + 8.0),
            ],
            theme::SIG_NOMINAL,
            Stroke::NONE,
        ));
    }

    // Audible playhead
    if let Some(playhead) = playhead_quarters(app)
        && geometry.visible(playhead)
    {
        let x = geometry.x_of(playhead);
        painter.vline(x, full_height, Stroke::new(2.0_f32, theme::ACCENT_AMBER));
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x - 5.0, rect.top() + LOCATOR_BAND_H),
                Pos2::new(x + 5.0, rect.top() + LOCATOR_BAND_H),
                Pos2::new(x, rect.top() + LOCATOR_BAND_H + 8.0),
            ],
            theme::ACCENT_AMBER,
            Stroke::NONE,
        ));
    }
}

fn playhead_quarters(app: &Workspace) -> Option<f64> {
    (app.player.is_some() && app.candidate_is_audible()).then(|| transport_quarter_notes(app))
}

fn clip_under(project: &Project, track_index: usize, quarters: f64) -> Option<&Clip> {
    let track = project.song.tracks.get(track_index)?;
    track.clips.iter().find(|clip| {
        let s = clip.start.as_f64();
        quarters >= s && quarters <= s + clip.duration.as_f64()
    })
}

fn pointer(ui: &mut Ui, cx: &mut Cx<'_>, geometry: &Geometry, rect: Rect, project: &Project) {
    let app = cx.app;
    let tracks = project.song.tracks.len();
    let area = Rect::from_min_max(Pos2::new(geometry.lanes.left(), rect.top()), rect.max);
    let response = ui.interact(
        area,
        ui.id().with("timeline-lanes"),
        Sense::click_and_drag(),
    );
    let place = |position: Pos2| TimelineAction::PlaceCursor {
        track: geometry
            .track_at(position.y, tracks)
            .unwrap_or(app.selected_track),
        column: geometry.column_at(position.x),
        columns: geometry.columns,
    };

    // Smart-Tool cursor feedback on hover
    if let Some(hover_pos) = response.hover_pos()
        && let Some(track_idx) = geometry.track_at(hover_pos.y, tracks)
    {
        let q = geometry.quarters_of(hover_pos.x);
        if let Some(clip) = clip_under(project, track_idx, q) {
            let right_x = geometry.x_of(clip.start.as_f64() + clip.duration.as_f64());
            let lane_y = hover_pos.y - geometry.lane_top(track_idx);
            if (hover_pos.x - right_x).abs() <= 8.0 {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            } else if lane_y >= 18.0 {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
        }
    }

    // Pro Tools Smart-Tool direct manipulation:
    // - Drag right edge of clip = Resize with live ghost
    // - Plain drag on clip body (y >= lane_top + 18px) = Move clip with live ghost
    // - Alt+drag on clip = Duplicate clip with live ghost
    // - Drag on top range strip (y < lane_top + 18px) or empty lane or ruler = Range selection
    let alt_held = ui.input(|input| input.modifiers.alt);
    if response.drag_started()
        && let Some(origin) = ui.input(|input| input.pointer.press_origin())
    {
        let mut started_clip_drag = false;
        if let Some(track_idx) = geometry.track_at(origin.y, tracks) {
            let q = geometry.quarters_of(origin.x);
            let lane_y = origin.y - geometry.lane_top(track_idx);
            if let Some(clip) = clip_under(project, track_idx, q) {
                let right_x = geometry.x_of(clip.start.as_f64() + clip.duration.as_f64());
                if (origin.x - right_x).abs() <= 8.0 {
                    cx.state.clip_drag = Some((
                        clip.id.clone(),
                        true,
                        false,
                        clip.start.as_f64(),
                        clip.duration.as_f64(),
                        clip.duration.as_f64(),
                    ));
                    started_clip_drag = true;
                } else if lane_y >= 18.0 || alt_held {
                    cx.state.clip_drag = Some((
                        clip.id.clone(),
                        false,
                        alt_held,
                        clip.start.as_f64(),
                        clip.duration.as_f64(),
                        clip.start.as_f64(),
                    ));
                    started_clip_drag = true;
                }
            }
        }
        if !started_clip_drag {
            if app.timeline_state.visual() {
                cx.send(Action::CancelMode);
            }
            cx.send(place(origin));
            cx.send(TimelineAction::BeginRange);
        }
    }

    // Update live clip ghost position while dragging
    if response.dragged()
        && let Some((_, is_resize, _, orig_start, _, ref mut live_q)) = cx.state.clip_drag
        && let Some(pos) = response.interact_pointer_pos()
    {
        let q = geometry.quarters_of(pos.x);
        let grid_q = app.timeline_state.grid(project).as_f64().max(0.25);
        if is_resize {
            *live_q = (((q - orig_start) / grid_q).round() * grid_q).max(grid_q);
        } else {
            let origin_x = ui
                .input(|input| input.pointer.press_origin())
                .map_or(pos.x, |p| p.x);
            let delta_q = geometry.quarters_of(pos.x) - geometry.quarters_of(origin_x);
            *live_q = (((orig_start + delta_q) / grid_q).round() * grid_q).max(0.0);
        }
    }

    if response.drag_stopped()
        && let Some((clip_id, is_resize, is_duplicate, orig_start, orig_dur, live_q)) =
            cx.state.clip_drag.take()
    {
        if is_resize {
            if (live_q - orig_dur).abs() > 1e-4 {
                let dur_mt = theme::quarters_to_musical_time(live_q.max(0.25));
                cx.send(Action::ResizeClip {
                    clip: clip_id,
                    duration: dur_mt.to_string(),
                });
            }
        } else if is_duplicate {
            let start_mt = theme::quarters_to_musical_time(live_q.max(0.0));
            cx.send(Action::DuplicateClip {
                new_id: format!("{clip_id}_copy"),
                clip: clip_id,
                start: start_mt.to_string(),
            });
        } else if (live_q - orig_start).abs() > 1e-4 {
            let start_mt = theme::quarters_to_musical_time(live_q.max(0.0));
            cx.send(Action::MoveClip {
                clip: clip_id,
                start: start_mt.to_string(),
            });
        }
    } else if cx.state.clip_drag.is_none()
        && let Some(position) = response.interact_pointer_pos()
    {
        if response.dragged() || response.clicked() || response.secondary_clicked() {
            cx.send(place(position));
        }
        if response.double_clicked()
            && let Some(track_idx) = geometry.track_at(position.y, tracks)
        {
            let q = geometry.quarters_of(position.x);
            if clip_under(project, track_idx, q).is_some() {
                cx.send(TimelineAction::GoDefinition);
            } else if let Some(track) = project.song.tracks.get(track_idx)
                && let Some(pattern) = track
                    .clips
                    .first()
                    .and_then(|c| project.song.patterns.iter().find(|p| p.id == c.pattern))
                    .or_else(|| project.song.patterns.first())
            {
                let grid_q = app.timeline_state.grid(project).as_f64().max(1.0);
                let snapped = ((q / grid_q).floor() * grid_q).max(0.0);
                let next_num = track.clips.len() + 1;
                cx.send(Action::CreateClip {
                    track: track.id.clone(),
                    clip: format!("{}_clip_{}", track.id, next_num),
                    pattern: pattern.id.clone(),
                    start: theme::quarters_to_musical_time(snapped).to_string(),
                    duration: pattern.span.to_string(),
                });
            }
        }
    }

    // Rich studio context menu with direct clip create/split/duplicate/delete + view jumps
    let cursor_pos = app.timeline_position();
    let hovered_clip = clip_under(project, cursor_pos.track, cursor_pos.at.as_f64())
        .map(|clip| (clip.id.clone(), clip.start, clip.duration));

    response.context_menu(|ui| {
        if ui.button("Play / Pause from here").clicked() {
            cx.send(Action::TogglePlayback);
            ui.close();
        }
        if ui.button("Add Locator here…").clicked() {
            cx.send(TimelineAction::OpenLocator);
            ui.close();
        }
        ui.separator();
        if let Some((clip_id, clip_start, clip_dur)) = &hovered_clip {
            if ui.button("Open Pattern in Piano Roll").clicked() {
                cx.send(TimelineAction::GoDefinition);
                ui.close();
            }
            if ui.button("Duplicate Clip after end").clicked() {
                let new_start = clip_start.checked_add(*clip_dur).unwrap_or(*clip_start);
                cx.send(Action::DuplicateClip {
                    clip: clip_id.clone(),
                    new_id: format!("{clip_id}_copy"),
                    start: new_start.to_string(),
                });
                ui.close();
            }
            if ui.button("Split Clip at cursor").clicked() {
                cx.send(Action::SplitClip {
                    clip: clip_id.clone(),
                    new_id: format!("{clip_id}_part2"),
                    at: cursor_pos.at.to_string(),
                });
                ui.close();
            }
            if ui
                .button(RichText::new("Delete Clip").color(theme::SIG_CLIP))
                .clicked()
            {
                cx.send(Action::DeleteClip {
                    clip: clip_id.clone(),
                });
                ui.close();
            }
            ui.separator();
        } else if let Some(track) = project.song.tracks.get(cursor_pos.track)
            && let Some(pattern) = project.song.patterns.first()
        {
            if ui
                .button(format!("Create Clip ({}) here", pattern.id))
                .clicked()
            {
                cx.send(Action::CreateClip {
                    track: track.id.clone(),
                    clip: format!("{}_clip_{}", track.id, track.clips.len() + 1),
                    pattern: pattern.id.clone(),
                    start: cursor_pos.at.to_string(),
                    duration: pattern.span.to_string(),
                });
                ui.close();
            }
            ui.separator();
        }
        for (label, action) in [
            (
                "Open Track Instrument",
                Action::Timeline(TimelineAction::GoInstrument),
            ),
            (
                "Open Track Processing Chain",
                Action::Timeline(TimelineAction::GoProcessing),
            ),
            (
                "Open Track in Mixer",
                Action::Timeline(TimelineAction::GoMixer),
            ),
            ("Inspect Source", Action::Timeline(TimelineAction::GoSource)),
        ] {
            if ui.button(label).clicked() {
                cx.send(action);
                ui.close();
            }
        }
    });

    if !response.hovered() {
        return;
    }
    let (scroll, zoom) = ui.input(|input| (input.smooth_scroll_delta.x, input.zoom_delta()));
    cx.state.zoom_scroll += zoom.ln();
    if cx.state.zoom_scroll.abs() >= ZOOM_SCROLL_STEP {
        cx.send(TimelineAction::Zoom(if cx.state.zoom_scroll > 0.0 {
            1
        } else {
            -1
        }));
        cx.state.zoom_scroll = 0.0;
    }
    cx.state.pan_scroll += scroll;
    if cx.state.pan_scroll.abs() >= PAN_SCROLL_STEP {
        cx.send(TimelineAction::Pan(if cx.state.pan_scroll > 0.0 {
            -1
        } else {
            1
        }));
        cx.state.pan_scroll = 0.0;
    }
}

/// Clean single-row contextual bar at the bottom of the Timeline.
fn contextual_bar(ui: &mut Ui, cx: &mut Cx<'_>, project: &Project) {
    let app = cx.app;
    let Some(track) = project.song.tracks.get(app.selected_track) else {
        ui.label(RichText::new("No Tracks in Song").color(theme::TEXT_MUTED));
        return;
    };
    let color = theme::track_color(app.selected_track);
    let cursor = app.timeline_position().at;
    let active_clip = clip_under(project, app.selected_track, cursor.as_f64());

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
        ui.painter().circle_filled(dot_rect.center(), 4.0, color);
        ui.label(RichText::new(&track.name).strong().size(12.0));
        ui.label(
            RichText::new(format!(
                "{} · {} clips",
                track.instrument,
                track.clips.len()
            ))
            .size(11.0)
            .color(theme::TEXT_SECONDARY),
        );

        if let Some(clip) = active_clip {
            ui.separator();
            ui.label(
                RichText::new(format!("CLIP {}", clip.pattern))
                    .size(11.0)
                    .strong()
                    .color(color),
            );
            ui.label(
                RichText::new(format!(
                    "@ {} · len {} · {:+.1} dB",
                    clip.start, clip.duration, clip.gain_db
                ))
                .monospace()
                .size(10.5)
                .color(theme::TEXT_SECONDARY),
            );
        }

        // Right-aligned quick studio view jumps for the selected track
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Named as the window's tabs are.
            if pill_toggle(ui, "FX Rack", false, "Open the FX Rack · gp").clicked() {
                cx.send(TimelineAction::GoProcessing);
            }
            if pill_toggle(ui, "Synth", false, "Open the Synth · gi").clicked() {
                cx.send(TimelineAction::GoInstrument);
            }
            if pill_toggle(ui, "Console", false, "Open the Console · gm").clicked() {
                cx.send(TimelineAction::GoMixer);
            }
            // Always in place, disabled off a clip, so the meter beside it never jumps.
            if ui
                .add_enabled_ui(active_clip.is_some(), |ui| {
                    pill_toggle(ui, "Piano Roll", false, "Open Pattern · gd")
                })
                .inner
                .clicked()
            {
                cx.send(TimelineAction::GoDefinition);
            }

            let identity = format!("track/{}", track.id);
            if let Some(meter) = app
                .mix_channel_indexes
                .get(&identity)
                .copied()
                .filter(|&i| i < app.telemetry.engine.channel_count)
                .map(|i| app.telemetry.engine.channel_activity[i])
            {
                ui.separator();
                dbfs_meter_horizontal(ui, meter.current_peak, meter.average, 118.0, true);
            }
        });
    });
}
