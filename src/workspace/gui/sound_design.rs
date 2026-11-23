//! Sound Design Studio: Hardware Macro Faceplate, Interactive Visual Synth Workbench (Oscillator /
//! Draggable Filter Bode / Draggable ADSR & LFO / Docked Modulation Matrix with Log-Hz Depth),
//! Auto-Fit 2D Patch Graph with Named Per-Port Jacks & Draggable Nodes, and Grouped Parameter Matrix.

use std::collections::{BTreeMap, VecDeque};
use std::f32::consts::PI;

use super::theme::{self, with_alpha};
use super::widgets::{
    context_combo, horizontal_mini_slider, led_button, pill_toggle, rotary_knob_modulated,
    section_header, unavailable,
};
use super::*;
use crate::model::{
    ControlPolarity, Graph, GraphNode, Instrument, InstrumentModulationRoute,
    InstrumentModulationSourceKind, NodeKind, Parameter, ParameterTarget, PresentationItemKind,
    PresentationSectionKind, SettingValue, Unit,
};

/// Shared studio sub-header across Faceplate, Synth, Patch Graph, and Parameters.
fn sound_subbar(ui: &mut Ui, cx: &mut Cx<'_>, project: &Project, instrument: &Instrument) {
    let app = cx.app;
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                for (view, label, key) in [
                    (View::Faceplate, "Faceplate", "5"),
                    (View::Instrument, "Synth", "6"),
                    (View::Graph, "Patch Graph", "9"),
                    (View::Parameters, "Parameters", "7"),
                ] {
                    if pill_toggle(
                        ui,
                        label,
                        app.view == view,
                        theme::ACCENT_AMBER,
                        &format!("{label} · Key {key}"),
                    )
                    .tab_pressed()
                    {
                        cx.send(Action::ShowView(view));
                    }
                }
                ui.separator();

                let names = project
                    .song
                    .instruments
                    .iter()
                    .map(|inst| inst.name.clone())
                    .collect::<Vec<_>>();
                context_combo(
                    ui,
                    cx,
                    "Instrument",
                    &names,
                    app.selected_instrument,
                    Action::CycleContext,
                );

                badge_chip(ui, &format!("{}", instrument.kind), theme::ACCENT_CYAN);
                badge_chip(
                    ui,
                    &if instrument.polyphony == 1 {
                        "Mono".to_owned()
                    } else {
                        format!("{} Voices", instrument.polyphony)
                    },
                    theme::TEXT_SECONDARY,
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if pill_toggle(
                        ui,
                        "Keys",
                        cx.state.show_keyboard,
                        theme::ACCENT_CYAN,
                        "Toggle on-screen studio keyboard",
                    )
                    .clicked()
                    {
                        cx.state.show_keyboard = !cx.state.show_keyboard;
                    }
                    if pill_toggle(
                        ui,
                        "Audition",
                        false,
                        theme::SIG_NOMINAL,
                        "Arm this Instrument for audition · A",
                    )
                    .clicked()
                    {
                        cx.send(Action::ArmSelectedAudition);
                    }
                });
            });
        });
}

fn badge_chip(ui: &mut Ui, text: &str, color: Color32) {
    let font = FontId::monospace(10.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), color);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(galley.size().x + 12.0, 19.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme::BG_WELL);
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0_f32, with_alpha(color, 0.38)),
        StrokeKind::Inside,
    );
    painter.text(rect.center(), Align2::CENTER_CENTER, text, font, color);
}

/// A small painted right-pointing arrow (the bundled fonts lack a "→" glyph).
fn route_arrow(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 19.0), Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;
    let stroke = Stroke::new(1.2_f32, theme::TEXT_MUTED);
    painter.line_segment(
        [
            Pos2::new(rect.left() + 2.0, y),
            Pos2::new(rect.right() - 3.0, y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(rect.right() - 7.0, y - 3.5),
            Pos2::new(rect.right() - 3.0, y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(rect.right() - 7.0, y + 3.5),
            Pos2::new(rect.right() - 3.0, y),
        ],
        stroke,
    );
}

/// A chip in a fixed-width column so successive rows line up; long text is truncated.
fn badge_chip_cell(ui: &mut Ui, text: &str, color: Color32, width: f32) {
    let max_chars = ((width - 12.0) / 6.2).max(3.0) as usize;
    ui.allocate_ui_with_layout(
        Vec2::new(width, 19.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_width(width);
            badge_chip(ui, &truncate_text(text, max_chars), color);
        },
    );
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        text.to_owned()
    } else if max_chars <= 1 {
        "…".to_owned()
    } else {
        let mut s: String = text.chars().take(max_chars - 1).collect();
        s.push('…');
        s
    }
}

/// Formats a `SettingValue` into clean studio text (zero raw Rust `{:?}` output).
fn format_setting_value(value: &SettingValue) -> String {
    match value {
        SettingValue::Number(v) => {
            if v.fract().abs() < 1e-4 {
                format!("{v:.0}")
            } else if v.abs() >= 100.0 {
                format!("{v:.1}")
            } else {
                format!("{v:.3}")
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_owned()
            }
        }
        SettingValue::Parameter(key) => format!("${key}"),
        SettingValue::Text(s) => s.clone(),
    }
}

/// Looks up the live/preview effective value for a local Instrument parameter if available.
fn effective_param_value(app: &Workspace, instrument: &Instrument, param_key: &str) -> Option<f64> {
    let (control_program, frame_position, _) = app.control_inspection();
    let inspector = control_program?;
    let target = ParameterTarget::Instrument {
        instrument: instrument.id.clone(),
        parameter: param_key.to_owned(),
    };
    let snap = inspector
        .borrow_mut()
        .snapshot_target_at(frame_position, &target)?;
    Some(snap.effective)
}

/// Computes the authored static modulation sweep `(low, high)` around `param.value` from active
/// modulation routes so knobs always display cyan modulation arcs even when transport is stopped.
fn param_static_mod_range(instrument: &Instrument, param: &Parameter) -> Option<(f64, f64)> {
    let mut neg_delta = 0.0_f64;
    let mut pos_delta = 0.0_f64;
    let mut any = false;
    for route in &instrument.modulation_routes {
        if route.bypass || route.target != param.key {
            continue;
        }
        let depth = param.unit.parse_value(&route.depth).unwrap_or(0.0);
        if depth.abs() <= 1e-7 {
            continue;
        }
        any = true;
        match route.polarity {
            ControlPolarity::Bipolar => {
                neg_delta -= depth.abs();
                pos_delta += depth.abs();
            }
            ControlPolarity::Unipolar => {
                if depth >= 0.0 {
                    pos_delta += depth;
                } else {
                    neg_delta += depth;
                }
            }
        }
    }
    if !any {
        return None;
    }
    let lo = (param.value + neg_delta).clamp(param.minimum, param.maximum);
    let hi = (param.value + pos_delta).clamp(param.minimum, param.maximum);
    if (hi - lo).abs() <= 1e-7 {
        None
    } else {
        Some((lo, hi))
    }
}

/// Checks if a parameter is automated in any Song Automation lane, returning its lane index.
fn find_automation_lane(project: &Project, instrument_id: &str, param_key: &str) -> Option<usize> {
    let expected = format!("instrument/{instrument_id}/{param_key}");
    project
        .song
        .automations
        .iter()
        .position(|lane| lane.target.to_string() == expected)
}

// ── 1. Hardware Macro Faceplate ───────────────────────────────────────────────────

