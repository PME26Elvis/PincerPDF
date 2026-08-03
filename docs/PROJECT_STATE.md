# Project State

- Updated: 2026-08-03
- Phase: P5 — Split family (P5.2 metadata safety and parity hardening)
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
- P5.1 split planner, QPDF materializer, trusted Tauri commands and deterministic browser workspace implemented and evidence-verified.
- P5.1 native command-boundary contract now covers three finalized outputs from registered path tokens and an explicit nested-bookmark depth request; Split desktop/compact screenshot checkpoints are wired into the browser acceptance suite.
- P5.1 materialization now reconstructs surviving source outline hierarchy for every output part, remaps page destinations through the exact ordered page vector, prunes dangling leaves, and verifies the rebuilt outline with QPDF before atomic finalization.
- P5.1 split evidence is green on the current PR head: native command-boundary outputs, nested bookmark depth, reconstructed outline destinations, Linux portable checks, and desktop/compact screenshot baselines all passed.
- Split outline reconstruction now fails closed with `capability_unavailable` for named destinations, unresolved/action-backed entries, and ambiguous duplicate-page destination identity; Merge's documented first-occurrence duplicate policy is unchanged.
- Split bookmark materialization now carries QPDF expansion state, writes descendant-correct `/Count` values including the negative collapsed form, and has Rust unit coverage for exact Unicode titles and closed nested outlines. Real-engine Unicode/style fixtures remain a separate gate.
- P4.2 production WebView2 and real Windows system-dialog acceptance completed.

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

The bootstrap now probes the D-drive target/temp directories before use and falls
back to the repository `target-local`/`tmp` pair when Windows denies those
directories. This keeps local Trunk, Cargo and Playwright runs reproducible even
when the configured secondary-drive folders are read-only. The filename
table-of-contents browser scenario passed 1/1 against the locally generated
Leptos bundle served from the deterministic static test server.

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

The real Windows Tauri/WebView2 executable also launches successfully from the D-drive toolchain. Its accessibility tree exposes the landmarks and controls, reports `qpdf version 11.3.0`, and keeps the action disabled in the empty state. ADR-017 records the trusted command boundary.

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

## Windows replacement boundary

ADR-019 corrects the earlier conservative assumption about the pinned standard library: Rust 1.97.1 documents `std::fs::rename` as replacing an existing file on Windows through `FileRenameInfoEx` or `MoveFileExW`. The local Windows suite now proves successful replacement, late-conflict preservation, locked-destination failure, preservation of the original locked bytes, and temporary-file cleanup. PincerPDF never deletes the destination before rename.

The replacement is accepted as an atomic namespace operation after the temporary PDF has been flushed and semantically verified. Filesystem-level directory-entry durability across sudden power loss remains explicitly OS/filesystem dependent rather than overclaimed.

## Windows system-dialog acceptance

ADR-020 closes the P4.2 boundary with a coordinate-free pywinauto Win32 driver against the optimized Tauri/WebView2 executable. The exact Python dependencies live under the configured D-drive development root rather than the system environment.

The native flow passed through real Windows dialogs and:

- cancelled a source picker without mutating the empty state;
- selected two deterministic three-page PDFs in one Open dialog;
- registered an output through Save As and produced a QPDF-verified six-page PDF;
- reselected the existing destination and handled the localized Windows overwrite confirmation;
- proved the default application conflict preserved sentinel bytes;
- enabled replacement explicitly and produced a new QPDF-valid six-page PDF.

The isolated system-dialog scenario passed in 10.2 seconds and the one-worker run completed in 17 seconds. Retained evidence:

```text
system-dialog-evidence.json
sha256=280fca04c83148aa0a08b1b389edb1c63b4d06d90406ddc5093137344c1791be

system-dialog-merge-completed.png
sha256=decad9c1abea9a269ebd2a944f2af456839de61590ecf31b0978625657c5bd3d

merged-from-system-dialog.pdf
sha256=3e41c7a08e2c2d2d830f8a7b0429e5dbcd504761bf0ff919f487c9b2095feef6
pages=6
```

