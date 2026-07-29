# AGENTS.md — PincerPDF Development Contract

## Mission

Reimplement PDFsam Basic `6.0.5-SNAPSHOT` behavior in Rust as an independently branded, Linux-first desktop application. PDF semantic correctness, automated evidence and visual quality are mandatory; a visually similar shell is not parity.

## Binding rules

1. Read `docs/PROJECT_STATE.md`, `docs/ROADMAP.md` and accepted ADRs before work.
2. Primary GUI stack is Tauri 2 + Leptos CSR. Domain/application behavior belongs in Rust and must not depend on the WebView.
3. Develop and test primarily in the pinned Linux container. Windows/macOS work begins only after Linux RC approval.
4. Mature native PDF engines are allowed only behind `PdfEnginePort`; domain/application/UI code must not name concrete engines.
5. Each feature needs objective evidence: tests, PDF semantic/render checks and applicable visual checkpoints.
6. Motion uses semantic tokens and reduced-motion behavior. Do not add ad-hoc durations/easings/distances.
7. Never log PDF passwords, silently overwrite output or finalize directly into a partially written destination.
8. Durable source, project state and decisions go to Git. Large reports, screenshots, private fixtures and delivery bundles remain gitignored.
9. This repo uses trunk-based development: prefer small atomic commits to `main`; temporary branches must be short-lived and deleted after integration.
10. Do not add GitHub Actions merely to compensate for an unverified local workflow. Linux container validation comes first.
11. Do not claim a phase or feature complete while required traceability rows, tests, PDF evidence or visual checkpoints are missing.
12. Preserve AGPL obligations and upstream attribution; never imply official PDFsam affiliation.

## Required inner loop

1. State the requirement/checkpoint being advanced.
2. Add or update tests with the behavior.
3. Run the smallest relevant validation.
4. Run `make check-fast` when Rust is available and `make verify-structure` always.
5. Update `docs/PROJECT_STATE.md` with exact evidence and blockers.
6. Commit and push a recoverable atomic checkpoint.

## Definition of Done

See `docs/development/DEFINITION_OF_DONE.md`. A generated PDF, a green happy path or a plausible screenshot alone is never Done.
