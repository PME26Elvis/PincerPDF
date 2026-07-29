SHELL := /usr/bin/env bash
.DEFAULT_GOAL := help

.PHONY: help bootstrap-check verify-structure fmt lint test check-fast doctor selection clean

help:
	@printf '%s\n' \
	  'PincerPDF development commands:' \
	  '  make bootstrap-check    Inspect required Linux/Rust tools' \
	  '  make verify-structure   Validate manifests, layout and policies without Cargo' \
	  '  make fmt                Format Rust source' \
	  '  make lint               Run Clippy with warnings denied' \
	  '  make test               Run workspace tests' \
	  '  make check-fast         Format check + Clippy + tests' \
	  '  make doctor             Run the CLI environment report' \
	  '  make selection SPEC=1-3,8 TOTAL=10'

bootstrap-check:
	@./scripts/bootstrap-check.sh

verify-structure:
	@python3 ./scripts/verify-repo.py

fmt:
	@cargo fmt --all

lint:
	@cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
	@cargo test --workspace --all-targets

check-fast:
	@./scripts/check-fast.sh

doctor:
	@cargo run -q -p pincerpdf-cli -- doctor

selection:
	@cargo run -q -p pincerpdf-cli -- selection "$(SPEC)" "$(TOTAL)"

clean:
	@cargo clean
