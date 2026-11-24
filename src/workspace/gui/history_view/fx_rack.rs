//! Read-only FX rack for a History version's inserts or instrument post processors.

use super::*;
use crate::model::{Processor, ProcessorKind, Unit, processor_setting_contract};
use crate::processor_semantics::{
    ResolvedProcessorSetting, ResolvedProcessorSettings, ResolvedSettingValue, descriptors,
    resolve_settings,
};

/// Draws the signal chain in order, comparing the same device across versions by ID.
pub(super) fn chain(
    ui: &mut Ui,
    view: &SoundView<'_>,
    processors: &[Processor],
    previous: Option<&[Processor]>,
    other: Option<&[Processor]>,
    changes: &[&ValueChange],
) {
    if processors.is_empty()
        && previous.is_none_or(<[_]>::is_empty)
        && other.is_none_or(<[_]>::is_empty)
    {
        change_chips(ui, changes);
        return;
    }
    let available = ui.available_width();
    ui.with_layout(
        Layout::left_to_right(Align::Min).with_main_wrap(true),
        |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            for (index, processor) in processors.iter().enumerate() {
                device(
                    ui,
                    available,
                    view,
                    processor,
                    slot(previous, &processor.id),
                    slot(other, &processor.id),
                    index,
                );
            }
            let removed = previous
                .into_iter()
                .flatten()
                .filter(|p| slot(Some(processors), &p.id).is_none());
            let compared = other.into_iter().flatten().filter(|p| {
                slot(Some(processors), &p.id).is_none() && slot(previous, &p.id).is_none()
            });
            for (processor, edited) in removed
                .map(|p| (p, true))
                .chain(compared.map(|p| (p, false)))
            {
                let title = theme::processor_title(processor.kind);
                let version = if edited { "Previous" } else { view.other_word };
                let note = format!("{version} {title} · {}", processor.id);
                readout(
                    ui,
                    available.min(220.0),
                    "Empty insert slot",
                    theme::TEXT_MUTED,
                    Some((edited, note)),
                );
            }
        },
    );
    let unpictured = changes
        .iter()
        .copied()
        .filter(|change| {
            let mut path = change.path.iter().map(String::as_str);
            if !path.any(|part| matches!(part, "inserts" | "post_processors")) {
                return true;
            }
            let _ = path.next(); // Processor ID.
            !matches!(path.next(), None | Some("settings" | "bypass" | "kind"))
        })
        .collect::<Vec<_>>();
    change_chips(ui, &unpictured);
}

/// Locates a processor and its signal-order position.
pub(super) fn slot<'a>(chain: Option<&'a [Processor]>, id: &str) -> Option<(usize, &'a Processor)> {
    chain?.iter().enumerate().find(|(_, p)| p.id == id)
}

/// Compares source-backed device state without source locations.
pub(super) fn same_device(a: &Processor, b: &Processor) -> bool {
    a.kind == b.kind
        && a.bypass == b.bypass
        && a.latency_frames == b.latency_frames
        && a.sidechain_inputs == b.sidechain_inputs
        && a.settings.len() == b.settings.len()
        && a.settings.iter().all(|s| {
            b.settings
                .iter()
                .any(|t| s.key == t.key && s.value == t.value)
        })
}

/// Detects value, membership and signal-order changes in a chain.
pub(super) fn same_chain(a: &[Processor], b: &[Processor]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| a.id == b.id && same_device(a, b))
}

/// Standard outline for a History edit or a difference between Then and Now.
pub(super) fn outline(edited: bool, differs: bool) -> Stroke {
    if edited {
        Stroke::new(1.0_f32, theme::SELECTION)
    } else if differs {
        Stroke::new(1.0_f32, theme::TEXT_MUTED)
    } else {
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE)
    }
}

