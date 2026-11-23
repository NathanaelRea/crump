//! Sequencing & Automation Studio:
//! - Interactive Piano Roll & Drum Step Sequencer (`EventGrid`) with direct double-click note/hit
//!   authoring, right-click context menu, live drag/resize ghosts, crisp drum hit pads, proper
//!   gutter audition NoteOff, multi-note velocity sweep commits, aligned gridlines, snap/zoom.
//! - Interactive 2D Breakpoint Curve Editor (`Automation`) with exact trapezoid polygon fill
//!   (zero protrusion), two-band locator/bar ruler, midpoint tension curve-bending handles,
//!   right-click curve/delete menu, snap/zoom, and `+ New Lane` creator.

use super::theme::{self, with_alpha};
use super::widgets::{
    Cell, apply_row_click, context_combo, led_button, pill_toggle, table, unavailable,
};
use super::*;
use crate::model::{
    AutomationInterpolation, AutomationLane, DrumVoice, ParameterTarget, Pattern, PatternEvent,
    Unit,
};
use crate::time::MusicalTime;

// ── Shared Edit Mode Switcher Header ────────────────────────────────────────

fn edit_mode_pills(ui: &mut Ui, cx: &mut Cx<'_>) {
    if pill_toggle(
        ui,
        "Piano Roll",
        cx.app.view == View::EventGrid,
        theme::ACCENT_AMBER,
        "Pattern Piano Roll & Velocity Editor · 4",
    )
    .clicked()
    {
        cx.send(Action::ShowView(View::EventGrid));
    }
    if pill_toggle(
        ui,
        "Automation",
        cx.app.view == View::Automation,
        theme::ACCENT_AMBER,
        "Interactive 2D Automation Curve Editor · 8",
    )
    .clicked()
    {
        cx.send(Action::ShowView(View::Automation));
    }
}

fn drum_voice_label(voice: DrumVoice) -> &'static str {
    match voice {
        DrumVoice::Kick => "Kick",
        DrumVoice::Snare => "Snare",
        DrumVoice::Hat => "Closed Hat",
        DrumVoice::Clap => "Clap",
    }
}

fn drum_voice_keyword(voice: DrumVoice) -> &'static str {
    match voice {
        DrumVoice::Kick => "kick",
        DrumVoice::Snare => "snare",
        DrumVoice::Hat => "hat",
        DrumVoice::Clap => "clap",
    }
}

fn drum_voice_midi(voice: DrumVoice) -> u8 {
    match voice {
        DrumVoice::Kick => 36,
        DrumVoice::Snare => 38,
        DrumVoice::Hat => 42,
        DrumVoice::Clap => 39,
    }
}

// ── Event Grid (Piano Roll + Drum Sequencer + Velocity Lane) ────────────────

pub(super) fn event_grid(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Piano Roll unavailable while Project source has errors");
        return;
    };
    let Some(pattern) = project.song.patterns.get(app.selected_pattern) else {
        unavailable(ui, "No Patterns authored in this Song");
        return;
    };

    // Release any active gutter audition key as soon as the primary mouse button lifts
    if !ui.input(|i| i.pointer.primary_down())
        && let Some(key) = cx.state.gutter_audition_key.take()
    {
        cx.send(Action::NoteOff { key });
    }

    let selected = clamp_index(app.selected_event, pattern.events.len());
    let is_looping_pattern = app.audition_loop.as_deref() == Some(pattern.id.as_str());

    // ── Slim 30px Sequencer Header Bar ──
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                edit_mode_pills(ui, cx);
                ui.separator();

                let names: Vec<String> =
                    project.song.patterns.iter().map(|p| p.id.clone()).collect();
                context_combo(
                    ui,
                    cx,
                    "Pattern",
                    &names,
                    app.selected_pattern,
                    Action::CycleContext,
                );
                ui.label(
                    RichText::new(format!("{} · {} ev", pattern.span, pattern.events.len()))
                        .monospace()
                        .size(10.5)
                        .color(theme::TEXT_SECONDARY),
                );

                ui.separator();

                // Grid snap pills: 1/4, 1/8, 1/16, 1/32, 1/8T, Off
                ui.label(
                    RichText::new("SNAP")
                        .size(9.5)
                        .strong()
                        .color(theme::TEXT_MUTED),
                );
                for (label, div) in [
                    ("1/4", 1.0_f64),
                    ("1/8", 0.5),
                    ("1/16", 0.25),
                    ("1/32", 0.125),
                    ("1/8T", 1.0 / 3.0),
                    ("Off", 0.0),
                ] {
                    let active = (cx.state.roll_snap_quarters - div).abs() < 0.01;
                    if led_button(
                        ui,
                        label,
                        active,
                        theme::ACCENT_CYAN,
                        Vec2::new(32.0, 20.0),
                        &format!("Snap edits to {label}"),
                    )
                    .clicked()
                    {
                        cx.state.roll_snap_quarters = div;
                    }
                }

                ui.separator();

                // Horizontal Zoom controls
                if led_button(
                    ui,
                    "−",
                    false,
                    theme::ACCENT_CYAN,
                    Vec2::new(18.0, 20.0),
                    "Zoom out Piano Roll",
                )
                .clicked()
                {
                    cx.state.roll_zoom = (cx.state.roll_zoom / 1.35).clamp(1.0, 4.0);
                }
                ui.label(
                    RichText::new(format!("{:.0}%", cx.state.roll_zoom * 100.0))
                        .monospace()
                        .size(10.0)
                        .color(theme::TEXT_SECONDARY),
                );
                if led_button(
                    ui,
                    "+",
                    false,
                    theme::ACCENT_CYAN,
                    Vec2::new(18.0, 20.0),
                    "Zoom in Piano Roll",
                )
                .clicked()
                {
                    cx.state.roll_zoom = (cx.state.roll_zoom * 1.35).clamp(1.0, 4.0);
                }

                ui.separator();

                if pill_toggle(
                    ui,
                    "Loop",
                    is_looping_pattern,
                    theme::ACCENT_AMBER,
                    "Loop this Pattern · L",
                )
                .clicked()
                {
                    cx.send(Action::TogglePatternLoop);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if pill_toggle(
                        ui,
                        "Event List",
                        cx.state.event_table_open,
                        theme::ACCENT_CYAN,
                        "Toggle tabular event inspector",
                    )
                    .clicked()
                    {
                        cx.state.event_table_open = !cx.state.event_table_open;
                    }
                    if let Some(ev) = pattern.events.get(selected) {
                        ui.label(
                            RichText::new(event_compact_summary(ev))
                                .monospace()
                                .size(10.5)
                                .color(theme::ACCENT_AMBER),
                        );
                    }
                });
            });
        });

    // Optional right-docked Event List inspector when toggled on
    if cx.state.event_table_open {
        egui::SidePanel::right("crump-event-list-drawer")
            .resizable(true)
            .default_width(340.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_SURFACE)
                    .inner_margin(egui::Margin::same(6))
                    .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK)),
            )
            .show_inside(ui, |ui| {
                event_inspector_table(ui, cx, pattern, selected);
            });
    }

    // Main Canvas: Interactive Piano Roll / Drum Grid (72%) + Synced Velocity & P-Lock Lane (28%)
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::BG_WELL)
                .inner_margin(egui::Margin::same(6)),
        )
        .show_inside(ui, |ui| {
            let total_h = ui.available_height().max(260.0);
            let vel_h = (total_h * 0.27).clamp(90.0, 148.0);
            let roll_h = (total_h - vel_h - 8.0).max(160.0);
            let zoom = cx.state.roll_zoom.clamp(1.0, 4.0);
            let canvas_w = (ui.available_width().max(320.0) * zoom).max(320.0);

            egui::ScrollArea::horizontal()
                .id_salt("piano-roll-hscroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        piano_roll_canvas(ui, cx, project, pattern, selected, canvas_w, roll_h);
                        velocity_and_locks_lane(
                            ui, cx, project, pattern, selected, canvas_w, vel_h,
                        );
                    });
                });
        });
}

fn event_compact_summary(ev: &PatternEvent) -> String {
    match ev {
        PatternEvent::Note {
            at,
            pitch,
            duration,
            velocity,
            ..
        } => format!(
            "{} @ {} len {} vel {:.0}%",
            crate::model::format_pitch(*pitch),
            at,
            duration,
            velocity * 100.0
        ),
        PatternEvent::Hit {
            at,
            voice,
            velocity,
            ..
        } => format!(
            "{} @ {} vel {:.0}%",
            drum_voice_label(*voice),
            at,
            velocity * 100.0
        ),
        PatternEvent::Sample {
            at,
            mapping,
            duration,
            velocity,
            ..
        } => format!(
            "{mapping} @ {} len {} vel {:.0}%",
            at,
            duration,
            velocity * 100.0
        ),
    }
}

enum RollRow {
    Pitch(u8),
    Drum(DrumVoice, String),
    Sample(String),
}

impl RollRow {
    fn label(&self) -> String {
        match self {
            Self::Pitch(p) => crate::model::format_pitch(*p),
            Self::Drum(_, name) | Self::Sample(name) => name.clone(),
        }
    }

    fn is_black_key(&self) -> bool {
        match self {
            Self::Pitch(p) => matches!(p % 12, 1 | 3 | 6 | 8 | 10),
            _ => false,
        }
    }

    fn matches(&self, event: &PatternEvent) -> bool {
        match (self, event) {
            (Self::Pitch(p), PatternEvent::Note { pitch, .. }) => p == pitch,
            (Self::Drum(v, _), PatternEvent::Hit { voice, .. }) => v == voice,
            (Self::Sample(m), PatternEvent::Sample { mapping, .. }) => m == mapping,
            _ => false,
        }
    }
}

