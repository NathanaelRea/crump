use super::*;
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};

/// Runs the first-party terminal workspace for one Project.
pub fn run(path: impl AsRef<Path>) -> Result<(), WorkspaceError> {
    let mut app = Workspace::open(path)?;
    app.set_help_line_count(rendering::help_line_count());
    let mut terminal = TerminalSession::enter()?;
    let result = run_loop(&mut terminal.terminal, &mut app);
    app.shutdown();
    let performance_report = app.performance.report();
    drop(terminal);
    if let Some(report) = performance_report {
        eprintln!("{report}");
    }
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut Workspace,
) -> Result<(), WorkspaceError> {
    let mut dirty = true;
    let mut next_refresh = Instant::now() + REFRESH_RATE;
    let mut pending_event: Option<Instant> = None;
    let mut keymap = Keymap::default();
    while !app.quit_requested() {
        app.performance.loop_iterations = app.performance.loop_iterations.saturating_add(1);
        dirty |= app.tick();
        if Instant::now() >= next_refresh {
            dirty |= app.poll_project_files();
            next_refresh = Instant::now() + REFRESH_RATE;
        }
        if app.player.is_some() || app.performance.visible() {
            dirty = true;
        }
        if dirty {
            let started = Instant::now();
            let result = terminal.draw(|frame| render(frame, app));
            app.performance.observe_draw(started.elapsed());
            result?;
            if let Some(started) = pending_event.take() {
                app.performance.observe_event_to_draw(started.elapsed());
            }
            dirty = false;
        }

        let until_refresh = next_refresh.saturating_duration_since(Instant::now());
        let animated = app.is_animating();
        let timeout = if app.has_pending_work() {
            BACKGROUND_POLL_RATE.min(until_refresh)
        } else if animated {
            TICK_RATE.min(until_refresh)
        } else {
            until_refresh
        };
        if event::poll(timeout)? {
            let event_read_started = Instant::now();
            match event::read()? {
                Event::Key(key) => {
                    app.performance.events = app.performance.events.saturating_add(1);
                    keymap.handle(app, key);
                    pending_event.get_or_insert(event_read_started);
                    dirty = true;
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => {
                        app.performance.events = app.performance.events.saturating_add(1);
                        app.dispatch(Action::MoveSelection(1));
                        pending_event.get_or_insert(event_read_started);
                        dirty = true;
                    }
                    MouseEventKind::ScrollUp => {
                        app.performance.events = app.performance.events.saturating_add(1);
                        app.dispatch(Action::MoveSelection(-1));
                        pending_event.get_or_insert(event_read_started);
                        dirty = true;
                    }
                    _ => {}
                },
                Event::Resize(_, _) | Event::FocusGained => dirty = true,
                Event::FocusLost | Event::Paste(_) => {}
            }
        }
        if app.take_editor_request() {
            pending_event = None;
            launch_source_editor(terminal, app)?;
            next_refresh = Instant::now() + REFRESH_RATE;
            dirty = true;
        }
    }
    Ok(())
}

fn launch_source_editor(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut Workspace,
) -> Result<(), WorkspaceError> {
    let Some(source_path) = app.source_edit_path()? else {
        return Ok(());
    };

    execute!(
        terminal.backend_mut(),
        PopKeyboardEnhancementFlags,
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    disable_raw_mode()?;
    let editor = std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| "vi".into());
    let result = Command::new(editor).arg(&source_path).status();

    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )?;
    let area = terminal.size()?.into();
    terminal.resize(area)?;
    app.status = match result {
        Ok(status) if status.success() => {
            let relative_path = source_path.strip_prefix(&app.root).unwrap_or(&source_path);
            format!("Editor closed · reloaded {}", relative_path.display())
        }
        Ok(status) => format!("Editor exited with {status} · reloaded Candidate"),
        Err(error) => format!("Could not launch $VISUAL/$EDITOR: {error}"),
    };
    app.refresh();
    Ok(())
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn enter() -> Result<Self, std::io::Error> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        if let Err(error) = execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        ) {
            let _ = execute!(
                stdout,
                PopKeyboardEnhancementFlags,
                DisableMouseCapture,
                LeaveAlternateScreen
            );
            let _ = disable_raw_mode();
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let mut stdout = std::io::stdout();
                let _ = execute!(
                    stdout,
                    PopKeyboardEnhancementFlags,
                    DisableMouseCapture,
                    LeaveAlternateScreen
                );
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            PopKeyboardEnhancementFlags,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}
