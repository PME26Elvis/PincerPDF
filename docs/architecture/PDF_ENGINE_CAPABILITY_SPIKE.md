# PDF Engine Capability Spike

Status: **Executing**  
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

The first probe measures:

1. QPDF structural checks and page-count inspection.
2. MuPDF information inspection and page rendering.
3. QPDF page extraction.
4. QPDF multi-source merge.
5. QPDF page rotation followed by structural and rendering checks.
6. QPDF AES-256 encryption, correct-password access, and wrong-password rejection.
7. Source and subset detection of outline and `AcroForm` catalog structures through QDF output.

Every external command records its redacted command line, exit code, duration, bounded stdout/stderr, and pass/fail status in `report.json`.

## Acceptance criteria

- Fixture generation is deterministic and independently parseable.
- Every source passes QPDF validation and MuPDF inspection/rendering.
- Derived extraction, merge, rotation, and encryption outputs pass their explicit semantic assertions.
- Wrong-password access fails without exposing either configured password in the report.
- The report is valid JSON and uploaded as a workflow artifact.
- Repository files remain unchanged after the probe.

## Decision rule

A successful command proves only the measured behavior on the fixture corpus. It does not grant a broad capability automatically. Production capability flags require a growing contract suite that includes malformed inputs, Unicode metadata, page boxes, inherited resources, object streams, forms, bookmarks, encryption variants, and differential outputs against the accepted PDFsam baseline.

The engine-responsibility ADR remains proposed until the report has been reviewed and preservation gaps are explicitly assigned to an adapter or application-layer reconstruction strategy.
