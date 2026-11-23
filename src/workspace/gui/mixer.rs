//! Studio Mixing Console: vertical channel strips grouped by Tracks | Groups | Returns
//! with a right-pinned Master bus strip aligned on a single 0 dB fader datum line,
//! long-throw faders, honest Peak+RMS dBFS meters, rotary pan pots, draggable sends with
//! PRE/POST tags, clickable insert bypass LEDs, bank filter pills, and Console | FX Rack sub-bar.

use super::theme::{self, with_alpha};
use super::widgets::preview_key;
use super::widgets::{
    Cell, apply_row_click, horizontal_mini_slider, led_button, paint_dbfs_meter_vertical,
    pill_toggle, rotary_knob, table, toolbar, unavailable, vertical_fader,
};
use super::*;
use crate::model::{ChannelStrip, OutputTarget, Processor, Unit};

const STRIP_WIDTH: f32 = 116.0;
const MASTER_WIDTH: f32 = 128.0;
/// Fixed upper-bay height above the fader/meter bay shared by channel strips and Master strip
/// so every fader in the console aligns horizontally on one unified 0 dB datum line.
const UPPER_BAY_HEIGHT: f32 = 276.0;

pub(super) fn mixer(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let Some(project) = app.bundle.project.as_ref() else {
        unavailable(ui, "Mixer unavailable while Project source has errors");
        return;
    };

    // Slim 30px Mix sub-bar: Console | FX Rack + Bank Filter Pills + Solo Clear + Table Mode
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                // Mix sub-view tabs (Console | FX Rack)
                if pill_toggle(
                    ui,
                    "Console",
                    app.view == View::Mixer && !cx.state.mixer_table_mode,
                    theme::ACCENT_AMBER,
                    "Studio vertical channel-strip mixing console · 2",
                )
                .tab_pressed()
                {
                    cx.state.mixer_table_mode = false;
                }
                if pill_toggle(
                    ui,
                    "FX Rack",
                    app.view == View::Processing,
                    theme::ACCENT_AMBER,
                    "Open Processing FX Rack & Device Inspector · 3",
                )
                .tab_pressed()
                {
                    cx.send(Action::ShowView(View::Processing));
                }

                ui.separator();

                // Bank visibility filter pills
                let filters: [(u8, &str, Color32); 4] = [
                    (0, "All", theme::TEXT_PRIMARY),
                    (
                        1,
                        &format!("Tracks ({})", project.song.tracks.len()),
                        theme::ACCENT_AMBER,
                    ),
                    (
                        2,
                        &format!("Groups ({})", project.song.groups.len()),
                        theme::ACCENT_CYAN,
                    ),
                    (
                        3,
                        &format!("Returns ({})", project.song.returns.len()),
                        theme::ACCENT_VIOLET,
                    ),
                ];
                for (code, label, accent) in filters {
                    if pill_toggle(
                        ui,
                        label,
                        cx.state.mixer_bank_filter == code,
                        accent,
                        "Filter console banks",
                    )
                    .tab_pressed()
                    {
                        cx.state.mixer_bank_filter = code;
                        cx.state.mixer_table_mode = false;
                    }
                }

                let solo_count = app.audition_plan.soloed_channels.len();
                if solo_count > 0
                    && led_button(
                        ui,
                        &format!("Clear Solo ({solo_count})"),
                        false,
                        theme::SIG_HOT,
                        Vec2::new(112.0, 20.0),
                        "Clear all transient channel solos",
                    )
                    .clicked()
                {
                    cx.send(Action::ClearAllSolos);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if pill_toggle(
                        ui,
                        "Params Table",
                        cx.state.mixer_table_mode,
                        theme::ACCENT_CYAN,
                        "Inspect all raw mixer parameter targets in table form",
                    )
                    .clicked()
                    {
                        cx.state.mixer_table_mode = !cx.state.mixer_table_mode;
                    }
                    if !project.song.sidechains.is_empty() {
                        ui.label(
                            RichText::new(format!(
                                "{} Sidechain{}",
                                project.song.sidechains.len(),
                                if project.song.sidechains.len() == 1 {
                                    ""
                                } else {
                                    "s"
                                }
                            ))
                            .size(10.5)
                            .color(theme::TEXT_MUTED),
                        );
                    }
                });
            });
        });

    if cx.state.mixer_table_mode {
        mixer_table(ui, cx);
        return;
    }

    // Compute ONE shared strip height from the console body area so channel strips and Master match pixel-for-pixel
    let console_h = (ui.available_height() - 16.0).clamp(470.0, 680.0);

    // Pin Master bus channel strip to the right edge of the Console
    egui::SidePanel::right("crump-mixer-master")
        .resizable(false)
        .exact_width(MASTER_WIDTH + 16.0)
        .frame(
            egui::Frame::new()
                .fill(theme::BG_WELL)
                .inner_margin(egui::Margin::symmetric(8, 8))
                .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK)),
        )
        .show_inside(ui, |ui| {
            master_strip(ui, cx, project, console_h);
        });

    // Horizontally scrollable channel strip deck: Tracks | Groups | Returns
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::BG_WELL)
                .inner_margin(egui::Margin::same(8)),
        )
        .show_inside(ui, |ui| {
            let filter = cx.state.mixer_bank_filter;
            let show_tracks = filter == 0 || filter == 1;
            let show_groups = (filter == 0 || filter == 2) && !project.song.groups.is_empty();
            let show_returns = (filter == 0 || filter == 3) && !project.song.returns.is_empty();

            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;

                        // ── Tracks Bank ──
                        if show_tracks {
                            for (index, track) in project.song.tracks.iter().enumerate() {
                                channel_strip(
                                    ui,
                                    cx,
                                    StripDescriptor {
                                        identity: format!("track/{}", track.id),
                                        target_prefix: format!("track/{}", track.id),
                                        name: &track.name,
                                        role: "TRACK",
                                        accent: theme::track_color(index),
                                        channel: &track.channel,
                                        height: console_h,
                                    },
                                );
                            }
                        }

                        // ── Groups Bank ──
                        if show_groups {
                            if show_tracks {
                                bank_divider(ui, "GRP", theme::ACCENT_CYAN, console_h);
                            }
                            for group in &project.song.groups {
                                channel_strip(
                                    ui,
                                    cx,
                                    StripDescriptor {
                                        identity: format!("group/{}", group.id),
                                        target_prefix: format!("group/{}", group.id),
                                        name: &group.name,
                                        role: "GROUP",
                                        accent: theme::ACCENT_CYAN,
                                        channel: &group.channel,
                                        height: console_h,
                                    },
                                );
                            }
                        }

                        // ── Returns Bank ──
                        if show_returns {
                            if show_tracks || show_groups {
                                bank_divider(ui, "RET", theme::ACCENT_VIOLET, console_h);
                            }
                            for ret in &project.song.returns {
                                channel_strip(
                                    ui,
                                    cx,
                                    StripDescriptor {
                                        identity: format!("return/{}", ret.id),
                                        target_prefix: format!("return/{}", ret.id),
                                        name: &ret.name,
                                        role: "RETURN",
                                        accent: theme::ACCENT_VIOLET,
                                        channel: &ret.channel,
                                        height: console_h,
                                    },
                                );
                            }
                        }
                    });
                });
        });
}