/// Captures the previous differing value, with edits taking precedence over comparisons.
pub(super) fn difference<T: PartialEq>(
    value: T,
    previous: Option<T>,
    other: Option<T>,
    word: &str,
    display: impl FnOnce(T) -> String,
) -> Option<(bool, String)> {
    if let Some(was) = previous.filter(|was| *was != value) {
        Some((true, format!("Was {}", display(was))))
    } else {
        other
            .filter(|was| *was != value)
            .map(|was| (false, format!("{word} {}", display(was))))
    }
}

/// Draws a bounded selector or signal-path row. Full values remain available on hover.
pub(super) fn readout(
    ui: &mut Ui,
    width: f32,
    text: &str,
    color: Color32,
    prior: Option<(bool, String)>,
) {
    let height = if prior.is_some() { 30.0 } else { 16.0 };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme::BG_WELL);
    painter.rect_stroke(
        rect,
        3.0,
        outline(
            prior.as_ref().is_some_and(|(edit, _)| *edit),
            prior.is_some(),
        ),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(Vec2::new(4.0, 1.0));
    line_text(
        ui,
        inner,
        text,
        FontId::proportional(10.0),
        color,
        Align::Min,
    );
    if let Some((edited, note)) = prior {
        line_text(
            ui,
            inner.translate(Vec2::new(0.0, 14.0)),
            &note,
            FontId::proportional(9.0),
            if edited {
                theme::TEXT_SECONDARY
            } else {
                theme::TEXT_MUTED
            },
            Align::Min,
        );
        response.on_hover_text(format!("{text}\n{note}"));
    } else {
        response.on_hover_text(text);
    }
}

/// Paints one bounded line with an ellipsis when its text exceeds the available width.
pub(super) fn line_text(
    ui: &Ui,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    align: Align,
) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = rect.width();
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let x = match align {
        Align::Min => rect.left(),
        Align::Center => rect.center().x - galley.size().x * 0.5,
        Align::Max => rect.right() - galley.size().x,
    };
    ui.painter().galley(Pos2::new(x, rect.top()), galley, color);
}

/// A read-only rotary control sized for device cards, panners and send banks.
pub(super) struct Knob<'a> {
    pub(super) label: &'a str,
    pub(super) value: f64,
    pub(super) range: (Unit, f64, f64),
    pub(super) comparison: Option<Mark>,
    pub(super) presence: Option<(bool, String)>,
    pub(super) word: &'a str,
    pub(super) accent: Color32,
}

