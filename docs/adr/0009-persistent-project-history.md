# ADR 0009: Persistent Project History

Status: Accepted
Date: 2026-07-28

Crump persists exact action-level Project History across close and reopen rather than delegating ordinary comparison and restoration to Git. Every entry is retained without automatic expiry or compaction.

Restoring an earlier state appends that exact source-and-media snapshot as a new entry referencing the selected entry. Restore never deletes history, moves a pointer, or creates a user-visible branch. Git operations remain user-controlled; Crump never silently changes repository history. Current Project source and the Active pointer, not the History log by itself, determine current state.
