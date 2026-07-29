# Definition of Done

A task/feature is Done only when all applicable items are true:

- Acceptance criteria and edge cases implemented.
- Domain validation and operation plan are unit/property tested.
- Adapter/engine contract tests pass.
- Relevant legacy test rows have a target and passing evidence.
- PDF output semantics and render are verified on the assigned corpus.
- Error, cancellation, conflict and recovery paths are covered.
- UI is keyboard accessible and reduced-motion compatible.
- Required E2E and screenshot/motion checkpoints pass.
- Logs/errors do not expose secrets.
- Documentation, feature matrix, ADR (if needed) and project state are updated.
- Code is committed and pushed to a recoverable branch, or a git bundle fallback is delivered.
- No known blocker/critical defect is hidden behind a TODO.

A generated file, a green happy-path test or a visually plausible screen alone never constitutes Done.
