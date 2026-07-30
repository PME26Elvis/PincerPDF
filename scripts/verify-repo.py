#!/usr/bin/env python3
"""Dependency-free structural verification for restricted bootstrap environments."""

from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

REQUIRED_FILES = (
    "AGENTS.md",
    "Cargo.toml",
    "Cargo.lock",
    "LICENSE",
    "NOTICE.md",
    "README.md",
    "rust-toolchain.toml",
    ".cargo/config.toml",
    ".devcontainer/Dockerfile",
    ".devcontainer/devcontainer.json",
    ".github/workflows/application-shell.yml",
    ".github/workflows/linux-quality.yml",
    ".github/workflows/merge-core.yml",
    "apps/pincerpdf-ui/index.html",
    "apps/pincerpdf-ui/styles.css",
    "apps/pincerpdf-desktop/src-tauri/tauri.conf.json",
    "apps/pincerpdf-desktop/src-tauri/capabilities/default.json",
    "apps/pincerpdf-desktop/src-tauri/icons/icon.ico",
    "apps/pincerpdf-desktop/src-tauri/icons/icon.png",
    "apps/pincerpdf-desktop/src-tauri/icons/icon.svg",
    "crates/pincerpdf-merge/Cargo.toml",
    "crates/pincerpdf-merge/src/lib.rs",
    "crates/pincerpdf-engine-qpdf/Cargo.toml",
    "crates/pincerpdf-engine-qpdf/src/lib.rs",
    "crates/pincerpdf-engine-qpdf/tests/merge_contract.rs",
    "docs/PROJECT_STATE.md",
    "docs/ROADMAP.md",
    "docs/architecture/adr/ADR-015-merge-core-boundary.md",
    "docs/architecture/adr/ADR-016-windows-first-local-development.md",
    "package.json",
    "package-lock.json",
    "playwright.config.mjs",
    "scripts/summarize-merge-evidence.py",
    "scripts/Enter-PincerPdfDev.ps1",
    "scripts/check-fast.ps1",
    "scripts/tests/test_summarize_merge_evidence.py",
    "tests/e2e/application-shell.spec.mjs",
)

REQUIRED_MEMBERS = {
    "apps/pincerpdf-cli",
    "apps/pincerpdf-desktop/src-tauri",
    "apps/pincerpdf-ui",
    "crates/pincerpdf-application",
    "crates/pincerpdf-domain",
    "crates/pincerpdf-engine-api",
    "crates/pincerpdf-engine-qpdf",
    "crates/pincerpdf-filesystem",
    "crates/pincerpdf-merge",
}

BANNED_TRACKED_PARTS = {
    ".pincer-local",
    ".fixtures-private",
    "node_modules",
    "target",
    "test-results",
    "playwright-report",
}


def fail(message: str) -> None:
    """Exit with a structural-policy failure."""

    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_toml(relative: str) -> dict:
    """Parse one repository TOML document."""

    path = ROOT / relative
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot parse {relative}: {error}")


def load_json(relative: str) -> dict:
    """Parse one repository JSON document."""

    path = ROOT / relative
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot parse {relative}: {error}")


def verify_dependency_locks() -> None:
    """Verify committed Rust and browser lockfiles cover P3 and P4.1."""

    cargo_lock = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    for package_name in (
        "pincerpdf-desktop",
        "pincerpdf-ui",
        "pincerpdf-merge",
        "pincerpdf-engine-qpdf",
        "tauri",
        "leptos",
    ):
        if f'name = "{package_name}"' not in cargo_lock:
            fail(f"Cargo.lock does not contain required package: {package_name}")

    package = load_json("package.json")
    package_lock = load_json("package-lock.json")
    if package_lock.get("lockfileVersion") != 3:
        fail("package-lock.json must use npm lockfileVersion 3")

    root_package = package_lock.get("packages", {}).get("", {})
    if root_package.get("devDependencies") != package.get("devDependencies"):
        fail("package-lock root devDependencies must match package.json")

    playwright_version = package.get("devDependencies", {}).get("@playwright/test")
    locked_playwright = package_lock.get("packages", {}).get("node_modules/@playwright/test", {})
    if playwright_version != "1.62.0" or locked_playwright.get("version") != playwright_version:
        fail("Playwright must remain exactly locked to 1.62.0 for P3 evidence")


