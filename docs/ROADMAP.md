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
| P9 | Windows release candidate | Planned | High-spec signed-or-signable Windows build passes local/native acceptance before release automation. |
| P10 | Linux/macOS release expansion | Deferred | Linux compatibility evidence is promoted to a release build and macOS work starts after Windows RC acceptance. |

## Current P4 checkpoints

- **P4.1 — Merge core: Complete.** Engine-independent request/orchestration, process-isolated QPDF execution, password redaction, timeout/cancellation, semantic verification, atomic finalization and real QPDF/MuPDF evidence are green.
- **P4.2 — Merge desktop slice: Complete.** The trusted Tauri command boundary, shared DTOs, accessible Merge workspace, deterministic browser adapter, real desktop-command contract, eight browser E2E scenarios, four Windows visual checkpoints, three production-protocol WebView2 E2E scenarios, Windows replacement recovery tests, coordinate-free real system-dialog E2E, and the Linux milestone lane are green.
- **P4.3 — Merge parity corpus: In progress.** Extend deterministic fixtures and contracts across mixed page boxes/rotation, metadata, bookmarks, encrypted inspection, Unicode/long paths, and mapped legacy behavior.
- Bookmark reconstruction, form collision handling, table of contents, footer, normalization/compression and full legacy parity remain later P4 checkpoints.
