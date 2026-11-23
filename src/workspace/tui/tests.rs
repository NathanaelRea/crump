use std::fs;

use super::*;
use crate::workspace::tests::settle_background;

#[test]
fn timeline_zoom_pan_and_fit_are_transient_keyboard_state() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let mut keys = Keymap::default();
    let revision = workspace.bundle.revision.clone();
    let source = fs::read_to_string(root.path().join("project.crump")).expect("source");
    let song_end = workspace.bundle.project.as_ref().expect("Project").song.end;

    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE),
    );
    assert_eq!(workspace.timeline_viewport.zoom_factor(), 2);
    let centered = workspace.timeline_viewport.window(song_end);
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE),
    );
    let panned = workspace.timeline_viewport.window(song_end);
    assert!(panned.start < centered.start);
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
    );
    assert!(workspace.timeline_viewport.is_fit());
    assert_eq!(workspace.bundle.revision, revision);
    assert_eq!(
        fs::read_to_string(root.path().join("project.crump")).expect("unchanged source"),
        source
    );
}

#[test]
fn timeline_keys_seek_select_ranges_and_only_ctrl_c_exits() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let mut keys = Keymap::default();
    workspace.commit_input("Initial publication".to_owned(), InputAction::Publish);
    settle_background(&mut workspace);
    workspace.view = View::Timeline;

    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    );
    assert_eq!(workspace.selected_track, 1);
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
    );
    assert_eq!(workspace.selected_track, 0);

    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
    );
    assert_eq!(
        workspace.timeline_position().at,
        crate::time::MusicalTime::whole(1)
    );
    assert!(matches!(
        workspace.work_in_flight,
        Some(WorkspaceWorkKind::Active {
            explicit_seek: true,
            initial_frame: Some(24_000),
            ..
        })
    ));
    settle_background(&mut workspace);

    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT),
    );
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
    );
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    assert!(matches!(
        workspace.audition_plan.range,
        AuditionRange {
            from: Some(ref start),
            to: Some(ref end),
            looping: false,
        } if start == "1qn" && end == "2qn"
    ));
    assert!(!workspace.timeline_state.visual());
    settle_background(&mut workspace);

    let one_shot = workspace.audition_plan.clone();
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT),
    );
    let mut expected_loop = one_shot;
    expected_loop.range.looping = true;
    assert_eq!(workspace.audition_plan, expected_loop);
    settle_background(&mut workspace);

    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
    );
    assert_eq!(
        workspace.timeline_position().at,
        crate::time::MusicalTime::whole(2)
    );
    assert!(workspace.status.contains("outside the armed range"));
    assert_eq!(
        workspace.timeline_frame_at(crate::time::MusicalTime::whole(2)),
        Some(48_000)
    );
    assert!(workspace.timeline_cursor_frame_for_plan().is_err());
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    );
    assert!(!workspace.playback_requested);
    assert!(workspace.status.contains("outside the armed range"));

    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT),
    );
    assert!(workspace.timeline_state.visual());
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    );
    assert!(!workspace.timeline_state.visual());
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
    );
    assert!(!workspace.quit);
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );
    assert!(workspace.quit);
}

#[test]
fn global_navigation_note_mode_and_timeline_context_jumps_are_modal() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let mut keys = Keymap::default();
    workspace.set_help_line_count(rendering::help_line_count());

    workspace.view = View::Mixer;
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    );
    assert_eq!(workspace.selected_mixer_parameter, 1);
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
    );
    assert_eq!(workspace.selected_mixer_parameter, 0);

    workspace.view = View::Help;
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
    );
    assert_eq!(
        usize::from(workspace.help_scroll),
        rendering::help_line_count().saturating_sub(1)
    );
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
    );
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
    );
    assert_eq!(workspace.help_scroll, 0);

    workspace.view = View::Instrument;
    let selected = (
        workspace.selected_instrument_section,
        workspace.selected_instrument_row,
    );
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
    );
    assert!(workspace.note_mode);
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    );
    assert_eq!(
        (
            workspace.selected_instrument_section,
            workspace.selected_instrument_row,
        ),
        selected
    );
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('['), KeyModifiers::CONTROL),
    );
    assert!(!workspace.note_mode);
    assert!(!workspace.quit);
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    );
    assert_ne!(
        (
            workspace.selected_instrument_section,
            workspace.selected_instrument_row,
        ),
        selected
    );

    workspace.view = View::Timeline;
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
    );
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    );
    assert_eq!(workspace.view, View::EventGrid);
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
    );
    assert_eq!(workspace.view, View::Timeline);
}

