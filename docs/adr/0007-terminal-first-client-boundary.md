# ADR 0007: Terminal-First Client Boundary

Status: Superseded in part by [ADR 0029](0029-desktop-window-is-the-only-client.md) (the Ratatui client is removed; the client-independent core stands)
Date: 2026-07-28

Crump's human client is a Ratatui production workspace, but terminal rendering does not define or own the Project model, authoring operations, persistence, or audio engine. The terminal combines keyboard production and source inspection while the core remains terminal-independent.

Crump has no browser client; [ADR 0027](0027-optional-desktop-client.md) supersedes the earlier exclusion of a desktop client with an optional one. Ratatui is an adapter over the same model and operations used by the CLI and engine, not an engine boundary.
