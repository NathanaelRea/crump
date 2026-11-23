//! egui desktop client over the client-independent [`Workspace`] session.
//!
//! This is the only part of the workspace that depends on a windowing library. Like the terminal
//! client it draws the session without changing it, and every change it makes goes through
//! [`Workspace::dispatch`]. Keys go through the shared keymap, so the window is keyboard-complete
//! in the same way as the terminal; pointer input is layered on top as further [`Action`] values.

use std::{
    collections::BTreeMap,
    process::Command,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    Ui, Vec2,
};

use super::keymap::Keymap;
use super::*;

mod arrangement;
mod input;
mod views;
mod widgets;

#[cfg(test)]
mod tests;

const TICK_RATE: Duration = Duration::from_millis(33);
const BACKGROUND_POLL_RATE: Duration = Duration::from_millis(5);
const REFRESH_RATE: Duration = Duration::from_millis(400);

/// Colours shared by every view.
mod palette {
    use super::Color32;

    pub(super) const ACCENT: Color32 = Color32::from_rgb(64, 200, 220);
    pub(super) const GOOD: Color32 = Color32::from_rgb(104, 204, 128);
    pub(super) const WARN: Color32 = Color32::from_rgb(232, 196, 88);
    pub(super) const BAD: Color32 = Color32::from_rgb(236, 100, 100);
    pub(super) const MUTED: Color32 = Color32::from_rgb(138, 146, 158);
    pub(super) const CLIP: Color32 = Color32::from_rgb(40, 88, 140);
    pub(super) const CLIP_EDGE: Color32 = Color32::from_rgb(104, 164, 232);
    pub(super) const LANE: Color32 = Color32::from_rgb(24, 27, 32);
    pub(super) const LANE_ALTERNATE: Color32 = Color32::from_rgb(29, 32, 38);
    pub(super) const GRID_BAR: Color32 = Color32::from_rgb(62, 68, 78);
    pub(super) const GRID_BEAT: Color32 = Color32::from_rgb(40, 44, 52);
}

/// Runs the desktop workspace window for one Project.
pub fn run(path: impl AsRef<Path>) -> Result<(), WorkspaceError> {
    let app = Workspace::open(path)?;
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Crump")
        .with_app_id("crump")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([720.0, 420.0]);
    if let Ok(icon) =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/branding/icon-192.png"))
    {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "crump",
        options,
        Box::new(move |creation| {
            style(&creation.egui_ctx);
            Ok(Box::new(Client {
                app,
                keymap: Keymap::default(),
                state: UiState::default(),
                next_refresh: Instant::now() + REFRESH_RATE,
            }))
        }),
    )
    .map_err(|error| WorkspaceError::Gui(error.to_string()))
}

fn style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.visuals.selection.bg_fill = Color32::from_rgb(24, 92, 108);
        style.visuals.selection.stroke = Stroke::new(1.0_f32, palette::ACCENT);
        style.visuals.hyperlink_color = palette::ACCENT;
        style.interaction.selectable_labels = false;
    });
}

/// Window-only state: nothing here is part of the workspace session.
#[derive(Default)]
struct UiState {
    /// The selection each table last scrolled to, so a table follows keyboard selection without
    /// fighting the scroll wheel.
    scrolled: BTreeMap<&'static str, usize>,
    /// Identity and text buffer of the input modal being edited.
    modal_input: Option<(String, String)>,
    /// Enter was pressed in the input modal and is applied once its text field has run.
    modal_submit: bool,
    modal_scroll: u16,
    source_scroll: Option<usize>,
    pan_scroll: f32,
    zoom_scroll: f32,
    piano_key: Option<char>,
}

/// One frame of drawing: the session to read, window state, and the actions the frame produced.
struct Cx<'a> {
    app: &'a Workspace,
    state: &'a mut UiState,
    actions: Vec<Action>,
}

impl Cx<'_> {
    fn send(&mut self, action: impl Into<Action>) {
        self.actions.push(action.into());
    }

    /// Moves the current view's row selection from `current` to `target`.
    fn select(&mut self, current: usize, target: usize) {
        if current != target {
            self.send(Action::MoveSelection(target as isize - current as isize));
        }
    }
}

