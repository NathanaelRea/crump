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
        let focused = inline_text_focused(&self.ctx, app);
        input::take_keys(
            &mut input,
            app,
            &mut self.keymap,
            &mut self.state.modal_submit,
            focused,
        );
        settle_view(app, &mut self.state);
        let mut actions = Vec::new();
        let output = self.ctx.run(input, |ctx| {
            actions.extend(draw(ctx, app, &mut self.state));
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
        // A clean Candidate has no Diagnostics list to show; the window steps over it.
        if view == View::Diagnostics {
            continue;
        }
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

#[test]
#[ignore = "writes visual GUI snapshots to CRUMP_GUI_SNAPSHOT_DIR"]
fn render_gui_snapshots() {
    use std::collections::HashMap;
    use std::io::Write;
    use std::path::PathBuf;

    let out_dir = PathBuf::from(
        std::env::var("CRUMP_GUI_SNAPSHOT_DIR")
            .unwrap_or_else(|_| "/tmp/crump_gui_snapshots".to_owned()),
    );
    fs::create_dir_all(&out_dir).expect("create snapshot dir");

    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("deep-house-reference");
    let mut app = Workspace::load(&fixture_root).expect("load deep-house-reference");
    assert!(app.bundle.project.is_some());

    // Populate realistic simulated telemetry and channel meter indexes so all meters look alive.
    if let Some(project) = app.bundle.project.as_ref() {
        let mut idx = 0usize;
        for track in &project.song.tracks {
            app.mix_channel_indexes
                .insert(format!("track/{}", track.id), idx);
            idx += 1;
        }
        for group in &project.song.groups {
            app.mix_channel_indexes
                .insert(format!("group/{}", group.id), idx);
            idx += 1;
        }
        for ret in &project.song.returns {
            app.mix_channel_indexes
                .insert(format!("return/{}", ret.id), idx);
            idx += 1;
        }
        app.mix_channel_indexes.insert("master".to_owned(), idx);
    }
    app.telemetry.callback_utilization = 0.14;
    app.telemetry.engine.current_peak = 0.62;
    app.telemetry.engine.active_voices = 6;
    app.telemetry.engine.channel_count = app.telemetry.engine.channel_activity.len();
    app.telemetry.engine.processor_count = app.telemetry.engine.processor_activity.len();
    for i in 0..app.telemetry.engine.channel_activity.len() {
        let level = ((i as f32 * 0.17 + 0.25) % 0.78).max(0.14);
        app.telemetry.engine.channel_activity[i].current_peak = level;
        app.telemetry.engine.channel_activity[i].average = level * 0.68;
    }
    for i in 0..app.telemetry.engine.processor_activity.len() {
        let level = ((i as f32 * 0.13 + 0.3) % 0.75).max(0.16);
        app.telemetry.engine.processor_activity[i].input_peak = level;
        app.telemetry.engine.processor_activity[i].output_peak = (level * 0.88).min(0.95);
        app.telemetry.engine.processor_activity[i].gain_reduction_db = -3.2 * ((i % 3) as f32);
        app.telemetry.engine.processor_activity[i].maximum_gain_reduction_db =
            -5.8 * ((i % 3) as f32);
    }

    let mut window = Window::new();
    let mut textures: HashMap<egui::TextureId, (usize, usize, Vec<Color32>)> = HashMap::new();

    // CRUMP_GUI_SEED_HISTORY renders History over a realistic three-session History.
    if std::env::var_os("CRUMP_GUI_SEED_HISTORY").is_some() {
        seed_history(&mut app);
        // Open the afternoon's bass run, as a user drilling in would.
        window.state.history_runs.insert("000000000014".to_owned());
    }
    let views_to_render = [
        ("01_timeline", View::Timeline),
        ("02_mixer", View::Mixer),
        ("03_processing", View::Processing),
        ("04_event_grid", View::EventGrid),
        ("05_faceplate", View::Faceplate),
        ("06_instrument", View::Instrument),
        ("07_parameters", View::Parameters),
        ("08_automation", View::Automation),
        ("09_graph", View::Graph),
        ("10_source", View::Source),
        ("12_history", View::History),
        ("13_help", View::Help),
    ];

    // CRUMP_GUI_SNAPSHOT_SCALE=2 renders at 2x pixel density for crisp documentation images.
    let scale: f32 = std::env::var("CRUMP_GUI_SNAPSHOT_SCALE")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &f32| (1.0..=4.0).contains(value))
        .unwrap_or(1.0);
    // CRUMP_GUI_SNAPSHOT_WIDTH renders a narrower window, to check the layout holds up.
    let screen = Vec2::new(
        std::env::var("CRUMP_GUI_SNAPSHOT_WIDTH")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value: &f32| (480.0..=SCREEN.x).contains(value))
            .unwrap_or(SCREEN.x),
        SCREEN.y,
    );
    let width = (screen.x * scale) as usize;
    let height = (screen.y * scale) as usize;

    for (name, view) in views_to_render {
        app.dispatch(Action::ShowView(view));
        let mut last_output = None;
        for _ in 0..3 {
            window.time += 0.1;
            let mut input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen)),
                time: Some(window.time),
                ..Default::default()
            };
            input
                .viewports
                .entry(input.viewport_id)
                .or_default()
                .native_pixels_per_point = Some(scale);
            let output = window.ctx.run(input, |ctx| {
                let _ = draw(ctx, &app, &mut window.state);
            });
            for (id, delta) in &output.textures_delta.set {
                let egui::ImageData::Color(image) = &delta.image;
                let [iw, ih] = image.size;
                if let Some([px, py]) = delta.pos {
                    if let Some((tw, _th, buf)) = textures.get_mut(id) {
                        for y in 0..ih {
                            for x in 0..iw {
                                let dst = (py + y) * *tw + (px + x);
                                if dst < buf.len() {
                                    buf[dst] = image.pixels[y * iw + x];
                                }
                            }
                        }
                    }
                } else {
                    textures.insert(*id, (iw, ih, image.pixels.clone()));
                }
            }
            for id in &output.textures_delta.free {
                textures.remove(id);
            }
            last_output = Some(output);
        }

        let output = last_output.expect("frame output");
        let primitives = window
            .ctx
            .tessellate(output.shapes, output.pixels_per_point);
        let mut fb = vec![[17u8, 19u8, 24u8]; width * height];

        for clipped in &primitives {
            let egui::epaint::Primitive::Mesh(mesh) = &clipped.primitive else {
                continue;
            };
            let tex = textures.get(&mesh.texture_id);
            let mut mesh = mesh.clone();
            for vertex in &mut mesh.vertices {
                vertex.pos = Pos2::new(vertex.pos.x * scale, vertex.pos.y * scale);
            }
            let clip = Rect::from_min_max(
                Pos2::new(
                    clipped.clip_rect.min.x * scale,
                    clipped.clip_rect.min.y * scale,
                ),
                Pos2::new(
                    clipped.clip_rect.max.x * scale,
                    clipped.clip_rect.max.y * scale,
                ),
            );
            let clip_min_x = (clip.min.x.floor() as isize).clamp(0, width as isize) as usize;
            let clip_min_y = (clip.min.y.floor() as isize).clamp(0, height as isize) as usize;
            let clip_max_x = (clip.max.x.ceil() as isize).clamp(0, width as isize) as usize;
            let clip_max_y = (clip.max.y.ceil() as isize).clamp(0, height as isize) as usize;

            for tri in mesh.indices.as_chunks::<3>().0 {
                let v0 = mesh.vertices[tri[0] as usize];
                let v1 = mesh.vertices[tri[1] as usize];
                let v2 = mesh.vertices[tri[2] as usize];

                let min_x = v0
                    .pos
                    .x
                    .min(v1.pos.x)
                    .min(v2.pos.x)
                    .floor()
                    .max(clip_min_x as f32) as usize;
                let max_x = v0
                    .pos
                    .x
                    .max(v1.pos.x)
                    .max(v2.pos.x)
                    .ceil()
                    .min(clip_max_x as f32) as usize;
                let min_y = v0
                    .pos
                    .y
                    .min(v1.pos.y)
                    .min(v2.pos.y)
                    .floor()
                    .max(clip_min_y as f32) as usize;
                let max_y = v0
                    .pos
                    .y
                    .max(v1.pos.y)
                    .max(v2.pos.y)
                    .ceil()
                    .min(clip_max_y as f32) as usize;
                if min_x >= max_x || min_y >= max_y {
                    continue;
                }

                let denom = (v1.pos.y - v2.pos.y) * (v0.pos.x - v2.pos.x)
                    + (v2.pos.x - v1.pos.x) * (v0.pos.y - v2.pos.y);
                if denom.abs() < 1e-6 {
                    continue;
                }
                let inv_denom = 1.0 / denom;

                for py in min_y..max_y {
                    let fy = py as f32 + 0.5;
                    for px in min_x..max_x {
                        let fx = px as f32 + 0.5;
                        let w0 = ((v1.pos.y - v2.pos.y) * (fx - v2.pos.x)
                            + (v2.pos.x - v1.pos.x) * (fy - v2.pos.y))
                            * inv_denom;
                        let w1 = ((v2.pos.y - v0.pos.y) * (fx - v2.pos.x)
                            + (v0.pos.x - v2.pos.x) * (fy - v2.pos.y))
                            * inv_denom;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                            continue;
                        }
                        let r = (w0 * f32::from(v0.color.r())
                            + w1 * f32::from(v1.color.r())
                            + w2 * f32::from(v2.color.r()))
                        .clamp(0.0, 255.0);
                        let g = (w0 * f32::from(v0.color.g())
                            + w1 * f32::from(v1.color.g())
                            + w2 * f32::from(v2.color.g()))
                        .clamp(0.0, 255.0);
                        let b = (w0 * f32::from(v0.color.b())
                            + w1 * f32::from(v1.color.b())
                            + w2 * f32::from(v2.color.b()))
                        .clamp(0.0, 255.0);
                        let a = (w0 * f32::from(v0.color.a())
                            + w1 * f32::from(v1.color.a())
                            + w2 * f32::from(v2.color.a()))
                        .clamp(0.0, 255.0);

                        let (sr, sg, sb, sa) = if let Some((tw, th, buf)) = tex {
                            let u = (w0 * v0.uv.x + w1 * v1.uv.x + w2 * v2.uv.x).clamp(0.0, 1.0);
                            let v = (w0 * v0.uv.y + w1 * v1.uv.y + w2 * v2.uv.y).clamp(0.0, 1.0);
                            let tx = ((u * (*tw as f32 - 1.0)).round() as usize)
                                .min(tw.saturating_sub(1));
                            let ty = ((v * (*th as f32 - 1.0)).round() as usize)
                                .min(th.saturating_sub(1));
                            let tc = buf[ty * *tw + tx];
                            (
                                r * f32::from(tc.r()) / 255.0,
                                g * f32::from(tc.g()) / 255.0,
                                b * f32::from(tc.b()) / 255.0,
                                a * f32::from(tc.a()) / 255.0,
                            )
                        } else {
                            (r, g, b, a)
                        };

                        let alpha = sa / 255.0;
                        let dst = &mut fb[py * width + px];
                        let inv_a = 1.0 - alpha;
                        dst[0] = (sr + f32::from(dst[0]) * inv_a).clamp(0.0, 255.0) as u8;
                        dst[1] = (sg + f32::from(dst[1]) * inv_a).clamp(0.0, 255.0) as u8;
                        dst[2] = (sb + f32::from(dst[2]) * inv_a).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }

        let path = out_dir.join(format!("{name}.ppm"));
        let mut file = fs::File::create(&path).expect("create ppm");
        writeln!(file, "P6\n{width} {height}\n255").expect("write header");
        let mut bytes = Vec::with_capacity(width * height * 3);
        for px in &fb {
            bytes.extend_from_slice(px);
        }
        file.write_all(&bytes).expect("write pixels");
    }

    app.shutdown();
}

