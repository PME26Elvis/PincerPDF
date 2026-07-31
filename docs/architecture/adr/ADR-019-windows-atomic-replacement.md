# ADR-019: Accept Rust 1.97 atomic file replacement on Windows

- Status: Accepted
- Date: 2026-07-30

## Context

P4.2 deliberately kept overwrite unavailable until PincerPDF could prove that a validated temporary PDF replaces an existing Windows destination without first deleting that destination. A delete-then-rename sequence creates a data-loss window and is not acceptable.

Rust 1.97.1 documents `std::fs::rename` as replacing an existing destination. On supported Windows 10 1607 or newer filesystems it uses `FileRenameInfoEx`; its fallback is `MoveFileExW`. PincerPDF already creates the temporary PDF as a hidden sibling, flushes its file contents, verifies its PDF semantics, and then calls this operation.

## Decision

1. Continue to use `std::fs::rename` for finalization on the pinned Rust 1.97.1 toolchain.
2. Permit `ExistingOutputPolicy::Replace` only when the user explicitly selected replacement and the destination existed during planning.
3. Never remove the destination before finalization.
4. Keep the original destination intact when Windows rejects replacement, including the common locked-file case.
5. Remove the uncommitted temporary sibling when the transaction fails or is dropped.
6. Treat file-data durability as proven by syncing the temporary file before the rename. Treat namespace durability across sudden power loss as filesystem/OS dependent until a safe, portable directory-flush mechanism is available on Windows.

## Executable evidence

The Merge test suite now proves:

- an existing output is replaced with the verified temporary bytes;
- a destination created after `Fail` planning is preserved and produces a conflict;
- a Windows destination opened without delete sharing makes replacement fail while preserving the old bytes;
- failed transactions clean the uncommitted temporary sibling after the lock is released.

The success and locked-failure scenarios run locally on the Windows-first development host. Portable success/race tests continue to run in the Linux compatibility lane.

## Consequences

- The overwrite policy can move from an implementation gate to UI/system-dialog acceptance.
- A successful return means the final path names the verified replacement; no observable delete-first window exists.
- Files opened by another Windows process without delete sharing remain a recoverable error.
- Strict power-loss durability of the directory entry is not overclaimed.

## Revisit trigger

Revisit if the pinned Rust implementation changes, a supported Windows filesystem lacks replacement semantics, a safe directory-flush abstraction becomes available, or fault-injection evidence reveals a namespace-loss case.
