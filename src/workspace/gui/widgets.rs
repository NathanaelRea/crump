//! Custom studio audio widgets: rotary knobs, console faders, logarithmic dBFS & GR meters,
//! hardware LED switches, staggered 2-octave piano keyboard, drum pads, and sleek tables.

use std::f32::consts::PI;

use egui_extras::{Column, TableBuilder};

use super::theme::{self, with_alpha};
use super::*;
use crate::model::Unit;
use crate::workspace::keymap::{DRUM_NOTES, KEYBOARD_NOTES};

/// Workspace text with symbols egui's bundled fonts lack replaced by covered glyphs.
pub(super) fn legible(text: &str) -> std::borrow::Cow<'_, str> {
    const SUBSTITUTES: [(char, char); 6] = [
        ('←', '⬅'),
        ('→', '➡'),
        ('↑', '⬆'),
        ('↓', '⬇'),
        ('●', '⏺'),
        ('│', '|'),
    ];
    if !text.contains(|character| SUBSTITUTES.iter().any(|(from, _)| *from == character)) {
        return text.into();
    }
    text.chars()
        .map(|character| {
            SUBSTITUTES
                .iter()
                .find_map(|(from, to)| (*from == character).then_some(*to))
                .unwrap_or(character)
        })
        .collect::<String>()
        .into()
}

/// Interaction output from a continuous studio control (knob, fader, mini slider).
pub(super) struct ControlOutcome {
    /// Final committed physical value when a drag finishes or double-click resets.
    pub committed: Option<f64>,
    /// Live display value (drag preview while dragging, otherwise authored value).
    pub live_value: f64,
    /// Single click on the control.
    pub clicked: bool,
    /// Secondary click (context / exact modal).
    pub right_clicked: bool,
}

// ── Logarithmic dBFS & Gain Reduction Metering ──────────────────────────────

/// Paints a calibrated vertical segmented logarithmic dBFS meter (`-60..+6 dBFS`) showing RMS body, Peak crest, peak-hold line, and clip latch LED.
pub(super) fn paint_dbfs_meter_vertical(painter: &egui::Painter, rect: Rect, peak: f32, avg: f32) {
    painter.rect_filled(rect, 2.0, theme::BG_WELL);
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0_f32, theme::SEAM_DARK),
        StrokeKind::Inside,
    );

    let inner = rect.shrink(1.5);
    if inner.height() < 6.0 || inner.width() < 3.0 {
        return;
    }

    let clip_h = 4.0_f32.min(inner.height() * 0.12);
    let bar_top = inner.top() + clip_h + 1.5;
    let bar_height = (inner.bottom() - bar_top).max(1.0);

    // Top clip latch LED
    let clipped = peak >= 0.99;
    painter.rect_filled(
        Rect::from_min_max(
            inner.left_top(),
            Pos2::new(inner.right(), inner.top() + clip_h),
        ),
        1.0,
        if clipped {
            theme::SIG_CLIP
        } else {
            with_alpha(theme::SIG_CLIP, 0.14)
        },
    );

    let peak_db = theme::amp_to_dbfs(peak);
    let avg_db = theme::amp_to_dbfs(avg);
    let peak_norm = theme::dbfs_to_norm(peak_db);
    let avg_norm = theme::dbfs_to_norm(avg_db);

    let segments = ((bar_height / 3.5) as usize).clamp(8, 44);
    let seg_gap = 1.0_f32;
    let seg_h = ((bar_height - seg_gap * (segments as f32 - 1.0)) / segments as f32).max(1.2);

    // Subtle 0 dBFS reference notch along edges
    let zero_y = inner.bottom() - theme::dbfs_to_norm(0.0) * bar_height;
    painter.hline(
        inner.x_range(),
        zero_y.clamp(bar_top, inner.bottom()),
        Stroke::new(0.8_f32, with_alpha(theme::TEXT_SECONDARY, 0.35)),
    );

    for s in 0..segments {
        let frac_low = s as f32 / segments as f32;
        let frac_high = (s + 1) as f32 / segments as f32;
        let y_bot = inner.bottom() - s as f32 * (seg_h + seg_gap);
        let y_top = (y_bot - seg_h).max(bar_top);
        let seg_rect = Rect::from_min_max(
            Pos2::new(inner.left(), y_top),
            Pos2::new(inner.right(), y_bot),
        );
        // Colour zones follow the meter's own dBFS scale: red above 0 dBFS, amber above -6.
        let frac_mid = (frac_low + frac_high) * 0.5;
        let base_color = if frac_mid >= theme::dbfs_to_norm(0.0) {
            theme::SIG_CLIP
        } else if frac_mid >= theme::dbfs_to_norm(-6.0) {
            theme::SIG_HOT
        } else {
            theme::SIG_NOMINAL
        };
        let color = if avg_norm >= frac_low + 0.01 {
            base_color
        } else if peak_norm >= frac_low + 0.015 {
            with_alpha(base_color, 0.68)
        } else {
            with_alpha(base_color, 0.09)
        };
        painter.rect_filled(seg_rect, 0.6, color);
    }

    // Crisp peak-hold hairline
    if peak_norm > 0.02 {
        let py = inner.bottom() - peak_norm * bar_height;
        painter.hline(
            inner.x_range(),
            py.clamp(bar_top, inner.bottom()),
            Stroke::new(1.5_f32, theme::TEXT_PRIMARY),
        );
    }
}

/// Paints an honest horizontal segmented logarithmic dBFS meter (RMS core + Peak crest)
/// with optional dB readout, always laid out left-to-right regardless of parent layout.
pub(super) fn dbfs_meter_horizontal(
    ui: &mut Ui,
    peak: f32,
    avg: f32,
    width: f32,
    show_readout: bool,
) -> egui::Response {
    let readout_w = if show_readout { 52.0 } else { 0.0 };
    let bar_w = (width - readout_w - if show_readout { 5.0 } else { 0.0 }).max(24.0);
    ui.allocate_ui_with_layout(
        Vec2::new(width, 14.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            let (rect, response) = ui.allocate_exact_size(Vec2::new(bar_w, 10.0), Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 2.5, theme::BG_WELL);
            painter.rect_stroke(
                rect,
                2.5,
                Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
                StrokeKind::Inside,
            );
            let inner = rect.shrink(1.5);
            let peak_db = theme::amp_to_dbfs(peak);
            let peak_norm = theme::dbfs_to_norm(peak_db);
            let avg_norm = theme::dbfs_to_norm(theme::amp_to_dbfs(avg));

            let segments = ((inner.width() / 4.0) as usize).clamp(10, 32);
            let gap = 1.0_f32;
            let seg_w =
                ((inner.width() - gap * (segments as f32 - 1.0)) / segments as f32).max(1.5);

            for s in 0..segments {
                let frac_low = s as f32 / segments as f32;
                let frac_high = (s + 1) as f32 / segments as f32;
                let x0 = inner.left() + s as f32 * (seg_w + gap);
                let x1 = (x0 + seg_w).min(inner.right());
                // Same dBFS zones as the vertical meter: red above 0 dBFS, amber above -6.
                let frac_mid = (frac_low + frac_high) * 0.5;
                let base_color = if frac_mid >= theme::dbfs_to_norm(0.0) {
                    theme::SIG_CLIP
                } else if frac_mid >= theme::dbfs_to_norm(-6.0) {
                    theme::SIG_HOT
                } else {
                    theme::SIG_NOMINAL
                };
                let color = if avg_norm >= frac_low + 0.01 {
                    base_color
                } else if peak_norm >= frac_low + 0.015 {
                    with_alpha(base_color, 0.68)
                } else {
                    with_alpha(base_color, 0.10)
                };
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(x0, inner.top()), Pos2::new(x1, inner.bottom())),
                    0.6,
                    color,
                );
            }
            if peak_norm > 0.02 {
                let px = inner.left() + peak_norm * inner.width();
                painter.vline(
                    px.clamp(inner.left(), inner.right()),
                    inner.y_range(),
                    Stroke::new(1.5_f32, theme::TEXT_PRIMARY),
                );
            }
            if show_readout {
                // A fixed, right-aligned slot so `-4.2 dB` and `-inf dB` never move neighbours.
                // Healthy levels read neutral; colour is kept for near-clip and clip.
                let db_text = format!("{} dB", theme::format_dbfs(peak));
                let color = if peak >= 0.99 {
                    theme::SIG_CLIP
                } else if peak_db >= -1.0 {
                    theme::SIG_HOT
                } else {
                    theme::TEXT_SECONDARY
                };
                let (slot, _) = ui.allocate_exact_size(Vec2::new(readout_w, 14.0), Sense::hover());
                ui.painter().text(
                    slot.right_center(),
                    Align2::RIGHT_CENTER,
                    db_text,
                    FontId::monospace(10.5),
                    color,
                );
            }
            response
        },
    )
    .inner
}

