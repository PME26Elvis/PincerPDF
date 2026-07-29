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
    "apps/pincerpdf-ui/index.html",
    "apps/pincerpdf-ui/styles.css",
    "apps/pincerpdf-desktop/src-tauri/tauri.conf.json",
    "apps/pincerpdf-desktop/src-tauri/capabilities/default.json",
    "docs/PROJECT_STATE.md",
    "docs/ROADMAP.md",
    "package.json",
    "playwright.config.mjs",
    "tests/e2e/application-shell.spec.mjs",
)

REQUIRED_MEMBERS = {
    "apps/pincerpdf-cli",
    "apps/pincerpdf-desktop/src-tauri",
    "apps/pincerpdf-ui",
    "crates/pincerpdf-application",
    "crates/pincerpdf-domain",
    "crates/pincerpdf-engine-api",
    "crates/pincerpdf-filesystem",
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


def verify_shell_contract() -> None:
    """Verify stable shell, accessibility, motion, and native-host contracts."""

    source = (ROOT / "apps/pincerpdf-ui/src/main.rs").read_text(encoding="utf-8")
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

    windows = config.get("app", {}).get("windows", [])
    if len(windows) != 1 or windows[0].get("label") != "main":
        fail("Tauri host must expose exactly one main shell window in P3")
    permissions = set(capability.get("permissions", []))
    if permissions != {"core:default"}:
        fail("P3 Tauri capability must remain least-privilege core:default")


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

    verify_shell_contract()

    print("PincerPDF structural verification passed")
    print(f"workspace_members={len(REQUIRED_MEMBERS)}")
    print(f"rust_toolchain={toolchain['channel']}")
    print("application_shell=verified")
    print("status=verified")


if __name__ == "__main__":
    main()
