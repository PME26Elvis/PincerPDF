­r‡^Ñf¥–Ø¦{n,yÊ'vÃ®¶›­#!/usr/bin/env python3
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


def outline_summary(document: dict[str, object]) -> list[dict[str, object]]:
    """Retain the stable recursive outline semantics from QPDF JSON."""
    outlines = document.get("outlines")
    if not isinstance(outlines, list):
        raise RuntimeError("QPDF outline JSON omitted the outlines array")

    def summarize(entries: list[object]) -> list[dict[str, object]]:
        result: list[dict[str, object]] = []
        for entry in entries:
            if not isinstance(entry, dict):
                raise RuntimeError("QPDF outline JSON contained a non-object item")
            children = entry.get("kids")
            if not isinstance(children, list):
                raise RuntimeError("QPDF outline JSON omitted bookmark children")
            result.append(
                {
                    "title": entry["title"],
                    "page": entry.get("destpageposfrom1"),
                    "children": summarize(children),
                }
            )
        return result

    return summarize(outlines)


def main() -> int:
    root = Path(sys.argv[1]).resolve()
    parity_output = (
        root
        / "è¼¸å‡º-merge-parity-long-path-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        / "å¹¾ä½•-metadata.pdf"
    )
    bookmark_output = root / "one-entry-bookmarks.pdf"
    retained_output = root / "retained-source-bookmarks.pdf"
    grouped_output = root / "retained-under-document-bookmarks.pdf"
    outputs = [
        root / "ordered.pdf",
        root / "encrypted-merge.pdf",
        bookmark_output,
        retained_output,
        grouped_output,
        parity_output,
    ]
    missing = [str(path) for path in outputs if not path.is_file()]
    if missing:
        raise SystemExit(f"missing merge evidence outputs: {', '.join(missing)}")

    report = {
        "schema": 3,
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

    report["parity"] = {
        "relative_path": str(parity_output.relative_to(root)).replace("\\", "/"),
        "info": command("mutool", "show", parity_output, "trailer/Info"),
        "pages": [
            {
                "media_box": command(
                    "mutool", "show", parity_output, f"pages/{page}/MediaBox"
                ),
                "crop_box": command(
                    "mutool", "show", parity_output, f"pages/{page}/CropBox"
                ),
                "rotate": command(
                    "mutool", "show", parity_output, f"pages/{page}/Rotate"
                ),
            }
            for page in range(1, 6)
        ],
    }
    report["bookmark_policies"] = {
        mode: outline_summary(
            json.loads(command("qpdf", "--json=2", "--json-key=outlines", output))
        )
        for mode, output in (
            ("one_entry_per_document", bookmark_output),
            ("retain", retained_output),
            ("retain_as_one_entry_per_document", grouped_output),
        )
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
