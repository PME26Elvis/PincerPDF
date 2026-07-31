#!/usr/bin/env python3
"""Drive PincerPDF's real Windows common file dialog without screen coordinates."""

from __future__ import annotations

import argparse
import csv
import io
import json
import os
import subprocess
import sys
import time
from pathlib import Path

development_root = Path(os.environ.get("PINCERPDF_DEV_ROOT", r"D:\PincerPDF-dev"))
site_packages = development_root / "python" / "site-packages"
for module_path in (
    site_packages,
    site_packages / "win32",
    site_packages / "win32" / "lib",
    site_packages / "pythonwin",
):
    sys.path.insert(0, str(module_path))
pywin32_dlls = site_packages / "pywin32_system32"
if hasattr(os, "add_dll_directory") and pywin32_dlls.is_dir():
    os.add_dll_directory(pywin32_dlls)

from pywinauto import Desktop


def pincerpdf_process_ids() -> set[int]:
    result = subprocess.run(
        [
            "tasklist.exe",
            "/FI",
            "IMAGENAME eq pincerpdf-desktop.exe",
            "/FO",
            "CSV",
            "/NH",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    process_ids: set[int] = set()
    for row in csv.reader(io.StringIO(result.stdout)):
        if len(row) >= 2 and row[0].lower() == "pincerpdf-desktop.exe":
            process_ids.add(int(row[1]))
    return process_ids


def descendants(dialog, control_id: int):
    return [
        control
        for control in dialog.descendants()
        if control.control_id() == control_id
    ]


def find_dialog(
    timeout: float,
    require_filename: bool | None = True,
    exclude: int | None = None,
):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        process_ids = pincerpdf_process_ids()
        for dialog in Desktop(backend="win32").windows(
            class_name="#32770",
            visible_only=True,
        ):
            if dialog.handle == exclude or dialog.process_id() not in process_ids:
                continue
            filename_controls = descendants(dialog, 1148)
            if not filename_controls:
                filename_controls = descendants(dialog, 1001)
            if require_filename and not filename_controls:
                continue
            if require_filename is False and filename_controls:
                continue
            return dialog, filename_controls
        time.sleep(0.1)
    raise TimeoutError(f"PincerPDF file dialog did not appear within {timeout:g} seconds")


def set_filename(filename_controls, value: str) -> dict[str, object]:
    candidates = []
    for control in filename_controls:
        candidates.extend([control, *control.descendants()])
    for control in candidates:
        setter = getattr(control, "set_edit_text", None)
        if setter is None:
            continue
        setter(value)
        return {
            "class": control.class_name(),
            "controlId": control.control_id(),
            "friendlyClass": control.friendly_class_name(),
        }
    raise RuntimeError("File-name control exposes no Win32 edit wrapper")


def invoke_command(dialog, control_id: int) -> dict[str, object]:
    candidates = descendants(dialog, control_id)
    buttons = [
        control
        for control in candidates
        if control.friendly_class_name() == "Button"
    ]
    control = (buttons or candidates)[0] if (buttons or candidates) else None
    if control is None:
        raise RuntimeError(f"Dialog command control ID {control_id} was not found")
    evidence = {
        "class": control.class_name(),
        "controlId": control.control_id(),
        "friendlyClass": control.friendly_class_name(),
        "text": control.window_text(),
    }
    control.click()
    return evidence


def confirm_overwrite(dialog) -> dict[str, object]:
    candidates = descendants(dialog, 6)
    buttons = [
        control
        for control in dialog.descendants()
        if control.friendly_class_name() == "Button"
    ]
    if not candidates:
        candidates = [
            button
            for button in buttons
            if button.window_text().replace("&", "").strip().lower()
            in {"yes", "y", "是", "確定"}
            or button.window_text().strip().startswith("是")
        ]
    if not candidates:
        inventory = [
            {
                "controlId": button.control_id(),
                "text": button.window_text(),
            }
            for button in buttons
        ]
        raise RuntimeError(f"Overwrite confirmation button was not found; buttons={inventory}")
    control = candidates[0]
    evidence = {
        "class": control.class_name(),
        "controlId": control.control_id(),
        "friendlyClass": control.friendly_class_name(),
        "text": control.window_text(),
    }
    control.click()
    return evidence


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--action", choices=("Open", "Save", "Cancel"), required=True)
    parser.add_argument("--paths-json", required=True)
    parser.add_argument("--confirm-overwrite", action="store_true")
    parser.add_argument("--timeout", type=float, default=30.0)
    arguments = parser.parse_args()

    paths = json.loads(arguments.paths_json)
    if arguments.action != "Cancel" and not paths:
        raise ValueError(f"At least one path is required for {arguments.action}")

    dialog, filename_controls = find_dialog(
        arguments.timeout,
        require_filename=None,
    )
    evidence: dict[str, object] = {
        "action": arguments.action,
        "dialogHandle": dialog.handle,
        "dialogText": dialog.window_text(),
        "paths": paths,
    }

    if arguments.action == "Cancel":
        dialog.close()
        evidence["command"] = "Window.close"
    else:
        value = (
            " ".join(f'"{path}"' for path in paths)
            if arguments.action == "Open"
            else str(paths[0])
        )
        if not filename_controls:
            controls = [
                {
                    "class": control.class_name(),
                    "controlId": control.control_id(),
                    "text": control.window_text(),
                }
                for control in dialog.descendants()
                if control.control_id() > 0
            ]
            raise RuntimeError(
                f"File-name control IDs 1148/1001 were not found; controls={controls}"
            )
        evidence["filenameControl"] = set_filename(filename_controls, value)
        evidence["acceptControl"] = invoke_command(dialog, 1)

    evidence["confirmationHandled"] = False
    if arguments.confirm_overwrite:
        confirmation, _ = find_dialog(
            8.0,
            require_filename=False,
            exclude=dialog.handle,
        )
        evidence["confirmationControl"] = confirm_overwrite(confirmation)
        evidence["confirmationHandled"] = True

    print(json.dumps(evidence, ensure_ascii=False, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:  # noqa: BLE001 - helper must surface cross-process diagnostics.
        print(f"{type(error).__name__}: {error}", file=sys.stderr)
        raise