impl Knob<'_> {
    pub(super) fn draw(
        self,
        ui: &mut Ui,
        width: f32,
        radius: f32,
        display: impl Fn(f64) -> String,
    ) {
        let (unit, min, max) = self.range;
        let prior = match self.comparison {
            Some(Mark::Edited(was)) => Some((was, format!("Was {}", display(was)))),
            Some(Mark::Differs(was))
                if !self.presence.as_ref().is_some_and(|(edited, _)| *edited) =>
            {
                Some((was, format!("{} {}", self.word, display(was))))
            }
            _ => None,
        };
        let status = prior
            .as_ref()
            .map(|(_, note)| {
                (
                    matches!(self.comparison, Some(Mark::Edited(_))),
                    note.as_str(),
                )
            })
            .or_else(|| {
                self.presence
                    .as_ref()
                    .map(|(edit, note)| (*edit, note.as_str()))
            });
        let edited = status.is_some_and(|(edit, _)| edit);
        let height = radius * 2.0 + 27.0 + if status.is_some() { 11.0 } else { 0.0 };
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter();
        if status.is_some() {
            painter.rect_stroke(rect, 4.0, outline(edited, true), StrokeKind::Inside);
        }
        let center = Pos2::new(rect.center().x, rect.top() + 13.0 + radius);
        let point = |t: f32, r| {
            let angle = std::f32::consts::PI * (0.75 + t * 1.5);
            center + Vec2::angled(angle) * r
        };
        let arc = |a: f32, b: f32, stroke| {
            if (a - b).abs() > 0.001 {
                painter.add(egui::Shape::line(
                    (0..=16)
                        .map(|i| point(egui::lerp(a..=b, i as f32 / 16.0), radius))
                        .collect(),
                    stroke,
                ));
            }
        };
        let norm = |n| theme::value_to_norm(n, min, max, unit);
        let position = norm(self.value);
        let origin = if widgets::fills_from_centre(unit, min, max) {
            norm(0.0)
        } else {
            0.0
        };
        let accent = if edited {
            theme::SELECTION
        } else {
            self.accent
        };
        painter.circle_filled(center, radius - 3.0, theme::BG_CARD);
        arc(0.0, 1.0, Stroke::new(2.0_f32, theme::BORDER_STRONG));
        arc(
            origin.min(position),
            origin.max(position),
            Stroke::new(2.0_f32, accent),
        );
        if let Some((was, _)) = &prior {
            let was = norm(*was);
            painter.line_segment(
                [point(was, radius - 1.5), point(was, radius + 3.0)],
                Stroke::new(1.2_f32, theme::TEXT_SECONDARY),
            );
        }
        painter.line_segment(
            [
                point(position, radius * 0.25),
                point(position, radius - 3.5),
            ],
            Stroke::new(
                1.6_f32,
                if edited {
                    accent
                } else {
                    theme::TEXT_SECONDARY
                },
            ),
        );
        let inner = rect.shrink2(Vec2::new(if width < 30.0 { 0.5 } else { 1.5 }, 1.0));
        line_text(
            ui,
            inner,
            self.label,
            FontId::proportional(if width < 30.0 { 8.0 } else { 9.0 }),
            theme::TEXT_SECONDARY,
            Align::Center,
        );
        let value = display(self.value);
        let below = inner.translate(Vec2::new(0.0, radius * 2.0 + 13.0));
        line_text(
            ui,
            below,
            &value,
            FontId::monospace(if width < 30.0 { 8.5 } else { 10.0 }),
            theme::TEXT_PRIMARY,
            Align::Center,
        );
        let mut tip = format!(
            "{} · {value}\n{} to {}",
            self.label,
            display(min),
            display(max)
        );
        if let Some((_, note)) = status {
            line_text(
                ui,
                below.translate(Vec2::new(0.0, 12.0)),
                note,
                FontId::proportional(9.0),
                theme::TEXT_SECONDARY,
                Align::Center,
            );
            tip.push('\n');
            tip.push_str(note);
        }
        response.on_hover_text(tip);
    }
}

