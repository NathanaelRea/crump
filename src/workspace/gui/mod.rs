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
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, Ui, Vec2,
};

use super::keymap::Keymap;
use super::*;
use widgets::TabPress;

mod arrangement;
mod input;
mod mixer;
mod processing;
mod sequencer;
mod sound_design;
mod theme;
mod utility_views;
mod views;
mod widgets;

#[cfg(test)]
mod tests;

const TICK_RATE: Duration = Duration::from_millis(33);
const BACKGROUND_POLL_RATE: Duration = Duration::from_millis(5);
const REFRESH_RATE: Duration = Duration::from_millis(400);

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
    theme::apply_theme(ctx);
}

/// Window-only state: nothing here is part of the workspace session.
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
    /// Live local drag preview values for smooth 60fps knob/fader/slider response before commit.
    drag_previews: BTreeMap<String, f64>,
    /// Committed values shown until their edit reloads; see [`UiState::settle_held_previews`].
    held_previews: BTreeMap<String, f64>,
    held_revision: Option<crate::project::RevisionId>,
    /// Toggles clicked whose edit has not reloaded yet, shown in their new state meanwhile.
    held_flags: BTreeMap<String, bool>,
    /// Studio keyboard base octave offset (`C3` = 3).
    keyboard_octave: i8,
    /// Sustain / hold latch for the on-screen studio keyboard.
    keyboard_latch: bool,
    /// Whether the docked studio keyboard strip is visible on sound/sequencer views.
    show_keyboard: bool,
    /// Optional compact table mode toggle in Mixer view.
    mixer_table_mode: bool,
    /// Mixer bank filter (`0=All, 1=Tracks, 2=Groups, 3=Returns`).
    mixer_bank_filter: u8,
    /// Channel-scope filter in Processing FX Rack (`None` = All channels).
    processing_owner_filter: Option<String>,
    /// Optional table mode toggle in Processing view.
    processing_table_mode: bool,
    /// Piano Roll snap grid in quarter notes (`0.25` = 1/16th, `0.0` = Off).
    roll_snap_quarters: f64,
    /// Piano Roll horizontal zoom multiplier.
    roll_zoom: f32,
    /// Currently sounding piano-gutter audition key so pointer release emits NoteOff.
    gutter_audition_key: Option<char>,
    /// Collapsible event list inspector drawer in Piano Roll.
    event_table_open: bool,
    /// Collapsible point list inspector drawer in Automation.
    automation_table_open: bool,
    /// Automation snap grid in quarter notes (`0.25` = 1/16th, `0.0` = Off).
    auto_snap_quarters: f64,
    /// Automation horizontal zoom multiplier.
    auto_zoom: f32,
    /// Active Automation segment midpoint curvature drag `(point_index, live_power)`.
    auto_curve_drag: Option<(usize, f64)>,
    /// Active Timeline clip drag `(clip_id, is_resize, is_duplicate, orig_start_q, orig_dur_q, live_q)`.
    clip_drag: Option<(String, bool, bool, f64, f64, f64)>,
    /// Active Piano Roll note drag `(event_index, is_resize, live_at_q, live_row_delta, live_dur_q)`.
    note_drag: Option<(usize, bool, f64, i32, f64)>,
    /// Active Automation breakpoint 2D drag `point_index`.
    auto_point_drag: Option<usize>,
    /// Search query in Parameters Deck.
    param_search: String,
    /// Parameter Deck filter mode (`0=All, 1=Automated, 2=Modulated`).
    param_filter_mode: u8,
    /// Search query in Shortcuts Reference.
    help_search: String,
    /// Last active sub-views per primary mode.
    last_mix_view: View,
    last_edit_view: View,
    last_sound_view: View,
    /// Pan offset, zoom scale, auto-fit marker, and user node drag offsets for Patch Graph.
    graph_pan: Vec2,
    graph_zoom: f32,
    graph_fitted_for: Option<String>,
    graph_node_offsets: BTreeMap<String, Vec2>,
    /// The status line last drawn and when it first appeared, so hints can expire.
    status_seen: Option<(String, Instant)>,
    /// The view drawn last frame, and the last one that was not Diagnostics, so the window can
    /// step off an empty Diagnostics list the way it came.
    last_view: View,
    before_diagnostics: View,
    /// The view the last frame drew, to spot a page switch.
    drawn_view: Option<View>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            scrolled: BTreeMap::new(),
            modal_input: None,
            modal_submit: false,
            modal_scroll: 0,
            source_scroll: None,
            pan_scroll: 0.0,
            zoom_scroll: 0.0,
            piano_key: None,
            drag_previews: BTreeMap::new(),
            held_previews: BTreeMap::new(),
            held_revision: None,
            held_flags: BTreeMap::new(),
            keyboard_octave: 3,
            keyboard_latch: false,
            show_keyboard: true,
            mixer_table_mode: false,
            mixer_bank_filter: 0,
            processing_owner_filter: None,
            processing_table_mode: false,
            roll_snap_quarters: 0.25,
            roll_zoom: 1.0,
            gutter_audition_key: None,
            event_table_open: false,
            automation_table_open: false,
            auto_snap_quarters: 0.25,
            auto_zoom: 1.0,
            auto_curve_drag: None,
            clip_drag: None,
            note_drag: None,
            auto_point_drag: None,
            param_search: String::new(),
            param_filter_mode: 0,
            help_search: String::new(),
            last_mix_view: View::Mixer,
            last_edit_view: View::EventGrid,
            last_sound_view: View::Faceplate,
            graph_pan: Vec2::ZERO,
            graph_zoom: 1.0,
            graph_fitted_for: None,
            graph_node_offsets: BTreeMap::new(),
            status_seen: None,
            last_view: View::Timeline,
            before_diagnostics: View::Timeline,
            drawn_view: None,
        }
    }
}

