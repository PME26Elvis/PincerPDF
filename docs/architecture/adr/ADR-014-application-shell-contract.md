# ADR-014: Establish the application-shell contract

- Status: Accepted
- Date: 2026-07-29

## Context

PincerPDF needs a professional desktop shell before tool vertical slices begin, but a polished interface must not imply that PDF operations are already available. The shell also needs deterministic browser evidence even though the shipping host is Tauri.

## Decision

- Use Leptos `0.8.20` in CSR mode with Trunk for the presentation layer.
- Use Tauri `2.11.5` with a single Linux-first window and a `core:default` capability only.
- Expose all eight planned PDF tools, each explicitly labelled **Not implemented** until its own capability and parity gates pass.
- Keep the primary file-action control disabled in P3.
- Provide semantic landmarks, a skip link, visible focus treatment, stable `data-testid` hooks, and both system and manual reduced-motion policies.
- Verify the same CSR shell in Chromium with Playwright and retain desktop/compact screenshots as workflow evidence.
- Keep framework types inside presentation and composition-root packages.

## Consequences

- P4 can begin from a stable shell without redesigning navigation, motion, accessibility, or host composition.
- Browser tests validate deterministic interaction contracts; native runtime behavior still requires Tauri-specific checks in later phases.
- The shell is intentionally useful for orientation but cannot execute PDF tasks yet.
- New PDF functionality must replace a visible gate rather than quietly wiring behavior behind a planned control.

## Revisit triggers

Revisit this decision if accessibility testing, native WebView behavior, security constraints, or measured rendering performance show that the shell contract cannot support the Linux release-candidate gate.
