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
    ".gitattributes",
    ".cargo/config.toml",
    ".devcontainer/Dockerfile",
    ".devcontainer/devcontainer.json",
    ".github/workflows/application-shell.yml",
    ".github/workflows/linux-quality.yml",
    ".github/workflows/merge-core.yml",
    "apps/pincerpdf-ui/index.html",
    "apps/pincerpdf-ui/src/merge_workspace.rs",
    "apps/pincerpdf-ui/src/native_bridge.rs",
    "apps/pincerpdf-ui/styles.css",
    "apps/pincerpdf-desktop/src-tauri/src/merge_commands.rs",
    "apps/pincerpdf-desktop/src-tauri/tauri.conf.json",
    "apps/pincerpdf-desktop/src-tauri/capabilities/default.json",
    "apps/pincerpdf-desktop/src-tauri/icons/icon.ico",
    "apps/pincerpdf-desktop/src-tauri/icons/icon.png",
    "apps/pincerpdf-desktop/src-tauri/icons/icon.svg",
    "crates/pincerpdf-merge/Cargo.toml",
    "crates/pincerpdf-merge/src/lib.rs",
    "crates/pincerpdf-desktop-api/Cargo.toml",
    "crates/pincerpdf-desktop-api/src/lib.rs",
    "crates/pincerpdf-engine-qpdf/Cargo.toml",
    "crates/pincerpdf-engine-qpdf/src/lib.rs",
    "crates/pincerpdf-engine-qpdf/tests/merge_contract.rs",
    "docs/PROJECT_STATE.md",
    "docs/ROADMAP.md",
    "docs/architecture/adr/ADR-015-merge-core-boundary.md",
    "docs/architecture/adr/ADR-016-windows-first-local-development.md",
    "docs/architecture/adr/ADR-017-trusted-desktop-merge-boundary.md",
    "docs/architecture/adr/ADR-018-production-webview-e2e.md",
    "docs/architecture/adr/ADR-019-windows-atomic-replacement.md",
    "docs/architecture/adr/ADR-020-semantic-windows-dialog-e2e.md",
    "package.json",
    "package-lock.json",
    "pnpm-workspace.yaml",
    "playwright.config.mjs",
    "wdio.native.conf.mjs",
    "scripts/summarize-merge-evidence.py",
    "scripts/Enter-PincerPdfDev.ps1",
    "scripts/check-fast.ps1",
    "scripts/check-native-e2e.ps1",
    "scripts/check-system-dialog-e2e.ps1",
    "scripts/invoke_windows_file_dialog.py",
    "requirements-windows-e2e.txt",
    "scripts/tests/test_summarize_merge_evidence.py",
    "tests/e2e/application-shell.spec.mjs",
    "tests/native/merge-shell.e2e.mjs",
    "tests/native/merge-system-dialog.e2e.mjs",
)