def verify_shell_contract() -> None:
    """Verify stable shell, accessibility, motion, and native-host contracts."""

    source = (ROOT / "apps/pincerpdf-ui/src/main.rs").read_text(encoding="utf-8")
    index = (ROOT / "apps/pincerpdf-ui/index.html").read_text(encoding="utf-8")
    styles = (ROOT / "apps/pincerpdf-ui/styles.css").read_text(encoding="utf-8")
    tests = (ROOT / "tests/e2e/application-shell.spec.mjs").read_text(encoding="utf-8")
    config = load_json("apps/pincerpdf-desktop/src-tauri/tauri.conf.json")
    capability = load_json("apps/pincerpdf-desktop/src-tauri/capabilities/default.json")

    required_test_ids = {
        "app-shell",
        "motion-toggle",
        "selected-tool-title",
        "selected-tool-status",
        "open-files",
    }
    missing_test_ids = sorted(
        test_id for test_id in required_test_ids if f'data-testid="{test_id}"' not in source
    )
    if missing_test_ids:
        fail(f"application shell test hooks missing: {missing_test_ids}")

    for slug in (
        "merge",
        "split",
        "split-bookmarks",
        "split-size",
        "alternate-mix",
        "insert-pages",
        "extract",
        "rotate",
    ):
        if f"tool-nav-{slug}" not in source:
            fail(f"tool navigation hook missing: {slug}")

    if "prefers-reduced-motion: reduce" not in styles or ".motion-reduced" not in styles:
        fail("both system and manual reduced-motion policies are required")
    if "Skip to main content" not in source or 'id="main-content"' not in source:
        fail("skip-link accessibility contract is incomplete")
    if 'Array(8).fill("Not implemented")' not in tests:
        fail("E2E must enforce explicit not-implemented states for all tools")
    if 'data-wasm-opt="0"' not in index:
        fail("P3 must disable the measured-incompatible Trunk wasm-opt post-link pass")
    if '"Application shell"</strong><small>"Leptos CSR + Tauri 2 · 5 E2E"' not in source:
        fail("P3 application-shell readiness evidence is not reflected in the UI")
    if 'class="readiness-icon is-progress"' in source:
        fail("completed P3 readiness must not remain visually marked in progress")

    windows = config.get("app", {}).get("windows", [])
    if len(windows) != 1 or windows[0].get("label") != "main":
        fail("Tauri host must expose exactly one main shell window in P3")
    permissions = set(capability.get("permissions", []))
    if permissions != {"core:default"}:
        fail("P3 Tauri capability must remain least-privilege core:default")

    bundle = config.get("bundle", {})
    if bundle.get("active") is not False or bundle.get("icon") != [
        "icons/icon.png",
        "icons/icon.ico",
    ]:
        fail("Tauri host must reference tracked PNG and Windows ICO assets while bundling remains disabled")


