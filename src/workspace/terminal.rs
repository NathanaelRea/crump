use super::*;

/// Runs the first-party terminal workspace for one Project.
pub fn run(path: impl AsRef<Path>) -> Result<(), WorkspaceError> {
    let mut app = Workspace::begin_load(path.as_ref())?;
    let mut terminal = TerminalSession::enter()?;
    let result = run_loop(&mut terminal.terminal, &mut app);
    app.finish_durable_mutation_before_shutdown();
    if let Some(player) = app.player.take() {
        app.retire_queue.retire(player);
    }
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
    while !app.quit {
        app.performance.loop_iterations = app.performance.loop_iterations.saturating_add(1);
        app.poll_audio();
        dirty |= app.poll_background_work();
        dirty |= app.release_stale_auditions();
        if Instant::now() >= next_refresh {
            dirty |= app.refresh_if_changed();
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
        let animated =
            app.player.is_some() || app.performance.visible() || !app.held_notes.is_empty();
        let timeout = if app.background_work_pending() {
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
                    app.handle_key(key);
                    pending_event.get_or_insert(event_read_started);
                    dirty = true;
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => {
                        app.performance.events = app.performance.events.saturating_add(1);
                        app.move_selection(1);
                        pending_event.get_or_insert(event_read_started);
                        dirty = true;
                    }
                    MouseEventKind::ScrollUp => {
                        app.performance.events = app.performance.events.saturating_add(1);
                        app.move_selection(-1);
                        pending_event.get_or_insert(event_read_started);
                        dirty = true;
                    }
                    _ => {}
                },
                Event::Resize(_, _) | Event::FocusGained => dirty = true,
                Event::FocusLost | Event::Paste(_) => {}
            }
        }
        if app.editor_requested {
            pending_event = None;
            app.editor_requested = false;
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
    let Some(relative_path) = app.bundle.sources.keys().nth(app.selected_source).cloned() else {
        app.status = "No Project source file is selected".to_owned();
        return Ok(());
    };
    if let Some(player) = app.player.as_mut() {
        let _ = player.all_notes_off();
    }
    app.held_notes.clear();

    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    disable_raw_mode()?;
    let editor = std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| "vi".into());
    let source_path = app.root.join(&relative_path);
    if !source_path.exists()
        && let Some(source) = app.bundle.sources.get(&relative_path)
    {
        // Personal-library imports are snapshotted under a safe project-local
        // display path. Editing one materializes that exact source locally so
        // the shared library is never modified as a side effect.
        write_atomic(&source_path, source.as_bytes())?;
    }
    let result = Command::new(editor).arg(&source_path).status();

    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    let area = terminal.size()?.into();
    terminal.resize(area)?;
    app.status = match result {
        Ok(status) if status.success() => {
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
    fn enter() -> Result<Self, io::Error> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let mut stdout = io::stdout();
                let _ = execute!(stdout, DisableMouseCapture, LeaveAlternateScreen);
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
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}