fn build_roll_rows(pattern: &Pattern) -> (Vec<RollRow>, bool) {
    let has_notes = pattern
        .events
        .iter()
        .any(|e| matches!(e, PatternEvent::Note { .. }));
    let has_hits = pattern
        .events
        .iter()
        .any(|e| matches!(e, PatternEvent::Hit { .. }));

    if has_notes {
        let mut min_p = 127_u8;
        let mut max_p = 0_u8;
        for ev in &pattern.events {
            if let PatternEvent::Note { pitch, .. } = ev {
                min_p = min_p.min(*pitch);
                max_p = max_p.max(*pitch);
            }
        }
        let mut lo = min_p.saturating_sub(3).max(24);
        let mut hi = max_p.saturating_add(4).min(96);
        while hi.saturating_sub(lo) < 17 {
            if lo > 24 {
                lo -= 1;
            }
            if hi < 96 {
                hi += 1;
            }
        }
        ((lo..=hi).rev().map(RollRow::Pitch).collect(), false)
    } else if has_hits {
        // Kit order, matching the drum pad bank: Kick, Snare, Hat, Clap.
        let mut rows = vec![
            RollRow::Drum(
                DrumVoice::Kick,
                drum_voice_label(DrumVoice::Kick).to_owned(),
            ),
            RollRow::Drum(
                DrumVoice::Snare,
                drum_voice_label(DrumVoice::Snare).to_owned(),
            ),
            RollRow::Drum(DrumVoice::Hat, drum_voice_label(DrumVoice::Hat).to_owned()),
            RollRow::Drum(
                DrumVoice::Clap,
                drum_voice_label(DrumVoice::Clap).to_owned(),
            ),
        ];
        for ev in &pattern.events {
            if let PatternEvent::Hit { voice, .. } = ev
                && !rows
                    .iter()
                    .any(|r| matches!(r, RollRow::Drum(v, _) if v == voice))
            {
                rows.push(RollRow::Drum(*voice, drum_voice_label(*voice).to_owned()));
            }
        }
        (rows, true)
    } else {
        let mut mappings = Vec::new();
        for ev in &pattern.events {
            if let PatternEvent::Sample { mapping, .. } = ev
                && !mappings.contains(mapping)
            {
                mappings.push(mapping.clone());
            }
        }
        if mappings.is_empty() {
            ((48..=65).rev().map(RollRow::Pitch).collect(), false)
        } else {
            (mappings.into_iter().map(RollRow::Sample).collect(), false)
        }
    }
}

const ROLL_GUTTER_W: f32 = 88.0;
const ROLL_RULER_H: f32 = 24.0;