/// Paints a top-down amber Gain Reduction meter (`0` to `-24 dB`) for dynamics processors.
pub(super) fn paint_gr_meter_vertical(
    painter: &egui::Painter,
    rect: Rect,
    gr_db: f32,
    max_gr_db: f32,
) {
    painter.rect_filled(rect, 2.0, theme::BG_WELL);
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0_f32, theme::SEAM_DARK),
        StrokeKind::Inside,
    );
    let inner = rect.shrink(1.5);
    let gr_norm = (gr_db.abs() / 24.0).clamp(0.0, 1.0);
    let max_norm = (max_gr_db.abs() / 24.0).clamp(0.0, 1.0);
    if gr_norm > 0.005 {
        let h = inner.height() * gr_norm;
        painter.rect_filled(
            Rect::from_min_max(inner.left_top(), Pos2::new(inner.right(), inner.top() + h)),
            1.0,
            theme::ACCENT_AMBER,
        );
    }
    if max_norm > 0.01 {
        let y = inner.top() + inner.height() * max_norm;
        painter.hline(
            inner.x_range(),
            y.clamp(inner.top(), inner.bottom()),
            Stroke::new(1.5_f32, theme::SIG_CLIP),
        );
    }
}

// ── Tactile Rotary Studio Knob ──────────────────────────────────────────────

/// Draws a studio rotary knob with 270° swept arc, optional live cyan modulation halo,
/// vertical drag (`Shift` = fine), double-click reset, and monospace value readout.
#[allow(clippy::too_many_arguments)]
pub(super) fn rotary_knob(
    ui: &mut Ui,
    state: &mut UiState,
    drag_key: &str,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    default: f64,
    unit: Unit,
    accent: Color32,
    mod_effective: Option<f64>,
    tooltip: &str,
    diameter: f32,
) -> ControlOutcome {
    rotary_knob_modulated(
        ui,
        state,
        drag_key,
        label,
        value,
        min,
        max,
        default,
        unit,
        accent,
        None,
        mod_effective,
        tooltip,
        diameter,
    )
}

/// Studio rotary knob supporting both authored static modulation range (`mod_range`, visible even
/// when transport is stopped) and live animated modulation (`mod_effective`).
#[allow(clippy::too_many_arguments)]
pub(super) fn rotary_knob_modulated(
    ui: &mut Ui,
    state: &mut UiState,
    drag_key: &str,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    default: f64,
    unit: Unit,
    accent: Color32,
    mod_range: Option<(f64, f64)>,
    mod_effective: Option<f64>,
    tooltip: &str,
    diameter: f32,
) -> ControlOutcome {
    let label_h = if label.is_empty() { 0.0 } else { 13.0 };
    let val_h = 13.0;
    // Reserve the full painted extent (scale ticks and modulation halo) so nothing the dial
    // draws can reach into the label above or the value readout below.
    let radius = diameter * 0.42;
    let arc_radius = radius + 4.2;
    let outer = arc_radius + if diameter >= 36.0 { 6.5 } else { 4.4 };
    let below = radius.max(outer * 0.71) + 2.0;
    let total_w = (diameter + 24.0).max(58.0).max(outer * 2.0 + 4.0);
    let total_h = label_h + 3.0 + outer + below + 3.0 + val_h;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(total_w, total_h), Sense::click_and_drag());

    let live_value = state.drag_preview(drag_key).unwrap_or(value);
    let mut committed = None;
    let mut current = live_value;

    if response.drag_started() {
        state.set_drag_preview(drag_key, current);
    }
    if response.dragged() {
        let delta_y = -ui.input(|input| input.pointer.delta().y);
        if delta_y.abs() > 0.0 {
            let fine = ui.input(|input| input.modifiers.shift);
            let sensitivity = if fine { 0.0008 } else { 0.0055 };
            let norm = (theme::value_to_norm(current, min, max, unit) + delta_y * sensitivity)
                .clamp(0.0, 1.0);
            current = theme::norm_to_value(norm, min, max, unit);
            state.set_drag_preview(drag_key, current);
        }
    }
    if response.drag_stopped()
        && let Some(final_val) = state.take_drag_preview(drag_key)
    {
        current = final_val;
        if (final_val - value).abs() > 1e-6 {
            committed = Some(final_val);
        }
    }
    if response.double_clicked() {
        current = default.clamp(min, max);
        state.hold_preview(drag_key, current);
        committed = Some(current);
    }

    let painter = ui.painter();
    let center = Pos2::new(rect.center().x, rect.top() + label_h + 3.0 + outer);

    // 270° arc: start at 135° (bottom-left), sweep clockwise to 405° (bottom-right)
    let start_angle = 0.75 * PI;
    let sweep = 1.5 * PI;
    let norm = theme::value_to_norm(current, min, max, unit);
    let val_angle = start_angle + norm * sweep;
    let bipolar = min < 0.0 && max > 0.0;

    // Subtle radial scale ticks around the outer perimeter
    if diameter >= 36.0 {
        let tick_count = 10;
        let tick_r0 = arc_radius + 2.5;
        let tick_r1 = arc_radius + 5.0;
        for i in 0..=tick_count {
            let t = i as f32 / tick_count as f32;
            let a = start_angle + t * sweep;
            let is_center = bipolar && i == tick_count / 2;
            let r_end = if is_center { tick_r1 + 1.5 } else { tick_r1 };
            let p0 = Pos2::new(center.x + tick_r0 * a.cos(), center.y + tick_r0 * a.sin());
            let p1 = Pos2::new(center.x + r_end * a.cos(), center.y + r_end * a.sin());
            painter.line_segment(
                [p0, p1],
                Stroke::new(
                    if is_center { 1.3_f32 } else { 0.8_f32 },
                    if is_center {
                        theme::TEXT_SECONDARY
                    } else {
                        with_alpha(theme::BORDER_STRONG, 0.7)
                    },
                ),
            );
        }
    }

    // Recessed background track arc
    paint_arc(
        painter,
        center,
        arc_radius,
        start_angle,
        start_angle + sweep,
        Stroke::new(3.4_f32, theme::BG_WELL),
    );
    paint_arc(
        painter,
        center,
        arc_radius,
        start_angle,
        start_angle + sweep,
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
    );

    // Active value arc (bipolar from 12 o'clock if min < 0 < max)
    let (arc_a, arc_b) = if bipolar {
        let zero_norm = theme::value_to_norm(0.0, min, max, unit);
        let zero_angle = start_angle + zero_norm * sweep;
        if val_angle >= zero_angle {
            (zero_angle, val_angle)
        } else {
            (val_angle, zero_angle)
        }
    } else {
        (start_angle, val_angle)
    };
    if (arc_b - arc_a).abs() > 0.02 {
        paint_arc(
            painter,
            center,
            arc_radius,
            arc_a,
            arc_b,
            Stroke::new(3.2_f32, accent),
        );
    }

    // Static authored modulation range halo (visible even when transport stopped)
    if let Some((mod_lo, mod_hi)) = mod_range {
        let lo_a =
            start_angle + theme::value_to_norm(mod_lo.clamp(min, max), min, max, unit) * sweep;
        let hi_a =
            start_angle + theme::value_to_norm(mod_hi.clamp(min, max), min, max, unit) * sweep;
        let (ma, mb) = if hi_a >= lo_a {
            (lo_a, hi_a)
        } else {
            (hi_a, lo_a)
        };
        if (mb - ma).abs() > 0.03 {
            paint_arc(
                painter,
                center,
                arc_radius + 3.4,
                ma,
                mb,
                Stroke::new(2.0_f32, with_alpha(theme::ACCENT_CYAN, 0.72)),
            );
        }
    }

    // Outer bright cyan live modulation halo if live/preview effective value differs from base
    if let Some(eff) = mod_effective {
        let eff_norm = theme::value_to_norm(eff, min, max, unit);
        let eff_angle = start_angle + eff_norm * sweep;
        let (ma, mb) = if eff_angle >= val_angle {
            (val_angle, eff_angle)
        } else {
            (eff_angle, val_angle)
        };
        if (mb - ma).abs() > 0.03 {
            paint_arc(
                painter,
                center,
                arc_radius + 3.4,
                ma,
                mb,
                Stroke::new(2.2_f32, theme::ACCENT_CYAN),
            );
        }
    }

    // Subtle drop shadow under machined cap
    painter.circle_filled(
        center + Vec2::new(0.0, 1.5),
        radius + 0.5,
        with_alpha(theme::SEAM_DARK, 0.65),
    );

    // Machined metallic dark dial cap
    let cap_fill = if response.dragged() {
        theme::BG_ACTIVE
    } else if response.hovered() {
        theme::BG_HOVER
    } else {
        theme::BG_CARD
    };
    painter.circle_filled(center, radius, cap_fill);
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(
            1.2_f32,
            if response.hovered() || response.dragged() {
                accent
            } else {
                theme::BORDER_STRONG
            },
        ),
    );
    // Subtle top specular highlight ring
    painter.circle_stroke(
        center,
        (radius - 2.0).max(2.0),
        Stroke::new(0.8_f32, with_alpha(theme::TEXT_PRIMARY, 0.10)),
    );

    // Crisp pointer notch
    let p_inner = Pos2::new(
        center.x + (radius * 0.25) * val_angle.cos(),
        center.y + (radius * 0.25) * val_angle.sin(),
    );
    let p_outer = Pos2::new(
        center.x + (radius * 0.84) * val_angle.cos(),
        center.y + (radius * 0.84) * val_angle.sin(),
    );
    painter.line_segment(
        [p_inner, p_outer],
        Stroke::new(2.0_f32, theme::TEXT_PRIMARY),
    );

    // Uppercase micro label above knob
    if !label.is_empty() {
        painter.text(
            Pos2::new(rect.center().x, rect.top() + 1.0),
            Align2::CENTER_TOP,
            label,
            FontId::proportional(10.0),
            theme::TEXT_SECONDARY,
        );
    }

    // Compact monospace value readout below knob
    let val_str = theme::format_compact_value(current, unit);
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 1.0),
        Align2::CENTER_BOTTOM,
        val_str,
        FontId::monospace(10.5),
        if response.dragged() {
            accent
        } else {
            theme::TEXT_PRIMARY
        },
    );

    let full_tip = if tooltip.is_empty() {
        format!(
            "{} · {} ({} .. {})\nDrag vertically · Shift+drag fine · Double-click reset",
            label,
            unit.format_value(current),
            unit.format_value(min),
            unit.format_value(max)
        )
    } else {
        format!(
            "{tooltip}\n{} ({} .. {})\nDrag vertically · Shift+drag fine · Double-click reset",
            unit.format_value(current),
            unit.format_value(min),
            unit.format_value(max)
        )
    };
    let clicked = response.clicked();
    let right_clicked = response.secondary_clicked();
    response.on_hover_text(full_tip);

    ControlOutcome {
        committed,
        live_value: current,
        clicked,
        right_clicked,
    }
}

