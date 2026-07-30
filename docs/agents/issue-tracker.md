# Issue Tracker: Local Markdown

Issues, product requirements, and wayfinding maps for this repository live as Markdown files in `.scratch/`.

The directory is intentionally ignored by Git. It is shared by agent sessions using this workspace, but it is not part of repository history or available from another clone.

## Conventions

- One effort per directory: `.scratch/<effort-slug>/`.
- The canonical wayfinding map is `.scratch/<effort-slug>/map.md`.
- Child issues are `.scratch/<effort-slug>/issues/<NN>-<slug>.md`, numbered from `01`.
- A `Type:` line records `research`, `prototype`, `grilling`, or `task`.
- A `Status:` line records `open`, `claimed`, or `resolved`.
- A `Blocked by:` line records ticket numbers and linked names. A ticket is unblocked when every blocker is resolved.
- Resolution discussion is appended under `## Answer`; other history goes under `## Comments`.

## Wayfinding Operations

- **Load a map:** Read only `map.md` to orient to its destination, notes, resolved decisions, fog, and scope boundary.
- **Find the frontier:** Scan its `issues/` directory in numeric order. The frontier consists of open, unclaimed tickets whose blockers are all resolved.
- **Claim a ticket:** Change `Status: open` to `Status: claimed` before doing any work.
- **Resolve a ticket:** Add its answer under `## Answer`, change its status to `resolved`, and append a linked one-line gist to the map's `Decisions so far` section.
- **Add dependencies:** Create all new issue files first, then add their `Blocked by:` relationships in a second pass.

## Other Skill Operations

When a skill says to publish an issue or PRD, create it within an appropriately named `.scratch/<effort-slug>/` directory. When it says to fetch an issue, read the referenced Markdown file.
