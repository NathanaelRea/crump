//! Horizontal FX Device Chain Rack (Ableton/Bitwig/FabFilter style):
//! - Upper Split: channel-scoped signal flow cards with compact pinned footers, channel-group
//!   divider badges, centered 3-column knobs with real defaults, clickable discrete choice cyclers,
//!   honest I/O + GR meters, and `smp` latency badges.
//! - Lower Split: Focused Device Deep Inspector with large FabFilter-style EQ/Dynamics/FX
//!   frequency & transfer analyzer and primary device controls.

use super::theme::{self, with_alpha};
use super::widgets::{
    Cell, apply_row_click, led_button, paint_dbfs_meter_vertical, paint_gr_meter_vertical,
    pill_toggle, rotary_knob, table, toolbar, unavailable,
};
use super::*;
use crate::engine::mix_graph::CompiledProcessorInspection;
use crate::model::{ProcessorKind, Unit};

const CARD_WIDTH: f32 = 278.0;

pub(super) fn processing(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(graph) = app.processing_graph() else {
        unavailable(
            ui,
            "Processing rack unavailable while Project source cannot compile",
        );
        return;
    };

    // Collect distinct owners in deterministic chain order
    let mut owners: Vec<String> = Vec::new();
    for proc in &graph.processors {
        if !owners.contains(&proc.owner) {
            owners.push(proc.owner.clone());
        }
    }

    // Validate current filter still exists
    if let Some(ref filter) = cx.state.processing_owner_filter
        && !owners.contains(filter)
    {
        cx.state.processing_owner_filter = None;
    }

    // ── Slim Top Mix Sub-Bar + Chain Scope Filter ──
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;

                // Matching Mix sub-view switcher (Console | FX Rack)
                if pill_toggle(
                    ui,
                    "Console",
                    app.view == View::Mixer,
                    theme::ACCENT_AMBER,
                    "Return to Mixer Console · 2",
                )
                .clicked()
                {
                    cx.send(Action::ShowView(View::Mixer));
                }
                if pill_toggle(
                    ui,
                    "FX Rack",
                    app.view == View::Processing && !cx.state.processing_table_mode,
                    theme::ACCENT_AMBER,
                    "Horizontal FX signal-chain device rack · 3",
                )
                .clicked()
                {
                    cx.state.processing_table_mode = false;
                }

                ui.separator();

                ui.label(
                    RichText::new("CHAIN")
                        .size(9.5)
                        .strong()
                        .color(theme::TEXT_MUTED),
                );
                if pill_toggle(
                    ui,
                    &format!("All ({})", graph.processors.len()),
                    cx.state.processing_owner_filter.is_none(),
                    theme::ACCENT_CYAN,
                    "Show all processors across the mix graph",
                )
                .clicked()
                {
                    cx.state.processing_owner_filter = None;
                }
                for owner in &owners {
                    let count = graph
                        .processors
                        .iter()
                        .filter(|p| p.owner == *owner)
                        .count();
                    let active =
                        cx.state.processing_owner_filter.as_deref() == Some(owner.as_str());
                    let short = short_owner_label(owner);
                    if pill_toggle(
                        ui,
                        &format!("{short} ({count})"),
                        active,
                        theme::ACCENT_CYAN,
                        &format!("Filter device rack to {owner}"),
                    )
                    .clicked()
                    {
                        cx.state.processing_owner_filter = Some(owner.clone());
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if pill_toggle(
                        ui,
                        "Params Table",
                        cx.state.processing_table_mode,
                        theme::ACCENT_CYAN,
                        "Show full processor inspection table",
                    )
                    .clicked()
                    {
                        cx.state.processing_table_mode = !cx.state.processing_table_mode;
                    }
                });
            });
        });

    if cx.state.processing_table_mode {
        processing_table(ui, cx, graph);
        return;
    }

    if graph.processors.is_empty() {
        unavailable(ui, "No insert or bus processors authored in this Project");
        return;
    }

    let visible: Vec<(usize, &CompiledProcessorInspection)> = graph
        .processors
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            cx.state
                .processing_owner_filter
                .as_deref()
                .is_none_or(|owner| p.owner == owner)
        })
        .collect();

    let selected = clamp_index(app.selected_processor, graph.processors.len());
    let focused_proc = graph.processors.get(selected);

    // ── Lower Split: Focused Device Deep Inspector (FabFilter-style Analyzer + Primary Controls) ──
    if let Some(proc) = focused_proc {
        egui::TopBottomPanel::bottom("crump-fx-deep-inspector")
            .resizable(false)
            .exact_height(228.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_SURFACE)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK)),
            )
            .show_inside(ui, |ui| {
                focused_device_inspector(ui, cx, selected, proc);
            });
    }

    // ── Upper Split: Compact Horizontal Scrollable Studio FX Device Rack ──
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::BG_WELL)
                .inner_margin(egui::Margin::same(10)),
        )
        .show_inside(ui, |ui| {
            let rack_cap = (ui.available_height() - 14.0).max(250.0);
            let tallest = visible
                .iter()
                .map(|(_, proc)| device_content_height(proc))
                .fold(250.0_f32, f32::max)
                .min(rack_cap);
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let mut prev_owner: Option<&str> = None;
                        for (global_idx, proc) in visible {
                            let is_new_owner = prev_owner.is_some_and(|p| p != proc.owner);
                            if let Some(prev) = prev_owner {
                                signal_connector(
                                    ui,
                                    prev == proc.owner,
                                    if is_new_owner {
                                        Some(short_owner_label(&proc.owner))
                                    } else {
                                        None
                                    },
                                    tallest,
                                );
                            }
                            prev_owner = Some(proc.owner.as_str());
                            let card_h = device_content_height(proc).clamp(250.0, rack_cap);
                            device_card(ui, cx, global_idx, proc, global_idx == selected, card_h);
                        }
                    });
                });
        });
}

/// The height a device card needs for its header, scope, choice pills, knob rows and footer, so
/// cards hug their controls instead of stretching to fill the rack.
fn device_content_height(proc: &CompiledProcessorInspection) -> f32 {
    let knobs = proc
        .settings
        .iter()
        .filter(|s| s.unit.is_some() && s.value.is_some())
        .count();
    let has_choices = proc.settings.iter().any(|s| s.choice.is_some());
    let rows = knobs.div_ceil(3).max(1) as f32;
    126.0 + if has_choices { 34.0 } else { 0.0 } + rows * 92.0 + 40.0
}

fn short_owner_label(owner: &str) -> &str {
    owner.split('/').next_back().unwrap_or(owner)
}