struct Client {
    app: Workspace,
    keymap: Keymap,
    state: UiState,
    next_refresh: Instant,
}

impl eframe::App for Client {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        input::take_keys(
            raw_input,
            &mut self.app,
            &mut self.keymap,
            &mut self.state.modal_submit,
        );
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.tick();
        if Instant::now() >= self.next_refresh {
            self.app.poll_project_files();
            self.next_refresh = Instant::now() + REFRESH_RATE;
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            self.app.dispatch(Action::Quit);
        }

        let actions = draw(ctx, &self.app, &mut self.state);
        let acted = !actions.is_empty();
        for action in actions {
            self.app.dispatch(action);
        }
        if self.app.take_editor_request() {
            open_source_file(&mut self.app);
        }
        if self.app.quit_requested() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let until_refresh = self.next_refresh.saturating_duration_since(Instant::now());
        let wait = if acted {
            Duration::ZERO
        } else if self.app.has_pending_work() {
            BACKGROUND_POLL_RATE.min(until_refresh)
        } else if self.app.is_animating() {
            TICK_RATE.min(until_refresh)
        } else {
            until_refresh
        };
        ctx.request_repaint_after(wait);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.app.shutdown();
        if let Some(report) = self.app.performance.report() {
            eprintln!("{report}");
        }
    }
}

/// Opens the selected source file with the desktop's default application.
///
/// A window has no terminal to lend to `$EDITOR`, so the file goes to `xdg-open` and the
/// workspace picks up saved changes through its usual file watch.
fn open_source_file(app: &mut Workspace) {
    let status = match app.source_edit_path() {
        Ok(Some(path)) => match Command::new("xdg-open").arg(&path).spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                format!(
                    "Opened {} · saved changes reload automatically",
                    path.strip_prefix(&app.root).unwrap_or(&path).display()
                )
            }
            Err(error) => format!("Could not launch xdg-open: {error}"),
        },
        Ok(None) => return,
        Err(error) => format!("Could not prepare the source file for editing: {error}"),
    };
    app.dispatch(Action::SetStatus(status));
}

/// Draws one frame and returns the actions its widgets produced.
fn draw(ctx: &egui::Context, app: &Workspace, state: &mut UiState) -> Vec<Action> {
    let mut cx = Cx {
        app,
        state,
        actions: Vec::new(),
    };
    egui::TopBottomPanel::top("crump-header").show(ctx, |ui| header(ui, &mut cx));
    egui::TopBottomPanel::top("crump-views").show(ctx, |ui| view_tabs(ui, &mut cx));
    egui::TopBottomPanel::bottom("crump-status").show(ctx, |ui| footer(ui, &cx));
    if keymap::NOTE_VIEWS.contains(&app.view) {
        egui::TopBottomPanel::bottom("crump-notes")
            .show(ctx, |ui| widgets::note_strip(ui, &mut cx));
    }
    egui::CentralPanel::default().show(ctx, |ui| {
        if loading(app) {
            ui.centered_and_justified(|ui| ui.spinner());
        } else {
            views::view(ui, &mut cx);
        }
    });
    modal(ctx, &mut cx);
    cx.actions
}

