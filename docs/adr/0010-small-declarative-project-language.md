# ADR 0010: Small Declarative Project Language

Status: Accepted
Date: 2026-07-28

Crump uses a purpose-built declarative DSL as the shared source for humans, Production Views, Authoring Agents, validation, playback, and rendering rather than using structured data alone or embedding a general-purpose language.

The language represents current musical, routing, processing, media, control, and History-facing semantics in readable source. It does not permit arbitrary execution, mutable variables, functions, or opaque project state.