fn paint_arc(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    stroke: Stroke,
) {
    let span = (end_angle - start_angle).abs();
    let steps = ((span / 0.06).ceil() as usize).clamp(8, 64);
    let points = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let a = start_angle + t * (end_angle - start_angle);
            Pos2::new(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect::<Vec<_>>();
    painter.add(egui::Shape::line(points, stroke));
}

// ── Console Vertical Fader ──────────────────────────────────────────────────

/// Console fader mapping: expands musical precision around `0 dB` (`+12..-60 dB`).
pub(super) fn fader_db_to_norm(db: f64, min_db: f64, max_db: f64) -> f32 {
    let v = db.clamp(min_db, max_db);
    if max_db <= min_db {
        return 0.5;
    }
    // Map [min_db, max_db] with curved console taper so 0 dB sits around 75% up.
    let lin = ((v - min_db) / (max_db - min_db)).clamp(0.0, 1.0);
    lin.powf(1.65) as f32
}

/// Inverse console fader mapping back to dB.
pub(super) fn fader_norm_to_db(norm: f32, min_db: f64, max_db: f64) -> f64 {
    let n = f64::from(norm.clamp(0.0, 1.0));
    min_db + n.powf(1.0 / 1.65) * (max_db - min_db)
}

/// Draws a long-throw vertical studio console fader with dedicated left dB scale gutter
/// and beveled machined cap that never overlaps scale labels.
#[allow(clippy::too_many_arguments)]
pub(super) fn vertical_fader(
    ui: &mut Ui,
    state: &mut UiState,
    drag_key: &str,
    db: f64,
    min_db: f64,
    max_db: f64,
    accent: Color32,
    width: f32,
    height: f32,
) -> ControlOutcome {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
    let mut current = state.drag_preview(drag_key).unwrap_or(db);
    let mut committed = None;

    let cap_h = 16.0;
    let track_top = rect.top() + cap_h * 0.5 + 2.0;
    let track_bot = rect.bottom() - cap_h * 0.5 - 2.0;
    let track_span = (track_bot - track_top).max(1.0);

    if response.drag_started() {
        state.set_drag_preview(drag_key, current);
    }
    if response.dragged() {
        let fine = ui.input(|input| input.modifiers.shift);
        if fine {
            let dy = -ui.input(|input| input.pointer.delta().y);
            let norm = (fader_db_to_norm(current, min_db, max_db) + dy / (track_span * 4.5))
                .clamp(0.0, 1.0);
            current = fader_norm_to_db(norm, min_db, max_db);
        } else if let Some(pos) = response.interact_pointer_pos() {
            let norm = ((track_bot - pos.y) / track_span).clamp(0.0, 1.0);
            current = fader_norm_to_db(norm, min_db, max_db);
        }
        state.set_drag_preview(drag_key, current);
    }
    if response.drag_stopped()
        && let Some(final_val) = state.take_drag_preview(drag_key)
    {
        current = final_val;
        if (final_val - db).abs() > 0.01 {
            committed = Some((final_val * 10.0).round() / 10.0);
        }
    }
    if response.double_clicked() {
        current = 0.0_f64.clamp(min_db, max_db);
        state.hold_preview(drag_key, current);
        committed = Some(current);
    }

    let painter = ui.painter();
    let label_gutter = if width >= 46.0 { 22.0 } else { 0.0 };
    let track_area_w = (width - label_gutter).max(20.0);
    let cx = rect.left() + label_gutter + track_area_w * 0.5;
    let cap_w = (track_area_w - 6.0).clamp(16.0, 24.0);
    let tick_right = cx - cap_w * 0.5 - 2.0;

    // dB scale ticks and labels cleanly inside the dedicated left gutter
    for &tick_db in &[12.0, 6.0, 0.0, -6.0, -12.0, -24.0, -48.0] {
        if tick_db < min_db || tick_db > max_db {
            continue;
        }
        let n = fader_db_to_norm(tick_db, min_db, max_db);
        let y = track_bot - n * track_span;
        let is_unity = tick_db.abs() < 0.01;
        // Unity stands out by weight and brightness, not by a warning colour.
        let tick_color = if is_unity {
            theme::TEXT_SECONDARY
        } else {
            theme::BORDER_STRONG
        };
        let tick_len = 4.0;
        painter.hline(
            (tick_right - tick_len)..=tick_right,
            y,
            Stroke::new(if is_unity { 1.4_f32 } else { 1.0_f32 }, tick_color),
        );
        // Every tick carries its value, so the scale never shows unexplained dashes.
        if label_gutter > 0.0 {
            painter.text(
                Pos2::new(tick_right - tick_len - 2.0, y),
                Align2::RIGHT_CENTER,
                if is_unity {
                    "0".to_owned()
                } else {
                    format!("{tick_db:.0}")
                },
                FontId::monospace(8.5),
                if is_unity {
                    theme::TEXT_SECONDARY
                } else {
                    theme::TEXT_MUTED
                },
            );
        }
    }

    // Recessed fader slot
    let slot = Rect::from_min_max(
        Pos2::new(cx - 2.5, track_top),
        Pos2::new(cx + 2.5, track_bot),
    );
    painter.rect_filled(slot, 2.0, theme::BG_WELL);
    painter.rect_stroke(
        slot,
        2.0,
        Stroke::new(1.0_f32, theme::SEAM_DARK),
        StrokeKind::Inside,
    );

    // Subtle colored fill below fader cap
    let norm = fader_db_to_norm(current, min_db, max_db);
    let cap_y = track_bot - norm * track_span;
    if cap_y < track_bot - 1.0 {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(cx - 1.5, cap_y),
                Pos2::new(cx + 1.5, track_bot - 1.0),
            ),
            1.0,
            with_alpha(accent, 0.55),
        );
    }

    // Machined studio fader cap with upper/lower bevel shading
    let cap = Rect::from_center_size(Pos2::new(cx, cap_y), Vec2::new(cap_w, cap_h));
    painter.rect_filled(
        cap.translate(Vec2::new(0.0, 1.5)),
        2.5,
        with_alpha(theme::SEAM_DARK, 0.65),
    );
    let cap_base = if response.dragged() {
        Color32::from_rgb(138, 148, 166)
    } else if response.hovered() {
        Color32::from_rgb(112, 122, 138)
    } else {
        Color32::from_rgb(86, 94, 110)
    };
    painter.rect_filled(cap, 2.5, cap_base);
    // Upper light bevel face
    painter.rect_filled(
        Rect::from_min_max(cap.left_top(), Pos2::new(cap.right(), cap.center().y)),
        CornerRadius {
            nw: 2,
            ne: 2,
            sw: 0,
            se: 0,
        },
        with_alpha(theme::TEXT_PRIMARY, 0.12),
    );
    painter.rect_stroke(
        cap,
        2.5,
        Stroke::new(
            1.0_f32,
            if response.dragged() {
                accent
            } else {
                theme::SEAM_DARK
            },
        ),
        StrokeKind::Inside,
    );
    // Top specular edge & center dark indicator groove on cap
    painter.hline(
        (cap.left() + 2.0)..=(cap.right() - 2.0),
        cap.top() + 1.5,
        Stroke::new(1.0_f32, with_alpha(theme::TEXT_PRIMARY, 0.38)),
    );
    painter.hline(
        (cap.left() + 2.0)..=(cap.right() - 2.0),
        cap.center().y,
        Stroke::new(2.0_f32, theme::BG_WELL),
    );

    let clicked = response.clicked();
    let right_clicked = response.secondary_clicked();
    response.on_hover_text(format!(
        "Level: {:+.1} dB\nDrag to ride fader · Shift fine · Double-click 0.0 dB · Right-click exact",
        current
    ));

    ControlOutcome {
        committed,
        live_value: current,
        clicked,
        right_clicked,
    }
}

