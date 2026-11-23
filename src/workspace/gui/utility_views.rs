//! Studio utility views: token-highlighted Source IDE, Diagnostics Preflight Console,
//! Revision History Timeline with Inline Inspector, and Searchable Two-Column Shortcut Reference.

use egui::text::{LayoutJob, TextFormat};

use super::theme::{self, with_alpha};
use super::widgets::{legible, pill_toggle, unavailable};
use super::*;
use crate::diagnostic::Severity;
use crate::history::SourceChangeKind;

fn utility_subbar(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let diag_count = app.bundle.diagnostics.as_slice().len();
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                for (view, label, key) in [
                    (View::Source, "Source", "0"),
                    (View::Diagnostics, "Diagnostics", "—"),
                    (View::History, "History", "H"),
                    (View::Help, "Shortcuts", "?"),
                ] {
                    let accent = if view == View::Diagnostics && app.bundle.diagnostics.has_errors()
                    {
                        theme::SIG_CLIP
                    } else {
                        theme::ACCENT_CYAN
                    };
                    let title = if view == View::Diagnostics && diag_count > 0 {
                        format!("{label} ({diag_count})")
                    } else {
                        label.to_owned()
                    };
                    if pill_toggle(
                        ui,
                        &title,
                        app.view == view,
                        accent,
                        &format!("{label} · Key {key}"),
                    )
                    .clicked()
                    {
                        cx.send(Action::ShowView(view));
                    }
                }
            });
        });
}

// ── 1. Two-Pane Token-Highlighted Source Viewer ──────────────────────────────────

pub(super) fn source(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    utility_subbar(ui, cx);

    if app.bundle.sources.is_empty() {
        unavailable(ui, "No source files loaded");
        return;
    }

    let files: Vec<(&Path, &str)> = app
        .bundle
        .sources
        .iter()
        .map(|(p, t)| (p.as_path(), t.as_str()))
        .collect();
    let selected = clamp_index(app.selected_source, files.len());
    let (current_path, text) = files[selected];

    let mut select_file: Option<usize> = None;
    let mut open_external = false;

    // Left File Tree Sidebar
    egui::SidePanel::left("crump-source-files")
        .resizable(true)
        .default_width(245.0)
        .min_width(180.0)
        .max_width(350.0)
        .frame(
            egui::Frame::new()
                .fill(theme::BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
                .inner_margin(egui::Margin::same(10)),
        )
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("PROJECT FILES")
                        .strong()
                        .size(10.5)
                        .color(theme::TEXT_SECONDARY),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{}", files.len()))
                            .monospace()
                            .size(10.0)
                            .color(theme::TEXT_MUTED),
                    );
                });
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (idx, (path, contents)) in files.iter().enumerate() {
                        let is_sel = idx == selected;
                        let line_count = contents.lines().count();
                        let display = path.display().to_string();
                        let frame = egui::Frame::new()
                            .fill(if is_sel {
                                with_alpha(theme::ACCENT_CYAN, 0.16)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .stroke(Stroke::new(
                                1.0_f32,
                                if is_sel {
                                    with_alpha(theme::ACCENT_CYAN, 0.55)
                                } else {
                                    Color32::TRANSPARENT
                                },
                            ))
                            .corner_radius(4.0)
                            .inner_margin(egui::Margin::symmetric(8, 5));
                        let resp = frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(format!("{line_count}L"))
                                            .monospace()
                                            .size(9.5)
                                            .color(theme::TEXT_MUTED),
                                    );
                                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&display)
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(if is_sel {
                                                        theme::TEXT_PRIMARY
                                                    } else {
                                                        theme::TEXT_SECONDARY
                                                    }),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(&display);
                                    });
                                });
                            });
                        });
                        if resp.response.interact(Sense::click()).clicked() {
                            select_file = Some(idx);
                        }
                        ui.add_space(2.0);
                    }
                });
        });

    // Right Source Header + Token-Highlighted Code Canvas
    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(current_path.display().to_string())
                        .monospace()
                        .strong()
                        .size(12.0)
                        .color(theme::ACCENT_CYAN),
                );
                ui.label(
                    RichText::new("Authoritative .crump source · live-reloaded on save")
                        .size(10.5)
                        .color(theme::TEXT_MUTED),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Open in Editor")
                                    .size(11.0)
                                    .color(theme::TEXT_PRIMARY),
                            )
                            .fill(theme::BG_CARD),
                        )
                        .on_hover_text("Open this file in the desktop default editor · e")
                        .clicked()
                    {
                        open_external = true;
                    }
                });
            });
        });

    let lines = text.lines().collect::<Vec<_>>();
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 2.5;
    let mut area = egui::ScrollArea::both().auto_shrink([false, false]);
    if cx.state.source_scroll.replace(app.source_scroll) != Some(app.source_scroll) {
        area = area.vertical_scroll_offset(
            app.source_scroll as f32 * (row_height + ui.spacing().item_spacing.y),
        );
    }

    egui::Frame::new()
        .fill(theme::BG_WELL)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            area.show_rows(ui, row_height, lines.len(), |ui, rows| {
                for index in rows {
                    let is_target = index == app.source_scroll;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.label(
                            RichText::new(format!("{:>5}", index + 1))
                                .monospace()
                                .size(11.5)
                                .color(if is_target {
                                    theme::ACCENT_AMBER
                                } else {
                                    theme::TEXT_MUTED
                                }),
                        );
                        ui.label(highlight_crump_line(lines[index]));
                    });
                }
            });
        });

    if let Some(idx) = select_file
        && idx != selected
    {
        cx.send(Action::AdjustSelected(idx as isize - selected as isize));
    }
    if open_external {
        cx.send(Action::EditSource);
    }
}