pub(super) fn faceplate(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Instrument Faceplate unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        unavailable(ui, "No Instrument");
        return;
    };

    sound_subbar(ui, cx, project, instrument);

    let selected = clamp_index(app.selected_parameter, instrument.faceplate.len());
    let mut pending_actions: Vec<Action> = Vec::new();
    let mut new_selected: Option<usize> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(10.0);
            let avail_w = ui.available_width();
            let deck_w = (avail_w - 28.0).clamp(580.0, 1140.0);
            let side_pad = ((avail_w - deck_w) * 0.5).max(12.0);

            ui.horizontal(|ui| {
                ui.add_space(side_pad);
                ui.vertical(|ui| {
                    ui.set_width(deck_w);

                    // Main Hardware Macro Synth Chassis
                    egui::Frame::new()
                        .fill(theme::BG_SURFACE)
                        .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::same(14))
                        .show(ui, |ui| {
                            // Chassis nameplate header
                            ui.horizontal(|ui| {
                                let (dot, _) =
                                    ui.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
                                ui.painter().circle_filled(
                                    dot.center(),
                                    4.0,
                                    theme::ACCENT_AMBER,
                                );
                                ui.label(
                                    RichText::new(instrument.name.to_uppercase())
                                        .strong()
                                        .size(14.0)
                                        .color(theme::TEXT_PRIMARY),
                                );
                                ui.label(
                                    RichText::new("MACRO FACEPLATE")
                                        .size(10.5)
                                        .color(theme::TEXT_MUTED),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if !instrument.presentation.tags.is_empty() {
                                        for tag in instrument.presentation.tags.iter().take(4) {
                                            badge_chip(ui, tag, theme::ACCENT_VIOLET);
                                        }
                                    }
                                });
                            });
                            ui.add_space(10.0);

                            if instrument.faceplate.is_empty() {
                                ui.label(
                                    RichText::new(
                                        "No Faceplate macros authored — edit parameters below or open Synth.",
                                    )
                                    .color(theme::TEXT_SECONDARY),
                                );
                            } else {
                                // Dynamic equal-width columns so 2, 3, or 4 macros span 100% evenly
                                let cols = instrument.faceplate.len().clamp(1, 4);
                                for row_chunk in instrument.faceplate.chunks(cols) {
                                    let row_start =
                                        (row_chunk.as_ptr() as usize
                                            - instrument.faceplate.as_ptr() as usize)
                                            / std::mem::size_of_val(&instrument.faceplate[0]);
                                    ui.columns(cols, |columns| {
                                        for (col_idx, control) in row_chunk.iter().enumerate() {
                                            let index = row_start + col_idx;
                                            let parameter = instrument
                                                .parameters
                                                .iter()
                                                .find(|p| p.key == control.parameter);
                                            let is_sel = index == selected;
                                            let mod_eff = parameter.and_then(|p| {
                                                effective_param_value(app, instrument, &p.key)
                                            });
                                            let mod_range = parameter.and_then(|p| {
                                                param_static_mod_range(instrument, p)
                                            });
                                            let mod_count = instrument
                                                .modulation_routes
                                                .iter()
                                                .filter(|r| {
                                                    r.target == control.parameter && !r.bypass
                                                })
                                                .count();
                                            let is_auto = find_automation_lane(
                                                project,
                                                &instrument.id,
                                                &control.parameter,
                                            )
                                            .is_some();

                                            let card_frame = egui::Frame::new()
                                                .fill(if is_sel {
                                                    theme::BG_CARD
                                                } else {
                                                    theme::BG_CANVAS
                                                })
                                                .stroke(Stroke::new(
                                                    if is_sel { 1.4_f32 } else { 1.0_f32 },
                                                    if is_sel {
                                                        theme::ACCENT_AMBER
                                                    } else {
                                                        theme::BORDER_SUBTLE
                                                    },
                                                ))
                                                .corner_radius(6.0)
                                                .inner_margin(egui::Margin::symmetric(12, 12));

                                            let resp = card_frame.show(&mut columns[col_idx], |ui| {
                                                ui.set_min_height(150.0);
                                                ui.vertical_centered(|ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.label(
                                                            RichText::new(
                                                                control.label.to_uppercase(),
                                                            )
                                                            .strong()
                                                            .size(11.5)
                                                            .color(if is_sel {
                                                                theme::ACCENT_AMBER
                                                            } else {
                                                                theme::TEXT_PRIMARY
                                                            }),
                                                        );
                                                        ui.with_layout(
                                                            Layout::right_to_left(Align::Center),
                                                            |ui| {
                                                                if mod_count > 0 {
                                                                    badge_chip(
                                                                        ui,
                                                                        &format!("MOD×{mod_count}"),
                                                                        theme::ACCENT_CYAN,
                                                                    );
                                                                }
                                                                if is_auto {
                                                                    badge_chip(
                                                                        ui,
                                                                        "AUTO",
                                                                        theme::SIG_NOMINAL,
                                                                    );
                                                                }
                                                            },
                                                        );
                                                    });
                                                    ui.add_space(6.0);

                                                    if let Some(param) = parameter {
                                                        let drag_key = format!(
                                                            "fp:{}:{}",
                                                            instrument.id, param.key
                                                        );
                                                        let out = rotary_knob_modulated(
                                                            ui,
                                                            cx.state,
                                                            &drag_key,
                                                            "",
                                                            param.value,
                                                            param.minimum,
                                                            param.maximum,
                                                            param.default,
                                                            param.unit,
                                                            theme::ACCENT_AMBER,
                                                            mod_range,
                                                            mod_eff,
                                                            &control.explanation,
                                                            // Macro knobs scale with their card
                                                            // so wide cards don't float a tiny dial.
                                                            (ui.available_width() * 0.16)
                                                                .clamp(68.0, 104.0),
                                                        );
                                                        if out.clicked || out.committed.is_some() {
                                                            new_selected = Some(index);
                                                        }
                                                        if let Some(v) = out.committed {
                                                            pending_actions.push(
                                                                Action::SetInstrumentParameterValue {
                                                                    instrument: instrument
                                                                        .id
                                                                        .clone(),
                                                                    parameter: param.key.clone(),
                                                                    value: v,
                                                                },
                                                            );
                                                        }
                                                        if out.right_clicked {
                                                            new_selected = Some(index);
                                                            pending_actions
                                                                .push(Action::OpenExactEdit);
                                                        }
                                                    } else {
                                                        ui.label(
                                                            RichText::new("Unbound")
                                                                .color(theme::SIG_CLIP),
                                                        );
                                                    }

                                                    ui.add_space(6.0);
                                                    ui.label(
                                                        RichText::new(truncate_text(
                                                            &control.explanation,
                                                            64,
                                                        ))
                                                        .size(10.0)
                                                        .color(theme::TEXT_MUTED),
                                                    );
                                                });
                                            });
                                            if resp.response.interact(Sense::click()).clicked() {
                                                new_selected = Some(index);
                                            }
                                        }
                                    });
                                    ui.add_space(8.0);
                                }
                            }
                        });

                    ui.add_space(10.0);

                    // Middle Row: Equalized Tone/Envelope Scopes + Quick Parameter Deck
                    ui.columns(2, |cols| {
                        // Left card: Filter & Envelope visual readout
                        egui::Frame::new()
                            .fill(theme::BG_SURFACE)
                            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::same(12))
                            .show(&mut cols[0], |ui| {
                                ui.set_min_height(152.0);
                                ui.horizontal(|ui| {
                                    section_header(ui, "TONE & ENVELOPE RESPONSE", theme::BORDER_STRONG);
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("Open Synth >")
                                                        .size(10.5)
                                                        .color(theme::ACCENT_CYAN),
                                                )
                                                .frame(false),
                                            )
                                            .clicked()
                                        {
                                            pending_actions
                                                .push(Action::ShowView(View::Instrument));
                                        }
                                    });
                                });
                                ui.add_space(6.0);
                                let half_w = ((ui.available_width() - 8.0) * 0.5).max(120.0);
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new("LPF BODE · 20Hz – 20kHz")
                                                .monospace()
                                                .size(9.0)
                                                .color(theme::ACCENT_CYAN),
                                        );
                                        paint_instrument_filter_scope(
                                            ui,
                                            app,
                                            cx.state,
                                            instrument,
                                            half_w,
                                            104.0,
                                            &mut pending_actions,
                                        );
                                    });
                                    ui.add_space(8.0);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new("AMP ENV · ADSR / LFO")
                                                .monospace()
                                                .size(9.0)
                                                .color(theme::ACCENT_VIOLET),
                                        );
                                        paint_instrument_adsr_scope(
                                            ui,
                                            cx.state,
                                            instrument,
                                            half_w,
                                            104.0,
                                            &mut pending_actions,
                                        );
                                    });
                                });
                            });

                        // Right card: Quick Parameters
                        egui::Frame::new()
                            .fill(theme::BG_SURFACE)
                            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::same(12))
                            .show(&mut cols[1], |ui| {
                                ui.set_min_height(152.0);
                                ui.horizontal(|ui| {
                                    section_header(ui, "QUICK PARAMETERS", theme::BORDER_STRONG);
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(format!(
                                                        "All {} Params >",
                                                        instrument.parameters.len()
                                                    ))
                                                    .size(10.0)
                                                    .color(theme::ACCENT_AMBER),
                                                )
                                                .frame(false),
                                            )
                                            .clicked()
                                        {
                                            pending_actions
                                                .push(Action::ShowView(View::Parameters));
                                        }
                                    });
                                });
                                ui.add_space(6.0);
                                let slider_w = (ui.available_width() - 4.0).max(140.0);
                                for param in instrument.parameters.iter().take(4) {
                                    let drag_key = format!("fpq:{}:{}", instrument.id, param.key);
                                    let out = horizontal_mini_slider(
                                        ui,
                                        cx.state,
                                        &drag_key,
                                        &param.name,
                                        param.value,
                                        param.minimum,
                                        param.maximum,
                                        param.default,
                                        param.unit,
                                        theme::ACCENT_CYAN,
                                        slider_w,
                                        23.0,
                                    );
                                    if let Some(v) = out.committed {
                                        pending_actions.push(
                                            Action::SetInstrumentParameterValue {
                                                instrument: instrument.id.clone(),
                                                parameter: param.key.clone(),
                                                value: v,
                                            },
                                        );
                                    }
                                    ui.add_space(4.0);
                                }
                            });
                    });

                    ui.add_space(10.0);

                    // Lower Dock: Active Modulation & Signal Path Summary Strip (eliminates lower void)
                    egui::Frame::new()
                        .fill(theme::BG_SURFACE)
                        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            // Hold the panel to the deck width so its edges line up with the
                            // panels above it.
                            ui.set_width(deck_w - 26.0);
                            ui.horizontal(|ui| {
                                section_header(ui, "ACTIVE MODULATION & ROUTING", theme::ACCENT_VIOLET);
                                ui.label(
                                    RichText::new(format!(
                                        "{} sources · {} routes",
                                        instrument.modulation_sources.len(),
                                        instrument.modulation_routes.len()
                                    ))
                                    .size(10.0)
                                    .color(theme::TEXT_MUTED),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("Open Patch Graph >")
                                                    .size(10.5)
                                                    .color(theme::ACCENT_CYAN),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        pending_actions.push(Action::ShowView(View::Graph));
                                    }
                                });
                            });
                            ui.add_space(6.0);
                            if instrument.modulation_routes.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new("Signal chain:")
                                            .size(11.0)
                                            .color(theme::TEXT_MUTED),
                                    );
                                    ui.label(
                                        RichText::new(summarize_osc_nodes(instrument))
                                            .size(11.0)
                                            .color(theme::TEXT_SECONDARY),
                                    );
                                });
                            } else {
                                for route in instrument.modulation_routes.iter().take(4) {
                                    draw_mod_route_row(
                                        ui,
                                        cx.state,
                                        instrument,
                                        route,
                                        None,
                                        &mut pending_actions,
                                    );
                                    ui.add_space(4.0);
                                }
                            }
                        });
                });
            });
            ui.add_space(12.0);
        });

    if let Some(idx) = new_selected {
        cx.select(selected, idx);
    }
    for action in pending_actions {
        cx.send(action);
    }
}

// ── 2. Interactive Visual Synth Workbench (`View::Instrument`) ────────────────────

pub(super) fn instrument_sections(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Instrument sections unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        unavailable(ui, "No Instrument");
        return;
    };
    let sections = app.instrument_sections();

    sound_subbar(ui, cx, project, instrument);

    let mut pending_actions: Vec<Action> = Vec::new();

    // Top 3 Interactive Hardware Synth Modules (Oscillator / Draggable Filter Bode / Draggable ADSR & LFO)
    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            let osc_params = pick_module_params(
                instrument,
                &[
                    "detune", "pitch", "tune", "wave", "shape", "drive", "sub", "gain", "level",
                    "mix", "tone",
                ],
                2,
            );
            let flt_params = pick_module_params(
                instrument,
                &["cut", "freq", "filter", "res", "q", "bright", "damp"],
                2,
            );
            let env_params = pick_module_params(
                instrument,
                &[
                    "attack", "decay", "sustain", "release", "rate", "depth", "lfo", "env",
                ],
                3,
            );
            let has_filter_node = instrument_has_filter_node(instrument);

            ui.columns(3, |cols| {
                // Module 1: Oscillator / Source
                synth_module_card(
                    &mut cols[0],
                    "OSCILLATOR / SOURCE",
                    theme::ACCENT_AMBER,
                    |ui| {
                        let w = ui.available_width();
                        paint_instrument_osc_scope(ui, instrument, w, 82.0);
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(truncate_text(&summarize_osc_nodes(instrument), 44))
                                .size(10.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.add_space(6.0);
                        draw_module_knob_row(
                            ui,
                            app,
                            cx.state,
                            instrument,
                            &osc_params,
                            theme::ACCENT_AMBER,
                            "osc",
                            &mut pending_actions,
                        );
                    },
                );

                // Module 2: Filter Bode Curve (Interactive Drag + Knobs)
                synth_module_card(&mut cols[1], "FILTER RESPONSE", theme::ACCENT_CYAN, |ui| {
                    let w = ui.available_width();
                    if has_filter_node || !flt_params.is_empty() {
                        paint_instrument_filter_scope(
                            ui,
                            app,
                            cx.state,
                            instrument,
                            w,
                            82.0,
                            &mut pending_actions,
                        );
                        ui.add_space(6.0);
                        let (cut, res) = find_filter_params(instrument);
                        ui.label(
                            RichText::new(format!(
                                "Drag dot · Cut {} · Res {:.0}%",
                                theme::format_compact_value(cut, Unit::Hertz),
                                res * 100.0
                            ))
                            .size(10.0)
                            .color(theme::TEXT_SECONDARY),
                        );
                    } else {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 82.0), Sense::hover());
                        let p = ui.painter_at(rect);
                        p.rect_filled(rect, 4.0, theme::BG_WELL);
                        p.rect_stroke(
                            rect,
                            4.0,
                            Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
                            StrokeKind::Inside,
                        );
                        p.hline(
                            (rect.left() + 8.0)..=(rect.right() - 8.0),
                            rect.center().y,
                            Stroke::new(1.5_f32, with_alpha(theme::TEXT_MUTED, 0.5)),
                        );
                        p.text(
                            rect.center() - Vec2::new(0.0, 10.0),
                            Align2::CENTER_CENTER,
                            "FIXED / NO FILTER NODE",
                            FontId::monospace(10.0),
                            theme::TEXT_MUTED,
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Direct pass-through voice topology")
                                .monospace()
                                .size(10.0)
                                .color(theme::TEXT_MUTED),
                        );
                    }
                    ui.add_space(6.0);
                    draw_module_knob_row(
                        ui,
                        app,
                        cx.state,
                        instrument,
                        &flt_params,
                        theme::ACCENT_CYAN,
                        "flt",
                        &mut pending_actions,
                    );
                });

                // Module 3: Envelope & LFO (Interactive Drag + Knobs)
                synth_module_card(&mut cols[2], "ENVELOPE & MOD", theme::ACCENT_VIOLET, |ui| {
                    let w = ui.available_width();
                    paint_instrument_adsr_scope(
                        ui,
                        cx.state,
                        instrument,
                        w,
                        82.0,
                        &mut pending_actions,
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!(
                            "{} sources · {} active routes",
                            instrument.modulation_sources.len(),
                            instrument
                                .modulation_routes
                                .iter()
                                .filter(|r| !r.bypass)
                                .count()
                        ))
                        .size(10.0)
                        .color(theme::TEXT_SECONDARY),
                    );
                    ui.add_space(6.0);
                    draw_module_knob_row(
                        ui,
                        app,
                        cx.state,
                        instrument,
                        &env_params,
                        theme::ACCENT_VIOLET,
                        "env",
                        &mut pending_actions,
                    );
                });
            });
        });

    let Some(section) = sections.get(app.selected_instrument_section) else {
        unavailable(ui, "No Instrument sections");
        return;
    };

    // Section Pill Selector Bar
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .inner_margin(egui::Margin::symmetric(12, 5))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (index, sec) in sections.iter().enumerate() {
                    let active = index == app.selected_instrument_section;
                    let count = sec.rows.len();
                    let label = if count > 0 {
                        format!("{} ({})", sec.name, count)
                    } else {
                        sec.name.clone()
                    };
                    if pill_toggle(ui, &label, active, theme::ACCENT_CYAN, &sec.name).clicked()
                        && !active
                    {
                        pending_actions.push(Action::AdjustSelected(
                            index as isize - app.selected_instrument_section as isize,
                        ));
                    }
                }
            });
        });

    // Lower Half: Permanently Docked Modulation Matrix + Section Explorer (eliminates 285px dead void)
    let selected_row = clamp_index(app.selected_instrument_row, section.rows.len());
    let mut select_row: Option<usize> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(8.0);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(12, 4))
                .show(ui, |ui| {
                    // Always dock the Modulation Matrix prominently when routes exist
                    if !instrument.modulation_routes.is_empty() {
                        draw_modulation_matrix(ui, app, cx.state, instrument, &mut pending_actions);
                        ui.add_space(10.0);
                    }

                    // Section Items Header + Rows
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{} ARCHITECTURE", section.name.to_uppercase()))
                                .strong()
                                .size(10.5)
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new(format!("· {} items", section.rows.len()))
                                .monospace()
                                .size(10.0)
                                .color(theme::TEXT_MUTED),
                        );
                    });
                    ui.add_space(5.0);

                    if section.rows.is_empty() {
                        ui.label(
                            RichText::new("No authored items in this section.")
                                .color(theme::TEXT_MUTED),
                        );
                    } else {
                        for (idx, row) in section.rows.iter().enumerate() {
                            // Avoid duplicating raw modulation route rows when Mod Matrix is already shown above
                            if section.kind == PresentationSectionKind::Modulation
                                && row.kind == Some(PresentationItemKind::Route)
                                && !instrument.modulation_routes.is_empty()
                            {
                                continue;
                            }
                            let is_sel = idx == selected_row;
                            let kind_color = match row.kind {
                                Some(PresentationItemKind::Node) => theme::TEXT_SECONDARY,
                                Some(PresentationItemKind::Parameter) => theme::ACCENT_CYAN,
                                Some(PresentationItemKind::Route)
                                | Some(PresentationItemKind::Source) => theme::ACCENT_VIOLET,
                                Some(PresentationItemKind::Bus) => theme::SIG_NOMINAL,
                                _ => theme::TEXT_SECONDARY,
                            };

                            let card = egui::Frame::new()
                                .fill(if is_sel {
                                    theme::BG_CARD
                                } else {
                                    theme::BG_SURFACE
                                })
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    if is_sel {
                                        theme::SELECTION
                                    } else {
                                        theme::BORDER_SUBTLE
                                    },
                                ))
                                .corner_radius(5.0)
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let kind_str = row.kind.map_or_else(
                                            || "POLICY".to_owned(),
                                            |k| format!("{k:?}").to_uppercase(),
                                        );
                                        badge_chip(ui, &kind_str, kind_color);

                                        ui.label(
                                            RichText::new(&row.id)
                                                .strong()
                                                .size(11.5)
                                                .color(theme::TEXT_PRIMARY),
                                        );

                                        if row.kind == Some(PresentationItemKind::Parameter)
                                            && let Some(param) = instrument
                                                .parameters
                                                .iter()
                                                .find(|p| p.key == row.id)
                                        {
                                            ui.add_space(8.0);
                                            let drag_key =
                                                format!("instsec:{}:{}", instrument.id, param.key);
                                            let out = horizontal_mini_slider(
                                                ui,
                                                cx.state,
                                                &drag_key,
                                                &param.name,
                                                param.value,
                                                param.minimum,
                                                param.maximum,
                                                param.default,
                                                param.unit,
                                                theme::ACCENT_AMBER,
                                                220.0,
                                                20.0,
                                            );
                                            if let Some(v) = out.committed {
                                                pending_actions.push(
                                                    Action::SetInstrumentParameterValue {
                                                        instrument: instrument.id.clone(),
                                                        parameter: param.key.clone(),
                                                        value: v,
                                                    },
                                                );
                                            }
                                        } else {
                                            ui.label(
                                                RichText::new(clean_section_detail(&row.detail))
                                                    .size(11.0)
                                                    .color(theme::TEXT_SECONDARY),
                                            );
                                        }

                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new("Source")
                                                                .size(10.0)
                                                                .color(theme::TEXT_MUTED),
                                                        )
                                                        .fill(theme::BG_WELL),
                                                    )
                                                    .on_hover_text(&row.source)
                                                    .clicked()
                                                {
                                                    select_row = Some(idx);
                                                    pending_actions.push(Action::OpenExactEdit);
                                                }
                                            },
                                        );
                                    });
                                });
                            if card.response.interact(Sense::click()).clicked() {
                                select_row = Some(idx);
                            }
                            ui.add_space(4.0);
                        }
                    }
                });
            ui.add_space(10.0);
        });

    if let Some(idx) = select_row {
        cx.select(selected_row, idx);
    }
    for action in pending_actions {
        cx.send(action);
    }
}