/// Where the top bar's `label` tab sits, found by probing along the bar.
fn top_bar_tab(window: &mut Window, app: &mut Workspace, view: View) -> Pos2 {
    app.dispatch(Action::ShowView(View::Help));
    (0..SCREEN.x as i32)
        .step_by(4)
        .map(|x| Pos2::new(x as f32, 22.0))
        .find(|position| {
            window.click(app, *position);
            let hit = app.view == view;
            app.dispatch(Action::ShowView(View::Help));
            hit
        })
        .expect("the top bar has the tab")
}

#[test]
fn tabs_switch_even_when_the_pointer_travels_during_the_click() {
    let (_root, mut app) = workspace(include_str!("../../../tests/fixtures/production_mix.crump"));
    let mut window = Window::new();
    window.frame(&mut app, Vec::new());
    let mix = top_bar_tab(&mut window, &mut app, View::Mixer);
    let history = top_bar_tab(&mut window, &mut app, View::History);
    for (target, view) in [(mix, View::Mixer), (history, View::History)] {
        app.dispatch(Action::ShowView(View::Timeline));
        // A quick flick: the button goes down, and the pointer is past click distance by the
        // time it comes up, which egui would otherwise treat as an abandoned drag.
        window.frame(&mut app, vec![Event::PointerMoved(target)]);
        window.frame(&mut app, vec![button(target, true)]);
        let moved = target + Vec2::new(9.0, 0.0);
        window.frame(
            &mut app,
            vec![Event::PointerMoved(moved), button(moved, false)],
        );
        assert_eq!(app.view, view);
        window.time += 2.0;
    }
    app.shutdown();
}