fn bank_divider(ui: &mut Ui, label: &str, color: Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(26.0, height), Sense::hover());
    let painter = ui.painter();
    let badge = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.top() + 14.0),
        Vec2::new(24.0, 16.0),
    );
    painter.rect_filled(badge, 3.0, with_alpha(color, 0.22));
    painter.rect_stroke(
        badge,
        3.0,
        Stroke::new(1.0_f32, with_alpha(color, 0.65)),
        StrokeKind::Inside,
    );
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::monospace(8.5),
        color,
    );
    painter.vline(
        rect.center().x,
        (badge.bottom() + 6.0)..=(rect.bottom() - 8.0),
        Stroke::new(1.0_f32, with_alpha(color, 0.35)),
    );
}

struct StripDescriptor<'a> {
    identity: String,
    target_prefix: String,
    name: &'a str,
    role: &'a str,
    accent: Color32,
    channel: &'a ChannelStrip,
    height: f32,
}

fn mixer_param_index(app: &Workspace, target_str: &str) -> Option<usize> {
    app.mixer_parameters
        .iter()
        .position(|p| p.target.to_string() == target_str)
}

fn channel_strip(ui: &mut Ui, cx: &mut Cx<'_>, desc: StripDescriptor<'_>) {
    let app = cx.app;
    let level_target = format!("{}/level", desc.target_prefix);
    let pan_param = desc
        .channel
        .panner
        .parameters()
        .first()
        .map_or_else(|| "pan".to_owned(), ToString::to_string);
    let pan_target = format!("{}/{pan_param}", desc.target_prefix);
    let level_idx = mixer_param_index(app, &level_target);
    let selected_idx = clamp_index(app.selected_mixer_parameter, app.mixer_parameters.len());
    let is_selected = app
        .mixer_parameters
        .get(selected_idx)
        .and_then(|p| p.channel_identity.as_deref())
        == Some(desc.identity.as_str());

    let is_solo = app.audition_plan.soloed_channels.contains(&desc.identity);
    let is_cue = app
        .audition_plan
        .cue
        .as_ref()
        .is_some_and(|cue| cue.channel == desc.identity);

    let (peak, avg) = app
        .mix_channel_indexes
        .get(&desc.identity)
        .copied()
        .filter(|&i| i < app.telemetry.engine.channel_count)
        .map_or((0.0, 0.0), |i| {
            let m = app.telemetry.engine.channel_activity[i];
            (m.current_peak, m.average)
        });

    let strip_h = desc.height;
    let inner_w = STRIP_WIDTH - 12.0;
    let fader_h = (strip_h - UPPER_BAY_HEIGHT - 62.0).clamp(160.0, 560.0);

    let frame = egui::Frame::new()
        .fill(if is_selected {
            Color32::from_rgb(34, 40, 52)
        } else {
            theme::BG_SURFACE
        })
        .stroke(Stroke::new(
            if is_selected { 1.4_f32 } else { 1.0_f32 },
            if is_selected {
                theme::SELECTION
            } else {
                theme::BORDER_SUBTLE
            },
        ))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::same(6));

    ui.push_id(&desc.identity, |ui| {
        ui.allocate_ui(Vec2::new(STRIP_WIDTH, strip_h), |ui| {
            frame.show(ui, |ui| {
                ui.set_width(inner_w);
                ui.set_min_height(strip_h - 12.0);
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 5.0;

                    // Fixed-height upper bay so all faders sit on the exact same vertical datum
                    ui.allocate_ui_with_layout(
                        Vec2::new(inner_w, UPPER_BAY_HEIGHT),
                        Layout::top_down(Align::Center),
                        |ui| {
                            ui.set_min_height(UPPER_BAY_HEIGHT);
                            ui.spacing_mut().item_spacing.y = 5.0;

                            // 1. Top 4px color bar
                            let (bar_rect, bar_resp) =
                                ui.allocate_exact_size(Vec2::new(inner_w, 4.0), Sense::click());
                            ui.painter().rect_filled(bar_rect, 2.0, desc.accent);
                            if bar_resp.clicked()
                                && let Some(idx) = level_idx
                            {
                                cx.send(Action::SelectMixerParameter(idx));
                            }

                            // 2. Channel name & output routing pill
                            // Names get two centred lines before ellipsizing, inside a fixed
                            // band so every strip keeps the same vertical datum.
                            let mut job = egui::text::LayoutJob::single_section(
                                desc.name.to_owned(),
                                egui::TextFormat {
                                    font_id: FontId::proportional(11.5),
                                    color: theme::TEXT_PRIMARY,
                                    ..Default::default()
                                },
                            );
                            job.wrap = egui::text::TextWrapping {
                                max_width: inner_w - 4.0,
                                max_rows: 2,
                                break_anywhere: false,
                                overflow_character: Some('…'),
                            };
                            job.halign = Align::Center;
                            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                            let (name_rect, name_resp) =
                                ui.allocate_exact_size(Vec2::new(inner_w, 30.0), Sense::click());
                            ui.painter().galley(
                                // Top-aligned so one- and two-line names share a baseline.
                                Pos2::new(name_rect.center().x, name_rect.top() + 1.0),
                                galley,
                                theme::TEXT_PRIMARY,
                            );
                            let name_resp = name_resp.on_hover_text(desc.name);
                            if name_resp.clicked()
                                && let Some(idx) = level_idx
                            {
                                cx.send(Action::SelectMixerParameter(idx));
                            }
                            let out_label = match &desc.channel.output {
                                OutputTarget::Master => "Master".to_owned(),
                                OutputTarget::Group(g) => format!("Grp {g}"),
                                OutputTarget::Return(r) => format!("Ret {r}"),
                            };
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("{} · {}", desc.role, out_label))
                                        .size(9.5)
                                        .color(theme::TEXT_MUTED),
                                )
                                .truncate(),
                            );

                            // 3. Insert Slots Mini-Rack (fixed 54px box)
                            inserts_box(ui, cx, &desc.channel.inserts, level_idx);

                            // 4. Sends Section (fixed 42px box)
                            sends_box(
                                ui,
                                cx,
                                &desc.target_prefix,
                                &desc.channel.sends,
                                desc.accent,
                            );

                            // 5. Rotary Pan Knob
                            let pan_val = desc.channel.panner.primary_position();
                            let pan_out = rotary_knob(
                                ui,
                                cx.state,
                                &preview_key::channel_pan(&desc.identity),
                                "PAN",
                                pan_val,
                                -1.0,
                                1.0,
                                0.0,
                                Unit::Pan,
                                theme::ACCENT_CYAN,
                                None,
                                "Stereo Pan position",
                                32.0,
                            );
                            if pan_out.clicked
                                && let Some(idx) = mixer_param_index(app, &pan_target).or(level_idx)
                            {
                                cx.send(Action::SelectMixerParameter(idx));
                            }
                            if let Some(new_pan) = pan_out.committed {
                                cx.send(Action::SetParameterTargetValue {
                                    target: pan_target.clone(),
                                    value: (new_pan * 100.0).round() / 100.0,
                                });
                            }

                            // 6. Hardware LED Row: M / S / C
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let btn_w = ((inner_w - 8.0) / 3.0).max(24.0);
                                let mute_key = preview_key::channel_mute(&desc.identity);
                                let muted = cx.state.flag(&mute_key, desc.channel.mute);
                                if led_button(
                                    ui,
                                    "M",
                                    muted,
                                    theme::MUTE_LIT,
                                    Vec2::new(btn_w, 20.0),
                                    "Durable source Mute · m",
                                )
                                .clicked()
                                {
                                    cx.state.hold_flag(&mute_key, !muted);
                                    if let Some(idx) = level_idx {
                                        cx.send(Action::SelectMixerParameter(idx));
                                    }
                                    cx.send(Action::ToggleChannelMute {
                                        identity: desc.identity.clone(),
                                    });
                                }
                                if led_button(
                                    ui,
                                    "S",
                                    is_solo,
                                    theme::SIG_HOT,
                                    Vec2::new(btn_w, 20.0),
                                    "Transient Solo · s",
                                )
                                .clicked()
                                {
                                    if let Some(idx) = level_idx {
                                        cx.send(Action::SelectMixerParameter(idx));
                                    }
                                    cx.send(Action::ToggleChannelSolo {
                                        identity: desc.identity.clone(),
                                    });
                                }
                                if led_button(
                                    ui,
                                    "C",
                                    is_cue,
                                    theme::ACCENT_CYAN,
                                    Vec2::new(btn_w, 20.0),
                                    "Transient post-fader Cue · c",
                                )
                                .clicked()
                                {
                                    if let Some(idx) = level_idx {
                                        cx.send(Action::SelectMixerParameter(idx));
                                    }
                                    cx.send(Action::ToggleChannelCue {
                                        identity: desc.identity.clone(),
                                        tap: TapPoint::PostFader,
                                    });
                                }
                            });
                        },
                    );

                    // 7. Horizontally Centered Long-Throw Console Fader + Honest dBFS Meter
                    let fader_w = 58.0;
                    let meter_w = 20.0;
                    let gap = 8.0;
                    let bay_w = fader_w + gap + meter_w;
                    let side_pad = ((inner_w - bay_w) * 0.5).max(0.0);
                    let mut live_db = desc.channel.level_db;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.add_space(side_pad);
                        let fader_out = vertical_fader(
                            ui,
                            cx.state,
                            &preview_key::channel_level(&desc.identity),
                            desc.channel.level_db,
                            -60.0,
                            12.0,
                            desc.accent,
                            fader_w,
                            fader_h,
                        );
                        live_db = fader_out.live_value;
                        if fader_out.clicked
                            && let Some(idx) = level_idx
                        {
                            cx.send(Action::SelectMixerParameter(idx));
                        }
                        if fader_out.right_clicked {
                            if let Some(idx) = level_idx {
                                cx.send(Action::SelectMixerParameter(idx));
                            }
                            cx.send(Action::OpenExactEdit);
                        }
                        if let Some(new_db) = fader_out.committed {
                            cx.send(Action::SetParameterTargetValue {
                                target: level_target.clone(),
                                value: new_db,
                            });
                        }
                        ui.add_space(gap);
                        let (meter_rect, _) =
                            ui.allocate_exact_size(Vec2::new(meter_w, fader_h), Sense::hover());
                        paint_dbfs_meter_vertical(ui.painter(), meter_rect, peak, avg);
                    });

                    // 8. Recessed digital dB value readout
                    let readout_rect = egui::Frame::new()
                        .fill(theme::BG_WELL)
                        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
                        .corner_radius(3.0)
                        .inner_margin(egui::Margin::symmetric(4, 3))
                        .show(ui, |ui| {
                            ui.set_width(inner_w - 8.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new(theme::format_compact_value(
                                        live_db,
                                        Unit::Decibels,
                                    ))
                                    .monospace()
                                    .size(10.5)
                                    .color(
                                        if live_db > 0.05 {
                                            theme::SIG_HOT
                                        } else {
                                            theme::TEXT_PRIMARY
                                        },
                                    ),
                                );
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if readout_rect.double_clicked() {
                        if let Some(idx) = level_idx {
                            cx.send(Action::SelectMixerParameter(idx));
                        }
                        cx.send(Action::OpenExactEdit);
                    } else if readout_rect.clicked()
                        && let Some(idx) = level_idx
                    {
                        cx.send(Action::SelectMixerParameter(idx));
                    }
                });
            });
        });
    });
}