fn pick_module_params<'a>(
    instrument: &'a Instrument,
    keywords: &[&str],
    limit: usize,
) -> Vec<&'a Parameter> {
    instrument
        .parameters
        .iter()
        .filter(|p| {
            let k = p.key.to_lowercase();
            let n = p.name.to_lowercase();
            keywords.iter().any(|kw| k.contains(kw) || n.contains(kw))
        })
        .take(limit)
        .collect()
}

fn instrument_has_filter_node(instrument: &Instrument) -> bool {
    let check_graph = |g: &Graph| {
        g.nodes.iter().any(|n| {
            let label = theme::node_kind_label(n.kind).to_lowercase();
            let id = n.id.to_lowercase();
            label.contains("filter")
                || label.contains("eq")
                || id.contains("filter")
                || id.contains("lpf")
                || id.contains("hpf")
        })
    };
    instrument.graph.as_ref().is_some_and(check_graph)
        || instrument.shared_graph.as_ref().is_some_and(check_graph)
}

#[allow(clippy::too_many_arguments)]
fn draw_module_knob_row(
    ui: &mut Ui,
    app: &Workspace,
    state: &mut UiState,
    instrument: &Instrument,
    params: &[&Parameter],
    accent: Color32,
    prefix: &str,
    pending_actions: &mut Vec<Action>,
) {
    if params.is_empty() {
        ui.horizontal(|ui| {
            ui.set_min_height(66.0);
            ui.label(
                RichText::new("No editable macro params bound to this stage")
                    .size(10.0)
                    .color(theme::TEXT_MUTED),
            );
        });
        return;
    }
    let cols = params.len().clamp(1, 3);
    ui.columns(cols, |columns| {
        for (i, param) in params.iter().enumerate() {
            columns[i].vertical_centered(|ui| {
                let drag_key = format!("synth:{prefix}:{}:{}", instrument.id, param.key);
                let mod_range = param_static_mod_range(instrument, param);
                let mod_eff = effective_param_value(app, instrument, &param.key);
                let out = rotary_knob_modulated(
                    ui,
                    state,
                    &drag_key,
                    &truncate_text(&param.name, 12),
                    param.value,
                    param.minimum,
                    param.maximum,
                    param.default,
                    param.unit,
                    accent,
                    mod_range,
                    mod_eff,
                    &format!("{} (${})", param.name, param.key),
                    38.0,
                );
                if let Some(v) = out.committed {
                    pending_actions.push(Action::SetInstrumentParameterValue {
                        instrument: instrument.id.clone(),
                        parameter: param.key.clone(),
                        value: v,
                    });
                }
            });
        }
    });
}

fn draw_modulation_matrix(
    ui: &mut Ui,
    app: &Workspace,
    state: &mut UiState,
    instrument: &Instrument,
    pending_actions: &mut Vec<Action>,
) {
    let (control_program, frame_position, _) = app.control_inspection();
    let live_contrib: BTreeMap<String, (Option<f64>, Unit)> = control_program
        .map(|inspector| {
            inspector
                .borrow_mut()
                .snapshot_instrument_routes_at(frame_position, &instrument.id)
                .into_iter()
                .map(|(unit, route)| (route.route, (route.contribution, unit)))
                .collect()
        })
        .unwrap_or_default();

    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                section_header(ui, "MODULATION MATRIX", theme::ACCENT_VIOLET);
                ui.label(
                    RichText::new("Drag depth bar · Double-click zero · Click power to bypass")
                        .size(10.0)
                        .color(theme::TEXT_MUTED),
                );
            });
            ui.add_space(6.0);

            for route in &instrument.modulation_routes {
                draw_mod_route_row(
                    ui,
                    state,
                    instrument,
                    route,
                    live_contrib.get(&route.id).copied(),
                    pending_actions,
                );
                ui.add_space(4.0);
            }
        });
}

fn draw_mod_route_row(
    ui: &mut Ui,
    state: &mut UiState,
    instrument: &Instrument,
    route: &InstrumentModulationRoute,
    live: Option<(Option<f64>, Unit)>,
    pending_actions: &mut Vec<Action>,
) {
    let target_param = instrument.parameters.iter().find(|p| p.key == route.target);
    let unit = target_param.map_or(Unit::Ratio, |p| p.unit);
    let max_depth = target_param
        .map(|p| (p.maximum - p.minimum).abs().max(1.0))
        .unwrap_or(1.0);
    let current_depth = unit.parse_value(&route.depth).unwrap_or(0.0);

    egui::Frame::new()
        .fill(if route.bypass {
            theme::BG_WELL
        } else {
            theme::BG_CANVAS
        })
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                if led_button(
                    ui,
                    if route.bypass { "OFF" } else { "ON" },
                    !route.bypass,
                    theme::ACCENT_VIOLET,
                    Vec2::new(34.0, 20.0),
                    "Toggle modulation route bypass",
                )
                .clicked()
                {
                    pending_actions.push(Action::ToggleInstrumentRouteBypassById {
                        instrument: instrument.id.clone(),
                        route: route.id.clone(),
                    });
                }

                badge_chip_cell(ui, &route.source, theme::ACCENT_VIOLET, 96.0);
                route_arrow(ui);
                badge_chip_cell(
                    ui,
                    target_param.map_or(&route.target, |p| &p.name),
                    theme::ACCENT_CYAN,
                    96.0,
                );
                // The depth bar takes the row's remaining width, leaving room for polarity and
                // the live readout, so bars in successive rows share both edges.
                let depth_w = (ui.available_width() - 96.0).clamp(140.0, 520.0);

                // Bipolar draggable depth bar with logarithmic perceptual fill for Unit::Hertz!
                let drag_key = format!("moddepth:{}:{}", instrument.id, route.id);
                let out = bipolar_depth_bar(
                    ui,
                    state,
                    &drag_key,
                    current_depth,
                    -max_depth,
                    max_depth,
                    target_param,
                    unit,
                    if route.bypass {
                        theme::TEXT_MUTED
                    } else {
                        theme::ACCENT_VIOLET
                    },
                    depth_w,
                    20.0,
                );
                if let Some(new_depth) = out.committed {
                    pending_actions.push(Action::SetInstrumentRouteDepth {
                        instrument: instrument.id.clone(),
                        route: route.id.clone(),
                        depth: unit.format_value(new_depth),
                    });
                }

                let pol = match route.polarity {
                    ControlPolarity::Unipolar => "UNI",
                    ControlPolarity::Bipolar => "BI",
                };
                ui.allocate_ui_with_layout(
                    Vec2::new(26.0, 19.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_width(26.0);
                        ui.label(
                            RichText::new(pol)
                                .monospace()
                                .size(9.5)
                                .color(theme::TEXT_MUTED),
                        );
                    },
                );

                if let Some((Some(contrib), u)) = live {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "mod {}",
                                theme::format_compact_value(contrib, u)
                            ))
                            .monospace()
                            .size(10.5)
                            .color(theme::ACCENT_CYAN),
                        );
                    });
                }
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn bipolar_depth_bar(
    ui: &mut Ui,
    state: &mut UiState,
    drag_key: &str,
    value: f64,
    min: f64,
    max: f64,
    target_param: Option<&Parameter>,
    unit: Unit,
    accent: Color32,
    width: f32,
    height: f32,
) -> widgets::ControlOutcome {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
    let mut current = state.drag_preview(drag_key).unwrap_or(value);
    let mut committed = None;

    if response.drag_started() {
        state.set_drag_preview(drag_key, current);
    }
    if response.dragged() {
        let dx = ui.input(|i| i.pointer.delta().x);
        if dx.abs() > 0.0 {
            let fine = ui.input(|i| i.modifiers.shift);
            let span = (max - min).abs().max(1e-6);
            let step = if fine {
                span * 0.0012
            } else {
                span / f64::from(width.max(40.0))
            };
            current = (current + f64::from(dx) * step).clamp(min, max);
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
        current = 0.0;
        state.hold_preview(drag_key, current);
        committed = Some(0.0);
    }

    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme::BG_WELL);
    let center_x = rect.center().x;

    // Perceptual visual fraction: logarithmic frequency travel for Hertz targets so e.g.
    // +1400 Hz on an 800 Hz cutoff fills ~38% of the bar instead of an imperceptible 14% sliver.
    let frac: f32 = if unit == Unit::Hertz {
        if let Some(p) = target_param {
            let base = p.value.max(20.0);
            let target_f = (base + current).clamp(p.minimum.max(20.0), p.maximum.max(21.0));
            let base_n = theme::value_to_norm(base, p.minimum, p.maximum, Unit::Hertz);
            let targ_n = theme::value_to_norm(target_f, p.minimum, p.maximum, Unit::Hertz);
            ((targ_n - base_n) * 1.45).clamp(-1.0, 1.0)
        } else {
            let sign = current.signum();
            let mag = (1.0 + current.abs() / 120.0).ln() / (1.0 + max.abs() / 120.0).ln().max(1e-6);
            (sign * mag).clamp(-1.0, 1.0) as f32
        }
    } else {
        ((current / max.abs().max(1e-6)) as f32).clamp(-1.0, 1.0)
    };

    let bar_end = center_x + frac * (rect.width() * 0.48);
    let (x0, x1) = if bar_end >= center_x {
        (center_x, bar_end)
    } else {
        (bar_end, center_x)
    };
    if (x1 - x0).abs() > 1.0 {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x0, rect.top() + 2.0),
                Pos2::new(x1, rect.bottom() - 2.0),
            ),
            2.0,
            with_alpha(accent, 0.58),
        );
    }
    painter.vline(
        center_x,
        (rect.top() + 2.0)..=(rect.bottom() - 2.0),
        Stroke::new(1.0_f32, theme::BORDER_STRONG),
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
    // The value sits on the side the fill does not grow into, so the centre line and fill edge
    // never cut through it.
    let (anchor, at) = if frac >= 0.0 {
        (
            Align2::LEFT_CENTER,
            Pos2::new(rect.left() + 6.0, rect.center().y),
        )
    } else {
        (
            Align2::RIGHT_CENTER,
            Pos2::new(rect.right() - 6.0, rect.center().y),
        )
    };
    painter.text(
        at,
        anchor,
        theme::format_compact_value(current, unit),
        FontId::monospace(10.0),
        theme::TEXT_PRIMARY,
    );

    widgets::ControlOutcome {
        committed,
        live_value: current,
        clicked: response.clicked(),
        right_clicked: response.secondary_clicked(),
    }
}

