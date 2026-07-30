# Project State

- Updated: 2026-07-30
- Phase: P4 — Merge vertical slice (P4.2 native-dialog acceptance)
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

## P4.1 Merge-core evidence

PR #6 established the first executable PDF-tool vertical slice while intentionally keeping the desktop Merge capability gate closed.

GitHub Actions run `30511384211` (`Linux quality`) completed successfully on head `0f00b8373b911bf38634c9f757813390a55f8559`:

- Rust formatting and Clippy passed with warnings denied,
- 21 portable Rust tests passed,
- the real-engine contract remained intentionally ignored in the portable lane,
- three evidence-summarizer regression tests passed,
- repository structure, CLI smoke tests and clean-tree validation passed.

GitHub Actions run `30511384208` (`Merge core`) completed successfully in the pinned Linux devcontainer:

- the same portable workspace checks passed,
- the ignored real-QPDF/MuPDF Merge contract passed,
- source order and deliberate page duplicates were preserved,
- AcroForm input was rejected before Merge,
- encrypted input completed without fixture passwords appearing in retained evidence,
- temporary sibling output passed semantic inspection before atomic finalization,
- QPDF `11.3.0` and MuPDF `1.21.1` identities were captured across their actual stdout/stderr behavior,
- the committed lock remained unchanged.

The ordered output contains five pages:

```text
pdf_sha256=69aae958278cdf7097f71a1e9565d77d21d846620e7c4b7de0088cf1fbb87211
text_sha256=86eea69463577647169ddccef56ed5eeccec6ee2e2920409a794b735bf8a19d6
```

The encrypted-input output contains six pages:

```text
pdf_sha256=18db80d8cde214812bb83e557862e8f861ee21129d1ba70a46e902d37ec25b7e
text_sha256=a031c751b8eebb99dd75f2060e53a72ddf3854c7d034792f851d4fa81e3da3b0
```

Artifact `8747323877` has digest `sha256:6a3cff1e05b27e9ff0e545d65b53faf5be20de794404c8b70c33f4c995e07670`.

Runs `30511384218` (`PDF engine capability probe`) and `30511384187` (`Application shell`) also completed successfully on the same head, confirming the 32-command engine baseline and the P3 Tauri/Leptos/Playwright shell remained intact.

## Windows-first local development evidence

ADR-016 supersedes the original Linux-first delivery order without weakening the cross-platform product requirement. The primary edit/build/test loop now runs locally on Windows, with large tools, caches, temporary files, browser artifacts and Cargo targets rooted under the configurable development directory. The current workstation uses `D:\PincerPDF-dev`.

The local environment has verified:

- pinned Rust/Cargo `1.97.1`, Rustfmt, Clippy and the `wasm32-unknown-unknown` target,
- Trunk `0.21.14`, the bundled Node `24.14.0`, Playwright `1.62.0` and an installed Chromium-compatible browser,
- official QPDF `11.3.0` and official MuPDF tools `1.21.0` under the same non-system drive,
- `cargo fmt`, warning-denied workspace Clippy, repository structure validation and all 21 portable Rust tests,
- native Windows Tauri host compilation and Leptos CSR release build,
- all five deterministic application-shell Chromium E2E scenarios and desktop/compact screenshots,
- the 32-command PDF capability probe,
- the ignored real-engine Merge contract: 1 passed, 0 failed.

The Windows Merge evidence contains five ordered pages and six encrypted-input pages. PDF byte hashes differ from the Linux artifacts because they were generated on a different tool/platform run, while the canonical extracted-text hashes now match Linux exactly:

```text
ordered text_sha256=86eea69463577647169ddccef56ed5eeccec6ee2e2920409a794b735bf8a19d6
encrypted text_sha256=a031c751b8eebb99dd75f2060e53a72ddf3854c7d034792f851d4fa81e3da3b0
```

The evidence summarizer now writes canonical LF UTF-8 bytes so host newline policy cannot create false semantic differences. Windows validation also exposed and fixed platform assumptions around Unix-only temporary-directory modes, cancellation-test signals, durable file handles and directory syncing. The development bootstrap loads Visual Studio before prepending the pinned D-drive tools, preventing compiler setup from silently hiding QPDF, MuPDF, Cargo or Trunk.

Linux remains the compatibility oracle for process/filesystem boundaries, WebKitGTK rendering, milestone integration and release evidence. MuPDF is `1.21.0` locally because the official `1.21.1` Windows release was source-only; Linux evidence remains pinned to `1.21.1`.

## P4.2 Merge-desktop evidence

The Windows-first P4.2 implementation now crosses four independently verified layers:

- shared Serde DTOs keep password-bearing requests out of `Debug`,
- the Tauri host owns native file dialogs, opaque path registration, QPDF discovery, blocking work and cooperative cancellation,
- the Leptos UI exposes ordered source rows, page-selection validation, duplication/removal/reordering, destination choice, explicit bookmark/form/conflict policy, progress, cancellation and result states,
- the browser adapter supplies deterministic fixtures only for UI, accessibility, motion and screenshot evidence.

