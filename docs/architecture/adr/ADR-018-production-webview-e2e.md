# ADR-018: Verify the production WebView without shipping test permissions

- Status: Accepted
- Date: 2026-07-30
- Depends on: ADR-014, ADR-016, ADR-017

## Context

Browser Playwright tests validate the Leptos interaction model with deterministic platform adapters, and the Rust desktop contract validates the real Merge command boundary. Neither proves that the release executable loads its embedded assets, that WebView2 accepts the production CSP, or that the global Tauri bridge reaches the native host.

The first Windows WebDriver probe exposed two production-only defects:

1. a direct `cargo build --release` used `devUrl` and opened `http://127.0.0.1:1420` unless Tauri's production custom protocol feature was enabled;
2. the embedded HTML loaded, but `connect-src` omitted `'self'`, so WebView2 blocked the same-origin WASM fetch and Leptos never mounted.

The current `@wdio/tauri-service` also expects its optional test plugin for advanced APIs and window-focus discovery. Adding that plugin and its execute permissions to a release-like binary would make the acceptance target less representative and increase its command surface.

## Decision

1. Run the Windows native shell suite against the optimized release binary built with `tauri/custom-protocol`.
2. Permit same-origin asset fetches in the production CSP while retaining the narrow Tauri IPC origins.
3. Use WebdriverIO's official external `tauri-driver` provider with EdgeDriver/WebView2.
4. Do not compile `tauri-plugin-wdio`, import its guest script, or grant `wdio:*` permissions in the production-like binary.
5. Use basic WebDriver script round-trips for DOM state/events and `window.__TAURI__.core.invoke` for the real application command bridge. Do not claim the unavailable WDIO mocking/log-forwarding APIs are covered.
6. Retain startup HTML, a machine-readable environment/resource report, browser security logs, and a native screenshot.
7. Keep native system-dialog automation as a separate acceptance layer. A production WebView test does not prove Windows file-picker behavior.
8. Pin all runner dependencies exactly. Override `@wdio/native-utils` to `2.5.0` because service `1.2.0` imports an API absent from its declared `2.4.0`. Pin the audited `brace-expansion`, `minimatch`, and `serialize-javascript` pair until upstream dependency ranges include patched releases.

## Consequences

- Release asset embedding, CSP, WebView2 rendering and the real Tauri IPC boundary fail in one local test when they regress.
- The acceptance executable has the same application permissions as the intended production binary.
- The suite cannot use `browser.tauri.mock`, backend log forwarding, or test-plugin window introspection.
- DOM interactions are grouped into a few WebDriver script round-trips because service `1.2.0` otherwise waits for the absent test plugin before every focus-sensitive element command.
- Dependency overrides are compatibility and security contracts. Upgrades must rerun the native suite and a registry audit before removing or changing them.

## Accepted evidence

The Windows release build opened `http://tauri.localhost/`, loaded the Leptos JavaScript/WASM, exposed `window.__TAURI__`, and invoked `merge_engine_status` through the real host. The suite passed 3/3 scenarios:

- QPDF `11.3.0` discovery through the native bridge;
- one available Merge workspace with seven independent tool gates;
- disabled empty-state execution, cancellation of an unknown operation, manual reduced motion, and native screenshot capture.

The final runner completed in nine seconds. `pnpm audit --audit-level moderate` reported no known vulnerabilities. The native screenshot is 3600 × 2110 with SHA-256:

```text
274ed135b05f0e2b346276c77c7e4da5ba998e6b6d3484340ffae411f6583875
```

## Revisit triggers

Revisit when the Tauri/WebdriverIO packages make plugin-free focus discovery constant-time, when a test-only feature-gated plugin binary is needed for APIs that basic WebDriver cannot cover, when Edge/WebView2 changes asset security behavior, or when Windows system-dialog automation is promoted into this runner.