fn synth_module_card(ui: &mut Ui, title: &str, accent: Color32, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_height(198.0);
            ui.vertical(|ui| {
                section_header(ui, title, accent);
                ui.add_space(6.0);
                body(ui);
            });
        });
}

fn clean_section_detail(detail: &str) -> String {
    detail
        .replace("GraphDomain::Voice", "Voice")
        .replace("GraphDomain::Shared", "Shared FX")
        .replace("Voice graph", "Voice")
        .replace("Shared graph", "Shared FX")
}

// ── Visual Synth Scopes (Oscillator / Draggable Filter Bode / Draggable ADSR + LFO) ──

fn summarize_osc_nodes(instrument: &Instrument) -> String {
    let Some(graph) = &instrument.graph else {
        return format!("{} engine", instrument.kind);
    };
    let oscs = graph
        .nodes
        .iter()
        .filter(|n| {
            matches!(
                n.kind,
                NodeKind::Oscillator
                    | NodeKind::AnalogOscillator
                    | NodeKind::Unison
                    | NodeKind::Wavetable
                    | NodeKind::Noise
                    | NodeKind::SampleSource
                    | NodeKind::Multisample
                    | NodeKind::Granular
            )
        })
        .map(|n| {
            let wave = n
                .settings
                .iter()
                .find(|s| s.key == "waveform" || s.key == "wave" || s.key == "shape")
                .map(|s| format_setting_value(&s.value))
                .unwrap_or_else(|| theme::node_kind_label(n.kind).to_owned());
            format!("{}: {wave}", n.id)
        })
        .collect::<Vec<_>>();
    if oscs.is_empty() {
        format!("{} nodes", graph.nodes.len())
    } else {
        oscs.into_iter().take(2).collect::<Vec<_>>().join(" · ")
    }
}

fn paint_instrument_osc_scope(ui: &mut Ui, instrument: &Instrument, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::BG_WELL);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    let inner = rect.shrink(6.0);
    painter.hline(
        inner.x_range(),
        inner.center().y,
        Stroke::new(1.0_f32, theme::GRID_BEAT),
    );
    painter.vline(
        inner.center().x,
        inner.y_range(),
        Stroke::new(1.0_f32, theme::GRID_SUB),
    );

    let wave_kind = instrument
        .graph
        .as_ref()
        .and_then(|g| {
            g.nodes.iter().find_map(|n| {
                n.settings
                    .iter()
                    .find(|s| s.key == "waveform" || s.key == "wave" || s.key == "shape")
                    .and_then(|s| match &s.value {
                        SettingValue::Text(t) => Some(t.to_lowercase()),
                        _ => None,
                    })
            })
        })
        .unwrap_or_else(|| "saw".to_owned());

    let steps = 80usize;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let phase = (t * 2.0) % 1.0;
            let y = if wave_kind.contains("sine") {
                (phase * 2.0 * PI).sin()
            } else if wave_kind.contains("square") || wave_kind.contains("pulse") {
                if phase < 0.5 { 0.85 } else { -0.85 }
            } else if wave_kind.contains("tri") {
                if phase < 0.25 {
                    phase * 4.0
                } else if phase < 0.75 {
                    2.0 - phase * 4.0
                } else {
                    phase * 4.0 - 4.0
                }
            } else {
                let mut s = 0.0_f32;
                for h in 1..=6 {
                    let hf = h as f32;
                    s += ((phase * 2.0 * PI * hf).sin()) / hf;
                }
                (s * 0.58).clamp(-0.92, 0.92)
            };
            Pos2::new(
                inner.left() + t * inner.width(),
                inner.center().y - y * (inner.height() * 0.42),
            )
        })
        .collect();

    for w in pts.windows(2) {
        painter.add(egui::Shape::convex_polygon(
            vec![
                w[0],
                w[1],
                Pos2::new(w[1].x, inner.center().y),
                Pos2::new(w[0].x, inner.center().y),
            ],
            with_alpha(theme::ACCENT_AMBER, 0.12),
            Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::line(
        pts,
        Stroke::new(2.0_f32, theme::ACCENT_AMBER),
    ));
    painter.text(
        Pos2::new(inner.left() + 2.0, inner.top()),
        Align2::LEFT_TOP,
        wave_kind.to_uppercase(),
        FontId::monospace(9.0),
        with_alpha(theme::ACCENT_AMBER, 0.85),
    );
}

fn find_filter_param_refs(instrument: &Instrument) -> (Option<&Parameter>, Option<&Parameter>) {
    let cut = instrument
        .parameters
        .iter()
        .find(|p| {
            p.unit == Unit::Hertz
                && (p.key.contains("cut") || p.key.contains("freq") || p.key.contains("filter"))
        })
        .or_else(|| instrument.parameters.iter().find(|p| p.unit == Unit::Hertz));
    let res = instrument
        .parameters
        .iter()
        .find(|p| p.key.contains("res") || p.key.contains("q"));
    (cut, res)
}

fn find_filter_params(instrument: &Instrument) -> (f64, f64) {
    let (cut_p, res_p) = find_filter_param_refs(instrument);
    let cutoff = cut_p.map_or(2400.0, |p| p.value);
    let res = res_p.map_or(0.28, |p| {
        ((p.value - p.minimum) / (p.maximum - p.minimum).max(1e-6)).clamp(0.0, 1.0)
    });
    (cutoff.clamp(20.0, 20000.0), res)
}

#[allow(clippy::too_many_arguments)]
fn paint_instrument_filter_scope(
    ui: &mut Ui,
    app: &Workspace,
    state: &mut UiState,
    instrument: &Instrument,
    width: f32,
    height: f32,
    pending_actions: &mut Vec<Action>,
) {
    let (cut_p, res_p) = find_filter_param_refs(instrument);
    let interactive = cut_p.is_some() || res_p.is_some();
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, height),
        if interactive {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );
    let inner = rect.shrink(6.0);

    let cut_key = cut_p.map(|p| format!("bode_cut:{}:{}", instrument.id, p.key));
    let res_key = res_p.map(|p| format!("bode_res:{}:{}", instrument.id, p.key));

    // Direct 2D drag on Bode plot adjusts Cutoff (X, log Hz) and Resonance (Y)
    if interactive
        && resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let nx = ((pos.x - inner.left()) / inner.width().max(1.0)).clamp(0.0, 1.0);
        let ny = (1.0 - (pos.y - inner.top()) / inner.height().max(1.0)).clamp(0.0, 1.0);
        if let (Some(p), Some(k)) = (cut_p, cut_key.as_deref()) {
            let hz = (20.0 * (20000.0_f64 / 20.0).powf(f64::from(nx))).clamp(p.minimum, p.maximum);
            state.set_drag_preview(k, hz);
        }
        if let (Some(p), Some(k)) = (res_p, res_key.as_deref()) {
            let val = p.minimum + f64::from(ny) * (p.maximum - p.minimum);
            state.set_drag_preview(k, val.clamp(p.minimum, p.maximum));
        }
    }
    if interactive && resp.drag_stopped() {
        if let (Some(p), Some(k)) = (cut_p, cut_key.as_deref())
            && let Some(v) = state.take_drag_preview(k)
        {
            pending_actions.push(Action::SetInstrumentParameterValue {
                instrument: instrument.id.clone(),
                parameter: p.key.clone(),
                value: v,
            });
        }
        if let (Some(p), Some(k)) = (res_p, res_key.as_deref())
            && let Some(v) = state.take_drag_preview(k)
        {
            pending_actions.push(Action::SetInstrumentParameterValue {
                instrument: instrument.id.clone(),
                parameter: p.key.clone(),
                value: v,
            });
        }
    }

    let cutoff = cut_key
        .as_deref()
        .and_then(|k| state.drag_preview(k))
        .or_else(|| cut_p.map(|p| p.value))
        .unwrap_or(2400.0)
        .clamp(20.0, 20000.0);
    let res = res_key
        .as_deref()
        .and_then(|k| state.drag_preview(k))
        .and_then(|v| {
            res_p.map(|p| ((v - p.minimum) / (p.maximum - p.minimum).max(1e-6)).clamp(0.0, 1.0))
        })
        .or_else(|| {
            res_p.map(|p| {
                ((p.value - p.minimum) / (p.maximum - p.minimum).max(1e-6)).clamp(0.0, 1.0)
            })
        })
        .unwrap_or(0.28);

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::BG_WELL);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(
            1.0_f32,
            if resp.hovered() || resp.dragged() {
                theme::ACCENT_CYAN
            } else {
                theme::BORDER_SUBTLE
            },
        ),
        StrokeKind::Inside,
    );

    for &(freq, lbl) in &[(100.0_f64, "100"), (1000.0, "1k"), (10000.0, "10k")] {
        let nx = ((freq / 20.0).ln() / (20000.0_f64 / 20.0).ln()) as f32;
        let x = inner.left() + nx * inner.width();
        painter.vline(x, inner.y_range(), Stroke::new(1.0_f32, theme::GRID_SUB));
        painter.text(
            Pos2::new(x + 2.0, inner.bottom() - 1.0),
            Align2::LEFT_BOTTOM,
            lbl,
            FontId::monospace(8.5),
            theme::TEXT_MUTED,
        );
    }
    let zero_db_y = inner.top() + inner.height() * 0.40;
    painter.hline(
        inner.x_range(),
        zero_db_y,
        Stroke::new(1.0_f32, theme::GRID_BEAT),
    );

    // Optional static modulation sweep band on Bode plot
    if let Some(p) = cut_p
        && let Some((lo, hi)) = param_static_mod_range(instrument, p)
    {
        let x_lo = inner.left()
            + (((lo.clamp(20.0, 20000.0) / 20.0).ln() / (20000.0_f64 / 20.0).ln()) as f32)
                * inner.width();
        let x_hi = inner.left()
            + (((hi.clamp(20.0, 20000.0) / 20.0).ln() / (20000.0_f64 / 20.0).ln()) as f32)
                * inner.width();
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x_lo, inner.top()),
                Pos2::new(x_hi, inner.bottom()),
            ),
            0.0,
            with_alpha(theme::ACCENT_VIOLET, 0.10),
        );
    }

    let q = 0.707 + res * 5.2;
    let steps = 72usize;
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let nx = i as f32 / steps as f32;
        let f = 20.0 * (20000.0_f64 / 20.0).powf(f64::from(nx));
        let r = f / cutoff;
        let r2 = r * r;
        let denom = ((1.0 - r2).powi(2) + (r / q).powi(2)).sqrt().max(1e-4);
        let db = (20.0 * (1.0 / denom).log10()).clamp(-28.0, 15.0);
        let y_frac = ((15.0 - db) / 43.0).clamp(0.0, 1.0) as f32;
        pts.push(Pos2::new(
            inner.left() + nx * inner.width(),
            inner.top() + y_frac * inner.height(),
        ));
    }

    for w in pts.windows(2) {
        painter.add(egui::Shape::convex_polygon(
            vec![
                w[0],
                w[1],
                Pos2::new(w[1].x, inner.bottom()),
                Pos2::new(w[0].x, inner.bottom()),
            ],
            with_alpha(theme::ACCENT_CYAN, 0.14),
            Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::line(
        pts,
        Stroke::new(2.0_f32, theme::ACCENT_CYAN),
    ));

    let cut_nx = ((cutoff / 20.0).ln() / (20000.0_f64 / 20.0).ln()).clamp(0.0, 1.0) as f32;
    let cut_x = inner.left() + cut_nx * inner.width();
    let dot_y = (zero_db_y - (res as f32) * inner.height() * 0.30)
        .clamp(inner.top() + 5.0, inner.bottom() - 5.0);
    painter.vline(
        cut_x,
        inner.y_range(),
        Stroke::new(1.0_f32, with_alpha(theme::ACCENT_AMBER, 0.55)),
    );
    painter.circle_filled(Pos2::new(cut_x, dot_y), 4.2, theme::ACCENT_AMBER);
    painter.circle_stroke(
        Pos2::new(cut_x, dot_y),
        4.2,
        Stroke::new(1.2_f32, theme::TEXT_PRIMARY),
    );

    if let Some(p) = cut_p
        && let Some(eff) = effective_param_value(app, instrument, &p.key)
    {
        let eff_nx = ((eff.clamp(20.0, 20000.0) / 20.0).ln() / (20000.0_f64 / 20.0).ln()) as f32;
        let ex = inner.left() + eff_nx * inner.width();
        painter.vline(
            ex,
            inner.y_range(),
            Stroke::new(1.5_f32, theme::ACCENT_CYAN),
        );
    }
}

fn paint_instrument_adsr_scope(
    ui: &mut Ui,
    state: &mut UiState,
    instrument: &Instrument,
    width: f32,
    height: f32,
    pending_actions: &mut Vec<Action>,
) {
    let dec_p = instrument
        .parameters
        .iter()
        .find(|p| p.key.contains("decay") || p.key.contains("release"));
    let sus_p = instrument
        .parameters
        .iter()
        .find(|p| p.key.contains("sustain"));
    let interactive = dec_p.is_some() || sus_p.is_some();

    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, height),
        if interactive {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );
    let inner = rect.shrink(7.0);

    let dec_key = dec_p.map(|p| format!("adsr_dec:{}:{}", instrument.id, p.key));
    let sus_key = sus_p.map(|p| format!("adsr_sus:{}:{}", instrument.id, p.key));

    if interactive
        && resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let nx = ((pos.x - inner.left()) / inner.width().max(1.0)).clamp(0.05, 0.95);
        let ny = (1.0 - (pos.y - inner.top()) / inner.height().max(1.0)).clamp(0.0, 1.0);
        if let (Some(p), Some(k)) = (dec_p, dec_key.as_deref()) {
            let v = p.minimum + f64::from(nx) * (p.maximum - p.minimum);
            state.set_drag_preview(k, v.clamp(p.minimum, p.maximum));
        }
        if let (Some(p), Some(k)) = (sus_p, sus_key.as_deref()) {
            let v = p.minimum + f64::from(ny) * (p.maximum - p.minimum);
            state.set_drag_preview(k, v.clamp(p.minimum, p.maximum));
        }
    }
    if interactive && resp.drag_stopped() {
        if let (Some(p), Some(k)) = (dec_p, dec_key.as_deref())
            && let Some(v) = state.take_drag_preview(k)
        {
            pending_actions.push(Action::SetInstrumentParameterValue {
                instrument: instrument.id.clone(),
                parameter: p.key.clone(),
                value: v,
            });
        }
        if let (Some(p), Some(k)) = (sus_p, sus_key.as_deref())
            && let Some(v) = state.take_drag_preview(k)
        {
            pending_actions.push(Action::SetInstrumentParameterValue {
                instrument: instrument.id.clone(),
                parameter: p.key.clone(),
                value: v,
            });
        }
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::BG_WELL);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(
            1.0_f32,
            if resp.hovered() || resp.dragged() {
                theme::ACCENT_VIOLET
            } else {
                theme::BORDER_SUBTLE
            },
        ),
        StrokeKind::Inside,
    );

    let find_p = |needle: &str, default: f64| -> f64 {
        instrument
            .parameters
            .iter()
            .find(|p| p.key.contains(needle))
            .map(|p| {
                let preview_v = if dec_p.is_some_and(|dp| dp.key == p.key) {
                    dec_key
                        .as_deref()
                        .and_then(|k| state.drag_preview(k))
                        .unwrap_or(p.value)
                } else if sus_p.is_some_and(|sp| sp.key == p.key) {
                    sus_key
                        .as_deref()
                        .and_then(|k| state.drag_preview(k))
                        .unwrap_or(p.value)
                } else {
                    p.value
                };
                ((preview_v - p.minimum) / (p.maximum - p.minimum).max(1e-6)).clamp(0.0, 1.0)
            })
            .unwrap_or(default)
    };
    let a = (find_p("attack", 0.08) as f32).clamp(0.04, 0.30);
    let d = (find_p("decay", 0.32) as f32).clamp(0.08, 0.35);
    let s = (find_p("sustain", 0.58) as f32).clamp(0.08, 0.95);
    let r = (find_p("release", 0.35) as f32).clamp(0.08, 0.35);

    let hold = 0.22_f32;
    let total = a + d + hold + r;
    let x0 = inner.left();
    let xa = x0 + (a / total) * inner.width();
    let xd = xa + (d / total) * inner.width();
    let xs = xd + (hold / total) * inner.width();
    let xr = inner.right();

    let y_bot = inner.bottom();
    let y_top = inner.top() + 3.0;
    let y_sus = y_bot - s * (y_bot - y_top);

    let pts = vec![
        Pos2::new(x0, y_bot),
        Pos2::new(xa, y_top),
        Pos2::new(xd, y_sus),
        Pos2::new(xs, y_sus),
        Pos2::new(xr, y_bot),
    ];

    for w in pts.windows(2) {
        painter.add(egui::Shape::convex_polygon(
            vec![
                w[0],
                w[1],
                Pos2::new(w[1].x, y_bot),
                Pos2::new(w[0].x, y_bot),
            ],
            with_alpha(theme::ACCENT_VIOLET, 0.18),
            Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::line(
        pts.clone(),
        Stroke::new(2.0_f32, theme::ACCENT_VIOLET),
    ));
    for p in &pts[1..4] {
        painter.circle_filled(*p, 3.2, theme::TEXT_PRIMARY);
        painter.circle_stroke(*p, 3.2, Stroke::new(1.2_f32, theme::ACCENT_VIOLET));
    }

    let has_lfo = instrument
        .modulation_sources
        .iter()
        .any(|m| matches!(m.kind, InstrumentModulationSourceKind::Lfo { .. }));
    if has_lfo {
        let lfo_pts: Vec<Pos2> = (0..=48)
            .map(|i| {
                let t = i as f32 / 48.0;
                let y = inner.center().y - (t * 4.0 * PI).sin() * (inner.height() * 0.22);
                Pos2::new(inner.left() + t * inner.width(), y)
            })
            .collect();
        painter.add(egui::Shape::line(
            lfo_pts,
            Stroke::new(1.2_f32, with_alpha(theme::ACCENT_CYAN, 0.55)),
        ));
    }
}

// ── 3. Auto-Fit Interactive 2D Signal-Flow Patch Graph (`View::Graph`) ────────────

#[derive(Clone)]
struct LayoutNode<'a> {
    node: &'a GraphNode,
    global_index: usize,
    rect: Rect,
    in_ports: Vec<String>,
    out_ports: Vec<String>,
}