/// One device card with the effective controls and its versioned response preview.
fn device(
    ui: &mut Ui,
    available: f32,
    view: &SoundView<'_>,
    processor: &Processor,
    previous: Option<(usize, &Processor)>,
    other: Option<(usize, &Processor)>,
    index: usize,
) {
    let old = previous.map(|(_, p)| p);
    let compared = other.map(|(_, p)| p);
    let tempo = view.shown.song.tempo_bpm;
    let Ok(settings) = resolve_settings(processor, tempo) else {
        // Old History can contain a setting no longer resolved by the current processor schema.
        ui.allocate_ui_with_layout(
            Vec2::new(available, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(available);
                effect_settings(ui, processor, old, compared, view.other_word);
            },
        );
        return;
    };
    let resolve = |p: Option<&Processor>, project: Option<&Project>| {
        p.zip(project)
            .filter(|(p, _)| p.kind == processor.kind)
            .and_then(|(p, project)| resolve_settings(p, project.song.tempo_bpm).ok())
    };
    let before = resolve(old, view.previous);
    let comparison = resolve(compared, view.other);
    let changed = |slot: (usize, &Processor)| slot.0 != index || !same_device(processor, slot.1);
    let edited = view.previous.is_some() && previous.is_none_or(changed);
    let differs = view.other.is_some() && other.is_none_or(changed);
    let controls = settings
        .iter()
        .filter(|s| s.descriptor.contract.continuous().is_some())
        .count();
    const KNOB: f32 = 62.0;
    const PREVIEW: f32 = 156.0;
    const GAP: f32 = 8.0;
    let content_width = available - 16.0;
    let split =
        preview_axes(processor.kind).is_some() && content_width >= PREVIEW + GAP + KNOB * 3.0;
    let preview_width = if split { PREVIEW + GAP } else { 0.0 };
    let max_columns = if matches!(
        processor.kind,
        ProcessorKind::Compressor | ProcessorKind::Gate
    ) {
        4
    } else {
        7
    };
    let controls_width = (controls.clamp(3, max_columns) as f32 * (KNOB + 3.0) - 3.0)
        .min(content_width - preview_width);
    let width = controls_width + preview_width;
    let accent = theme::processor_color(processor.kind);
    let color = if processor.bypass {
        theme::TEXT_MUTED
    } else {
        accent
    };
    let word = view.other_word;
    // A frame measures after painting. Reserve its width so the wrapping row can
    // place the card before the painter receives any coordinates.
    ui.allocate_ui_with_layout(
        Vec2::new(width + 16.0, 0.0),
        Layout::top_down(Align::Min),
        |ui| {
            egui::Frame::new()
                .fill(theme::BG_SURFACE)
                .stroke(outline(edited, differs))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::same(7))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.y = 5.0;
                    let (header, response) =
                        ui.allocate_exact_size(Vec2::new(width, 29.0), Sense::hover());
                    let state = if processor.bypass { "Bypassed" } else { "On" };
                    let state_change = difference(
                        processor.bypass,
                        old.map(|p| p.bypass),
                        compared.map(|p| p.bypass),
                        word,
                        |b| if b { "bypassed" } else { "on" }.to_owned(),
                    );
                    let title = theme::processor_title(processor.kind);
                    let identity = format!("{:02}  {}", index + 1, processor.id);
                    let title_area = header.with_max_x(header.right() - 75.0);
                    line_text(
                        ui,
                        title_area,
                        title,
                        FontId::proportional(13.0),
                        color,
                        Align::Min,
                    );
                    line_text(
                        ui,
                        title_area.translate(Vec2::new(0.0, 15.0)),
                        &identity,
                        FontId::monospace(9.0),
                        theme::TEXT_SECONDARY,
                        Align::Min,
                    );
                    let led = Rect::from_min_max(
                        header.right_top() - Vec2::new(72.0, 0.0),
                        header.right_bottom(),
                    );
                    ui.painter().circle_filled(
                        Pos2::new(
                            led.right() - if processor.bypass { 47.0 } else { 18.0 },
                            led.top() + 8.0,
                        ),
                        2.6,
                        color,
                    );
                    line_text(
                        ui,
                        led,
                        state,
                        FontId::proportional(10.0),
                        if state_change.as_ref().is_some_and(|(edit, _)| *edit) {
                            theme::SELECTION
                        } else {
                            color
                        },
                        Align::Max,
                    );
                    if let Some((_, note)) = state_change {
                        line_text(
                            ui,
                            led.translate(Vec2::new(0.0, 14.0)),
                            &note,
                            FontId::proportional(9.0),
                            theme::TEXT_SECONDARY,
                            Align::Max,
                        );
                    }
                    let position = |(i, p): (usize, &Processor)| (i, p.kind);
                    let prior = difference(
                        (index, processor.kind),
                        previous.map(position),
                        other.map(position),
                        word,
                        |(i, k)| format!("{} in slot {}", theme::processor_title(k), i + 1),
                    )
                    .or_else(|| {
                        if view.previous.is_some() && previous.is_none() {
                            Some((true, "Was absent".to_owned()))
                        } else if view.other.is_some() && other.is_none() {
                            Some((false, format!("{word} absent")))
                        } else {
                            None
                        }
                    });
                    if let Some((_, note)) = &prior {
                        response.on_hover_text(format!("{title} · {}\n{note}", processor.id));
                        ui.label(RichText::new(note).size(9.0).color(theme::TEXT_SECONDARY));
                    } else {
                        response.on_hover_text(format!("{title} · {}", processor.id));
                    }
                    let draw_preview = |ui: &mut Ui| {
                        visualizer(
                            ui,
                            processor.kind,
                            &settings,
                            before.as_ref(),
                            comparison.as_ref(),
                            color,
                        );
                    };
                    let draw_controls = |ui: &mut Ui| {
                        ui.set_width(controls_width);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                            for setting in settings.iter() {
                                let key = setting.descriptor.key;
                                let Some((unit, min, max)) =
                                    processor_setting_contract(processor.kind, key)
                                else {
                                    continue;
                                };
                                let Some(value) = settings.numeric(key) else {
                                    continue;
                                };
                                let previous = before.as_ref().and_then(|s| s.numeric(key));
                                let other = comparison.as_ref().and_then(|s| s.numeric(key));
                                Knob {
                                    label: &setting_label(key),
                                    value,
                                    range: (unit, min, max),
                                    comparison: mark(value, previous, other),
                                    presence: absent_setting(
                                        before.as_ref(),
                                        comparison.as_ref(),
                                        key,
                                        word,
                                    ),
                                    word,
                                    accent: color,
                                }
                                .draw(
                                    ui,
                                    KNOB.min(controls_width),
                                    12.0,
                                    |v| format_setting(key, unit, v),
                                );
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                            for descriptor in descriptors(processor.kind) {
                                let key = descriptor.key;
                                let shown = settings.get(key);
                                let was = before.as_ref().and_then(|s| s.get(key));
                                let compared = comparison.as_ref().and_then(|s| s.get(key));
                                let (value, prior) = match shown {
                                    Some(setting) if descriptor.contract.continuous().is_none() => {
                                        let prior = difference(
                                            &setting.value,
                                            was.map(|s| &s.value),
                                            compared.map(|s| &s.value),
                                            word,
                                            |value| resolved_text(setting, value),
                                        )
                                        .or_else(|| {
                                            absent_setting(
                                                before.as_ref(),
                                                comparison.as_ref(),
                                                key,
                                                word,
                                            )
                                        });
                                        (resolved_text(setting, &setting.value), prior)
                                    }
                                    // Conditional settings disappear when the band or sync mode is disabled.
                                    None if was.is_some() || compared.is_some() => {
                                        let prior = if let Some(was) = was {
                                            (
                                                true,
                                                format!("Was {}", resolved_text(was, &was.value)),
                                            )
                                        } else if let Some(compared) = compared {
                                            (
                                                false,
                                                format!(
                                                    "{word} {}",
                                                    resolved_text(compared, &compared.value)
                                                ),
                                            )
                                        } else {
                                            unreachable!("a comparison setting is present");
                                        };
                                        ("Off".to_owned(), Some(prior))
                                    }
                                    _ => continue,
                                };
                                readout(
                                    ui,
                                    controls_width.min(118.0),
                                    &format!("{}  {value}", setting_label(key)),
                                    theme::TEXT_SECONDARY,
                                    prior,
                                );
                            }
                        });
                    };
                    if split {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = GAP;
                            ui.vertical(|ui| {
                                ui.set_width(PREVIEW);
                                draw_preview(ui);
                            });
                            ui.vertical(draw_controls);
                        });
                    } else {
                        draw_preview(ui);
                        draw_controls(ui);
                    }
                });
        },
    );
}