fn inserts_box(ui: &mut Ui, cx: &mut Cx<'_>, inserts: &[Processor], level_idx: Option<usize>) {
    egui::Frame::new()
        .fill(theme::BG_WELL)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .corner_radius(3.0)
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(50.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if inserts.is_empty() {
                    ui.label(
                        RichText::new("No inserts")
                            .size(9.5)
                            .color(theme::TEXT_MUTED),
                    );
                } else {
                    for proc in inserts.iter().take(3) {
                        let color = theme::processor_color(proc.kind);
                        let bypass_key = preview_key::processor_bypass(&proc.id);
                        let durable = cx.state.flag(&bypass_key, proc.bypass);
                        let bypassed = cx
                            .app
                            .audition_plan
                            .transient_bypass
                            .get(&proc.id)
                            .copied()
                            .unwrap_or(durable);
                        let (rect, resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 14.0),
                            Sense::click(),
                        );
                        let led_zone = Rect::from_min_max(
                            rect.left_top(),
                            Pos2::new(rect.left() + 14.0, rect.bottom()),
                        );
                        let p = ui.painter();
                        p.rect_filled(
                            rect,
                            2.0,
                            if resp.hovered() {
                                theme::BG_HOVER
                            } else {
                                theme::BG_CARD
                            },
                        );
                        p.circle_filled(
                            Pos2::new(rect.left() + 6.5, rect.center().y),
                            3.0,
                            if bypassed { theme::TEXT_MUTED } else { color },
                        );
                        p.with_clip_rect(rect.shrink2(Vec2::new(13.0, 0.0))).text(
                            Pos2::new(rect.left() + 13.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            theme::processor_title(proc.kind),
                            FontId::proportional(9.5),
                            if bypassed {
                                theme::TEXT_MUTED
                            } else {
                                theme::TEXT_PRIMARY
                            },
                        );
                        if resp.clicked() {
                            let clicked_led = resp
                                .interact_pointer_pos()
                                .is_some_and(|pos| led_zone.contains(pos));
                            if clicked_led {
                                cx.state.hold_flag(&bypass_key, !durable);
                                cx.send(Action::ToggleProcessorDurableBypass {
                                    processor: proc.id.clone(),
                                });
                            } else {
                                if let Some(idx) = level_idx {
                                    cx.send(Action::SelectMixerParameter(idx));
                                }
                                if let Some(graph) = cx.app.processing_graph()
                                    && let Some(pos) =
                                        graph.processors.iter().position(|item| item.id == proc.id)
                                {
                                    cx.send(Action::SelectProcessor(pos));
                                }
                            }
                        }
                        if resp.double_clicked() {
                            if let Some(graph) = cx.app.processing_graph()
                                && let Some(pos) =
                                    graph.processors.iter().position(|item| item.id == proc.id)
                            {
                                cx.send(Action::SelectProcessor(pos));
                            }
                            cx.send(Action::ShowView(View::Processing));
                        }
                        resp.on_hover_text(format!(
                            "{} ({})\nClick LED to toggle bypass · Double-click to open FX Rack",
                            theme::processor_title(proc.kind),
                            if bypassed { "Bypassed" } else { "Active" }
                        ));
                    }
                    if inserts.len() > 3 {
                        ui.label(
                            RichText::new(format!("+{} more FX", inserts.len() - 3))
                                .size(8.5)
                                .color(theme::ACCENT_CYAN),
                        );
                    }
                }
            });
        });
}

