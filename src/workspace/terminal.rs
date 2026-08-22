use super::*;

/// Runs the first-party terminal workspace for one Project.
pub fn run(path: impl AsRef<Path>) -> Result<(), WorkspaceError> {
    let mut app = Workspace::load(path.as_ref())?;
    let mut terminal = TerminalSession::enter()?;
    let result = run_loop(&mut terminal.terminal, &mut app);
    if let Some(player) = app.player.take() {
        let _ = player.shutdown();
    }
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut Workspace,
) -> Result<(), WorkspaceError> {
    let mut last_refresh = Instant::now();
    while !app.quit {
        app.poll_audio();
        app.release_stale_auditions();
        if last_refresh.elapsed() >= REFRESH_RATE {
            app.refresh();
            last_refresh = Instant::now();
        }
        terminal.draw(|frame| render(frame, app))?;
        if event::poll(TICK_RATE)? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => app.move_selection(1),
                    MouseEventKind::ScrollUp => app.move_selection(-1),
                    _ => {}
                },
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
            }
        }
        if app.editor_requested {
            app.editor_requested = false;
            launch_source_editor(terminal, app)?;
            last_refresh = Instant::now();
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
