# Project State

- Updated: 2026-07-29
- Phase: P2 — PDF engine capability spike (starting)
- Repository: https://github.com/PME26Elvis/PincerPDF
- Upstream baseline: PDFsam Basic `6.0.5-SNAPSHOT`
- Delivery model: trunk-based, atomic checkpoints to `main`

## Completed

- Phase P0 development dossier accepted.
- Phase P1 reproducible Linux environment completed.
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

GitHub Actions run `30425104784` (`Devcontainer verification`, Ubuntu 24.04) completed successfully:

- built the exact `.devcontainer/Dockerfile`,
- verified `rustc 1.97.1`, `cargo 1.97.1`, `trunk 0.21.14`, Node `22.16.0`, QPDF `11.3.0`, and MuPDF tools `1.21.1`,
- ran `make bootstrap-check` inside the image,
- reran formatting, Clippy and all 13 workspace tests inside the image,
- verified the repository remained clean after validation,
- exported BuildKit layers to the GitHub Actions cache for later image checks.

Local restricted-container checks also passed:

- `python3 scripts/verify-repo.py`,
- `bash -n scripts/bootstrap-check.sh scripts/check-fast.sh`,
- `python3 -m compileall scripts`,
- `git diff --check` for every repair checkpoint.

## Exact next actions

1. Squash-integrate PR #2 and retain its path-scoped devcontainer workflow.
2. Start P2 with executable PDF fixtures and QPDF/MuPDF capability probes.
3. Select engine responsibilities only from measured fixture evidence, not by assumption.
4. Scaffold the Tauri 2 + Leptos shell after the first capability report is reproducible.
5. Keep cross-platform packaging deferred until the Linux release-candidate gate.

## Completion status

P1 is complete. The pinned Linux development image and the dependency-free Rust foundation are both reproducibly verified.