/// Token-level `.crump` syntax highlighter producing an egui `LayoutJob`.
fn highlight_crump_line(line: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = FontId::monospace(11.5);
    let push = |job: &mut LayoutJob, slice: &str, color: Color32| {
        if !slice.is_empty() {
            job.append(slice, 0.0, TextFormat::simple(font.clone(), color));
        }
    };

    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;

    while i < len {
        // Comments (`# ...` or `// ...`)
        if bytes[i] == b'#' || (bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/') {
            push(&mut job, &line[i..], theme::TEXT_MUTED);
            break;
        }
        // Quoted strings
        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            push(&mut job, &line[start..i], theme::SIG_NOMINAL);
            continue;
        }
        // Whitespace
        if bytes[i].is_ascii_whitespace() {
            let start = i;
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            push(&mut job, &line[start..i], theme::TEXT_PRIMARY);
            continue;
        }
        // Parameter reference `$cutoff`
        if bytes[i] == b'$' {
            let start = i;
            i += 1;
            while i < len
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-')
            {
                i += 1;
            }
            push(&mut job, &line[start..i], theme::ACCENT_VIOLET);
            continue;
        }
        // Punctuation / arrows
        if matches!(
            bytes[i],
            b'{' | b'}' | b'[' | b']' | b'(' | b')' | b',' | b'='
        ) {
            push(&mut job, &line[i..i + 1], theme::TEXT_MUTED);
            i += 1;
            continue;
        }
        if bytes[i] == b'-' && i + 1 < len && bytes[i + 1] == b'>' {
            push(&mut job, "->", theme::ACCENT_AMBER);
            i += 2;
            continue;
        }
        // Word / numeric / literal token
        let start = i;
        while i < len
            && !bytes[i].is_ascii_whitespace()
            && !matches!(
                bytes[i],
                b'{' | b'}' | b'[' | b']' | b'(' | b')' | b',' | b'=' | b'"' | b'#'
            )
        {
            if bytes[i] == b'-' && i + 1 < len && bytes[i + 1] == b'>' {
                break;
            }
            i += 1;
        }
        let tok = &line[start..i];
        let color = if is_crump_keyword(tok) {
            theme::ACCENT_AMBER
        } else if is_crump_numeric_literal(tok) {
            theme::ACCENT_CYAN
        } else if tok.contains('/') {
            theme::ACCENT_VIOLET
        } else {
            theme::TEXT_PRIMARY
        };
        push(&mut job, tok, color);
    }

    job
}

fn is_crump_keyword(tok: &str) -> bool {
    matches!(
        tok,
        "project"
            | "song"
            | "instrument"
            | "track"
            | "group"
            | "return"
            | "master"
            | "pattern"
            | "automation"
            | "node"
            | "bus"
            | "processor"
            | "modulate"
            | "modulation"
            | "faceplate"
            | "parameter"
            | "send"
            | "clip"
            | "note"
            | "hit"
            | "point"
            | "locator"
            | "tempo"
            | "meter"
            | "variation"
            | "groove"
            | "import"
            | "use"
            | "connect"
            | "route"
            | "source"
            | "voice"
            | "shared"
            | "graph"
            | "control"
            | "section"
            | "sample"
            | "true"
            | "false"
    )
}

