# Merge feature and legacy-test traceability

- Updated: 2026-07-31
- Upstream baseline: PDFsam Basic `6.0.5-SNAPSHOT`
- Current checkpoint: P4.3 parity corpus

This ledger distinguishes a verified behavior from a complete Merge feature.
Rows remain partial until every policy named by the phase-one specification has
executable evidence.

## Feature ledger

| ID | Required behavior | Status | Current evidence | Remaining work |
| --- | --- | --- | --- | --- |
| MERGE-001 | Multiple inputs, ordering and duplicates | Verified | Rust planner/service tests; real QPDF/MuPDF ordered and duplicate-page contract | Add stress/performance thresholds during hardening |
| MERGE-002 | Per-input page ranges | Verified | Parser properties plus real disjoint, reordered, bounded and open-ended-to-last-page selections | Add only stress/performance coverage during hardening |
| MERGE-003 | Bookmark policies | Partial | Discard-and-report is verified; output is checked for no outline tree | Retain, one entry per document, retain under one entry |
| MERGE-004 | AcroForm policies | Partial | Form-bearing input is rejected before output creation | Rename fields, merge, flatten and discard |
| MERGE-005 | Table of contents | Planned | None | Filename and document-title modes |
| MERGE-006 | Blank page after odd input | Planned | None | Typed parity plan and rendered contract |
| MERGE-007 | Filename footer | Planned | None | Content overlay, Unicode filename and rendering evidence |
| MERGE-008 | Page normalization | Partial | `None` preserves MediaBox, CropBox and rotation across mixed geometry | Same width and orientation-aware same width |
| MERGE-009 | Single valid output | Verified baseline | QPDF structure/page count, MuPDF text/render, atomic output and system-dialog flow | Revalidate for every advanced-policy combination |

The P4.3 geometry fixture contains portrait, landscape, cropped, offset and
rotated pages plus source document metadata. The real-engine contract selects
those pages out of order, appends an open-ended `2-` selection through the last
page of another source, reads through a Unicode long source path and writes
through a Unicode long output path. It verifies:

- page-origin text order;
- exact MediaBox and CropBox arrays;
- 90° and 270° rotations;
- a valid five-page output;
- no implicit inheritance of one source document's Info dictionary.

The last item records current safe baseline behavior, not the final
table-of-contents or document-metadata product policy.

## Legacy test migration

| Upstream test | Legacy intent | Current target | Disposition |
| --- | --- | --- | --- |
| `MergeOptionsPaneTest.java` | Advanced option defaults and selection | Browser advanced-policy E2E plus future policy-specific component tests | Partial |
| `MergeParametersBuilderTest.java` | Convert validated UI state to merge parameters | Typed desktop DTO, `MergeRequest` validation, ordered real-engine contracts | Replaced; advanced fields pending |
| `MergeSelectionPaneTest.java` | Source selection, ordering, duplication and validation | Browser E2E, page-selection unit tests, native system-dialog E2E | Replaced for current fields |
| `TestCycles.java` | Cycle/reordering helper behavior | Immutable source vector ordering and duplicate-preservation tests | Replaced |

No row is marked fully migrated merely because an implementation-detail Java
test became obsolete. Feature-ledger status remains the completion authority.