def verify_merge_core_contract() -> None:
    """Verify the durable P4.1 Merge boundary while the UI remains gated."""

    merge_source = (ROOT / "crates/pincerpdf-merge/src/lib.rs").read_text(encoding="utf-8")
    qpdf_source = (ROOT / "crates/pincerpdf-engine-qpdf/src/lib.rs").read_text(encoding="utf-8")
    contract = (ROOT / "crates/pincerpdf-engine-qpdf/tests/merge_contract.rs").read_text(
        encoding="utf-8"
    )
    makefile = (ROOT / "Makefile").read_text(encoding="utf-8")
    workflow = (ROOT / ".github/workflows/merge-core.yml").read_text(encoding="utf-8")
    ui_source = (ROOT / "apps/pincerpdf-ui/src/main.rs").read_text(encoding="utf-8")

    for token in (
        "SecretString([REDACTED])",
        "pub struct CancellationToken",
        "pub struct ExecutionControl",
        "pub trait MergeEnginePort",
        "ensure_output_does_not_alias_source",
        "struct OutputTransaction",
    ):
        if token not in merge_source:
            fail(f"P4.1 Merge contract missing: {token}")

    for token in (
        "--password-file=<redacted>",
        "options.mode(0o600)",
        "builder.mode(0o700)",
        "child.kill()",
        "qdf_catalog_has_key",
    ):
        if token not in qpdf_source:
            fail(f"QPDF adapter safety contract missing: {token}")

    for token in (
        '#[ignore = "requires pinned qpdf/mutool and generated PDF fixtures"]',
        "PINCERPDF_MERGE_EVIDENCE_DIR",
        "mutool_text",
    ):
        if token not in contract:
            fail(f"real Merge contract evidence hook missing: {token}")

    if "merge-contract:" not in makefile or "make merge-contract" not in workflow:
        fail("P4.1 real-engine contract is not wired into Make/Actions")
    if "python3 -m unittest discover -s scripts/tests" not in workflow:
        fail("P4.1 evidence summarizer regression tests are not wired into Actions")
    if 'short_label: "Merge"' not in ui_source or '"Not implemented"' not in ui_source:
        fail("P4.1 must keep the Merge UI capability gate closed")


def main() -> None:
    """Run repository structural verification."""

    missing = [path for path in REQUIRED_FILES if not (ROOT / path).is_file()]
    if missing:
        fail(f"required files missing: {', '.join(missing)}")

    workspace = load_toml("Cargo.toml")
    members = set(workspace.get("workspace", {}).get("members", []))
    if members != REQUIRED_MEMBERS:
        fail(f"workspace member mismatch: expected {sorted(REQUIRED_MEMBERS)}, got {sorted(members)}")

    package = workspace.get("workspace", {}).get("package", {})
    if package.get("edition") != "2024":
        fail("workspace edition must remain 2024")
    if package.get("license") != "AGPL-3.0-or-later":
        fail("workspace license must remain AGPL-3.0-or-later")

    toolchain = load_toml("rust-toolchain.toml").get("toolchain", {})
    if not re.fullmatch(r"\d+\.\d+\.\d+", str(toolchain.get("channel", ""))):
        fail("Rust toolchain must be pinned to an exact stable patch version")
    required_components = {"clippy", "rustfmt"}
    if not required_components.issubset(set(toolchain.get("components", []))):
        fail("rust-toolchain.toml must include clippy and rustfmt")
    if "wasm32-unknown-unknown" not in toolchain.get("targets", []):
        fail("rust-toolchain.toml must include the Leptos CSR wasm target")

    for member in sorted(REQUIRED_MEMBERS):
        manifest_path = ROOT / member / "Cargo.toml"
        source_path = ROOT / member / "src"
        if not manifest_path.is_file() or not source_path.is_dir():
            fail(f"workspace member incomplete: {member}")
        manifest = load_toml(str(manifest_path.relative_to(ROOT)))
        if manifest.get("package", {}).get("publish") is not False:
            fail(f"internal package must set publish = false: {member}")
        if manifest.get("lints", {}).get("workspace") is not True:
            fail(f"workspace lints not inherited: {member}")

    for path in ROOT.rglob("*"):
        if any(part in BANNED_TRACKED_PARTS for part in path.relative_to(ROOT).parts):
            continue
        if path.is_file() and path.suffix == ".rs":
            text = path.read_text(encoding="utf-8")
            if "#![forbid(unsafe_code)]" not in text:
                fail(f"Rust source does not explicitly forbid unsafe code: {path.relative_to(ROOT)}")

    verify_dependency_locks()
    verify_shell_contract()
    verify_merge_core_contract()

    print("PincerPDF structural verification passed")
    print(f"workspace_members={len(REQUIRED_MEMBERS)}")
    print(f"rust_toolchain={toolchain['channel']}")
    print("dependency_locks=verified")
    print("application_shell=verified")
    print("merge_core=verified")
    print("status=verified")


if __name__ == "__main__":
    main()