fn signal_connector(ui: &mut Ui, same_chain: bool, next_owner: Option<&str>, height: f32) {
    let w = if same_chain { 22.0 } else { 42.0 };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, height), Sense::hover());
    let p = ui.painter();
    let cy = rect.top() + 86.0;
    if same_chain {
        p.line_segment(
            [
                Pos2::new(rect.left() + 2.0, cy),
                Pos2::new(rect.right() - 4.0, cy),
            ],
            Stroke::new(1.8_f32, theme::ACCENT_CYAN),
        );
        p.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(rect.right() - 8.0, cy - 4.0),
                Pos2::new(rect.right() - 2.0, cy),
                Pos2::new(rect.right() - 8.0, cy + 4.0),
            ],
            theme::ACCENT_CYAN,
            Stroke::NONE,
        ));
    } else {
        p.vline(
            rect.center().x,
            (rect.top() + 26.0)..=(rect.bottom() - 10.0),
            Stroke::new(1.0_f32, theme::BORDER_STRONG),
        );
        if let Some(owner) = next_owner {
            let badge = Rect::from_center_size(
                Pos2::new(rect.center().x, rect.top() + 12.0),
                Vec2::new(38.0, 16.0),
            );
            p.rect_filled(badge, 3.0, with_alpha(theme::ACCENT_CYAN, 0.2));
            p.rect_stroke(
                badge,
                3.0,
                Stroke::new(1.0_f32, with_alpha(theme::ACCENT_CYAN, 0.55)),
                StrokeKind::Inside,
            );
            p.with_clip_rect(badge.shrink(2.0)).text(
                badge.center(),
                Align2::CENTER_CENTER,
                owner.to_uppercase(),
                FontId::monospace(8.0),
                theme::ACCENT_CYAN,
            );
        }
    }
}

/// Sensible physical default for double-click reset on processor settings.
fn sensible_default_for(key: &str, unit: Unit, min: f64, max: f64) -> f64 {
    let k = key.to_ascii_lowercase();
    let raw = if k.contains("gain") || k.contains("makeup") || k.contains("output") {
        0.0
    } else if k.contains("mix") || k.contains("wet") {
        0.5
    } else if k.contains("threshold") {
        -18.0
    } else if k.contains("ratio") {
        4.0
    } else if k.contains("q") || k.contains("resonance") {
        0.707
    } else if k.contains("drive") {
        1.0
    } else if (matches!(unit, Unit::Decibels) && min <= 0.0 && max >= 0.0)
        || matches!(unit, Unit::Pan)
    {
        0.0
    } else {
        (min + max) * 0.5
    };
    raw.clamp(min, max)
}

/// Next choice when cycling a discrete processor setting pill directly.
fn next_discrete_choice(kind: ProcessorKind, key: &str, current: &str) -> Option<&'static str> {
    let options: &[&str] = match (kind, key) {
        (_, "tempo_sync") | (_, "sync") => &["true", "false"],
        (_, "detector") => &["peak", "rms"],
        (_, "division") => &["1/16", "1/8", "1/8d", "1/4", "1/4d", "1/2"],
        (ProcessorKind::Delay, "mode") => &["stereo", "ping_pong"],
        (ProcessorKind::Saturation, "mode" | "curve" | "shape") => {
            &["soft", "hard", "tape", "tube"]
        }
        (_, "phase") => &["normal", "invert"],
        _ => return None,
    };
    let pos = options.iter().position(|&opt| opt == current).unwrap_or(0);
    Some(options[(pos + 1) % options.len()])
}

