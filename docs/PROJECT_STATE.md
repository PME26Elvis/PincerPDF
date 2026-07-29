# Project State

- Updated: 2026-07-29
- Phase: P1 — Reproducible Linux environment (in progress)
- Repository: https://github.com/PME26Elvis/PincerPDF
- Upstream baseline: PDFsam Basic `6.0.5-SNAPSHOT`
- Delivery model: trunk-based, atomic checkpoints to `main`

## Completed

- Phase P0 development dossier accepted.
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

## Current validation lane

The present execution container has no Rust toolchain and cannot download one. A narrowly scoped GitHub Actions Linux quality lane now supplies the missing compiler environment. It runs only for relevant `main` changes or manual dispatch, uses the exact pinned Rust toolchain, and does not introduce a cross-platform matrix. Rust formatting, Clippy, tests and CLI smoke results remain pending until that workflow completes successfully.

## Evidence from this checkpoint

- `python3 scripts/verify-repo.py`: required file, TOML, workspace/lint/license/toolchain and source-policy checks.
- `bash -n scripts/bootstrap-check.sh scripts/check-fast.sh`: shell syntax validation.
- `python3 -m compileall scripts`: Python verifier syntax validation.
- Rust tests are included beside their behavior but remain unexecuted until the pinned toolchain is available.

## Exact next actions

1. Run the `Linux quality` GitHub Actions workflow on the current `main` checkpoint.
2. Fix every formatting, compiler, Clippy, test, lockfile and CLI-smoke finding until the lane is green.
3. Record the successful run and exact commit here, completing the Rust portion of P1.
4. Add the Tauri 2 + Leptos shell only after the foundation workspace is green.
5. Begin P2 with executable QPDF/MuPDF capability fixtures; do not choose a primary PDF engine by assumption.

## Completion status

P1 is not complete. A fresh container bootstrap and successful Rust quality gate are still required.
