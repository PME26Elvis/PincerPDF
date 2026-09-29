# ADR-024: Filename table-of-contents pages

- Status: Accepted for P4.8 partial parity
- Date: 2026-08-03
- Scope: Merge output policy

## Decision

When `MergeTocPolicy::FileNames` is enabled, PincerPDF creates a private PDF
contents document before final assembly. It uses one A4 page for each group of
35 ordered source rows, lists each source filename and its first output page,
then prepends those pages to the assembled source output with QPDF. Bookmark
planning starts after the inserted contents pages, so document-level entries
continue to target source pages rather than the contents page.

The default remains `None`. The policy crosses the engine request, application
options, native desktop DTO and Leptos advanced-safety controls.

## Rationale

- A separate generated document avoids mutating source pages and makes page
  insertion explicit in the page-count contract.
- Pagination is deterministic and bounded, with no dependence on source page
  geometry or external browser rendering.
- Applying contents before bookmark reconstruction keeps the final outline tree
  based on the actual shifted output page objects.

## Known limits

The first mode lists filenames only and uses Helvetica/WinAnsi on fixed A4
pages. Document-title mode, Unicode fonts, configurable typography and visual
golden evidence remain follow-up work before MERGE-005 can become Verified.
