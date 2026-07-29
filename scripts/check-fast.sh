#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all --check

cargo_scope=(--workspace --all-targets)
if [[ "${PINCERPDF_PORTABLE_ONLY:-0}" == "1" ]]; then
  cargo_scope+=(--exclude pincerpdf-desktop)
fi

cargo clippy "${cargo_scope[@]}" --all-features -- -D warnings
cargo test "${cargo_scope[@]}"
python3 scripts/verify-repo.py