// ── Inline Horizontal Mini Slider ───────────────────────────────────────────

/// Compact inline horizontal draggable bar for Track Headers, Mixer Sends, and Parameter Decks.
#[allow(clippy::too_many_arguments)]
pub(super) fn horizontal_mini_slider(
    ui: &mut Ui,
    state: &mut UiState,
    drag_key: &str,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    default: f64,
    unit: Unit,
    accent: Color32,
    width: f32,
    height: f32,
) -> ControlOutcome {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
    let mut current = state.drag_preview(drag_key).unwrap_or(value);
    let mut committed = None;

    if response.drag_started() {
        state.set_drag_preview(drag_key, current);
    }
    if response.dragged() {
        let dx = ui.input(|input| input.pointer.delta().x);
        if dx.abs() > 0.0 {
            let fine = ui.input(|input| input.modifiers.shift);
            let scale = if fine { 0.0012 } else { 1.0 / width.max(40.0) };
            let norm = (theme::value_to_norm(current, min, max, unit) + dx * scale).clamp(0.0, 1.0);
            current = theme::norm_to_value(norm, min, max, unit);
            state.set_drag_preview(drag_key, current);
        }
    }
    if response.drag_stopped()
        && let Some(final_val) = state.take_drag_preview(drag_key)
    {
        current = final_val;
        if (final_val - value).abs() > 1e-5 {
            committed = Some(final_val);
        }
    }
    if response.double_clicked() {
        current = default.clamp(min, max);
        state.hold_preview(drag_key, current);
        committed = Some(current);
    }

    let painter = ui.painter();
    // The readout owns an opaque gutter on the right, sized to the widest value this control can
    // show so it never jitters while dragging. The fill lives only in the rail to its left, so no
    // fill boundary can ever split the digits.
    let value_font = FontId::monospace(10.0);
    // Sampled across the whole range rather than including the live value, so an intermediate
    // value wider than either end (`-59.4 dB`) cannot widen the gutter mid-drag.
    let gutter_w = (0..=16)
        .map(|step: u8| theme::norm_to_value(f32::from(step) / 16.0, min, max, unit))
        .chain([min, max, default])
        .map(|v| {
            painter
                .layout_no_wrap(
                    theme::format_compact_value(v, unit),
                    value_font.clone(),
                    theme::TEXT_PRIMARY,
                )
                .size()
                .x
        })
        .fold(0.0_f32, f32::max)
        + 10.0;
    // One minimum value box for every mini slider, so neighbouring rows line up.
    let gutter_w = gutter_w.max(64.0).min(rect.width() * 0.6);
    let rail = Rect::from_min_max(rect.min, Pos2::new(rect.right() - gutter_w, rect.bottom()));
    let gutter = Rect::from_min_max(Pos2::new(rail.right(), rect.top()), rect.max);

    painter.rect_filled(rect, 3.0, theme::BG_WELL);
    let norm = theme::value_to_norm(current, min, max, unit);
    let fill_w = (rail.width() * norm).clamp(0.0, rail.width());
    // Pan and symmetric signed amounts (-x..x) fill out from their centre, so centred reads
    // calm and both directions read alike. Gain is never bipolar: -60..+6 dB fills from the left.
    let bipolar = fills_from_centre(unit, min, max);
    if bipolar {
        let zero_x = rail.left() + rail.width() * theme::value_to_norm(0.0, min, max, unit);
        let value_x = rail.left() + fill_w;
        let (left, right) = (zero_x.min(value_x), zero_x.max(value_x));
        if right - left > 0.5 {
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(left, rail.top()), Pos2::new(right, rail.bottom())),
                0.0,
                with_alpha(accent, 0.36),
            );
        }
        painter.vline(
            zero_x,
            (rail.top() + 2.0)..=(rail.bottom() - 2.0),
            Stroke::new(1.0_f32, theme::BORDER_STRONG),
        );
    } else if fill_w > 1.0 {
        painter.rect_filled(
            Rect::from_min_max(rail.min, Pos2::new(rail.left() + fill_w, rail.bottom())),
            CornerRadius {
                nw: 3,
                sw: 3,
                ne: 0,
                se: 0,
            },
            with_alpha(accent, 0.36),
        );
    }
    painter.vline(
        gutter.left(),
        (rect.top() + 1.0)..=(rect.bottom() - 1.0),
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
    );
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(
            1.0_f32,
            if response.hovered() || response.dragged() {
                accent
            } else {
                theme::BORDER_SUBTLE
            },
        ),
        StrokeKind::Inside,
    );
    painter.text(
        Pos2::new(gutter.right() - 5.0, gutter.center().y),
        Align2::RIGHT_CENTER,
        theme::format_compact_value(current, unit),
        value_font,
        theme::TEXT_PRIMARY,
    );
    // A narrow bipolar rail (a pan control) keeps its label in the tooltip: a letter over a
    // centre-out fill reads badly.
    if !label.is_empty() && !(unit == Unit::Pan && rail.width() < 48.0) {
        let label_clip = rail.shrink2(Vec2::new(4.0, 0.0));
        if label_clip.width() > 8.0 {
            let fitted = ellipsize(
                painter,
                label,
                &FontId::proportional(10.0),
                label_clip.width() - 2.0,
            );
            painter.with_clip_rect(label_clip).text(
                Pos2::new(rail.left() + 5.0, rail.center().y),
                Align2::LEFT_CENTER,
                fitted,
                FontId::proportional(10.0),
                theme::TEXT_PRIMARY,
            );
        }
    }

    let clicked = response.clicked();
    let right_clicked = response.secondary_clicked();
    response.on_hover_text(format!(
        "{label}: {}\nDrag horizontally · Shift fine · Double-click reset",
        unit.format_value(current)
    ));

    ControlOutcome {
        committed,
        live_value: current,
        clicked,
        right_clicked,
    }
}