impl LayoutNode<'_> {
    fn in_port_pos(&self, port: &str, header_h: f32) -> Pos2 {
        let idx = self.in_ports.iter().position(|p| p == port).unwrap_or(0);
        let count = self.in_ports.len().max(1);
        let body_top = self.rect.top() + header_h + 6.0;
        let body_bot = self.rect.bottom() - 6.0;
        let frac = (idx as f32 + 0.5) / count as f32;
        let y = body_top + frac * (body_bot - body_top).max(10.0);
        Pos2::new(self.rect.left(), y)
    }

    fn out_port_pos(&self, port: &str, header_h: f32) -> Pos2 {
        let idx = self.out_ports.iter().position(|p| p == port).unwrap_or(0);
        let count = self.out_ports.len().max(1);
        let body_top = self.rect.top() + header_h + 6.0;
        let body_bot = self.rect.bottom() - 6.0;
        let frac = (idx as f32 + 0.5) / count as f32;
        let y = body_top + frac * (body_bot - body_top).max(10.0);
        Pos2::new(self.rect.right(), y)
    }
}

pub(super) fn graph(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Graph unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        unavailable(ui, "No Instrument");
        return;
    };

    sound_subbar(ui, cx, project, instrument);

    let Some(voice) = &instrument.graph else {
        unavailable(
            ui,
            "Stock synthesized percussion · kick · snare · hat · clap · no editable topology",
        );
        return;
    };

    let voice_order = graph_processing_node_indexes(instrument, voice);
    let voice_bus_count = graph_processing_bus_indexes(voice, &voice_order).len();
    let shared_order = instrument
        .shared_graph
        .as_ref()
        .map(|s| graph_processing_node_indexes(instrument, s))
        .unwrap_or_default();
    let shared_node_offset = voice.nodes.len() + voice_bus_count;

    let mut request_fit = cx.state.graph_fitted_for.as_deref() != Some(instrument.id.as_str());

    // Toolbar strip for Graph controls
    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .inner_margin(egui::Margin::symmetric(12, 4))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Voice Domain: {}/{} nodes · {} cables",
                        voice.nodes.len(),
                        voice.max_nodes,
                        voice.connections.len()
                    ))
                    .size(10.5)
                    .color(theme::TEXT_SECONDARY),
                );
                if let Some(shared) = &instrument.shared_graph {
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "Shared Post-Voice: {}/{} nodes · {} cables",
                            shared.nodes.len(),
                            shared.max_nodes,
                            shared.connections.len()
                        ))
                        .monospace()
                        .size(10.5)
                        .color(theme::ACCENT_CYAN),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Reset Layout").size(10.5))
                                .fill(theme::BG_CARD),
                        )
                        .clicked()
                    {
                        let prefix = format!("{}:", instrument.id);
                        cx.state
                            .graph_node_offsets
                            .retain(|k, _| !k.starts_with(&prefix));
                        request_fit = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Fit").size(10.5).color(theme::ACCENT_CYAN),
                            )
                            .fill(theme::BG_CARD),
                        )
                        .clicked()
                    {
                        request_fit = true;
                    }
                    if ui
                        .add(egui::Button::new("+").fill(theme::BG_CARD))
                        .clicked()
                    {
                        cx.state.graph_zoom = (cx.state.graph_zoom * 1.15).clamp(0.45, 1.6);
                    }
                    ui.label(
                        RichText::new(format!("{:.0}%", cx.state.graph_zoom * 100.0))
                            .monospace()
                            .size(10.5)
                            .color(theme::TEXT_MUTED),
                    );
                    if ui
                        .add(egui::Button::new("−").fill(theme::BG_CARD))
                        .clicked()
                    {
                        cx.state.graph_zoom = (cx.state.graph_zoom / 1.15).clamp(0.45, 1.6);
                    }
                });
            });
        });

    let selected = app.selected_graph_node;
    let mut clicked_global: Option<usize> = None;
    let mut pending_actions: Vec<Action> = Vec::new();

    // Right Node Inspector Panel (allocated BEFORE canvas so canvas_rect never overlaps it!)
    egui::SidePanel::right("crump-graph-inspector")
        .resizable(true)
        .default_width(268.0)
        .min_width(220.0)
        .max_width(350.0)
        .frame(
            egui::Frame::new()
                .fill(theme::BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
                .inner_margin(egui::Margin::same(12)),
        )
        .show_inside(ui, |ui| {
            draw_graph_inspector(
                ui,
                cx.state,
                instrument,
                voice,
                &voice_order,
                &shared_order,
                shared_node_offset,
                selected,
                &mut pending_actions,
            );
        });

    // Main 2D Patch Canvas
    let avail = ui.available_size();
    let (canvas_rect, canvas_resp) = ui.allocate_exact_size(
        Vec2::new(avail.x.max(300.0), avail.y.max(240.0)),
        Sense::click_and_drag(),
    );

    // Base geometry at zoom = 1.0 to decide stacking + auto-fit
    let base_node_w = 178.0_f32;
    let base_node_h = 92.0_f32;
    let base_col_gap = 40.0_f32;
    let base_row_gap = 26.0_f32;

    let probe_voice = layout_graph_nodes(
        &instrument.id,
        voice,
        &voice_order,
        0,
        Pos2::ZERO,
        base_node_w,
        base_node_h,
        base_col_gap,
        base_row_gap,
        &cx.state.graph_node_offsets,
        1.0,
    );
    let probe_voice_box = nodes_bounding_rect(&probe_voice)
        .unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 120.0)))
        .expand2(Vec2::new(26.0, 26.0));

    let probe_shared_box = instrument.shared_graph.as_ref().map(|shared| {
        let probe = layout_graph_nodes(
            &instrument.id,
            shared,
            &shared_order,
            shared_node_offset,
            Pos2::ZERO,
            base_node_w,
            base_node_h,
            base_col_gap,
            base_row_gap,
            &cx.state.graph_node_offsets,
            1.0,
        );
        nodes_bounding_rect(&probe)
            .unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::new(180.0, 100.0)))
            .expand2(Vec2::new(26.0, 26.0))
    });

    // Stack Shared Post-Voice FX domain neatly BELOW Voice domain when horizontal side-by-side
    // would exceed canvas width (prevents cramped miniature zoom or right-side clipping!)
    let stack_shared_below = probe_shared_box.is_some_and(|sb| {
        probe_voice_box.width() + 72.0 + sb.width() > (canvas_rect.width() - 36.0)
    });

    if request_fit {
        let total_w = if let Some(sb) = probe_shared_box {
            if stack_shared_below {
                probe_voice_box.width().max(sb.width())
            } else {
                probe_voice_box.width() + 72.0 + sb.width()
            }
        } else {
            probe_voice_box.width()
        };
        let total_h = if let Some(sb) = probe_shared_box {
            if stack_shared_below {
                probe_voice_box.height() + 56.0 + sb.height()
            } else {
                probe_voice_box.height().max(sb.height())
            }
        } else {
            probe_voice_box.height()
        };

        let pad_x = 40.0_f32;
        let pad_y = 36.0_f32;
        let fit_x = (canvas_rect.width() - pad_x).max(100.0) / total_w.max(1.0);
        let fit_y = (canvas_rect.height() - pad_y).max(100.0) / total_h.max(1.0);
        let fit_zoom = fit_x.min(fit_y).clamp(0.50, 1.25);
        cx.state.graph_zoom = fit_zoom;

        let scaled_w = total_w * fit_zoom;
        let scaled_h = total_h * fit_zoom;
        let left_margin = ((canvas_rect.width() - scaled_w) * 0.5).max(20.0);
        let top_margin = ((canvas_rect.height() - scaled_h) * 0.5).max(20.0);
        cx.state.graph_pan = Vec2::new(
            left_margin - probe_voice_box.left() * fit_zoom,
            top_margin - probe_voice_box.top() * fit_zoom,
        );
        cx.state.graph_fitted_for = Some(instrument.id.clone());
    }

    let zoom = cx.state.graph_zoom.clamp(0.45, 1.6);
    let node_w = base_node_w * zoom;
    let node_h = base_node_h * zoom;
    let col_gap = base_col_gap * zoom;
    let row_gap = base_row_gap * zoom;
    let header_h = (22.0 * zoom).clamp(15.0, 26.0);
    let origin = canvas_rect.left_top() + cx.state.graph_pan;

    // Layout Voice Domain
    let voice_layout = layout_graph_nodes(
        &instrument.id,
        voice,
        &voice_order,
        0,
        origin,
        node_w,
        node_h,
        col_gap,
        row_gap,
        &cx.state.graph_node_offsets,
        zoom,
    );
    let voice_bounds = nodes_bounding_rect(&voice_layout)
        .unwrap_or(Rect::from_min_size(origin, Vec2::new(200.0, 120.0)));
    let voice_box = voice_bounds.expand2(Vec2::new(26.0 * zoom, 26.0 * zoom));

    // Layout Shared Domain (side-by-side or neatly stacked below)
    let (shared_layout, shared_box) = if let Some(shared) = &instrument.shared_graph {
        let shared_origin = if stack_shared_below {
            Pos2::new(origin.x, voice_box.bottom() + 56.0 * zoom)
        } else {
            Pos2::new(voice_box.right() + 72.0 * zoom, origin.y)
        };
        let layout = layout_graph_nodes(
            &instrument.id,
            shared,
            &shared_order,
            shared_node_offset,
            shared_origin,
            node_w,
            node_h,
            col_gap,
            row_gap,
            &cx.state.graph_node_offsets,
            zoom,
        );
        let sbox =
            nodes_bounding_rect(&layout).map(|sb| sb.expand2(Vec2::new(26.0 * zoom, 26.0 * zoom)));
        (layout, sbox)
    } else {
        (Vec::new(), None)
    };

    // Node drag vs canvas pan interaction
    let drag_node_key = format!("_graph_dragging_node:{}", instrument.id);
    if canvas_resp.drag_started()
        && let Some(pos) = canvas_resp.interact_pointer_pos()
    {
        let hit_idx = voice_layout
            .iter()
            .chain(shared_layout.iter())
            .find(|ln| ln.rect.contains(pos))
            .map(|ln| ln.global_index);
        if let Some(gidx) = hit_idx {
            cx.state.set_drag_preview(&drag_node_key, gidx as f64);
            clicked_global = Some(gidx);
        }
    }
    if canvas_resp.dragged() {
        let delta = ui.input(|i| i.pointer.delta());
        if let Some(gidx_f) = cx.state.drag_preview(&drag_node_key) {
            let gidx = gidx_f as usize;
            if let Some(ln) = voice_layout
                .iter()
                .chain(shared_layout.iter())
                .find(|n| n.global_index == gidx)
            {
                let key = format!("{}:{}", instrument.id, ln.node.id);
                let entry = cx.state.graph_node_offsets.entry(key).or_insert(Vec2::ZERO);
                *entry += delta / zoom.max(0.25);
            }
        } else {
            cx.state.graph_pan += delta;
        }
    }
    if canvas_resp.drag_stopped() {
        cx.state.cancel_drag_preview(&drag_node_key);
    }
    if canvas_resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        if ui.input(|i| i.modifiers.ctrl || i.modifiers.command) && scroll.y.abs() > 0.1 {
            cx.state.graph_zoom = (cx.state.graph_zoom * (1.0 + scroll.y * 0.002)).clamp(0.45, 1.6);
        } else if scroll.length_sq() > 0.1 {
            cx.state.graph_pan += scroll;
        }
    }

    // Strict clipped painter
    let painter = ui.painter_at(canvas_rect);
    painter.rect_filled(canvas_rect, 0.0, theme::BG_CANVAS);

    // Subtle dot-matrix studio grid
    let grid_step = (28.0 * zoom).max(14.0);
    let ox = (cx.state.graph_pan.x % grid_step + grid_step) % grid_step;
    let oy = (cx.state.graph_pan.y % grid_step + grid_step) % grid_step;
    let mut gx = canvas_rect.left() + ox;
    while gx < canvas_rect.right() {
        let mut gy = canvas_rect.top() + oy;
        while gy < canvas_rect.bottom() {
            painter.circle_filled(Pos2::new(gx, gy), 1.0, theme::BORDER_SUBTLE);
            gy += grid_step;
        }
        gx += grid_step;
    }

    // Paint Voice Domain container frame
    painter.rect_filled(voice_box, 8.0, with_alpha(theme::BG_SURFACE, 0.45));
    painter.rect_stroke(
        voice_box,
        8.0,
        // Neutral container: amber is kept for selection, and this frame wraps everything.
        Stroke::new(1.0_f32, theme::BORDER_STRONG),
        StrokeKind::Inside,
    );
    painter.text(
        Pos2::new(voice_box.left() + 12.0, voice_box.top() + 7.0),
        Align2::LEFT_TOP,
        if instrument.polyphony == 1 {
            "MONO VOICE DOMAIN".to_owned()
        } else {
            format!("POLYPHONIC VOICE DOMAIN · {} VOICES", instrument.polyphony)
        },
        FontId::monospace((10.0 * zoom).clamp(8.5, 11.0)),
        theme::TEXT_MUTED,
    );

    // Paint Shared Domain container + bridge cable
    if let Some(sbox) = shared_box {
        painter.rect_filled(sbox, 8.0, with_alpha(theme::BG_SURFACE, 0.45));
        painter.rect_stroke(
            sbox,
            8.0,
            Stroke::new(1.0_f32, theme::BORDER_STRONG),
            StrokeKind::Inside,
        );
        painter.text(
            Pos2::new(sbox.left() + 12.0, sbox.top() + 7.0),
            Align2::LEFT_TOP,
            "SHARED POST-VOICE FX DOMAIN",
            FontId::monospace((10.0 * zoom).clamp(8.5, 11.0)),
            theme::TEXT_MUTED,
        );

        let (p0, p1) = if stack_shared_below {
            let x = (voice_box.center().x).clamp(sbox.left() + 40.0, sbox.right() - 40.0);
            (Pos2::new(x, voice_box.bottom()), Pos2::new(x, sbox.top()))
        } else {
            (
                Pos2::new(voice_box.right(), voice_box.center().y),
                Pos2::new(sbox.left(), sbox.center().y),
            )
        };
        paint_bridge_cable(&painter, p0, p1, stack_shared_below);
    }

    let selected_id: Option<String> = voice_layout
        .iter()
        .chain(shared_layout.iter())
        .find(|n| n.global_index == selected)
        .map(|n| n.node.id.clone());

    // Paint Cables First (routed to exact per-port jacks!)
    paint_graph_cables(
        &painter,
        voice,
        &voice_layout,
        selected_id.as_deref(),
        header_h,
        zoom,
    );
    if let Some(shared) = &instrument.shared_graph {
        paint_graph_cables(
            &painter,
            shared,
            &shared_layout,
            selected_id.as_deref(),
            header_h,
            zoom,
        );
    }

    // Paint Nodes + Hit Test Clicks
    let pointer_click = canvas_resp
        .clicked()
        .then(|| canvas_resp.interact_pointer_pos())
        .flatten();
    let pointer_dbl = canvas_resp
        .double_clicked()
        .then(|| canvas_resp.interact_pointer_pos())
        .flatten();

    for ln in voice_layout.iter().chain(shared_layout.iter()) {
        let is_sel = ln.global_index == selected;
        paint_graph_node_card(&painter, ln, is_sel, zoom, header_h);
        if pointer_click.is_some_and(|p| ln.rect.contains(p)) {
            clicked_global = Some(ln.global_index);
        }
        if pointer_dbl.is_some_and(|p| ln.rect.contains(p)) {
            clicked_global = Some(ln.global_index);
            pending_actions.push(Action::OpenExactEdit);
        }
    }

    if let Some(idx) = clicked_global {
        cx.select(selected, idx);
    }
    for action in pending_actions {
        cx.send(action);
    }
}