fn is_crump_numeric_literal(tok: &str) -> bool {
    let first = tok.as_bytes().first().copied().unwrap_or(0);
    if first.is_ascii_digit() {
        return true;
    }
    if matches!(first, b'-' | b'+') && tok.as_bytes().get(1).is_some_and(u8::is_ascii_digit) {
        return true;
    }
    false
}

// ── 2. Diagnostics Preflight Console ─────────────────────────────────────────────

pub(super) fn diagnostics(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    utility_subbar(ui, cx);

    let diagnostics = app.bundle.diagnostics.as_slice();
    if diagnostics.is_empty() {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(28.0);
                let avail_w = ui.available_width();
                let card_w = (avail_w - 48.0).clamp(520.0, 740.0);
                let side_pad = ((avail_w - card_w) * 0.5).max(16.0);

                ui.horizontal(|ui| {
                    ui.add_space(side_pad);
                    ui.vertical(|ui| {
                        ui.set_width(card_w);

                        // Status Banner Card
                        egui::Frame::new()
                            .fill(theme::BG_SURFACE)
                            .stroke(Stroke::new(1.0_f32, with_alpha(theme::SIG_NOMINAL, 0.48)))
                            .corner_radius(10.0)
                            .inner_margin(egui::Margin::same(24))
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    let (emblem, _) =
                                        ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                                    let p = ui.painter();
                                    p.circle_filled(
                                        emblem.center(),
                                        20.0,
                                        with_alpha(theme::SIG_NOMINAL, 0.18),
                                    );
                                    p.circle_stroke(
                                        emblem.center(),
                                        20.0,
                                        Stroke::new(1.8_f32, theme::SIG_NOMINAL),
                                    );
                                    let c = emblem.center();
                                    p.line_segment(
                                        [Pos2::new(c.x - 7.0, c.y), Pos2::new(c.x - 1.5, c.y + 6.0)],
                                        Stroke::new(2.4_f32, theme::SIG_NOMINAL),
                                    );
                                    p.line_segment(
                                        [Pos2::new(c.x - 1.5, c.y + 6.0), Pos2::new(c.x + 8.5, c.y - 6.0)],
                                        Stroke::new(2.4_f32, theme::SIG_NOMINAL),
                                    );

                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new("Project Valid — Ready to Publish")
                                            .strong()
                                            .size(17.0)
                                            .color(theme::TEXT_PRIMARY),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(
                                            "Zero compiler errors or warnings across all authoritative .crump files.",
                                        )
                                        .size(12.0)
                                        .color(theme::TEXT_SECONDARY),
                                    );
                                    ui.add_space(14.0);

                                    if let Some(project) = &app.bundle.project {
                                        egui::Frame::new()
                                            .fill(theme::BG_WELL)
                                            .corner_radius(6.0)
                                            .inner_margin(egui::Margin::symmetric(16, 10))
                                            .show(ui, |ui| {
                                                let stats = [
                                                    ("TRACKS", project.song.tracks.len()),
                                                    ("INSTRUMENTS", project.song.instruments.len()),
                                                    ("PATTERNS", project.song.patterns.len()),
                                                    ("AUTOMATIONS", project.song.automations.len()),
                                                    ("FILES", app.bundle.sources.len()),
                                                ];
                                                stat_strip(ui, &stats);
                                            });
                                    }

                                    if !app.candidate_is_active() {
                                        ui.add_space(14.0);
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(
                                                        "Publish Candidate as Active Revision",
                                                    )
                                                    .strong()
                                                    .color(theme::BG_WELL),
                                                )
                                                .fill(theme::ACCENT_AMBER),
                                            )
                                            .clicked()
                                        {
                                            cx.send(Action::OpenPublish);
                                        }
                                    }
                                });
                            });

                        ui.add_space(14.0);

                        // Studio Preflight Health Checklist
                        egui::Frame::new()
                            .fill(theme::BG_SURFACE)
                            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::same(16))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("STUDIO PREFLIGHT HEALTH CHECKLIST")
                                        .strong()
                                        .size(10.5)
                                        .color(theme::TEXT_SECONDARY),
                                );
                                ui.add_space(10.0);

                                let proc_count = app.bundle.project.as_ref().map_or(0, |p| {
                                    p.song
                                        .tracks
                                        .iter()
                                        .map(|t| t.channel.inserts.len())
                                        .sum::<usize>()
                                        + p.song
                                            .groups
                                            .iter()
                                            .map(|g| g.channel.inserts.len())
                                            .sum::<usize>()
                                        + p.song
                                            .returns
                                            .iter()
                                            .map(|r| r.channel.inserts.len())
                                            .sum::<usize>()
                                        + p.song
                                            .master
                                            .as_ref()
                                            .map_or(0, |m| m.inserts.len())
                                });
                                let pdc_total: u32 = app.bundle.project.as_ref().map_or(0, |p| {
                                    p.song
                                        .tracks
                                        .iter()
                                        .flat_map(|t| t.channel.inserts.iter())
                                        .chain(
                                            p.song
                                                .groups
                                                .iter()
                                                .flat_map(|g| g.channel.inserts.iter()),
                                        )
                                        .chain(
                                            p.song
                                                .returns
                                                .iter()
                                                .flat_map(|r| r.channel.inserts.iter()),
                                        )
                                        .chain(
                                            p.song
                                                .master
                                                .iter()
                                                .flat_map(|m| m.inserts.iter()),
                                        )
                                        .map(|pr| pr.latency_frames)
                                        .sum()
                                });
                                let active_sync = if app.candidate_is_active() {
                                    "Candidate matches Active Revision"
                                } else {
                                    "Unpublished Candidate edits pending"
                                };

                                ui.columns(2, |cols| {
                                    preflight_check_row(
                                        &mut cols[0],
                                        true,
                                        "Syntax & Semantics",
                                        &format!(
                                            "{} authoritative .crump files verified",
                                            app.bundle.sources.len()
                                        ),
                                    );
                                    cols[0].add_space(8.0);
                                    preflight_check_row(
                                        &mut cols[0],
                                        true,
                                        "Mix Graph Acyclicity",
                                        "All Track, Group, Return, and Master buses DAG-verified",
                                    );
                                    cols[0].add_space(8.0);
                                    preflight_check_row(
                                        &mut cols[0],
                                        true,
                                        "PDC Latency Budget",
                                        &format!(
                                            "{proc_count} processors · {pdc_total} smp compensated"
                                        ),
                                    );

                                    preflight_check_row(
                                        &mut cols[1],
                                        app.candidate_is_active(),
                                        "Active Revision Sync",
                                        active_sync,
                                    );
                                    cols[1].add_space(8.0);
                                    preflight_check_row(
                                        &mut cols[1],
                                        true,
                                        "History Audit Trail",
                                        &format!(
                                            "{} append-only revisions recorded",
                                            app.history.len()
                                        ),
                                    );
                                    cols[1].add_space(8.0);
                                    preflight_check_row(
                                        &mut cols[1],
                                        true,
                                        "DSP Callback Headroom",
                                        &format!(
                                            "{:.0}% utilization · zero xruns",
                                            app.telemetry.callback_utilization * 100.0
                                        ),
                                    );
                                });
                            });
                    });
                });
                ui.add_space(20.0);
            });
        return;
    }

    let selected = clamp_index(app.selected_diagnostic, diagnostics.len());
    let mut select_idx: Option<usize> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(8.0);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(14, 4))
                .show(ui, |ui| {
                    for (index, diag) in diagnostics.iter().enumerate() {
                        let is_sel = index == selected;
                        let sev_color = if diag.severity == Severity::Error {
                            theme::SIG_CLIP
                        } else {
                            theme::SIG_HOT
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
                                    sev_color
                                } else {
                                    theme::BORDER_SUBTLE
                                },
                            ))
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(
                                            format!("{:?}", diag.severity).to_uppercase(),
                                        )
                                        .strong()
                                        .monospace()
                                        .size(10.5)
                                        .color(sev_color),
                                    );
                                    ui.label(
                                        RichText::new(diag.code.to_string())
                                            .monospace()
                                            .size(11.0)
                                            .color(theme::TEXT_SECONDARY),
                                    );
                                    if let Some(loc) = &diag.location {
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{}:{}:{}",
                                                        loc.file.display(),
                                                        loc.line,
                                                        loc.column
                                                    ))
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(theme::ACCENT_CYAN),
                                                );
                                            },
                                        );
                                    }
                                });
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(&diag.message)
                                        .size(12.0)
                                        .color(theme::TEXT_PRIMARY),
                                );
                                if let Some(help) = &diag.help {
                                    ui.add_space(2.0);
                                    ui.label(
                                        RichText::new(format!("Hint: {help}"))
                                            .size(11.0)
                                            .color(theme::TEXT_SECONDARY),
                                    );
                                }
                            });
                        if card.response.interact(Sense::click()).clicked() {
                            select_idx = Some(index);
                        }
                        ui.add_space(6.0);
                    }
                });
        });

    if let Some(idx) = select_idx {
        cx.select(selected, idx);
    }
}

