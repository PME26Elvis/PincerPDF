# Project State

- Updated: 2026-07-29
- Phase: P3 — Application shell and design system (starting)
- Repository: https://github.com/PME26Elvis/PincerPDF
- Upstream baseline: PDFsam Basic `6.0.5-SNAPSHOT`
- Delivery model: trunk-based, atomic checkpoints to `main`

## Completed

- Phase P0 development dossier accepted.
- Phase P1 reproducible Linux environment completed.
- Phase P2 baseline PDF engine capability spike completed.
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
- Shared BuildKit cache scope established for the image verification and PDF capability workflows.

## Rust foundation evidence

GitHub Actions run `30424532988` (`Linux quality`, Ubuntu 24.04) completed successfully:

- pinned `rustc 1.97.1 (8bab26f4f 2026-07-14)`,
- `cargo fmt --all -- --check`,
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
- `cargo test --workspace --all-targets`: 13 passed, 0 failed,
- CLI `doctor` smoke test,
- CLI page-selection smoke test (`1-3,8` against 10 pages -> `1,2,3,8`),
- post-validation `git diff --exit-code`,
- dependency-free structural verification for five workspace members.

## Devcontainer evidence

GitHub Actions run `30425104784` completed the original image validation. Run `30427856581` repeated the image build and full repository gate after introducing the shared cache scope. Both completed successfully.

The image provides and verifies:

- `rustc 1.97.1`,
- `cargo 1.97.1`,
- `trunk 0.21.14`,
- Node `22.16.0`,
- QPDF `11.3.0`,
- MuPDF tools `1.21.1`,
- `make bootstrap-check`, formatting, Clippy and all 13 workspace tests,
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

Measured semantic gaps are now architectural contracts:

- page subset retained three bookmark nodes but produced one dangling destination,
- page assembly retained no outline tree,
- simple one-page AcroForm selection retained one field array and one widget.

ADR-013 assigns initial responsibilities: QPDF for capability-gated structural transformation/encryption, MuPDF for rendering/visual evidence, and PincerPDF for outline remapping/pruning/rebuilding. Form preservation remains provisional.

## Exact next actions

1. Squash-integrate PR #3 and retain the executable capability report workflow.
2. Scaffold the Leptos CSR application shell and Tauri 2 desktop host without faking PDF-tool completion.
3. Establish design/motion tokens, reduced-motion behavior and stable `data-testid` hooks.
4. Add deterministic browser shell E2E and the first Linux screenshot checkpoints.
5. Begin the Merge vertical slice only after the P3 shell exit gate is green.
6. Expand PDF fixtures continuously before granting broader engine capabilities.

## Completion status

P0, P1 and the P2 baseline are complete. P3 starts from reproducible Linux, Rust and PDF-engine evidence rather than assumed framework or engine behavior.