#[allow(clippy::too_many_arguments)]
fn piano_roll_canvas(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    project: &Project,
    pattern: &Pattern,
    selected: usize,
    canvas_w: f32,
    height: f32,
) {
    let app = cx.app;
    let (rows, is_drum_grid) = build_roll_rows(pattern);
    let span = pattern.span.as_f64().max(1.0);

    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(canvas_w, height), Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 4.0, theme::BG_CANVAS);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    let grid_rect = Rect::from_min_max(
        Pos2::new(rect.left() + ROLL_GUTTER_W, rect.top() + ROLL_RULER_H),
        Pos2::new(rect.right() - 1.0, rect.bottom() - 1.0),
    );
    let ruler_rect = Rect::from_min_max(
        Pos2::new(grid_rect.left(), rect.top() + 1.0),
        Pos2::new(grid_rect.right(), grid_rect.top()),
    );
    let corner_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 1.0, rect.top() + 1.0),
        Pos2::new(grid_rect.left(), grid_rect.top()),
    );

    painter.rect_filled(corner_rect, 0.0, theme::BG_SURFACE);
    painter.rect_filled(ruler_rect, 0.0, theme::BG_SURFACE);
    painter.hline(
        rect.x_range(),
        grid_rect.top(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );
    painter.text(
        corner_rect.center(),
        Align2::CENTER_CENTER,
        if is_drum_grid {
            "DRUM KIT"
        } else {
            "PIANO ROLL"
        },
        FontId::proportional(9.5),
        theme::TEXT_MUTED,
    );

    // Crisp clamped row heights: drum lanes are compact 44px step rows (never 90px pillars!)
    let row_h = if is_drum_grid {
        (grid_rect.height() / rows.len().max(4) as f32).clamp(36.0, 76.0)
    } else {
        (grid_rect.height() / rows.len().max(1) as f32).clamp(15.0, 34.0)
    };
    let active_rows_bottom = (grid_rect.top() + rows.len() as f32 * row_h).min(grid_rect.bottom());

    let x_of = |q: f64| grid_rect.left() + (q / span).clamp(0.0, 1.0) as f32 * grid_rect.width();
    let q_of =
        |x: f32| f64::from(((x - grid_rect.left()) / grid_rect.width()).clamp(0.0, 1.0)) * span;

    // Paint horizontal pitch/voice rows + left piano/drum gutter
    for (i, row) in rows.iter().enumerate() {
        let top = grid_rect.top() + i as f32 * row_h;
        if top >= grid_rect.bottom() {
            break;
        }
        let bot = (top + row_h).min(grid_rect.bottom());
        let lane = Rect::from_min_max(
            Pos2::new(grid_rect.left(), top),
            Pos2::new(grid_rect.right(), bot),
        );
        let gutter = Rect::from_min_max(
            Pos2::new(rect.left() + 1.0, top),
            Pos2::new(grid_rect.left(), bot),
        );

        let black = row.is_black_key();
        let lane_fill = if black {
            theme::BG_WELL
        } else if i % 2 == 1 {
            theme::BG_LANE_ALT
        } else {
            theme::BG_CANVAS
        };
        painter.rect_filled(lane, 0.0, lane_fill);
        painter.hline(
            lane.x_range(),
            bot,
            Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        );

        match row {
            RollRow::Pitch(p) => {
                let is_c = p % 12 == 0;
                if black {
                    painter.rect_filled(gutter, 0.0, Color32::from_rgb(20, 23, 30));
                    painter.rect_filled(
                        Rect::from_min_max(
                            gutter.left_top(),
                            Pos2::new(gutter.right() - 18.0, gutter.bottom()),
                        ),
                        0.0,
                        Color32::from_rgb(28, 33, 44),
                    );
                } else {
                    painter.rect_filled(
                        gutter,
                        0.0,
                        if is_c {
                            Color32::from_rgb(186, 194, 206)
                        } else {
                            Color32::from_rgb(156, 164, 176)
                        },
                    );
                }
                painter.hline(
                    gutter.x_range(),
                    bot,
                    Stroke::new(1.0_f32, theme::SEAM_DARK),
                );
                if is_c || row_h >= 19.0 {
                    painter.text(
                        Pos2::new(gutter.right() - 6.0, (top + bot) * 0.5),
                        Align2::RIGHT_CENTER,
                        row.label(),
                        FontId::monospace(9.5),
                        if black {
                            theme::TEXT_MUTED
                        } else {
                            theme::BG_WELL
                        },
                    );
                }
            }
            RollRow::Drum(voice, _) => {
                let accent = theme::drum_voice_color(*voice);
                let hit_count = pattern
                    .events
                    .iter()
                    .filter(|e| matches!(e, PatternEvent::Hit { voice: v, .. } if v == voice))
                    .count();
                painter.rect_filled(gutter, 0.0, theme::BG_SURFACE);
                painter.rect_filled(
                    Rect::from_min_max(
                        gutter.left_top(),
                        Pos2::new(gutter.left() + 4.0, gutter.bottom()),
                    ),
                    0.0,
                    accent,
                );
                painter.hline(
                    gutter.x_range(),
                    bot,
                    Stroke::new(1.0_f32, theme::SEAM_DARK),
                );
                painter
                    .with_clip_rect(gutter.shrink2(Vec2::new(6.0, 0.0)))
                    .text(
                        Pos2::new(gutter.left() + 10.0, top + row_h * 0.36),
                        Align2::LEFT_CENTER,
                        row.label(),
                        FontId::proportional(11.0),
                        theme::TEXT_PRIMARY,
                    );
                painter.text(
                    Pos2::new(gutter.left() + 10.0, top + row_h * 0.72),
                    Align2::LEFT_CENTER,
                    format!("{hit_count} hits"),
                    FontId::monospace(8.5),
                    theme::TEXT_MUTED,
                );
            }
            RollRow::Sample(_) => {
                let accent = theme::track_color(i);
                painter.rect_filled(gutter, 0.0, theme::BG_SURFACE);
                painter.rect_filled(
                    Rect::from_min_max(
                        gutter.left_top(),
                        Pos2::new(gutter.left() + 4.0, gutter.bottom()),
                    ),
                    0.0,
                    accent,
                );
                painter.hline(
                    gutter.x_range(),
                    bot,
                    Stroke::new(1.0_f32, theme::SEAM_DARK),
                );
                painter
                    .with_clip_rect(gutter.shrink2(Vec2::new(6.0, 0.0)))
                    .text(
                        Pos2::new(gutter.left() + 10.0, (top + bot) * 0.5),
                        Align2::LEFT_CENTER,
                        row.label(),
                        FontId::proportional(10.5),
                        theme::TEXT_PRIMARY,
                    );
            }
        }
    }

    // Subtle continuing drum/sequencer background bay below active drum rows so there's zero dead slab
    if active_rows_bottom < grid_rect.bottom() - 2.0 {
        let lower_gutter = Rect::from_min_max(
            Pos2::new(rect.left() + 1.0, active_rows_bottom),
            Pos2::new(grid_rect.left(), grid_rect.bottom()),
        );
        painter.rect_filled(lower_gutter, 0.0, theme::BG_WELL);
        let lower_grid = Rect::from_min_max(
            Pos2::new(grid_rect.left(), active_rows_bottom),
            grid_rect.max,
        );
        painter.rect_filled(lower_grid, 0.0, theme::BG_WELL);
        // Subtle step-matrix legend / author hint
        if lower_grid.height() > 40.0 {
            let hint = painter.layout_no_wrap(
                "Double-click any step cell to add a Hit or Note · Drag to move · Right-click for options"
                    .to_owned(),
                FontId::proportional(10.5),
                theme::TEXT_MUTED,
            );
            let hint_rect = Align2::CENTER_CENTER.anchor_size(lower_grid.center(), hint.size());
            painter.rect_filled(hint_rect.expand2(Vec2::new(12.0, 6.0)), 4.0, theme::BG_WELL);
            painter.galley(hint_rect.min, hint, theme::TEXT_MUTED);
        }
    }

    painter.vline(
        grid_rect.left(),
        rect.y_range(),
        Stroke::new(1.0_f32, theme::SEAM_DARK),
    );

    // Vertical bar, beat, and 1/16th subdivision gridlines + ruler numbers
    let bar_len = project
        .song
        .meter
        .bar_length()
        .map_or(4.0, MusicalTime::as_f64);
    let steps_per_quarter = 4_u32;
    let total_steps = (span * f64::from(steps_per_quarter)).ceil() as u32;
    for step in 0..=total_steps {
        let q = f64::from(step) / f64::from(steps_per_quarter);
        if q > span + 1e-6 {
            break;
        }
        let x = x_of(q);
        let is_bar = (q % bar_len).abs() < 1e-4;
        let is_beat = step.is_multiple_of(steps_per_quarter);
        let color = if is_bar {
            theme::GRID_BAR
        } else if is_beat {
            theme::GRID_BEAT
        } else {
            theme::GRID_SUB
        };
        // Drum grids end their lines at the last lane; below it is plain canvas.
        let line_bottom = if is_drum_grid {
            active_rows_bottom.max(grid_rect.top())
        } else {
            grid_rect.bottom()
        };
        painter.vline(
            x,
            grid_rect.top()..=line_bottom,
            Stroke::new(1.0_f32, color),
        );
        if is_bar {
            let bar_num = (q / bar_len).round() as u32 + 1;
            painter.vline(
                x,
                (ruler_rect.bottom() - 8.0)..=ruler_rect.bottom(),
                Stroke::new(1.0_f32, theme::BORDER_STRONG),
            );
            painter.text(
                Pos2::new(x + 4.0, ruler_rect.center().y),
                Align2::LEFT_CENTER,
                format!("{bar_num}.1"),
                FontId::monospace(10.0),
                theme::TEXT_SECONDARY,
            );
        } else if is_beat && grid_rect.width() / span as f32 >= 28.0 {
            let beat_in_bar = ((q % bar_len).round() as u32) + 1;
            let bar_num = (q / bar_len).floor() as u32 + 1;
            painter.text(
                Pos2::new(x + 3.0, ruler_rect.center().y),
                Align2::LEFT_CENTER,
                format!("{bar_num}.{beat_in_bar}"),
                FontId::monospace(9.0),
                theme::TEXT_MUTED,
            );
        }
    }

    // Paint note blocks & crisp studio drum hit diamonds/pads
    let mut blocks: Vec<(usize, Rect, f32, bool)> = Vec::with_capacity(pattern.events.len());
    for (idx, ev) in pattern.events.iter().enumerate() {
        let Some(row_idx) = rows.iter().position(|r| r.matches(ev)) else {
            continue;
        };
        let top = grid_rect.top() + row_idx as f32 * row_h;
        let at_q = ev.at().as_f64();
        let active_now = event_is_active(app, &pattern.id, ev);

        match ev {
            PatternEvent::Hit {
                voice, velocity, ..
            } => {
                let vel = *velocity as f32;
                let base_color = theme::drum_voice_color(*voice);
                let cx_hit = x_of(at_q);
                let step_w = (x_of(at_q + 0.25) - cx_hit).clamp(14.0, 28.0);
                let pad_h = (row_h - 10.0).clamp(18.0, 28.0);
                let pad_top = top + (row_h - pad_h) * 0.5;
                let block = Rect::from_min_size(
                    Pos2::new(cx_hit + 1.0, pad_top),
                    Vec2::new(step_w - 2.0, pad_h),
                );
                let alpha = (0.45 + vel.clamp(0.0, 1.0) * 0.52).clamp(0.45, 0.96);
                let fill = if active_now {
                    theme::SIG_NOMINAL
                } else {
                    with_alpha(base_color, alpha)
                };
                // Sleek tactile rounded drum pad with diamond onset jewel
                painter.rect_filled(block, 4.0, fill);
                // Bottom velocity fill indicator bar
                let vel_w = (block.width() - 4.0) * vel.clamp(0.1, 1.0);
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(block.left() + 2.0, block.bottom() - 4.0),
                        Vec2::new(vel_w, 2.5),
                    ),
                    1.0,
                    theme::TEXT_PRIMARY,
                );
                // Crisp drum diamond glyph at center
                let c = Pos2::new(block.center().x, block.center().y - 1.5);
                let r = 4.2_f32;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(c.x, c.y - r),
                        Pos2::new(c.x + r, c.y),
                        Pos2::new(c.x, c.y + r),
                        Pos2::new(c.x - r, c.y),
                    ],
                    if idx == selected {
                        theme::TEXT_PRIMARY
                    } else {
                        theme::BG_WELL
                    },
                    Stroke::NONE,
                ));
                painter.rect_stroke(
                    block,
                    4.0,
                    Stroke::new(
                        if idx == selected { 1.8_f32 } else { 1.0_f32 },
                        if idx == selected {
                            theme::TEXT_PRIMARY
                        } else {
                            with_alpha(theme::SEAM_DARK, 0.7)
                        },
                    ),
                    StrokeKind::Inside,
                );
                let _ = voice;
                blocks.push((idx, block, block.right(), false));
            }
            PatternEvent::Note {
                pitch,
                duration,
                velocity,
                ..
            } => {
                let dur_q = duration.as_f64().max(0.125);
                let vel = *velocity as f32;
                let base_color = theme::ACCENT_AMBER;
                let left = x_of(at_q);
                let right = x_of(at_q + dur_q).max(left + 10.0);
                let block = Rect::from_min_max(
                    Pos2::new(left + 0.5, top + 1.5),
                    Pos2::new(right - 0.5, (top + row_h - 1.5).min(grid_rect.bottom())),
                );
                let alpha = (0.45 + vel.clamp(0.0, 1.0) * 0.52).clamp(0.45, 0.98);
                let fill = if active_now {
                    theme::SIG_NOMINAL
                } else {
                    with_alpha(base_color, alpha)
                };
                painter.rect_filled(block, 3.0, fill);
                painter.rect_filled(
                    Rect::from_min_max(
                        block.left_top(),
                        Pos2::new(block.left() + 3.0, block.bottom()),
                    ),
                    CornerRadius {
                        nw: 3,
                        ne: 0,
                        sw: 3,
                        se: 0,
                    },
                    base_color,
                );
                painter.rect_stroke(
                    block,
                    3.0,
                    Stroke::new(
                        if idx == selected { 1.8_f32 } else { 1.0_f32 },
                        if idx == selected {
                            theme::TEXT_PRIMARY
                        } else {
                            with_alpha(theme::SEAM_DARK, 0.7)
                        },
                    ),
                    StrokeKind::Inside,
                );
                if block.width() >= 26.0 && block.height() >= 13.0 {
                    painter
                        .with_clip_rect(block.shrink2(Vec2::new(4.0, 0.0)))
                        .text(
                            Pos2::new(block.left() + 5.0, block.center().y),
                            Align2::LEFT_CENTER,
                            crate::model::format_pitch(*pitch),
                            FontId::monospace(9.5),
                            theme::BG_WELL,
                        );
                }
                blocks.push((idx, block, right, true));
            }
            PatternEvent::Sample {
                mapping,
                duration,
                velocity,
                ..
            } => {
                let dur_q = duration.as_f64().max(0.25);
                let vel = *velocity as f32;
                let base_color = theme::ACCENT_CYAN;
                let left = x_of(at_q);
                let right = x_of(at_q + dur_q).max(left + 12.0);
                let block = Rect::from_min_max(
                    Pos2::new(left + 0.5, top + 2.0),
                    Pos2::new(right - 0.5, (top + row_h - 2.0).min(grid_rect.bottom())),
                );
                let alpha = (0.45 + vel.clamp(0.0, 1.0) * 0.52).clamp(0.45, 0.96);
                painter.rect_filled(
                    block,
                    3.0,
                    if active_now {
                        theme::SIG_NOMINAL
                    } else {
                        with_alpha(base_color, alpha)
                    },
                );
                painter.rect_stroke(
                    block,
                    3.0,
                    Stroke::new(
                        if idx == selected { 1.8_f32 } else { 1.0_f32 },
                        if idx == selected {
                            theme::TEXT_PRIMARY
                        } else {
                            with_alpha(theme::SEAM_DARK, 0.7)
                        },
                    ),
                    StrokeKind::Inside,
                );
                if block.width() >= 30.0 {
                    painter
                        .with_clip_rect(block.shrink2(Vec2::new(4.0, 0.0)))
                        .text(
                            Pos2::new(block.left() + 5.0, block.center().y),
                            Align2::LEFT_CENTER,
                            mapping,
                            FontId::monospace(9.5),
                            theme::BG_WELL,
                        );
                }
                blocks.push((idx, block, right, true));
            }
        }
    }

    // Live translucent note/hit drag & resize ghost + vertical cyan snap guide
    if let Some((idx, is_resize, live_at, live_row_delta, live_dur)) = cx.state.note_drag
        && let Some(ev) = pattern.events.get(idx)
        && let Some(orig_row) = rows.iter().position(|r| r.matches(ev))
    {
        let target_row = (orig_row as i32 + live_row_delta)
            .clamp(0, rows.len().saturating_sub(1) as i32) as usize;
        let top = grid_rect.top() + target_row as f32 * row_h;
        let gx0 = x_of(live_at);
        let gx1 = x_of(live_at + live_dur.max(0.18)).max(gx0 + 14.0);
        let ghost = Rect::from_min_max(
            Pos2::new(gx0, top + 2.0),
            Pos2::new(gx1, (top + row_h - 2.0).min(grid_rect.bottom())),
        );
        painter.rect_filled(ghost, 3.0, with_alpha(theme::ACCENT_CYAN, 0.30));
        painter.rect_stroke(
            ghost,
            3.0,
            Stroke::new(1.8_f32, theme::ACCENT_CYAN),
            StrokeKind::Inside,
        );
        let guide_x = if is_resize { gx1 } else { gx0 };
        painter.vline(
            guide_x,
            grid_rect.y_range(),
            Stroke::new(1.2_f32, with_alpha(theme::ACCENT_CYAN, 0.8)),
        );
        let row_lbl = rows
            .get(target_row)
            .map_or_else(String::new, RollRow::label);
        let hud = if is_resize {
            format!("LEN {}", theme::quarters_to_musical_time(live_dur))
        } else {
            format!("{row_lbl} @ {}", theme::quarters_to_musical_time(live_at))
        };
        let pill = Rect::from_min_size(
            Pos2::new(
                (guide_x + 6.0).min(grid_rect.right() - 96.0),
                (top - 18.0).max(grid_rect.top() + 2.0),
            ),
            Vec2::new(92.0, 17.0),
        );
        painter.rect_filled(pill, 3.0, theme::BG_WELL);
        painter.rect_stroke(
            pill,
            3.0,
            Stroke::new(1.0_f32, theme::ACCENT_CYAN),
            StrokeKind::Inside,
        );
        painter.text(
            pill.center(),
            Align2::CENTER_CENTER,
            hud,
            FontId::monospace(9.5),
            theme::ACCENT_CYAN,
        );
    }

    // Parameter lock diamonds along the bottom of the ruler
    for lock in &pattern.locks {
        let x = x_of(lock.at.as_f64());
        painter.circle_filled(
            Pos2::new(x, ruler_rect.bottom() - 4.0),
            3.0,
            theme::ACCENT_VIOLET,
        );
    }

    // Live playhead cursor inside the roll
    if app.player.is_some() && app.candidate_is_audible() {
        let play_q = transport_quarter_notes(app) % span;
        let px = x_of(play_q);
        painter.vline(
            px,
            ruler_rect.top()..=grid_rect.bottom(),
            Stroke::new(1.8_f32, theme::ACCENT_AMBER),
        );
    }

    // Hover cursor feedback (ResizeHorizontal on melodic right edge, Grab on note body)
    if let Some(hover_pos) = response.hover_pos()
        && let Some((_, block, right_x, resizable)) = blocks
            .iter()
            .rev()
            .find(|(_, b, _, _)| b.expand(3.0).contains(hover_pos))
    {
        if *resizable && (hover_pos.x - right_x).abs() <= 6.0 && block.width() >= 14.0 {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        } else {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }

    // Piano / drum gutter press audition (releases cleanly via gutter_audition_key!)
    if response.is_pointer_button_down_on()
        && cx.state.gutter_audition_key.is_none()
        && let Some(pos) = response.interact_pointer_pos()
        && pos.x < grid_rect.left()
        && pos.y >= grid_rect.top()
        && pos.y < active_rows_bottom
    {
        let row_idx = ((pos.y - grid_rect.top()) / row_h) as usize;
        let pitch = match rows.get(row_idx) {
            Some(RollRow::Pitch(p)) => Some(*p),
            Some(RollRow::Drum(v, _)) => Some(drum_voice_midi(*v)),
            _ => None,
        };
        if let Some(pitch) = pitch {
            cx.send(Action::NoteOnVelocity {
                key: 'g',
                pitch,
                velocity: 0.85,
            });
            cx.state.gutter_audition_key = Some('g');
        }
    }

    // Single click selects note/hit
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && pos.x >= grid_rect.left()
        && let Some((idx, _, _, _)) = blocks
            .iter()
            .rev()
            .find(|(_, b, _, _)| b.expand(2.0).contains(pos))
    {
        cx.send(Action::SelectEvent(*idx));
    }

    // Double click: on existing note -> exact edit; on empty cell -> create note/hit directly!
    if response.double_clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && pos.x >= grid_rect.left()
        && pos.y >= grid_rect.top()
        && pos.y < active_rows_bottom
    {
        if let Some((idx, _, _, _)) = blocks
            .iter()
            .rev()
            .find(|(_, b, _, _)| b.expand(2.0).contains(pos))
        {
            cx.send(Action::SelectEvent(*idx));
            cx.send(Action::OpenExactEdit);
        } else {
            let row_idx = ((pos.y - grid_rect.top()) / row_h) as usize;
            let raw_q = q_of(pos.x);
            let snap = cx.state.roll_snap_quarters;
            let snapped_q = if snap > 0.0 {
                ((raw_q / snap).floor() * snap).clamp(0.0, (span - snap).max(0.0))
            } else {
                raw_q.clamp(0.0, (span - 0.125).max(0.0))
            };
            let at_str = theme::quarters_to_musical_time(snapped_q).to_string();
            match rows.get(row_idx) {
                Some(RollRow::Pitch(pitch)) => {
                    let dur_q = if snap > 0.0 { snap.max(0.25) } else { 0.5 };
                    cx.send(Action::CreatePatternNote {
                        pattern: pattern.id.clone(),
                        at: at_str,
                        pitch: crate::model::format_pitch(*pitch),
                        duration: theme::quarters_to_musical_time(dur_q).to_string(),
                        velocity: 0.85,
                    });
                }
                Some(RollRow::Drum(voice, _)) => {
                    cx.send(Action::CreatePatternHit {
                        pattern: pattern.id.clone(),
                        at: at_str,
                        voice: drum_voice_keyword(*voice).to_owned(),
                        velocity: 0.85,
                    });
                }
                _ => {}
            }
        }
    }

    // Drag start on note/hit
    if response.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        && let Some((idx, block, right_x, resizable)) = blocks
            .iter()
            .rev()
            .find(|(_, b, _, _)| b.expand(3.0).contains(origin))
        && let Some(ev) = pattern.events.get(*idx)
    {
        cx.send(Action::SelectEvent(*idx));
        let is_resize = *resizable && (origin.x - right_x).abs() <= 6.0 && block.width() >= 14.0;
        let orig_dur = match ev {
            PatternEvent::Note { duration, .. } | PatternEvent::Sample { duration, .. } => {
                duration.as_f64()
            }
            PatternEvent::Hit { .. } => 0.25,
        };
        cx.state.note_drag = Some((*idx, is_resize, ev.at().as_f64(), 0, orig_dur));
    }

    // Live drag preview updates
    if response.dragged()
        && let Some((idx, is_resize, ref mut live_at, ref mut live_row_delta, ref mut live_dur)) =
            cx.state.note_drag
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(ev) = pattern.events.get(idx)
    {
        let snap = cx.state.roll_snap_quarters;
        let q = q_of(pos.x);
        if is_resize {
            let raw_dur = q - ev.at().as_f64();
            *live_dur = if snap > 0.0 {
                ((raw_dur / snap).round() * snap).max(snap)
            } else {
                raw_dur.max(0.0625)
            };
        } else {
            *live_at = if snap > 0.0 {
                ((q / snap).round() * snap).clamp(0.0, (span - snap).max(0.0))
            } else {
                q.clamp(0.0, (span - 0.0625).max(0.0))
            };
            if let Some(orig_row) = rows.iter().position(|r| r.matches(ev)) {
                let target_row = (((pos.y - grid_rect.top()) / row_h).floor() as i32)
                    .clamp(0, rows.len().saturating_sub(1) as i32);
                *live_row_delta = target_row - orig_row as i32;
            }
        }
    }

    // Commit note/hit drag on release
    if response.drag_stopped()
        && let Some((idx, is_resize, live_at, live_row_delta, live_dur)) = cx.state.note_drag.take()
        && let Some(ev) = pattern.events.get(idx)
    {
        if is_resize {
            let dur_mt = theme::quarters_to_musical_time(live_dur.max(0.0625));
            cx.send(Action::SetNoteDurationValue {
                pattern: pattern.id.clone(),
                event: idx,
                duration: dur_mt.to_string(),
            });
        } else {
            if (live_at - ev.at().as_f64()).abs() > 0.01 {
                let at_mt = theme::quarters_to_musical_time(live_at);
                cx.send(Action::SetEventTimeValue {
                    pattern: pattern.id.clone(),
                    event: idx,
                    time: at_mt.to_string(),
                });
            }
            if live_row_delta != 0
                && let Some(orig_row) = rows.iter().position(|r| r.matches(ev))
            {
                let target_row = (orig_row as i32 + live_row_delta)
                    .clamp(0, rows.len().saturating_sub(1) as i32)
                    as usize;
                match (ev, rows.get(target_row)) {
                    (PatternEvent::Note { .. }, Some(RollRow::Pitch(new_pitch))) => {
                        cx.send(Action::SetNotePitchValue {
                            pattern: pattern.id.clone(),
                            event: idx,
                            pitch: crate::model::format_pitch(*new_pitch),
                        });
                    }
                    (PatternEvent::Hit { .. }, Some(RollRow::Drum(new_voice, _))) => {
                        cx.send(Action::SetHitVoiceValue {
                            pattern: pattern.id.clone(),
                            event: idx,
                            voice: drum_voice_keyword(*new_voice).to_owned(),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // Right-click context menu on selected note/hit
    response.context_menu(|ui| {
        if let Some(ev) = pattern.events.get(selected) {
            ui.label(
                RichText::new(event_compact_summary(ev))
                    .monospace()
                    .size(10.0)
                    .color(theme::ACCENT_AMBER),
            );
            ui.separator();
            if ui.button("Edit Exact Value…").clicked() {
                cx.send(Action::OpenExactEdit);
                ui.close();
            }
            for (lbl, v) in [
                ("Velocity: Soft (45%)", 0.45),
                ("Velocity: Med (75%)", 0.75),
                ("Velocity: Accent (98%)", 0.98),
            ] {
                if ui.button(lbl).clicked() {
                    cx.send(Action::SetEventVelocityValue {
                        pattern: pattern.id.clone(),
                        event: selected,
                        velocity: v,
                    });
                    ui.close();
                }
            }
            ui.separator();
            if ui
                .button(RichText::new("Delete Event").color(theme::SIG_CLIP))
                .clicked()
            {
                cx.send(Action::DeletePatternEvent {
                    pattern: pattern.id.clone(),
                    event: selected,
                });
                ui.close();
            }
        }
    });
}

/// Synced bottom Velocity lollipop lane with aligned bar/beat gridlines, multi-note sweep commit,
/// and horizontal chord/drum stem offset separation.
#[allow(clippy::too_many_arguments)]
fn velocity_and_locks_lane(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    project: &Project,
    pattern: &Pattern,
    selected: usize,
    canvas_w: f32,
    height: f32,
) {
    let span = pattern.span.as_f64().max(1.0);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(canvas_w, height), Sense::click_and_drag());
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
        Pos2::new(rect.left() + ROLL_GUTTER_W, rect.bottom() - 1.0),
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
        Pos2::new(gutter.left() + 8.0, gutter.top() + 8.0),
        Align2::LEFT_TOP,
        "VELOCITY",
        FontId::proportional(9.5),
        theme::TEXT_SECONDARY,
    );
    painter.text(
        Pos2::new(gutter.left() + 8.0, gutter.top() + 24.0),
        Align2::LEFT_TOP,
        "Drag to sculpt",
        FontId::proportional(8.5),
        theme::TEXT_MUTED,
    );
    if !pattern.locks.is_empty() {
        painter.text(
            Pos2::new(gutter.left() + 8.0, gutter.bottom() - 22.0),
            Align2::LEFT_BOTTOM,
            format!("{} P-Locks", pattern.locks.len()),
            FontId::proportional(9.0),
            theme::ACCENT_VIOLET,
        );
    }
    if pattern.variation.is_some() {
        painter.text(
            Pos2::new(gutter.left() + 8.0, gutter.bottom() - 8.0),
            Align2::LEFT_BOTTOM,
            "Variation",
            FontId::proportional(9.0),
            theme::ACCENT_CYAN,
        );
    }

    // Horizontal 25% / 50% / 75% reference lines
    for frac in [0.25, 0.5, 0.75] {
        let y = lane.bottom() - lane.height() * frac;
        painter.hline(lane.x_range(), y, Stroke::new(1.0_f32, theme::GRID_SUB));
    }

    // Aligned vertical bar/beat gridlines inside the velocity lane
    let x_of = |q: f64| lane.left() + (q / span).clamp(0.0, 1.0) as f32 * lane.width();
    let bar_len = project
        .song
        .meter
        .bar_length()
        .map_or(4.0, MusicalTime::as_f64);
    let total_beats = span.ceil() as u32;
    for beat in 0..=total_beats {
        let q = f64::from(beat);
        if q > span + 1e-4 {
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

    let pad_top = 8.0;
    let pad_bot = 6.0;
    let stem_h = (lane.height() - pad_top - pad_bot).max(12.0);

    // Precompute stem x-positions with tiny horizontal fanning for simultaneous chord/drum hits
    let mut stem_xs: Vec<f32> = Vec::with_capacity(pattern.events.len());
    for (idx, ev) in pattern.events.iter().enumerate() {
        let at_q = ev.at().as_f64();
        let same_before = pattern.events[..idx]
            .iter()
            .filter(|other| (other.at().as_f64() - at_q).abs() < 0.005)
            .count();
        let x = x_of(at_q) + 3.0 + (same_before as f32 * 4.5);
        stem_xs.push(x.min(lane.right() - 4.0));
    }

    // Interactive velocity sculpting: commits EVERY swept event on release (never drops previous!)
    if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
        && lane.contains(pos)
    {
        let nearest = stem_xs
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (**a - pos.x).abs().total_cmp(&(**b - pos.x).abs()));
        if let Some((idx, &sx)) = nearest
            && (sx - pos.x).abs() <= 14.0
        {
            let vel = f64::from(((lane.bottom() - pad_bot - pos.y) / stem_h).clamp(0.05, 1.0));
            let key = format!("vel-{}-{idx}", pattern.id);
            cx.state.set_drag_preview(&key, vel);
            cx.send(Action::SelectEvent(idx));
            if response.clicked() {
                cx.state.take_drag_preview(&key);
                cx.send(Action::SetEventVelocityValue {
                    pattern: pattern.id.clone(),
                    event: idx,
                    velocity: (vel * 100.0).round() / 100.0,
                });
            }
        }
    }
    if response.drag_stopped() {
        for idx in 0..pattern.events.len() {
            let key = format!("vel-{}-{idx}", pattern.id);
            if let Some(vel) = cx.state.take_drag_preview(&key) {
                cx.send(Action::SetEventVelocityValue {
                    pattern: pattern.id.clone(),
                    event: idx,
                    velocity: (vel * 100.0).round() / 100.0,
                });
            }
        }
    }

    // Paint color-coded lollipop velocity stems
    for (idx, ev) in pattern.events.iter().enumerate() {
        let base_vel = match ev {
            PatternEvent::Note { velocity, .. }
            | PatternEvent::Hit { velocity, .. }
            | PatternEvent::Sample { velocity, .. } => *velocity,
        };
        let key = format!("vel-{}-{idx}", pattern.id);
        let vel = cx.state.drag_preview(&key).unwrap_or(base_vel) as f32;
        let x = stem_xs[idx];
        let y_bot = lane.bottom() - pad_bot;
        let y_top = y_bot - vel.clamp(0.0, 1.0) * stem_h;
        let color = if idx == selected {
            theme::TEXT_PRIMARY
        } else {
            match ev {
                PatternEvent::Hit { voice, .. } => theme::drum_voice_color(*voice),
                PatternEvent::Sample { .. } => theme::ACCENT_CYAN,
                PatternEvent::Note { .. } => theme::ACCENT_AMBER,
            }
        };
        painter.vline(
            x,
            y_top..=y_bot,
            Stroke::new(2.4_f32, with_alpha(color, 0.85)),
        );
        painter.circle_filled(
            Pos2::new(x, y_top),
            if idx == selected { 4.2 } else { 3.2 },
            color,
        );
    }
}

fn event_inspector_table(ui: &mut Ui, cx: &mut Cx<'_>, pattern: &Pattern, selected: usize) {
    let app = cx.app;
    ui.label(
        RichText::new("EVENT LIST")
            .size(10.5)
            .strong()
            .color(theme::TEXT_SECONDARY),
    );
    let click = table(
        ui,
        cx.state,
        "event-grid",
        &[
            ("At", 72.0),
            ("Note / Voice", 104.0),
            ("Len", 68.0),
            ("Vel", 54.0),
        ],
        pattern.events.len(),
        Some(selected),
        |index| {
            let ev = &pattern.events[index];
            let (what, dur, vel) = match ev {
                PatternEvent::Note {
                    pitch,
                    duration,
                    velocity,
                    ..
                } => (
                    crate::model::format_pitch(*pitch),
                    duration.to_string(),
                    *velocity,
                ),
                PatternEvent::Hit {
                    voice, velocity, ..
                } => (
                    drum_voice_label(*voice).to_owned(),
                    "—".to_owned(),
                    *velocity,
                ),
                PatternEvent::Sample {
                    mapping,
                    duration,
                    velocity,
                    ..
                } => (mapping.clone(), duration.to_string(), *velocity),
            };
            let active = event_is_active(app, &pattern.id, ev);
            let col = if active {
                theme::SIG_NOMINAL
            } else {
                theme::TEXT_PRIMARY
            };
            vec![
                Cell::Colored(ev.at().to_string(), col),
                Cell::Colored(what, col),
                Cell::Colored(dur, theme::TEXT_SECONDARY),
                Cell::Colored(format!("{:.0}%", vel * 100.0), theme::ACCENT_AMBER),
            ]
        },
    );
    apply_row_click(cx, click, selected);
}

// ── Visual Interactive 2D Automation Curve Editor ───────────────────────────

pub(super) fn automation(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Automation unavailable while Project source has errors");
        return;
    };

    // ── Slim 30px Automation Header Bar ──
    let mut active_snapshot_eff: Option<f64> = None;
    let lane_opt = project.song.automations.get(app.selected_automation);
    let spec = lane_opt.and_then(|l| project.parameter_spec(&l.target));

    let (control_program, frame_position, _live_control) = app.control_inspection();
    let snapshot = lane_opt.and_then(|lane| {
        control_program.and_then(|inspector| {
            inspector
                .borrow_mut()
                .snapshot_target_at(frame_position, &lane.target)
        })
    });
    if let Some(ref s) = snapshot {
        active_snapshot_eff = Some(s.effective);
    }

    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                edit_mode_pills(ui, cx);
                ui.separator();

                let names: Vec<String> = project
                    .song
                    .automations
                    .iter()
                    .map(|l| format!("{} · {}", l.id, l.target))
                    .collect();
                context_combo(
                    ui,
                    cx,
                    "Lane",
                    &names,
                    app.selected_automation,
                    Action::CycleContext,
                );

                // "+ New Lane" menu button for creating automation on any automatable song parameter
                ui.menu_button(
                    RichText::new("+ Lane").size(11.0).color(theme::SIG_NOMINAL),
                    |ui| {
                        ui.set_min_width(220.0);
                        ui.label(
                            RichText::new("CREATE AUTOMATION LANE")
                                .size(9.5)
                                .strong()
                                .color(theme::TEXT_MUTED),
                        );
                        ui.separator();
                        let mut count = 0usize;
                        for inst in &project.song.instruments {
                            for param in &inst.parameters {
                                let target = format!("instrument/{}/{}", inst.id, param.key);
                                if project
                                    .song
                                    .automations
                                    .iter()
                                    .any(|l| l.target.to_string() == target)
                                {
                                    continue;
                                }
                                if ui
                                    .button(format!("{} · {}", inst.name, param.name))
                                    .clicked()
                                {
                                    cx.send(Action::CreateAutomationLane {
                                        lane: format!("{}_{}", inst.id, param.key),
                                        target,
                                        initial_value: theme::format_crump_literal(
                                            param.value,
                                            param.unit,
                                        ),
                                    });
                                    ui.close();
                                }
                                count += 1;
                            }
                        }
                        for track in &project.song.tracks {
                            let target = format!("track/{}/level", track.id);
                            if !project
                                .song
                                .automations
                                .iter()
                                .any(|l| l.target.to_string() == target)
                            {
                                if ui.button(format!("{} · Level", track.name)).clicked() {
                                    cx.send(Action::CreateAutomationLane {
                                        lane: format!("{}_level", track.id),
                                        target,
                                        initial_value: theme::format_crump_literal(
                                            track.channel.level_db,
                                            Unit::Decibels,
                                        ),
                                    });
                                    ui.close();
                                }
                                count += 1;
                            }
                        }
                        if count == 0 {
                            ui.label(
                                RichText::new("All primary parameters automated")
                                    .color(theme::TEXT_MUTED),
                            );
                        }
                    },
                );

                ui.separator();

                // Snap pills
                ui.label(
                    RichText::new("SNAP")
                        .size(9.5)
                        .strong()
                        .color(theme::TEXT_MUTED),
                );
                for (lbl, div) in [("1/4", 1.0_f64), ("1/8", 0.5), ("1/16", 0.25), ("Off", 0.0)] {
                    let active = (cx.state.auto_snap_quarters - div).abs() < 0.01;
                    if led_button(
                        ui,
                        lbl,
                        active,
                        theme::ACCENT_CYAN,
                        Vec2::new(32.0, 20.0),
                        &format!("Snap breakpoints to {lbl}"),
                    )
                    .clicked()
                    {
                        cx.state.auto_snap_quarters = div;
                    }
                }

                ui.separator();

                // Horizontal Zoom
                if led_button(
                    ui,
                    "−",
                    false,
                    theme::ACCENT_CYAN,
                    Vec2::new(18.0, 20.0),
                    "Zoom out Automation",
                )
                .clicked()
                {
                    cx.state.auto_zoom = (cx.state.auto_zoom / 1.35).clamp(1.0, 4.0);
                }
                if led_button(
                    ui,
                    "+",
                    false,
                    theme::ACCENT_CYAN,
                    Vec2::new(18.0, 20.0),
                    "Zoom in Automation",
                )
                .clicked()
                {
                    cx.state.auto_zoom = (cx.state.auto_zoom * 1.35).clamp(1.0, 4.0);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if pill_toggle(
                        ui,
                        "Point List",
                        cx.state.automation_table_open,
                        theme::ACCENT_CYAN,
                        "Toggle automation point table",
                    )
                    .clicked()
                    {
                        cx.state.automation_table_open = !cx.state.automation_table_open;
                    }
                    if let Some(val) = &snapshot {
                        ui.label(
                            RichText::new(format!(
                                "EFF {}",
                                theme::format_compact_value(val.effective, val.unit)
                            ))
                            .monospace()
                            .size(10.5)
                            .strong()
                            .color(theme::ACCENT_CYAN),
                        );
                    } else if let Some(s) = spec {
                        ui.label(
                            RichText::new(format!(
                                "BASE {}",
                                theme::format_compact_value(s.base, s.unit)
                            ))
                            .monospace()
                            .size(10.5)
                            .color(theme::TEXT_SECONDARY),
                        );
                    }
                });
            });
        });

    let Some(lane) = lane_opt else {
        unavailable(
            ui,
            "No Song Automation lanes authored yet · Click + Lane above to create one",
        );
        return;
    };

    let unit = spec.map_or(Unit::Ratio, |s| s.unit);
    let min_val = spec.map_or(0.0, |s| s.minimum);
    let max_val = spec.map_or(1.0, |s| s.maximum);
    let selected = clamp_index(app.selected_automation_point, lane.points.len());

    // Compact bottom selected-point bar
    egui::TopBottomPanel::bottom("crump-auto-point-bar")
        .frame(
            egui::Frame::new()
                .fill(theme::BG_SURFACE)
                .inner_margin(egui::Margin::symmetric(10, 5))
                .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK)),
        )
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(pt) = lane.points.get(selected) {
                    ui.label(
                        RichText::new(format!("POINT {}", pt.id))
                            .size(10.5)
                            .strong()
                            .color(theme::ACCENT_AMBER),
                    );
                    ui.label(
                        RichText::new(format!(
                            "@ {}  ·  {}",
                            pt.at,
                            unit.parse_value(&pt.value)
                                .map_or_else(|_| pt.value.clone(), |v| theme::format_compact_value(v, unit))
                        ))
                            .monospace()
                            .size(11.0)
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new("CURVE OUT")
                            .size(9.5)
                            .strong()
                            .color(theme::TEXT_MUTED),
                    );
                    for (label, curve_str, is_active) in [
                        (
                            "Step",
                            "step",
                            matches!(pt.interpolation, AutomationInterpolation::Step),
                        ),
                        (
                            "Linear",
                            "linear",
                            matches!(pt.interpolation, AutomationInterpolation::Linear),
                        ),
                        (
                            "Exp",
                            "power(2)",
                            matches!(pt.interpolation, AutomationInterpolation::Power(e) if e > 1.0),
                        ),
                        (
                            "Log",
                            "power(0.5)",
                            matches!(pt.interpolation, AutomationInterpolation::Power(e) if e < 1.0),
                        ),
                    ] {
                        if pill_toggle(
                            ui,
                            label,
                            is_active,
                            theme::ACCENT_CYAN,
                            "Set outgoing curve shape (or drag diamond midpoint handle)",
                        )
                        .clicked()
                        {
                            cx.send(Action::SetAutomationPointCurve {
                                lane: lane.id.clone(),
                                point: pt.id.clone(),
                                curve: curve_str.to_owned(),
                            });
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if lane.points.len() > 1
                            && pt.at != MusicalTime::ZERO
                            && led_button(
                                ui,
                                "Delete",
                                false,
                                theme::SIG_CLIP,
                                Vec2::new(54.0, 22.0),
                                "Delete selected automation point",
                            )
                            .clicked()
                        {
                            cx.send(Action::DeleteAutomationPoint {
                                lane: lane.id.clone(),
                                point: pt.id.clone(),
                            });
                        }
                        if pill_toggle(
                            ui,
                            "Exact…",
                            false,
                            theme::ACCENT_AMBER,
                            "Edit exact value · Enter",
                        )
                        .clicked()
                        {
                            cx.send(Action::OpenExactEdit);
                        }
                        ui.label(
                            RichText::new(
                                "Double-click add · Drag handles or mid-diamonds · Right-click curve",
                            )
                            .size(9.5)
                            .color(theme::TEXT_MUTED),
                        );
                    });
                }
            });
        });

    if cx.state.automation_table_open {
        egui::SidePanel::right("crump-auto-table-drawer")
            .resizable(true)
            .default_width(340.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_SURFACE)
                    .inner_margin(egui::Margin::same(6))
                    .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK)),
            )
            .show_inside(ui, |ui| {
                let click = table(
                    ui,
                    cx.state,
                    "automation",
                    &[
                        ("Point", 86.0),
                        ("Time", 76.0),
                        ("Value", 84.0),
                        ("Curve", 72.0),
                    ],
                    lane.points.len(),
                    Some(selected),
                    |idx| {
                        let pt = &lane.points[idx];
                        vec![
                            pt.id.clone().into(),
                            pt.at.to_string().into(),
                            Cell::Colored(pt.value.clone(), theme::ACCENT_CYAN),
                            pt.interpolation.to_string().into(),
                        ]
                    },
                );
                apply_row_click(cx, click, selected);
            });
    }

    // Main Full-Height Interactive 2D Curve Canvas
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::BG_WELL)
                .inner_margin(egui::Margin::same(8)),
        )
        .show_inside(ui, |ui| {
            let zoom = cx.state.auto_zoom.clamp(1.0, 4.0);
            let avail = ui.available_size().max(Vec2::new(320.0, 240.0));
            let canvas_size = Vec2::new(avail.x * zoom, avail.y);
            egui::ScrollArea::horizontal()
                .id_salt("auto-canvas-hscroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    automation_curve_canvas(
                        ui,
                        cx,
                        project,
                        lane,
                        selected,
                        unit,
                        min_val,
                        max_val,
                        active_snapshot_eff,
                        canvas_size,
                    );
                });
        });
}