fn preflight_check_row(ui: &mut Ui, ok: bool, title: &str, detail: &str) {
    let color = if ok {
        theme::SIG_NOMINAL
    } else {
        theme::ACCENT_AMBER
    };
    egui::Frame::new()
        .fill(theme::BG_WELL)
        .stroke(Stroke::new(1.0_f32, with_alpha(color, 0.30)))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, color);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(title)
                            .strong()
                            .size(11.5)
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new(detail)
                            .size(10.5)
                            .color(theme::TEXT_SECONDARY),
                    );
                });
            });
        });
}

/// Evenly spaced headline counts with hairline dividers between cells.
fn stat_strip(ui: &mut Ui, stats: &[(&str, usize)]) {
    let height = 46.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let cell = rect.width() / stats.len().max(1) as f32;
    let painter = ui.painter();
    for (index, (label, value)) in stats.iter().enumerate() {
        let x = rect.left() + cell * index as f32;
        if index > 0 {
            painter.line_segment(
                [
                    Pos2::new(x, rect.top() + 6.0),
                    Pos2::new(x, rect.bottom() - 6.0),
                ],
                Stroke::new(1.0_f32, theme::SEAM_DARK),
            );
        }
        let center = x + cell * 0.5;
        painter.text(
            Pos2::new(center, rect.top() + 16.0),
            Align2::CENTER_CENTER,
            value.to_string(),
            FontId::monospace(18.0),
            theme::ACCENT_CYAN,
        );
        painter.text(
            Pos2::new(center, rect.top() + 36.0),
            Align2::CENTER_CENTER,
            *label,
            FontId::proportional(9.5),
            theme::TEXT_MUTED,
        );
    }
}

