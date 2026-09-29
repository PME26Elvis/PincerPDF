≠rá^—f•ñÿ¶{^ly 'v√Æ∂õ≠# ADR-021 ‚Äî Rebuild document-level Merge bookmarks after page assembly

- Status: Accepted
- Date: 2026-07-31
- Scope: P4 Merge bookmark policy

## Context

The P2 engine probe established that QPDF page assembly does not preserve input
outline trees, while page subsetting can leave destinations that no longer
resolve. PincerPDF therefore cannot claim bookmark parity by forwarding either
behavior.

The first non-discard policy required by `MERGE-003` was one top-level entry
per ordered source document. Each entry must target that source row's first
contributed output page, including when selections reorder pages or the same
file appears more than once.

## Decision

`Discard` remains the default. `OneEntryPerDocument` is a typed policy crossing
the Leptos, desktop DTO, application and engine boundaries. The broader
source-outline policies and their retention contract are defined by ADR-022.

For the document-level policy, the QPDF adapter:

1. assembles selected pages into a private intermediate PDF without outlines;
2. asks QPDF JSON v2 for the actual output page object references, trailer root,
   catalog and maximum object identifier;
3. creates a private JSON update that preserves the complete catalog, adds a
   new outline root, and adds one linked item per ordered source;
4. writes the updated PDF to the application-owned temporary destination;
5. runs QPDF structural validation; and
6. reads QPDF's high-level outline JSON and requires every title, destination
   page and child count to match the application plan.

The update deliberately omits the trailer. A capability spike showed that
replacing the trailer with an incomplete dictionary can remove `/Size` and
produce a repair warning; updating only the catalog and new objects produces a
clean file.

Document-level titles use the source file base name (without its extension).
Duplicate source rows create duplicate entries at their distinct output offsets. JSON capture is bounded,
inherits timeout/cancellation, and fails closed if truncated or malformed. The
private update file contains no passwords and is removed with its private
temporary directory.

## Consequences

- PincerPDF owns bookmark policy and verification instead of relying on
  incidental QPDF page-copy behavior.
- The real Windows contract proves two entries targeting output pages 1 and 2;
  the real system-dialog scenario proves two six-page entries targeting pages
  1 and 4.
- Source outline preservation, retaining source trees below a document entry,
  and destination remapping are specified and verified separately in ADR-022.
- P4.4 was integrated to `main` only after its Linux compatibility evidence was
  green.

## References

- [QPDF JSON documentation](https://qpdf.readthedocs.io/en/latest/json.html)
- `ADR-013-initial-pdf-engine-responsibility-split.md`
- `ADR-022-source-outline-reconstruction.md`
- `docs/compatibility/MERGE_TRACEABILITY.md`