#[test]
fn timeline_goto_search_marks_and_durable_locators_share_exact_positions() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let mut keys = Keymap::default();
    workspace.view = View::Timeline;

    workspace.commit_input("1:2".to_owned(), InputAction::TimelineGoto);
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
    );
    assert_eq!(
        workspace.timeline_position().at,
        crate::time::MusicalTime::ZERO
    );
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL),
    );
    assert_eq!(
        workspace.timeline_position().at,
        crate::time::MusicalTime::whole(1)
    );

    workspace.commit_input("1:3".to_owned(), InputAction::TimelineGoto);
    assert_eq!(
        workspace.timeline_position().at,
        crate::time::MusicalTime::whole(2)
    );
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE),
    );
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    );
    workspace.commit_input("drums".to_owned(), InputAction::TimelineSearch);
    assert_eq!(workspace.timeline_position().track, 1);
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('\''), KeyModifiers::NONE),
    );
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    );
    assert_eq!(
        workspace.timeline_position(),
        TimelinePosition {
            at: crate::time::MusicalTime::whole(2),
            track: 0,
        }
    );

    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Char('M'), KeyModifiers::SHIFT),
    );
    assert!(matches!(
        workspace.modal.as_ref(),
        Some(Modal::Input {
            action: InputAction::TimelineLocator { at },
            ..
        }) if at == "2qn"
    ));
    workspace.modal = None;
    workspace.commit_input(
        "middle Middle".to_owned(),
        InputAction::TimelineLocator {
            at: "2qn".to_owned(),
        },
    );
    settle_background(&mut workspace);
    let source = fs::read_to_string(root.path().join("project.crump")).expect("edited source");
    assert!(
        source.contains("locator middle \"Middle\" at 2qn"),
        "{source}"
    );
}

#[test]
fn timeline_renders_arrangement_activity_at_full_and_narrow_sizes() {
    fn render_text(workspace: &Workspace, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, workspace))
            .expect("render Timeline");
        let buffer = terminal.backend().buffer();
        (0..height)
            .flat_map(|y| {
                (0..width)
                    .map(move |x| buffer[(x, y)].symbol().to_owned())
                    .chain(std::iter::once("\n".to_owned()))
            })
            .collect()
    }

    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    workspace.timeline_ascii = false;

    let full = render_text(&workspace, 120, 28);
    assert!(full.contains("Track · FIT"), "{full}");
    assert!(full.contains("Selected Track"), "{full}");
    assert!(full.contains("Lead"), "{full}");
    assert!(full.contains('▌'), "{full}");
    assert!(full.contains('▐'), "{full}");
    assert!(
        ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█']
            .into_iter()
            .any(|glyph| full.contains(glyph)),
        "{full}"
    );

    let narrow = render_text(&workspace, 40, 12);
    assert!(narrow.contains("Timeline"), "{narrow}");
    assert!(narrow.contains("Lead"), "{narrow}");
    assert!(
        ['▌', '▐', '━', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█']
            .into_iter()
            .any(|glyph| narrow.contains(glyph)),
        "{narrow}"
    );

    // Extremely constrained layouts must degrade without panicking.
    let _ = render_text(&workspace, 20, 8);
    let _ = render_text(&workspace, 8, 5);

    workspace.timeline_ascii = true;
    let ascii = render_text(&workspace, 40, 12);
    assert!(ascii.contains('['), "{ascii}");
    assert!(
        ![
            '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█', '▌', '▐', '━', '▓', '◆', '┊'
        ]
        .into_iter()
        .any(|glyph| ascii.contains(glyph)),
        "{ascii}"
    );
}

#[test]
fn background_startup_draws_loading_state_before_exact_reconstruction() {
    let root = tempfile::tempdir().expect("temporary Project");
    fs::write(
        root.path().join("project.crump"),
        include_str!("../../../tests/fixtures/minimal.crump"),
    )
    .expect("write fixture");

    let mut workspace = Workspace::begin_load(root.path()).expect("begin background load");
    assert!(workspace.bundle.project.is_none());
    assert!(workspace.background_work_pending());
    let backend = ratatui::backend::TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("loading terminal");
    terminal
        .draw(|frame| render(frame, &workspace))
        .expect("draw loading state");

    settle_background(&mut workspace);
    assert_eq!(
        workspace
            .bundle
            .project
            .as_ref()
            .expect("loaded Project")
            .name,
        "Minimal"
    );
}

