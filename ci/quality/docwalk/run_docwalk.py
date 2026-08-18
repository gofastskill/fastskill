#!/usr/bin/env python3
"""Blind doc-walk harness (spec 005, P3): does the doc still say what actually happens?

Three deterministic-then-LLM stages, and the split is the entire point (spec 005
section 2): a reader who "knows" the right command and silently fixes a doc's mistake
is the worst possible judge of whether that doc is broken. So the stages are:

    extract.py    deterministic  -- pull fenced code blocks out of the .mdx, in order,
                                     verbatim; classify + pre-filter, never edit.
    runner.py     deterministic  -- execute each runnable block exactly as written, in
                                     one sandbox per doc, capture output.
    classify.py   LLM            -- ONLY compares doc-claimed output vs actual output
                                     and labels the divergence (match/drift/broken). It
                                     never authors, repairs, or suggests a command.

This tier is **soft** (spec 005 Q3): a poor doc, drifted output, or a "broken" verdict
all exit 0 and show up in the report. Only a broken *harness* — no binary, or a gateway
that cannot be reached at all — exits 2. That mirrors `ci/quality/error_quality/run_suite.py`
exactly; see that module's docstring for the same design note in the sibling tier.

Usage:
    LLM_GATEWAY_URL=... LLM_GATEWAY_KEY=... LLM_GATEWAY_MODEL=... \\
        python3 ci/quality/docwalk/run_docwalk.py [--binary PATH] [--json OUT]
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
import sys
from collections import Counter

HERE = pathlib.Path(__file__).resolve().parent
REPO_ROOT = HERE.parent.parent.parent
DOCS_PATH = HERE / "docs.json"

sys.path.insert(0, str(HERE))
from extract import Block, extract_doc  # noqa: E402
from runner import run_doc_blocks  # noqa: E402
from classify import classify_block  # noqa: E402

sys.path.insert(0, str(HERE.parent / "error_quality"))
from judge import gateway_from_env  # noqa: E402

ANCHOR_LABELS = {
    "output-comment": "inline `# Output:` comment",
    "next-block": "the output shown directly under this command",
    "prose-hint": "prose claim elsewhere in the doc",
}


def find_binary(explicit: str | None) -> str | None:
    """Locate the binary under test — same order-of-precedence as error_quality's:
    explicit flag, then env, then a local debug/release build, then PATH last (PATH
    picking up an installed release while you believe you're testing HEAD is exactly
    the mistake a doc-walk exists to catch)."""
    for candidate in (explicit, os.environ.get("FASTSKILL_BIN")):
        if candidate:
            path = pathlib.Path(candidate)
            if path.is_file():
                return str(path.resolve())
            print(f"binary not found: {candidate}")
            return None
    for rel in ("target/debug/fastskill", "target/release/fastskill"):
        path = REPO_ROOT / rel
        if path.is_file():
            return str(path.resolve())
    found = shutil.which("fastskill")
    if found:
        print(f"warning: falling back to fastskill on PATH ({found}); "
              f"this may not be the build under test")
        return found
    print("no fastskill binary; pass --binary or set FASTSKILL_BIN")
    return None


def _claimed_text(block: Block) -> str:
    parts = []
    for a in block.anchors:
        label = ANCHOR_LABELS.get(a.kind, a.kind)
        parts.append(f"[{label}]\n{a.text}")
    return "\n\n".join(parts)


def process_doc(doc_rel: str, binary: str, gw, capture_only: bool, max_requests: int, trials: int):
    doc_path = REPO_ROOT / doc_rel
    extracted = extract_doc(str(doc_path))
    run_doc_blocks(binary, extracted.blocks)

    rows = []
    skipped_at_cap = []
    for block in extracted.blocks:
        row = {
            "doc": doc_rel,
            "index": block.index,
            "line_start": block.line_start,
            "line_end": block.line_end,
            "lang": block.lang,
            "kind": block.kind,
            "filename": block.filename,
            "run": block.run,
            "skip_reason": block.skip_reason,
            "notes": block.notes,
            "anchors": [{"kind": a.kind, "text": a.text} for a in block.anchors],
            "output": block.output,
            "exit_code": block.exit_code,
            "timed_out": block.timed_out,
            "classify_status": "not_applicable",
            "classify_verdict": None,
            "classify_quote": "",
            "classify_inconclusive": False,
        }

        if block.kind == "command" and block.run:
            if not block.anchors:
                row["classify_status"] = "no_anchor"
            elif capture_only:
                row["classify_status"] = "capture_only"
            elif gw is None:
                row["classify_status"] = "no_anchor"
            elif gw.requests_made + trials > max_requests:
                row["classify_status"] = "skipped_cap"
                skipped_at_cap.append(f"{doc_rel}#{block.index}")
            else:
                result = classify_block(
                    gw, block.index, block.command_text, _claimed_text(block),
                    block.output or "", block.exit_code, trials=trials,
                )
                if not result.trials:
                    row["classify_status"] = "classify_error"
                    row["classify_error"] = result.error
                elif result.inconclusive:
                    row["classify_status"] = "inconclusive"
                    row["classify_verdict"] = result.verdict
                    row["classify_quote"] = result.quote
                    row["classify_inconclusive"] = True
                else:
                    row["classify_status"] = result.verdict
                    row["classify_verdict"] = result.verdict
                    row["classify_quote"] = result.quote

        rows.append(row)

    prose_sections = [
        {"heading": p.heading, "line": p.line, "reason": p.reason}
        for p in extracted.prose_sections
    ]

    commands_total = sum(1 for b in extracted.blocks if b.kind == "command")
    commands_run = sum(1 for b in extracted.blocks if b.kind == "command" and b.run)
    commands_flagged = commands_total - commands_run
    file_bodies = sum(1 for b in extracted.blocks if b.kind == "file")
    prose_blocks = sum(
        1 for b in extracted.blocks if b.kind == "prose" and b.consumed_as_anchor_for is None
    )
    prose_consumed = sum(
        1 for b in extracted.blocks if b.kind == "prose" and b.consumed_as_anchor_for is not None
    )
    anchors_found = sum(len(b.anchors) for b in extracted.blocks)

    counts = {
        "blocks_total": len(extracted.blocks),
        "commands_total": commands_total,
        "commands_run": commands_run,
        "commands_flagged_human_review": commands_flagged,
        "file_bodies": file_bodies,
        "prose_blocks_flagged": prose_blocks,
        "prose_blocks_consumed_as_anchor": prose_consumed,
        "prose_sections_flagged": len(prose_sections),
        "anchors_found": anchors_found,
    }

    return {
        "doc": doc_rel,
        "counts": counts,
        "prose_sections": prose_sections,
        "blocks": rows,
    }, skipped_at_cap


def _print_doc_summary(doc_report: dict) -> None:
    c = doc_report["counts"]
    print(f"\n{doc_report['doc']}")
    print(
        f"  blocks={c['blocks_total']}  "
        f"commands: run={c['commands_run']} flagged={c['commands_flagged_human_review']}  "
        f"file-bodies={c['file_bodies']}  "
        f"prose: flagged={c['prose_blocks_flagged']} consumed-as-anchor={c['prose_blocks_consumed_as_anchor']}  "
        f"prose-sections-flagged={c['prose_sections_flagged']}  "
        f"anchors={c['anchors_found']}"
    )
    for row in doc_report["blocks"]:
        if row["kind"] == "command" and row["run"]:
            status = row["classify_status"]
            mark = {
                "match": "MATCH ", "drift": "DRIFT ", "broken": "BROKEN",
                "inconclusive": "UNSURE", "no_anchor": "ran   ", "capture_only": "capture",
                "skipped_cap": "SKIP  ", "classify_error": "ERROR ",
            }.get(status, status)
            exit_repr = "timeout" if row["timed_out"] else row["exit_code"]
            print(f"    [{block_tag(row)}] {mark:<8} exit={exit_repr} line={row['line_start']}")
            if status in ("drift", "broken") and row["classify_quote"]:
                print(f"             └─ {row['classify_quote'][:160]}")
        elif row["kind"] == "command" and not row["run"]:
            print(f"    [{block_tag(row)}] FLAGGED line={row['line_start']} — {row['skip_reason']}")
        elif row["kind"] == "file":
            print(f"    [{block_tag(row)}] FILE    line={row['line_start']} — {row['skip_reason']}")
        elif row["kind"] == "prose":
            if row["skip_reason"] and row["skip_reason"].startswith("consumed"):
                continue  # not independently interesting; already shown as an anchor
            print(f"    [{block_tag(row)}] PROSE   line={row['line_start']} — {row['skip_reason']}")
    for p in doc_report["prose_sections"]:
        print(f"    [section] line={p['line']} {p['heading']!r} — {p['reason']}")


def block_tag(row: dict) -> str:
    return f"#{row['index']:>2}"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--binary", default=None)
    ap.add_argument("--json", default=None, help="write full results here")
    ap.add_argument("--summary", default=os.environ.get("GITHUB_STEP_SUMMARY"))
    ap.add_argument("--trials", type=int, default=3)
    ap.add_argument("--max-requests", type=int, default=300,
                    help="hard cap; past it remaining classifications are skipped AND logged")
    ap.add_argument("--capture-only", action="store_true",
                    help="extract + run commands, print captures, make NO gateway calls")
    args = ap.parse_args()

    binary = find_binary(args.binary)
    if binary is None:
        return 2
    docs = json.loads(DOCS_PATH.read_text())["docs"]
    print(f"binary  {binary}")
    print(f"docs    {len(docs)}")

    gw = None
    if not args.capture_only:
        try:
            gw = gateway_from_env()
        except SystemExit as exc:
            print(f"cannot run: {exc}")
            return 2
        print(f"model   {gw.model}")
        # Preflight: prove the gateway is actually reachable before spending a whole
        # run's worth of docs against it. Mirrors gateway_smoke.py's "fail loudly
        # here is the point" — a broken harness (per the interface contract) means
        # unreachable, not merely "misclassified one block".
        try:
            gw.complete("Reply with the single word OK.", max_tokens=512)
        except RuntimeError as exc:
            print(f"cannot run: gateway unreachable — {exc}")
            return 2
    print("-" * 78)

    doc_reports = []
    all_skipped = []
    for doc_rel in docs:
        report, skipped = process_doc(doc_rel, binary, gw, args.capture_only, args.max_requests, args.trials)
        doc_reports.append(report)
        all_skipped.extend(skipped)
        _print_doc_summary(report)

    print("-" * 78)
    totals = Counter()
    verdicts = Counter()
    for r in doc_reports:
        for k, v in r["counts"].items():
            totals[k] += v
        for row in r["blocks"]:
            if row["classify_status"] in ("match", "drift", "broken", "inconclusive"):
                verdicts[row["classify_status"]] += 1
    print(
        f"totals  blocks={totals['blocks_total']}  "
        f"commands run={totals['commands_run']} flagged={totals['commands_flagged_human_review']}  "
        f"file-bodies={totals['file_bodies']}  "
        f"prose flagged={totals['prose_blocks_flagged']}+sections={totals['prose_sections_flagged']}  "
        f"anchors={totals['anchors_found']}"
    )
    if gw:
        print(
            f"judge   requests={gw.requests_made}  "
            f"match={verdicts['match']} drift={verdicts['drift']} broken={verdicts['broken']} "
            f"inconclusive={verdicts['inconclusive']}"
        )
    if all_skipped:
        print(f"\nSKIPPED {len(all_skipped)} classification(s) at the {args.max_requests}-request "
              f"cap: {', '.join(all_skipped)}")

    if args.json:
        pathlib.Path(args.json).write_text(json.dumps(
            {
                "binary": binary,
                "model": gw.model if gw else None,
                "capture_only": args.capture_only,
                "requests_made": gw.requests_made if gw else 0,
                "totals": dict(totals),
                "verdicts": dict(verdicts),
                "skipped_at_cap": all_skipped,
                "docs": doc_reports,
            },
            indent=2,
        ))

    if args.summary:
        lines = ["## Blind doc-walk", ""]
        lines.append(
            f"Binary: `{binary}` · {len(docs)} doc(s) · "
            + (f"judge `{gw.model}` · {gw.requests_made} requests" if gw else "capture-only (no gateway calls)")
        )
        lines += ["", "| Doc | Blocks | Run | Flagged (human-review) | File bodies | Anchors |",
                  "|---|---|---|---|---|---|"]
        for r in doc_reports:
            c = r["counts"]
            lines.append(
                f"| `{r['doc']}` | {c['blocks_total']} | {c['commands_run']} | "
                f"{c['commands_flagged_human_review'] + c['prose_blocks_flagged'] + c['prose_sections_flagged']} | "
                f"{c['file_bodies']} | {c['anchors_found']} |"
            )
        if gw:
            lines += ["", f"Verdicts: match={verdicts['match']} · drift={verdicts['drift']} · "
                          f"broken={verdicts['broken']} · inconclusive={verdicts['inconclusive']}"]
            drifted = [
                (r["doc"], row) for r in doc_reports for row in r["blocks"]
                if row["classify_status"] in ("drift", "broken")
            ]
            if drifted:
                lines += ["", "### Divergence found", ""]
                for doc_rel, row in drifted:
                    lines += [
                        f"**`{doc_rel}` block #{row['index']}** (line {row['line_start']}, "
                        f"`{row['classify_status']}`)",
                        "", "```", row["output"][:600] if row["output"] else "(no output)", "```",
                        "", f"> {row['classify_quote'][:300]}", "",
                    ]
        if all_skipped:
            lines += ["", f"_Skipped at the request cap: {', '.join(all_skipped)}_"]
        with open(args.summary, "a") as fh:
            fh.write("\n".join(lines) + "\n")

    # Soft tier: divergence verdicts never fail the run. Only a broken harness does,
    # and that already returned 2 above.
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
