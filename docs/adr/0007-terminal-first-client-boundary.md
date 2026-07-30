# ADR 0007: Terminal-First Client Boundary

Status: Accepted
Date: 2026-07-28

Crump's first supported human client will be a Ratatui production workspace, but terminal rendering will not define or own the Project model, authoring operations, or audio engine. The terminal keeps keyboard production, source inspection, and an external agent harness cohesive while allowing the core to outlive Ratatui's visual constraints. This decision does not commit a desktop or browser client; it only prevents the initial client from becoming an irreversible engine boundary.