/// Reports a conditional control missing in a version where the device exists.
fn absent_setting(
    previous: Option<&ResolvedProcessorSettings>,
    other: Option<&ResolvedProcessorSettings>,
    key: &str,
    word: &str,
) -> Option<(bool, String)> {
    difference(
        true,
        previous.map(|s| s.contains(key)),
        other.map(|s| s.contains(key)),
        word,
        |_| "off".to_owned(),
    )
}

/// Formats a resolved discrete value or a numeric value for an unavailable control.
fn resolved_text(setting: &ResolvedProcessorSetting, value: &ResolvedSettingValue) -> String {
    match value {
        ResolvedSettingValue::Choice(value) => readable_literal(value),
        ResolvedSettingValue::Unsigned(value) => value.to_string(),
        ResolvedSettingValue::Numeric(value) => setting.descriptor.contract.numeric().map_or_else(
            || value.to_string(),
            |(unit, _, _, _)| format_setting(setting.descriptor.key, unit, *value),
        ),
    }
}

/// Names device panel controls without chopping technical terms mid-word.
fn setting_label(key: &str) -> String {
    key.replace("frequency", "freq")
        .replace("threshold", "thresh")
        .replace("feedback", "fdbk")
        .replace("crossfade", "xfade")
        .replace('_', " ")
        .to_uppercase()
}

