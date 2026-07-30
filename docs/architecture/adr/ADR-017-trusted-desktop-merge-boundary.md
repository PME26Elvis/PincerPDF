# ADR-017: Keep Merge paths and execution behind a trusted desktop boundary

- Status: Accepted
- Date: 2026-07-30
- Owners: PincerPDF maintainers

## Context

P4.2 makes Merge available in the Leptos workspace. The WebView needs to select files, edit an ordered plan, start work and request cancellation, but it must not become the authority for arbitrary filesystem paths or execute QPDF directly. Browser E2E also needs a deterministic adapter without pretending that it verifies native dialogs or the PDF engine.

## Decision

1. Keep native dialogs, path resolution, QPDF discovery and Merge execution in the Tauri host.
2. Return session-scoped opaque path tokens to the UI. Display paths are presentation data only and are never accepted as filesystem authority.
3. Share explicit Serde DTOs through `pincerpdf-desktop-api`; password-bearing request types deliberately omit `Debug`.
4. Run blocking engine work outside the WebView event loop and track each active operation with a validated identifier and cooperative cancellation token.
5. Reject a duplicate active operation identifier without replacing or cancelling the original token.
6. Keep the P4.2 existing-output policy on `Fail`. Windows replacement remains unavailable until a durable atomic replacement strategy and recovery tests exist.
7. Use a deterministic in-browser adapter only for UI state, accessibility, motion and screenshot tests. It cannot satisfy native or engine acceptance.
8. Require three independent evidence layers before treating the slice as usable:
   - Rust tests for DTOs, opaque token resolution, task conflicts and cancellation;
   - an ignored real-engine contract that crosses the desktop command boundary using registered tokens;
   - a launched Tauri/WebView2 smoke check plus browser E2E and visual checkpoints.

## Consequences

- The UI cannot submit a newly invented local path to Merge.
- Native file access remains auditable in one host module.
- Browser tests stay fast and deterministic while their limits remain explicit.
- The host owns cleanup and cancellation lifecycle, including conflict-safe task registration.
- Session token storage is intentionally in-memory and is discarded when the app exits.
- A complete native dialog-driven E2E remains a separate acceptance item; a browser adapter is not evidence for it.

## Revisit

Revisit when persistent workspaces need path reauthorization, multiple windows share tasks, Windows atomic replacement is proven, or the native automation layer can reliably exercise system dialogs across supported platforms.
