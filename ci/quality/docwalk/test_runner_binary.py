#!/usr/bin/env python3
"""Regression test: the doc-walk must run the binary it was given.

This exists because it did not. `run_doc_blocks(binary, blocks)` accepted `binary`,
reported it, and never used it — the sandbox inherited the ambient `PATH`, so every
doc block silently ran whatever `fastskill` happened to be installed on the machine
while the harness printed the path it believed it was testing.

That is the worst shape a bug can take in a quality tool: it produced confident,
plausible numbers about the wrong artifact, and matching numbers across two runs
looked like independent confirmation when both shared the same fault.

Stdlib only; run directly:

    python3 ci/quality/docwalk/test_runner_binary.py
"""

from __future__ import annotations

import pathlib
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from extract import Block  # noqa: E402
from runner import run_doc_blocks  # noqa: E402


def _block(raw: str) -> Block:
    return Block(
        index=0,
        line_start=1,
        line_end=1,
        lang="bash",
        filename=None,
        raw=raw,
        kind="command",
        run=True,
    )


def test_named_invocation_resolves_to_the_given_binary() -> None:
    """A doc saying `fastskill …` must reach the binary passed in, not one on PATH."""
    with tempfile.TemporaryDirectory() as tmp:
        fake = pathlib.Path(tmp) / "fastskill-under-test"
        fake.write_text("#!/bin/sh\necho I-AM-THE-BINARY-UNDER-TEST\n")
        fake.chmod(0o755)

        block = _block("fastskill --version")
        run_doc_blocks(str(fake), [block])

        assert block.output is not None, "block was never run"
        assert "I-AM-THE-BINARY-UNDER-TEST" in block.output, (
            "the doc's `fastskill` did not resolve to the binary passed in — the "
            f"harness is testing something else. Got: {block.output!r}"
        )
        assert block.exit_code == 0, f"expected exit 0, got {block.exit_code}"


def test_other_commands_still_resolve_normally() -> None:
    """Pinning `fastskill` must not break ordinary tools docs legitimately use."""
    with tempfile.TemporaryDirectory() as tmp:
        fake = pathlib.Path(tmp) / "fastskill"
        fake.write_text("#!/bin/sh\nexit 0\n")
        fake.chmod(0o755)

        block = _block("echo hello-from-echo")
        run_doc_blocks(str(fake), [block])

        assert "hello-from-echo" in (block.output or ""), (
            f"inherited PATH was lost; `echo` did not run. Got: {block.output!r}"
        )


def test_shim_is_not_created_inside_the_sandbox() -> None:
    """The shim must not appear in the working tree.

    The doc-walk judges what a doc's own steps create, so an unexpected directory in
    the sandbox would corrupt the signal the harness exists to produce.
    """
    with tempfile.TemporaryDirectory() as tmp:
        fake = pathlib.Path(tmp) / "fastskill"
        fake.write_text("#!/bin/sh\nexit 0\n")
        fake.chmod(0o755)

        block = _block("ls -a1")
        run_doc_blocks(str(fake), [block])

        listed = (block.output or "").split()
        unexpected = [
            name
            for name in listed
            if name not in (".", "..") and not name.startswith("_docwalk_block_")
        ]
        assert not unexpected, (
            f"sandbox should contain only the block script, found: {unexpected}"
        )


if __name__ == "__main__":
    failures = 0
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            try:
                fn()
                print(f"ok    {name}")
            except AssertionError as exc:
                failures += 1
                print(f"FAIL  {name}\n      {exc}")
    print("-" * 60)
    print("all passed" if not failures else f"{failures} failure(s)")
    raise SystemExit(1 if failures else 0)