fn header(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let project_name = match app.bundle.project.as_ref() {
            Some(project) => project.name.as_str(),
            None if loading(app) => "Loading…",
            None => "Unparsable Project",
        };
        ui.label(
            RichText::new(format!("CRUMP · {project_name}"))
                .strong()
                .size(16.0)
                .color(palette::ACCENT),
        );
        let (state, color) = if loading(app) {
            ("LOADING", palette::MUTED)
        } else if app.bundle.diagnostics.has_errors() {
            ("INVALID CANDIDATE", palette::BAD)
        } else if app.candidate_is_active() {
            ("ACTIVE", palette::GOOD)
        } else {
            ("VALID CANDIDATE", palette::WARN)
        };
        ui.label(RichText::new(state).strong().color(color));
        ui.label(
            RichText::new(format!(
                "C:{}  A:{}",
                short_revision(&app.bundle.revision),
                short_optional(app.active_revision.as_ref())
            ))
            .monospace()
            .color(palette::MUTED),
        )
        .on_hover_text("Candidate and Active Revision");
        if app.note_mode {
            ui.label(RichText::new("NOTE MODE").strong().color(palette::WARN));
        } else if app.timeline_state.visual() && app.view == View::Timeline {
            ui.label(RichText::new("RANGE MODE").strong().color(palette::GOOD));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            widgets::meter(ui, app.telemetry.engine.current_peak, 150.0)
                .on_hover_text("Monitor peak");
        });
    });
    ui.horizontal(|ui| {
        let play = if app.player.is_none() {
            "▶ Play"
        } else if app.playback_paused {
            "▶ Resume"
        } else {
            "⏸ Pause"
        };
        if ui.button(play).on_hover_text("Space").clicked() {
            cx.send(Action::TogglePlayback);
        }
        if ui.button("⏹ Stop").on_hover_text("S").clicked() {
            cx.send(Action::StopPlayback);
        }
        ui.separator();
        if ui
            .button("Publish…")
            .on_hover_text("Publish the Candidate as the Active Revision · P")
            .clicked()
        {
            cx.send(Action::OpenPublish);
        }
        if ui
            .button("Reload")
            .on_hover_text("Reload source and History from disk · r")
            .clicked()
        {
            cx.send(Action::Reload);
        }
        ui.separator();
        ui.label(RichText::new(transport_summary(app)).monospace());
    });
    ui.add_space(2.0);
}

/// Whether the session is still reading the Project for the first time.
fn loading(app: &Workspace) -> bool {
    app.bundle.project.is_none() && app.bundle.revision.0.is_empty() && app.has_pending_work()
}

/// Transport, audition, and stream facts, matching the terminal header.
fn transport_summary(app: &Workspace) -> String {
    if app.player.is_none() {
        return format!(
            "stopped · armed {} via {:?}",
            audition_target_spec(&app.audition_plan.target),
            app.audition_plan.path
        );
    }
    let audition = app
        .audition_status
        .as_ref()
        .map_or_else(String::new, |status| {
            format!(
                " · {} via {:?}{}",
                audition_target_spec(&status.target),
                status.path,
                if status.looping { " LOOP" } else { "" }
            )
        });
    format!(
        "{}{}{audition} · {:?}{} · voices {} · MIDI {}",
        transport_label(app),
        if app.playback_paused { " PAUSED" } else { "" },
        app.telemetry.stream_state,
        if app.telemetry.transition_active {
            " XFADE/RESTART"
        } else {
            ""
        },
        app.telemetry.engine.active_voices,
        if app.telemetry.midi_connected {
            "connected"
        } else {
            "off"
        }
    )
}

fn view_tabs(ui: &mut Ui, cx: &mut Cx<'_>) {
    ui.horizontal_wrapped(|ui| {
        for (index, view) in VIEWS.iter().enumerate() {
            let key = match index {
                0..=8 => (index + 1).to_string(),
                9 => "0".to_owned(),
                10 => "—".to_owned(),
                11 => "H".to_owned(),
                _ => "?".to_owned(),
            };
            let mut label = RichText::new(view.title());
            if *view == View::Diagnostics && cx.app.bundle.diagnostics.has_errors() {
                label = label.color(palette::BAD);
            }
            if ui
                .selectable_label(cx.app.view == *view, label)
                .on_hover_text(format!("Key {key}"))
                .clicked()
            {
                cx.send(Action::ShowView(*view));
            }
        }
    });
}

fn footer(ui: &mut Ui, cx: &Cx<'_>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("■").color(palette::ACCENT));
        ui.label(widgets::legible(&cx.app.status));
    });
    if let Some(performance) = cx.app.performance.footer() {
        ui.label(RichText::new(performance).small().color(palette::MUTED));
    }
}

