# PincerPDF

A modern, Linux-first PDF workbench reimplemented in Rust.

PincerPDF is an independent project based on the functionality and open-source codebase of PDFsam Basic. It is not affiliated with or endorsed by PDFsam or Sober Lemur S.r.l.

## Current status

Phase P1 is in progress. The repository currently contains the durable development contract, a pinned Linux container definition, and the first dependency-free Rust foundation crates. The Tauri/Leptos application shell and PDF engine adapters are intentionally not presented as complete.

## Foundation implemented

- Typed one-based PDF page numbers and page-selection parsing.
- Explicit task lifecycle and transition validation.
- Replaceable PDF engine capability/inspection port.
- Application capability gate for the eight planned PDF tools.
- Pure output-path planning for temporary/atomic finalization.
- Minimal dependency-free CLI with `doctor` and `selection` commands.
- Structural repository verification and reproducible container recipe.

## Development

Read these files before changing code:

1. [`AGENTS.md`](AGENTS.md)
2. [`docs/PROJECT_STATE.md`](docs/PROJECT_STATE.md)
3. [`docs/ROADMAP.md`](docs/ROADMAP.md)
4. [`docs/development/DEFINITION_OF_DONE.md`](docs/development/DEFINITION_OF_DONE.md)
5. Accepted ADRs under [`docs/architecture/adr/`](docs/architecture/adr/)

Primary local commands:

```bash
make verify-structure
make bootstrap-check
make check-fast
make doctor
```

`make check-fast` requires the pinned Rust toolchain. In restricted environments, `make verify-structure` still validates repository shape, manifests and policy invariants without downloading dependencies.

## License

GNU Affero General Public License v3 or later. See [`LICENSE`](LICENSE) and [`NOTICE.md`](NOTICE.md).
