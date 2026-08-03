# Project State

- Updated: 2026-08-03
- Phase: P5 — Split family (P5.1 planner, materializer and desktop slice)
- Repository: https://github.com/PME26Elvis/PincerPDF
- Upstream baseline: PDFsam Basic `6.0.5-SNAPSHOT`
- Delivery model: Windows-first local verification with atomic checkpoints to `main`; Linux milestone/release compatibility evidence

## Completed

- Phase P0 development dossier accepted.
- Phase P1 reproducible Linux environment completed.
- Phase P2 baseline PDF engine capability spike completed.
- Phase P3 application shell and design system completed.
- Repository initialized and independent AGPL attribution established.
- Durable agent/development contract added.
- Exact Rust `1.97.1` toolchain and Linux devcontainer recipe defined.
- Dependency-free Rust workspace foundation created:
  - page number/range parsing and resolution,
  - explicit task-state transitions,
  - PDF engine capability/inspection port,
  - application capability validation,
  - atomic output-path planning,
  - minimal CLI doctor/selection commands.
- Structural verification can run without Cargo or network access.
- Narrow Linux GitHub Actions fallback established for relevant source changes and explicit validation PRs.
- Cached, path-scoped devcontainer verification lane established.
- Shared BuildKit cache scope established for the image verification, PDF capability, and application-shell workflows.
- Leptos CSR shell established with eight explicitly gated PDF workspaces.
- Tauri 2 single-window host established with only `core:default` capability.
- Responsive design tokens, visible focus, skip navigation, semantic landmarks, system/manual reduced motion, and stable test hooks established.
- CI-generated Cargo/npm locks and tracked application icon committed.
- P4.1 engine-independent Merge core and process-isolated QPDF adapter completed.
- P4.2 trusted Tauri command boundary, shared desktop DTOs, accessible Merge workspace, deterministic browser adapter, and Windows visual checkpoints implemented.
- P5.1 split planner, QPDF materializer, trusted Tauri commands and deterministic browser workspace implemented locally; the portable/native evidence gate is still open.
- P5.1 native command-boundary contract now covers three finalized outputs from registered path tokens and an explicit nested-bookmark depth request; Split desktop/compact screenshot checkpoints are wired into the browser acceptance suite.
- P5.1 materialization now reconstructs surviving source outline hierarchy for every output part, remaps page destinations through the exact ordered page vector, prunes dangling leaves, and verifies the rebuilt outline with QPDF before atomic finalization.
- P4.2 production WebView2 and real Windows system-dialog acceptance completed.

## Rust foundation evidence

GitHub Actions run `30424532988` (`Linux quality`, Ubuntu 24.04) completed successfully:

- pinned `rustc 1.97.1 (8bab26f4f 2026-07-14)`,
- `cargo fmt --all -- --check`,
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
- `cargo test --workspace --all-targets`: 13 passed, 0 failed,
- CLI `doctor` smoke test,
- CLI page-selection smoke test (`1-3,8` against 10 pages -> `1,2,3,8`),
- post-validation `git diff --exit-code`.

## Devcontainer evidence

GitHub Actions run `30425104784` completed the original image validation. Run `30427856581` repeated the image build and full repository gate after introducing the shared cache scope. Both completed successfully.

The image provides and verifies:

- `rustc 1.97.1`,
- `cargo 1.97.1`,
- `trunk 0.21.14`,
- Node `22.16.0`,
- QPDF `11.3.0`,
- MuPDF tools `1.21.1`,
- clean repository state after validation.

## PDF engine baseline evidence

GitHub Actions run `30427856572` (`PDF engine capability probe`) completed successfully:

- 32/32 external command measurements passed,
- deterministic PDF fixture generation and SHA-256 reporting,
- QPDF structure/page-count checks, extraction, assembly, rotation and AES-256 encryption,
- correct-password access and redacted wrong-password rejection,
- MuPDF inspection and rendering,
- QDF object-graph analysis of outlines and AcroForms,
- artifact `8714417168`, digest `sha256:a8b836838b12766bcf946905ae585420685309937a254abdae265aefa0810e6c`,
- post-probe clean-tree validation.

Measured semantic gaps are architectural contracts:

- page subset retained three bookmark nodes but produced one dangling destination,
- page assembly retained no outline tree,
- simple one-page AcroForm selection retained one field array and one widget.

ADR-013 assigns initial responsibilities: QPDF for capability-gated structural transformation/encryption, MuPDF for rendering/visual evidence, and PincerPDF for outline remapping/pruning/rebuilding. Form preservation remains provisional.

## P3 application-shell evidence

PR #5 repaired the incomplete validation gate left when PR #4 was merged before its shell checks were green.

GitHub Actions run `30442277577` (`Linux quality