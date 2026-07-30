# Linux Container Compatibility Workflow

The pinned devcontainer is the reproducible Linux compatibility and release environment. The primary implementation loop runs on Windows under ADR-016. GitHub is durable source storage; `.pincer-local/` holds large evidence and private fixtures.

## Bootstrap

```bash
make bootstrap-check
make verify-structure
make check-fast
```

The image pins Rust and Node base images and installs Linux Tauri/WebKitGTK dependencies, Xvfb, Chromium, QPDF, MuPDF tools and inspection utilities. Cargo jobs default to four for the approximately 6 GiB/no-swap development environment.

## Linux compatibility loop

1. Read project state and the relevant ADR/spec.
2. Add or adjust a failing test or executable contract.
3. Implement the smallest complete behavior.
4. Run the narrow test and then `make check-fast`.
5. For UI changes, run deterministic E2E plus assigned screenshot/motion checkpoints.
6. Update project state/evidence.
7. Record Linux-specific evidence and promote the verified atomic checkpoint.

## Failure policy

A missing dependency, OOM, inaccessible package registry or unavailable display driver is an infrastructure failure. Record it precisely and repair the environment; never reinterpret it as permission to skip validation.

## Remote execution policy

Use `.github/workflows/linux-quality.yml` and the engine/application workflows for milestone, compatibility and release evidence, or when a behavior can only be measured on Linux. They are not the ordinary Windows edit/compile loop and are not a substitute for local native Windows validation. A green run may satisfy the Linux portion of a checkpoint, but local structural verification and durable evidence updates are still required.