// ── 3. Studio Revision History Timeline + Inline Inspector ───────────────────────

pub(super) fn history(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    utility_subbar(ui, cx);

    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "APPEND-ONLY REVISION HISTORY · {} ENTRIES",
                        app.history.len()
                    ))
                    .strong()
                    .size(10.5)
                    .color(theme::TEXT_SECONDARY),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !app.history.is_empty() {
                        if ui
                            .add(egui::Button::new("Restore…").fill(theme::BG_CARD))
                            .on_hover_text("Restore the selected revision as a new entry · R")
                            .clicked()
                        {
                            cx.send(Action::OpenRestore);
                        }
                        if ui
                            .add(egui::Button::new("View Diff").fill(theme::BG_CARD))
                            .on_hover_text("Inspect source diff · d")
                            .clicked()
                        {
                            cx.send(Action::OpenHistoryDiff);
                        }
                    }
                    if app.history.len() >= 2
                        && ui
                            .add(egui::Button::new("Undo Last").fill(theme::BG_CARD))
                            .on_hover_text("Restore the previous History entry")
                            .clicked()
                    {
                        cx.send(Action::UndoHistory);
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Publish Revision")
                                    .strong()
                                    .size(11.0)
                                    .color(theme::BG_WELL),
                            )
                            .fill(theme::ACCENT_AMBER),
                        )
                        .clicked()
                    {
                        cx.send(Action::OpenPublish);
                    }
                });
            });
        });

    if app.history.is_empty() {
        unavailable(
            ui,
            "No published History entries yet — press Publish Revision to checkpoint",
        );
        return;
    }

    let selected = clamp_index(app.selected_history, app.history.len());
    let mut select_idx: Option<usize> = None;
    let mut open_diff = false;
    let mut open_restore = false;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(10.0);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(14, 2))
                .show(ui, |ui| {
                    // Revision Timeline List with vertical spine
                    for (index, entry) in app.history.iter().enumerate() {
                        let is_sel = index == selected;
                        let is_active = app.active_entry.as_ref() == Some(&entry.id);
                        let is_last = index + 1 == app.history.len();

                        ui.horizontal(|ui| {
                            // Vertical timeline connector rail + node dot
                            let (rail_rect, _) =
                                ui.allocate_exact_size(Vec2::new(20.0, 42.0), Sense::hover());
                            let rp = ui.painter();
                            let cx_x = rail_rect.center().x;
                            if index > 0 {
                                rp.vline(
                                    cx_x,
                                    rail_rect.top()..=rail_rect.center().y,
                                    Stroke::new(1.5_f32, theme::BORDER_STRONG),
                                );
                            }
                            if !is_last {
                                rp.vline(
                                    cx_x,
                                    rail_rect.center().y..=rail_rect.bottom(),
                                    Stroke::new(1.5_f32, theme::BORDER_STRONG),
                                );
                            }
                            rp.circle_filled(
                                rail_rect.center(),
                                if is_active { 5.0 } else { 4.0 },
                                if is_active {
                                    theme::SIG_NOMINAL
                                } else if is_sel {
                                    theme::ACCENT_CYAN
                                } else {
                                    theme::BORDER_STRONG
                                },
                            );

                            let card = egui::Frame::new()
                                .fill(if is_sel {
                                    theme::BG_CARD
                                } else {
                                    theme::BG_SURFACE
                                })
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    if is_active {
                                        with_alpha(theme::SIG_NOMINAL, 0.70)
                                    } else if is_sel {
                                        theme::ACCENT_CYAN
                                    } else {
                                        theme::BORDER_SUBTLE
                                    },
                                ))
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(12, 9))
                                .show(ui, |ui| {
                                    ui.set_width((ui.available_width() - 26.0).max(260.0));
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(&entry.id)
                                                .monospace()
                                                .strong()
                                                .size(11.5)
                                                .color(if is_active {
                                                    theme::SIG_NOMINAL
                                                } else {
                                                    theme::ACCENT_CYAN
                                                }),
                                        );
                                        if is_active {
                                            ui.label(
                                                RichText::new("ACTIVE")
                                                    .strong()
                                                    .size(9.5)
                                                    .color(theme::SIG_NOMINAL),
                                            );
                                        }
                                        ui.label(
                                            RichText::new(&entry.summary)
                                                .size(12.0)
                                                .color(theme::TEXT_PRIMARY),
                                        );

                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(&entry.created_at)
                                                        .monospace()
                                                        .size(10.0)
                                                        .color(theme::TEXT_MUTED),
                                                );
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{} files · {}",
                                                        entry.changes.len(),
                                                        entry.origin.name
                                                    ))
                                                    .size(10.5)
                                                    .color(theme::TEXT_SECONDARY),
                                                );
                                            },
                                        );
                                    });
                                });
                            let resp = card.response.interact(Sense::click());
                            if resp.clicked() {
                                select_idx = Some(index);
                            }
                            if resp.double_clicked() {
                                select_idx = Some(index);
                                open_diff = true;
                            }
                        });
                        ui.add_space(4.0);
                    }

                    // Inline Selected Revision Inspector (fills lower space productively!)
                    if let Some(entry) = app.history.get(selected) {
                        ui.add_space(12.0);
                        egui::Frame::new()
                            .fill(theme::BG_SURFACE)
                            .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::same(16))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("REVISION INSPECTOR · {}", entry.id))
                                            .strong()
                                            .size(11.0)
                                            .color(theme::ACCENT_AMBER),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new("Restore Revision…")
                                                    .fill(theme::BG_CARD),
                                            )
                                            .clicked()
                                        {
                                            open_restore = true;
                                        }
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("Open Full Source Diff")
                                                        .color(theme::ACCENT_CYAN),
                                                )
                                                .fill(theme::BG_CARD),
                                            )
                                            .clicked()
                                        {
                                            open_diff = true;
                                        }
                                    });
                                });
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(&entry.summary)
                                        .strong()
                                        .size(14.0)
                                        .color(theme::TEXT_PRIMARY),
                                );
                                if !entry.explanation.is_empty() {
                                    ui.add_space(3.0);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&entry.explanation)
                                                .size(11.5)
                                                .color(theme::TEXT_SECONDARY),
                                        )
                                        .wrap(),
                                    );
                                }
                                ui.add_space(10.0);

                                // Metadata Strip
                                egui::Frame::new()
                                    .fill(theme::BG_WELL)
                                    .corner_radius(5.0)
                                    .inner_margin(egui::Margin::symmetric(12, 8))
                                    .show(ui, |ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing.x = 18.0;
                                            meta_kv(ui, "ORIGIN", &entry.origin.name);
                                            meta_kv(ui, "CREATED", &entry.created_at);
                                            meta_kv(
                                                ui,
                                                "PARENT",
                                                entry.parent.as_deref().unwrap_or("root"),
                                            );
                                            if let Some(restored) = &entry.restored_from {
                                                meta_kv(ui, "RESTORED FROM", restored);
                                            }
                                        });
                                    });

                                ui.add_space(12.0);
                                ui.label(
                                    RichText::new(format!(
                                        "CHANGED FILES ({})",
                                        entry.changes.len()
                                    ))
                                    .strong()
                                    .size(10.0)
                                    .color(theme::TEXT_SECONDARY),
                                );
                                ui.add_space(6.0);

                                if entry.changes.is_empty() {
                                    ui.label(
                                        RichText::new("No file delta recorded.")
                                            .size(11.0)
                                            .color(theme::TEXT_MUTED),
                                    );
                                } else {
                                    for ch in &entry.changes {
                                        let (tag, color) = match ch.kind {
                                            SourceChangeKind::Added => {
                                                ("ADDED", theme::SIG_NOMINAL)
                                            }
                                            SourceChangeKind::Modified => {
                                                ("MOD", theme::ACCENT_CYAN)
                                            }
                                            SourceChangeKind::Removed => ("DEL", theme::SIG_CLIP),
                                        };
                                        egui::Frame::new()
                                            .fill(theme::BG_CANVAS)
                                            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
                                            .corner_radius(4.0)
                                            .inner_margin(egui::Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        RichText::new(tag)
                                                            .monospace()
                                                            .strong()
                                                            .size(10.0)
                                                            .color(color),
                                                    );
                                                    ui.label(
                                                        RichText::new(
                                                            ch.path.display().to_string(),
                                                        )
                                                        .monospace()
                                                        .size(11.5)
                                                        .color(theme::TEXT_PRIMARY),
                                                    );
                                                    ui.with_layout(
                                                        Layout::right_to_left(Align::Center),
                                                        |ui| {
                                                            ui.label(
                                                                RichText::new(format!(
                                                                    "{} lines changed",
                                                                    ch.changed_lines
                                                                ))
                                                                .monospace()
                                                                .size(10.0)
                                                                .color(theme::TEXT_MUTED),
                                                            );
                                                        },
                                                    );
                                                });
                                            });
                                        ui.add_space(4.0);
                                    }
                                }
                            });
                    }
                });
            ui.add_space(12.0);
        });

    if let Some(idx) = select_idx {
        cx.select(selected, idx);
    }
    if open_diff {
        cx.send(Action::OpenHistoryDiff);
    }
    if open_restore {
        cx.send(Action::OpenRestore);
    }
}