fn sends_box(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    target_prefix: &str,
    sends: &[crate::model::Send],
    accent: Color32,
) {
    egui::Frame::new()
        .fill(theme::BG_WELL)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .corner_radius(3.0)
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(38.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if sends.is_empty() {
                    ui.label(RichText::new("No sends").size(9.5).color(theme::TEXT_MUTED));
                } else {
                    for send in sends.iter().take(2) {
                        // Post-fader is the usual send, so only a pre-fader tap is called out.
                        let label = match send.tap {
                            TapPoint::PreFader => format!("{} pre", send.target),
                            TapPoint::PostFader => send.target.clone(),
                        };
                        let out = horizontal_mini_slider(
                            ui,
                            cx.state,
                            &format!("send-{target_prefix}-{}", send.id),
                            &label,
                            send.level_db,
                            -60.0,
                            6.0,
                            -12.0,
                            Unit::Decibels,
                            accent,
                            ui.available_width(),
                            15.0,
                        );
                        if let Some(val) = out.committed {
                            cx.send(Action::SetParameterTargetValue {
                                target: format!("send/{}/level", send.id),
                                value: (val * 10.0).round() / 10.0,
                            });
                        }
                    }
                }
            });
        });
}

fn master_strip(ui: &mut Ui, cx: &mut Cx<'_>, project: &Project, strip_h: f32) {
    let app = cx.app;
    let master = &project.song.master;
    let level_target = "master/level".to_owned();
    let level_idx = mixer_param_index(app, &level_target);
    let selected_idx = clamp_index(app.selected_mixer_parameter, app.mixer_parameters.len());
    let is_selected = level_idx == Some(selected_idx);

    let (peak, avg) = master_levels(app);
    let peak_db = theme::amp_to_dbfs(peak);
    let headroom = (0.0 - peak_db).max(0.0);

    let inner_w = MASTER_WIDTH - 12.0;
    let fader_h = (strip_h - UPPER_BAY_HEIGHT - 62.0).clamp(160.0, 560.0);

    egui::Frame::new()
        .fill(Color32::from_rgb(33, 36, 44))
        .stroke(Stroke::new(
            1.4_f32,
            if is_selected {
                theme::ACCENT_AMBER
            } else {
                theme::BORDER_STRONG
            },
        ))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(inner_w);
            ui.set_min_height(strip_h - 12.0);
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 5.0;

                // Fixed-height upper bay IDENTICAL to channel_strip so Master fader aligns on 0 dB datum
                ui.allocate_ui_with_layout(
                    Vec2::new(inner_w, UPPER_BAY_HEIGHT),
                    Layout::top_down(Align::Center),
                    |ui| {
                        ui.set_min_height(UPPER_BAY_HEIGHT);
                        ui.spacing_mut().item_spacing.y = 5.0;

                        let (bar_rect, _) =
                            ui.allocate_exact_size(Vec2::new(inner_w, 4.0), Sense::hover());
                        ui.painter().rect_filled(bar_rect, 2.0, theme::ACCENT_AMBER);

                        ui.label(
                            RichText::new("MASTER")
                                .strong()
                                .size(12.5)
                                .color(theme::ACCENT_AMBER),
                        );
                        ui.label(
                            RichText::new("STEREO OUT")
                                .size(9.5)
                                .color(theme::TEXT_MUTED),
                        );

                        let empty_inserts: &[crate::model::Processor] = &[];
                        let master_inserts = master
                            .as_ref()
                            .map_or(empty_inserts, |m| m.inserts.as_slice());
                        inserts_box(ui, cx, master_inserts, level_idx);

                        // Master Loudness & Headroom Telemetry Bay
                        egui::Frame::new()
                            .fill(theme::BG_WELL)
                            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
                            .corner_radius(4.0)
                            .inner_margin(egui::Margin::symmetric(6, 5))
                            .show(ui, |ui| {
                                ui.set_width(inner_w - 12.0);
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 3.5;
                                    for (tag, val, col) in [
                                        (
                                            "PEAK",
                                            format!("{} dB", theme::format_dbfs(peak)),
                                            if peak >= 0.99 {
                                                theme::SIG_CLIP
                                            } else if peak_db >= -1.0 {
                                                theme::SIG_HOT
                                            } else {
                                                theme::TEXT_PRIMARY
                                            },
                                        ),
                                        (
                                            "Avg",
                                            format!("{} dB", theme::format_dbfs(avg)),
                                            theme::TEXT_SECONDARY,
                                        ),
                                        (
                                            "HEADROOM",
                                            if peak > 0.001 {
                                                format!("+{headroom:.1} dB")
                                            } else {
                                                "—".to_owned()
                                            },
                                            theme::ACCENT_CYAN,
                                        ),
                                        (
                                            "RATE",
                                            format!("{:.1}k", project.sample_rate as f32 / 1000.0),
                                            theme::TEXT_MUTED,
                                        ),
                                    ] {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(tag)
                                                    .size(8.5)
                                                    .strong()
                                                    .color(theme::TEXT_MUTED),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(val)
                                                            .monospace()
                                                            .size(9.5)
                                                            .color(col),
                                                    );
                                                },
                                            );
                                        });
                                    }
                                });
                            });

                        // Quick Master FX Rack jump button
                        if led_button(
                            ui,
                            "MASTER FX",
                            false,
                            theme::ACCENT_CYAN,
                            Vec2::new(inner_w, 22.0),
                            "Open Master bus inserts in FX Rack",
                        )
                        .tab_pressed()
                        {
                            cx.state.processing_owner_filter = Some("master".to_owned());
                            cx.send(Action::ShowView(View::Processing));
                        }
                    },
                );

                // Centered Master Fader + Wide Honest Meter
                let master_level_db = master.as_ref().map_or(0.0, |m| m.level_db);
                let fader_w = 60.0;
                let meter_w = 24.0;
                let gap = 8.0;
                let side_pad = ((inner_w - (fader_w + gap + meter_w)) * 0.5).max(0.0);
                let mut live_db = master_level_db;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(side_pad);
                    let fader_out = vertical_fader(
                        ui,
                        cx.state,
                        &preview_key::channel_level("master"),
                        master_level_db,
                        -60.0,
                        12.0,
                        theme::TEXT_SECONDARY,
                        fader_w,
                        fader_h,
                    );
                    live_db = fader_out.live_value;
                    if fader_out.clicked
                        && let Some(idx) = level_idx
                    {
                        cx.send(Action::SelectMixerParameter(idx));
                    }
                    if fader_out.right_clicked {
                        if let Some(idx) = level_idx {
                            cx.send(Action::SelectMixerParameter(idx));
                        }
                        cx.send(Action::OpenExactEdit);
                    }
                    if let Some(new_db) = fader_out.committed {
                        cx.send(Action::SetParameterTargetValue {
                            target: level_target.clone(),
                            value: new_db,
                        });
                    }
                    ui.add_space(gap);
                    let (meter_rect, _) =
                        ui.allocate_exact_size(Vec2::new(meter_w, fader_h), Sense::hover());
                    paint_dbfs_meter_vertical(ui.painter(), meter_rect, peak, avg);
                });

                // Compact recessed Master dB readout (never stretches vertically!)
                let readout_resp = egui::Frame::new()
                    .fill(theme::BG_WELL)
                    .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
                    .corner_radius(3.0)
                    .inner_margin(egui::Margin::symmetric(4, 3))
                    .show(ui, |ui| {
                        ui.set_width(inner_w - 8.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new(theme::format_compact_value(live_db, Unit::Decibels))
                                    .monospace()
                                    .size(11.0)
                                    .strong()
                                    .color(theme::TEXT_PRIMARY),
                            );
                        });
                    })
                    .response
                    .interact(Sense::click());
                if readout_resp.double_clicked() {
                    if let Some(idx) = level_idx {
                        cx.send(Action::SelectMixerParameter(idx));
                    }
                    cx.send(Action::OpenExactEdit);
                } else if readout_resp.clicked()
                    && let Some(idx) = level_idx
                {
                    cx.send(Action::SelectMixerParameter(idx));
                }
            });
        });
}

