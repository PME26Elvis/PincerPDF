# ADR-028: Conservative size-based split estimation

- Status: Accepted for P5.1
- Date: 2026-08-03
- Scope: Split-by-size planning and QPDF materialization

## Decision

Split-by-size planning is engine-independent once an adapter supplies one
conservative single-page serialized-size estimate for every source page. The
planner validates that estimates are complete and ordered, then greedily
groups consecutive pages without exceeding the requested byte limit. A page
whose estimate alone exceeds the limit fails before any output is created.

The QPDF adapter supplies these estimates by materializing each page through
the same `--empty --pages` path used by split outputs and measuring the
temporary PDF bytes. This avoids pretending that source object spans are
serialized output sizes. The resulting `SplitPlan` carries the byte limit;
the estimator adds a fixed 4096-byte serialization safety margin, and the
materializer checks every temporary output before atomic finalization and
removes all outputs if a real result exceeds the estimate.

The internal CLI exposes the same path as `split <OUTPUT_DIR> <SOURCE.pdf>
size:BYTES`; bookmark mode is available as `bookmarks`, while the existing
page-rule syntax remains unchanged.

## Invariants

- exactly one nonzero estimate exists for each one-based source page;
- page estimates are consumed in source order and cannot be silently sorted;
- greedy grouping preserves page order and does not duplicate or omit pages;
- a single page larger than the configured limit is rejected during planning;
- no final output is created before all plan validation succeeds; and
- a materialized output larger than the carried limit is never finalized.

## Deferred behavior

The estimator is intentionally conservative and may produce more parts than a
future optimized object-graph estimator. Encrypted-page estimation,
bookmark-destination remapping, output compression policy and the browser /
native split-by-size UI remain separate parity work.
