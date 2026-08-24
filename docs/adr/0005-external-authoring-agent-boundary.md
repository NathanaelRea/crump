# ADR 0005: External Authoring Agent Boundary

Status: Accepted
Date: 2026-07-28

External Authoring Agents edit normal Project files and use stable `crump` CLI operations for machine-readable inspection, validation, checks, explicit publication, History, modules, assets, and rendering. Publication validates the complete source state and records Agent origin and explanation whether or not the TUI is running; unpublished filesystem writes remain Candidate Revisions.

Model selection, inference, and the conversation loop belong to a user-chosen harness running beside Crump. Crump embeds no model runtime, provider integration, chat loop, Agent terminal, or PTY. It has no MCP server or harness-specific plug-in.

Every core operation remains manually available without a model, paid service, paid asset, or hosted backend. Every Agent Change enters Authoritative Source through normal validation and reversible Project History.
