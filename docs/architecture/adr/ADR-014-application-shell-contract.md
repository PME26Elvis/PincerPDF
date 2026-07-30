# ADR-014: Establish the application-shell contract

- Status: Accepted
- Date: 2026-07-29

## Context

PincerPDF needs a professional desktop shell before tool vertical slices begin, but a polished interface must not imply that PDF operations are already available. The shell also needs deterministic browser evidence even though the shipping host is Tauri.

The pinned Trunk `0.21.14` toolchain downloads `wasm-opt version_123`. Measured CI evidence showed that the Rust `1.97.1` release WASM compiled successfully, but this post-link optimizer rejected its bulk-memory operations because the downloaded validator was not invoked with the required feature support.

## Decision

- Use Leptos `0.8.20` in CSR mode with Trunk for the presentation layer.
- Use Tauri `2.11.5` with a single cross-platform window and a `core:default` capability only.
- Expose all eight planned PDF tools, each explicitly labelled **Not implemented** until its own capability and parity gates pass.
- Keep the primary file-action control disabled in P3.
- Provide semantic landmarks, a skip link, visible focus treatment, stable `data-testid` hooks, and both system and manual reduced-motion policies.
- Verify the same CSR shell in Chromium with Playwright and retain desktop/compact screenshots as workflow evidence.
- Keep framework types inside presentation and composition-root packages.
- Set `data-wasm-opt="0"` on the Trunk Rust asset. Keep Cargo release optimization and thin LTO enabled; do not run the incompatible Binaryen post-link pass until a pinned replacement is measured successfully.

## Consequences

- P4 can begin from a stable shell without redesigning navigation, motion, accessibility, or host composition.
- Browser tests validate deterministic interaction contracts; native runtime behavior still requires Tauri-specific checks in later phases.
- The shell is intentionally useful for orientation but cannot execute PDF tasks yet.
- New PDF functionality must replace a visible gate rather than quietly wiring behavior behind a planned control.
- Release WASM is larger than it might be after a compatible post-link optimizer, but correctness and reproducibility take precedence over a premature size optimization.
- A future Trunk/Binaryen upgrade is a measured build-tool change and must rerun release-build and E2E evidence before re-enabling wasm optimization.

## Evidence

GitHub Actions run `30442275280` passed complete workspace checks, Tauri host compilation, Leptos CSR release build, five Chromium Playwright tests, desktop/compact screenshot capture, and clean-tree validation. Artifact `8720198828` has digest `sha256:29f4870e1a4232573eb8138544875c9ebc6c02f86d87d821d24a40b13417f193`.

## Revisit triggers

Revisit this decision if accessibility testing, native WebView behavior, security constraints, measured rendering performance, WASM size budgets, or a newer pinned Trunk/Binaryen combination show that the shell contract should change before the Windows release-candidate gate.
