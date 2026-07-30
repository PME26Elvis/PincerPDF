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


def command(*args: str | Path) -> str:
    completed = subprocess.run(
        [str(arg) for arg in args],
        text=True,
        capture_output=True,
        check=True,
    )
    return completed.stdout.strip()


def main() -> int:
    root = Path(sys.argv[1]).resolve()
    outputs = [root / "ordered.pdf", root / "encrypted-merge.pdf"]
    missing = [str(path) for path in outputs if not path.is_file()]
    if missing:
        raise SystemExit(f"missing merge evidence outputs: {', '.join(missing)}")

    report = {
        "schema": 1,
        "qpdf_version": command("qpdf", "--version").splitlines()[0],
        "mutool_version": command("mutool", "-v").splitlines()[0],
        "outputs": {},
    }
    for output in outputs:
        command("qpdf", "--check", output)
        text_path = output.with_suffix(".txt")
        text_path.write_text(
            command("mutool", "draw", "-q", "-F", "txt", "-o", "-", output) + "\n",
            encoding="utf-8",
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
    (root / "report.json").write_text(serialized, encoding="utf-8")
    print(serialized, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