fn device_card(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    global_idx: usize,
    proc: &CompiledProcessorInspection,
    is_selected: bool,
    card_h: f32,
) {
    let app = cx.app;
    let accent = theme::processor_color(proc.kind);
    let meter = (proc.index < app.telemetry.engine.processor_count)
        .then(|| app.telemetry.engine.processor_activity[proc.index]);
    let transient = app.audition_plan.transient_bypass.get(&proc.id).copied();
    let effective_bypass = meter.map_or_else(
        || transient.unwrap_or(proc.authored_bypass),
        |m| m.effective_bypass,
    );
    let active_audio = !effective_bypass;

    let frame = egui::Frame::new()
        .fill(if is_selected {
            Color32::from_rgb(31, 37, 48)
        } else {
            theme::BG_SURFACE
        })
        .stroke(Stroke::new(
            if is_selected { 1.6_f32 } else { 1.0_f32 },
            if is_selected {
                accent
            } else {
                theme::BORDER_SUBTLE
            },
        ))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(0));

    let resp = frame
        .show(ui, |ui| {
            ui.set_width(CARD_WIDTH);
            ui.set_min_height(card_h);
            ui.set_max_height(card_h);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;

                // 1. Top color accent strip
                let (top_bar, _) =
                    ui.allocate_exact_size(Vec2::new(CARD_WIDTH, 3.0), Sense::hover());
                ui.painter().rect_filled(
                    top_bar,
                    CornerRadius {
                        nw: 6,
                        ne: 6,
                        sw: 0,
                        se: 0,
                    },
                    if active_audio {
                        accent
                    } else {
                        theme::TEXT_MUTED
                    },
                );

                // 2. Elevated Device Header Bar
                egui::Frame::new()
                    .fill(theme::BG_CARD)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let (pwr_rect, pwr_resp) =
                                ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                            let led_color = if transient.is_some() {
                                theme::SIG_HOT
                            } else if active_audio {
                                theme::SIG_NOMINAL
                            } else {
                                theme::TEXT_MUTED
                            };
                            theme::paint_power_icon(
                                ui.painter(),
                                pwr_rect,
                                led_color,
                                active_audio,
                            );
                            let pwr_resp = pwr_resp.on_hover_text(
                                "Click: A/B transient bypass · Shift+Click: durable source bypass",
                            );
                            if pwr_resp.clicked() {
                                cx.send(Action::SelectProcessor(global_idx));
                                if ui.input(|i| i.modifiers.shift) {
                                    cx.send(Action::ToggleProcessorDurableBypass {
                                        processor: proc.id.clone(),
                                    });
                                } else {
                                    cx.send(Action::ToggleProcessorTransientBypass {
                                        processor: proc.id.clone(),
                                    });
                                }
                            }

                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.label(
                                    RichText::new(theme::processor_title(proc.kind))
                                        .strong()
                                        .size(12.5)
                                        .color(if active_audio {
                                            theme::TEXT_PRIMARY
                                        } else {
                                            theme::TEXT_MUTED
                                        }),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{} · {}",
                                        short_owner_label(&proc.owner),
                                        proc.id
                                    ))
                                    .size(9.5)
                                    .color(theme::TEXT_MUTED),
                                );
                            });

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                if led_button(
                                    ui,
                                    ">",
                                    false,
                                    accent,
                                    Vec2::new(18.0, 18.0),
                                    "Move later in chain · J",
                                )
                                .clicked()
                                {
                                    cx.send(Action::MoveProcessor {
                                        processor: proc.id.clone(),
                                        direction: MoveDirection::Later,
                                    });
                                }
                                if led_button(
                                    ui,
                                    "<",
                                    false,
                                    accent,
                                    Vec2::new(18.0, 18.0),
                                    "Move earlier in chain · K",
                                )
                                .clicked()
                                {
                                    cx.send(Action::MoveProcessor {
                                        processor: proc.id.clone(),
                                        direction: MoveDirection::Earlier,
                                    });
                                }
                                if proc.output_latency_frames > 0 {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} smp",
                                            proc.output_latency_frames
                                        ))
                                        .monospace()
                                        .size(9.0)
                                        .color(theme::TEXT_MUTED),
                                    )
                                    .on_hover_text(format!(
                                        "PDC Latency: {} samples",
                                        proc.output_latency_frames
                                    ));
                                }
                            });
                        });
                    });

                // 3. Recessed Body: Visualizer + Clickable Choices + Centered 3-Col Knobs
                let body_h = (card_h - 82.0).max(180.0);
                egui::Frame::new()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_min_height(body_h - 12.0);
                        ui.set_max_height(body_h - 12.0);
                        ui.spacing_mut().item_spacing.y = 6.0;

                        visualizer_bay(ui, proc, meter, accent, active_audio, 64.0);

                        // 4. Interactive discrete choice pills (click to cycle!)
                        let choices: Vec<_> = proc
                            .settings
                            .iter()
                            .filter(|s| s.choice.is_some())
                            .collect();
                        if !choices.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(5.0, 4.0);
                                for setting in choices {
                                    let val = setting.choice.as_deref().unwrap_or("—");
                                    let next = next_discrete_choice(proc.kind, &setting.key, val);
                                    let pill_resp = egui::Frame::new()
                                        .fill(theme::BG_WELL)
                                        .stroke(Stroke::new(
                                            1.0_f32,
                                            // Neutral frame like any selector; the accent lives
                                            // only in the value text so it never reads as an error.
                                            if next.is_some() {
                                                theme::BORDER_STRONG
                                            } else {
                                                theme::BORDER_SUBTLE
                                            },
                                        ))
                                        .corner_radius(3.0)
                                        .inner_margin(egui::Margin::symmetric(6, 2))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                ui.label(
                                                    RichText::new(setting.key.to_uppercase())
                                                        .size(8.5)
                                                        .color(theme::TEXT_MUTED),
                                                );
                                                ui.label(
                                                    RichText::new(val)
                                                        .monospace()
                                                        .size(9.5)
                                                        .color(accent),
                                                );
                                            });
                                        })
                                        .response
                                        .interact(Sense::click())
                                        .on_hover_text(if let Some(n) = next {
                                            format!("Click to switch {} -> {n}", setting.key)
                                        } else {
                                            format!("Click to edit {}", setting.key)
                                        });
                                    if pill_resp.clicked() {
                                        cx.send(Action::SelectProcessor(global_idx));
                                        if let Some(next_val) = next {
                                            cx.send(Action::SetProcessorSettingValue {
                                                processor: proc.id.clone(),
                                                setting: setting.key.clone(),
                                                value: next_val.to_owned(),
                                            });
                                        } else {
                                            cx.send(Action::OpenExactEdit);
                                        }
                                    }
                                }
                            });
                        }

                        // 5. Evenly Centered 3-Column Rotary Knob Grid
                        let continuous: Vec<_> = proc
                            .settings
                            .iter()
                            .filter_map(|s| {
                                Some((
                                    s,
                                    s.unit?,
                                    s.value?,
                                    s.minimum.unwrap_or(0.0),
                                    s.maximum.unwrap_or(1.0),
                                ))
                            })
                            .collect();

                        let scroll_h = ui.available_height().max(72.0);
                        egui::ScrollArea::vertical()
                            .id_salt(("proc-knobs", &proc.id))
                            .max_height(scroll_h)
                            .show(ui, |ui| {
                                let col_w = ((CARD_WIDTH - 24.0) / 3.0).floor();
                                for chunk in continuous.chunks(3) {
                                    ui.horizontal_top(|ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        for (setting, unit, value, min, max) in chunk {
                                            ui.allocate_ui_with_layout(
                                                Vec2::new(col_w, 72.0),
                                                Layout::top_down(Align::Center),
                                                |ui| {
                                                    let knob_accent = if !active_audio {
                                                        theme::TEXT_MUTED
                                                    } else if setting.authored {
                                                        accent
                                                    } else {
                                                        theme::TEXT_SECONDARY
                                                    };
                                                    let label = short_setting_label(&setting.key);
                                                    let default_val = sensible_default_for(
                                                        &setting.key,
                                                        *unit,
                                                        *min,
                                                        *max,
                                                    );
                                                    let out = rotary_knob(
                                                        ui,
                                                        cx.state,
                                                        &format!(
                                                            "proc-{}-{}",
                                                            proc.id, setting.key
                                                        ),
                                                        &label,
                                                        *value,
                                                        *min,
                                                        *max,
                                                        default_val,
                                                        *unit,
                                                        knob_accent,
                                                        None,
                                                        &format!(
                                                            "{}.{}{}",
                                                            proc.id,
                                                            setting.key,
                                                            if setting.authored {
                                                                " (authored)"
                                                            } else {
                                                                " (default)"
                                                            }
                                                        ),
                                                        38.0,
                                                    );
                                                    if out.clicked {
                                                        cx.send(Action::SelectProcessor(
                                                            global_idx,
                                                        ));
                                                    }
                                                    if out.right_clicked {
                                                        cx.send(Action::SelectProcessor(
                                                            global_idx,
                                                        ));
                                                        cx.send(Action::OpenExactEdit);
                                                    }
                                                    if let Some(new_val) = out.committed {
                                                        let formatted = theme::format_crump_literal(
                                                            new_val, *unit,
                                                        );
                                                        cx.send(Action::SetProcessorSettingValue {
                                                            processor: proc.id.clone(),
                                                            setting: setting.key.clone(),
                                                            value: formatted,
                                                        });
                                                    }
                                                },
                                            );
                                        }
                                    });
                                }
                            });
                    });

                // 6. Pinned Hardware Status Footer (Sidechain badge + Active/Bypassed toggle)
                egui::Frame::new()
                    .fill(theme::BG_WELL)
                    .inner_margin(egui::Margin::symmetric(10, 5))
                    .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if !proc.sidechain_inputs.is_empty() {
                                ui.label(
                                    RichText::new(format!(
                                        "SC: {}",
                                        proc.sidechain_inputs.join(", ")
                                    ))
                                    .size(9.5)
                                    .color(theme::ACCENT_VIOLET),
                                );
                            } else {
                                ui.label(
                                    RichText::new(format!("{} params", proc.settings.len()))
                                        .size(9.5)
                                        .color(theme::TEXT_MUTED),
                                );
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if led_button(
                                    ui,
                                    if proc.authored_bypass {
                                        "BYPASSED"
                                    } else {
                                        "ACTIVE"
                                    },
                                    !proc.authored_bypass,
                                    theme::SIG_NOMINAL,
                                    Vec2::new(66.0, 18.0),
                                    "Toggle durable source bypass · B",
                                )
                                .clicked()
                                {
                                    cx.send(Action::ToggleProcessorDurableBypass {
                                        processor: proc.id.clone(),
                                    });
                                }
                            });
                        });
                    });
            });
        })
        .response
        .interact(Sense::click());

    if resp.clicked() {
        cx.send(Action::SelectProcessor(global_idx));
    }
}