// ── Hardware LED Switches & Segmented Pills ─────────────────────────────────

/// Crisp studio hardware LED toggle switch (`M`, `S`, `C`, `A`, etc.).
/// Whether a slider fills out from its centre: pan and symmetric signed amounts (`-x..x`), never
/// gain, whose `-60..+6 dB` range reads as a level filling from the left.
pub(super) fn fills_from_centre(unit: Unit, min: f64, max: f64) -> bool {
    unit == Unit::Pan
        || (unit != Unit::Decibels && min < 0.0 && (min + max).abs() <= 1e-9 * max.abs().max(1.0))
}

/// Keys for optimistic preview values. Every control that shows the same durable value uses the
/// same key, so dragging one moves the others at once and a committed value shows everywhere
/// until its edit reloads, instead of each control catching up separately.
pub(super) mod preview_key {
    pub(in crate::workspace::gui) fn instrument_parameter(
        instrument: &str,
        parameter: &str,
    ) -> String {
        format!("param:{instrument}:{parameter}")
    }

    pub(in crate::workspace::gui) fn processor_setting(processor: &str, setting: &str) -> String {
        format!("proc:{processor}:{setting}")
    }

    /// `identity` is the channel's mixer identity: `track/ID`, `group/ID`, `return/ID`, `master`.
    pub(in crate::workspace::gui) fn channel_level(identity: &str) -> String {
        format!("level:{identity}")
    }

    pub(in crate::workspace::gui) fn channel_pan(identity: &str) -> String {
        format!("pan:{identity}")
    }

    pub(in crate::workspace::gui) fn channel_mute(identity: &str) -> String {
        format!("mute:{identity}")
    }

    pub(in crate::workspace::gui) fn processor_bypass(processor: &str) -> String {
        format!("bypass:{processor}")
    }
}

/// A left-aligned label in a slot of exactly `width`, truncated with a tooltip when it overflows,
/// so changing text never moves the widgets after it.
pub(super) fn fixed_label(ui: &mut Ui, width: f32, text: RichText) -> egui::Response {
    ui.allocate_ui_with_layout(
        Vec2::new(width, ui.spacing().interact_size.y),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.add(egui::Label::new(text).truncate())
        },
    )
    .inner
}

/// The one Publish control style: neutral, with an amber outline and text only while a valid
/// Candidate waits to be published. Filled amber is kept for the publish dialog itself.
pub(super) fn publish_button(ui: &mut Ui, label: &str, needs_publish: bool) -> egui::Response {
    let (text, stroke) = if needs_publish {
        (
            theme::ACCENT_AMBER,
            Stroke::new(1.0_f32, theme::ACCENT_AMBER),
        )
    } else {
        (
            theme::TEXT_SECONDARY,
            Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        )
    };
    ui.add(
        egui::Button::new(RichText::new(label).size(10.5).strong().color(text))
            .fill(theme::BG_CARD)
            .stroke(stroke),
    )
}

/// Selection of a tab-like control that should take effect on mouse-down.
pub(super) trait TabPress {
    /// Whether the control was pressed or clicked this frame. Tabs select on press, as a
    /// DAW's do: a quick click whose pointer is still travelling would otherwise read as a
    /// drag and be dropped. Only use it where selecting twice is harmless.
    fn tab_pressed(&self) -> bool;
}

impl TabPress for egui::Response {
    fn tab_pressed(&self) -> bool {
        self.clicked()
            || (self.is_pointer_button_down_on()
                && self.ctx.input(|input| input.pointer.primary_pressed()))
    }
}

pub(super) fn led_button(
    ui: &mut Ui,
    label: &str,
    active: bool,
    lit_color: Color32,
    size: Vec2,
    tooltip: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let (bg, border, text_color) = if !ui.is_enabled() {
        (theme::BG_CARD, theme::BORDER_SUBTLE, theme::TEXT_MUTED)
    } else if active {
        (
            with_alpha(lit_color, 0.28),
            lit_color,
            if lit_color == theme::SIG_HOT {
                theme::SIG_HOT
            } else {
                theme::TEXT_PRIMARY
            },
        )
    } else if response.hovered() {
        (theme::BG_HOVER, theme::BORDER_STRONG, theme::TEXT_PRIMARY)
    } else {
        // Unlit but available: secondary text, so it never reads as a disabled control.
        (theme::BG_CARD, theme::BORDER_SUBTLE, theme::TEXT_SECONDARY)
    };
    painter.rect_filled(rect, 3.0, bg);
    painter.rect_stroke(rect, 3.0, Stroke::new(1.0_f32, border), StrokeKind::Inside);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.5),
        text_color,
    );
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(legible(tooltip))
    }
}

/// `text` shortened with a trailing ellipsis so it fits `max_width` in `font`.
pub(super) fn ellipsize(
    painter: &egui::Painter,
    text: &str,
    font: &FontId,
    max_width: f32,
) -> String {
    let width = |t: &str| {
        painter
            .layout_no_wrap(t.to_owned(), font.clone(), theme::TEXT_PRIMARY)
            .size()
            .x
    };
    if width(text) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (1..chars.len()).rev() {
        let candidate: String = chars[..keep].iter().collect::<String>() + "…";
        if width(&candidate) <= max_width {
            return candidate;
        }
    }
    "…".to_owned()
}

