//! The Timeline view: Tracks and Clips painted along the Song, with pointer navigation.

use super::widgets::{toolbar, unavailable};
use super::*;
use crate::time::MusicalTime;

const HEADER_WIDTH: f32 = 200.0;
pub(super) const RULER_HEIGHT: f32 = 34.0;
const LANE_HEIGHT: f32 = 44.0;
/// Width of one projected activity column. The session bins Events and waveforms per column.
const COLUMN_WIDTH: f32 = 2.0;
const MAX_COLUMNS: usize = 4096;
/// Scroll distance that pans the viewport one step.
const PAN_SCROLL_STEP: f32 = 48.0;
/// Pinch or Ctrl-scroll factor, as a natural logarithm, that changes zoom one level.
const ZOOM_SCROLL_STEP: f32 = 0.25;

const CURSOR: Color32 = palette::GOOD;
const PLAYHEAD: Color32 = palette::BAD;

pub(super) fn timeline(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(
            ui,
            "Timeline unavailable while Project source is unparseable",
        );
        return;
    };
    controls(ui, cx, project);
    egui::TopBottomPanel::bottom("crump-timeline-details")
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| {
            ui.separator();
            details(ui, app, project);
        });
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| lanes(ui, cx, project));
}

fn controls(ui: &mut Ui, cx: &mut Cx<'_>, project: &Project) {
    let app = cx.app;
    let timeline = |action: TimelineAction| Action::Timeline(action);
    ui.horizontal_wrapped(|ui| {
        toolbar(
            ui,
            cx,
            &[
                (
                    "⏮",
                    timeline(TimelineAction::MoveSongEdge { end: false }),
                    "Song start · g0",
                ),
                (
                    "◀ Clip",
                    timeline(TimelineAction::MoveBoundary(-1)),
                    "Previous Clip boundary · b",
                ),
                (
                    "◀",
                    timeline(TimelineAction::MoveGrid(-1)),
                    "One grid step back · h",
                ),
                (
                    "▶",
                    timeline(TimelineAction::MoveGrid(1)),
                    "One grid step forward · l",
                ),
                (
                    "Clip ▶",
                    timeline(TimelineAction::MoveBoundary(1)),
                    "Next Clip boundary · w",
                ),
                (
                    "⏭",
                    timeline(TimelineAction::MoveSongEdge { end: true }),
                    "Song end · g$",
                ),
            ],
        );
        ui.separator();
        ui.label("Grid");
        toolbar(
            ui,
            cx,
            &[(
                "−",
                timeline(TimelineAction::ChangeGrid(-1)),
                "Finer grid · [",
            )],
        );
        ui.label(RichText::new(app.timeline_state.grid_label()).monospace());
        toolbar(
            ui,
            cx,
            &[(
                "+",
                timeline(TimelineAction::ChangeGrid(1)),
                "Coarser grid · ]",
            )],
        );
        ui.separator();
        ui.label("Zoom");
        toolbar(
            ui,
            cx,
            &[("−", timeline(TimelineAction::Zoom(-1)), "Zoom out · -")],
        );
        ui.label(
            RichText::new(if app.timeline_viewport.is_fit() {
                "fit".to_owned()
            } else {
                format!("{}×", app.timeline_viewport.zoom_factor())
            })
            .monospace(),
        );
        toolbar(
            ui,
            cx,
            &[
                ("+", timeline(TimelineAction::Zoom(1)), "Zoom in · +"),
                (
                    "Fit",
                    timeline(TimelineAction::Fit),
                    "Fit the whole Song · Home",
                ),
            ],
        );
        if ui
            .selectable_label(app.timeline_state.follow_playhead(), "Follow")
            .on_hover_text("Keep the playhead in view · f")
            .clicked()
        {
            cx.send(TimelineAction::ToggleFollow);
        }
        ui.separator();
        toolbar(
            ui,
            cx,
            &[
                (
                    "Go to…",
                    timeline(TimelineAction::OpenGoto),
                    "Exact cursor position · :",
                ),
                (
                    "Search…",
                    timeline(TimelineAction::OpenSearch),
                    "Search Tracks, Clips, Patterns, and Locators · /",
                ),
                (
                    "Locator…",
                    timeline(TimelineAction::OpenLocator),
                    "Create a durable Locator at the cursor · M",
                ),
            ],
        );
    });
    ui.horizontal_wrapped(|ui| {
        if app.timeline_state.visual() {
            toolbar(
                ui,
                cx,
                &[
                    (
                        "Play range",
                        timeline(TimelineAction::ApplyRange { looping: false }),
                        "Audition the range once · Enter",
                    ),
                    (
                        "Loop range",
                        timeline(TimelineAction::ApplyRange { looping: true }),
                        "Audition the range as a loop · L",
                    ),
                    (
                        "Fit range",
                        timeline(TimelineAction::FitRange),
                        "Fit the range in the viewport · zf",
                    ),
                    (
                        "Swap edge",
                        timeline(TimelineAction::SwapRangeEdge),
                        "Move the other edge of the range · o",
                    ),
                    ("Cancel range", Action::CancelMode, "Leave RANGE mode · Esc"),
                ],
            );
        } else {
            toolbar(
                ui,
                cx,
                &[
                    (
                        "Select range",
                        timeline(TimelineAction::BeginRange),
                        "Start a range at the cursor, or drag across the lanes · V",
                    ),
                    (
                        "Loop",
                        Action::ToggleAuditionRangeLoop,
                        "Toggle looping of the armed range · L",
                    ),
                ],
            );
        }
        ui.separator();
        toolbar(
            ui,
            cx,
            &[
                (
                    "Arm Track",
                    Action::ArmSelectedAudition,
                    "Audition only the selected Track · A",
                ),
                (
                    "−dB",
                    Action::AdjustSelected(-1),
                    "Track level one step down · ←",
                ),
                (
                    "+dB",
                    Action::AdjustSelected(1),
                    "Track level one step up · →",
                ),
                ("Level…", Action::OpenExactEdit, "Exact Track level · Enter"),
                ("Pan…", Action::OpenTrackPan, "Exact Track pan · p"),
            ],
        );
        ui.separator();
        ui.label("Open");
        toolbar(
            ui,
            cx,
            &[
                (
                    "Pattern",
                    timeline(TimelineAction::GoDefinition),
                    "Event Grid of the Clip under the cursor · gd",
                ),
                (
                    "Source",
                    timeline(TimelineAction::GoSource),
                    "Source of the item under the cursor · gf",
                ),
                (
                    "Instrument",
                    timeline(TimelineAction::GoInstrument),
                    "Instrument of the selected Track · gi",
                ),
                (
                    "Mixer",
                    timeline(TimelineAction::GoMixer),
                    "Mixer channel of the selected Track · gm",
                ),
                (
                    "Processing",
                    timeline(TimelineAction::GoProcessing),
                    "Processing chain of the selected Track · gp",
                ),
            ],
        );
    });
    let window = app.timeline_viewport.window(project.song.end);
    ui.label(
        RichText::new(format!(
            "cursor {} · {}–{} · playhead {} · {} BPM · {}{}",
            timeline_time_label(app.timeline_position().at, project.song.meter),
            timeline_time_label(window.start, project.song.meter),
            timeline_time_label(window.end, project.song.meter),
            transport_label(app),
            project.song.tempo_bpm,
            project.song.meter,
            app.timeline_cache
                .warning()
                .map_or_else(String::new, |warning| format!(" · {warning}"))
        ))
        .monospace()
        .color(palette::MUTED),
    );
}