/// Lower Split: FabFilter-style Focused Device Deep Inspector for the selected FX device.
fn focused_device_inspector(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    selected_idx: usize,
    proc: &CompiledProcessorInspection,
) {
    let app = cx.app;
    let accent = theme::processor_color(proc.kind);
    let meter = (proc.index < app.telemetry.engine.processor_count)
        .then(|| app.telemetry.engine.processor_activity[proc.index]);
    let transient = app.audition_plan.transient_bypass.get(&proc.id).copied();
    let effective_bypass = meter.map_or_else(
        || transient.unwrap_or(proc.authored_bypass),
        |m| m.effective_bypass,
    );
    let active_audio = !effective_bypass;

    // Header strip of Focused Device Inspector
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (dot, _) = ui.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
        ui.painter().circle_filled(
            dot.center(),
            4.5,
            if active_audio {
                accent
            } else {
                theme::TEXT_MUTED
            },
        );
        ui.label(
            RichText::new(format!(
                "INSPECTOR · {}",
                theme::processor_title(proc.kind).to_uppercase()
            ))
            .strong()
            .size(11.5)
            .color(theme::TEXT_PRIMARY),
        );
        ui.label(
            RichText::new(format!(
                "{} / {} · Latency {} smp",
                proc.owner, proc.id, proc.output_latency_frames
            ))
            .monospace()
            .size(10.0)
            .color(theme::TEXT_SECONDARY),
        );
        if !proc.sidechain_inputs.is_empty() {
            ui.label(
                RichText::new(format!("SIDECHAIN {}", proc.sidechain_inputs.join(", ")))
                    .size(9.5)
                    .strong()
                    .color(theme::ACCENT_VIOLET),
            );
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if led_button(
                ui,
                if proc.authored_bypass {
                    "SOURCE OFF"
                } else {
                    "SOURCE ON"
                },
                !proc.authored_bypass,
                theme::SIG_NOMINAL,
                Vec2::new(76.0, 20.0),
                "Toggle durable source bypass · B",
            )
            .clicked()
            {
                cx.send(Action::ToggleProcessorDurableBypass {
                    processor: proc.id.clone(),
                });
            }
            if led_button(
                ui,
                if transient.is_some() {
                    "A/B ACTIVE"
                } else {
                    "A/B BYPASS"
                },
                transient.is_some(),
                theme::SIG_HOT,
                Vec2::new(78.0, 20.0),
                "Toggle transient A/B comparison bypass · b",
            )
            .clicked()
            {
                cx.send(Action::ToggleProcessorTransientBypass {
                    processor: proc.id.clone(),
                });
            }
            if led_button(
                ui,
                "Later >",
                false,
                accent,
                Vec2::new(56.0, 20.0),
                "Move processor later in chain · J",
            )
            .clicked()
            {
                cx.send(Action::MoveProcessor {
                    processor: proc.id.clone(),
                    direction: MoveDirection::Later,
                });
            }
            if led_button(
                ui,
                "< Earlier",
                false,
                accent,
                Vec2::new(62.0, 20.0),
                "Move processor earlier in chain · K",
            )
            .clicked()
            {
                cx.send(Action::MoveProcessor {
                    processor: proc.id.clone(),
                    direction: MoveDirection::Earlier,
                });
            }
        });
    });

    ui.add_space(6.0);

    // Split body: Left 62% Large Studio Analyzer | Right 38% Primary Precision Knobs & Meters
    let avail_w = ui.available_width();
    let avail_h = ui.available_height().max(150.0);
    let left_w = (avail_w * 0.60).max(380.0);
    let right_w = (avail_w - left_w - 12.0).max(240.0);

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;

        // Left: Large High-Res FabFilter-style Interactive Analyzer
        ui.allocate_ui(Vec2::new(left_w, avail_h), |ui| {
            paint_deep_analyzer(
                ui,
                proc,
                meter,
                accent,
                active_audio,
                Vec2::new(left_w, avail_h),
            );
        });

        // Right: Large Precision Knobs for the Focused Device's Primary Parameters
        egui::Frame::new()
            .fill(theme::BG_WELL)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
            .corner_radius(5.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(right_w - 20.0);
                ui.set_min_height(avail_h - 16.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("PRECISION CONTROLS")
                                .size(9.5)
                                .strong()
                                .color(theme::TEXT_MUTED),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(m) = meter {
                                ui.label(
                                    RichText::new(format!(
                                        "IN {} · OUT {} · GR {:.1}dB",
                                        theme::format_dbfs(m.input_peak),
                                        theme::format_dbfs(m.output_peak),
                                        m.gain_reduction_db
                                    ))
                                    .monospace()
                                    .size(9.5)
                                    .color(theme::TEXT_SECONDARY),
                                );
                            }
                        });
                    });

                    let mut continuous: Vec<_> = proc
                        .settings
                        .iter()
                        .filter_map(|s| {
                            Some((
                                s,
                                s.unit?,
                                s.value?,
                                s.minimum.unwrap_or(0.0),
                                s.maximum.unwrap_or(1.0),
                            ))
                        })
                        .collect();
                    // Show authored settings first, up to 4 large hero knobs
                    continuous.sort_by_key(|(s, ..)| !s.authored);
                    let hero_count = continuous.len().clamp(1, 4);
                    let slot_w = ((right_w - 24.0) / hero_count as f32).max(66.0);

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (setting, unit, value, min, max) in continuous.iter().take(4) {
                            ui.allocate_ui_with_layout(
                                Vec2::new(slot_w, 96.0),
                                Layout::top_down(Align::Center),
                                |ui| {
                                    let default_val =
                                        sensible_default_for(&setting.key, *unit, *min, *max);
                                    let out = rotary_knob(
                                        ui,
                                        cx.state,
                                        &format!("insp-{}-{}", proc.id, setting.key),
                                        &short_setting_label(&setting.key),
                                        *value,
                                        *min,
                                        *max,
                                        default_val,
                                        *unit,
                                        if active_audio {
                                            accent
                                        } else {
                                            theme::TEXT_MUTED
                                        },
                                        None,
                                        &format!("{}.{}", proc.id, setting.key),
                                        52.0,
                                    );
                                    if out.right_clicked {
                                        cx.send(Action::SelectProcessor(selected_idx));
                                        cx.send(Action::OpenExactEdit);
                                    }
                                    if let Some(new_val) = out.committed {
                                        cx.send(Action::SetProcessorSettingValue {
                                            processor: proc.id.clone(),
                                            setting: setting.key.clone(),
                                            value: theme::format_crump_literal(new_val, *unit),
                                        });
                                    }
                                },
                            );
                        }
                    });

                    ui.label(
                        RichText::new(
                            "Drag vertically · Shift fine · Double-click reset · Right-click exact",
                        )
                        .size(9.5)
                        .color(theme::TEXT_MUTED),
                    );
                });
            });
    });
}