impl UiState {
    /// The value a control shows instead of the session's: a drag in progress, or a value it
    /// committed whose edit has not reloaded yet.
    fn drag_preview(&self, key: &str) -> Option<f64> {
        self.drag_previews
            .get(key)
            .or_else(|| self.held_previews.get(key))
            .copied()
    }

    fn set_drag_preview(&mut self, key: &str, value: f64) {
        self.drag_previews.insert(key.to_owned(), value);
    }

    /// Ends a drag and keeps showing its final value until the edit it commits reloads.
    fn take_drag_preview(&mut self, key: &str) -> Option<f64> {
        let value = self.drag_previews.remove(key)?;
        self.held_previews.insert(key.to_owned(), value);
        Some(value)
    }

    /// Shows `value` until the edit committing it reloads, such as a double-click reset.
    fn hold_preview(&mut self, key: &str, value: f64) {
        self.drag_previews.remove(key);
        self.held_previews.insert(key.to_owned(), value);
    }

    /// A toggle's state as shown: the session's, unless a click on it has not reloaded yet.
    fn flag(&self, key: &str, actual: bool) -> bool {
        self.held_flags.get(key).copied().unwrap_or(actual)
    }

    /// Shows a toggle in `value` until the edit committing it reloads.
    fn hold_flag(&mut self, key: &str, value: bool) {
        self.held_flags.insert(key.to_owned(), value);
    }

    /// Ends a drag that commits nothing.
    fn cancel_drag_preview(&mut self, key: &str) {
        self.drag_previews.remove(key);
    }

