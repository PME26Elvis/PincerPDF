# ADR-002: Use Tauri 2 with Leptos CSR

- Status: Accepted
- Date: 2026-07-29

## Context

PincerPDF is a long-running Rust rewrite of a mature Java/JavaFX PDF desktop application, developed primarily in an ephemeral Linux container with strict parity and visual quality requirements.

## Decision

DOM/CSS enables the supplied motion system, browser E2E and semantic accessibility while keeping UI logic in Rust.

## Consequences

- Implementation and tests must follow this direction.
- Deviations require a superseding ADR and traceability update.
- Phase gates include evidence that this decision is operating as intended.

## Revisit trigger

Revisit only when executable evidence shows the decision blocks required parity, reliability, security or delivery—not merely because another tool is fashionable.