fn collect_node_ports(graph: &Graph, node_id: &str) -> (Vec<String>, Vec<String>) {
    let mut ins = Vec::new();
    let mut outs = Vec::new();
    for conn in &graph.connections {
        if conn.to.node == node_id && !ins.contains(&conn.to.port) {
            ins.push(conn.to.port.clone());
        }
        if conn.from.node == node_id && !outs.contains(&conn.from.port) {
            outs.push(conn.from.port.clone());
        }
    }
    if ins.is_empty() {
        ins.push("in".to_owned());
    }
    if outs.is_empty() {
        outs.push("out".to_owned());
    }
    (ins, outs)
}

#[allow(clippy::too_many_arguments)]
fn layout_graph_nodes<'a>(
    instrument_id: &str,
    graph: &'a Graph,
    order: &[usize],
    global_offset: usize,
    origin: Pos2,
    node_w: f32,
    node_h: f32,
    col_gap: f32,
    row_gap: f32,
    user_offsets: &BTreeMap<String, Vec2>,
    zoom: f32,
) -> Vec<LayoutNode<'a>> {
    let id_to_pos: BTreeMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(ord_pos, &node_idx)| (graph.nodes[node_idx].id.as_str(), ord_pos))
        .collect();

    let mut cols = vec![0usize; order.len()];
    let mut in_deg = vec![0usize; order.len()];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); order.len()];

    for conn in &graph.connections {
        if let (Some(&u), Some(&v)) = (
            id_to_pos.get(conn.from.node.as_str()),
            id_to_pos.get(conn.to.node.as_str()),
        ) && u != v
        {
            adj[u].push(v);
            in_deg[v] += 1;
        }
    }

    let mut q: VecDeque<usize> = (0..order.len()).filter(|&i| in_deg[i] == 0).collect();
    while let Some(u) = q.pop_front() {
        for &v in &adj[u] {
            cols[v] = cols[v].max(cols[u] + 1);
            in_deg[v] = in_deg[v].saturating_sub(1);
            if in_deg[v] == 0 {
                q.push_back(v);
            }
        }
    }

    let mut col_counts: BTreeMap<usize, usize> = BTreeMap::new();
    let mut out = Vec::with_capacity(order.len());
    for (ord_pos, &node_idx) in order.iter().enumerate() {
        let node = &graph.nodes[node_idx];
        let col = cols[ord_pos];
        let row = *col_counts.get(&col).unwrap_or(&0);
        col_counts.insert(col, row + 1);
        let (in_ports, out_ports) = collect_node_ports(graph, &node.id);
        let port_rows = in_ports.len().max(out_ports.len());
        let h = node_h.max((24.0 + port_rows as f32 * 18.0) * zoom);
        let offset = user_offsets
            .get(&format!("{instrument_id}:{}", node.id))
            .copied()
            .unwrap_or(Vec2::ZERO)
            * zoom;
        let x = origin.x + col as f32 * (node_w + col_gap) + offset.x;
        let y = origin.y + 18.0 * zoom + row as f32 * (node_h + row_gap) + offset.y;
        out.push(LayoutNode {
            node,
            global_index: global_offset + ord_pos,
            rect: Rect::from_min_size(Pos2::new(x, y), Vec2::new(node_w, h)),
            in_ports,
            out_ports,
        });
    }
    out
}