/// High-resolution FabFilter-style analyzer canvas in the Focused Device Inspector.
fn paint_deep_analyzer(
    ui: &mut Ui,
    proc: &CompiledProcessorInspection,
    meter: Option<crate::engine::mix_graph::ProcessorMeter>,
    accent: Color32,
    active: bool,
    size: Vec2,
) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 5.0, theme::BG_WELL);
    painter.rect_stroke(
        rect,
        5.0,
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    let in_peak = meter.map_or(0.0, |m| m.input_peak);
    let out_peak = meter.map_or(0.0, |m| m.output_peak);
    let gr_db = meter.map_or(0.0, |m| m.gain_reduction_db);
    let max_gr = meter.map_or(0.0, |m| m.maximum_gain_reduction_db);

    // Input & Output tall studio meters
    let in_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.top() + 18.0),
        Pos2::new(rect.left() + 20.0, rect.bottom() - 10.0),
    );
    let out_rect = Rect::from_min_max(
        Pos2::new(rect.right() - 20.0, rect.top() + 18.0),
        Pos2::new(rect.right() - 8.0, rect.bottom() - 10.0),
    );
    painter.text(
        Pos2::new(in_rect.center().x, rect.top() + 8.0),
        Align2::CENTER_CENTER,
        "IN",
        FontId::monospace(8.5),
        theme::TEXT_MUTED,
    );
    painter.text(
        Pos2::new(out_rect.center().x, rect.top() + 8.0),
        Align2::CENTER_CENTER,
        "OUT",
        FontId::monospace(8.5),
        theme::TEXT_MUTED,
    );
    paint_dbfs_meter_vertical(painter, in_rect, in_peak, in_peak * 0.72);
    paint_dbfs_meter_vertical(painter, out_rect, out_peak, out_peak * 0.72);

    let plot = Rect::from_min_max(
        Pos2::new(in_rect.right() + 34.0, rect.top() + 12.0),
        Pos2::new(out_rect.left() - 14.0, rect.bottom() - 20.0),
    );
    let curve_color = if active { accent } else { theme::TEXT_MUTED };

    match proc.kind {
        ProcessorKind::Eq => {
            // dB axis labels on left
            for (db, lbl) in [
                (12.0, "+12"),
                (6.0, "+6"),
                (0.0, "0"),
                (-6.0, "-6"),
                (-12.0, "-12"),
            ] {
                let y = plot.center().y - (db / 15.0) * (plot.height() * 0.45);
                painter.hline(
                    plot.x_range(),
                    y,
                    Stroke::new(
                        1.0_f32,
                        if db == 0.0 {
                            theme::BORDER_STRONG
                        } else {
                            theme::GRID_BEAT
                        },
                    ),
                );
                painter.text(
                    Pos2::new(plot.left() - 5.0, y),
                    Align2::RIGHT_CENTER,
                    lbl,
                    FontId::monospace(8.5),
                    theme::TEXT_MUTED,
                );
            }
            // Logarithmic frequency gridlines & labels
            for &(freq, lbl) in &[
                (20.0_f64, "20Hz"),
                (100.0, "100"),
                (500.0, "500"),
                (1000.0, "1k"),
                (5000.0, "5k"),
                (20000.0, "20k"),
            ] {
                let t = ((freq / 20.0).ln() / (1000.0_f64).ln()).clamp(0.0, 1.0) as f32;
                let x = plot.left() + t * plot.width();
                painter.vline(x, plot.y_range(), Stroke::new(1.0_f32, theme::GRID_BEAT));
                painter.text(
                    Pos2::new(x, plot.bottom() + 4.0),
                    Align2::CENTER_TOP,
                    lbl,
                    FontId::monospace(8.5),
                    theme::TEXT_MUTED,
                );
            }
            paint_eq_curve_detailed(painter, plot, proc, curve_color);
        }
        ProcessorKind::Compressor | ProcessorKind::Gate | ProcessorKind::Limiter => {
            let gr_rect = Rect::from_min_max(
                Pos2::new(plot.right() - 16.0, plot.top()),
                plot.right_bottom(),
            );
            painter.text(
                Pos2::new(gr_rect.center().x, rect.top() + 8.0),
                Align2::CENTER_CENTER,
                "GR",
                FontId::monospace(8.5),
                theme::ACCENT_AMBER,
            );
            paint_gr_meter_vertical(painter, gr_rect, gr_db, max_gr);
            let graph = Rect::from_min_max(
                plot.left_top(),
                Pos2::new(gr_rect.left() - 12.0, plot.bottom()),
            );
            paint_dynamics_preview(painter, graph, proc, curve_color, gr_db, in_peak, true);
        }
        _ => {
            paint_fx_transfer_preview(painter, plot, proc, curve_color, true);
        }
    }
}

