# Contributing

PincerPDF uses specification- and evidence-driven trunk-based development.

- Start with `AGENTS.md` and `docs/PROJECT_STATE.md`.
- Keep commits atomic, tested and independently reversible.
- Push stable checkpoints directly to `main`; use a short-lived branch only when an isolated experiment would destabilize the trunk.
- Do not retain merged branches.
- Do not add cross-platform packaging complexity before the Linux release-candidate gate.
- Behavior changes require tests and an update to durable project state or traceability evidence.
- Run `make verify-structure` and, when the pinned toolchain is available, `make check-fast` before publishing.
