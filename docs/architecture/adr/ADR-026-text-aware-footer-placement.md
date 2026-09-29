# ADR-026: Text-aware footer placement

- Status: Accepted for P4.7 partial parity
- Date: 2026-08-03
- Scope: Merge filename footer overlay

## Decision

When a filename footer is enabled, the QPDF adapter may invoke the configured
`MuPDF` text extractor for each contributing source page. Its bounded
structured-text block boxes are converted from MuPDF's top-origin coordinates
to PDF user-space coordinates. The overlay chooses the lower quiet band when
there is at least 36 points of clearance; otherwise it chooses the upper band
when that band is available. If neither band is clear, the roomier edge wins
deterministically. Generated odd-page blanks retain an empty overlay page.

The renderer is an optional placement aid rather than a hard capability gate.
If it cannot be started, times out, or emits no parseable boxes, the adapter
uses the existing geometry-only lower-left position and still produces a valid
output.

## Rationale

- It avoids the most common footer collision without modifying source content.
- The placement remains stable across repeated pages and does not depend on
  rasterization or pixel thresholds.
- Keeping the fallback non-fatal preserves QPDF-only deployments and makes the
  feature usable on systems where MuPDF is not installed.
- The explicit quiet-band contract is testable without requiring Python or a
  GUI compositor.

## Limits

Structured text does not describe every visible object. Forms, annotations,
images, vector artwork, writing modes and rotated text can still occupy a
quiet-looking band. Those cases require a richer page-content collision model,
Unicode font embedding and visual golden evidence before the feature is
promoted to verified parity.
