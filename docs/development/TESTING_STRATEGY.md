# Master Test Strategy

## Layered model

| Layer | Purpose | Primary tools | Gate |
| --- | --- | --- | --- |
| L0 Static | Formatting, lint, unsafe/dependency/license policy | rustfmt, Clippy, cargo-deny/audit | Every local checkpoint |
| L1 Unit/property | Domain values, planners, state machines, naming, migrations | cargo test, proptest | Every feature commit |
| L2 Adapter/contract | Filesystem, persistence, engine argument/result mapping | Rust integration tests, fake engines | Before vertical slice merge |
| L3 PDF differential | Legacy oracle vs Rust/engine semantic and rendered output | qpdf/MuPDF tools, custom comparator | Required per tool |
| L4 Browser UI E2E | Leptos user workflows with deterministic platform adapters | Playwright | Every UI vertical slice |
| L5 Native desktop E2E | Tauri commands/events, real filesystem and platform ports | Windows local driver first; Linux tauri-driver/WebDriverIO + Xvfb at compatibility gates | Boundary flows and RC |
| L6 Visual/motion | Platform-specific static baselines, checkpoint submissions and motion frames | Playwright screenshots, native capture, ffmpeg, Vision review | Per milestone/RC |
| L7 Non-functional | Performance, stress, fuzz, mutation, accessibility, security | criterion/hyperfine, cargo-fuzz, mutation tool, audits | Hardening/RC |

## Legacy-test migration

The workbook contains all 229 upstream test files and 907 detected test methods. Each row receives a target Rust test ID/layer and disposition. `NoHeadless` tests are not skipped; they are candidates for browser component tests, native Windows E2E, packaged Linux compatibility E2E or visual checkpoints.

## Test naming

Use feature IDs and intent, e.g. `MERGE-003_bookmark_policy_retain_as_one_entry`. E2E IDs and screenshot IDs are stable and referenced from feature rows.

## Evidence

A passing command is accompanied by machine-readable reports, exact source commit, tool/engine versions and fixture manifest. Screenshot/motion evidence names the checkpoint ID and environment fingerprint.

## Platform order

The ordinary inner loop runs locally on Windows: portable Rust checks, native Tauri compilation, Leptos/Trunk builds, Chromium E2E and Windows visual checkpoints. QPDF/MuPDF contracts run locally once their pinned tools are installed under the configured development drive.

Linux remains mandatory evidence for engine-boundary changes, milestone integration, release candidates and any change touching filesystem/process/WebView assumptions. GitHub Actions is an auxiliary compatibility lane rather than the default compiler.

## Flake policy

Tests are never retried silently to create green status. A flaky test is quarantined only with an issue, owner, reason and expiration; its feature cannot be marked Verified if the quarantined test is the only acceptance evidence.