/// Where the lanes sit on screen and how Musical Time maps onto them.
struct Geometry {
    lanes: Rect,
    columns: usize,
    start: f64,
    span: f64,
}

impl Geometry {
    fn x_of(&self, quarters: f64) -> f32 {
        self.lanes.left() + ((quarters - self.start) / self.span) as f32 * self.lanes.width()
    }

    fn visible(&self, quarters: f64) -> bool {
        quarters >= self.start && quarters <= self.start + self.span
    }

    fn lane_top(&self, track: usize) -> f32 {
        self.lanes.top() + LANE_HEIGHT * track as f32
    }

    fn column_at(&self, x: f32) -> usize {
        let fraction = ((x - self.lanes.left()) / self.lanes.width()).clamp(0.0, 1.0);
        ((fraction * self.columns as f32) as usize).min(self.columns - 1)
    }

    /// The Track under a pointer height, or `None` above the first lane, in the ruler.
    fn track_at(&self, y: f32, tracks: usize) -> Option<usize> {
        (y >= self.lanes.top() && tracks > 0)
            .then(|| (((y - self.lanes.top()) / LANE_HEIGHT) as usize).min(tracks - 1))
    }
}

fn lanes(ui: &mut Ui, cx: &mut Cx<'_>, project: &Project) {
    let app = cx.app;
    let tracks = &project.song.tracks;
    let size = Vec2::new(
        ui.available_width().max(HEADER_WIDTH + 80.0),
        RULER_HEIGHT + LANE_HEIGHT * tracks.len().max(1) as f32,
    );
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let lanes = Rect::from_min_max(
        Pos2::new(rect.left() + HEADER_WIDTH, rect.top() + RULER_HEIGHT),
        rect.max,
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
        columns,
        start,
        span: (projection.window.end.as_f64() - start).max(f64::EPSILON),
    };
    let painter = ui.painter_at(rect);
    let timed = painter.with_clip_rect(Rect::from_min_max(
        Pos2::new(lanes.left(), rect.top()),
        rect.max,
    ));

    for (index, track) in tracks.iter().enumerate() {
        let top = geometry.lane_top(index);
        let lane = Rect::from_min_max(
            Pos2::new(lanes.left(), top),
            Pos2::new(lanes.right(), top + LANE_HEIGHT),
        );
        timed.rect_filled(
            lane,
            0.0,
            if index % 2 == 0 {
                palette::LANE
            } else {
                palette::LANE_ALTERNATE
            },
        );
        let header = Rect::from_min_max(
            Pos2::new(rect.left(), top),
            Pos2::new(lanes.left(), top + LANE_HEIGHT),
        );
        track_header(ui, cx, &painter, header, index, track);
    }
    grid(&timed, &geometry, rect, project);
    for (index, track) in tracks.iter().enumerate() {
        clips(&timed, &geometry, index, track);
        if let Some(lane) = projection
            .lanes
            .iter()
            .find(|lane| lane.track_id == track.id)
        {
            activity(&timed, &geometry, index, &lane.cells);
        }
    }
    markers(&timed, &geometry, rect, app, project);
    pointer(ui, cx, &geometry, rect, tracks.len());
}

