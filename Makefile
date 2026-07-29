SHELL := /usr/bin/env bash
.DEFAULT_GOAL := help

.PHONY: help bootstrap-check verify-structure fmt lint test check-fast doctor selection fixtures-pdf probe-pdf-engines web-build desktop-check shell-e2e clean

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
	  '  make selection SPEC=1-3,8 TOTAL=10' \
	  '  make fixtures-pdf       Generate deterministic PDF engine fixtures' \
	  '  make probe-pdf-engines  Run QPDF/MuPDF capability measurements' \
	  '  make web-build          Build the Leptos CSR shell with Trunk' \
	  '  make desktop-check      Compile-check the Tauri 2 host' \
	  '  make shell-e2e          Run browser shell verification'

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

fixtures-pdf:
	@rm -rf .artifacts/pdf-engine-probe/fixtures
	@python3 tests/fixtures/pdf/generate_fixtures.py .artifacts/pdf-engine-probe/fixtures

probe-pdf-engines: fixtures-pdf
	@rm -rf .artifacts/pdf-engine-probe/work .artifacts/pdf-engine-probe/render .artifacts/pdf-engine-probe/report.json
	@python3 scripts/probe-pdf-engines.py \
	  --fixtures .artifacts/pdf-engine-probe/fixtures \
	  --output .artifacts/pdf-engine-probe

web-build:
	@cd apps/pincerpdf-ui && trunk build --release

desktop-check:
	@cargo check --locked -p pincerpdf-desktop

shell-e2e:
	@npm run test:e2e

clean:
	@cargo clean
