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
restricted environment before any PDF materializer is enabled in the UI. Its
engine-backed `split` command accepts `bookmarks[:DEPTH]`; omitted depth keeps
the top-level policy and an explicit depth exercises nested outline selection.
The QPDF materializer now reads the source outline once, maps destinations
through each part's exact ordered page vector, prunes leaves whose destinations
are not present, and rebuilds the surviving hierarchy against the output page
objects before page-count and atomic-finalization checks. This applies to every
split rule, not only bookmark-boundary planning, so a page subset never claims
bookmark preservation while silently emitting dangling destinations. When QPDF
reports a named destination, unresolved destination, or action-backed outline,
the adapter fails closed with `capability_unavailable` rather than silently
dropping the entry. Split-only output vectors that repeat a source page also
fail closed when a surviving outline would have more than one valid output
occurrence; Merge retains its separately documented first-occurrence policy.
Direct-page outline nodes also carry QPDF's expansion state through
reconstruction, and `/Count` values are computed over all descendants with the
PDF-specified negative form for collapsed nodes.

## Invariants

- zero-page sources fail before a plan is returned;
- fixed-count plans cover every source page exactly once;
- explicit ranges preserve user order and deliberate duplicates;
- range bounds are checked against the inspected source page count;
- output ordinals and filename stems are stable across repeated runs; and
- no plan can cause output creation before application-level validation; and
- every surviving bookmark destination is remapped to a verified output page,
  while a source node with no surviving destination is retained only when it
  still contains a surviving descendant.

## Deferred behavior

Named destination resolution, action preservation, outline style/color/open-
state fidelity, and duplicate-page selections with ambiguous destination
identity remain gated until dedicated real-engine fixtures define their safe
policy. The parser now has explicit safety gates for these inputs, so they
cannot be mistaken for successfully preserved bookmarks. The nested boundary
selection, desktop depth control, and page-subset outline reconstruction are
implemented and covered by the split contract. Split-by-size estimation is
defined separately by ADR-028. Private QPDF staging may live on
a different filesystem from the selected output directory; the adapter copies
into the same-volume hidden sibling before the final atomic rename so Linux
containers and Windows volumes share the same correctness boundary.

