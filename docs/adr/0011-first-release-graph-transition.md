# ADR 0011: First-Release Graph Transition

Status: Accepted
Date: 2026-07-28

For the First Public Release, a valid instrument-topology change may reset the changed instrument's voices and effect state behind a short protective fade, while transport and unchanged Tracks continue and the workspace reports the restart. Requiring exact voice, envelope, delay, or reverb state migration would front-load a difficult runtime problem before the Authoring Agent loop is validated; restarting the whole transport would make iteration unnecessarily disruptive. Exact migration and edit-specific transition policies remain destination work.
