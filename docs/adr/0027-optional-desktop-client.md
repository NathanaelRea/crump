# ADR 0027: Optional Desktop Client

Status: Accepted
Date: 2026-11-22
Supersedes: the "no desktop or browser client" clause of [ADR 0007](0007-terminal-first-client-boundary.md)

Crump gains a second human client: an egui desktop window, compiled only with the opt-in `gui` Cargo feature and opened with `crump gui`. The Ratatui workspace stays the default and the reference client. Crump still has no browser client.

Both clients are adapters over one `Workspace` session. Neither owns Project state: each draws the session read-only and changes it only through `Workspace::dispatch(Action)`. They share one key-binding table (`workspace::keymap`), so the window is keyboard-complete in exactly the way the terminal is, and pointer input is expressed as further `Action` values rather than as window-side state.

## Consequences

- A capability lands in the session and its `Action` vocabulary first; a client only decides how to offer it. A feature reachable from one client and not the other is a defect unless it is inherently terminal- or pointer-shaped.
- Pointer positions cross the seam in the session's own terms (`TimelineAction::PlaceCursor` takes a column of the viewport), so a client needs no Musical Time arithmetic and grid snapping stays in one place.
- The window cannot lend a terminal to `$EDITOR`; it hands the selected source file to `xdg-open` and relies on the existing file watch to reload saved changes.
- `gui` is not a default feature: default builds, the `.deb`, and the headless build keep their dependency set. CI compiles and tests it through `--all-features`.
- The window's views are drawn without a display in tests by running egui headless, so client behaviour is covered without a compositor.

## Alternatives

- Rendering the Ratatui frame into a window would have reused every view but kept a character grid, with no real pointer targets, meters, or arrangement drawing.
- A browser client would need a server process and a wire protocol for the session, which the in-process seam does not.