/// Formats ratios according to their meaning and keeps fractional milliseconds visible.
fn format_setting(key: &str, unit: Unit, value: f64) -> String {
    match (unit, key) {
        (Unit::Ratio, "ratio") => format!("{value:.2}:1"),
        (Unit::Ratio, "drive") => format!("{value:.2}×"),
        (Unit::Ratio, "mid1_q" | "mid2_q") => format!("{value:.2}"),
        (Unit::Seconds, _) if value > 0.0 && value < 0.01 => format!("{:.2} ms", value * 1_000.0),
        (Unit::Decibels, _) => format!("{value:+.1} dB"),
        _ => theme::format_compact_value(value, unit),
    }
}

/// Title and horizontal endpoints of a device preview.
fn preview_axes(kind: ProcessorKind) -> Option<(&'static str, &'static str, &'static str)> {
    use ProcessorKind::*;
    Some(match kind {
        Eq => ("FREQUENCY RESPONSE", "20 Hz", "20 kHz"),
        Compressor | Gate | Limiter => ("TRANSFER", "−96 dB", "+12 dB"),
        Saturation | Crusher => ("TRANSFER", "−1", "+1"),
        Delay => ("ECHO TAPS", "0", ""),
        Reverb => ("REVERB TAIL", "0", "2 s"),
        ModulationDelay => ("DELAY MODULATION", "0", "1 s"),
        Utility | Stutter | Latency => return None,
    })
}