#[test]
fn reload_bursts_coalesce_while_navigation_and_draw_remain_available() {
    use std::sync::{Arc, Barrier, mpsc};

    let root = tempfile::tempdir().expect("temporary Project");
    let path = root.path().join("project.crump");
    let source = include_str!("../../../tests/fixtures/minimal.crump");
    fs::write(&path, source).expect("write fixture");
    let mut workspace = Workspace::load(root.path()).expect("load workspace");
    let mut keys = Keymap::default();
    let candidate_reloads = workspace.performance.candidate_reloads;
    let gate = Arc::new(Barrier::new(2));
    let worker_gate = Arc::clone(&gate);
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    workspace
        .worker
        .submit(move || {
            started_tx.send(()).expect("signal blocked job");
            worker_gate.wait();
            WorkspaceWorkResult::Reload(ReloadBatch {
                candidate: None,
                history: None,
            })
        })
        .expect("submit blocked job");
    started_rx.recv().expect("blocked job started");

    fs::write(
        &path,
        source.replace("project \"Minimal\"", "project \"Burst B\""),
    )
    .expect("write intermediate Candidate");
    workspace.refresh_if_changed();
    keys.handle(
        &mut workspace,
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    );
    assert_eq!(workspace.view, View::Mixer);
    let backend = ratatui::backend::TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("responsive terminal");
    terminal
        .draw(|frame| render(frame, &workspace))
        .expect("draw while reload is blocked");

    fs::write(
        &path,
        source.replace("project \"Minimal\"", "project \"Burst C\""),
    )
    .expect("write newest Candidate");
    workspace.refresh_if_changed();
    gate.wait();
    settle_background(&mut workspace);

    assert_eq!(
        workspace.bundle.project.as_ref().expect("Project").name,
        "Burst C"
    );
    assert_eq!(
        workspace.performance.candidate_reloads,
        candidate_reloads + 1,
        "the superseded intermediate Candidate must never be reconstructed"
    );
}

#[test]
#[ignore = "manual release-mode workspace performance profile"]
fn profile_workspace_hot_paths() {
    const DRAW_ITERATIONS: u32 = 50;
    const TERMINAL_WIDTH: u16 = 140;
    const TERMINAL_HEIGHT: u16 = 32;
    let project = std::env::var_os("CRUMP_WORKSPACE_PROFILE_PROJECT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/fixtures/minimal.crump"));

    let started = Instant::now();
    let mut workspace = Workspace::load(&project).expect("load profiled workspace");
    eprintln!(
        "workspace-profile load_ms={:.3} project={} terminal={}x{}",
        started.elapsed().as_secs_f64() * 1_000.0,
        project.display(),
        TERMINAL_WIDTH,
        TERMINAL_HEIGHT,
    );

    for sample in 0..5 {
        let started = Instant::now();
        let changed = workspace.refresh_if_changed();
        let queued = started.elapsed();
        settle_background(&mut workspace);
        eprintln!(
            "workspace-profile unchanged-poll sample={sample} changed={changed} queue_us={:.3} completion_us={:.3}",
            queued.as_secs_f64() * 1_000_000.0,
            started.elapsed().as_secs_f64() * 1_000_000.0
        );
    }
    let started = Instant::now();
    workspace.refresh();
    let queued = started.elapsed();
    settle_background(&mut workspace);
    eprintln!(
        "workspace-profile forced-refresh queue_us={:.3} completion_ms={:.3}",
        queued.as_secs_f64() * 1_000_000.0,
        started.elapsed().as_secs_f64() * 1_000.0
    );

    for view in VIEWS {
        workspace.view = view;
        workspace.clamp_selections();
        let backend = ratatui::backend::TestBackend::new(TERMINAL_WIDTH, TERMINAL_HEIGHT);
        let mut terminal = Terminal::new(backend).expect("profile terminal");
        terminal
            .draw(|frame| render(frame, &workspace))
            .expect("warm profile draw");
        let started = Instant::now();
        let allocations = allocation_counter::measure(|| {
            for _ in 0..DRAW_ITERATIONS {
                terminal
                    .draw(|frame| render(frame, std::hint::black_box(&workspace)))
                    .expect("profile draw");
            }
        });
        let elapsed = started.elapsed();
        eprintln!(
            "workspace-profile draw view={} iterations={DRAW_ITERATIONS} total_ms={:.3} mean_us={:.3} allocations={} allocated_bytes={} allocations_per_draw={:.1} bytes_per_draw={:.1}",
            view.title(),
            elapsed.as_secs_f64() * 1_000.0,
            elapsed.as_secs_f64() * 1_000_000.0 / f64::from(DRAW_ITERATIONS),
            allocations.count_total,
            allocations.bytes_total,
            allocations.count_total as f64 / f64::from(DRAW_ITERATIONS),
            allocations.bytes_total as f64 / f64::from(DRAW_ITERATIONS),
        );
    }
}