fn nodes_bounding_rect(nodes: &[LayoutNode<'_>]) -> Option<Rect> {
    let mut iter = nodes.iter();
    let first = iter.next()?.rect;
    Some(iter.fold(first, |acc, n| acc.union(n.rect)))
}

fn paint_bridge_cable(painter: &egui::Painter, p0: Pos2, p1: Pos2, vertical: bool) {
    if vertical {
        let mid_y = (p0.y + p1.y) * 0.5;
        let c1 = Pos2::new(p0.x, mid_y);
        let c2 = Pos2::new(p1.x, mid_y);
        let pts: Vec<Pos2> = (0..=20)
            .map(|i| {
                let t = i as f32 / 20.0;
                let u = 1.0 - t;
                Pos2::new(
                    u * u * u * p0.x
                        + 3.0 * u * u * t * c1.x
                        + 3.0 * u * t * t * c2.x
                        + t * t * t * p1.x,
                    u * u * u * p0.y
                        + 3.0 * u * u * t * c1.y
                        + 3.0 * u * t * t * c2.y
                        + t * t * t * p1.y,
                )
            })
            .collect();
        painter.add(egui::Shape::line(
            pts,
            Stroke::new(2.2_f32, theme::SIG_NOMINAL),
        ));
        painter.circle_filled(p0, 3.5, theme::SIG_NOMINAL);
        painter.circle_filled(p1, 3.5, theme::SIG_NOMINAL);
    } else {
        paint_bezier_cable(painter, p0, p1, theme::SIG_NOMINAL, 2.2);
    }
    let mid = Pos2::new((p0.x + p1.x) * 0.5, (p0.y + p1.y) * 0.5);
    let badge = Rect::from_center_size(mid, Vec2::new(66.0, 16.0));
    painter.rect_filled(badge, 3.0, theme::BG_WELL);
    painter.rect_stroke(
        badge,
        3.0,
        Stroke::new(1.0_f32, with_alpha(theme::SIG_NOMINAL, 0.6)),
        StrokeKind::Inside,
    );
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        "voice_sum",
        FontId::monospace(9.0),
        theme::SIG_NOMINAL,
    );
}

fn paint_graph_cables(
    painter: &egui::Painter,
    graph: &Graph,
    layout: &[LayoutNode<'_>],
    selected_id: Option<&str>,
    header_h: f32,
    zoom: f32,
) {
    let by_id: BTreeMap<&str, &LayoutNode<'_>> =
        layout.iter().map(|ln| (ln.node.id.as_str(), ln)).collect();

    for conn in &graph.connections {
        let Some(to_node) = by_id.get(conn.to.node.as_str()) else {
            continue;
        };
        let to_port = to_node.in_port_pos(&conn.to.port, header_h);
        let (from_port, base_color) = if let Some(from_node) = by_id.get(conn.from.node.as_str()) {
            (
                from_node.out_port_pos(&conn.from.port, header_h),
                theme::node_color(from_node.node.kind),
            )
        } else {
            let stub = Pos2::new(to_port.x - 18.0 * zoom, to_port.y);
            painter.circle_filled(stub, 2.5, theme::SIG_NOMINAL);
            (stub, theme::SIG_NOMINAL)
        };

        let highlighted =
            selected_id.is_some_and(|sel| sel == conn.from.node || sel == conn.to.node);
        let color = if highlighted {
            base_color
        } else {
            with_alpha(base_color, 0.62)
        };
        paint_bezier_cable(
            painter,
            from_port,
            to_port,
            color,
            if highlighted { 2.3 } else { 1.5 },
        );
    }
}

fn paint_bezier_cable(painter: &egui::Painter, from: Pos2, to: Pos2, color: Color32, width: f32) {
    let dx = ((to.x - from.x).abs() * 0.48).max(24.0);
    let c1 = Pos2::new(from.x + dx, from.y);
    let c2 = Pos2::new(to.x - dx, to.y);
    let steps = 24usize;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let u = 1.0 - t;
            let x = u * u * u * from.x
                + 3.0 * u * u * t * c1.x
                + 3.0 * u * t * t * c2.x
                + t * t * t * to.x;
            let y = u * u * u * from.y
                + 3.0 * u * u * t * c1.y
                + 3.0 * u * t * t * c2.y
                + t * t * t * to.y;
            Pos2::new(x, y)
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(width, color)));
    painter.circle_filled(from, 2.8, color);
    painter.circle_filled(to, 2.8, color);
}

fn paint_graph_node_card(
    painter: &egui::Painter,
    ln: &LayoutNode<'_>,
    selected: bool,
    zoom: f32,
    header_h: f32,
) {
    let node = ln.node;
    let rect = ln.rect;
    let accent = theme::node_color(node.kind);

    // Subtle drop shadow
    painter.rect_filled(
        rect.translate(Vec2::new(0.0, 2.0)),
        6.0,
        with_alpha(theme::SEAM_DARK, 0.55),
    );
    painter.rect_filled(
        rect,
        6.0,
        if selected {
            theme::BG_CARD
        } else {
            theme::BG_SURFACE
        },
    );

    // Colored header band
    let header = Rect::from_min_max(
        rect.left_top(),
        Pos2::new(rect.right(), rect.top() + header_h),
    );
    painter.rect_filled(header, 6.0, with_alpha(accent, 0.24));
    painter.hline(
        rect.left()..=rect.right(),
        header.bottom(),
        Stroke::new(1.0_f32, with_alpha(accent, 0.45)),
    );
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(
            if selected { 1.8_f32 } else { 1.0_f32 },
            if selected {
                theme::ACCENT_AMBER
            } else {
                theme::BORDER_STRONG
            },
        ),
        StrokeKind::Inside,
    );

    // Header text: truncated node ID (left) + kind tag (right) with zero collision
    let kind_label = theme::node_kind_label(node.kind);
    let id_font_size = (12.0 * zoom).clamp(10.0, 13.5);
    let kind_font_size = (8.5 * zoom).clamp(7.5, 10.0);
    let kind_w = kind_label.chars().count() as f32 * kind_font_size * 0.62;
    let id_room = rect.width() - 13.0 - kind_w - 10.0;
    let id_max_chars = ((id_room / (id_font_size * 0.56)) as usize).max(4);
    let clip = painter.with_clip_rect(rect.shrink(3.0));
    clip.text(
        Pos2::new(rect.left() + 7.0, header.center().y),
        Align2::LEFT_CENTER,
        truncate_text(&node.id, id_max_chars),
        FontId::proportional(id_font_size),
        theme::TEXT_PRIMARY,
    );
    clip.text(
        Pos2::new(rect.right() - 6.0, header.center().y),
        Align2::RIGHT_CENTER,
        kind_label,
        FontId::monospace(kind_font_size),
        accent,
    );

    // Center body: compact top 2 settings, inset past the widest port label on each side so
    // jack names and settings never overlap.
    let port_font = FontId::monospace((9.0 * zoom).clamp(8.0, 10.5));
    let port_char_w = port_font.size * 0.62;
    let label_span = |ports: &[String], max: usize| {
        let longest = ports.iter().map(|p| p.chars().count().min(max)).max();
        longest.map_or(8.0, |len| 6.0 + len as f32 * port_char_w + 8.0)
    };
    let body_left = rect.left() + label_span(&ln.in_ports, 8);
    let body_right = rect.right() - label_span(&ln.out_ports, 8);
    let setting_font = FontId::monospace((10.0 * zoom).clamp(8.5, 11.5));
    let setting_chars = ((body_right - body_left) / (setting_font.size * 0.62)).max(4.0) as usize;
    let body_clip = painter.with_clip_rect(Rect::from_min_max(
        Pos2::new(body_left, header.bottom() + 2.0),
        Pos2::new(body_right, rect.bottom() - 3.0),
    ));
    let mut y = header.bottom() + 7.0 * zoom;
    for setting in node.settings.iter().take(2) {
        body_clip.text(
            Pos2::new(body_left, y),
            Align2::LEFT_TOP,
            truncate_text(&setting.key, setting_chars),
            setting_font.clone(),
            theme::TEXT_MUTED,
        );
        y += 12.0 * zoom;
        body_clip.text(
            Pos2::new(body_left, y),
            Align2::LEFT_TOP,
            truncate_text(&format_setting_value(&setting.value), setting_chars),
            setting_font.clone(),
            if matches!(setting.value, SettingValue::Parameter(_)) {
                theme::ACCENT_CYAN
            } else {
                theme::TEXT_PRIMARY
            },
        );
        y += 15.0 * zoom;
    }

    // Named per-port Input Jacks (left edge)
    for port in &ln.in_ports {
        let p = ln.in_port_pos(port, header_h);
        painter.circle_filled(p, 3.2, theme::BG_WELL);
        painter.circle_stroke(p, 3.2, Stroke::new(1.2_f32, accent));
        clip.text(
            Pos2::new(rect.left() + 6.0, p.y),
            Align2::LEFT_CENTER,
            truncate_text(port, 8),
            port_font.clone(),
            theme::TEXT_MUTED,
        );
    }

    // Named per-port Output Jacks (right edge)
    for port in &ln.out_ports {
        let p = ln.out_port_pos(port, header_h);
        painter.circle_filled(p, 3.2, theme::BG_WELL);
        painter.circle_stroke(p, 3.2, Stroke::new(1.2_f32, accent));
        clip.text(
            Pos2::new(rect.right() - 6.0, p.y),
            Align2::RIGHT_CENTER,
            truncate_text(port, 8),
            port_font.clone(),
            theme::TEXT_MUTED,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_graph_inspector(
    ui: &mut Ui,
    state: &mut UiState,
    instrument: &Instrument,
    voice: &Graph,
    voice_order: &[usize],
    shared_order: &[usize],
    shared_node_offset: usize,
    selected: usize,
    pending_actions: &mut Vec<Action>,
) {
    section_header(ui, "NODE INSPECTOR", theme::ACCENT_AMBER);
    ui.add_space(6.0);

    let voice_domain = if instrument.polyphony == 1 {
        "Mono Voice"
    } else {
        "Polyphonic Voice"
    };
    let resolved: Option<(&Graph, &GraphNode, &str)> = if selected < voice_order.len() {
        Some((voice, &voice.nodes[voice_order[selected]], voice_domain))
    } else if let Some(shared) = &instrument.shared_graph
        && selected >= shared_node_offset
        && selected < shared_node_offset + shared_order.len()
    {
        Some((
            shared,
            &shared.nodes[shared_order[selected - shared_node_offset]],
            "Shared Post-Voice FX",
        ))
    } else {
        voice_order
            .first()
            .map(|&idx| (voice, &voice.nodes[idx], voice_domain))
    };

    let Some((domain_graph, node, domain_name)) = resolved else {
        ui.label(RichText::new("Select a node on the canvas.").color(theme::TEXT_MUTED));
        return;
    };

    let accent = theme::node_color(node.kind);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&node.id)
                .strong()
                .size(14.5)
                .color(theme::TEXT_PRIMARY),
        );
        badge_chip(ui, theme::node_kind_label(node.kind), accent);
    });
    // Domain and source location on their own lines, so neither wraps mid-phrase.
    ui.label(
        RichText::new(domain_name)
            .size(10.5)
            .color(theme::TEXT_SECONDARY),
    );
    ui.label(
        RichText::new(format!(
            "{}:{}",
            node.location.file.display(),
            node.location.line
        ))
        .monospace()
        .size(9.5)
        .color(theme::TEXT_MUTED),
    );
    ui.add_space(8.0);

    section_header(ui, "SETTINGS & PARAMETERS", theme::BORDER_STRONG);
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Measured inside the scroll area, so the scrollbar's width is already taken out;
            // measured outside, the rows overflow it and the resizable panel creeps wider.
            let slider_w = (ui.available_width() - 4.0).max(160.0);
            for setting in &node.settings {
                if let SettingValue::Parameter(param_key) = &setting.value
                    && let Some(param) = instrument.parameters.iter().find(|p| p.key == *param_key)
                {
                    let drag_key = format!("graph:{}:{}:{}", instrument.id, node.id, param.key);
                    let out = horizontal_mini_slider(
                        ui,
                        state,
                        &drag_key,
                        &format!("{} (${})", setting.key, param.key),
                        param.value,
                        param.minimum,
                        param.maximum,
                        param.default,
                        param.unit,
                        theme::ACCENT_CYAN,
                        slider_w,
                        22.0,
                    );
                    if let Some(v) = out.committed {
                        pending_actions.push(Action::SetInstrumentParameterValue {
                            instrument: instrument.id.clone(),
                            parameter: param.key.clone(),
                            value: v,
                        });
                    }
                    ui.add_space(4.0);
                } else {
                    egui::Frame::new()
                        .fill(theme::BG_WELL)
                        .corner_radius(3.0)
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
                            ui.set_width(slider_w - 16.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&setting.key)
                                        .size(11.0)
                                        .color(theme::TEXT_SECONDARY),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(format_setting_value(&setting.value))
                                            .monospace()
                                            .size(11.0)
                                            .color(theme::TEXT_PRIMARY),
                                    );
                                });
                            });
                        });
                    ui.add_space(3.0);
                }
            }

            ui.add_space(10.0);
            section_header(ui, "SIGNAL CONNECTIONS", theme::BORDER_STRONG);
            ui.add_space(4.0);
            let mut any_conn = false;
            for conn in &domain_graph.connections {
                if conn.from.node == node.id || conn.to.node == node.id {
                    any_conn = true;
                    ui.label(
                        RichText::new(format!(
                            "{}.{} -> {}.{}",
                            conn.from.node, conn.from.port, conn.to.node, conn.to.port
                        ))
                        .monospace()
                        .size(10.0)
                        .color(if conn.from.node == node.id {
                            theme::ACCENT_AMBER
                        } else {
                            theme::ACCENT_CYAN
                        }),
                    );
                }
            }
            if !any_conn {
                ui.label(
                    RichText::new("No direct buses")
                        .size(10.5)
                        .color(theme::TEXT_MUTED),
                );
            }

            ui.add_space(12.0);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Open Node in Source")
                            .size(11.0)
                            .color(theme::TEXT_PRIMARY),
                    )
                    .fill(theme::BG_CARD),
                )
                .clicked()
            {
                pending_actions.push(Action::OpenExactEdit);
            }
        });
}

