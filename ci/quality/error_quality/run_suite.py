#!/usr/bin/env python3
"""Run fastskill's error paths and judge the messages (spec 005, P2).

The *capture* is deterministic: each case runs a real command against a real binary in a
disposable sandbox. Only the *scoring* is LLM. That split is the whole design — an
assertion-based test would have to hard-code wording and would fail on every harmless
rephrase, which is why these steps were left to a human in the first place.

This tier is **soft** (spec 005): it reports, it does not gate. A judge verdict is
advisory, so a poor message exits 0 and shows up in the report. Only a broken *harness*
(no binary, unreachable gateway) is a real failure.

Usage:
    LLM_GATEWAY_URL=... LLM_GATEWAY_KEY=... LLM_GATEWAY_MODEL=... \\
        python3 ci/quality/error_quality/run_suite.py [--binary PATH] [--json OUT]
                                                     [--summary FILE] [--max-requests N]
                                                     [--capture-only]

Exit codes: 0 = ran (regardless of verdicts), 2 = could not run.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from judge import gateway_from_env, judge_case  # noqa: E402

HERE = pathlib.Path(__file__).resolve().parent
SUITE_PATH = HERE / "suite.json"
COMMAND_TIMEOUT = 60

MANIFESTS = {
    # A valid project manifest: enough for commands to resolve a project root.
    "default": (
        '[metadata]\nid = "quality-sandbox"\nversion = "1.0.0"\n\n'
        '[tool.fastskill]\nskills_directory = "skills"\n\n[dependencies]\n'
    ),
    # Deliberately missing [dependencies] — drives the config-validation error path.
    "minimal_no_dependencies": (
        '[metadata]\nid = "quality-sandbox"\nversion = "1.0.0"\n\n'
        '[tool.fastskill]\nskills_directory = "skills"\n'
    ),
}


def find_binary(explicit: str | None) -> str:
    """Locate the binary under test.

    Order matters: an explicit path wins, then the env var CI sets, then a local debug
    build, then PATH. PATH is LAST on purpose — picking up an installed release while
    believing you tested the build under review is exactly the mistake this tier exists
    to avoid making about error messages.
    """
    for candidate in (explicit, os.environ.get("FASTSKILL_BIN")):
        if candidate:
            path = pathlib.Path(candidate)
            if path.is_file():
                return str(path.resolve())
            raise SystemExit(f"binary not found: {candidate}")

    for rel in ("target/debug/fastskill", "target/release/fastskill"):
        path = pathlib.Path(rel)
        if path.is_file():
            return str(path.resolve())

    found = shutil.which("fastskill")
    if found:
        print(f"warning: falling back to fastskill on PATH ({found}); "
              f"this may not be the build under test")
        return found
    raise SystemExit("no fastskill binary; pass --binary or set FASTSKILL_BIN")


def prepare_sandbox(root: pathlib.Path, setup: dict) -> None:
    """Build a disposable project for one case. Never reuses state between cases."""
    (root / "skills").mkdir(parents=True, exist_ok=True)
    manifest = MANIFESTS[setup.get("manifest", "default")]
    (root / "skill-project.toml").write_text(manifest)
    for name in setup.get("mkdir", []):
        (root / name).mkdir(parents=True, exist_ok=True)


def run_case(binary: str, case: dict) -> dict:
    """Execute one case and capture what a user would see."""
    with tempfile.TemporaryDirectory(prefix="fs-quality-") as tmp:
        root = pathlib.Path(tmp)
        prepare_sandbox(root, case.get("setup", {}))
        try:
            proc = subprocess.run(
                [binary, *case["command"]],
                cwd=root,
                capture_output=True,
                text=True,
                timeout=COMMAND_TIMEOUT,
                # A bare environment except PATH/HOME: a stray OPENAI_API_KEY or
                # FASTSKILL_* in the ambient environment would change which error path
                # runs, making the captured message depend on the machine.
                env={"PATH": os.environ.get("PATH", ""), "HOME": str(root)},
            )
            out, err, code, timed_out = proc.stdout, proc.stderr, proc.returncode, False
        except subprocess.TimeoutExpired:
            out, err, code, timed_out = "", "", None, True

    # Users see one stream. Judge what they see, not what the plumbing separated.
    combined = "\n".join(part.strip() for part in (out, err) if part.strip())
    # Sandbox paths are noise and differ per run; keep the message stable across runs.
    combined = combined.replace(str(root), "<project>")
    return {
        "command": "fastskill " + " ".join(case["command"]),
        "output": combined,
        "exit_code": code,
        "timed_out": timed_out,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", default=None)
    ap.add_argument("--json", default=None, help="write full results here")
    ap.add_argument("--summary", default=os.environ.get("GITHUB_STEP_SUMMARY"))
    ap.add_argument("--trials", type=int, default=3)
    ap.add_argument("--max-requests", type=int, default=300,
                    help="hard cap; past it remaining cases are skipped AND logged")
    ap.add_argument("--capture-only", action="store_true",
                    help="run commands and print captures without calling the gateway")
    args = ap.parse_args()

    cases = json.loads(SUITE_PATH.read_text())["cases"]
    binary = find_binary(args.binary)
    print(f"binary  {binary}")
    print(f"cases   {len(cases)}")

    gw = None
    if not args.capture_only:
        try:
            gw = gateway_from_env()
            print(f"model   {gw.model}")
        except SystemExit as exc:
            print(f"cannot run: {exc}")
            return 2
    print("-" * 78)

    rows, skipped = [], []
    for case in cases:
        captured = run_case(binary, case)

        # A case that succeeds did not produce the error it exists to grade. Judging the
        # success output would score the wrong thing and quietly weaken the suite.
        if captured["timed_out"]:
            print(f"BROKEN   {case['id']:<34} command timed out after {COMMAND_TIMEOUT}s")
            rows.append({"id": case["id"], "status": "timeout", **captured})
            continue
        if captured["exit_code"] == 0:
            print(f"BROKEN   {case['id']:<34} exited 0 — expected an error to grade")
            rows.append({"id": case["id"], "status": "unexpected_success", **captured})
            continue

        if args.capture_only:
            first = captured["output"].splitlines()[0] if captured["output"] else "(no output)"
            print(f"capture  {case['id']:<34} exit={captured['exit_code']} {first[:60]}")
            rows.append({"id": case["id"], "status": "captured", **captured})
            continue

        if gw.requests_made + args.trials > args.max_requests:
            skipped.append(case["id"])
            continue

        result = judge_case(gw, {"id": case["id"], **captured}, trials=args.trials)
        if not result.verdicts:
            status = "error"
            print(f"ERROR    {case['id']:<34} {result.error}")
        else:
            status = "pass" if result.passed else "poor"
            mark = "ok  " if result.passed else "POOR"
            flag = " (inconclusive)" if result.inconclusive else ""
            print(f"{mark:<8} {case['id']:<34} score={result.score:.1f}{flag}")
            if not result.passed:
                print(f"         └─ {result.verdicts[0].justification[:110]}")
        rows.append({
            "id": case["id"], "status": status, **captured,
            "score": result.score, "passed": result.passed,
            "inconclusive": result.inconclusive,
            "justifications": [v.justification for v in result.verdicts],
        })

    # Silent truncation would read as "everything was covered". Name what was dropped.
    if skipped:
        print(f"\nSKIPPED {len(skipped)} case(s) at the {args.max_requests}-request cap: "
              f"{', '.join(skipped)}")

    poor = [r for r in rows if r.get("status") == "poor"]
    broken = [r for r in rows if r.get("status") in ("timeout", "unexpected_success")]
    print("-" * 78)
    print(f"judged {len([r for r in rows if r.get('status') in ('pass', 'poor')])}  "
          f"poor {len(poor)}  broken {len(broken)}  skipped {len(skipped)}"
          + (f"  requests {gw.requests_made}" if gw else ""))

    if args.json:
        pathlib.Path(args.json).write_text(json.dumps(
            {"binary": binary, "model": gw.model if gw else None,
             "poor": len(poor), "broken": len(broken), "skipped": skipped,
             "cases": rows}, indent=2))

    if args.summary:
        lines = ["## Error-message quality", ""]
        if gw:
            lines.append(f"Judge: `{gw.model}` · {gw.requests_made} requests")
        lines += ["", "| Case | Verdict | Score |", "|---|---|---|"]
        for r in rows:
            verdict = {"pass": "ok", "poor": "**poor**"}.get(r.get("status"), r.get("status"))
            score = f"{r['score']:.1f}" if r.get("score") is not None else "—"
            lines.append(f"| `{r['id']}` | {verdict} | {score} |")
        if poor:
            lines += ["", "### Messages judged poor", ""]
            for r in poor:
                lines += [f"**`{r['id']}`** — `{r['command']}`", "",
                          "```", r["output"][:600], "```", "",
                          f"> {(r.get('justifications') or [''])[0][:300]}", ""]
        if skipped:
            lines += ["", f"_Skipped at the request cap: {', '.join(skipped)}_"]
        with open(args.summary, "a") as fh:
            fh.write("\n".join(lines) + "\n")

    # Soft tier: verdicts never fail the run. Only a broken harness does.
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