    /// Drops held values once the session has reloaded past them or has no edit outstanding,
    /// whether it landed or failed.
    fn settle_held_previews(&mut self, app: &Workspace) {
        let revision = &app.bundle.revision;
        if self.held_revision.as_ref() != Some(revision) || !app.edits_pending() {
            self.held_previews.clear();
            self.held_flags.clear();
            self.held_revision = Some(revision.clone());
        }
    }
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

pub(super) fn inline_text_focused(ctx: &egui::Context, app: &Workspace) -> bool {
    app.modal.is_none()
        && matches!(app.view, View::Parameters | View::Help)
        && ctx.wants_keyboard_input()
}

impl eframe::App for Client {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let focused = inline_text_focused(ctx, &self.app);
        input::take_keys(
            raw_input,
            &mut self.app,
            &mut self.keymap,
            &mut self.state.modal_submit,
            focused,
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

        settle_view(&mut self.app, &mut self.state);
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

/// Keeps the window off an empty Diagnostics list. Diagnostics have no page of their own in the
/// window, only a chip while problems exist, so Tab steps over the list and fixing the last
/// problem returns to the view it was opened from.
fn settle_view(app: &mut Workspace, state: &mut UiState) {
    if app.view == View::Diagnostics && app.bundle.diagnostics.as_slice().is_empty() {
        app.dispatch(match state.last_view {
            View::Source => Action::CycleView(1),
            View::History => Action::CycleView(-1),
            _ => Action::ShowView(state.before_diagnostics),
        });
    }
    state.last_view = app.view;
    if app.view != View::Diagnostics {
        state.before_diagnostics = app.view;
    }
}

/// Opens the selected source file with the desktop's default application.
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
    state.settle_held_previews(app);
    let mut cx = Cx {
        app,
        state,
        actions: Vec::new(),
    };

    // Unified 44px Studio Top Transport & Navigation Bar
    egui::TopBottomPanel::top("crump-topbar")
        .exact_height(44.0)
        .frame(
            egui::Frame::new()
                .fill(theme::BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
                .inner_margin(egui::Margin::symmetric(10, 6)),
        )
        .show(ctx, |ui| studio_top_bar(ui, &mut cx));

    // Slim 22px Studio Status Footer
    egui::TopBottomPanel::bottom("crump-status")
        .exact_height(22.0)
        .frame(
            egui::Frame::new()
                .fill(theme::BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
                .inner_margin(egui::Margin::symmetric(10, 3)),
        )
        .show(ctx, |ui| footer(ui, &mut cx));

    // Tactile Studio Keyboard / Drum Pad Dock on Sound & Event Grid views
    let keyboard_eligible = keymap::NOTE_VIEWS.contains(&app.view) || app.view == View::EventGrid;
    if keyboard_eligible && cx.state.show_keyboard {
        // One panel memory per view: the drum pads and the keyboard differ in height.
        egui::TopBottomPanel::bottom(egui::Id::new((
            "crump-notes",
            std::mem::discriminant(&app.view),
        )))
        .frame(
            egui::Frame::new()
                .fill(theme::BG_SURFACE)
                .stroke(Stroke::new(1.0_f32, theme::SEAM_DARK))
                .inner_margin(egui::Margin::symmetric(10, 5)),
        )
        .show(ctx, |ui| widgets::note_strip(ui, &mut cx));
    }

    // A view's first frame measures itself (scroll extents, panel heights); discard that pass
    // so a page switch is drawn once, already laid out, instead of settling visibly.
    if cx.state.drawn_view != Some(app.view) {
        cx.state.drawn_view = Some(app.view);
        ctx.request_discard("view changed");
    }

    // Primary Studio Canvas (zero extra padding so Timeline / Mixer / Piano Roll fill edge-to-edge)
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::BG_CANVAS))
        .show(ctx, |ui| {
            if loading(app) {
                ui.centered_and_justified(|ui| ui.spinner());
            } else {
                views::view(ui, &mut cx);
            }
        });

    modal(ctx, &mut cx);
    cx.actions
}

/// Primary 4-mode studio navigation grouping for the 13 views.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PrimaryMode {
    Arrange,
    Mix,
    Edit,
    Sound,
    Utility,
}

fn primary_mode_of(view: View) -> PrimaryMode {
    match view {
        View::Timeline => PrimaryMode::Arrange,
        View::Mixer | View::Processing => PrimaryMode::Mix,
        View::EventGrid | View::Automation => PrimaryMode::Edit,
        View::Faceplate | View::Instrument | View::Graph | View::Parameters => PrimaryMode::Sound,
        View::Source | View::Diagnostics | View::History | View::Help => PrimaryMode::Utility,
    }
}

/// Unified single-row 44px Studio Top Bar:
/// Left: 4 Primary Mode Pills (`Arrange | Mix | Edit | Sound`)
/// Center: Recessed Hardware Transport Deck (Vector RTZ / Play / Stop / Loop + LCD Bar:Beat / BPM / Meter / Target)
/// Right: Utility micro-pills (`Src | Diag | Hist | ?`) + Publish/Undo + DSP% + Master dBFS Meter + Project Badge
fn studio_top_bar(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    let mode = primary_mode_of(app.view);
    match app.view {
        View::Mixer | View::Processing => cx.state.last_mix_view = app.view,
        View::EventGrid | View::Automation => cx.state.last_edit_view = app.view,
        View::Faceplate | View::Instrument | View::Graph | View::Parameters => {
            cx.state.last_sound_view = app.view;
        }
        _ => {}
    }

    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        // ── Left: Brand mark + 4 Primary Mode Segmented Control ──
        let (mark, _) = ui.allocate_exact_size(Vec2::new(6.0, 18.0), Sense::hover());
        ui.painter().rect_filled(mark, 2.0, theme::ACCENT_AMBER);

        egui::Frame::new()
            .fill(theme::BG_WELL)
            .corner_radius(5.0)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
            .inner_margin(egui::Margin::same(2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (m, label, target, tip) in [
                        (
                            PrimaryMode::Arrange,
                            "Arrange",
                            View::Timeline,
                            "Timeline Arrangement · Key 1",
                        ),
                        (
                            PrimaryMode::Mix,
                            "Mix",
                            cx.state.last_mix_view,
                            "Mixing Console & FX Rack · Key 2 / 3",
                        ),
                        (
                            PrimaryMode::Edit,
                            "Edit",
                            cx.state.last_edit_view,
                            "Piano Roll & Automation · Key 4 / 8",
                        ),
                        (
                            PrimaryMode::Sound,
                            "Sound",
                            cx.state.last_sound_view,
                            "Faceplate, Synth, Graph & Params · Key 5-7, 9",
                        ),
                    ] {
                        if widgets::pill_toggle(ui, label, mode == m, theme::ACCENT_AMBER, tip)
                            .tab_pressed()
                        {
                            cx.send(Action::ShowView(target));
                        }
                    }
                });
            });

        ui.add_space(6.0);

        // ── Center: Recessed Hardware Transport Deck ──
        let playing = app.player.is_some() && !app.playback_paused;
        let looping = app.audition_status.as_ref().is_some_and(|s| s.looping)
            || app.audition_plan.range.looping;

        egui::Frame::new()
            .fill(theme::BG_WELL)
            .corner_radius(5.0)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
            .inner_margin(egui::Margin::symmetric(6, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;

                    if widgets::icon_button(
                        ui,
                        Vec2::new(24.0, 22.0),
                        false,
                        theme::TEXT_PRIMARY,
                        "Return to Zero (Bar 1)",
                        theme::paint_rtz_icon,
                    )
                    .clicked()
                    {
                        cx.send(Action::Timeline(TimelineAction::MoveSongEdge {
                            end: false,
                        }));
                    }

                    if widgets::icon_button(
                        ui,
                        Vec2::new(28.0, 22.0),
                        playing,
                        theme::SIG_NOMINAL,
                        if playing {
                            "Pause · Space"
                        } else {
                            "Play · Space"
                        },
                        |p, r, c| {
                            if playing {
                                theme::paint_pause_icon(p, r, c);
                            } else {
                                theme::paint_play_icon(p, r, c);
                            }
                        },
                    )
                    .clicked()
                    {
                        cx.send(Action::TogglePlayback);
                    }

                    if widgets::icon_button(
                        ui,
                        Vec2::new(24.0, 22.0),
                        false,
                        theme::SIG_CLIP,
                        "Stop · S",
                        theme::paint_stop_icon,
                    )
                    .clicked()
                    {
                        cx.send(Action::StopPlayback);
                    }

                    if widgets::icon_button(
                        ui,
                        Vec2::new(24.0, 22.0),
                        looping,
                        theme::ACCENT_AMBER,
                        "Toggle Loop · L",
                        theme::paint_loop_icon,
                    )
                    .clicked()
                    {
                        cx.send(Action::ToggleAuditionRangeLoop);
                    }

                    ui.separator();

                    // LCD Bar:Beat Position
                    // One position format everywhere: bar.beat.sixteenth, 1-based.
                    let pos_text = match (&app.bundle.project, app.player.is_some()) {
                        (Some(project), playing) => {
                            let q = if playing {
                                transport_quarter_notes(app)
                            } else {
                                app.timeline_position().at.as_f64()
                            };
                            theme::format_bar_beat(q, project.song.meter)
                        }
                        (None, true) => transport_label(app),
                        (None, false) => "1.1.1".to_owned(),
                    };
                    // Fixed slot sized for `999.4.4`, so the deck never shifts as bars count up.
                    let (pos_rect, pos_resp) =
                        ui.allocate_exact_size(Vec2::new(70.0, 20.0), Sense::hover());
                    ui.painter().text(
                        pos_rect.center(),
                        Align2::CENTER_CENTER,
                        pos_text,
                        FontId::monospace(13.5),
                        theme::TEXT_PRIMARY,
                    );
                    pos_resp.on_hover_text("Transport position (bar.beat.sixteenth)");

                    // Draggable Tempo & Meter from Project
                    if let Some(project) = &app.bundle.project {
                        ui.separator();
                        let bpm_key = "transport-bpm";
                        let live_bpm = cx
                            .state
                            .drag_preview(bpm_key)
                            .unwrap_or(project.song.tempo_bpm);
                        let (bpm_rect, bpm_resp) =
                            ui.allocate_exact_size(Vec2::new(58.0, 20.0), Sense::click_and_drag());
                        if bpm_resp.hovered() || bpm_resp.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                        }
                        if bpm_resp.drag_started() {
                            cx.state.set_drag_preview(bpm_key, live_bpm);
                        }
                        if bpm_resp.dragged() {
                            let fine = ui.input(|i| i.modifiers.shift);
                            let step = if fine { 0.1 } else { 0.5 };
                            let next = (live_bpm - f64::from(bpm_resp.drag_delta().y) * step)
                                .clamp(20.0, 300.0);
                            cx.state.set_drag_preview(bpm_key, next);
                        }
                        if bpm_resp.drag_stopped()
                            && let Some(final_bpm) = cx.state.take_drag_preview(bpm_key)
                        {
                            let rounded = (final_bpm * 2.0).round() / 2.0;
                            if (rounded - project.song.tempo_bpm).abs() > 0.05 {
                                cx.send(Action::SetSongTempo { bpm: rounded });
                            }
                        }
                        ui.painter().rect_filled(
                            bpm_rect,
                            3.0,
                            if bpm_resp.hovered() || bpm_resp.dragged() {
                                theme::BG_HOVER
                            } else {
                                Color32::TRANSPARENT
                            },
                        );
                        ui.painter().text(
                            bpm_rect.center(),
                            Align2::CENTER_CENTER,
                            format!("{live_bpm:.0} BPM"),
                            FontId::monospace(11.0),
                            theme::ACCENT_AMBER,
                        );
                        bpm_resp.on_hover_text(
                            "Song Tempo · Drag vertically to adjust · Shift+drag fine",
                        );

                        let meter = &project.song.meter;
                        ui.label(
                            RichText::new(format!("{}/{}", meter.numerator, meter.denominator))
                                .monospace()
                                .size(11.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                    }

                    // Active voices in a fixed slot that stays put when nothing sounds.
                    ui.separator();
                    let voices = app.telemetry.engine.active_voices;
                    let (voice_rect, voice_resp) =
                        ui.allocate_exact_size(Vec2::new(28.0, 20.0), Sense::hover());
                    ui.painter().text(
                        voice_rect.center(),
                        Align2::CENTER_CENTER,
                        format!("{voices}v"),
                        FontId::monospace(10.5),
                        if voices > 0 {
                            theme::TEXT_SECONDARY
                        } else {
                            theme::TEXT_MUTED
                        },
                    );
                    voice_resp.on_hover_text("Active polyphonic synth voices");

                    // Clean clickable Audition Target indicator
                    ui.separator();
                    let target_short = compact_audition_target(app);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(target_short)
                                    .monospace()
                                    .size(10.5)
                                    .color(theme::ACCENT_CYAN),
                            )
                            .fill(theme::BG_SURFACE)
                            .min_size(Vec2::new(96.0, 20.0)),
                        )
                        .on_hover_text("Click to choose Audition target · A")
                        .clicked()
                    {
                        cx.send(Action::OpenAuditionTarget);
                    }
                });
            });

        // ── Right: Utility Icons, Master dBFS Meter, DSP%, Publish, Undo, Project Status ──
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;

            // Utility pills (?, History, Source), rendered right to left. Diagnostics have no
            // page of their own here: a red chip appears only while the Candidate has problems.
            let has_errors = app.bundle.diagnostics.has_errors();
            for (v, label, tip) in [
                (View::Help, "?", "Shortcuts & Help · ?"),
                (View::History, "History", "Revision History · H"),
                (View::Source, "Source", "Authoritative .crump Source · 0"),
            ] {
                let active = app.view == v;
                let width = if label == "?" {
                    22.0
                } else {
                    14.0 + label.chars().count() as f32 * 6.4
                };
                if widgets::led_button(
                    ui,
                    label,
                    active,
                    theme::SELECTION,
                    Vec2::new(width, 22.0),
                    tip,
                )
                .tab_pressed()
                {
                    cx.send(Action::ShowView(v));
                }
            }

            ui.separator();

            // Master Calibrated Logarithmic dBFS Meter (rendered left-to-right internally)
            let (master_peak, master_avg) = master_levels(app);
            widgets::dbfs_meter_horizontal(ui, master_peak, master_avg, 96.0, true)
                .on_hover_text("Master peak and average (dBFS), as on the Master strip");

            // DSP CPU load indicator
            let dsp_pct = (app.telemetry.callback_utilization * 100.0).clamp(0.0, 99.9);
            let dsp_color = if dsp_pct >= 75.0 {
                theme::SIG_CLIP
            } else if dsp_pct >= 45.0 {
                theme::SIG_HOT
            } else {
                theme::TEXT_SECONDARY
            };
            let (dsp_rect, dsp_resp) =
                ui.allocate_exact_size(Vec2::new(54.0, 20.0), Sense::hover());
            ui.painter().text(
                dsp_rect.right_center(),
                Align2::RIGHT_CENTER,
                format!("DSP {dsp_pct:.0}%"),
                FontId::monospace(10.0),
                dsp_color,
            );
            dsp_resp.on_hover_text("Audio callback DSP load");

            ui.separator();

            // Publish & Undo CTAs
            let settling = app.publication_settling();
            let needs_publish = needs_publish(app);
            if widgets::publish_button(ui, "Publish", needs_publish)
                .on_hover_text("Publish Candidate as Active Revision · P")
                .clicked()
            {
                cx.send(Action::OpenPublish);
            }

            let can_undo = app.history.len() > 1;
            if ui
                .add_enabled(
                    can_undo,
                    egui::Button::new(RichText::new("Undo").size(10.5).color(if can_undo {
                        theme::TEXT_PRIMARY
                    } else {
                        theme::TEXT_MUTED
                    }))
                    .fill(theme::BG_CARD),
                )
                .on_hover_text("Restore previous Revision History entry")
                .clicked()
            {
                cx.send(Action::UndoHistory);
            }

            // Compact Project Name (truncated cleanly) + Live Revision Dot
            let project_name = match app.bundle.project.as_ref() {
                Some(project) => project.name.as_str(),
                None if loading(app) => "Loading…",
                None => "Broken",
            };
            let (dot_color, rev_tip) = if loading(app) {
                (theme::TEXT_MUTED, "Loading Project…".to_owned())
            } else if has_errors {
                // The error chip carries the colour; the dot only steps back.
                (
                    theme::TEXT_MUTED,
                    "Invalid Candidate (has diagnostics)".to_owned(),
                )
            } else if app.candidate_is_active() || settling {
                (
                    theme::SIG_NOMINAL,
                    format!(
                        "Active Revision {} (in sync)",
                        short_revision(&app.bundle.revision)
                    ),
                )
            } else {
                (
                    theme::SIG_HOT,
                    format!(
                        "Unpublished Candidate {} (Active: {})",
                        short_revision(&app.bundle.revision),
                        short_optional(app.active_revision.as_ref())
                    ),
                )
            };
            // The name takes whatever the transport cluster leaves free, within reason.
            let name_w = ui
                .painter()
                .layout_no_wrap(
                    project_name.to_owned(),
                    FontId::proportional(11.0),
                    theme::TEXT_PRIMARY,
                )
                .size()
                .x
                .clamp(112.0, 280.0)
                .min((ui.available_width() - 12.0).max(80.0))
                + 4.0;
            ui.add_sized(
                Vec2::new(name_w, 20.0),
                egui::Label::new(
                    RichText::new(project_name)
                        .strong()
                        .size(11.0)
                        .color(theme::TEXT_PRIMARY),
                )
                .truncate(),
            )
            .on_hover_text(rev_tip.clone());

            let (dot, dot_resp) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 3.8, dot_color);
            dot_resp.on_hover_text(rev_tip);

            // Problems sit beside the project they belong to, left of everything that stays put.
            if let Some((label, color)) = issue_chip(app) {
                let width = 14.0 + label.chars().count() as f32 * 6.4;
                if widgets::led_button(
                    ui,
                    &label,
                    true,
                    color,
                    Vec2::new(width, 22.0),
                    "Problems in the Candidate source · click to inspect",
                )
                .tab_pressed()
                {
                    cx.send(Action::ShowView(View::Diagnostics));
                }
            }
        });
    });
}