fn short_setting_label(key: &str) -> String {
    let clean = key
        .replace("_db", "")
        .replace("_hz", "")
        .replace("_ms", "")
        .replace("_seconds", "")
        .replace("_ratio", "")
        .replace('_', " ");
    // Abbreviate long words the way hardware panels do instead of cutting mid-word.
    let words: Vec<String> = clean
        .split_whitespace()
        .map(|word| match word {
            "frequency" | "freq" => "FREQ".to_owned(),
            "resonance" => "RES".to_owned(),
            "threshold" => "THRESH".to_owned(),
            "release" if clean.len() > 10 => "REL".to_owned(),
            "attack" if clean.len() > 10 => "ATK".to_owned(),
            "feedback" => "FDBK".to_owned(),
            "lookahead" => "LOOKAHEAD".to_owned(),
            other => other.to_uppercase(),
        })
        .collect();
    let up = words.join(" ");
    if up.chars().count() > 12 {
        let mut short: String = up.chars().take(11).collect();
        short.push('…');
        short
    } else {
        up
    }
}

/// Paints a compact domain-specific visualizer bay inside each FX device card.
fn visualizer_bay(
    ui: &mut Ui,
    proc: &CompiledProcessorInspection,
    meter: Option<crate::engine::mix_graph::ProcessorMeter>,
    accent: Color32,
    active: bool,
    bay_h: f32,
) {
    let total_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total_w, bay_h), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, 4.0, theme::BG_WELL);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    let in_peak = meter.map_or(0.0, |m| m.input_peak);
    let out_peak = meter.map_or(0.0, |m| m.output_peak);
    let gr_db = meter.map_or(0.0, |m| m.gain_reduction_db);
    let max_gr = meter.map_or(0.0, |m| m.maximum_gain_reduction_db);

    let in_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 4.0, rect.top() + 4.0),
        Pos2::new(rect.left() + 13.0, rect.bottom() - 4.0),
    );
    let out_rect = Rect::from_min_max(
        Pos2::new(rect.right() - 13.0, rect.top() + 4.0),
        Pos2::new(rect.right() - 4.0, rect.bottom() - 4.0),
    );
    paint_dbfs_meter_vertical(painter, in_rect, in_peak, in_peak * 0.72);
    paint_dbfs_meter_vertical(painter, out_rect, out_peak, out_peak * 0.72);

    let center = Rect::from_min_max(
        Pos2::new(in_rect.right() + 6.0, rect.top() + 4.0),
        Pos2::new(out_rect.left() - 6.0, rect.bottom() - 4.0),
    );

    let curve_color = if active { accent } else { theme::TEXT_MUTED };

    match proc.kind {
        ProcessorKind::Eq => {
            let mid_y = center.center().y;
            painter.hline(
                center.x_range(),
                mid_y,
                Stroke::new(1.0_f32, theme::GRID_BAR),
            );
            for frac in [0.22, 0.5, 0.78] {
                painter.vline(
                    center.left() + center.width() * frac,
                    center.y_range(),
                    Stroke::new(1.0_f32, theme::GRID_BEAT),
                );
            }
            paint_eq_curve_detailed(painter, center, proc, curve_color);
            painter.text(
                Pos2::new(center.left() + 3.0, center.bottom() - 1.0),
                Align2::LEFT_BOTTOM,
                "20Hz",
                FontId::monospace(8.0),
                theme::TEXT_MUTED,
            );
            painter.text(
                Pos2::new(center.right() - 3.0, center.bottom() - 1.0),
                Align2::RIGHT_BOTTOM,
                "20k",
                FontId::monospace(8.0),
                theme::TEXT_MUTED,
            );
        }
        ProcessorKind::Compressor | ProcessorKind::Gate | ProcessorKind::Limiter => {
            let gr_rect = Rect::from_min_max(
                Pos2::new(center.right() - 11.0, center.top()),
                center.right_bottom(),
            );
            paint_gr_meter_vertical(painter, gr_rect, gr_db, max_gr);
            let graph_rect = Rect::from_min_max(
                center.left_top(),
                Pos2::new(gr_rect.left() - 6.0, center.bottom()),
            );
            paint_dynamics_preview(
                painter,
                graph_rect,
                proc,
                curve_color,
                gr_db,
                in_peak,
                false,
            );
        }
        _ => {
            paint_fx_transfer_preview(painter, center, proc, curve_color, false);
        }
    }
}

fn setting_val(proc: &CompiledProcessorInspection, needle: &str) -> Option<f64> {
    proc.settings
        .iter()
        .find(|s| s.key.contains(needle))
        .and_then(|s| s.value)
}

