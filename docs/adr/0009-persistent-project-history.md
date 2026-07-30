# ADR 0009: Persistent Project History

Status: Accepted
Date: 2026-07-28

Crump will persist its exact, action-level Project History across close and reopen rather than delegating ordinary comparison and restoration to Git. The First Public Release retains every entry without automatic expiry or compaction. Restoring an earlier state appends that exact state as a new entry referencing the selected entry; it never deletes history, moves a pointer, or creates a user-visible branch. This gives beginners and Agent workflows a dependable musical change timeline without requiring version-control knowledge. Git integration may create user-requested milestones or branches, but Crump will never silently alter repository history, and Project History remains non-authoritative relative to the current source.
