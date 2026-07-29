# PDF Engine Capability Spike

Status: **Baseline measured**  
Phase: P2  
Decision owner: architecture ADR process

## Purpose

PincerPDF must not select a PDF engine from reputation or API surface alone. This spike measures behavior in the exact pinned Linux development image before assigning production responsibilities to QPDF, MuPDF, or future adapters.

## Deterministic fixture set

`tests/fixtures/pdf/generate_fixtures.py` writes PDF 1.7 files without third-party Python packages. Generated files are intentionally tiny and inspectable:

- `plain-three-pages.pdf`: mixed page sizes, metadata, and an existing 90-degree page rotation.
- `bookmarks.pdf`: three pages with a two-level outline tree.
- `acroform.pdf`: one page containing a text-field widget and catalog `AcroForm`.
- `encrypted.pdf`: derived during the probe with QPDF; passwords are never written to the report.

The generator writes object offsets, xref tables, trailer references, byte lengths, and SHA-256 values deterministically. Generated binaries and reports remain under `.artifacts/` and are not committed.

## Measured operations

The baseline probe measures:

1. QPDF structural checks and page-count inspection.
2. MuPDF information inspection and page rendering.
3. QPDF page extraction.
4. QPDF multi-source page assembly.
5. QPDF page rotation followed by structural and rendering checks.
6. QPDF AES-256 encryption, correct-password access, and wrong-password rejection.
7. QDF object-graph analysis of outline nodes/destinations and AcroForm field/widget structures before and after page selection.
8. QDF inspection of page-assembly output for outline preservation.

Every external command records its redacted command line, exit code, duration, bounded stdout/stderr, and pass/fail status in `report.json`.

## Baseline evidence

GitHub Actions run `30427856572` completed successfully in the pinned Linux image. Artifact `8714417168` has digest `sha256:a8b836838b12766bcf946905ae585420685309937a254abdae265aefa0810e6c` and contains the JSON report, deterministic fixtures, derived PDFs and rendered pages.

Environment:

- QPDF `11.3.0`
- MuPDF tools `1.21.1`
- pinned PincerPDF Linux development image

Results:

- 32/32 measured commands passed.
- QPDF validated all deterministic source and derived PDFs.
- QPDF returned correct page counts for source, extracted, assembled and password-unlocked documents.
- MuPDF inspected and rendered all source fixtures and the rotated output.
- QPDF produced page extraction, page assembly, rotation and AES-256 encrypted outputs accepted by the corresponding checks.
- Correct-password access succeeded; wrong-password access failed with a non-zero exit code while evidence remained redacted.
- The report contains none of the three configured test password strings.

Deterministic source fixture hashes:

- `plain-three-pages.pdf`: `df84e64a0575b5027b4f40552a3df0a162cb6d8c5eaf38cb29beb0caaa0be4ca`
- `bookmarks.pdf`: `409bd3157225468eb91fb9474aece1592412e15cf054f9f67ad3712b4202b454`
- `acroform.pdf`: `aaf80f49c45189d401c8b76a18ae5362943cfccc51c9b48bf1c6ca8e3e4bb58e`

## Semantic observations

- The source bookmark fixture contained three outline nodes and no dangling destinations.
- Selecting pages 1–2 retained all three outline nodes but changed the removed page destination to `null`; the output therefore contained one dangling outline destination.
- The measured page assembly contained three page objects but no outline tree.
- The source AcroForm contained one field array and one widget annotation.
- Selecting its only page retained the catalog AcroForm, one field array and one widget annotation.

Token presence alone is not accepted as preservation evidence. The outline result demonstrates why object-graph validation is mandatory.

## Architectural result

The baseline supports the responsibility split recorded in `ADR-013`:

- QPDF is the initial structural transformation/encryption candidate behind isolated Rust ports.
- MuPDF is the initial renderer and visual-verification oracle.
- PincerPDF owns outline parsing, page-target remapping, pruning and rebuilding.
- Form preservation remains experimental pending a broader corpus.

No concrete engine is referenced from domain, application or UI code. Capabilities are granted only by contract suites for the exact behavior and engine version measured.

## Acceptance criteria

- Fixture generation is deterministic and independently parseable.
- Every source passes QPDF validation and MuPDF inspection/rendering.
- Derived extraction, assembly, rotation and encryption outputs pass explicit semantic assertions.
- Wrong-password access fails without exposing either configured password in the report.
- Outline and form claims are based on object-graph analysis rather than token presence alone.
- The report is valid JSON and uploaded as a workflow artifact.
- Repository files remain unchanged after the probe.

All baseline acceptance criteria passed.

## Next corpus expansion

Before production parity claims, extend the suite with:

- malformed and recoverable PDFs,
- xref streams and object streams,
- inherited page boxes and resources,
- Unicode metadata and outline titles,
- named destinations and action dictionaries,
- multi-page/duplicate-name AcroForms and appearance streams,
- merge collisions and bookmark policies,
- multiple encryption revisions and permission combinations,
- PDFsam differential outputs for the accepted baseline.
