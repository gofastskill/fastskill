#!/usr/bin/env python3
"""Require full-file line coverage for production Rust files changed from a base ref."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


def fail(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def main() -> None:
    if len(sys.argv) != 4:
        fail("usage: check-modified-rust-coverage.py BASE_REF COVERAGE_JSON MIN_PERCENT")

    base_ref, report_name, minimum_text = sys.argv[1:]
    try:
        minimum = float(minimum_text)
    except ValueError:
        fail(f"invalid minimum percentage: {minimum_text}")

    root = Path(
        subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    ).resolve()
    changed_output = subprocess.run(
        [
            "git",
            "diff",
            "--name-only",
            "--diff-filter=ACMRT",
            f"{base_ref}...HEAD",
            "--",
            "crates/*/src/*.rs",
            "crates/*/src/**/*.rs",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    changed = sorted(
        line
        for line in changed_output.splitlines()
        if line
        and not Path(line).name.endswith("tests.rs")
        and "/tests/" not in line
    )

    report = json.loads(Path(report_name).read_text(encoding="utf-8"))
    if not report.get("data"):
        fail("coverage report contains no data")

    coverage: dict[str, tuple[int, int, float]] = {}
    for item in report["data"][0].get("files", []):
        path = Path(item["filename"]).resolve()
        try:
            relative = path.relative_to(root).as_posix()
        except ValueError:
            continue
        lines = item["summary"]["lines"]
        coverage[relative] = (lines["covered"], lines["count"], lines["percent"])

    if not changed:
        print("No modified production Rust files")
        return

    failures: list[str] = []
    print(f"Modified production Rust file coverage (required: > {minimum:g}%):")
    for name in changed:
        measured = coverage.get(name)
        if measured is None:
            # llvm-cov omits files that contain no instrumentable regions, such
            # as module roots made entirely of `mod` and `pub use` declarations.
            # The all-features workspace run links every production crate, so a
            # compiled executable region with no hits is reported as 0% rather
            # than omitted.
            print(f"  PASS {name}: N/A (no instrumentable lines)")
            continue
        covered, count, percent = measured
        passed = percent > minimum
        print(f"  {'PASS' if passed else 'FAIL'} {name}: {percent:.2f}% ({covered}/{count})")
        if not passed:
            failures.append(f"{name}: {percent:.2f}% ({covered}/{count})")

    if failures:
        fail(
            "modified files must have line coverage above "
            f"{minimum:g}%:\n  " + "\n  ".join(failures)
        )


if __name__ == "__main__":
    main()