fn modal(ctx: &egui::Context, cx: &mut Cx<'_>) {
    let Some(modal) = &cx.app.modal else {
        cx.state.modal_input = None;
        cx.state.modal_submit = false;
        return;
    };
    let response = egui::Modal::new(egui::Id::new("crump-modal")).show(ctx, |ui| {
        ui.set_width((ctx.content_rect().width() * 0.7).clamp(360.0, 900.0));
        match modal {
            Modal::Input {
                title,
                prompt,
                value,
                replace_on_type,
                ..
            } => input_modal(ui, cx, title, prompt, value, *replace_on_type),
            Modal::ConfirmRestore { entry_id } => {
                ui.heading("Confirm History Restore");
                ui.label(format!("Restore exact state from {entry_id}?"));
                ui.label(
                    "Current source will be replaced atomically and the restored state appended as a new History Entry.",
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Restore").on_hover_text("y / Enter").clicked() {
                        cx.send(Action::ModalSubmit);
                    }
                    if ui.button("Cancel").on_hover_text("n / Esc").clicked() {
                        cx.send(Action::ModalDismiss);
                    }
                });
            }
            Modal::Text {
                title,
                body,
                scroll,
            } => text_modal(ui, cx, title, body, *scroll),
        }
    });
    if response.should_close() {
        cx.send(Action::ModalDismiss);
    }
}

fn input_modal(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    title: &str,
    prompt: &str,
    value: &str,
    select_all: bool,
) {
    ui.heading(widgets::legible(title));
    ui.label(widgets::legible(prompt));
    ui.add_space(6.0);
    let identity = format!("{title}\n{prompt}");
    let opening = cx
        .state
        .modal_input
        .as_ref()
        .is_none_or(|(open, _)| *open != identity);
    let mut buffer = match cx.state.modal_input.take() {
        Some((_, buffer)) if !opening => buffer,
        _ => value.to_owned(),
    };
    let mut output = egui::TextEdit::singleline(&mut buffer)
        .font(egui::TextStyle::Monospace)
        .desired_width(f32::INFINITY)
        .show(ui);
    if opening {
        output.response.request_focus();
        let end = if select_all {
            0
        } else {
            buffer.chars().count()
        };
        output
            .state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(end),
                egui::text::CCursor::new(buffer.chars().count()),
            )));
        output.state.store(ui.ctx(), output.response.id);
    }
    let submit = std::mem::take(&mut cx.state.modal_submit);
    if output.response.changed() || (submit && buffer != value) {
        cx.send(Action::ModalSetInput(buffer.clone()));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Apply").on_hover_text("Enter").clicked() || submit {
            cx.send(Action::ModalSubmit);
        }
        if ui.button("Cancel").on_hover_text("Esc").clicked() {
            cx.send(Action::ModalDismiss);
        }
    });
    cx.state.modal_input = Some((identity, buffer));
}

fn text_modal(ui: &mut Ui, cx: &mut Cx<'_>, title: &str, body: &str, scroll: u16) {
    ui.heading(widgets::legible(title));
    let lines = body.lines().collect::<Vec<_>>();
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    let mut area = egui::ScrollArea::both()
        .auto_shrink([false, true])
        .max_height(ui.ctx().content_rect().height() * 0.6);
    if std::mem::replace(&mut cx.state.modal_scroll, scroll) != scroll {
        area = area.vertical_scroll_offset(f32::from(scroll) * row_height);
    }
    area.show_rows(ui, row_height, lines.len(), |ui, rows| {
        for line in &lines[rows] {
            let color = match line.as_bytes().first() {
                Some(b'+') => palette::GOOD,
                Some(b'-') => palette::BAD,
                Some(b'@') => palette::ACCENT,
                _ => ui.visuals().text_color(),
            };
            ui.label(
                RichText::new(widgets::legible(line))
                    .monospace()
                    .color(color),
            );
        }
    });
    ui.add_space(8.0);
    if ui.button("Close").on_hover_text("Esc").clicked() {
        cx.send(Action::ModalDismiss);
    }
}