#[test]
fn returning_to_a_view_draws_its_settled_layout_on_the_first_frame() {
    let (_root, mut app) = workspace(include_str!("../../../tests/fixtures/production_mix.crump"));
    let mut window = Window::new();
    let views = VIEWS
        .into_iter()
        .filter(|view| *view != View::Diagnostics)
        .collect::<Vec<_>>();
    let mut frame = |app: &mut Workspace| {
        window.time += 0.1;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(window.time),
            ..Default::default()
        };
        let output = window.ctx.run(input, |ctx| {
            let _ = draw(ctx, app, &mut window.state);
        });
        output
            .shapes
            .iter()
            .map(|clipped| {
                // Whole points, which also makes empty and degenerate shapes comparable.
                let rect = clipped.shape.visual_bounding_rect();
                [rect.min.x, rect.min.y, rect.max.x, rect.max.y].map(|edge| edge as i32)
            })
            .collect::<Vec<_>>()
    };
    for view in &views {
        app.dispatch(Action::ShowView(*view));
        for _ in 0..4 {
            frame(&mut app);
        }
    }
    for pair in views.windows(2) {
        app.dispatch(Action::ShowView(pair[0]));
        for _ in 0..4 {
            frame(&mut app);
        }
        app.dispatch(Action::ShowView(pair[1]));
        let first = frame(&mut app);
        for _ in 0..3 {
            frame(&mut app);
        }
        assert_eq!(
            first,
            frame(&mut app),
            "{:?} -> {:?} shifted",
            pair[0],
            pair[1]
        );
    }
    app.shutdown();
}