/// Evaluates and paints the EQ frequency response driven by actual processor settings + band nodes.
fn paint_eq_curve_detailed(
    painter: &egui::Painter,
    rect: Rect,
    proc: &CompiledProcessorInspection,
    color: Color32,
) {
    let mid_y = rect.center().y;
    let hpf = setting_val(proc, "highpass")
        .or_else(|| setting_val(proc, "hp_"))
        .unwrap_or(20.0);
    let lpf = setting_val(proc, "lowpass")
        .or_else(|| setting_val(proc, "lp_"))
        .unwrap_or(20000.0);
    let low_freq = setting_val(proc, "low_freq").unwrap_or(120.0);
    let low_gain = setting_val(proc, "low_gain")
        .or_else(|| setting_val(proc, "low_shelf"))
        .unwrap_or(0.0);
    let mid_freq = setting_val(proc, "mid_freq")
        .or_else(|| setting_val(proc, "bell_freq"))
        .or_else(|| setting_val(proc, "freq"))
        .unwrap_or(1000.0);
    let mid_gain = setting_val(proc, "mid_gain")
        .or_else(|| setting_val(proc, "bell_gain"))
        .or_else(|| setting_val(proc, "gain"))
        .unwrap_or(0.0);
    let mid_q = setting_val(proc, "mid_q")
        .or_else(|| setting_val(proc, "q"))
        .unwrap_or(1.2)
        .clamp(0.3, 8.0);
    let high_freq = setting_val(proc, "high_freq").unwrap_or(5500.0);
    let high_gain = setting_val(proc, "high_gain")
        .or_else(|| setting_val(proc, "high_shelf"))
        .unwrap_or(0.0);

    let eval_db = |freq: f64| -> f64 {
        let mut db = 0.0_f64;
        if hpf > 24.0 && freq < hpf * 1.5 {
            let ratio = (freq / hpf).clamp(0.08, 2.0);
            db -= (1.0 / (ratio * ratio) - 0.45).max(0.0) * 6.0;
        }
        if lpf < 18500.0 && freq > lpf * 0.7 {
            let ratio = (lpf / freq).clamp(0.08, 2.0);
            db -= (1.0 / (ratio * ratio) - 0.45).max(0.0) * 6.0;
        }
        let low_w = 1.0 / (1.0 + (freq / low_freq).powi(2));
        let high_w = 1.0 / (1.0 + (high_freq / freq).powi(2));
        let mid_dist = (freq / mid_freq).ln();
        let mid_w = (-mid_dist * mid_dist * (mid_q * 1.6)).exp();
        db + low_gain * low_w + mid_gain * mid_w + high_gain * high_w
    };

    let steps = 72;
    let mut pts = Vec::with_capacity(steps + 1);
    let mut prev_x = rect.left();
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let freq = 20.0_f64 * (1000.0_f64).powf(f64::from(t));
        let db = eval_db(freq);
        let norm_y = (db.clamp(-15.0, 15.0) / 15.0) as f32;
        let x = rect.left() + t * rect.width();
        let y =
            (mid_y - norm_y * (rect.height() * 0.44)).clamp(rect.top() + 2.0, rect.bottom() - 2.0);
        // Subtle shaded fill between curve and 0 dB center line
        if i > 0 && (y - mid_y).abs() > 1.0 {
            let top_y = y.min(mid_y);
            let bot_y = y.max(mid_y);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(prev_x, top_y), Pos2::new(x, bot_y)),
                0.0,
                with_alpha(color, 0.16),
            );
        }
        prev_x = x;
        pts.push(Pos2::new(x, y));
    }
    painter.add(egui::Shape::line(pts, Stroke::new(2.0_f32, color)));

    // Glowing band control nodes when tall enough
    if rect.height() >= 80.0 {
        for &(freq, gain, tag) in &[
            (low_freq, low_gain, "L"),
            (mid_freq, mid_gain, "M"),
            (high_freq, high_gain, "H"),
        ] {
            if gain.abs() > 0.05 || freq > 25.0 {
                let t = ((freq / 20.0).ln() / (1000.0_f64).ln()).clamp(0.02, 0.98) as f32;
                let db = eval_db(freq);
                let norm_y = (db.clamp(-15.0, 15.0) / 15.0) as f32;
                let px = rect.left() + t * rect.width();
                let py = (mid_y - norm_y * (rect.height() * 0.44))
                    .clamp(rect.top() + 6.0, rect.bottom() - 6.0);
                painter.circle_filled(Pos2::new(px, py), 6.0, with_alpha(color, 0.28));
                painter.circle_filled(Pos2::new(px, py), 3.5, color);
                painter.text(
                    Pos2::new(px, py - 9.0),
                    Align2::CENTER_BOTTOM,
                    format!("{tag} {:+.1}dB", gain),
                    FontId::monospace(8.5),
                    theme::TEXT_PRIMARY,
                );
            }
        }
    }
}

/// Distinct, physically accurate Dynamics transfer curves for Compressor, Gate, and Limiter
/// plus a live input-level indicator ball along the curve.
fn paint_dynamics_preview(
    painter: &egui::Painter,
    rect: Rect,
    proc: &CompiledProcessorInspection,
    color: Color32,
    gr_db: f32,
    in_peak: f32,
    show_axes: bool,
) {
    // 1:1 unity reference diagonal + subtle grid
    for frac in [0.25, 0.5, 0.75] {
        painter.hline(
            rect.x_range(),
            rect.top() + rect.height() * frac,
            Stroke::new(1.0_f32, theme::GRID_BEAT),
        );
        painter.vline(
            rect.left() + rect.width() * frac,
            rect.y_range(),
            Stroke::new(1.0_f32, theme::GRID_BEAT),
        );
    }
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(1.0_f32, theme::GRID_BAR),
    );

    let thresh = setting_val(proc, "threshold")
        .or_else(|| setting_val(proc, "ceiling"))
        .unwrap_or(match proc.kind {
            ProcessorKind::Limiter => -1.0,
            ProcessorKind::Gate => -32.0,
            _ => -18.0,
        })
        .clamp(-48.0, 0.0);
    let ratio = setting_val(proc, "ratio")
        .unwrap_or(match proc.kind {
            ProcessorKind::Limiter => 20.0,
            ProcessorKind::Gate => 8.0,
            _ => 4.0,
        })
        .clamp(1.0, 20.0);
    let knee = setting_val(proc, "knee").unwrap_or(6.0).clamp(0.0, 18.0);

    let eval_out_db = |in_db: f64| -> f64 {
        match proc.kind {
            ProcessorKind::Gate => {
                if in_db >= thresh {
                    in_db
                } else {
                    (thresh - (thresh - in_db) * ratio).max(-48.0)
                }
            }
            ProcessorKind::Limiter => in_db.min(thresh),
            _ => {
                // Smooth soft-knee compression curve
                let half_k = (knee * 0.5).max(0.5);
                if in_db <= thresh - half_k {
                    in_db
                } else if in_db >= thresh + half_k {
                    thresh + (in_db - thresh) / ratio
                } else {
                    let x = in_db - (thresh - half_k);
                    in_db + (1.0 / ratio - 1.0) * (x * x) / (4.0 * half_k)
                }
            }
        }
    };

    let to_pos = |in_db: f64, out_db: f64| -> Pos2 {
        let tx = ((in_db + 48.0) / 48.0).clamp(0.0, 1.0) as f32;
        let ty = ((out_db + 48.0) / 48.0).clamp(0.0, 1.0) as f32;
        Pos2::new(
            rect.left() + tx * rect.width(),
            rect.bottom() - ty * rect.height(),
        )
    };

    let steps = 48;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let in_db = -48.0 + (i as f64 / steps as f64) * 48.0;
            to_pos(in_db, eval_out_db(in_db))
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(2.0_f32, color)));

    // Threshold marker dot
    let knee_pos = to_pos(thresh, eval_out_db(thresh));
    painter.circle_filled(knee_pos, 3.2, color);

    // Live input-level tracking ball along the transfer curve
    if in_peak > 0.005 {
        let in_db = f64::from(theme::amp_to_dbfs(in_peak)).clamp(-48.0, 0.0);
        let live_pos = to_pos(in_db, eval_out_db(in_db));
        painter.circle_filled(live_pos, 5.5, with_alpha(theme::ACCENT_AMBER, 0.35));
        painter.circle_filled(live_pos, 3.0, theme::ACCENT_AMBER);
    }

    painter.text(
        Pos2::new(rect.left() + 4.0, rect.top() + 3.0),
        Align2::LEFT_TOP,
        format!("GR {gr_db:.1} dB"),
        FontId::monospace(9.5),
        if gr_db.abs() > 0.3 {
            theme::ACCENT_AMBER
        } else {
            theme::TEXT_MUTED
        },
    );

    if show_axes {
        for &(db, lbl) in &[(-48.0, "-48"), (-24.0, "-24"), (-12.0, "-12"), (0.0, "0dB")] {
            let p = to_pos(db, -48.0);
            painter.text(
                Pos2::new(p.x, rect.bottom() + 4.0),
                Align2::CENTER_TOP,
                lbl,
                FontId::monospace(8.5),
                theme::TEXT_MUTED,
            );
        }
        painter.text(
            Pos2::new(rect.right() - 4.0, rect.top() + 3.0),
            Align2::RIGHT_TOP,
            format!("THR {thresh:.0}dB · {ratio:.1}:1"),
            FontId::monospace(9.0),
            theme::TEXT_SECONDARY,
        );
    }
}

