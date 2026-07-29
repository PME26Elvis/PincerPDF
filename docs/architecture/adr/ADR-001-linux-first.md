# ADR-001: Adopt Linux-first delivery

- Status: Accepted
- Date: 2026-07-29

## Context

PincerPDF is a long-running Rust rewrite of a mature Java/JavaFX PDF desktop application, developed primarily in an ephemeral Linux container with strict parity and visual quality requirements.

## Decision

Develop and validate a complete Linux version in the container before adding Windows/macOS release lanes.

## Consequences

- Implementation and tests must follow this direction.
- Deviations require a superseding ADR and traceability update.
- Phase gates include evidence that this decision is operating as intended.

## Revisit trigger

Revisit only when executable evidence shows the decision blocks required parity, reliability, security or delivery—not merely because another tool is fashionable.
