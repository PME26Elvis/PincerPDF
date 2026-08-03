# ADR-027: Engine-independent Split planner boundary

- Status: Accepted for P5.1
- Date: 2026-08-03
- Scope: Split family foundation

## Decision

Introduce `pincerpdf-split` as an engine-independent planner. It owns the
partition rules `every-page`, `every:N`, and ordered explicit ranges, and
returns exact one-based page vectors plus deterministic output stems. Bookmark
boundaries retain a human-readable title, the zero-based source outline depth,
and are validated for strictly ascending one-based page targets. The planner
performs no filesystem writes, PDF parsing, password handling or engine
invocation. The QPDF adapter consumes the plan through a separate materializer
that applies hidden sibling outputs, page-count verification, source-alias
protection and atomic finalization for every part. Bookmark inspection now
supports an explicit depth: depth `0` preserves the original top-level
behavior, while a nested depth recursively traverses the outline and requires
valid destinations only for the selected nodes. Intermediate outline nodes may
omit destinations when they contain usable descendants.

The internal CLI exposes `split-plan` so the planner can be smoke-tested in a
restricted environment before any PDF materializer is enabled in the UI.

## Invariants

- zero-page sources fail before a plan is returned;
- fixed-count plans cover every source page exactly once;
- explicit ranges preserve user order and deliberate duplicates;
- range bounds are checked against the inspected source page count;
- output ordinals and filename stems are stable across repeated runs; and
- no plan can cause output creation before application-level validation.

## Deferred behavior

Bookmark destination remapping and collision policies remain gated until their
own real-engine contract fixtures prove page conservation and output validity.
The nested boundary selection and desktop depth control are now implemented,
but the materializer intentionally emits destination-free page subsets until
outline reconstruction is separately verified. Split-by-size estimation is
defined separately by ADR-028.