/// Plots a static response using the resolved values, with the old response behind the current one.
fn visualizer(
    ui: &mut Ui,
    kind: ProcessorKind,
    settings: &ResolvedProcessorSettings,
    previous: Option<&ResolvedProcessorSettings>,
    other: Option<&ResolvedProcessorSettings>,
    accent: Color32,
) {
    use ProcessorKind::*;
    let Some((label, left, right)) = preview_axes(kind) else {
        return;
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 65.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, theme::BG_WELL);
    let plot = Rect::from_min_max(
        rect.left_top() + Vec2::new(8.0, 17.0),
        rect.right_bottom() - Vec2::new(8.0, 14.0),
    );
    let pos = |x: f64, y: f64| {
        Pos2::new(
            egui::lerp(plot.x_range(), x as f32),
            egui::lerp(plot.bottom()..=plot.top(), y.clamp(0.0, 1.0) as f32),
        )
    };
    for frac in [0.0, 0.25, 0.5, 0.75, 1.0] {
        painter.vline(
            pos(frac, 0.0).x,
            plot.y_range(),
            Stroke::new(1.0_f32, theme::GRID_BEAT),
        );
        painter.hline(
            plot.x_range(),
            pos(0.0, frac).y,
            Stroke::new(1.0_f32, theme::GRID_BEAT),
        );
    }
    if matches!(kind, Compressor | Gate | Limiter | Saturation | Crusher) {
        painter.line_segment(
            [plot.left_bottom(), plot.right_top()],
            Stroke::new(1.0_f32, theme::BORDER_STRONG),
        );
    }
    let duration = if kind == Delay {
        [Some(settings), previous, other]
            .into_iter()
            .flatten()
            .filter_map(|s| s.numeric("time"))
            .fold(0.0, f64::max)
            * 7.3
    } else {
        2.0
    };
    let points = |s: &ResolvedProcessorSettings| {
        let num = |key| s.numeric(key).unwrap_or_default();
        if kind == Delay {
            let interval = num("time");
            let (mix, feedback) = (num("mix"), num("feedback"));
            let mut points = Vec::with_capacity(34);
            points.push(pos(0.0, 1.0 - mix));
            points.push(pos(0.0, 0.0));
            for i in 1..=8 {
                let x = f64::from(i) * interval / duration.max(interval);
                if x > 1.0 {
                    break;
                }
                points.extend([pos(x, 0.0), pos(x, mix * feedback.powi(i - 1)), pos(x, 0.0)]);
            }
            points.push(pos(1.0, 0.0));
            return points;
        }

        (0..=96)
            .map(|i| {
                let x = f64::from(i) / 96.0;
                let input = x * 2.0 - 1.0;
                let value = match kind {
                    Eq => 0.5 + eq_db(s, 20.0 * 1000_f64.powf(x)) / 52.0,
                    Compressor | Gate | Limiter => {
                        let db = -96.0 + x * 108.0;
                        let threshold = num("threshold");
                        let ratio = num("ratio");
                        let wet = match kind {
                            Limiter => db.min(num("ceiling")),
                            Gate => {
                                db - ((threshold - db).max(0.0)
                                    * if s.choice("mode") == Some("gate") {
                                        8.0
                                    } else {
                                        ratio - 1.0
                                    })
                                .min(num("range"))
                            }
                            _ => {
                                db - compressor_reduction(db - threshold, ratio, num("knee"))
                                    + num("makeup")
                            }
                        };
                        let out = if kind == Limiter {
                            wet
                        } else {
                            let mix = num("mix");
                            let amp = |db| 10_f64.powf(db / 20.0);
                            20.0 * ((1.0 - mix) * amp(db) + mix * amp(wet)).log10()
                        };
                        (out + 96.0) / 108.0
                    }
                    Saturation => {
                        let driven = input * num("drive");
                        let wet = match s.choice("mode") {
                            Some("tanh") => driven.tanh(),
                            Some("tape") => (driven + 0.05).tanh() - 0.05_f64.tanh(),
                            _ => driven / (1.0 + driven.abs()),
                        };
                        let output = (input + (wet - input) * num("mix"))
                            * 10_f64.powf(num("output") / 20.0);
                        (output + 1.0) * 0.5
                    }
                    Crusher => {
                        let steps = 2_f64.powf(num("bit_depth") - 1.0);
                        let wet = (input * steps).round() / steps;
                        let output = (input + (wet - input) * num("mix"))
                            * 10_f64.powf(num("output") / 20.0);
                        (output + 1.0) * 0.5
                    }
                    Reverb => {
                        let elapsed = x * duration - num("pre_delay");
                        let tail = if elapsed < 0.0 {
                            0.0
                        } else {
                            let decay = 0.12 + 0.98 * num("decay");
                            let ripple = (elapsed * (55.0 + 35.0 * num("diffusion"))).sin();
                            ripple * (-elapsed / decay).exp() * num("mix")
                        };
                        0.5 + 0.48 * tail
                    }
                    ModulationDelay => {
                        let time = num("delay")
                            + num("depth") * (x * std::f64::consts::TAU * num("rate")).sin();
                        time / 0.07
                    }
                    _ => unreachable!("this device has no response preview"),
                };
                pos(x, value)
            })
            .collect::<Vec<_>>()
    };
    let current = points(settings);
    let previous = previous.map(points).filter(|old| *old != current);
    let other = if previous.is_none() {
        other.map(points).filter(|old| *old != current)
    } else {
        None
    };
    let note = if let Some(old) = previous {
        painter.add(egui::Shape::line(
            old,
            Stroke::new(1.3_f32, theme::TEXT_SECONDARY),
        ));
        Some("Prev.")
    } else if let Some(other) = other {
        painter.add(egui::Shape::line(
            other,
            Stroke::new(1.0_f32, theme::TEXT_MUTED),
        ));
        // The controls' comparison notes name the other side.
        Some("A/B")
    } else {
        None
    };
    painter.add(egui::Shape::line(current, Stroke::new(1.8_f32, accent)));
    let axis_font = FontId::monospace(8.0);
    painter.text(
        rect.left_top() + Vec2::new(7.0, 3.0),
        Align2::LEFT_TOP,
        label,
        axis_font.clone(),
        theme::TEXT_SECONDARY,
    );
    if let Some(note) = note {
        painter.text(
            rect.right_top() + Vec2::new(-7.0, 3.0),
            Align2::RIGHT_TOP,
            note,
            axis_font.clone(),
            theme::TEXT_SECONDARY,
        );
    }
    painter.text(
        rect.left_bottom() + Vec2::new(7.0, -3.0),
        Align2::LEFT_BOTTOM,
        left,
        axis_font.clone(),
        theme::TEXT_MUTED,
    );
    let end = if kind == Delay {
        format!("{duration:.2} s")
    } else {
        right.to_owned()
    };
    painter.text(
        rect.right_bottom() + Vec2::new(-7.0, -3.0),
        Align2::RIGHT_BOTTOM,
        end,
        axis_font,
        theme::TEXT_MUTED,
    );
}

