# ADR-020: Drive Windows common dialogs through semantic controls

- Status: Accepted
- Date: 2026-07-31
- Depends on: ADR-016, ADR-017, ADR-018, ADR-019

## Context

P4.2 needs acceptance evidence for behavior that browser adapters and direct
Tauri command tests cannot prove: cancellation of a real Windows file picker,
multi-file selection, save-destination registration, the Windows overwrite
confirmation, safe conflict handling, and explicit replacement.

Screen-coordinate automation would be sensitive to resolution, DPI, window
position, theme, language and timing. The production WebView driver also does
not own operating-system dialogs.

## Decision

1. Keep system-dialog acceptance as a narrow layer on top of the optimized
   Tauri/WebView2 executable.
2. Drive Windows common dialogs with the pinned pywinauto Win32 backend.
3. Locate dialogs by the PincerPDF process and the standard `#32770` window
   class. Locate filename and command controls through semantic Win32 control
   IDs and button roles, never screen coordinates.
4. Handle localized overwrite confirmation through the standard command ID
   first and a bounded button-label fallback second. Retain the selected
   control in machine-readable evidence.
5. Install the exact Python test dependency set under the configurable
   development drive. Do not install it into the system Python environment.
6. Use deterministic PDF fixtures and QPDF to validate the final file instead
   of accepting a UI success message as sufficient evidence.
7. Prove both policies against the same existing destination: replacement is
   disabled by default and preserves the old bytes; explicit opt-in replaces
   them with a structurally valid six-page PDF.
8. Retain JSON evidence and a final native screenshot outside the repository.
9. Run this layer as a single WebDriver spec so process-name discovery cannot
   cross into another native-spec application instance, including teardown
   overlap.

## Accepted evidence

The Windows suite completed the following flow through real dialogs:

- cancelled a source picker and returned to a usable empty state;
- selected two three-page PDF fixtures in one Open dialog;
- selected the output through Save As and created a six-page PDF;
- reselected the existing destination and accepted Windows' localized
  overwrite confirmation;
- observed the application's default conflict without changing sentinel bytes;
- opted into replacement and produced a QPDF-valid six-page PDF.

The isolated test passed in 10.2 seconds. The retained evidence is:

```text
system-dialog-evidence.json
sha256=280fca04c83148aa0a08b1b389edb1c63b4d06d90406ddc5093137344c1791be

system-dialog-merge-completed.png
sha256=decad9c1abea9a269ebd2a944f2af456839de61590ecf31b0978625657c5bd3d

merged-from-system-dialog.pdf
sha256=3e41c7a08e2c2d2d830f8a7b0429e5dbcd504761bf0ff919f487c9b2095feef6
pages=6
```

## Consequences

- Native dialog regressions fail locally without weakening production
  permissions or replacing real filesystem behavior with test adapters.
- The runner currently targets Windows common-dialog semantics. Other platform
  dialogs require their own adapters and evidence.
- Button-label fallback is intentionally narrow and diagnostic. Unsupported
  localization fails with a semantic control inventory rather than clicking an
  arbitrary position.
- Dependency and UI-driver upgrades must rerun the complete flow.
- This runner does not retry failures; it isolates the single application
  process instead of masking process-selection races.

## Revisit triggers

Revisit when Windows changes common-dialog control semantics, Tauri exposes a
stable production-safe system-dialog automation hook, localization coverage is
expanded, or the runner is ported to another desktop platform.