/// Optional power-user table mode showing all raw mixer parameter targets.
fn mixer_table(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        toolbar(
            ui,
            cx,
            &[
                ("−", Action::AdjustSelected(-1), "Step down · Left"),
                ("+", Action::AdjustSelected(1), "Step up · Right"),
                ("Exact…", Action::OpenExactEdit, "Enter exact value · Enter"),
            ],
        );
        ui.separator();
        toolbar(
            ui,
            cx,
            &[
                ("Mute", Action::ToggleMixerMute, "Authored Mute · m"),
                ("Solo", Action::ToggleMixerSolo, "Transient Solo · s"),
                (
                    "Cue Post",
                    Action::ToggleMixerCue(TapPoint::PostFader),
                    "Post-fader cue · c",
                ),
            ],
        );
    });
    let selected = clamp_index(app.selected_mixer_parameter, app.mixer_parameters.len());
    let click = table(
        ui,
        cx.state,
        "mixer",
        &[
            ("Target", 280.0),
            ("Value", 100.0),
            ("Domain", 170.0),
            ("Peak (dBFS)", 160.0),
            ("M/S/C", 68.0),
            ("Auto/Mod", 72.0),
        ],
        app.mixer_parameters.len(),
        Some(selected),
        |index| {
            let parameter = &app.mixer_parameters[index];
            let spec = &parameter.spec;
            let identity = parameter.channel_identity.as_ref();
            let meter = identity
                .and_then(|identity| app.mix_channel_indexes.get(identity).copied())
                .filter(|index| *index < app.telemetry.engine.channel_count)
                .map(|index| app.telemetry.engine.channel_activity[index]);
            let audition = identity.map_or_else(
                || "—".to_owned(),
                |identity| {
                    let mark = |on: bool, letter: char| if on { letter } else { '·' };
                    format!(
                        "{}{}{}",
                        mark(parameter.muted, 'M'),
                        mark(app.audition_plan.soloed_channels.contains(identity), 'S'),
                        mark(
                            app.audition_plan
                                .cue
                                .as_ref()
                                .is_some_and(|cue| cue.channel == *identity),
                            'C'
                        )
                    )
                },
            );
            vec![
                parameter.target.to_string().into(),
                Cell::Colored(
                    theme::format_compact_value(spec.base, spec.unit),
                    theme::ACCENT_AMBER,
                ),
                format!(
                    "{} – {}",
                    theme::format_compact_value(spec.minimum, spec.unit),
                    theme::format_compact_value(spec.maximum, spec.unit)
                )
                .into(),
                meter.map_or_else(|| "—".into(), |m| Cell::Meter(m.current_peak)),
                audition.into(),
                format!(
                    "{}/{}",
                    if spec.automatable { "A" } else { "·" },
                    if spec.modulatable { "M" } else { "·" }
                )
                .into(),
            ]
        },
    );
    apply_row_click(cx, click, selected);
}
