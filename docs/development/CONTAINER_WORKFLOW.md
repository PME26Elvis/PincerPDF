# Container Development Workflow

The pinned devcontainer is the primary implementation and verification environment. GitHub is durable source storage; `.pincer-local/` holds large evidence and private fixtures.

## Bootstrap

```bash
make bootstrap-check
make verify-structure
make check-fast
```

The image pins Rust and Node base images and installs Linux Tauri/WebKitGTK dependencies, Xvfb, Chromium, QPDF, MuPDF tools and inspection utilities. Cargo jobs default to four for the approximately 6 GiB/no-swap development environment.

## Inner loop

1. Read project state and the relevant ADR/spec.
2. Add or adjust a failing test or executable contract.
3. Implement the smallest complete behavior.
4. Run the narrow test and then `make check-fast`.
5. For UI changes, run deterministic E2E plus assigned screenshot/motion checkpoints.
6. Update project state/evidence.
7. Push an atomic recoverable commit to `main`.

## Failure policy

A missing dependency, OOM, inaccessible package registry or unavailable display driver is an infrastructure failure. Record it precisely and repair the environment; never reinterpret it as permission to skip validation.

## Restricted-container fallback

When the active execution container cannot install the pinned Rust toolchain, use `.github/workflows/linux-quality.yml` as the narrow remote compiler lane. It deliberately runs one Ubuntu job, only on relevant `main` changes or manual dispatch. It is not a substitute for later packaged-app or cross-platform validation. A green run may satisfy the compiler/Clippy/test portion of a checkpoint, but local structural verification and durable evidence updates are still required.