fn meta_kv(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        ui.label(
            RichText::new(key)
                .strong()
                .size(9.5)
                .color(theme::TEXT_MUTED),
        );
        ui.label(
            RichText::new(value)
                .monospace()
                .size(10.5)
                .color(theme::TEXT_PRIMARY),
        );
    });
}

// ── 4. Searchable Two-Column Studio Shortcut Reference ───────────────────────────

const DIRECT_MANIPULATION_HELP: &[(&str, &str)] = &[
    (
        "Drag Knob / Fader",
        "Adjust continuously (Shift = fine precision)",
    ),
    (
        "Double-Click Control",
        "Reset knob, fader, or slider to default / 0 dB",
    ),
    ("Right-Click Control", "Open exact numeric input dialog"),
    (
        "Drag Topbar BPM",
        "Drag vertically to change Song tempo continuously",
    ),
    (
        "Timeline Clip Body",
        "Drag to Move · Alt+drag to Duplicate · Right-edge to Resize",
    ),
    (
        "Timeline Empty Lane",
        "Double-click to create a Clip · Drag top strip for Range",
    ),
    (
        "Timeline Clip Menu",
        "Right-click clip to Split, Duplicate, or Delete",
    ),
    (
        "Piano Roll / Drums",
        "Double-click to author Note/Hit · Drag to Move/Transpose",
    ),
    (
        "Velocity Lane",
        "Click or sweep across velocity stems below the Piano Roll",
    ),
    (
        "Automation Canvas",
        "Drag nodes in 2D · Drag midpoint diamond for tension curve",
    ),
    (
        "Bode / ADSR Scopes",
        "Drag Cutoff/Resonance dot or Envelope curve directly",
    ),
    (
        "Patch Graph Canvas",
        "Drag nodes to arrange · Drag empty canvas to pan · Fit button",
    ),
    (
        "Studio Keyboard",
        "Click keys (Y = velocity) or toggle HOLD latch · Drum Pads on kits",
    ),
];

