# Domain Docs

Crump uses a single domain context.

## Read Before Exploring

- Read `CONTEXT.md` for canonical product and music-production terminology.
- Read relevant records under `docs/adr/` before changing an architectural boundary.
- If either location is absent or has no relevant entry, proceed without inventing one.

## Consumer Rules

- Use glossary terms consistently in code, issues, tests, and documentation.
- Record only domain meanings in `CONTEXT.md`; keep implementation details and requirements elsewhere.
- Surface any proposed change that contradicts an accepted ADR.
- Create ADRs only for consequential, hard-to-reverse choices made through a real tradeoff.
