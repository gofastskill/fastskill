#!/usr/bin/env python3
"""Deterministic runner for the blind doc-walk harness (spec 005, P3).

Executes exactly what `extract.py` pulled out of the doc — no repair, no
substitution, no "obviously they meant". One sandbox **per doc**, not per block: a
real reader walks a doc top to bottom in one working directory, so a manifest a doc
creates in step 2 has to still be there for step 4. Isolating every block into its own
throwaway sandbox (the way `ci/quality/error_quality` does per case) would make an
internally-consistent multi-step doc look broken for no reason.

Stdlib only.
"""

from __future__ import annotations

import os
import pathlib
import subprocess
import tempfile

from extract import Block

COMMAND_TIMEOUT = 60


def run_doc_blocks(binary: str, blocks: list[Block]) -> None:
    """Walk a doc's blocks in order in one persistent sandbox, mutating them in place.

    Sets on each executed/staged block:
        - block.output / block.exit_code / block.timed_out   (kind == "command", run)
        - nothing on blocks that are never run — their `skip_reason` already explains
          why, set by the extractor.

    Deliberately does **not** pre-seed the sandbox with anything beyond an empty
    directory. Extending the setup on the harness's own initiative (e.g. `mkdir
    skills/` because a later `add` will need it) would be exactly the kind of silent
    self-correction the whole design exists to avoid — if a doc's own steps do not
    create a directory it later depends on, that is a real finding, not a harness bug.
    """
    with tempfile.TemporaryDirectory(prefix="fs-docwalk-") as tmp, tempfile.TemporaryDirectory(
        prefix="fs-docwalk-bin-"
    ) as bin_tmp:
        root = pathlib.Path(tmp)

        # Docs invoke `fastskill` by NAME, so `binary` has to reach the subprocess as
        # that name or the resolved build is never actually exercised. Expose it via a
        # shim directory placed FIRST on PATH.
        #
        # A shim rather than prepending the binary's own directory, because `--binary`
        # may point at a differently-named file (`fastskill-custom`, a release artifact)
        # and the docs would still say `fastskill`.
        #
        # The shim lives OUTSIDE the sandbox on purpose: the doc-walk judges what a
        # doc's own steps create, so an extra directory inside the working tree would
        # corrupt exactly the signal this harness exists to produce.
        shim_dir = pathlib.Path(bin_tmp)
        shim = shim_dir / "fastskill"
        shim.symlink_to(pathlib.Path(binary).resolve())

        # PATH is still inherited beyond the shim: docs legitimately use `ls`, `cat`,
        # `curl`. Only `fastskill` itself is pinned.
        env = {
            "PATH": f"{shim_dir}{os.pathsep}{os.environ.get('PATH', '')}",
            "HOME": str(root),
            "TERM": "dumb",
        }

        for block in blocks:
            if block.kind == "file":
                if block.filename:
                    target = root / block.filename
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_text(block.raw)
                    block.notes.append(f"written to sandbox as {block.filename!r}")
                continue

            if block.kind != "command" or not block.run:
                continue

            script = root / f"_docwalk_block_{block.index}.sh"
            script.write_text(block.raw)
            try:
                proc = subprocess.run(
                    ["bash", str(script)],
                    cwd=root,
                    capture_output=True,
                    text=True,
                    timeout=COMMAND_TIMEOUT,
                    env=env,
                    # Verbatim execution of a doc means no human is present to answer
                    # a prompt. An interactive command (e.g. `fastskill init` with no
                    # `--yes`) hits EOF immediately, exactly as it would for anyone
                    # who actually piped this doc into a shell non-interactively.
                    stdin=subprocess.DEVNULL,
                )
                out, err, code, timed_out = proc.stdout, proc.stderr, proc.returncode, False
            except subprocess.TimeoutExpired:
                out, err, code, timed_out = "", "", None, True

            combined = "\n".join(part.strip() for part in (out, err) if part.strip())
            combined = combined.replace(str(root), "<sandbox>")
            block.output = combined
            block.exit_code = code
            block.timed_out = timed_out