#[test]
fn tab_steps_over_an_empty_diagnostics_list() {
    let (_root, mut app) = workspace(include_str!("../../../tests/fixtures/minimal.crump"));
    assert!(app.bundle.diagnostics.as_slice().is_empty());
    let mut window = Window::new();
    app.dispatch(Action::ShowView(View::Source));
    window.frame(&mut app, Vec::new());
    app.dispatch(Action::CycleView(1));
    window.frame(&mut app, Vec::new());
    assert_eq!(app.view, View::History);
    app.dispatch(Action::CycleView(-1));
    window.frame(&mut app, Vec::new());
    assert_eq!(app.view, View::Source);
    app.shutdown();
}

#[test]
fn the_footer_names_only_problems() {
    for quiet in [
        "Candidate reconstructed · verifying latest filesystem generation",
        "Saved and published history entry 000000000004",
        "Published revision loaded",
        "Edit queued · 2 pending",
        "Valid external Candidate loaded · press P to publish",
        "Opened Instrument `error-bass`",
        "Restored 000000000003 as new entry",
    ] {
        assert_eq!(status_tone(quiet), StatusTone::Quiet, "{quiet}");
    }
    for guidance in [
        "Select a processor before editing settings",
        "Selected Instrument is unavailable",
        "No further Locator",
        "Candidate is already the Active Revision",
        "Create a Locator as `ID NAME`",
    ] {
        assert_eq!(status_tone(guidance), StatusTone::Guidance, "{guidance}");
    }
    for failure in [
        "Edit transaction failed: expected revision mismatch",
        "Could not start Project mutation: busy",
        "Invalid Candidate cannot publish; inspect Diagnostics",
    ] {
        assert_eq!(status_tone(failure), StatusTone::Failure, "{failure}");
    }
}