/// Static dB response of the enabled EQ bands, summed in signal order.
fn eq_db(settings: &ResolvedProcessorSettings, frequency: f64) -> f64 {
    let num = |key| settings.numeric(key);
    let power = |r: f64| 1.0 + r.powi(4);
    let mut db = 0.0;
    if let Some(cutoff) = num("high_pass") {
        db -= 10.0 * power(cutoff / frequency).log10();
    }
    if let Some(cutoff) = num("low_pass") {
        db -= 10.0 * power(frequency / cutoff).log10();
    }
    for (freq, gain, high) in [
        ("low_frequency", "low_gain", false),
        ("high_frequency", "high_gain", true),
    ] {
        if let Some(center) = num(freq) {
            let ratio = if high {
                center / frequency
            } else {
                frequency / center
            };
            let amplitude = 10_f64.powf(num(gain).unwrap_or_default() / 40.0);
            db += 10.0 * (amplitude.powi(4) * power(ratio / amplitude) / power(ratio)).log10();
        }
    }
    for (freq, gain, q) in [
        ("mid1_frequency", "mid1_gain", "mid1_q"),
        ("mid2_frequency", "mid2_gain", "mid2_q"),
    ] {
        if let Some(center) = num(freq) {
            let ratio = frequency / center;
            let amplitude = 10_f64.powf(num(gain).unwrap_or_default() / 40.0);
            let width = ratio / num(q).unwrap_or(1.0);
            let base = (1.0 - ratio.powi(2)).powi(2);
            db += 10.0
                * ((base + (width * amplitude).powi(2)) / (base + (width / amplitude).powi(2)))
                    .log10();
        }
    }
    db
}

/// Steady-state, soft-knee compressor reduction for a level relative to threshold.
fn compressor_reduction(over: f64, ratio: f64, knee: f64) -> f64 {
    let slope = 1.0 - 1.0 / ratio;
    if knee == 0.0 || over >= knee * 0.5 {
        over.max(0.0) * slope
    } else {
        (over + knee * 0.5).max(0.0).powi(2) * slope / (2.0 * knee)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rack_wraps_before_a_device_exceeds_the_panel() {
        let fixture = "tests/fixtures/production_mix.crump";
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/production_mix.crump"
        ));
        let project = crate::dsl::parse_project(std::path::Path::new(fixture), source)
            .project
            .expect("production mix fixture");
        let view = SoundView {
            shown: &project,
            previous: None,
            other: None,
            other_word: "now",
        };
        let inserts = &project.song.groups[0].channel.inserts;
        egui::__run_test_ui(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(500.0, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(500.0);
                chain(ui, &view, inserts, None, None, &[]);
                assert!(
                    ui.min_rect().width() <= 500.0,
                    "rack widened the panel: {:?}",
                    ui.min_rect()
                );
                assert!(ui.min_rect().height() > 200.0, "rack did not wrap");
            });
        });
    }
}