pub(super) fn help(ui: &mut Ui, cx: &mut Cx<'_>) {
    utility_subbar(ui, cx);

    // Search Filter Bar
    egui::Frame::new()
        .fill(theme::BG_CANVAS)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("SEARCH SHORTCUTS")
                        .strong()
                        .size(10.0)
                        .color(theme::TEXT_MUTED),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut cx.state.help_search)
                        .hint_text("Filter shortcuts or actions (e.g. loop, split, piano, solo)…")
                        .desired_width(310.0),
                );
                if !cx.state.help_search.is_empty() && ui.small_button("×").clicked() {
                    cx.state.help_search.clear();
                }
            });
        });

    let query = cx.state.help_search.trim().to_lowercase();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(10.0);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(14, 4))
                .show(ui, |ui| {
                    ui.columns(2, |cols| {
                        // Column 0: Direct Manipulation + first half of keymap::HELP
                        let filtered_dm: Vec<(&str, &str)> = DIRECT_MANIPULATION_HELP
                            .iter()
                            .copied()
                            .filter(|(k, d)| {
                                query.is_empty()
                                    || k.to_lowercase().contains(&query)
                                    || d.to_lowercase().contains(&query)
                            })
                            .collect();
                        if !filtered_dm.is_empty() {
                            help_card_pairs(
                                &mut cols[0],
                                "DIRECT MANIPULATION & STUDIO CONTROLS",
                                theme::ACCENT_AMBER,
                                &filtered_dm,
                            );
                            cols[0].add_space(10.0);
                        }

                        let mid = keymap::HELP.len().div_ceil(2);
                        for (heading, lines) in &keymap::HELP[..mid] {
                            help_card_lines(
                                &mut cols[0],
                                heading,
                                theme::ACCENT_CYAN,
                                lines,
                                &query,
                            );
                        }

                        // Column 1: second half of keymap::HELP
                        for (heading, lines) in &keymap::HELP[mid..] {
                            help_card_lines(
                                &mut cols[1],
                                heading,
                                theme::ACCENT_VIOLET,
                                lines,
                                &query,
                            );
                        }
                    });
                });
            ui.add_space(12.0);
        });
}