#[test]
fn an_edit_never_shows_its_candidate_as_unpublished_on_the_way_through() {
    let (_root, mut app) = workspace(include_str!("../../../tests/fixtures/minimal.crump"));
    crate::workspace::tests::settle_background(&mut app);
    for bpm in [124.0, 126.0] {
        app.dispatch(Action::SetSongTempo { bpm });
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.background_work_pending() {
            app.poll_background_work();
            // What the Publish button and revision dot read: amber only once settled.
            assert!(
                app.candidate_is_active() || app.publication_settling(),
                "the edit at {bpm} BPM surfaced an unpublished Candidate mid-reload"
            );
            assert!(Instant::now() < deadline, "edit timed out");
            std::thread::yield_now();
        }
        assert!(app.candidate_is_active(), "the edit published itself");
    }
    app.shutdown();
}

#[test]
fn positions_count_beats_in_the_meter_denominator() {
    let common = crate::time::Meter::new(4, 4).expect("4/4");
    assert_eq!(theme::format_bar_beat(0.0, common), "1.1.1");
    assert_eq!(theme::format_bar_beat(5.75, common), "2.2.4");
    let compound = crate::time::Meter::new(6, 8).expect("6/8");
    // Six eighth-note beats of two sixteenths each, three quarter notes to the bar.
    assert_eq!(theme::format_bar_beat(2.5, compound), "1.6.1");
    assert_eq!(theme::format_bar_beat(3.25, compound), "2.1.2");
}

#[test]
fn controls_of_one_value_share_its_preview() {
    // Dragging any control of a value moves every other control of it at once, and a
    // committed value shows everywhere until its edit reloads.
    let mut state = UiState::default();
    let knob = widgets::preview_key::instrument_parameter("bass", "cutoff");
    let bar = widgets::preview_key::instrument_parameter("bass", "cutoff");
    state.set_drag_preview(&knob, 0.42);
    assert_eq!(state.drag_preview(&bar), Some(0.42));
    state.take_drag_preview(&knob);
    assert_eq!(state.drag_preview(&bar), Some(0.42));
    assert_eq!(
        widgets::preview_key::channel_level("track/kick"),
        widgets::preview_key::channel_level(&format!("track/{}", "kick")),
    );

    let mute = widgets::preview_key::channel_mute("track/kick");
    assert!(!state.flag(&mute, false));
    state.hold_flag(&mute, true);
    assert!(
        state.flag(&mute, false),
        "a clicked toggle shows its new state at once"
    );
}