/// The one section-header style: a short accent bar and a muted uppercase title. Color lives
/// only in the bar, so headers read as one family across every view.
pub(super) fn section_header(ui: &mut Ui, title: &str, accent: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 7.0;
        let (bar, _) = ui.allocate_exact_size(Vec2::new(3.0, 12.0), Sense::hover());
        ui.painter().rect_filled(bar, 1.0, accent);
        ui.label(
            RichText::new(title)
                .strong()
                .size(10.5)
                .color(theme::TEXT_SECONDARY),
        );
    });
}

/// Sleek studio pill toggle for primary workspace modes and sub-view selectors.
/// A tab or toggle. Every one shares the same active treatment, a raised neutral tab with a
/// slim selection-coloured underline, so tabs never compete with real attention states.
pub(super) fn pill_toggle(ui: &mut Ui, label: &str, active: bool, tooltip: &str) -> egui::Response {
    let font = FontId::proportional(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), theme::TEXT_PRIMARY);
    let size = Vec2::new((galley.size().x + 18.0).max(46.0), 24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    // Active reads as a raised neutral tab with a slim accent underline, not a coloured box,
    // so the accent stays a small cue instead of competing with real attention states.
    let (bg, border, fg) = if !ui.is_enabled() {
        (
            Color32::TRANSPARENT,
            Color32::TRANSPARENT,
            theme::TEXT_MUTED,
        )
    } else if active {
        (theme::BG_ACTIVE, theme::BORDER_STRONG, theme::TEXT_PRIMARY)
    } else if response.hovered() {
        (theme::BG_HOVER, theme::BORDER_STRONG, theme::TEXT_PRIMARY)
    } else {
        (
            Color32::TRANSPARENT,
            Color32::TRANSPARENT,
            theme::TEXT_SECONDARY,
        )
    };
    painter.rect_filled(rect, 4.0, bg);
    if border != Color32::TRANSPARENT {
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, border), StrokeKind::Inside);
    }
    if active {
        let underline = Rect::from_min_max(
            Pos2::new(rect.left() + 6.0, rect.bottom() - 3.0),
            Pos2::new(rect.right() - 6.0, rect.bottom() - 1.0),
        );
        painter.rect_filled(underline, 1.0, theme::SELECTION);
    }
    painter.text(rect.center(), Align2::CENTER_CENTER, label, font, fg);
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(legible(tooltip))
    }
}

/// Compact studio icon button with custom painter callback.
pub(super) fn icon_button(
    ui: &mut Ui,
    size: Vec2,
    active: bool,
    accent: Color32,
    tooltip: &str,
    paint_icon: impl FnOnce(&egui::Painter, Rect, Color32),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let (bg, border, icon_color) = if active {
        (with_alpha(accent, 0.25), accent, accent)
    } else if response.hovered() {
        (theme::BG_HOVER, theme::BORDER_STRONG, theme::TEXT_PRIMARY)
    } else {
        (theme::BG_CARD, theme::BORDER_SUBTLE, theme::TEXT_SECONDARY)
    };
    painter.rect_filled(rect, 4.0, bg);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, border), StrokeKind::Inside);
    paint_icon(painter, rect.shrink(3.0), icon_color);
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(legible(tooltip))
    }
}

// ── Shared Tables & Utility Widgets ─────────────────────────────────────────

/// One table cell.
pub(super) enum Cell {
    Text(String),
    Colored(String, Color32),
    /// A level from zero to one, drawn as a logarithmic dBFS bar.
    Meter(f32),
}

impl From<String> for Cell {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for Cell {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

/// A pointer click on a table row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowClick {
    Single(usize),
    Double(usize),
}

/// A selectable, virtually scrolled studio table.
pub(super) fn table(
    ui: &mut Ui,
    state: &mut UiState,
    id: &'static str,
    columns: &[(&str, f32)],
    rows: usize,
    selected: Option<usize>,
    mut row: impl FnMut(usize) -> Vec<Cell>,
) -> Option<RowClick> {
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
    let mut builder = TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .resizable(true)
        .sense(Sense::click())
        .auto_shrink([false, false])
        .cell_layout(Layout::left_to_right(Align::Center));
    for (index, (_, width)) in columns.iter().enumerate() {
        builder = builder.column(if index + 1 == columns.len() {
            Column::remainder().clip(true)
        } else {
            Column::initial(*width).at_least(28.0).clip(true)
        });
    }
    if let Some(selected) = selected.filter(|selected| *selected < rows)
        && state.scrolled.insert(id, selected) != Some(selected)
    {
        builder = builder.scroll_to_row(selected, None);
    }
    let mut click = None;
    builder
        .header(row_height, |mut header| {
            for (title, _) in columns {
                header.col(|ui| {
                    ui.label(
                        RichText::new(title.to_uppercase())
                            .size(10.5)
                            .strong()
                            .color(theme::TEXT_SECONDARY),
                    );
                });
            }
        })
        .body(|body| {
            body.rows(row_height, rows, |mut table_row| {
                let index = table_row.index();
                table_row.set_selected(selected == Some(index));
                for cell in row(index) {
                    table_row.col(|ui| match cell {
                        Cell::Text(text) => {
                            ui.label(RichText::new(text).color(theme::TEXT_PRIMARY));
                        }
                        Cell::Colored(text, color) => {
                            ui.label(RichText::new(text).color(color));
                        }
                        Cell::Meter(level) => {
                            dbfs_meter_horizontal(
                                ui,
                                level,
                                level * 0.7,
                                ui.available_width().min(148.0),
                                true,
                            );
                        }
                    });
                }
                let response = table_row.response();
                if response.double_clicked() {
                    click = Some(RowClick::Double(index));
                } else if response.clicked() {
                    click = Some(RowClick::Single(index));
                }
            });
        });
    click
}

/// Applies a row click: select the row, and open its exact edit on a double click.
pub(super) fn apply_row_click(cx: &mut Cx<'_>, click: Option<RowClick>, current: usize) {
    match click {
        Some(RowClick::Single(index)) => cx.select(current, index),
        Some(RowClick::Double(index)) => {
            cx.select(current, index);
            cx.send(Action::OpenExactEdit);
        }
        None => {}
    }
}

/// A row of compact studio toolbar buttons.
pub(super) fn toolbar(ui: &mut Ui, cx: &mut Cx<'_>, buttons: &[(&str, Action, &str)]) {
    for (label, action, hint) in buttons {
        if ui.button(*label).on_hover_text(legible(hint)).clicked() {
            cx.send(action.clone());
        }
    }
}

/// A drop-down over a view's secondary context (Pattern, Instrument, Automation lane).
pub(super) fn context_combo(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    label: &str,
    names: &[String],
    current: usize,
    step: impl Fn(isize) -> Action,
) {
    if !label.is_empty() {
        ui.label(
            RichText::new(label.to_uppercase())
                .size(10.0)
                .strong()
                .color(theme::TEXT_MUTED),
        );
    }
    // A fixed, truncating width: a long name never pushes the controls after the picker.
    egui::ComboBox::from_id_salt(format!("ctx-combo-{label}"))
        .width(156.0)
        .truncate()
        .selected_text(
            RichText::new(names.get(current).map_or("—", String::as_str))
                .color(theme::TEXT_PRIMARY),
        )
        .show_ui(ui, |ui| {
            for (index, name) in names.iter().enumerate() {
                if ui.selectable_label(index == current, name).clicked() && index != current {
                    cx.send(step(index as isize - current as isize));
                }
            }
        });
}

/// A clean bounded empty-state card centered in the workspace without stretching full-screen.
pub(super) fn unavailable(ui: &mut Ui, message: &str) {
    let top_pad = (ui.available_height() * 0.28).clamp(24.0, 180.0);
    ui.add_space(top_pad);
    ui.vertical_centered(|ui| {
        egui::Frame::new()
            .fill(theme::BG_SURFACE)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::symmetric(28, 18))
            .show(ui, |ui| {
                ui.set_max_width(460.0);
                ui.label(
                    RichText::new(message)
                        .color(theme::TEXT_SECONDARY)
                        .size(13.0),
                );
            });
    });
}

