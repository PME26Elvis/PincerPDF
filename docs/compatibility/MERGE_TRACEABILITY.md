# Merge feature and legacy-test traceability

- Updated: 2026-08-03
- Upstream baseline: PDFsam Basic `6.0.5-SNAPSHOT`
- Current checkpoint: P4.8 table of contents (filename and document-title modes)

This ledger distinguishes a verified behavior from a complete Merge feature.
Rows remain partial until every policy named by the phase-one specification has
executable evidence.

## Feature ledger

| ID | Required behavior | Status | Current evidence | Remaining work |
| --- | --- | --- | --- | --- |
| MERGE-001 | Multiple inputs, ordering and duplicates | Verified | Rust planner/service tests; real QPDF/MuPDF ordered and duplicate-page contract | Add stress/performance thresholds during hardening |
| MERGE-002 | Per-input page ranges | Verified | Parser properties plus real disjoint, reordered, bounded and open-ended-to-last-page selections | Add only stress/performance coverage during hardening |
| MERGE-003 | Bookmark policies | Partial | All four typed policies are verified with QPDF JSON: discard, one entry, retained relevant hierarchy, and retained hierarchy under a document entry | Define/verify non-page destinations, actions, style, color and open-state fidelity |
| MERGE-004 | AcroForm policies | Partial | Form-bearing input is rejected before output creation | Rename fields, merge, flatten and discard |
| MERGE-005 | Table of contents | Partial | Real QPDF/MuPDF contract verifies generated filename and metadata-title contents pages, source first-page numbers, filename fallback and bookmark offsets; browser E2E verifies both explicit policies | Unicode typography and visual golden baseline |
| MERGE-006 | Blank page after odd input | Verified | Real QPDF/MuPDF contract verifies per-source insertion, final-source insertion, source-matched MediaBox/CropBox/rotation, empty text extraction, inherited `/Pages` geometry and bookmark destination offset remapping; browser E2E verifies the control | Add only stress/performance coverage during hardening |
| MERGE-007 | Filename footer | Partial | Real QPDF/MuPDF contract verifies a per-output-page source filename overlay, source geometry preservation and blank-page omission; browser E2E verifies the explicit control | Embedded Unicode font, non-ASCII filename fidelity, visual baseline and footer collision policy |
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

## Filename footer policy

The current P4.7 implementation generates a private one-page-per-output-page
overlay after QPDF page assembly. Each overlay page copies the contributing
source page's MediaBox, CropBox and rotation, and places the source filename
near the lower-left media-box origin. Generated odd-page blanks deliberately
receive no footer. The overlay is applied before any bookmark reconstruction,
so output outline destinations continue to refer to the final page objects.

The first contract slice uses a built-in Helvetica/WinAnsi resource and
therefore preserves ASCII filenames only. Non-ASCII names are replaced with a
visible fallback marker rather than being silently emitted as invalid PDF
literal bytes. Unicode font embedding and collision-aware placement remain
explicit follow-up work under MERGE-007.

## Document-level bookmark policy

ADR-021 defines document entries and ADR-022 defines source-outline retention. PincerPDF reconstructs a new
outline tree only after QPDF has assembled the selected pages, using the actual
output page object references rather than source references.

Windows real-engine evidence proves:

- source file base names become ordered top-level titles;
- the first entry targets output page 1;
- the second entry targets output page 2 when the first source contributes one
  selected page;
- duplicate source rows remain distinct entries;
- the result passes QPDF structural validation; and
- the complete catalog survives the JSON update while the trailer is not
  replaced.

The real-engine source-outline contract additionally selects source pages 2-3
from a nested-outline fixture. It prunes excluded `Chapter 1`, retains
`Chapter 2` at output page 1 and its `Appendix` child at output page 2. The
grouped policy places that retained hierarchy below the fixture base-name entry
at output page 2 after a preceding plain source. `MERGE-003` stays Partial only
because non-page destinations and outline presentation attributes remain out of
scope.

## Legacy test migration

| Upstream test | Legacy intent | Current target | Disposition |
| --- | --- | --- | --- |
| `MergeOptionsPaneTest.java` | Advanced option defaults and selection | Browser advanced-policy E2E plus future policy-specific component tests | Partial |
| `MergeParametersBuilderTest.java` | Convert validated UI state to merge parameters | Typed desktop DTO, `MergeRequest` validation, ordered real-engine contracts | Replaced; advanced fields pending |
| `MergeSelectionPaneTest.java` | Source selection, ordering, duplication and validation | Browser E2E, page-selection unit tests, native system-dialog E2E | Replaced for current fields |
| `TestCycles.java` | Cycle/reordering helper behavior | Immutable source vector ordering and duplicate-preservation tests | Replaced |

No row is marked fully migrated merely because an implementation-detail Java
test became obsolete. Feature-ledger status remains the completion authority.
