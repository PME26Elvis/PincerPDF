# Project State

- Updated: 2026-07-29
- Phase: P4 — Merge vertical slice (starting)
- Repository: https://github.com/PME26Elvis/PincerPDF
- Upstream baseline: PDFsam Basic `6.0.5-SNAPSHOT`
- Delivery model: trunk-based, atomic checkpoints to `main`

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

GitHub Actions run `30442277577` (`Linux quality`) completed successfully on the repaired P3 head:

- dependency and shell structural contracts passed,
- portable workspace formatting and Clippy passed with warnings denied,
- all 13 Rust tests passed,
- CLI doctor and selection smoke tests passed,
- committed lockfiles remained clean.

GitHub Actions run `30442275280` (`Application shell`) completed successfully in the pinned Linux devcontainer:

- complete workspace formatting, Clippy, and tests passed,
- the native Tauri 2 host compiled with the tracked icon and least-privilege capability,
- the Leptos CSR release build completed,
- five Chromium Playwright tests passed in 42.5 seconds,
- all eight tool entries remained explicitly **Not implemented**,
- workspace selection, manual reduced motion, keyboard skip navigation, and screenshot capture passed,
- clean-tree verification passed.

Workflow artifact `8720198828` has digest `sha256:29f4870e1a4232573eb8138544875c9ebc6c02f86d87d821d24a40b13417f193` and contains:

- release HTML/CSS/JavaScript/WASM output,
- Playwright HTML report,
- committed Cargo/npm lock evidence,
- `application-shell-desktop.png` at 1440 × 1278,
- `application-shell-compact.png` at 390 × 3199.

Trunk `0.21.14` downloads `wasm-opt version_123`, which rejected the Rust `1.97.1` bulk-memory output despite the WASM release compilation succeeding. P3 therefore explicitly disables that incompatible post-link pass with `data-wasm-opt="0"`; Cargo release optimization and thin LTO remain enabled. ADR-014 records this measured compatibility decision.

## Exact next actions

1. Squash-integrate PR #5 after the final evidence-state checks pass.
2. Start P4 with a real Merge request/domain/application vertical slice behind the existing capability gate.
3. Implement a process-isolated QPDF merge adapter with redacted evidence, bounded output, timeout/cancellation, and atomic finalization.
4. Add merge fixture contracts for page order, duplicates, mixed page boxes/rotation, metadata, encryption, forms, and bookmark policy.
5. Replace only the Merge UI gate after engine, application, E2E, and parity evidence pass.
6. Keep all other seven tools visibly gated.

## Completion status

P0 through P3 are complete. P4 starts from a reproducible Linux environment, measured PDF-engine responsibility split, and a verified Tauri/Leptos application shell rather than an untested UI scaffold.