// ── Staggered 3-Octave Studio Piano Keyboard & MPC Drum Pads ────────────────

/// Authentic staggered studio piano keyboard (melodic) or tactile MPC pad bank (drums/sampler).
pub(super) fn note_strip(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let drums = match app.view {
        View::EventGrid => app
            .bundle
            .project
            .as_ref()
            .and_then(|p| p.song.patterns.get(app.selected_pattern))
            .is_some_and(|pat| {
                pat.events
                    .iter()
                    .any(|e| matches!(e, crate::model::PatternEvent::Hit { .. }))
            }),
        _ => app.selected_instrument_is_drums(),
    };
    let octave = cx.state.keyboard_octave;
    let latch = cx.state.keyboard_latch;

    ui.add_space(3.0);
    ui.horizontal(|ui| {
        // Left compact control cluster
        egui::Frame::new()
            .fill(theme::BG_WELL)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if drums { "DRUM PADS" } else { "KEYS" })
                                .size(9.5)
                                .strong()
                                .color(theme::TEXT_MUTED),
                        );
                        if !drums {
                            if led_button(
                                ui,
                                "−",
                                false,
                                theme::ACCENT_CYAN,
                                Vec2::new(18.0, 16.0),
                                "Octave down",
                            )
                            .clicked()
                            {
                                cx.state.keyboard_octave =
                                    (cx.state.keyboard_octave - 1).clamp(1, 6);
                            }
                            ui.label(
                                RichText::new(format!("C{octave}"))
                                    .monospace()
                                    .size(10.5)
                                    .color(theme::TEXT_PRIMARY),
                            );
                            if led_button(
                                ui,
                                "+",
                                false,
                                theme::ACCENT_CYAN,
                                Vec2::new(18.0, 16.0),
                                "Octave up",
                            )
                            .clicked()
                            {
                                cx.state.keyboard_octave =
                                    (cx.state.keyboard_octave + 1).clamp(1, 6);
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        if led_button(
                            ui,
                            "HOLD",
                            latch,
                            theme::ACCENT_AMBER,
                            Vec2::new(40.0, 18.0),
                            "Latch clicked note while adjusting knobs",
                        )
                        .clicked()
                        {
                            cx.state.keyboard_latch = !cx.state.keyboard_latch;
                            if !cx.state.keyboard_latch
                                && let Some(key) = cx.state.piano_key.take()
                            {
                                cx.send(Action::NoteOff { key });
                            }
                        }
                        // The voice count lives in the transport deck only.
                        if app.note_mode {
                            ui.label(
                                RichText::new("LIVE")
                                    .size(9.5)
                                    .strong()
                                    .color(theme::ACCENT_AMBER),
                            );
                        }
                    });
                });
            });

        ui.add_space(4.0);

        if drums {
            drum_pad_bank(ui, cx);
        } else {
            staggered_piano(ui, cx);
        }
    });
    ui.add_space(3.0);
}

fn drum_pad_bank(ui: &mut Ui, cx: &mut Cx<'_>) {
    const FALLBACK_DRUMS: [&str; 8] = [
        "Kick",
        "Snare",
        "Closed Hat",
        "Clap",
        "Open Hat",
        "Rim",
        "Low Tom",
        "Crash",
    ];
    let app = cx.app;
    let instrument = app
        .bundle
        .project
        .as_ref()
        .and_then(|p| p.song.instruments.get(app.selected_instrument));

    // Build dynamic pad list from sampler mappings if present, otherwise full 8-pad studio drum bank.
    let mut pads: Vec<(char, u8, String)> = Vec::new();
    if let Some(inst) = instrument
        && let Some(sampler) = &inst.sampler
        && !sampler.mappings.is_empty()
    {
        for (idx, mapping) in sampler.mappings.iter().take(8).enumerate() {
            let key = DRUM_NOTES
                .get(idx)
                .map(|(k, _)| *k)
                .unwrap_or_else(|| (b'1' + idx as u8) as char);
            let pitch = mapping.root_key;
            pads.push((key, pitch, mapping.id.clone()));
        }
    }
    if pads.is_empty() {
        const EXTRA_KEYS: [(char, u8); 4] = [('g', 46), ('h', 37), ('j', 45), ('k', 49)];
        for (idx, name) in FALLBACK_DRUMS.iter().enumerate() {
            let (key, pitch) = DRUM_NOTES
                .get(idx)
                .copied()
                .unwrap_or_else(|| EXTRA_KEYS[(idx - DRUM_NOTES.len()).min(3)]);
            pads.push((key, pitch, (*name).to_owned()));
        }
    }

    let avail_w = ui.available_width().max(240.0);
    let pad_count = pads.len().max(1);
    let gap = 6.0_f32;
    let pad_w = ((avail_w - (pad_count as f32 - 1.0) * gap) / pad_count as f32).max(68.0);
    let pad_h = 46.0;
    let mut pressed: Option<(char, u8, f32)> = None;

    ui.spacing_mut().item_spacing.x = gap;
    for (idx, (key, pitch, name)) in pads.iter().enumerate() {
        let held = app.held_notes.contains_key(key) || cx.state.piano_key == Some(*key);
        let accent = theme::track_color(idx);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(pad_w, pad_h), Sense::click_and_drag());
        let painter = ui.painter();
        let fill = if held {
            with_alpha(accent, 0.35)
        } else if response.hovered() {
            theme::BG_HOVER
        } else {
            theme::BG_CARD
        };
        painter.rect_filled(rect, 5.0, fill);
        painter.rect_stroke(
            rect,
            5.0,
            Stroke::new(
                if held { 1.8_f32 } else { 1.0_f32 },
                if held { accent } else { theme::BORDER_STRONG },
            ),
            StrokeKind::Inside,
        );
        // Top color bar inside pad
        painter.rect_filled(
            Rect::from_min_max(
                rect.left_top() + Vec2::new(2.0, 2.0),
                Pos2::new(rect.right() - 2.0, rect.top() + 5.0),
            ),
            1.5,
            accent,
        );
        painter.with_clip_rect(rect.shrink(4.0)).text(
            Pos2::new(rect.left() + 8.0, rect.top() + 12.0),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(11.0),
            theme::TEXT_PRIMARY,
        );
        painter.text(
            Pos2::new(rect.left() + 8.0, rect.bottom() - 6.0),
            Align2::LEFT_BOTTOM,
            format!("P{:02}", idx + 1),
            FontId::monospace(9.0),
            theme::TEXT_MUTED,
        );
        painter.text(
            Pos2::new(rect.right() - 7.0, rect.bottom() - 6.0),
            Align2::RIGHT_BOTTOM,
            key.to_ascii_uppercase().to_string(),
            FontId::monospace(10.0),
            theme::TEXT_SECONDARY,
        );
        if response.is_pointer_button_down_on() {
            let vel = response.interact_pointer_pos().map_or(0.85, |p| {
                ((p.y - rect.top()) / rect.height()).clamp(0.25, 1.0)
            });
            pressed = Some((*key, *pitch, vel));
        }
    }
    apply_audition_press(cx, pressed);
}