## P4.3 Merge parity-corpus evidence

The first P4.3 corpus slice is executable locally and tracked in
`docs/compatibility/MERGE_TRACEABILITY.md`. A deterministic fourth fixture adds
source Info metadata and three deliberately different page geometries.

The real QPDF/MuPDF contract now also:

- reads the fixture through a Unicode long source path;
- writes the final PDF through a different Unicode long output path;
- selects the three geometry pages in `2,1,3` order and appends pages `2-`
  through the last page of a second source;
- checks exact MediaBox, CropBox and rotation values on all five output pages;
- checks page-origin text order and QPDF structural validity;
- confirms the current safe baseline does not implicitly copy one source's
  Info dictionary to the merged document.

The Windows contract passed as one ignored real-engine test in 3.71 seconds.
The parity output contains five pages:

```text
fixture_sha256=78f437df1567432560bef76d11f7baa2766224016a7f6999e493738f7389434c
pdf_sha256=789f36bc6ad1eb8576325af7f4f2ff88a2a316b1d20d01370bab08fb4451d953
text_sha256=5b4feb5a1205192b7037be8b789201356ddd9d7ce19801f3ddba394dd57dabe2
report_sha256=ae38b46d4e2ace13d61fa74e67c2616cc3452141c3c092ff7b4b9064056c5784
```

MERGE-001, MERGE-002 and the baseline of MERGE-009 are verified. MERGE-003,
MERGE-004 and MERGE-008 remain partial, and MERGE-005 through MERGE-007 remain
planned; the ledger does not overstate full Merge parity.

PR #11 was squash-merged to `main` as
`5d665571cf82c7f0c48d72a62f825f23a937c21a` after every relevant check passed
on exact source head `11d46ce2d2ca3df8a0cf5e9e2f87ee1f4ed7d675`:

- Linux quality run `30603683715`;
- Merge core run `30603683704`;
- PDF engine capability probe run `30603675535`; and
- Application shell run `30603675531`.

This closes the P4.3 Windows/Linux parity-corpus checkpoint.

## P4.4 document-level bookmark evidence (integrated)

ADR-021 accepted the first non-discard Merge bookmark policy. The shared DTO,
Leptos workspace, trusted desktop command, application service and QPDF adapter
now carry an explicit choice between `Discard` and `OneEntryPerDocument`;
discard remains the default.

For the document-level policy, PincerPDF assembles pages without outlines,
reads the actual output page and catalog object references through bounded QPDF
JSON v2, adds a new outline tree through a private JSON update, structurally
checks the result, and then requires the high-level outline titles and output
page positions to match the application plan. The updater preserves the full
catalog and deliberately omits the trailer.

Local Windows evidence is green:

- warning-denied workspace Clippy;
- 29 portable Rust tests passed, with two real-engine tests intentionally
  ignored in that lane;
- the real QPDF/MuPDF Merge contract passed, including the prior ordering,
  encryption, form rejection and geometry corpus;
- the real native desktop command-boundary contract passed with two generated
  entries;
- 9/9 deterministic browser E2E scenarios passed; and
- 8 Python structure/evidence tests passed.

The dedicated one-entry visual checkpoint was inspected at full resolution.
The selected state, descriptions and policy summary remain aligned without
clipping, overlap or unintended horizontal scrolling:

```text
merge-bookmark-policy-desktop.png
dimensions=1440x1564
sha256=d648eaf41a69d7faf6415add7338925e5b2a173d9b1a173317b7b28044315918
```

The three-page real-engine bookmark output contains:

```text
plain-three-pages.pdf -> output page 1
bookmarks.pdf         -> output page 2

pdf_sha256=3f2690fa6e37e0f9834d1e1631337d51393a52b422ca0d308ab30276744e8524
text_sha256=0d7f2ba5be2117774902f4098c27e8a795a2b02beb711b7a7ac8b5be60534b9e
report_sha256=72f79d66014234da2beece4742ace007206ee5d21ac35660d0dca026752ecdf7
```

