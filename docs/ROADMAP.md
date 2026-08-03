# Roadmap

| Phase | Name | Status | Exit gate |
| --- | --- | --- | --- |
| P0 | Documentation baseline | Complete | Dossier accepted; durable repo guidance established. |
| P1 | Reproducible Linux environment | Complete | Fresh pinned container bootstraps and `make check-fast` passes. |
| P2 | PDF engine capability spike | Complete | Engine responsibility split supported by executable structural/render evidence and ADR-013. |
| P3 | Application shell and design system | Complete | Tauri/Leptos shell, accessibility basics, five browser E2E checks, and desktop/compact visual checkpoints pass. |
| P4 | Merge vertical slice | In progress | Merge parity and assigned legacy-test rows have passing evidence. |
| P5 | Split family | In progress | Engine-independent split planner is deterministic; QPDF output adapter and corpus gates remain. |
| P6 | Remaining PDF tools | Planned | All eight PDF tools reach verified functional parity. |
| P7 | Desktop completeness | Planned | Non-tool original features are implemented or explicitly replaced. |
| P8 | Hardening | Planned | Reliability, security, accessibility, performance and corpus gates meet RC thresholds. |
| P9 | Windows release candidate | Planned | High-spec signed-or-signable Windows build passes local/native acceptance before release automation. |
| P10 | Linux/macOS release expansion | Deferred | Linux compatibility evidence is promoted to a release build and macOS work starts after Windows RC acceptance. |

## Current P4 checkpoints

- **P4.1 — Merge core: Complete.** Engine-independent request/orchestration, process-isolated QPDF execution, password redaction, timeout/cancellation, semantic verification, atomic finalization and real QPDF/MuPDF evidence are green.
- **P4.2 — Merge desktop slice: Complete.** The trusted Tauri command boundary, shared DTOs, accessible Merge workspace, deterministic browser adapter, real desktop-command contract, eight browser E2E scenarios, four Windows visual checkpoints, three production-protocol WebView2 E2E scenarios, Windows replacement recovery tests, coordinate-free real system-dialog E2E, and the Linux milestone lane are green.
- **P4.3 — Merge parity corpus: Complete.** Windows and Linux evidence is green for mixed MediaBox/CropBox/rotation, safe source-metadata discard, Unicode/long paths, open-ended-to-last-page ranges and the durable feature/legacy-test ledger.
- **P4.4 — Document-level bookmarks: Complete.** Explicit discard and one-entry-per-document policies crossed the UI, desktop and engine boundaries and were integrated as `main@7a5f49e034a9098eae0794d6e38f7b751506a0ee` after Windows and Linux evidence passed.
- **P4.5 — Relevant source outlines: In progress.** `Retain` and `RetainAsOneEntryPerDocument` reconstruct only source hierarchy whose destinations survive the page selection. Windows real-engine evidence is green; full local cross-stack and Linux review evidence remain the exit gate.
- **P4.6 — Odd-page blank insertion: Complete.** Each odd-page source, including the final source, receives a verified blank page whose MediaBox, CropBox and rotation match the source's final selected page; bookmark destination offsets are remapped.
- **P4.7 — Filename footer overlay: Partial.** An explicit UI/DTO policy applies a geometry-matched source filename overlay to every non-blank output page and leaves generated blanks empty; ASCII QPDF/MuPDF contract evidence is green.
- **P4.8 — Table of contents: Partial.** Explicit filename and document-title UI/DTO policies prepend generated contents pages (with pagination for larger source lists) and keep document bookmarks pointed at shifted source pages; both real-engine modes are covered locally.
- Form collision handling, Unicode footer/contents fonts, normalization/compression and full legacy parity remain later P4 work. Footer placement now has a MuPDF text-aware quiet-band implementation, but still needs semantic coverage for forms, annotations, images and rotated writing.

## Current P5 checkpoints

- **P5.1 — Split planner/materializer: In progress.** `pincerpdf-split` plans every-page, fixed-count and explicit range outputs without engine or filesystem side effects. The QPDF adapter now materializes each part with page-count conservation, hidden sibling outputs and atomic finalization; the real contract is green locally and awaiting the next Linux Actions evidence. Bookmark-aware splitting and split-by-size remain gated.
