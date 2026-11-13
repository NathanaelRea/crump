# ADR 0025: Module-Owned Media Assets

Status: Accepted
Date: 2026-11-12

## Context

An Instrument Module is meant to be copied or imported across Projects. A sample-backed module could not be: its zones named asset IDs that only the importing `project.crump` declared. Reusing one 18-sample violin meant copying its media and pasting 18 hash-bearing declarations into the root, and a 32-instrument orchestra Project needed 424 of them. The declarations described the module's content, but lived in a different file from the module.

## Decision

A `crump-module 2` module may declare `asset` entries at top level, using the ordinary asset syntax and usage tags.

- A module asset path is relative to the module's directory. Import resolves it to a normalized Project-relative path, which must not leave the module's directory.
- After import, a module asset is an ordinary Project Media Asset. It shares the Project's asset ID namespace, path uniqueness, budgets, exact-byte SHA-256 identity, Revision hashing, History snapshots, and changed-media diagnostics. The engine, validator and edits see no new kind of asset.
- `asset collect` leaves module-owned media in place, so the module directory stays self-contained.
- A personal-library module is a single source file. A `library:` import, `module install`, or library validation of a module that owns assets fails with `CRL010`. `module export` still copies only the Instrument declaration, whose asset references the importing Project must then satisfy.
- `crump-module 1` modules cannot own assets.

## Consequences

- Copying a module directory with its media into a Project needs one `import` line.
- Two imported modules declaring the same asset ID conflict, exactly as two Project declarations would.
- Bringing media into the personal library would need a multi-file library format. That is deferred.

## Alternatives

- **Keep assets Project-only and generate the declarations.** This keeps the root as the only authority but leaves every copy carrying hundreds of lines that duplicate the module's own data.
- **Implicit media discovery from a module's `media/` folder.** Rejected: it would make media identity depend on directory contents instead of readable source with exact hashes.
