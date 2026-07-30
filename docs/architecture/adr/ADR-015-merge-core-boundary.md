# ADR-015: Establish the P4.1 Merge core boundary

- Status: Accepted
- Date: 2026-07-30
- Depends on: ADR-013, P3 application-shell gate

## Context

P4 begins the first real PDF-tool vertical slice. A Merge button must not be enabled merely because QPDF can concatenate page objects. The application needs an engine-independent request model, exact page-order planning, secret-safe native process execution, semantic output verification, and atomic finalization.

The P2 probe measured that QPDF page assembly does not retain an outline tree and that form preservation has only a narrow single-page signal. P4.1 therefore cannot honestly advertise bookmark-preserving or form-preserving Merge.

## Decision

- Add a presentation-independent `pincerpdf-merge` feature crate containing request validation, ordered source/page planning, cancellation and timeout policy, the operation-specific engine port, semantic verification, and atomic output finalization.
- Add a `pincerpdf-engine-qpdf` process adapter that implements inspection and Merge without leaking QPDF command construction into the application or UI.
- Preserve source order and deliberate page duplicates exactly.
- Accept encrypted inputs through temporary line-oriented password files; command evidence always replaces the password-file argument with a redaction marker.
- Bound stdout and stderr capture, drain both streams, and kill the child on timeout or cooperative cancellation.
- Write only to a hidden sibling temporary path. Inspect page count, bookmarks, and forms before an atomic rename to the user destination.
- Intentionally discard source bookmarks in P4.1 and report how many bookmark-bearing sources were encountered.
- Reject any source containing an AcroForm until the form collision and appearance-stream corpus is complete.
- Keep the Merge UI action gated until P4.2 provides a Tauri command, deterministic platform adapters, browser/native E2E, and accepted visual states.

## Consequences

- The internal CLI can perform a real full-document Merge and serves as the first executable composition root.
- P4.1 is useful and independently testable, but it is not complete PDFsam Merge parity.
- Bookmark rebuilding, metadata policy, form policy, table of contents, footer, normalization, compression, and advanced filename/workspace behavior remain explicit P4 work rather than hidden omissions.
- A successful QPDF process is not sufficient: the application verifies the temporary output before finalization.

## Evidence contract

The dedicated Linux workflow must retain:

- exact Cargo lock output,
- all Rust unit tests,
- real QPDF/MuPDF Merge contract output,
- ordered and duplicate-page text evidence,
- encrypted-input redaction checks,
- form rejection evidence,
- semantic `qpdf --check` and page counts,
- output SHA-256 values and engine versions.

## Accepted evidence

GitHub Actions run `30511384208` passed this contract on source head `0f00b8373b911bf38634c9f757813390a55f8559` with QPDF `11.3.0` and MuPDF `1.21.1`. The ordered five-page output and encrypted-input six-page output both passed semantic inspection and independent text extraction. Artifact `8747323877` has digest `sha256:6a3cff1e05b27e9ff0e545d65b53faf5be20de794404c8b70c33f4c995e07670`.

The same head passed Linux quality run `30511384211`, the 32-command PDF engine capability probe run `30511384218`, and application-shell regression run `30511384187`. P4.1 is therefore accepted without changing the P3 Merge UI gate.
