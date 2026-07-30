#!/usr/bin/env python3
"""Create a deterministic, secret-scanned summary for the P4.1 Merge contract."""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_text(value: str) -> bytes:
    """Encode extracted text with stable LF endings on every platform."""
    normalized = value.replace("\r\n", "\n").replace("\r", "\n").strip()
    return f"{normalized}\n".encode()


def command(*args: str | Path) -> str:
    completed = subprocess.run(
        [str(arg) for arg in args],
        text=True,
        capture_output=True,
        check=True,
    )
    return completed.stdout.strip()


def first_non_empty_line(*streams: str) -> str:
    """Return the first non-empty line across ordered process streams."""
    for stream in streams:
        for line in stream.splitlines():
            if stripped := line.strip():
                return stripped
    raise RuntimeError("command produced no version output")


def command_version(*args: str | Path) -> str:
    """Read a tool version whether the executable reports it on stdout or stderr."""
    completed = subprocess.run(
        [str(arg) for arg in args],
        text=True,
        capture_output=True,
        check=True,
    )
    return first_non_empty_line(completed.stdout, completed.stderr)


def main() -> int:
    root = Path(sys.argv[1]).resolve()
    outputs = [root / "ordered.pdf", root / "encrypted-merge.pdf"]
    missing = [str(path) for path in outputs if not path.is_file()]
    if missing:
        raise SystemExit(f"missing merge evidence outputs: {', '.join(missing)}")

    report = {
        "schema": 1,
        "qpdf_version": command_version("qpdf", "--version"),
        "mutool_version": command_version("mutool", "-v"),
        "outputs": {},
    }
    for output in outputs:
        command("qpdf", "--check", output)
        text_path = output.with_suffix(".txt")
        text_path.write_bytes(
            canonical_text(
                command("mutool", "draw", "-q", "-F", "txt", "-o", "-", output)
            )
        )
        report["outputs"][output.name] = {
            "bytes": output.stat().st_size,
            "sha256": sha256(output),
            "pages": int(command("qpdf", "--show-npages", output)),
            "text_sha256": sha256(text_path),
        }

    serialized = json.dumps(report, indent=2, sort_keys=True) + "\n"
    for secret in ("p4-secret", "p4-owner"):
        if secret in serialized:
            raise SystemExit("merge evidence leaked a fixture password")
    (root / "report.json").write_bytes(serialized.encode())
    print(serialized, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
