# Roadmap

| Phase | Name | Status | Exit gate |
| --- | --- | --- | --- |
| P0 | Documentation baseline | Complete | Dossier accepted; durable repo guidance established. |
| P1 | Reproducible Linux environment | Complete | Fresh pinned container bootstraps and `make check-fast` passes. |
| P2 | PDF engine capability spike | Complete | Engine responsibility split supported by executable structural/render evidence and ADR-013. |
| P3 | Application shell and design system | Complete | Tauri/Leptos shell, accessibility basics, five browser E2E checks, and desktop/compact visual checkpoints pass. |
| P4 | Merge vertical slice | In progress | Merge parity and assigned legacy-test rows have passing evidence. |
| P5 | Split family | Planned | No page loss/duplication across split corpus and edge cases. |
| P6 | Remaining PDF tools | Planned | All eight PDF tools reach verified functional parity. |
| P7 | Desktop completeness | Planned | Non-tool original features are implemented or explicitly replaced. |
| P8 | Hardening | Planned | Reliability, security, accessibility, performance and corpus gates meet RC thresholds. |
| P9 | Linux release candidate | Planned | High-spec Linux build accepted before GitHub multi-platform workflows. |
| P10 | Cross-platform expansion | Deferred | Starts only after Linux RC acceptance. |

## Current P4 checkpoints

- **P4.1 — Merge core: Complete.** Engine-independent request/orchestration, process-isolated QPDF execution, password redaction, timeout/cancellation, semantic verification, atomic finalization and real QPDF/MuPDF evidence are green.
- **P4.2 — Merge desktop slice: Next.** Add the Tauri command boundary, accessible Merge workspace, deterministic adapters, browser/native E2E and visual checkpoints before unlocking the Merge action.
- Bookmark reconstruction, form collision handling, metadata policy, table of contents, footer, normalization/compression and full legacy parity remain later P4 checkpoints.
