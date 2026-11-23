use std::fs;

use eframe::egui::{Event, Key, Modifiers, PointerButton, RawInput};

use super::*;
use crate::workspace::keymap::{KeyCode, KeyPhase};

const SCREEN: Vec2 = Vec2::new(1280.0, 800.0);

fn workspace(fixture: &str) -> (tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(root.path().join("project.crump"), fixture).expect("write fixture");
    let workspace = Workspace::load(root.path()).expect("load workspace");
    (root, workspace)
}

/// A headless window: one egui context fed synthetic input, with the client's own frame steps.
struct Window {
    ctx: egui::Context,
    state: UiState,
    keymap: Keymap,
    time: f64,
}

impl Window {
    fn new() -> Self {
        let ctx = egui::Context::default();
        style(&ctx);
        Self {
            ctx,
            state: UiState::default(),
            keymap: Keymap::default(),
            time: 0.0,
        }
    }

    /// Runs one frame as the client does: keys first, then drawing, then the frame's actions.
    /// Returns those actions and the number of shapes the frame painted.
    fn frame(&mut self, app: &mut Workspace, events: Vec<Event>) -> (Vec<Action>, usize) {
        self.time += 0.1;
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        input::take_keys(
            &mut input,
            app,
            &mut self.keymap,
            &mut self.state.modal_submit,
        );
        let mut actions = Vec::new();
        let output = self.ctx.run(input, |ctx| {
            actions = draw(ctx, app, &mut self.state);
        });
        for action in &actions {
            app.dispatch(action.clone());
        }
        (actions, output.shapes.len())
    }

    fn press(&mut self, app: &mut Workspace, position: Pos2) -> Vec<Action> {
        let mut actions = self.frame(app, vec![Event::PointerMoved(position)]).0;
        actions.extend(self.frame(app, vec![button(position, true)]).0);
        actions
    }

    fn click(&mut self, app: &mut Workspace, position: Pos2) -> Vec<Action> {
        let mut actions = self.press(app, position);
        actions.extend(self.frame(app, vec![button(position, false)]).0);
        // Keep later clicks from pairing with this one into a double click.
        self.time += 2.0;
        actions
    }
}

fn button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn placed(actions: &[Action]) -> bool {
    actions
        .iter()
        .any(|action| matches!(action, Action::Timeline(TimelineAction::PlaceCursor { .. })))
}

/// A point inside the first Track lane at a horizontal fraction of the lanes, found by probing
/// down the window until a click reaches the lanes.
fn first_lane(window: &mut Window, app: &mut Workspace) -> f32 {
    (80..400)
        .step_by(6)
        .map(|y| y as f32)
        .find(|y| placed(&window.click(app, Pos2::new(SCREEN.x - 40.0, *y))))
        .expect("the Timeline lanes accept clicks")
        + super::arrangement::RULER_HEIGHT
        + 8.0
}

#[test]
fn every_view_and_modal_draws_without_a_display() {
    let (_root, mut app) = workspace(include_str!("../../../tests/fixtures/production_mix.crump"));
    assert!(app.bundle.project.is_some());
    let mut window = Window::new();
    for view in VIEWS {
        app.dispatch(Action::ShowView(view));
        window.frame(&mut app, Vec::new());
        let (_, shapes) = window.frame(&mut app, Vec::new());
        assert!(shapes > 20, "{view:?} painted only {shapes} shapes");
        assert_eq!(app.view, view, "drawing {view:?} must not change the view");
    }

    app.dispatch(Action::ShowView(View::Timeline));
    app.dispatch(Action::Timeline(TimelineAction::OpenGoto));
    window.frame(&mut app, Vec::new());
    assert!(matches!(app.modal, Some(Modal::Input { .. })));
    app.dispatch(Action::ModalDismiss);
    app.modal = Some(Modal::Text {
        title: "Diff".to_owned(),
        body: "@@ -1 +1 @@\n-old\n+new\n".to_owned(),
        scroll: 0,
    });
    window.frame(&mut app, Vec::new());
    app.modal = Some(Modal::ConfirmRestore {
        entry_id: "000000000001".to_owned(),
    });
    window.frame(&mut app, Vec::new());
    app.shutdown();
}

#[test]
fn an_unparseable_project_still_draws_every_view() {
    let (_root, mut app) = workspace("crump 1\nproject \"Broken\" {\n");
    assert!(app.bundle.diagnostics.has_errors());
    let mut window = Window::new();
    for view in VIEWS {
        app.dispatch(Action::ShowView(view));
        window.frame(&mut app, Vec::new());
    }
    app.shutdown();
}

#[test]
fn the_default_fonts_cover_every_symbol_the_client_draws() {
    let window = Window::new();
    let _ = window.ctx.run(RawInput::default(), |_| {});
    let symbols = widgets::legible("·×–—…←↑→↓−⏭⏮⏸⏹│▶◀●✔■");
    let missing = window.ctx.fonts_mut(|fonts| {
        symbols
            .chars()
            .filter(|symbol| {
                !fonts.has_glyph(&FontId::proportional(14.0), *symbol)
                    || !fonts.has_glyph(&FontId::monospace(14.0), *symbol)
            })
            .collect::<String>()
    });
    assert_eq!(missing, "", "symbols without a glyph");
}