fn paint_fx_transfer_preview(
    painter: &egui::Painter,
    rect: Rect,
    proc: &CompiledProcessorInspection,
    color: Color32,
    detailed: bool,
) {
    let mid_y = rect.center().y;
    painter.hline(
        rect.x_range(),
        mid_y,
        Stroke::new(1.0_f32, theme::GRID_BEAT),
    );
    if detailed {
        for frac in [0.25, 0.5, 0.75] {
            painter.vline(
                rect.left() + rect.width() * frac,
                rect.y_range(),
                Stroke::new(1.0_f32, theme::GRID_BEAT),
            );
        }
    }
    let drive = setting_val(proc, "drive").unwrap_or(1.5).clamp(0.2, 12.0) as f32;
    let mix = setting_val(proc, "mix").unwrap_or(0.5).clamp(0.0, 1.0) as f32;
    let feedback = setting_val(proc, "feedback")
        .or_else(|| setting_val(proc, "decay"))
        .unwrap_or(0.45)
        .clamp(0.05, 0.95) as f32;
    let steps = if detailed { 80 } else { 44 };
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let x = rect.left() + t * rect.width();
            let y_norm = match proc.kind {
                ProcessorKind::Saturation => {
                    let s = (t * 2.0 - 1.0) * drive;
                    s.tanh()
                }
                ProcessorKind::Delay => {
                    let taps = 8.0_f32;
                    let phase = (t * taps).fract();
                    let idx = (t * taps).floor();
                    let env = feedback.powf(idx * 0.7);
                    if phase < 0.14 {
                        env * (1.0 - phase / 0.14)
                    } else {
                        0.0
                    }
                }
                ProcessorKind::Reverb => {
                    let decay = (-t * (3.6 - feedback * 2.4)).exp();
                    (t * 28.0).sin() * decay
                }
                _ => (t * std::f32::consts::TAU * 2.5).sin() * (0.35 + mix * 0.45),
            };
            let y = mid_y - y_norm * (rect.height() * 0.42);
            Pos2::new(x, y)
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(1.9_f32, color)));
    painter.text(
        Pos2::new(rect.left() + 4.0, rect.top() + 3.0),
        Align2::LEFT_TOP,
        format!("MIX {:.0}%", mix * 100.0),
        FontId::monospace(9.0),
        theme::TEXT_SECONDARY,
    );
}

/// Optional power-user table view for inspecting all processors in a flat list.
fn processing_table(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    graph: &crate::engine::mix_graph::MixGraphInspection,
) {
    let app = cx.app;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        toolbar(
            ui,
            cx,
            &[
                (
                    "A/B Bypass",
                    Action::ToggleTransientProcessorBypass,
                    "Transient bypass · b",
                ),
                (
                    "Source Bypass",
                    Action::ToggleDurableProcessorBypass,
                    "Authored bypass · B",
                ),
                (
                    "Edit Setting…",
                    Action::OpenExactEdit,
                    "Edit setting · Enter",
                ),
                (
                    "Earlier",
                    Action::MoveSelectedProcessor(MoveDirection::Earlier),
                    "Move earlier · K",
                ),
                (
                    "Later",
                    Action::MoveSelectedProcessor(MoveDirection::Later),
                    "Move later · J",
                ),
            ],
        );
    });
    let selected = clamp_index(app.selected_processor, graph.processors.len());
    let click = table(
        ui,
        cx.state,
        "processing",
        &[
            ("#", 32.0),
            ("Owner / ID", 220.0),
            ("Type", 100.0),
            ("Status", 110.0),
            ("Input", 130.0),
            ("Output", 130.0),
            ("GR (dB)", 84.0),
            ("Latency", 64.0),
            ("Key Settings", 260.0),
        ],
        graph.processors.len(),
        Some(selected),
        |index| {
            let proc = &graph.processors[index];
            let meter = (proc.index < app.telemetry.engine.processor_count)
                .then(|| app.telemetry.engine.processor_activity[proc.index]);
            let transient = app.audition_plan.transient_bypass.get(&proc.id).copied();
            let effective = meter.map_or_else(
                || transient.unwrap_or(proc.authored_bypass),
                |m| m.effective_bypass,
            );
            let status_cell = if transient.is_some() {
                Cell::Colored("A/B Override".to_owned(), theme::SIG_HOT)
            } else if effective {
                Cell::Colored("Bypassed".to_owned(), theme::TEXT_MUTED)
            } else {
                Cell::Colored("Active".to_owned(), theme::SIG_NOMINAL)
            };
            let settings = proc
                .settings
                .iter()
                .filter(|s| s.authored)
                .map(|s| {
                    let v = s.choice.clone().unwrap_or_else(|| {
                        s.unit.zip(s.value).map_or_else(
                            || "—".to_owned(),
                            |(u, val)| theme::format_compact_value(val, u),
                        )
                    });
                    format!("{}={v}", s.key)
                })
                .collect::<Vec<_>>()
                .join(" · ");
            vec![
                proc.index.to_string().into(),
                format!("{} / {}", proc.owner, proc.id).into(),
                Cell::Colored(
                    theme::processor_title(proc.kind).to_owned(),
                    theme::processor_color(proc.kind),
                ),
                status_cell,
                meter.map_or_else(|| "—".into(), |m| Cell::Meter(m.input_peak)),
                meter.map_or_else(|| "—".into(), |m| Cell::Meter(m.output_peak)),
                meter
                    .map_or_else(
                        || "—".to_owned(),
                        |m| {
                            format!(
                                "{:.1} / {:.1}",
                                m.gain_reduction_db, m.maximum_gain_reduction_db
                            )
                        },
                    )
                    .into(),
                format!("{} smp", proc.output_latency_frames).into(),
                settings.into(),
            ]
        },
    );
    apply_row_click(cx, click, selected);
}
