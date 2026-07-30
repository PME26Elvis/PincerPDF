# PincerPDF

A modern, cross-platform PDF workbench reimplemented in Rust, developed Windows-first with continuous Linux compatibility evidence.

PincerPDF is an independent project based on the functionality and open-source codebase of PDFsam Basic. It is not affiliated with or endorsed by PDFsam or Sober Lemur S.r.l.

## Current status

Phase P4 is in progress. The Rust foundation, reproducible Linux environment, PDF-engine capability probe, Tauri/Leptos application shell, and first verified Merge core are complete. The desktop Merge workspace remains capability-gated until its command, E2E, visual and native-engine acceptance evidence passes.

## Foundation implemented

- Typed one-based PDF page numbers and page-selection parsing.
- Explicit task lifecycle and transition validation.
- Replaceable PDF engine capability/inspection port.
- Application capability gate for the eight planned PDF tools.
- Pure output-path planning for temporary/atomic finalization.
- Minimal dependency-free CLI with `doctor` and `selection` commands.
- Structural repository verification and reproducible container recipe.
- Process-isolated QPDF Merge adapter with timeout, cancellation, password redaction, semantic verification and atomic finalization.
- Tauri 2 + Leptos CSR application shell with deterministic browser E2E coverage.

## Development

Read these files before changing code:

1. [`AGENTS.md`](AGENTS.md)
2. [`docs/PROJECT_STATE.md`](docs/PROJECT_STATE.md)
3. [`docs/ROADMAP.md`](docs/ROADMAP.md)
4. [`docs/development/DEFINITION_OF_DONE.md`](docs/development/DEFINITION_OF_DONE.md)
5. Accepted ADRs under [`docs/architecture/adr/`](docs/architecture/adr/)

Primary Windows commands:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/check-fast.ps1
```

For an interactive development shell, allow scripts for the current process and dot-source the environment:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
. .\scripts\Enter-PincerPdfDev.ps1
```

Linux compatibility commands:

```bash
make verify-structure
make bootstrap-check
make check-fast
make doctor
```

Both quality paths require the pinned Rust toolchain. In restricted environments, `python scripts/verify-repo.py` still validates repository shape, manifests and policy invariants without downloading dependencies.

## License

GNU Affero General Public License v3 or later. See [`LICENSE`](LICENSE) and [`NOTICE.md`](NOTICE.md).