#[test]
fn timeline_clicks_place_the_cursor_and_drags_select_a_range() {
    let (_root, mut app) = workspace(include_str!("../../../tests/fixtures/minimal.crump"));
    let song_end = app.bundle.project.as_ref().expect("Project").song.end;
    let mut window = Window::new();
    window.frame(&mut app, Vec::new());
    let y = first_lane(&mut window, &mut app);
    assert_eq!(
        app.timeline_position().at,
        song_end,
        "a click at the right edge snaps to the Song end"
    );

    // The lanes begin after the Track headers; a click just inside them is the Song start.
    let left = (0..600)
        .step_by(10)
        .map(|x| x as f32)
        .find(|x| placed(&window.click(&mut app, Pos2::new(*x, y))))
        .expect("the lanes have a left edge");
    assert_eq!(app.timeline_position().at, crate::time::MusicalTime::ZERO);
    assert_eq!(app.selected_track, 0);

    let right = SCREEN.x - 40.0;
    let middle = (left + right) / 2.0;
    window.press(&mut app, Pos2::new(left + 4.0, y));
    for x in [left + 40.0, middle - 20.0, middle] {
        window.frame(&mut app, vec![Event::PointerMoved(Pos2::new(x, y))]);
    }
    window.frame(&mut app, vec![button(Pos2::new(middle, y), false)]);
    let range = app.timeline_state.visual_range().expect("dragged range");
    assert_eq!(range.start, crate::time::MusicalTime::ZERO);
    assert_eq!(range.end, crate::time::MusicalTime::whole(2));
    app.shutdown();
}

#[test]
fn table_rows_select_on_click_and_view_tabs_switch_views() {
    let (_root, mut app) = workspace(include_str!("../../../tests/fixtures/production_mix.crump"));
    let mut window = Window::new();
    app.dispatch(Action::ShowView(View::Mixer));
    window.frame(&mut app, Vec::new());
    assert!(app.mixer_parameters.len() > 3);
    assert_eq!(app.selected_mixer_parameter, 0);
    let moved = (160..700).step_by(7).map(|y| y as f32).any(|y| {
        window.click(&mut app, Pos2::new(120.0, y));
        app.selected_mixer_parameter != 0
    });
    assert!(moved, "clicking a later Mixer row selects it");
    assert_eq!(app.view, View::Mixer);
    app.shutdown();
}

#[test]
fn keys_reach_the_shared_keymap_and_never_egui() {
    let (_root, mut app) = workspace(include_str!("../../../tests/fixtures/minimal.crump"));
    let mut window = Window::new();
    let key = |key, modifiers| Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };

    window.frame(&mut app, vec![Event::Text("2".to_owned())]);
    assert_eq!(app.view, View::Mixer);
    window.frame(&mut app, vec![key(Key::Tab, Modifiers::SHIFT)]);
    assert_eq!(app.view, View::Timeline);
    // A printable key arrives as both a key and a text event and must act once.
    window.frame(
        &mut app,
        vec![key(Key::Plus, Modifiers::NONE), Event::Text("+".to_owned())],
    );
    assert_eq!(app.timeline_viewport.zoom_factor(), 2);
    window.frame(&mut app, vec![key(Key::Home, Modifiers::NONE)]);
    assert!(app.timeline_viewport.is_fit());

    // The exact-entry prompt opens with its value selected, so typing replaces it.
    window.frame(&mut app, vec![Event::Text(":".to_owned())]);
    assert!(matches!(app.modal, Some(Modal::Input { .. })));
    window.frame(&mut app, Vec::new());
    window.frame(&mut app, vec![Event::Text("1:3".to_owned())]);
    let Some(Modal::Input { value, .. }) = &app.modal else {
        panic!("typing keeps the prompt open");
    };
    assert_eq!(value, "1:3");
    assert_eq!(
        app.view,
        View::Timeline,
        "typed digits must not switch views"
    );
    window.frame(&mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert!(app.modal.is_none());
    assert_eq!(
        app.timeline_position().at,
        crate::time::MusicalTime::whole(2)
    );

    window.frame(&mut app, vec![Event::Copy]);
    assert!(app.quit_requested());
    app.shutdown();
}

#[test]
fn window_keys_translate_to_client_neutral_keys() {
    let control = Modifiers::CTRL;
    let code = |key, pressed, modifiers| input::key_input(key, pressed, modifiers);
    assert_eq!(
        code(Key::Enter, true, Modifiers::NONE).map(|key| key.code),
        Some(KeyCode::Enter)
    );
    assert_eq!(
        code(Key::Tab, true, Modifiers::SHIFT).map(|key| key.code),
        Some(KeyCode::BackTab)
    );
    let chord = code(Key::O, true, control).expect("Ctrl-O");
    assert_eq!(chord.code, KeyCode::Char('o'));
    assert!(chord.modifiers.control);
    assert_eq!(
        code(Key::OpenBracket, true, control).map(|key| key.code),
        Some(KeyCode::Char('['))
    );
    assert_eq!(code(Key::A, true, Modifiers::NONE), None);
    assert_eq!(code(Key::Space, true, Modifiers::NONE), None);
    let release = code(Key::A, false, Modifiers::NONE).expect("release");
    assert_eq!(release.code, KeyCode::Char('a'));
    assert_eq!(release.phase, KeyPhase::Release);
    assert_eq!(code(Key::Enter, false, Modifiers::NONE), None);
}
