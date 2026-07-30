# ADR-016: Adopt Windows-first local development

- Status: Accepted
- Date: 2026-07-30
- Supersedes: ADR-001 delivery order

## Context

PincerPDF was initially planned around an ephemeral Debian container. The active development environment is now a persistent Windows workstation with Visual Studio Build Tools, WebView2/Chromium, and sufficient secondary-drive capacity. Rust, Cargo, build output and expandable frontend/PDF tool caches can live outside the system drive. Waiting for GitHub-hosted Linux jobs after every source edit would make the feedback loop slower without improving the correctness of platform-neutral code.

The existing Linux devcontainer and its evidence remain valuable. This decision changes execution priority, not the product's portability goals or quality threshold.

## Decision

1. Use Windows as the primary day-to-day implementation and verification platform.
2. Keep heavyweight local dependencies, package caches, temporary files, browser downloads and Cargo targets under a configurable non-system development root. The current workstation uses `D:\PincerPDF-dev`.
3. Require the Windows inner loop to cover Rust formatting, Clippy with warnings denied, all applicable workspace tests, native Tauri host compilation, Leptos/Trunk build, deterministic Chromium E2E and assigned Windows visual checkpoints.
4. Install pinned QPDF/MuPDF tools under the same development root and run engine contracts locally when the tool behavior is portable.
5. Retain the pinned Linux devcontainer as the compatibility oracle. Run Linux Actions for engine/process/filesystem boundary changes, milestone integration, release candidates, and explicit Linux visual/native evidence.
6. Defer macOS packaging and native validation until the first Windows release candidate.
7. Preserve platform-neutral domain/application layers and platform-specific baselines; Windows-first does not authorize Windows-only business logic.

## Consequences

- Most edit/compile/test cycles no longer wait for GitHub Actions.
- Windows-specific Tauri resources, paths and WebView behavior are detected earlier.
- Linux-specific permissions, process semantics and WebKitGTK rendering still require periodic container evidence.
- P1 Linux environment work remains valid historical and compatibility evidence.
- Screenshot baselines and native E2E evidence are platform-specific.
- The roadmap's first release-candidate gate becomes Windows; Linux is promoted from continuous compatibility evidence during the following release-expansion phase.

## Evidence required

- A pinned Rust/WASM/Trunk toolchain executes from the configured development root.
- The complete Windows workspace quality gate passes with no untracked generated source.
- Native Windows Tauri compilation and browser E2E pass locally.
- QPDF/MuPDF identities and real-engine contracts are captured after their local installation.
- Linux milestone workflows remain green before a Windows release candidate is accepted.

## Revisit triggers

Revisit if native Windows automation proves unreliable, required PDF tools cannot be pinned safely, the workstation cannot support deterministic visual evidence, or Windows-first changes begin degrading mandatory Linux compatibility.
