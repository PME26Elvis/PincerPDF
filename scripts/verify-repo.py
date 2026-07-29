#!/usr/bin/env python3
"""Dependency-free structural verification for restricted bootstrap environments."""

from __future__ import annotations

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
    ".github/workflows/linux-quality.yml",
    "docs/PROJECT_STATE.md",
    "docs/ROADMAP.md",
)

REQUIRED_MEMBERS = {
    "apps/pincerpdf-cli",
    "crates/pincerpdf-application",
    "crates/pincerpdf-domain",
    "crates/pincerpdf-engine-api",
    "crates/pincerpdf-filesystem",
}

BANNED_TRACKED_PARTS = {
    ".pincer-local",
    ".fixtures-private",
    "target",
    "test-results",
    "playwright-report",
}


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_toml(relative: str) -> dict:
    path = ROOT / relative
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot parse {relative}: {error}")


def main() -> None:
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

    print("PincerPDF structural verification passed")
    print(f"workspace_members={len(REQUIRED_MEMBERS)}")
    print(f"rust_toolchain={toolchain['channel']}")
    print("status=verified")


if __name__ == "__main__":
    main()
