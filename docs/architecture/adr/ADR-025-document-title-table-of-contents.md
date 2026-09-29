# ADR-025: Document-title table-of-contents pages

- Status: Accepted
- Scope: P4 Merge table-of-contents policy
- Date: 2026-08-03

## Decision

Add `MergeTocPolicy::DocumentTitles` as a second generated contents mode. The
adapter reads the source PDF information dictionary during the existing bounded
inspection step and uses a non-empty `/Title` value for each contents row. If a
source has no usable title, the source filename remains the deterministic
fallback. The policy shares the filename mode's pagination, A4 geometry, page
offset planning, atomic output and bookmark reconstruction.

Metadata titles are not used to rename source bookmark entries or filename
footers; those remain stable filename-based presentation. This keeps the
contents policy explicit and prevents a document's metadata from silently
changing unrelated output labels.

## Measured limits

The first implementation decodes PDF literal strings and UTF-16BE hexadecimal
strings from QPDF's normalized information dictionary. Unsupported encodings or
empty values use the filename fallback. Font embedding, PDFDocEncoding fidelity
outside the covered corpus, and visual golden evidence remain follow-up work
under the broader table-of-contents and typography parity rows.

## Evidence

- Rust unit tests cover literal escaping, UTF-16BE hex titles, malformed values
  and deterministic fallback behavior.
- The real QPDF/MuPDF Merge contract verifies a metadata title and a filename
  fallback in a generated contents page.
- Browser E2E verifies the explicit document-title policy control and its
  deterministic page-count result.
