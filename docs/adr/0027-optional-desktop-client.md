# ADR 0027: Optional Desktop Client

Status: Accepted
Date: 2026-11-22
Supersedes: the "no desktop or browser client" clause of [ADR 0007](0007-terminal-first-client-boundary.md)
Amended: 2026-11-23, the desktop client is now default (see "Amendment" below)
Superseded in part by: [ADR 0029](0029-desktop-window-is-the-only-client.md), which removes the terminal client and its shared key bindings

Crump gains a second human client: an egui desktop window, compiled only with the opt-in `gui` Cargo feature and opened with `crump gui`. The Ratatui workspace stays the default and the reference client. Crump still has no browser client.

Both clients are adapters over one `Workspace` session. Neither owns Project state: each draws the session read-only and changes it only through `Workspace::dispatch(Action)`. They share one key-binding table (`workspace::keymap`), so the window is keyboard-complete in exactly the way the terminal is, and pointer input is expressed as further `Action` values rather than as window-side state.

## Consequences

- A capability lands in the session and its `Action` vocabulary first; a client only decides how to offer it. A feature reachable from one client and not the other is a defect unless it is inherently terminal- or pointer-shaped.
- Pointer positions cross the seam in the session's own terms (`TimelineAction::PlaceCursor` takes a column of the viewport), so a client needs no Musical Time arithmetic and grid snapping stays in one place.
- The window cannot lend a terminal to `$EDITOR`; it hands the selected source file to `xdg-open` and relies on the existing file watch to reload saved changes.
- `gui` is not a default feature: default builds, the `.deb`, and the headless build keep their dependency set. CI compiles and tests it through `--all-features`.
- The window's views are drawn without a display in tests by running egui headless, so client behaviour is covered without a compositor.

## Amendment: desktop client by default (2026-11-23)

`gui` is now a default feature, and a bare `crump` with no subcommand opens the window when it is interactive (no `--json`), a Wayland or X11 display is set, and the Project is known or a terminal can run the picker. Otherwise the earlier rule stands: an interactive terminal gets the Ratatui workspace and everything else gets `status`. `crump workspace` and `crump gui` choose a client explicitly. The Ratatui workspace remains the reference client that every capability must reach, and agents are unaffected because `--json` with no subcommand still means `status`.

This reverses the "not a default feature" consequence above: default builds and the `.deb` now carry the egui dependencies, and the `.deb` recommends the runtime graphics libraries the window loads. A headless build drops it with `--no-default-features --features pipewire,tui`.

## Alternatives

- Rendering the Ratatui frame into a window would have reused every view but kept a character grid, with no real pointer targets, meters, or arrangement drawing.
- A browser client would need a server process and a wire protocol for the session, which the in-process seam does not.