fn track_header(
    ui: &Ui,
    cx: &mut Cx<'_>,
    painter: &egui::Painter,
    header: Rect,
    index: usize,
    track: &crate::model::Track,
) {
    let selected = cx.app.selected_track;
    let painter = painter.with_clip_rect(header);
    painter.rect_filled(
        header,
        0.0,
        if index == selected {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().faint_bg_color
        },
    );
    painter.text(
        header.left_top() + Vec2::new(8.0, 6.0),
        Align2::LEFT_TOP,
        &track.name,
        FontId::proportional(14.0),
        ui.visuals().strong_text_color(),
    );
    painter.text(
        header.left_bottom() + Vec2::new(8.0, -6.0),
        Align2::LEFT_BOTTOM,
        format!(
            "{:+.1} dB · pan {:+.2}",
            track.channel.level_db,
            track.channel.panner.primary_position()
        ),
        FontId::proportional(11.0),
        palette::MUTED,
    );
    let response = ui
        .interact(
            header,
            ui.id().with(("track-header", index)),
            Sense::click(),
        )
        .on_hover_text("Click to select · double-click for an exact level");
    if response.clicked() || response.double_clicked() {
        cx.select(selected, index);
    }
    if response.double_clicked() {
        cx.send(Action::OpenExactEdit);
    }
}

/// Bar and beat lines with bar numbers in the ruler.
fn grid(painter: &egui::Painter, geometry: &Geometry, rect: Rect, project: &Project) {
    let meter = project.song.meter;
    let bar = meter.bar_length().map_or(4.0, MusicalTime::as_f64);
    let beat = 4.0 / f64::from(meter.denominator.max(1));
    let pixels_per_quarter = f64::from(geometry.lanes.width()) / geometry.span;
    let end = geometry.start + geometry.span;
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(geometry.lanes.left(), rect.top()),
            Pos2::new(rect.right(), geometry.lanes.top()),
        ),
        0.0,
        palette::LANE_ALTERNATE,
    );
    if beat * pixels_per_quarter >= 7.0 {
        let mut index = (geometry.start / beat).ceil();
        while index * beat < end {
            painter.vline(
                geometry.x_of(index * beat),
                geometry.lanes.y_range(),
                Stroke::new(1.0_f32, palette::GRID_BEAT),
            );
            index += 1.0;
        }
    }
    let bar_pixels = (bar * pixels_per_quarter).max(f64::EPSILON);
    let label_every = (56.0 / bar_pixels).ceil().max(1.0) as u64;
    let line_every = (4.0 / bar_pixels).ceil().max(1.0) as u64;
    let mut index = (geometry.start / bar).ceil() as u64;
    while (index as f64) * bar < end {
        let x = geometry.x_of(index as f64 * bar);
        if index.is_multiple_of(line_every) {
            painter.vline(
                x,
                geometry.lanes.y_range(),
                Stroke::new(1.0_f32, palette::GRID_BAR),
            );
        }
        if index.is_multiple_of(label_every) {
            painter.vline(
                x,
                (geometry.lanes.top() - 8.0)..=geometry.lanes.top(),
                Stroke::new(1.0_f32, palette::MUTED),
            );
            painter.text(
                Pos2::new(x + 3.0, rect.top() + 3.0),
                Align2::LEFT_TOP,
                index + 1,
                FontId::proportional(11.0),
                palette::MUTED,
            );
        }
        index += 1;
    }
}