fn compact_audition_target(app: &Workspace) -> String {
    let raw = app
        .audition_status
        .as_ref()
        .map(|s| audition_target_spec(&s.target))
        .unwrap_or_else(|| audition_target_spec(&app.audition_plan.target));
    if raw.len() > 16 {
        format!("{}…", &raw[..15])
    } else {
        raw
    }
}

/// The Master bus peak and average, from its own meter when the engine reports one, so the top
/// bar and the Master strip always agree.
fn master_levels(app: &Workspace) -> (f32, f32) {
    let engine = &app.telemetry.engine;
    app.mix_channel_indexes
        .get("master")
        .copied()
        .filter(|&index| index < engine.channel_count)
        .map_or((engine.current_peak, engine.current_peak * 0.65), |index| {
            let meter = engine.channel_activity[index];
            (meter.current_peak, meter.average)
        })
}

/// Whether a valid Candidate waits to be published. Only once the session has settled: an edit
/// publishes itself, and its reload must not flash the Publish button on the way through.
fn needs_publish(app: &Workspace) -> bool {
    !app.candidate_is_active()
        && !app.bundle.diagnostics.has_errors()
        && !loading(app)
        && !app.publication_settling()
}

/// The top-bar problem chip: only while the Candidate has diagnostics, red for errors.
fn issue_chip(app: &Workspace) -> Option<(String, Color32)> {
    let diagnostics = app.bundle.diagnostics.as_slice();
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == crate::diagnostic::Severity::Error)
        .count();
    let plural =
        |count: usize, noun: &str| format!("{count} {noun}{}", if count == 1 { "" } else { "s" });
    if errors > 0 {
        Some((plural(errors, "error"), theme::SIG_CLIP))
    } else if !diagnostics.is_empty() {
        Some((plural(diagnostics.len(), "warning"), theme::SIG_HOT))
    } else {
        None
    }
}

