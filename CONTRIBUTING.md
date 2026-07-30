# Contributing

PincerPDF uses specification- and evidence-driven trunk-based development.

- Start with `AGENTS.md` and `docs/PROJECT_STATE.md`.
- Keep commits atomic, tested and independently reversible.
- Push stable checkpoints directly to `main`; use a short-lived branch only when an isolated experiment would destabilize the trunk.
- Do not retain merged branches.
- Keep the Windows local quality loop green before publishing and preserve the Linux devcontainer as the compatibility oracle.
- Do not add macOS packaging complexity before the Windows release-candidate gate.
- Behavior changes require tests and an update to durable project state or traceability evidence.
- Run `scripts/check-fast.ps1` on Windows or `make check-fast` in Linux before publishing.