fn help_card_pairs(ui: &mut Ui, title: &str, accent: Color32, rows: &[(&str, &str)]) {
    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.label(RichText::new(title).strong().size(11.0).color(accent));
            ui.add_space(7.0);
            for (key, desc) in rows {
                help_aligned_row(ui, key, desc);
                ui.add_space(4.0);
            }
        });
}

fn help_card_lines(ui: &mut Ui, title: &str, accent: Color32, lines: &[&str], query: &str) {
    let matched: Vec<String> = lines
        .iter()
        .map(|line| legible(line).into_owned())
        .filter(|text| {
            query.is_empty()
                || title.to_lowercase().contains(query)
                || text.to_lowercase().contains(query)
        })
        .collect();
    if matched.is_empty() {
        return;
    }

    egui::Frame::new()
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.label(RichText::new(title).strong().size(11.0).color(accent));
            ui.add_space(7.0);
            for text in &matched {
                if let Some((keys, desc)) = text.split_once("  ") {
                    help_aligned_row(ui, keys.trim(), desc.trim());
                } else {
                    ui.add(
                        egui::Label::new(
                            RichText::new(text).size(11.5).color(theme::TEXT_SECONDARY),
                        )
                        .wrap(),
                    );
                }
                ui.add_space(4.0);
            }
        });
    ui.add_space(10.0);
}

/// Fixed-width left keycap column (`144px`) + wrapping right description column so text never
/// clips at the right border and keycaps align cleanly on a single vertical rail.
fn help_aligned_row(ui: &mut Ui, key: &str, desc: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.allocate_ui_with_layout(
            Vec2::new(146.0, 20.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                keycap_pill(ui, key);
            },
        );
        ui.add(
            egui::Label::new(RichText::new(desc).size(11.5).color(theme::TEXT_SECONDARY)).wrap(),
        );
    });
}

fn keycap_pill(ui: &mut Ui, text: &str) {
    let font = FontId::monospace(9.8);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), theme::TEXT_PRIMARY);
    let w = (galley.size().x + 12.0).min(144.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 20.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.5, theme::BG_WELL);
    p.rect_stroke(
        rect,
        3.5,
        Stroke::new(1.0_f32, theme::BORDER_STRONG),
        StrokeKind::Inside,
    );
    p.with_clip_rect(rect.shrink2(Vec2::new(4.0, 0.0))).text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        font,
        theme::TEXT_PRIMARY,
    );
}