/// Whether the session is still reading the Project for the first time.
fn loading(app: &Workspace) -> bool {
    app.bundle.project.is_none() && app.bundle.revision.0.is_empty() && app.has_pending_work()
}

/// How long a hint about a refused action stays in the footer.
const GUIDANCE_LINGER: Duration = Duration::from_secs(6);

/// How the footer treats the session's status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusTone {
    /// Progress and confirmations: the window already shows the outcome, so nothing is said.
    Quiet,
    /// An action was refused or needs something first; shown in amber for a few seconds.
    Guidance,
    /// Something failed; shown in red until the next status replaces it.
    Failure,
}

fn status_tone(status: &str) -> StatusTone {
    const FAILURE: [&str; 8] = [
        "failed",
        "could not",
        "cannot",
        "invalid",
        "rejected",
        "delayed",
        "is stale",
        "error",
    ];
    const GUIDANCE: [&str; 17] = [
        "unavailable",
        "not found",
        " before ",
        " first",
        "requires",
        "must ",
        "has no ",
        "no further",
        "no prior",
        "no clip",
        "no project",
        "no continuous",
        "no timeline",
        "is undefined",
        "is unset",
        "nothing earlier",
        "already",
    ];
    let status = status.to_lowercase();
    if FAILURE.iter().any(|word| status.contains(word)) {
        StatusTone::Failure
    } else if GUIDANCE.iter().any(|word| status.contains(word))
        || status.contains(" as `")
        || status.contains("is the only")
    {
        StatusTone::Guidance
    } else {
        StatusTone::Quiet
    }
}