The trusted native command-boundary output uses the same source twice with
different selections and proves duplicate document titles remain distinct at
output pages 1 and 3:

```text
pdf_sha256=9f7f991982dd5599071c513ddc1ab5b33cdd158263759833a246647164d2b558
```

The optimized production-protocol Tauri/WebView2 suite passed 4/4 scenarios,
including both explicit bookmark modes. Its retained native screenshot:

```text
sha256=24394ee20a7e5c496b9914040d2d5e09416c606609c34496d13edeb0a82996cd
```

The coordinate-free real Windows dialog flow also passed. It selected the
complete plain and bookmark fixtures, produced six pages, verified document
entries at pages 1 and 4, preserved sentinel bytes under the default conflict
policy, and then atomically replaced them after explicit opt-in:

```text
system-dialog-evidence.json
sha256=6a4304d82675001d49065748a5cbf6252a651b4e0e558fbf7e216d7b54d774e9

system-dialog-merge-completed.png
sha256=128dbf45623f9f01bf7c4209b03d8e12566806de6dc5ebe67eb75a0e823db2ce

merged-from-system-dialog.pdf
sha256=bc1bf55202027b9ed3049b8f95e5be721ebb4a175ca7556c72aef50242f627de
pages=6
```

P4.4 was squash-integrated as `main@7a5f49e034a9098eae0794d6e38f7b751506a0ee`
after its Linux quality, Merge core and application-shell workflows succeeded.

## P4.5 relevant source-outline evidence

ADR-022 adds `Retain` and `RetainAsOneEntryPerDocument` alongside the existing
`Discard` and `OneEntryPerDocument` policies. The QPDF adapter reads bounded
source-outline JSON before page assembly, prunes excluded leaves, retains
containers with retained descendants, and rebuilds the output tree using actual
output page object references. Source titles use file base names, and a
deliberately duplicated selected source page targets its first output occurrence.

Local Windows evidence is green:

- format, warning-denied Clippy and the complete portable Rust workspace suite;
- a real QPDF/MuPDF contract that verifies a root-retained hierarchy (`Chapter
  2` then `Appendix`) and the grouped hierarchy under the source base-name;
- recursive evidence summarization for all three outline-generating policies.
- 10/10 deterministic Chromium browser E2E scenarios, including both retained
  policy states and an inspected 1440px retained-policy visual checkpoint;
- 4/4 production-protocol WebView2 scenarios; and
- the coordinate-free real Windows open/save-dialog regression flow.

The remaining P4.5 exit work is the path-scoped Linux review evidence before
integration. The local release build can be reused by native suites; this
avoids treating repeated link time as a test result.

## P4.6 odd-page blank insertion evidence

The Merge request, desktop IPC DTO, Leptos workspace and QPDF adapter now carry
`add_blank_page_if_odd`. The adapter creates a private valid blank PDF matching
the final selected page's MediaBox, CropBox and rotation, then adds it after
every source whose resolved selection has an odd length, including the final
source. Bookmark planning advances by the inserted page so document entries
remain accurate.

Local Windows evidence is green:

- real QPDF/MuPDF contract: six-page output, blank page at output page 4,
  source-matched `[0 0 612 792]` MediaBox with 90-degree rotation, empty
  MuPDF text extraction, inherited `/Pages` geometry coverage, and one-entry
  destinations at pages 1 and 5;
- complete workspace Rust tests and compilation; and
- 10/10 Chromium E2E scenarios, including the accessible control and its
  enabled state alongside bookmark policies.

## P4.7 filename footer overlay evidence

The Merge request, desktop IPC DTO, Leptos workspace and QPDF adapter now carry
`add_filename_footer`. When enabled, QPDF first assembles the selected pages
and then receives a private one-page-per-output-page overlay. Each non-blank
overlay page contains its source filename and copies the source MediaBox,
CropBox and rotation; generated odd-page blanks are intentionally left empty.
Bookmark reconstruction remains the final metadata step.