The complete local portable gate passes with warning-denied Clippy and 23 Rust tests. Two real-engine tests remain ignored in the portable lane and pass when explicitly supplied with the pinned local QPDF/MuPDF tools and generated fixtures:

- P4.1 Merge engine contract: 1 passed;
- P4.2 desktop command-boundary contract: 1 passed.

The desktop contract registers source and destination paths inside `DesktopState`, submits only opaque tokens, preserves the requested `3,1` plus repeated-source page `2` order, and produces a verified three-page PDF:

```text
sha256=2b6b5ee06e034119456ee169f50b85c35cb22343adc62e0573b54ea3114518d9
```

The P4.2 Playwright suite passes 8/8 scenarios in 23.4 seconds. It verifies the single available Merge tool and seven independent gates, ordered planning, page-range errors, duplication, destination selection, running/completed states, advanced safety policy, reduced motion, keyboard skip navigation, and four Windows visual checkpoints:

```text
merge-empty-desktop.png       1440 x 1121
merge-configured-desktop.png  1440 x 1264
merge-completed-desktop.png   1440 x 1264
merge-completed-compact.png    390 x 3110
```

The real Windows Tauri/WebView2 executable also launches successfully from the D-drive toolchain. Its accessibility tree exposes the landmarks and controls, reports `qpdf version 11.3.0`, and keeps the action disabled in the empty state. A complete system-dialog-driven UI automation run is still pending; the browser adapter and direct command contract do not substitute for that acceptance item. ADR-017 records this boundary.

### Production WebView2 acceptance

ADR-018 adds a fast native layer between deterministic browser tests and future system-dialog automation. It builds the optimized desktop executable with Tauri's production custom protocol and drives the actual embedded WebView2 through the official external `tauri-driver`.

The first probe found and fixed two release-only defects:

- a direct release build still opened `http://127.0.0.1:1420` unless `tauri/custom-protocol` was enabled;
- the production CSP blocked the same-origin WASM fetch because `connect-src` omitted `'self'`.

The corrected run opened `http://tauri.localhost/`, loaded the JavaScript/WASM, exposed `window.__TAURI__`, and crossed the real `merge_engine_status` and `cancel_merge` command bridge. The suite passed 3/3 scenarios in 2.6–4.8 seconds of test time and nine seconds end to end:

- QPDF `11.3.0` discovery;
- eight tool entries, one available Merge workspace and seven independent gates;
- disabled empty-state execution, unknown-operation cancellation, manual reduced motion and native screenshot capture.

The production-like binary still has only `core:default`; no WDIO Rust plugin, guest script or `wdio:*` capability is shipped. The retained native screenshot is 3600 × 2110:

```text
sha256=274ed135b05f0e2b346276c77c7e4da5ba998e6b6d3484340ffae411f6583875
```

The exact runner dependency graph includes compatibility/security overrides for the current Tauri service packaging gap and patched transitive packages. Both npm and pnpm audits report zero known vulnerabilities, and the native suite remains green after the overrides.

### P4.2 Linux compatibility evidence

PR #8 was squash-merged to `main` as `30281395a5aee6016df5f8b493c2de435338cb2f` after every required check passed on the exact source head `505978bc62d0133e19e2da0ea29d2c91b5e5808a`:

- Linux quality run `30521001566`;
- Merge core run `30521001569`;
- PDF engine capability probe run `30521001582`;
- Application shell run `30521001596`.

The retained artifacts are:

```text
PDF engine capability evidence
artifact=8750889460
sha256=7d631671a4801a76e60914fd90480aad0541c5d7eadb413963a8f5a687c8d5a3

Merge core evidence
artifact=8751044520
sha256=34cbdaed2e7c55ac4fddf47a7debda9be69463778db6f3b9f2864be804956b95

Application shell evidence
artifact=8751048133
sha256=179ade86c57b298a8f3922b8f32b8ff9159a5ad784dc179914fe4a7ef0eaa6b9
```

This closes the P4.2 Linux milestone lane without restoring Actions as the ordinary edit/build/test loop.

## Active known boundary

`ExistingOutputPolicy::Replace` is not yet accepted as a durable Windows behavior because `std::fs::rename` does not replace an existing destination there. P4.2 must keep conflict handling on `Fail` until an atomic Windows replacement implementation and recovery tests pass; removing the destination before rename is not an acceptable substitute.

## Exact next actions

1. Complete the Windows system-dialog-driven Merge E2E with real fixtures, output verification, cancellation and conflict recovery; the production WebView2 layer is already green.
2. Implement and verify durable Windows replacement or keep overwrite visibly unavailable.
3. Extend the Merge corpus for mixed page boxes/rotation, metadata policy, bookmarks, encrypted inspection and long/Unicode paths.
4. Map the remaining Merge legacy-test rows and close functional-parity gaps before P4 exit.
5. Keep all other seven tools visibly gated while P4 continues.

## Completion status

P0 through P3 and the P4.1 Merge core are complete. P4 remains in progress. P4.2 implementation, portable/native-command tests, browser E2E, Windows visual checkpoints, production-protocol WebView2 E2E and Linux milestone evidence are green; native system-dialog automation and durable Windows replacement remain before the checkpoint is complete.