fn footer(ui: &mut Ui, cx: &mut Cx<'_>) {
    let app = cx.app;
    // The footer speaks only when something is wrong; routine progress stays silent.
    let since = match &cx.state.status_seen {
        Some((text, since)) if *text == app.status => *since,
        _ => {
            let now = Instant::now();
            cx.state.status_seen = Some((app.status.clone(), now));
            now
        }
    };
    let shown = match status_tone(&app.status) {
        StatusTone::Failure => Some(theme::SIG_CLIP),
        StatusTone::Guidance if since.elapsed() < GUIDANCE_LINGER => Some(theme::SIG_HOT),
        _ => None,
    };
    if shown.is_some() {
        // Repaint once the hint expires, so it leaves without waiting for input.
        ui.ctx().request_repaint_after(GUIDANCE_LINGER);
    }
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if let Some(color) = shown {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 2.8, color);
            // Leave room for the mode badge and the revision readout on the right.
            let width = (ui.available_width() - 320.0).max(120.0);
            ui.allocate_ui(Vec2::new(width, 16.0), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(widgets::legible(&app.status))
                            .size(10.5)
                            .color(color),
                    )
                    .truncate(),
                )
                .on_hover_text(widgets::legible(&app.status));
            });
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(performance) = app.performance.footer() {
                ui.label(
                    RichText::new(performance)
                        .monospace()
                        .size(9.5)
                        .color(theme::TEXT_MUTED),
                );
            } else {
                ui.label(
                    RichText::new(format!(
                        "Rev {} · MIDI {}",
                        short_revision(&app.bundle.revision),
                        if app.telemetry.midi_connected {
                            "ON"
                        } else {
                            "OFF"
                        }
                    ))
                    .monospace()
                    .size(9.5)
                    .color(theme::TEXT_MUTED),
                );
            }
            // Mode badges sit on the fixed right side, so a status message never moves them.
            if app.note_mode {
                ui.label(
                    RichText::new("NOTE MODE")
                        .strong()
                        .size(10.0)
                        .color(theme::SIG_HOT),
                );
            } else if app.timeline_state.visual() && app.view == View::Timeline {
                ui.label(
                    RichText::new("RANGE SELECT")
                        .strong()
                        .size(10.0)
                        .color(theme::SIG_NOMINAL),
                );
            }
        });
    });
}