Local Windows evidence is green:

- real QPDF/MuPDF contract: four-page output, source filenames extracted on
  all four contributing pages, valid structure and no page-count drift;
- warning-denied workspace Clippy and all portable Rust tests; and
- browser advanced-policy E2E coverage for the explicit footer control and its
  state retention while bookmark policies change.

This is intentionally Partial rather than complete parity. The first overlay
uses Helvetica/WinAnsi and replaces non-ASCII filename characters with a
visible fallback marker. The adapter now asks `MuPDF` for bounded structured
text and selects a deterministic quiet top/bottom band when available, while
falling back safely when the optional extractor cannot run. Unicode font
embedding, semantic form/annotation/image collision handling and visual golden
evidence remain required before MERGE-007 is Verified.

## P4.8 table-of-contents evidence

The Merge request, desktop DTO, Leptos workspace and QPDF adapter now carry
explicit `MergeTocPolicy::FileNames` and `MergeTocPolicy::DocumentTitles`
options. Both prepend a generated one-or-more-page contents document and shift
document-level bookmark destinations after the inserted pages. Document-title
mode reads a non-empty PDF information-dictionary `/Title`, with a deterministic
filename fallback. Contents pages use fixed A4 geometry and the same bounded,
process-isolated QPDF assembly boundary as the rest of Merge.

Local Windows evidence is green:

- real QPDF/MuPDF contract: seven-page filename and document-title outputs with
  valid contents pages, source labels, page numbers and matching bookmark
  targets; document-title mode verifies a metadata title and a filename
  fallback;
- warning-denied focused Rust checks; and
- browser advanced-policy E2E coverage for the explicit filename contents
  control and its state retention.

This remains Partial: Unicode font embedding and a visual golden baseline are
still required before MERGE-005 becomes Verified. ADR-025 records the measured
metadata-title decoding and fallback policy.

Windows path-length hardening is included in this slice: when a requested
destination is close to `MAX_PATH`, the atomic temporary sibling keeps the
same parent but uses a compact hidden name so QPDF can open it without changing
the final output path. The Unicode/long-path geometry contract is green on the
local Windows toolchain after this adjustment.

## P5.2 latest split evidence

The current P5.2 PR head passed the Linux quality run `153`, PDF engine
capability probe run `112`, and Application shell run `130`. The portable
workspace suite now includes 24 passing QPDF adapter unit tests, including
exact Unicode outline titles, nested hierarchy, collapsed expansion state and
descendant-correct PDF `/Count` values. The native Split command-boundary
contract and browser acceptance remain green. The application-shell artifact
is `8852222069` with digest
`sha256:a6f5d72a7c46bca344d39b544ee323b05146f86bc47a1cc09e557fe8264e931f`.
The PDF capability artifact is `8851987689` with digest
`sha256:8a688e422859301270a061c6a5004f8d4019a37759697afef91f2241b5816a7e`.
No Python runtime was used in the local development or validation loop.

## Exact next actions

1. Add dedicated real-engine fixtures for named destinations, action-backed
   outlines, and outline style/color attributes so safe preservation policies
   can replace the current fail-closed gates.
2. Extend Split contract evidence for Unicode outline titles, expansion state,
   and duplicate-page identity once the fixture policy is defined.
3. Continue closing the remaining MERGE-003 through MERGE-008 policy gaps,
   especially Unicode typography and semantic overlay collisions.

## Completion status

P0 through P3, P4.1 Merge core and P4.2 Merge desktop acceptance are complete.
P4 remains in progress at the parity-corpus checkpoint. P5.1 split planner,
materializer and desktop evidence are complete; P5.2 is in progress on
metadata-safety and parity fixtures. Portable/native-command tests, browser
E2E, Windows visual checkpoints, production-protocol WebView2 E2E, Linux
milestone evidence, atomic-replacement recovery and real system-dialog
automation are green.