fn staggered_piano(ui: &mut Ui, cx: &mut Cx<'_>) {
    // 3 full octaves + closing C = 37 semitones (22 white keys, 15 black keys) spanning full dock
    const WHITE_SEMITONES: [u8; 22] = [
        0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23, 24, 26, 28, 29, 31, 33, 35, 36,
    ];
    // Black keys: (white_slot_index_to_left, semitone_offset)
    const BLACK_KEYS: [(usize, u8); 15] = [
        (0, 1),
        (1, 3),
        (3, 6),
        (4, 8),
        (5, 10),
        (7, 13),
        (8, 15),
        (10, 18),
        (11, 20),
        (12, 22),
        (14, 25),
        (15, 27),
        (17, 30),
        (18, 32),
        (19, 34),
    ];

    let base_pitch: u8 = ((cx.state.keyboard_octave + 1) * 12).clamp(24, 72) as u8;
    let avail_w = ui.available_width().max(300.0);
    let key_h = 48.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(avail_w, key_h), Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 4.0, theme::BG_WELL);
    let white_w = (rect.width() - 2.0) / WHITE_SEMITONES.len() as f32;
    let black_w = white_w * 0.62;
    let black_h = key_h * 0.60;

    let key_for_semitone = |semitone: u8| -> char {
        KEYBOARD_NOTES
            .get(semitone as usize)
            .map(|(k, _)| *k)
            .unwrap_or_else(|| char::from(b'a' + (semitone % 26)))
    };

    let pointer = if response.is_pointer_button_down_on() {
        response.interact_pointer_pos()
    } else {
        None
    };

    // Hit-test black keys first (they sit visually on top)
    let mut pressed: Option<(char, u8, f32)> = None;
    if let Some(pos) = pointer {
        for &(slot, semi) in &BLACK_KEYS {
            let x = rect.left() + 1.0 + (slot as f32 + 1.0) * white_w - black_w * 0.5;
            let krect =
                Rect::from_min_size(Pos2::new(x, rect.top() + 1.0), Vec2::new(black_w, black_h));
            if krect.contains(pos) {
                let vel = ((pos.y - krect.top()) / krect.height()).clamp(0.25, 1.0);
                pressed = Some((key_for_semitone(semi), base_pitch.saturating_add(semi), vel));
                break;
            }
        }
        if pressed.is_none() {
            for (slot, &semi) in WHITE_SEMITONES.iter().enumerate() {
                let x = rect.left() + 1.0 + slot as f32 * white_w;
                let krect = Rect::from_min_size(
                    Pos2::new(x, rect.top() + 1.0),
                    Vec2::new(white_w, key_h - 2.0),
                );
                if krect.contains(pos) {
                    let vel = ((pos.y - krect.top()) / krect.height()).clamp(0.25, 1.0);
                    pressed = Some((key_for_semitone(semi), base_pitch.saturating_add(semi), vel));
                    break;
                }
            }
        }
    }

    let app = cx.app;
    // Paint white keys (muted warm ivory so they don't blind the dark studio workspace)
    for (slot, &semi) in WHITE_SEMITONES.iter().enumerate() {
        let pitch = base_pitch.saturating_add(semi);
        let key_ch = key_for_semitone(semi);
        let is_held = app.held_notes.contains_key(&key_ch)
            || pressed.is_some_and(|(_, p, _)| p == pitch)
            || (cx.state.keyboard_latch && cx.state.piano_key == Some(key_ch));
        let x = rect.left() + 1.0 + slot as f32 * white_w;
        let krect = Rect::from_min_size(
            Pos2::new(x + 0.5, rect.top() + 1.0),
            Vec2::new((white_w - 1.5).max(4.0), key_h - 2.0),
        );
        let fill = if is_held {
            theme::ACCENT_AMBER
        } else {
            Color32::from_rgb(150, 158, 172)
        };
        painter.rect_filled(
            krect,
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: 3,
                se: 3,
            },
            fill,
        );
        // Subtle bottom lip shadow
        painter.hline(
            krect.x_range(),
            krect.bottom() - 1.0,
            Stroke::new(1.0_f32, with_alpha(theme::SEAM_DARK, 0.4)),
        );
        // C keys carry their pitch; every mapped key also carries its QWERTY shortcut.
        let mapped = (semi as usize) < KEYBOARD_NOTES.len() && white_w >= 26.0;
        if semi % 12 == 0 {
            painter.text(
                Pos2::new(krect.center().x, krect.bottom() - 3.0),
                Align2::CENTER_BOTTOM,
                crate::model::format_pitch(pitch),
                FontId::monospace(9.5),
                if is_held {
                    theme::BG_WELL
                } else {
                    Color32::from_rgb(46, 52, 64)
                },
            );
            if mapped {
                painter.text(
                    Pos2::new(krect.center().x, krect.bottom() - 16.0),
                    Align2::CENTER_BOTTOM,
                    key_ch.to_ascii_uppercase().to_string(),
                    FontId::monospace(8.5),
                    if is_held {
                        theme::BG_WELL
                    } else {
                        Color32::from_rgb(78, 86, 100)
                    },
                );
            }
        } else if mapped {
            painter.text(
                Pos2::new(krect.center().x, krect.bottom() - 3.0),
                Align2::CENTER_BOTTOM,
                key_ch.to_ascii_uppercase().to_string(),
                FontId::monospace(8.5),
                if is_held {
                    theme::BG_WELL
                } else {
                    Color32::from_rgb(78, 86, 100)
                },
            );
        }
    }

    // Paint overlapping ebony black keys on top
    for &(slot, semi) in &BLACK_KEYS {
        let pitch = base_pitch.saturating_add(semi);
        let key_ch = key_for_semitone(semi);
        let is_held = app.held_notes.contains_key(&key_ch)
            || pressed.is_some_and(|(_, p, _)| p == pitch)
            || (cx.state.keyboard_latch && cx.state.piano_key == Some(key_ch));
        let x = rect.left() + 1.0 + (slot as f32 + 1.0) * white_w - black_w * 0.5;
        let krect =
            Rect::from_min_size(Pos2::new(x, rect.top() + 1.0), Vec2::new(black_w, black_h));
        let fill = if is_held {
            theme::ACCENT_AMBER
        } else {
            Color32::from_rgb(24, 28, 36)
        };
        painter.rect_filled(
            krect,
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: 3,
                se: 3,
            },
            fill,
        );
        painter.rect_stroke(
            krect,
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: 3,
                se: 3,
            },
            Stroke::new(
                1.0_f32,
                if is_held {
                    theme::TEXT_PRIMARY
                } else {
                    theme::SEAM_DARK
                },
            ),
            StrokeKind::Inside,
        );
        if (semi as usize) < KEYBOARD_NOTES.len() && black_w >= 14.0 {
            painter.text(
                Pos2::new(krect.center().x, krect.bottom() - 7.0),
                Align2::CENTER_BOTTOM,
                key_ch.to_ascii_uppercase().to_string(),
                FontId::monospace(8.0),
                if is_held {
                    theme::BG_WELL
                } else {
                    theme::TEXT_MUTED
                },
            );
        }
        // Subtle front bevel on black key
        if !is_held {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(krect.left() + 2.0, krect.bottom() - 5.0),
                    Pos2::new(krect.right() - 2.0, krect.bottom() - 1.5),
                ),
                1.0,
                Color32::from_rgb(44, 50, 62),
            );
        }
    }

    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    apply_audition_press(cx, pressed);
}

fn apply_audition_press(cx: &mut Cx<'_>, pressed: Option<(char, u8, f32)>) {
    let held = cx.state.piano_key;
    if cx.state.keyboard_latch {
        if let Some((key, pitch, vel)) = pressed
            && held != Some(key)
        {
            if let Some(prev) = held {
                cx.send(Action::NoteOff { key: prev });
            }
            cx.send(Action::NoteOnVelocity {
                key,
                pitch,
                velocity: vel,
            });
            cx.state.piano_key = Some(key);
        }
        return;
    }
    if held != pressed.map(|(key, _, _)| key) {
        if let Some(key) = held {
            cx.send(Action::NoteOff { key });
        }
        if let Some((key, pitch, vel)) = pressed {
            cx.send(Action::NoteOnVelocity {
                key,
                pitch,
                velocity: vel,
            });
        }
        cx.state.piano_key = pressed.map(|(key, _, _)| key);
    }
}