#[allow(clippy::too_many_arguments)]
fn automation_curve_canvas(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    project: &Project,
    lane: &AutomationLane,
    selected: usize,
    unit: Unit,
    min_val: f64,
    max_val: f64,
    live_effective: Option<f64>,
    canvas_size: Vec2,
) {
    let app = cx.app;
    let (rect, response) = ui.allocate_exact_size(canvas_size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 4.0, theme::BG_CANVAS);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    const AXIS_W: f32 = 72.0;
    const LOC_BAND_H: f32 = 16.0;
    const RULER_H: f32 = 38.0;
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + AXIS_W, rect.top() + RULER_H),
        Pos2::new(rect.right() - 14.0, rect.bottom() - 12.0),
    );
    let locator_band = Rect::from_min_max(
        Pos2::new(plot.left(), rect.top() + 1.0),
        Pos2::new(rect.right() - 1.0, rect.top() + 1.0 + LOC_BAND_H),
    );
    let bar_band = Rect::from_min_max(
        Pos2::new(plot.left(), locator_band.bottom()),
        Pos2::new(rect.right() - 1.0, plot.top()),
    );
    let axis = Rect::from_min_max(
        Pos2::new(rect.left() + 1.0, plot.top()),
        Pos2::new(plot.left(), rect.bottom() - 1.0),
    );

    // Corner, two-band ruler, and Y-axis backdrop
    painter.rect_filled(
        Rect::from_min_max(
            rect.left_top() + Vec2::splat(1.0),
            Pos2::new(plot.left(), plot.top()),
        ),
        0.0,
        theme::BG_SURFACE,
    );
    painter.rect_filled(locator_band, 0.0, Color32::from_rgb(20, 24, 31));
    painter.rect_filled(bar_band, 0.0, theme::BG_SURFACE);
    painter.rect_filled(axis, 0.0, theme::BG_SURFACE);
    painter.hline(
        Pos2::new(plot.left(), 0.0).x..=rect.right(),
        locator_band.bottom(),
        Stroke::new(1.0_f32, with_alpha(theme::BORDER_SUBTLE, 0.65)),
    );
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
        Pos2::new(rect.left() + AXIS_W * 0.5, rect.top() + RULER_H * 0.5),
        Align2::CENTER_CENTER,
        format!("{unit}").to_uppercase(),
        FontId::monospace(9.5),
        theme::TEXT_MUTED,
    );

    let song_end = project.song.end.as_f64().max(16.0);
    let bar_len = project
        .song
        .meter
        .bar_length()
        .map_or(4.0, MusicalTime::as_f64);
    let x_of = |q: f64| plot.left() + (q / song_end).clamp(0.0, 1.0) as f32 * plot.width();
    let q_of = |x: f32| f64::from(((x - plot.left()) / plot.width()).clamp(0.0, 1.0)) * song_end;
    let y_pad = 8.0_f32;
    let y_top_val = plot.top() + y_pad;
    let y_bot_val = plot.bottom() - y_pad;
    let y_span = (y_bot_val - y_top_val).max(20.0);
    let y_of = |v: f64| {
        let n = theme::value_to_norm(v, min_val, max_val, unit);
        y_bot_val - n * y_span
    };
    let val_of = |y: f32| {
        let n = ((y_bot_val - y) / y_span).clamp(0.0, 1.0);
        theme::norm_to_value(n, min_val, max_val, unit)
    };

    // Ghost clip silhouettes from the target track
    let ghost_track = project
        .song
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
            let x0 = x_of(clip.start.as_f64());
            let x1 = x_of(clip.start.as_f64() + clip.duration.as_f64());
            let ghost = Rect::from_min_max(
                Pos2::new(x0, plot.top() + 2.0),
                Pos2::new(x1, plot.bottom() - 2.0),
            );
            // A neutral lift marks where the owning track plays; the track hue would tint the
            // whole lane.
            painter.rect_filled(ghost, 2.0, with_alpha(theme::TEXT_PRIMARY, 0.025));
            painter.rect_stroke(
                ghost,
                2.0,
                Stroke::new(1.0_f32, with_alpha(tr_col, 0.10)),
                StrokeKind::Inside,
            );
        }
    }

    // Horizontal physical-unit reference gridlines & cleanly inset Y-axis labels
    // The range ends are labelled exactly; interior references are rounded values placed at
    // their true positions, so the ruler reads like a scale rather than arbitrary numbers.
    let mut ruler: Vec<f64> = vec![min_val, max_val];
    let interior: Vec<f64> = if matches!(unit, Unit::Hertz) {
        [
            20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0,
        ]
        .into_iter()
        .filter(|v| *v > min_val && *v < max_val)
        .collect()
    } else {
        let raw = (max_val - min_val) / 4.0;
        let mag = 10_f64.powf(raw.abs().max(1e-9).log10().floor());
        let step = [1.0, 2.0, 2.5, 5.0, 10.0]
            .into_iter()
            .map(|m| m * mag)
            .find(|s| *s >= raw)
            .unwrap_or(10.0 * mag);
        let mut v = (min_val / step).floor() * step + step;
        let mut out = Vec::new();
        while v < max_val - 1e-9 && out.len() < 8 {
            out.push(v);
            v += step;
        }
        out
    };
    let y_of_val = |v: f64| y_bot_val - theme::value_to_norm(v, min_val, max_val, unit) * y_span;
    for v in interior {
        let y = y_of_val(v);
        if ruler.iter().all(|r| (y_of_val(*r) - y).abs() >= 22.0) {
            ruler.push(v);
        }
    }
    for v in ruler {
        let y = y_of_val(v);
        painter.hline(plot.x_range(), y, Stroke::new(1.0_f32, theme::GRID_BEAT));
        painter.text(
            Pos2::new(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            theme::format_compact_value(v, unit),
            FontId::monospace(9.5),
            theme::TEXT_SECONDARY,
        );
    }

    // Vertical bar gridlines + bar numerals (bottom band) + Locator pills (top band, ZERO collision!)
    let bar_px = (bar_len / song_end) as f32 * plot.width();
    let label_every = theme::bar_label_interval(52.0 / bar_px.max(1.0));
    let total_bars = (song_end / bar_len).ceil() as u32;
    for bar in 0..=total_bars {
        let q = f64::from(bar) * bar_len;
        if q > song_end + 1e-4 {
            break;
        }
        let x = x_of(q);
        let major = bar.is_multiple_of(label_every) || bar_px >= 24.0;
        painter.vline(
            x,
            plot.y_range(),
            Stroke::new(
                1.0_f32,
                if major {
                    theme::GRID_BAR
                } else {
                    theme::GRID_SUB
                },
            ),
        );
        if bar.is_multiple_of(label_every) {
            painter.vline(
                x,
                (bar_band.top() + 4.0)..=bar_band.bottom(),
                Stroke::new(1.0_f32, theme::BORDER_STRONG),
            );
            painter.text(
                Pos2::new(x + 4.0, bar_band.center().y),
                Align2::LEFT_CENTER,
                format!("{}", bar + 1),
                FontId::monospace(10.0),
                theme::TEXT_SECONDARY,
            );
        }
    }
    for loc in &project.song.locators {
        let x = x_of(loc.at.as_f64());
        painter.vline(
            x,
            plot.y_range(),
            Stroke::new(1.0_f32, with_alpha(theme::SIG_HOT, 0.30)),
        );
        let fw = (loc.name.len() as f32 * 5.6 + 10.0).clamp(28.0, 82.0);
        // A flag that would run off the right edge hangs left of its line instead.
        let flag_x = if x + fw > locator_band.right() - 2.0 {
            x - fw
        } else {
            x
        };
        let flag = Rect::from_min_size(
            Pos2::new(flag_x, locator_band.top() + 1.5),
            Vec2::new(fw, LOC_BAND_H - 3.0),
        );
        painter.rect_filled(flag, 2.0, with_alpha(theme::SIG_HOT, 0.90));
        painter.text(
            Pos2::new(flag.left() + 4.0, flag.center().y),
            Align2::LEFT_CENTER,
            &loc.name,
            FontId::proportional(9.0),
            theme::BG_WELL,
        );
    }

    // Resolve points with any live drag preview or curvature bend applied
    let resolved: Vec<(usize, &str, f64, f64, AutomationInterpolation)> = lane
        .points
        .iter()
        .enumerate()
        .map(|(idx, pt)| {
            let parsed_v = unit.parse_value(&pt.value).unwrap_or(min_val);
            let drag_v = cx
                .state
                .drag_preview(&format!("auto-v-{}-{}", lane.id, pt.id))
                .unwrap_or(parsed_v);
            let drag_q = cx
                .state
                .drag_preview(&format!("auto-q-{}-{}", lane.id, pt.id))
                .unwrap_or(pt.at.as_f64());
            let interp = match cx.state.auto_curve_drag {
                Some((c_idx, live_exp)) if c_idx == idx => AutomationInterpolation::Power(live_exp),
                _ => pt.interpolation,
            };
            (idx, pt.id.as_str(), drag_q, drag_v, interp)
        })
        .collect();

    // Sample true interpolated polyline & midpoint tension handles
    let mut curve_pts: Vec<Pos2> = Vec::new();
    let mut mid_handles: Vec<(usize, Pos2, f64)> = Vec::new();
    for seg in resolved.windows(2) {
        let (idx0, _, q0, v0, interp) = seg[0];
        let (_, _, q1, v1, _) = seg[1];
        match interp {
            AutomationInterpolation::Step => {
                curve_pts.push(Pos2::new(x_of(q0), y_of(v0)));
                curve_pts.push(Pos2::new(x_of(q1), y_of(v0)));
                curve_pts.push(Pos2::new(x_of(q1), y_of(v1)));
            }
            AutomationInterpolation::Linear => {
                // Playback interpolates linearly in value; on a log axis that is a curve, so
                // sample it rather than drawing a screen-space chord.
                let steps = 28;
                for s in 0..=steps {
                    let t = f64::from(s) / f64::from(steps);
                    curve_pts.push(Pos2::new(
                        x_of(q0 + t * (q1 - q0)),
                        y_of(v0 + t * (v1 - v0)),
                    ));
                }
                let mid_q = (q0 + q1) * 0.5;
                let mid_v = (v0 + v1) * 0.5;
                if (x_of(q1) - x_of(q0)) >= 28.0 {
                    mid_handles.push((idx0, Pos2::new(x_of(mid_q), y_of(mid_v)), 1.0));
                }
            }
            AutomationInterpolation::Power(exp) => {
                let steps = 28;
                for s in 0..=steps {
                    let t = f64::from(s) / f64::from(steps);
                    let q = q0 + t * (q1 - q0);
                    let v = v0 + t.powf(exp) * (v1 - v0);
                    curve_pts.push(Pos2::new(x_of(q), y_of(v)));
                }
                let mid_q = (q0 + q1) * 0.5;
                let mid_v = v0 + (0.5_f64).powf(exp) * (v1 - v0);
                if (x_of(q1) - x_of(q0)) >= 28.0 {
                    mid_handles.push((idx0, Pos2::new(x_of(mid_q), y_of(mid_v)), exp));
                }
            }
        }
    }
    if let Some(&(_, _, last_q, last_v, _)) = resolved.last() {
        if curve_pts.is_empty() {
            curve_pts.push(Pos2::new(x_of(last_q), y_of(last_v)));
        }
        if last_q < song_end {
            curve_pts.push(Pos2::new(x_of(song_end), y_of(last_v)));
        }
    }

    // P0 FIX: Exact convex trapezoid fill strictly hugging every curve segment (ZERO protrusion!)
    if curve_pts.len() >= 2 {
        let fill_col = with_alpha(theme::ACCENT_CYAN, 0.14);
        for pair in curve_pts.windows(2) {
            let p0 = pair[0];
            let p1 = pair[1];
            if (p1.x - p0.x).abs() >= 0.3 {
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        p0,
                        p1,
                        Pos2::new(p1.x, plot.bottom()),
                        Pos2::new(p0.x, plot.bottom()),
                    ],
                    fill_col,
                    Stroke::NONE,
                ));
            }
        }
        painter.add(egui::Shape::line(
            curve_pts,
            Stroke::new(2.2_f32, theme::ACCENT_CYAN),
        ));
    }

    // Paint midpoint curvature tension diamond handles
    for &(seg_idx, mpos, _exp) in &mid_handles {
        let active_bend = cx.state.auto_curve_drag.is_some_and(|(i, _)| i == seg_idx);
        let r = if active_bend { 5.0 } else { 3.8 };
        let diamond = vec![
            Pos2::new(mpos.x, mpos.y - r),
            Pos2::new(mpos.x + r, mpos.y),
            Pos2::new(mpos.x, mpos.y + r),
            Pos2::new(mpos.x - r, mpos.y),
        ];
        painter.add(egui::Shape::convex_polygon(
            diamond,
            if active_bend {
                theme::ACCENT_AMBER
            } else {
                theme::BG_WELL
            },
            Stroke::new(
                1.3_f32,
                if active_bend {
                    theme::TEXT_PRIMARY
                } else {
                    with_alpha(theme::ACCENT_CYAN, 0.85)
                },
            ),
        ));
    }

    // Live playhead & glowing effective value dot
    if app.player.is_some() && app.candidate_is_audible() {
        let px = x_of(transport_quarter_notes(app));
        painter.vline(
            px,
            bar_band.top()..=plot.bottom(),
            Stroke::new(1.6_f32, theme::ACCENT_AMBER),
        );
        if let Some(eff) = live_effective {
            let ey = y_of(eff);
            painter.circle_filled(
                Pos2::new(px, ey),
                7.0,
                with_alpha(theme::ACCENT_AMBER, 0.25),
            );
            painter.circle_filled(Pos2::new(px, ey), 3.8, theme::ACCENT_AMBER);
        }
    }

    // Paint draggable breakpoint handles + clamped callout badge
    let mut handle_positions: Vec<(usize, Pos2)> = Vec::with_capacity(resolved.len());
    for &(idx, _id, q, v, _) in &resolved {
        let pos = Pos2::new(x_of(q), y_of(v));
        let is_sel = idx == selected;
        if is_sel {
            painter.circle_filled(pos, 9.5, with_alpha(theme::ACCENT_AMBER, 0.25));
        }
        painter.circle_filled(
            pos,
            if is_sel { 5.4 } else { 4.2 },
            if is_sel {
                theme::ACCENT_AMBER
            } else {
                theme::BG_WELL
            },
        );
        painter.circle_stroke(
            pos,
            if is_sel { 5.4 } else { 4.2 },
            Stroke::new(
                1.8_f32,
                if is_sel {
                    theme::TEXT_PRIMARY
                } else {
                    theme::ACCENT_CYAN
                },
            ),
        );
        if is_sel {
            let label = theme::format_compact_value(v, unit);
            let pill_w = (label.len() as f32 * 6.5 + 12.0).clamp(44.0, 96.0);
            let pill_x = (pos.x + 10.0).clamp(plot.left() + 4.0, plot.right() - pill_w - 4.0);
            let pill_y = (pos.y - 22.0).clamp(plot.top() + 4.0, plot.bottom() - 20.0);
            let pill = Rect::from_min_size(Pos2::new(pill_x, pill_y), Vec2::new(pill_w, 17.0));
            painter.rect_filled(pill, 3.0, theme::BG_WELL);
            painter.rect_stroke(
                pill,
                3.0,
                Stroke::new(1.0_f32, theme::ACCENT_AMBER),
                StrokeKind::Inside,
            );
            painter.text(
                pill.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::monospace(9.5),
                theme::TEXT_PRIMARY,
            );
        }
        handle_positions.push((idx, pos));
    }

    // Hover cursor feedback for handles & curve tension diamonds
    if let Some(hpos) = response.hover_pos() {
        if handle_positions
            .iter()
            .any(|(_, p)| p.distance(hpos) <= 11.0)
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        } else if mid_handles.iter().any(|(_, p, _)| p.distance(hpos) <= 10.0) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
    }

    // Single click selects point
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some((idx, _)) = handle_positions
            .iter()
            .find(|(_, p)| p.distance(pos) <= 11.0)
    {
        cx.send(Action::SelectAutomationPoint(*idx));
    }

    // Double click: on handle -> exact edit; on midpoint diamond -> reset linear; on empty canvas -> add point
    if response.double_clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && plot.contains(pos)
    {
        if let Some((idx, _)) = handle_positions
            .iter()
            .find(|(_, p)| p.distance(pos) <= 11.0)
        {
            cx.send(Action::SelectAutomationPoint(*idx));
            cx.send(Action::OpenExactEdit);
        } else if let Some((seg_idx, _, _)) =
            mid_handles.iter().find(|(_, p, _)| p.distance(pos) <= 10.0)
            && let Some(pt) = lane.points.get(*seg_idx)
        {
            cx.send(Action::SetAutomationPointCurve {
                lane: lane.id.clone(),
                point: pt.id.clone(),
                curve: "linear".to_owned(),
            });
        } else {
            let snap = cx.state.auto_snap_quarters;
            let raw_q = q_of(pos.x);
            let q = if snap > 0.0 {
                ((raw_q / snap).round() * snap).clamp(snap, song_end)
            } else {
                raw_q.clamp(0.125, song_end)
            };
            let v = val_of(pos.y);
            let at_mt = theme::quarters_to_musical_time(q);
            let mut next_num = lane.points.len() + 1;
            let mut new_id = format!("p{next_num}");
            while lane.points.iter().any(|p| p.id == new_id) {
                next_num += 1;
                new_id = format!("p{next_num}");
            }
            cx.send(Action::CreateAutomationPoint {
                lane: lane.id.clone(),
                point: new_id,
                at: at_mt.to_string(),
                value: theme::format_crump_literal(v, unit),
                curve: "linear".to_owned(),
            });
        }
    }

    // Drag start: breakpoint handle OR midpoint curvature diamond
    if response.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        if let Some((idx, _)) = handle_positions
            .iter()
            .find(|(_, p)| p.distance(origin) <= 12.0)
        {
            cx.send(Action::SelectAutomationPoint(*idx));
            cx.state.auto_point_drag = Some(*idx);
        } else if let Some((seg_idx, _, cur_exp)) = mid_handles
            .iter()
            .find(|(_, p, _)| p.distance(origin) <= 11.0)
        {
            cx.send(Action::SelectAutomationPoint(*seg_idx));
            cx.state.auto_curve_drag = Some((*seg_idx, *cur_exp));
        }
    }

    // Active midpoint curvature drag
    if response.dragged()
        && let Some((seg_idx, ref mut live_exp)) = cx.state.auto_curve_drag
    {
        let dy = ui.input(|i| i.pointer.delta().y);
        if dy.abs() > 0.0 {
            let factor = (f64::from(dy) * 0.015).exp();
            *live_exp = (*live_exp * factor).clamp(0.25, 4.0);
        }
        let _ = seg_idx;
    }

    // Active breakpoint 2D drag
    if response.dragged()
        && let Some(idx) = cx.state.auto_point_drag
        && let Some(pt) = lane.points.get(idx)
        && let Some(pos) = response.interact_pointer_pos()
    {
        let v = val_of(pos.y);
        cx.state
            .set_drag_preview(&format!("auto-v-{}-{}", lane.id, pt.id), v);
        if idx > 0 {
            let snap = cx.state.auto_snap_quarters;
            let min_q = lane
                .points
                .get(idx - 1)
                .map_or(0.25, |prev| prev.at.as_f64() + 0.125);
            let max_q = lane
                .points
                .get(idx + 1)
                .map_or(song_end, |next| (next.at.as_f64() - 0.125).max(min_q));
            let raw_q = q_of(pos.x);
            let snapped_q = if snap > 0.0 {
                ((raw_q / snap).round() * snap).clamp(min_q, max_q)
            } else {
                raw_q.clamp(min_q, max_q)
            };
            cx.state
                .set_drag_preview(&format!("auto-q-{}-{}", lane.id, pt.id), snapped_q);
        }
    }

    // Commit curvature bend or breakpoint drag
    if response.drag_stopped() {
        if let Some((seg_idx, final_exp)) = cx.state.auto_curve_drag.take()
            && let Some(pt) = lane.points.get(seg_idx)
        {
            let curve = if (final_exp - 1.0).abs() < 0.06 {
                "linear".to_owned()
            } else {
                format!("power({final_exp:.2})")
            };
            cx.send(Action::SetAutomationPointCurve {
                lane: lane.id.clone(),
                point: pt.id.clone(),
                curve,
            });
        }
        if let Some(idx) = cx.state.auto_point_drag.take()
            && let Some(pt) = lane.points.get(idx)
        {
            if let Some(new_v) = cx
                .state
                .take_drag_preview(&format!("auto-v-{}-{}", lane.id, pt.id))
            {
                cx.send(Action::SetAutomationPointValue {
                    lane: lane.id.clone(),
                    point: pt.id.clone(),
                    value: theme::format_crump_literal(new_v, unit),
                });
            }
            if let Some(new_q) = cx
                .state
                .take_drag_preview(&format!("auto-q-{}-{}", lane.id, pt.id))
                && (new_q - pt.at.as_f64()).abs() > 0.05
            {
                let at_mt = theme::quarters_to_musical_time(new_q);
                cx.send(Action::SetAutomationPointTime {
                    lane: lane.id.clone(),
                    point: pt.id.clone(),
                    time: at_mt.to_string(),
                });
            }
        }
    }

    // Right-click studio context menu on automation curve/points
    response.context_menu(|ui| {
        if let Some(pt) = lane.points.get(selected) {
            ui.label(
                RichText::new(format!("POINT {} · @ {}", pt.id, pt.at))
                    .monospace()
                    .size(10.0)
                    .color(theme::ACCENT_AMBER),
            );
            ui.separator();
            for (lbl, curve) in [
                ("Curve: Linear", "linear"),
                ("Curve: Step", "step"),
                ("Curve: Exponential", "power(2)"),
                ("Curve: Logarithmic", "power(0.5)"),
            ] {
                if ui.button(lbl).clicked() {
                    cx.send(Action::SetAutomationPointCurve {
                        lane: lane.id.clone(),
                        point: pt.id.clone(),
                        curve: curve.to_owned(),
                    });
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Edit Exact Value…").clicked() {
                cx.send(Action::OpenExactEdit);
                ui.close();
            }
            if lane.points.len() > 1
                && pt.at != MusicalTime::ZERO
                && ui
                    .button(RichText::new("Delete Point").color(theme::SIG_CLIP))
                    .clicked()
            {
                cx.send(Action::DeleteAutomationPoint {
                    lane: lane.id.clone(),
                    point: pt.id.clone(),
                });
                ui.close();
            }
        }
    });
}