fn clips(painter: &egui::Painter, geometry: &Geometry, index: usize, track: &crate::model::Track) {
    let top = geometry.lane_top(index);
    let end = geometry.start + geometry.span;
    for clip in &track.clips {
        let clip_start = clip.start.as_f64();
        let clip_end = clip_start + clip.duration.as_f64();
        if clip_end <= geometry.start || clip_start >= end {
            continue;
        }
        let body = Rect::from_min_max(
            Pos2::new(geometry.x_of(clip_start), top + 2.0),
            Pos2::new(
                geometry.x_of(clip_end).max(geometry.x_of(clip_start) + 2.0),
                top + LANE_HEIGHT - 2.0,
            ),
        );
        // Translucent, so overlapping Clips read as a darker band.
        painter.rect_filled(body, 3.0, palette::CLIP.gamma_multiply(0.8));
        painter.rect_stroke(
            body,
            3.0,
            Stroke::new(1.0_f32, palette::CLIP_EDGE),
            StrokeKind::Inside,
        );
        if body.width() >= 28.0 {
            painter.with_clip_rect(body.shrink(2.0)).text(
                body.left_top() + Vec2::new(5.0, 2.0),
                Align2::LEFT_TOP,
                &clip.pattern,
                FontId::proportional(10.0),
                Color32::from_gray(210),
            );
        }
    }
}

/// Event onsets, sustained notes, and sample waveforms, one bar per projected column.
fn activity(
    painter: &egui::Painter,
    geometry: &Geometry,
    index: usize,
    cells: &[timeline::TimelineCell],
) {
    let top = geometry.lane_top(index) + 14.0;
    let bottom = geometry.lane_top(index) + LANE_HEIGHT - 4.0;
    let height = bottom - top;
    let width = geometry.lanes.width() / geometry.columns as f32;
    for (column, cell) in cells.iter().enumerate() {
        let left = geometry.lanes.left() + width * column as f32;
        let x = left..=(left + width.max(1.0));
        if cell.sample_active {
            let half = cell.sample_peak.clamp(0.0, 1.0) * height / 2.0;
            let middle = (top + bottom) / 2.0;
            painter.rect_filled(
                Rect::from_x_y_ranges(x, (middle - half.max(0.5))..=(middle + half.max(0.5))),
                0.0,
                palette::WARN,
            );
        } else if cell.activity > 0.0 {
            let level = (cell.activity / (1.0 + cell.activity)).sqrt();
            painter.rect_filled(
                Rect::from_x_y_ranges(x, (bottom - level * height)..=bottom),
                0.0,
                palette::ACCENT,
            );
        } else if cell.sustained > 0.0 {
            painter.rect_filled(
                Rect::from_x_y_ranges(x, (bottom - 2.0)..=bottom),
                0.0,
                palette::ACCENT.gamma_multiply(0.6),
            );
        }
    }
}

/// Range selection, Locators, cursor, and playhead.
fn markers(
    painter: &egui::Painter,
    geometry: &Geometry,
    rect: Rect,
    app: &Workspace,
    project: &Project,
) {
    let full_height = rect.top()..=rect.bottom();
    if let Some(range) = app.timeline_state.visual_range() {
        painter.rect_filled(
            Rect::from_x_y_ranges(
                geometry.x_of(range.start.as_f64())..=geometry.x_of(range.end.as_f64()),
                full_height.clone(),
            ),
            0.0,
            CURSOR.gamma_multiply(0.18),
        );
    }
    for locator in &project.song.locators {
        let at = locator.at.as_f64();
        if !geometry.visible(at) {
            continue;
        }
        let x = geometry.x_of(at);
        painter.vline(
            x,
            full_height.clone(),
            Stroke::new(1.0_f32, palette::WARN.gamma_multiply(0.7)),
        );
        painter.text(
            Pos2::new(x + 3.0, geometry.lanes.top() - 2.0),
            Align2::LEFT_BOTTOM,
            &locator.name,
            FontId::proportional(10.0),
            palette::WARN,
        );
    }
    let position = app.timeline_position();
    let cursor = position.at.as_f64();
    if geometry.visible(cursor) {
        let x = geometry.x_of(cursor);
        painter.vline(x, full_height.clone(), Stroke::new(1.0_f32, CURSOR));
        // The cursor is thicker on the Track it belongs to.
        let top = geometry.lane_top(position.track);
        painter.vline(x, top..=(top + LANE_HEIGHT), Stroke::new(3.0_f32, CURSOR));
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x - 5.0, rect.top()),
                Pos2::new(x + 5.0, rect.top()),
                Pos2::new(x, rect.top() + 8.0),
            ],
            CURSOR,
            Stroke::NONE,
        ));
    }
    if let Some(playhead) = playhead_quarters(app)
        && geometry.visible(playhead)
    {
        painter.vline(
            geometry.x_of(playhead),
            full_height,
            Stroke::new(2.0_f32, PLAYHEAD),
        );
    }
}

