#!/usr/bin/env python3
"""Run reproducible QPDF/MuPDF capability probes against deterministic fixtures."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shlex
import subprocess
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable, Sequence


@dataclass(frozen=True)
class CommandResult:
    probe_id: str
    command: list[str]
    exit_code: int
    expected_exit_codes: list[int]
    duration_ms: int
    stdout: str
    stderr: str

    @property
    def passed(self) -> bool:
        return self.exit_code in self.expected_exit_codes


class ProbeFailure(RuntimeError):
    pass


def clipped(text: str, limit: int = 4000) -> str:
    text = text.strip()
    if len(text) <= limit:
        return text
    return text[:limit] + f"\n... clipped {len(text) - limit} characters"


def run(
    probe_id: str,
    command: Sequence[str | Path],
    *,
    expected: Iterable[int] = (0,),
    display_command: Sequence[str] | None = None,
) -> CommandResult:
    argv = [str(part) for part in command]
    expected_codes = sorted(set(expected))
    started = time.monotonic()
    completed = subprocess.run(argv, text=True, capture_output=True, check=False)
    result = CommandResult(
        probe_id=probe_id,
        command=list(display_command) if display_command is not None else argv,
        exit_code=completed.returncode,
        expected_exit_codes=expected_codes,
        duration_ms=round((time.monotonic() - started) * 1000),
        stdout=clipped(completed.stdout),
        stderr=clipped(completed.stderr),
    )
    print(f"[{probe_id}] exit={result.exit_code} expected={expected_codes} :: {shlex.join(result.command)}")
    if result.stdout:
        print(result.stdout)
    if result.stderr:
        print(result.stderr)
    if not result.passed:
        raise ProbeFailure(
            f"{probe_id} returned {result.exit_code}; expected one of {expected_codes}"
        )
    return result


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def make_qdf(source: Path, work: Path, results: list[CommandResult]) -> Path:
    qdf = work / f"{source.stem}.qdf.pdf"
    results.append(
        run(
            f"qpdf.qdf.{source.stem}",
            ["qpdf", "--qdf", "--object-streams=disable", source, qdf],
        )
    )
    return qdf


def analyze_qdf(qdf: Path) -> dict[str, object]:
    data = qdf.read_bytes()
    objects = {
        int(match.group(1)): match.group(2).strip()
        for match in re.finditer(rb"(?ms)^(\d+) 0 obj\n(.*?)\nendobj", data)
    }
    outline_nodes = 0
    dangling_outline_destinations = 0
    for body in objects.values():
        if b"/Title" not in body:
            continue
        outline_nodes += 1
        destination = re.search(rb"/Dest\s*\[\s*(\d+) 0 R", body)
        if destination is None:
            continue
        target = objects.get(int(destination.group(1)), b"").strip()
        if target == b"null":
            dangling_outline_destinations += 1

    return {
        "has_outlines": b"/Outlines" in data,
        "outline_nodes": outline_nodes,
        "dangling_outline_destinations": dangling_outline_destinations,
        "has_acroform": b"/AcroForm" in data,
        "field_arrays": len(re.findall(rb"/Fields\s*\[", data)),
        "widget_annotations": len(re.findall(rb"/Subtype\s*/Widget", data)),
        "page_objects": len(re.findall(rb"/Type\s*/Page(?!s)\b", data)),
    }


def page_count(source: Path, probe_id: str, results: list[CommandResult]) -> int:
    result = run(probe_id, ["qpdf", "--show-npages", source])
    results.append(result)
    try:
        return int(result.stdout)
    except ValueError as error:
        raise ProbeFailure(f"{probe_id} returned non-integer page count: {result.stdout!r}") from error


def assert_equal(actual: object, expected: object, label: str) -> None:
    if actual != expected:
        raise ProbeFailure(f"{label}: expected {expected!r}, got {actual!r}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixtures", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()

    fixtures = arguments.fixtures.resolve()
    output = arguments.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    work = output / "work"
    render = output / "render"
    work.mkdir(parents=True, exist_ok=True)
    render.mkdir(parents=True, exist_ok=True)

    required = {
        "plain": fixtures / "plain-three-pages.pdf",
        "bookmarks": fixtures / "bookmarks.pdf",
        "acroform": fixtures / "acroform.pdf",
    }
    missing = [str(path) for path in required.values() if not path.is_file()]
    if missing:
        raise ProbeFailure(f"missing fixtures: {', '.join(missing)}")

    results: list[CommandResult] = []
    results.append(run("qpdf.version", ["qpdf", "--version"]))
    results.append(run("mupdf.version", ["mutool", "-v"]))

    expected_pages = {"plain": 3, "bookmarks": 3, "acroform": 1}
    observed_pages: dict[str, int] = {}
    for name, source in required.items():
        results.append(run(f"qpdf.check.{name}", ["qpdf", "--check", source]))
        observed_pages[name] = page_count(source, f"qpdf.pages.{name}", results)
        assert_equal(observed_pages[name], expected_pages[name], f"page count for {name}")
        results.append(run(f"mupdf.info.{name}", ["mutool", "info", source]))

        pattern = render / f"{name}-%d.png"
        results.append(
            run(
                f"mupdf.render.{name}",
                ["mutool", "draw", "-q", "-F", "png", "-r", "72", "-o", pattern, source],
            )
        )
        rendered = sorted(render.glob(f"{name}-*.png"))
        assert_equal(len(rendered), expected_pages[name], f"rendered page count for {name}")

    bookmarks_source = analyze_qdf(make_qdf(required["bookmarks"], work, results))
    form_source = analyze_qdf(make_qdf(required["acroform"], work, results))
    assert_equal(bookmarks_source["has_outlines"], True, "bookmark fixture outline")
    assert_equal(bookmarks_source["outline_nodes"], 3, "bookmark fixture node count")
    assert_equal(
        bookmarks_source["dangling_outline_destinations"],
        0,
        "bookmark fixture dangling destinations",
    )
    assert_equal(form_source["has_acroform"], True, "form fixture AcroForm")
    assert_equal(form_source["field_arrays"], 1, "form fixture field array")
    assert_equal(form_source["widget_annotations"], 1, "form fixture widget count")

    extracted = work / "extracted-page-2.pdf"
    results.append(
        run(
            "qpdf.extract",
            ["qpdf", required["plain"], "--pages", ".", "2", "--", extracted],
        )
    )
    assert_equal(page_count(extracted, "qpdf.pages.extracted", results), 1, "extract page count")

    merged = work / "merged.pdf"
    results.append(
        run(
            "qpdf.merge",
            [
                "qpdf",
                "--empty",
                "--pages",
                required["plain"],
                "1",
                required["bookmarks"],
                "2-3",
                "--",
                merged,
            ],
        )
    )
    assert_equal(page_count(merged, "qpdf.pages.merged", results), 3, "merged page count")

    rotated = work / "rotated.pdf"
    results.append(
        run(
            "qpdf.rotate",
            ["qpdf", required["plain"], "--rotate=+90:1", rotated],
        )
    )
    results.append(run("qpdf.check.rotated", ["qpdf", "--check", rotated]))
    results.append(
        run(
            "mupdf.render.rotated",
            ["mutool", "draw", "-q", "-F", "png", "-r", "72", "-o", render / "rotated-%d.png", rotated],
        )
    )

    encrypted = work / "encrypted.pdf"
    results.append(
        run(
            "qpdf.encrypt",
            [
                "qpdf",
                "--encrypt",
                "pincer-user",
                "pincer-owner",
                "256",
                "--",
                required["plain"],
                encrypted,
            ],
            display_command=[
                "qpdf",
                "--encrypt",
                "<redacted-user-password>",
                "<redacted-owner-password>",
                "256",
                "--",
                str(required["plain"]),
                str(encrypted),
            ],
        )
    )
    results.append(run("qpdf.encryption.info", ["qpdf", "--show-encryption", encrypted]))
    unlocked = run(
        "qpdf.encryption.correct_password",
        ["qpdf", "--password=pincer-user", "--show-npages", encrypted],
        display_command=["qpdf", "--password=<redacted>", "--show-npages", str(encrypted)],
    )
    results.append(unlocked)
    assert_equal(int(unlocked.stdout), 3, "encrypted page count")
    results.append(
        run(
            "qpdf.encryption.wrong_password",
            ["qpdf", "--password=wrong", "--show-npages", encrypted],
            expected=range(1, 256),
            display_command=["qpdf", "--password=<redacted-wronf~", "--show-npages", str(encrypted)],
        )
    )

    bookmark_subset = work / "bookmark-subset.pdf"
    results.append(
        run(
            "qpdf.bookmark_subset",
            ["qpdf", required["bookmarks"], "--pages", ".", "1-2", "--", bookmark_subset],
        )
    )
    form_subset = work / "form-subset.pdf"
    results.append(
        run(
            "qpdf.form_subset",
            ["qpdf", required["acroform"], "--pages", ".", "1", "--", form_subset],
        )
    )
    bookmarks_subset = analyze_qdf(make_qdf(bookmark_subset, work, results))
    form_subset = analyze_qdf(make_qdf(form_subset, work, results))
    merged_structure = analyze_qdf(make_qdf(merged, work, results))

    # These are deliberately asserted as observed behavior of the pinned engine.
    # Any future change must trigger an architectural review rather than silently
    # broadening or narrowing an advertised capability.
    assert_equal(bookmarks_subset["outline_nodes"], 3, "bookmark subset node count")
    assert_equal(
        bookmarks_subset["dangling_outline_destinations"],
        1,
        "bookmark subset dangling destinations",
    )
    assert_equal(merged_structure["has_outlines"], False, "merged outline preservation")
    assert_equal(form_subset["has_acroform"], True, "form subset AcroForm")
    assert_equal(form_subset["field_arrays"], 1, "form subset field array")
    assert_equal(form_subset["widget_annotations"], 1, "form subset widget count")

    structure = {
        "bookmarks_source": bookmarks_source,
        "bookmarks_subset": bookmarks_subset,
        "form_source": form_source,
        "form_subset": form_subset,
        "merged": merged_structure,
    }

    report = {
        "schema": 1,
        "fixtures": {
            name: {
                "path": str(path.relative_to(fixtures)),
                "bytes": path.stat().st_size,
                "sha256": sha256(path),
                "pages": observed_pages[name],
            }
            for name, path in required.items()
        },
        "structure": structure,
        "derived_outputs": {
            path.name: {"bytes": path.stat().st_size, "sha256": sha256(path)}
            for path in [extracted, merged, rotated, encrypted, bookmark_subset, form_subset]
        },
        "commands": [asdict(result) | {"passed": result.passed} for result in results],
        "summary": {
            "commands": len(results),
            "passed": sum(result.passed for result in results),
            "failed": sum(not result.passed for result in results),
        },
    }
    report_path = output / "report.json"
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"report={report_path}")
    print(json.dumps(report["summary"], sort_keys=True))
    print(json.dumps(structure, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ProbeFailure) as error:
        print(f"ERROR: {error}", file=__import__('sys').stderr)
        raise SystemExit(1) from error