// ── 4. Grouped 2/3-Column Studio Parameter Matrix (`View::Parameters`) ────────────

pub(super) fn parameters(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Parameters unavailable");
        return;
    };
    let Some(instrument) = project.song.instruments.get(app.selected_instrument) else {
        unavailable(ui, "No Instrument");
        return;
    };

    sound_subbar(ui, cx, project, instrument);

    // Search + Filter Pills strip
    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    RichText::new("SEARCH")
                        .strong()
                        .size(10.0)
                        .color(theme::TEXT_MUTED),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut cx.state.param_search)
                        .hint_text(RichText::new("Filter by name or key…").color(theme::TEXT_MUTED))
                        .font(FontId::proportional(12.0))
                        .desired_width(220.0),
                );
                if !cx.state.param_search.is_empty() && ui.small_button("×").clicked() {
                    cx.state.param_search.clear();
                }

                ui.separator();
                for (mode, label, accent) in [
                    (0u8, "All", theme::ACCENT_AMBER),
                    (1u8, "Automated", theme::SIG_NOMINAL),
                    (2u8, "Modulated", theme::ACCENT_VIOLET),
                ] {
                    if pill_toggle(
                        ui,
                        label,
                        cx.state.param_filter_mode == mode,
                        accent,
                        &format!("Show {label} parameters"),
                    )
                    .clicked()
                    {
                        cx.state.param_filter_mode = mode;
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} params · Click A to automate · Double-click resets",
                            instrument.parameters.len()
                        ))
                        .size(10.5)
                        .color(theme::TEXT_MUTED),
                    );
                });
            });
        });

    let selected = clamp_index(app.selected_parameter, instrument.parameters.len());
    let query = cx.state.param_search.trim().to_lowercase();
    let filter_mode = cx.state.param_filter_mode;
    let mut new_selected: Option<usize> = None;
    let mut pending_actions: Vec<Action> = Vec::new();

    let filtered: Vec<(usize, &Parameter)> = instrument
        .parameters
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            if !query.is_empty()
                && !p.name.to_lowercase().contains(&query)
                && !p.key.to_lowercase().contains(&query)
            {
                return false;
            }
            match filter_mode {
                1 => find_automation_lane(project, &instrument.id, &p.key).is_some(),
                2 => instrument
                    .modulation_routes
                    .iter()
                    .any(|r| r.target == p.key && !r.bypass),
                _ => true,
            }
        })
        .collect();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(10.0);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(12, 2))
                .show(ui, |ui| {
                    if filtered.is_empty() {
                        ui.label(
                            RichText::new("No parameters match the current filter.")
                                .color(theme::TEXT_MUTED),
                        );
                    } else {
                        let avail_w = ui.available_width();
                        let cols = if avail_w >= 960.0 && filtered.len() >= 3 {
                            3
                        } else if filtered.len() >= 2 {
                            2
                        } else {
                            1
                        };
                        for chunk in filtered.chunks(cols) {
                            ui.columns(cols, |columns| {
                                for (col_idx, &(index, param)) in chunk.iter().enumerate() {
                                    let is_sel = index == selected;
                                    let mod_eff = effective_param_value(app, instrument, &param.key);
                                    let mod_range = param_static_mod_range(instrument, param);
                                    let mod_count = instrument
                                        .modulation_routes
                                        .iter()
                                        .filter(|r| r.target == param.key && !r.bypass)
                                        .count();
                                    let auto_lane =
                                        find_automation_lane(project, &instrument.id, &param.key);

                                    draw_parameter_matrix_card(
                                        &mut columns[col_idx],
                                        cx.state,
                                        instrument,
                                        param,
                                        index,
                                        is_sel,
                                        mod_range,
                                        mod_eff,
                                        mod_count,
                                        auto_lane,
                                        &mut new_selected,
                                        &mut pending_actions,
                                    );
                                }
                            });
                            ui.add_space(8.0);
                        }
                    }

                    ui.add_space(8.0);

                    // Lower Studio Coverage Summary (Automation Lanes + Internal Modulation Matrix)
                    egui::Frame::new()
                        .fill(theme::BG_SURFACE)
                        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.columns(2, |cols| {
                                // Left: Automation Coverage
                                cols[0].vertical(|ui| {
                                    section_header(ui, "SONG AUTOMATION COVERAGE", theme::SIG_NOMINAL);
                                    ui.add_space(5.0);
                                    let inst_lanes: Vec<_> = project
                                        .song
                                        .automations
                                        .iter()
                                        .filter(|l| {
                                            l.target
                                                .to_string()
                                                .starts_with(&format!("instrument/{}/", instrument.id))
                                        })
                                        .collect();
                                    if inst_lanes.is_empty() {
                                        ui.label(
                                            RichText::new(
                                                "No active Automation lanes on this Instrument yet. Click 'A' on any parameter card above to create one.",
                                            )
                                            .size(11.0)
                                            .color(theme::TEXT_MUTED),
                                        );
                                    } else {
                                        for lane in inst_lanes {
                                            ui.horizontal(|ui| {
                                                badge_chip(ui, "AUTO", theme::SIG_NOMINAL);
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{} ({} pts)",
                                                        lane.target,
                                                        lane.points.len()
                                                    ))
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(theme::TEXT_PRIMARY),
                                                );
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if ui
                                                            .add(
                                                                egui::Button::new(
                                                                    RichText::new("Edit >")
                                                                        .size(10.0)
                                                                        .color(theme::ACCENT_CYAN),
                                                                )
                                                                .frame(false),
                                                            )
                                                            .clicked()
                                                        {
                                                            pending_actions.push(Action::ShowView(
                                                                View::Automation,
                                                            ));
                                                        }
                                                    },
                                                );
                                            });
                                            ui.add_space(3.0);
                                        }
                                    }
                                });

                                // Right: Internal Modulation Coverage
                                cols[1].vertical(|ui| {
                                    section_header(ui, "INTERNAL MODULATION ROUTES", theme::ACCENT_VIOLET);
                                    ui.add_space(5.0);
                                    if instrument.modulation_routes.is_empty() {
                                        ui.label(
                                            RichText::new(
                                                "No internal modulation routes authored on this Instrument.",
                                            )
                                            .size(11.0)
                                            .color(theme::TEXT_MUTED),
                                        );
                                    } else {
                                        for route in &instrument.modulation_routes {
                                            ui.horizontal(|ui| {
                                                badge_chip(
                                                    ui,
                                                    &route.source,
                                                    if route.bypass {
                                                        theme::TEXT_MUTED
                                                    } else {
                                                        theme::ACCENT_VIOLET
                                                    },
                                                );
                                                route_arrow(ui);
                                                let target = instrument
                                                    .parameters
                                                    .iter()
                                                    .find(|p| p.key == route.target);
                                                badge_chip(
                                                    ui,
                                                    target.map_or(&route.target, |p| &p.name),
                                                    theme::ACCENT_CYAN,
                                                );
                                                let depth = target
                                                    .and_then(|p| {
                                                        p.unit.parse_value(&route.depth).ok().map(|v| {
                                                            theme::format_compact_value(v, p.unit)
                                                        })
                                                    })
                                                    .unwrap_or_else(|| route.depth.clone());
                                                ui.label(
                                                    RichText::new(depth)
                                                        .monospace()
                                                        .size(10.5)
                                                        .color(theme::TEXT_SECONDARY),
                                                );
                                            });
                                            ui.add_space(3.0);
                                        }
                                    }
                                });
                            });
                        });
                });
            ui.add_space(10.0);
        });

    if let Some(idx) = new_selected {
        cx.select(selected, idx);
    }
    for action in pending_actions {
        cx.send(action);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_parameter_matrix_card(
    ui: &mut Ui,
    state: &mut UiState,
    instrument: &Instrument,
    param: &Parameter,
    index: usize,
    is_sel: bool,
    mod_range: Option<(f64, f64)>,
    mod_eff: Option<f64>,
    mod_count: usize,
    auto_lane: Option<usize>,
    new_selected: &mut Option<usize>,
    pending_actions: &mut Vec<Action>,
) {
    let card = egui::Frame::new()
        .fill(if is_sel {
            theme::BG_CARD
        } else {
            theme::BG_SURFACE
        })
        .stroke(Stroke::new(
            if is_sel { 1.4_f32 } else { 1.0_f32 },
            if is_sel {
                theme::ACCENT_AMBER
            } else {
                theme::BORDER_SUBTLE
            },
        ))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(11))
        .show(ui, |ui| {
            // Card Header: Name + A/M Interactive Chips
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(&param.name)
                            .strong()
                            .size(12.5)
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new(format!("${} · {}", param.key, param.unit))
                            .monospace()
                            .size(9.5)
                            .color(theme::TEXT_MUTED),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // M modulation indicator
                    if mod_count > 0 {
                        badge_chip(ui, &format!("M×{mod_count}"), theme::ACCENT_VIOLET);
                    } else if param.modulatable {
                        badge_chip(ui, "M", theme::TEXT_MUTED);
                    }
                    // Interactive A button: jump to existing Automation lane or create a new one!
                    if param.automatable {
                        let a_active = auto_lane.is_some();
                        let tip = if a_active {
                            "Jump to Automation lane for this parameter"
                        } else {
                            "Create Automation lane for this parameter"
                        };
                        if led_button(
                            ui,
                            "A",
                            a_active,
                            theme::SIG_NOMINAL,
                            Vec2::new(22.0, 19.0),
                            tip,
                        )
                        .clicked()
                        {
                            if a_active {
                                pending_actions.push(Action::ShowView(View::Automation));
                            } else {
                                pending_actions.push(Action::CreateAutomationLane {
                                    lane: format!("{}_{}", instrument.id, param.key),
                                    target: format!("instrument/{}/{}", instrument.id, param.key),
                                    initial_value: theme::format_crump_literal(
                                        param.value,
                                        param.unit,
                                    ),
                                });
                                pending_actions.push(Action::ShowView(View::Automation));
                            }
                        }
                    }
                });
            });

            ui.add_space(8.0);

            // Card Body: Tactile Rotary Knob + Range Metadata & Slider
            ui.horizontal(|ui| {
                let drag_key = format!("pmat:{}:{}", instrument.id, param.key);
                let out = rotary_knob_modulated(
                    ui,
                    state,
                    &drag_key,
                    "",
                    param.value,
                    param.minimum,
                    param.maximum,
                    param.default,
                    param.unit,
                    theme::ACCENT_AMBER,
                    mod_range,
                    mod_eff,
                    &format!("{} (${})", param.name, param.key),
                    52.0,
                );
                if out.clicked || out.committed.is_some() {
                    *new_selected = Some(index);
                }
                if let Some(v) = out.committed {
                    pending_actions.push(Action::SetInstrumentParameterValue {
                        instrument: instrument.id.clone(),
                        parameter: param.key.clone(),
                        value: v,
                    });
                }
                if out.right_clicked {
                    *new_selected = Some(index);
                    pending_actions.push(Action::OpenExactEdit);
                }

                ui.add_space(6.0);
                ui.vertical(|ui| {
                    let slider_w = (ui.available_width() - 4.0).max(96.0);
                    let slider_key = format!("pmats:{}:{}", instrument.id, param.key);
                    let sout = horizontal_mini_slider(
                        ui,
                        state,
                        &slider_key,
                        "Value",
                        param.value,
                        param.minimum,
                        param.maximum,
                        param.default,
                        param.unit,
                        theme::ACCENT_CYAN,
                        slider_w,
                        22.0,
                    );
                    if let Some(v) = sout.committed {
                        pending_actions.push(Action::SetInstrumentParameterValue {
                            instrument: instrument.id.clone(),
                            parameter: param.key.clone(),
                            value: v,
                        });
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Range {} – {}",
                            theme::format_compact_value(param.minimum, param.unit),
                            theme::format_compact_value(param.maximum, param.unit),
                        ))
                        .monospace()
                        .size(9.5)
                        .color(theme::TEXT_MUTED),
                    );
                    ui.label(
                        RichText::new(format!(
                            "Default {}",
                            theme::format_compact_value(param.default, param.unit),
                        ))
                        .monospace()
                        .size(9.5)
                        .color(theme::TEXT_MUTED),
                    );
                    // The Live row always takes its line, so cards keep their height as
                    // modulation moves the effective value on and off the authored one.
                    let live = mod_eff
                        .filter(|eff| (eff - param.value).abs() > 1e-3)
                        .map_or_else(
                            || " ".to_owned(),
                            |eff| format!("Live {}", theme::format_compact_value(eff, param.unit)),
                        );
                    ui.label(
                        RichText::new(live)
                            .monospace()
                            .size(9.5)
                            .color(theme::ACCENT_CYAN),
                    );
                });
            });
        });

    if card.response.interact(Sense::click()).clicked() {
        *new_selected = Some(index);
    }
}