REQUIRED_MEMBERS = {
    "apps/pincerpdf-cli",
    "apps/pincerpdf-desktop/src-tauri",
    "apps/pincerpdf-ui",
    "crates/pincerpdf-application",
    "crates/pincerpdf-desktop-api",
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
    """Verify committed Rust and browser lockfiles cover the active P4 slice."""

    cargo_lock = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    for package_name in (
        "pincerpdf-desktop",
        "pincerpdf-ui",
        "pincerpdf-desktop-api",
        "pincerpdf-merge",
        "pincerpdf-engine-qpdf",
        "tauri",
        "tauri-plugin-dialog",
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

    required_native_dependencies = {
        "@wdio/cli": "9.30.0",
        "@wdio/local-runner": "9.30.0",
        "@wdio/mocha-framework": "9.30.0",
        "@wdio/spec-reporter": "9.29.1",
        "@wdio/tauri-service": "1.2.0",
        "webdriverio": "9.30.0",
    }
    for name, version in required_native_dependencies.items():
        if package.get("devDependencies", {}).get(name) != version:
            fail(f"native WebView dependency must remain exactly locked: {name}@{version}")

    required_overrides = {
        "@wdio/native-utils": "2.5.0",
        "brace-expansion": "5.0.8",
        "minimatch": "10.2.6",
        "serialize-javascript": "7.0.5",
    }
    if package.get("overrides") != required_overrides:
        fail("native WebView dependency compatibility/security overrides changed")

    locked_override_paths = {
        "@wdio/native-utils": "node_modules/@wdio/native-utils",
        "brace-expansion": "node_modules/brace-expansion",
        "minimatch": "node_modules/minimatch",
        "serialize-javascript": "node_modules/serialize-javascript",
    }
    for name, path in locked_override_paths.items():
        locked_version = package_lock.get("packages", {}).get(path, {}).get("version")
        if locked_version != required_overrides[name]:
            fail(f"package-lock does not resolve required override: {name}")

    pnpm_workspace = (ROOT / "pnpm-workspace.yaml").read_text(encoding="utf-8")
    for name, version in required_overrides.items():
        if f'"{name}": "{version}"' not in pnpm_workspace:
            fail(f"pnpm workspace does not resolve required override: {name}")
    for allowed_build in ("edgedriver: true", "esbuild: true", "geckodriver: false"):
        if allowed_build not in pnpm_workspace:
            fail(f"pnpm build-script policy missing: {allowed_build}")


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
    if 'Array(7).fill("Not implemented")' not in tests:
        fail("E2E must keep all seven post-Merge tools explicitly gated")
    if 'data-wasm-opt="0"' not in index:
        fail("P3 must disable the measured-incompatible Trunk wasm-opt post-link pass")
    if '"1 available · 7 planned"' not in source or '"Available"' not in source:
        fail("P4.2 Merge availability and remaining independent gates are not visible")

    windows = config.get("app", {}).get("windows", [])
    if len(windows) != 1 or windows[0].get("label") != "main":
        fail("Tauri host must expose exactly one main shell window in P3")
    if config.get("app", {}).get("withGlobalTauri") is not True:
        fail("Rust/WASM invoke bridge requires the explicitly configured global Tauri API")
    csp = config.get("app", {}).get("security", {}).get("csp", "")
    if "connect-src 'self' ipc: http://ipc.localhost" not in csp:
        fail("production CSP must permit same-origin WASM fetch plus Tauri IPC")
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
    """Verify the durable P4.1 Merge engine and application boundary."""

    merge_source = (ROOT / "crates/pincerpdf-merge/src/lib.rs").read_text(encoding="utf-8")
    qpdf_source = (ROOT / "crates/pincerpdf-engine-qpdf/src/lib.rs").read_text(encoding="utf-8")
    contract = (ROOT / "crates/pincerpdf-engine-qpdf/tests/merge_contract.rs").read_text(
        encoding="utf-8"
    )
    makefile = (ROOT / "Makefile").read_text(encoding="utf-8")
    workflow = (ROOT / ".github/workflows/merge-core.yml").read_text(encoding="utf-8")

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


def verify_merge_desktop_contract() -> None:
    """Verify the trusted P4.2 command, UI, browser-adapter and safety contracts."""

    api = (ROOT / "crates/pincerpdf-desktop-api/src/lib.rs").read_text(encoding="utf-8")
    commands = (
        ROOT / "apps/pincerpdf-desktop/src-tauri/src/merge_commands.rs"
    ).read_text(encoding="utf-8")
    host = (ROOT / "apps/pincerpdf-desktop/src-tauri/src/main.rs").read_text(
        encoding="utf-8"
    )
    ui = (ROOT / "apps/pincerpdf-ui/src/merge_workspace.rs").read_text(
        encoding="utf-8"
    )
    bridge = (ROOT / "apps/pincerpdf-ui/src/native_bridge.rs").read_text(
        encoding="utf-8"
    )
    tests = (ROOT / "tests/e2e/application-shell.spec.mjs").read_text(
        encoding="utf-8"
    )
    makefile = (ROOT / "Makefile").read_text(encoding="utf-8")
    workflow = (ROOT / ".github/workflows/application-shell.yml").read_text(
        encoding="utf-8"
    )

    for token in (
        "pub struct MergeRunRequest",
        "pub struct MergeInputRequest",
        "pub struct CommandError",
        "pub path_token: String",
        "This type intentionally does not implement `Debug`",
    ):
        if token not in api:
            fail(f"P4.2 desktop API contract missing: {token}")

    for token in (
        "pub struct DesktopState",
        "register_path",
        "resolve_path",
        "pub async fn pick_merge_sources",
        "pub async fn pick_merge_destination",
        "pub async fn run_merge",
        "pub fn cancel_merge",
        "ExistingOutputPolicy::Fail",
        "spawn_blocking",
        "Entry::Occupied",
        "native_command_boundary_merges_only_registered_paths",
    ):
        if token not in commands:
            fail(f"P4.2 trusted desktop command contract missing: {token}")

    for token in (
        "tauri_plugin_dialog::init()",
        "merge_engine_status",
        "pick_merge_sources",
        "pick_merge_destination",
        "run_merge",
        "cancel_merge",
    ):
        if token not in host:
            fail(f"P4.2 Tauri composition missing: {token}")

    for token in (
        'data-testid="merge-workspace"',
        'data-testid="merge-source-row"',
        'data-testid="run-merge"',
        "browser_sources()",
        "complete_browser_merge_after_delay",
        "Discard and report",
        "Reject before processing",
    ):
        if token not in ui:
            fail(f"P4.2 Merge UI contract missing: {token}")

    if '__TAURI__", "core"' not in bridge or "serde_json" not in bridge:
        fail("P4.2 Rust/WASM bridge is not wired to typed Tauri command messages")

    for token in (
        "one available Merge tool",
        "ordered Merge plan",
        "running and completed Merge states",
        "advanced safety policies",
        "merge-completed-compact.png",
    ):
        if token not in tests:
            fail(f"P4.2 E2E acceptance contract missing: {token}")

    if "merge-desktop-contract:" not in makefile:
        fail("P4.2 native desktop contract is not wired into Make")
    if (
        "make merge-desktop-contract" not in workflow
        or "crates/pincerpdf-desktop-api/**" not in workflow
    ):
        fail("P4.2 native desktop contract is not wired into its Linux compatibility lane")


def verify_native_webview_contract() -> None:
    """Verify production-protocol WebView2 E2E without shipping test permissions."""

    package = load_json("package.json")
    config = (ROOT / "wdio.native.conf.mjs").read_text(encoding="utf-8")
    script = (ROOT / "scripts/check-native-e2e.ps1").read_text(encoding="utf-8")
    test = (ROOT / "tests/native/merge-shell.e2e.mjs").read_text(encoding="utf-8")
    desktop_manifest = (
        ROOT / "apps/pincerpdf-desktop/src-tauri/Cargo.toml"
    ).read_text(encoding="utf-8")
    desktop_host = (
        ROOT / "apps/pincerpdf-desktop/src-tauri/src/main.rs"
    ).read_text(encoding="utf-8")
    capability = (
        ROOT / "apps/pincerpdf-desktop/src-tauri/capabilities/default.json"
    ).read_text(encoding="utf-8")

    if package.get("scripts", {}).get("test:e2e:native") != (
        "wdio run wdio.native.conf.mjs"
    ):
        fail("native WebView2 package script is missing")

    for token in (
        'driverProvider: "official"',
        "autoInstallTauriDriver: true",
        "autoDownloadEdgeDriver: true",
        'browserName: "tauri"',
        "PINCERPDF_NATIVE_E2E_ARTIFACT_DIR",
    ):
        if token not in config:
            fail(f"native WebView2 runner contract missing: {token}")

    for token in (
        "--features tauri/custom-protocol",
        "pnpm exec wdio run wdio.native.conf.mjs",
        "PINCERPDF_NATIVE_E2E_ARTIFACT_DIR",
    ):
        if token not in script:
            fail(f"production WebView2 build/run contract missing: {token}")

    for token in (
        "window.__TAURI__.core",
        '"merge_engine_status"',
        '"cancel_merge"',
        '"native-startup.json"',
        '"native-startup.html"',
        '"native-merge-empty-windows.png"',
        "dispatchEvent(new MouseEvent",
    ):
        if token not in test:
            fail(f"native WebView2 acceptance evidence missing: {token}")

    production_sources = "\n".join((desktop_manifest, desktop_host, capability))
    if "tauri-plugin-wdio" in production_sources or '"wdio:' in capability:
        fail("production desktop binary must not include WDIO plugin code or permissions")


def verify_windows_dialog_contract() -> None:
    """Verify coordinate-free real Windows common-dialog acceptance."""

    package = load_json("package.json")
    requirements = (
        ROOT / "requirements-windows-e2e.txt"
    ).read_text(encoding="utf-8").splitlines()
    runner = (
        ROOT / "scripts/check-system-dialog-e2e.ps1"
    ).read_text(encoding="utf-8")
    driver = (
        ROOT / "scripts/invoke_windows_file_dialog.py"
    ).read_text(encoding="utf-8")
    test = (
        ROOT / "tests/native/merge-system-dialog.e2e.mjs"
    ).read_text(encoding="utf-8")

    if package.get("scripts", {}).get("test:e2e:system-dialog") != (
        "powershell.exe -NoProfile -ExecutionPolicy Bypass "
        "-File scripts/check-system-dialog-e2e.ps1"
    ):
        fail("Windows system-dialog package script is missing")

    if requirements != [
        "pywinauto==0.6.9",
        "pywin32==312",
        "comtypes==1.4.16",
        "six==1.17.0",
    ]:
        fail("Windows system-dialog dependencies must remain exactly pinned")

    for token in (
        "PINCERPDF_DEV_ROOT",
        "requirements-windows-e2e.txt",
        "PINCERPDF_SYSTEM_DIALOG_E2E",
        "PINCERPDF_SYSTEM_DIALOG_FIXTURES",
        "PINCERPDF_SYSTEM_DIALOG_OUTPUT_DIR",
        "pnpm exec wdio run wdio.native.conf.mjs",
        "--spec tests/native/merge-system-dialog.e2e.mjs",
    ):
        if token not in runner:
            fail(f"Windows system-dialog runner contract missing: {token}")

    for token in (
        'Desktop(backend="win32")',
        'class_name="#32770"',
        "dialog.process_id()",
        "control.control_id()",
        "set_edit_text",
        "confirm_overwrite",
        "control.click()",
    ):
        if token not in driver:
            fail(f"semantic Windows dialog driver contract missing: {token}")
    for forbidden in ("click_input", "move_mouse", "set_cursor_pos"):
        if forbidden in driver:
            fail(f"Windows dialog driver must not use coordinate input: {forbidden}")

    for token in (
        'invokeDialog("Cancel")',
        'invokeDialog("Open", [plain, bookmarks])',
        'invokeDialog("Save", [output])',
        '"replace-existing-output"',
        "conflictDialog: conflictDialogEvidence",
        "conflictPreservedSha256",
        "replacementQpdfCheck: true",
        "save: saveEvidence",
        '"--show-npages"',
        '"--check"',
        '"system-dialog-evidence.json"',
        '"system-dialog-merge-completed.png"',
    ):
        if token not in test:
            fail(f"Windows system-dialog acceptance evidence missing: {token}")


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
    verify_merge_desktop_contract()
    verify_native_webview_contract()
    verify_windows_dialog_contract()

    print("PincerPDF structural verification passed")
    print(f"workspace_members={len(REQUIRED_MEMBERS)}")
    print(f"rust_toolchain={toolchain['channel']}")
    print("dependency_locks=verified")
    print("application_shell=verified")
    print("merge_core=verified")
    print("merge_desktop=verified")
    print("native_webview=verified")
    print("windows_system_dialog=verified")
    print("status=verified")


if __name__ == "__main__":
    main()