fn modal(ctx: &egui::Context, cx: &mut Cx<'_>) {
    let Some(modal) = &cx.app.modal else {
        cx.state.modal_input = None;
        cx.state.modal_submit = false;
        return;
    };
    let response = egui::Modal::new(egui::Id::new("crump-modal")).show(ctx, |ui| {
        ui.set_width((ctx.content_rect().width() * 0.65).clamp(360.0, 860.0));
        match modal {
            Modal::Input {
                title,
                prompt,
                value,
                replace_on_type,
                ..
            } => input_modal(ui, cx, title, prompt, value, *replace_on_type),
            Modal::ConfirmRestore { entry_id } => {
                ui.label(
                    RichText::new("CONFIRM HISTORY RESTORE")
                        .strong()
                        .size(13.0)
                        .color(theme::ACCENT_AMBER),
                );
                ui.add_space(6.0);
                ui.label(format!("Restore exact state from {entry_id}?"));
                ui.label(
                    RichText::new(
                        "Current source will be replaced atomically and appended as a new History Entry.",
                    )
                    .size(11.5)
                    .color(theme::TEXT_SECONDARY),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Restore")
                                    .strong()
                                    .color(theme::BG_WELL),
                            )
                            .fill(theme::ACCENT_AMBER),
                        )
                        .on_hover_text("y / Enter")
                        .clicked()
                    {
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
    ui.label(
        RichText::new(widgets::legible(title))
            .strong()
            .size(14.0)
            .color(theme::ACCENT_AMBER),
    );
    ui.label(
        RichText::new(widgets::legible(prompt))
            .size(11.5)
            .color(theme::TEXT_SECONDARY),
    );
    ui.add_space(8.0);
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
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui
            .add(
                egui::Button::new(RichText::new("Apply").strong().color(theme::BG_WELL))
                    .fill(theme::ACCENT_AMBER),
            )
            .on_hover_text("Enter")
            .clicked()
            || submit
        {
            cx.send(Action::ModalSubmit);
        }
        if ui.button("Cancel").on_hover_text("Esc").clicked() {
            cx.send(Action::ModalDismiss);
        }
    });
    cx.state.modal_input = Some((identity, buffer));
}

fn text_modal(ui: &mut Ui, cx: &mut Cx<'_>, title: &str, body: &str, scroll: u16) {
    ui.label(
        RichText::new(widgets::legible(title))
            .strong()
            .size(14.0)
            .color(theme::ACCENT_CYAN),
    );
    ui.add_space(6.0);
    let lines = body.lines().collect::<Vec<_>>();
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    let mut area = egui::ScrollArea::both()
        .auto_shrink([false, true])
        .max_height(ui.ctx().content_rect().height() * 0.6);
    if std::mem::replace(&mut cx.state.modal_scroll, scroll) != scroll {
        area = area.vertical_scroll_offset(f32::from(scroll) * row_height);
    }
    egui::Frame::new()
        .fill(theme::BG_WELL)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            area.show_rows(ui, row_height, lines.len(), |ui, rows| {
                for line in &lines[rows] {
                    let color = match line.as_bytes().first() {
                        Some(b'+') => theme::SIG_NOMINAL,
                        Some(b'-') => theme::SIG_CLIP,
                        Some(b'@') => theme::ACCENT_CYAN,
                        _ => theme::TEXT_PRIMARY,
                    };
                    ui.label(
                        RichText::new(widgets::legible(line))
                            .monospace()
                            .color(color),
                    );
                }
            });
        });
    ui.add_space(8.0);
    if ui.button("Close").on_hover_text("Esc").clicked() {
        cx.send(Action::ModalDismiss);
    }
}