#[test]
fn only_pan_and_symmetric_amounts_fill_from_the_centre() {
    use crate::model::Unit;
    assert!(widgets::fills_from_centre(Unit::Pan, -1.0, 1.0));
    assert!(widgets::fills_from_centre(Unit::Semitones, -12.0, 12.0));
    assert!(!widgets::fills_from_centre(Unit::Decibels, -60.0, 6.0));
    assert!(!widgets::fills_from_centre(Unit::Decibels, -24.0, 24.0));
    assert!(!widgets::fills_from_centre(Unit::Ratio, 0.0, 1.0));
}

#[test]
fn history_lists_the_newest_entry_first() {
    let (_root, mut app) = workspace(include_str!("../../../tests/fixtures/minimal.crump"));
    crate::workspace::tests::settle_background(&mut app);
    for bpm in [124.0, 126.0] {
        app.dispatch(Action::SetSongTempo { bpm });
        crate::workspace::tests::settle_background(&mut app);
    }
    assert!(app.history.len() >= 2);
    assert_eq!(
        app.active_entry.as_ref(),
        Some(&app.history[0].id),
        "the latest published entry leads the list"
    );
    app.shutdown();
}

/// A realistic three-session History, newest first, for rendering the History view.
fn seed_history(app: &mut Workspace) {
    use crate::history::{ChangeOrigin, HistoryEntry, OriginKind, SourceChange, SourceChangeKind};
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let stamp = |secs: i64| {
        let t = time::OffsetDateTime::from_unix_timestamp(secs).unwrap();
        t.format(&time::format_description::well_known::Rfc3339)
            .unwrap()
    };
    // (minutes ago, origin, summary, explanation, file)
    let agent = "Claude";
    let script: Vec<(i64, &str, &str, &str, &str)> = vec![
        // Yesterday evening: arranging the hook.
        (1150, "", "Set tempo 122.0 BPM", "", "project.crump"),
        (
            1146,
            "",
            "Add Clip lead-hook-3 on lead",
            "",
            "project.crump",
        ),
        (
            1144,
            "",
            "Move Clip lead-hook-3 to 49bar",
            "",
            "project.crump",
        ),
        (1141, "", "Set lead level", "", "project.crump"),
        (1140, "", "Set lead pan", "", "project.crump"),
        (
            1132,
            "",
            "Set dh-lead.position",
            "",
            "instruments/dh-lead.crump",
        ),
        (
            1131,
            "",
            "Set dh-lead.position",
            "",
            "instruments/dh-lead.crump",
        ),
        (
            1129,
            "",
            "Set dh-lead.level",
            "",
            "instruments/dh-lead.crump",
        ),
        (1110, "", "Create Locator drop-2", "", "project.crump"),
        // This morning: an agent shapes the bass filter sweep.
        (
            290,
            agent,
            "Create Automation lane bass-cutoff",
            "Open the bass filter into the drop so the low end blooms on bar 65.",
            "project.crump",
        ),
        (
            289,
            agent,
            "Add point p1 in bass-cutoff",
            "",
            "project.crump",
        ),
        (
            289,
            agent,
            "Add point p2 in bass-cutoff",
            "",
            "project.crump",
        ),
        (
            288,
            agent,
            "Set bass-cutoff.p2 curve = power(2)",
            "",
            "project.crump",
        ),
        // This afternoon: sound design and drums.
        (
            48,
            "",
            "Set dh-bass.cutoff",
            "",
            "instruments/dh-bass.crump",
        ),
        (
            47,
            "",
            "Set dh-bass.cutoff",
            "",
            "instruments/dh-bass.crump",
        ),
        (46, "", "Set dh-bass.drive", "", "instruments/dh-bass.crump"),
        (
            45,
            "",
            "Set dh-bass.cutoff",
            "",
            "instruments/dh-bass.crump",
        ),
        (40, "", "Add hit snare in kick-four", "", "project.crump"),
        (39, "", "Add hit snare in kick-four", "", "project.crump"),
        (38, "", "Set kick-four[3] velocity", "", "project.crump"),
        (
            30,
            "",
            "Set Processor music-color durable bypass to true",
            "",
            "project.crump",
        ),
        (29, "", "Set music-color.drive = 1.2", "", "project.crump"),
        (
            27,
            "",
            "Set Processor music-color durable bypass to false",
            "",
            "project.crump",
        ),
        (
            20,
            "",
            "Restore history entry 000000000016",
            "",
            "instruments/dh-bass.crump",
        ),
        (12, "", "Set bass level", "", "project.crump"),
        (6, "", "Set dh-bass.cutoff", "", "instruments/dh-bass.crump"),
    ];
    let mut entries: Vec<HistoryEntry> = script
        .iter()
        .enumerate()
        .map(|(i, (ago, who, summary, why, file))| HistoryEntry {
            schema_version: 1,
            id: format!("{:012}", i + 1),
            sequence: i as u64 + 1,
            revision: crate::project::RevisionId(format!("{:064x}", i + 1)),
            created_at: stamp(now - ago * 60),
            origin: ChangeOrigin {
                kind: if who.is_empty() {
                    OriginKind::Human
                } else {
                    OriginKind::Agent
                },
                name: if who.is_empty() { "workspace" } else { who }.to_owned(),
            },
            summary: (*summary).to_owned(),
            explanation: (*why).to_owned(),
            parent: (i > 0).then(|| format!("{i:012}")),
            restored_from: summary
                .starts_with("Restore history entry")
                .then(|| "000000000016".to_owned()),
            changes: vec![SourceChange {
                path: std::path::PathBuf::from(file),
                kind: SourceChangeKind::Modified,
                before_sha256: None,
                after_sha256: None,
                changed_lines: 1,
            }],
        })
        .collect();
    entries.reverse();
    app.active_entry = Some(entries[0].id.clone());
    app.history = entries;
    // Select a change inside the bass run of the afternoon session.
    app.selected_history = 10;
}

