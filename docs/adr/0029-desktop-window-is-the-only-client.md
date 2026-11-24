# ADR 0029: The Desktop Window Is the Only Human Client

Status: Accepted
Date: 2026-11-23
Supersedes: the Ratatui client of [ADR 0007](0007-terminal-first-client-boundary.md) and the shared terminal key bindings of [ADR 0027](0027-optional-desktop-client.md)

Crump removes the Ratatui terminal workspace. The egui desktop window is the one human client: a bare interactive `crump` opens it when a display is available, `crump gui` (alias `crump workspace`) opens it explicitly, and everything else, including `--json` and plain terminals, gets `status`. The `tui` Cargo feature and the `crossterm`, `ratatui`, and `unicode-width` dependencies are gone.

The window has its own keyboard shortcuts in desktop audio-app conventions (Space plays, Ctrl-Z undoes, Ctrl-S publishes, Ctrl-1–6 switch pages, arrows select and step, `M` turns on the note keyboard) instead of the terminal's modal, Vim-style table. Keeping two clients keyboard-identical had made the window carry prefix keys and modes that only made sense in a character grid.

What ADR 0007 established about the core still holds: the Project model, authoring operations, persistence, engine, and renderer depend on no client library, and the client changes the session only through `Workspace::dispatch(Action)`. Agents are unaffected; the CLI is their boundary.

## Consequences

- A headless machine (SSH, a console) has the CLI but no interactive workspace.
- `crump play` no longer puts the terminal in raw mode; `Ctrl-C` interrupts it like any other process.
- Workspace code that only served the terminal (character-column Timeline rulers, ASCII fallback, help scrolling, terminal draw latency) is removed rather than kept behind a feature.
