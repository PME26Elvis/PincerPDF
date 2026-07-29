# ADR-013: Initial PDF engine responsibility split

- Status: Accepted
- Date: 2026-07-29
- Depends on: ADR-004, executable capability probe

## Context

PincerPDF needs mature PDF parsing, transformation and rendering behavior without allowing concrete engines to leak into domain, application or UI code. The first deterministic Linux probe exercised QPDF 11.3.0 and MuPDF tools 1.21.1 against generated page, outline, form and encryption fixtures.

A command succeeding is not enough to advertise semantic parity. The probe therefore inspects QDF object graphs and records page counts, outline nodes, dangling destinations, AcroForm field arrays, widget annotations, rendered pages and password behavior.

## Decision

### QPDF responsibility

Use a process-isolated QPDF adapter as the first candidate for:

- structural inspection and page-count discovery,
- page extraction and page-sequence assembly,
- page rotation,
- AES-256 encryption and password-gated access,
- normalized QDF output used by semantic contract tests.

These responsibilities remain capability-gated by exact executable contract suites. Domain and application code interact only through `PdfEnginePort` and operation-specific ports; they never construct QPDF command lines.

### MuPDF responsibility

Use MuPDF tools as the first renderer and independent visual oracle for:

- deterministic page rasterization,
- thumbnail/preview experiments,
- visual differential evidence after structural transforms,
- secondary inspection when it provides information independent of QPDF.

The initial decision does not grant MuPDF responsibility for document mutation.

### Outline ownership

PincerPDF owns outline planning and semantic repair.

Measured QPDF page selection retained all three source outline nodes after removing the third page, but left one destination pointing to `null`. Measured page assembly did not retain an outline tree. Therefore raw QPDF output must not be advertised as bookmark-preserving.

Before writing an output that promises bookmarks, PincerPDF must:

1. parse the source outline tree into a domain model,
2. map source page identities to output page identities,
3. prune or redirect entries whose target pages are absent,
4. rebuild parent/child/sibling links and counts,
5. validate that every destination resolves to an output page.

### Form capability remains provisional

The single-page fixture retained its catalog AcroForm, one field array and one widget annotation after page selection. This is encouraging evidence, not broad form parity. The `Forms` capability remains experimental until the corpus covers multi-page fields, shared resources, duplicate field names, merge collisions, appearance streams, flattening and encrypted forms.

### Adapter safety contract

Every native process adapter must provide:

- redacted command evidence,
- stable error-code mapping,
- bounded stdout/stderr capture,
- cancellation and timeout behavior,
- temporary sibling output followed by validated atomic finalization,
- exact engine identity/version reporting,
- no password or secret persistence in logs, arguments shown to users, or reports.

## Consequences

- The first Merge/Extract/Rotate vertical slices may use QPDF behind an adapter once their dedicated contract suites are green.
- Bookmark-aware Merge and Split require PincerPDF outline reconstruction rather than direct command wrapping.
- Rendering and screenshot evidence can proceed through MuPDF without coupling UI code to the renderer.
- Engine upgrades are behavior changes: the capability probe must run and any changed semantic observation requires ADR review.
- Future FFI or library adapters may replace process adapters without changing domain/application contracts.