#[test]
fn history_groups_sessions_and_folds_back_to_back_edits() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/deep-house-reference");
    let mut app = Workspace::load(&root).expect("load");
    seed_history(&mut app);
    let (sessions, runs) = super::history_view::tests::shape(&app);
    assert_eq!(
        sessions, 3,
        "an evening, an agent's morning and this afternoon"
    );
    let runs = runs
        .iter()
        .map(|(subject, edits)| format!("{subject} ×{edits}"))
        .collect::<Vec<_>>();
    assert_eq!(
        runs,
        [
            "Deep House Bass ×1",
            "Sub and Analog Bass ×1",
            "Restore ×1",
            "Saturator on Music ×3",
            "kick-four ×3",
            "Deep House Bass ×4",
        ]
    );
    app.shutdown();
}

#[test]
fn compare_with_now_diffs_the_current_version_against_the_selected_one() {
    let (_root, mut app) = workspace(include_str!("../../../tests/fixtures/minimal.crump"));
    crate::workspace::tests::settle_background(&mut app);
    for bpm in [124.0, 131.0] {
        app.dispatch(Action::SetSongTempo { bpm });
        crate::workspace::tests::settle_background(&mut app);
    }
    // Newest first: the older 124 BPM version is second.
    app.selected_history = 1;
    app.dispatch(Action::CompareHistoryWithActive);
    let Some(Modal::Text { body, .. }) = &app.modal else {
        panic!("compare opens a diff");
    };
    assert!(body.contains("-    tempo 131bpm"), "{body}");
    assert!(body.contains("+    tempo 124bpm"), "{body}");
    app.shutdown();
}