/// The audible transport position in quarter notes, when it describes the Candidate on screen.
fn playhead_quarters(app: &Workspace) -> Option<f64> {
    (app.player.is_some() && app.candidate_is_audible()).then(|| transport_quarter_notes(app))
}

fn pointer(ui: &mut Ui, cx: &mut Cx<'_>, geometry: &Geometry, rect: Rect, tracks: usize) {
    let app = cx.app;
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
    if response.drag_started()
        && let Some(origin) = ui.input(|input| input.pointer.press_origin())
    {
        if app.timeline_state.visual() {
            cx.send(Action::CancelMode);
        }
        cx.send(place(origin));
        cx.send(TimelineAction::BeginRange);
    }
    if let Some(position) = response.interact_pointer_pos() {
        if response.dragged() || response.clicked() || response.secondary_clicked() {
            cx.send(place(position));
        }
        if response.double_clicked() {
            cx.send(TimelineAction::GoDefinition);
        }
    }
    response.context_menu(|ui| {
        for (label, action) in [
            ("Play from here", Action::TogglePlayback),
            (
                "Add Locator here…",
                Action::Timeline(TimelineAction::OpenLocator),
            ),
            (
                "Open Pattern",
                Action::Timeline(TimelineAction::GoDefinition),
            ),
            ("Open Source", Action::Timeline(TimelineAction::GoSource)),
            (
                "Open Instrument",
                Action::Timeline(TimelineAction::GoInstrument),
            ),
            ("Open Mixer", Action::Timeline(TimelineAction::GoMixer)),
            ("Arm this Track", Action::ArmSelectedAudition),
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
        // Scrolling content to the right reveals earlier Song time.
        cx.send(TimelineAction::Pan(if cx.state.pan_scroll > 0.0 {
            -1
        } else {
            1
        }));
        cx.state.pan_scroll = 0.0;
    }
}

fn details(ui: &mut Ui, app: &Workspace, project: &Project) {
    let Some(track) = project.song.tracks.get(app.selected_track) else {
        ui.label(RichText::new("No Tracks").color(palette::MUTED));
        return;
    };
    ui.label(
        RichText::new(format!(
            "{} · Instrument {} · {:.1} dB · pan {:+.2} · {} Clip{}",
            track.name,
            track.instrument,
            track.channel.level_db,
            track.channel.panner.primary_position(),
            track.clips.len(),
            if track.clips.len() == 1 { "" } else { "s" }
        ))
        .strong(),
    );
    let clip_details = track
        .clips
        .iter()
        .map(|clip| {
            format!(
                "{} {}+{} {:?} off={} tr={:+}st gain={:+.1}db fade={}/{}{}",
                clip.id,
                clip.start,
                clip.duration,
                clip.mode,
                clip.source_offset,
                clip.transpose_semitones,
                clip.gain_db,
                clip.fade_in,
                clip.fade_out,
                clip.groove
                    .as_deref()
                    .map_or_else(String::new, |groove| format!(" groove={groove}"))
            )
        })
        .collect::<Vec<_>>();
    egui::ScrollArea::vertical()
        .max_height(54.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if clip_details.is_empty() {
                ui.label(RichText::new("No arranged Clips").color(palette::MUTED));
            }
            for clip in clip_details {
                ui.label(RichText::new(clip).monospace().small());
            }
        });
    let joined = |items: Vec<String>| {
        if items.is_empty() {
            "—".to_owned()
        } else {
            items.join(", ")
        }
    };
    ui.label(
        RichText::new(format!(
            "Locators: {} · Grooves: {}",
            joined(
                project
                    .song
                    .locators
                    .iter()
                    .map(|locator| format!("{}={} ({})", locator.id, locator.at, locator.name))
                    .collect()
            ),
            joined(
                project
                    .song
                    .grooves
                    .iter()
                    .map(|groove| groove.id.clone())
                    .collect()
            )
        ))
        .small()
        .color(palette::MUTED),
    );
}
