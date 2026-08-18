#!/usr/bin/env python3
"""Calibrate the error-quality judge against human verdicts (spec 005, P2).

This is the go/no-go gate. Spec 005 Q4 requires a model change to pass calibration
before taking effect; running it FIRST answers the prior question -- whether the pinned
model can grade error-message quality at all -- before a harness is built around it.

Agreement is measured on pass/fail, not on the exact score: two reasonable graders
routinely differ by a point while agreeing on whether a message is acceptable.

Usage:
    LLM_GATEWAY_URL=... LLM_GATEWAY_KEY=... LLM_GATEWAY_MODEL=... \\
        python3 ci/quality/error_quality/calibrate.py [--trials N] [--json OUT]

Exit codes: 0 = calibrated, 1 = below threshold, 2 = could not run.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from judge import gateway_from_env, judge_case  # noqa: E402

CASES_PATH = pathlib.Path(__file__).resolve().parent / "cases.json"

# Below this the judge is not trustworthy enough to report findings a human will act on.
# Deliberately demanding: a judge that is wrong a third of the time generates review
# work rather than saving it, and the fixture is small enough that each case matters.
AGREEMENT_THRESHOLD = 0.80


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--trials", type=int, default=3, help="trials per case (majority vote)")
    ap.add_argument("--json", type=str, default=None, help="write full results here")
    args = ap.parse_args()

    cases = json.loads(CASES_PATH.read_text())["cases"]
    try:
        gw = gateway_from_env()
    except SystemExit as exc:
        print(f"cannot run: {exc}")
        return 2

    # A fixture skewed to one verdict can be gamed by a judge that always answers the
    # same way, so state the balance up front and make an always-X baseline visible.
    expected_pass = sum(1 for c in cases if c["expected"]["pass"])
    print(f"model     {gw.model}")
    print(f"cases     {len(cases)} ({expected_pass} pass / {len(cases) - expected_pass} fail)")
    print(f"trials    {args.trials} per case, majority vote")
    baseline = max(expected_pass, len(cases) - expected_pass) / len(cases)
    print(f"baseline  {baseline:.0%} (always answering the more common verdict)")
    print("-" * 78)

    agree = disagree = inconclusive = errored = 0
    rows = []

    for case in cases:
        want = case["expected"]["pass"]
        res = judge_case(gw, case, trials=args.trials)

        if not res.verdicts:
            status, errored = "ERROR", errored + 1
        elif res.inconclusive and res.passed is None:
            status, inconclusive = "SPLIT", inconclusive + 1
        else:
            matched = res.passed == want
            if res.inconclusive:
                # A bare majority counts toward agreement but is flagged: it is a weak
                # signal, and a judge that is only ever barely right is not reliable.
                status = "agree?" if matched else "DIFFER?"
            else:
                status = "agree" if matched else "DIFFER"
            if matched:
                agree += 1
            else:
                disagree += 1

        got = "-" if res.passed is None else ("pass" if res.passed else "fail")
        score = f"{res.score:.1f}" if res.score is not None else " - "
        print(f"{status:<8} {case['id']:<34} want={'pass' if want else 'fail'} got={got:<4} score={score}")
        if res.error:
            print(f"         └─ {res.error}")
        elif status.startswith(("DIFFER", "SPLIT")) and res.verdicts:
            print(f"         └─ judge said: {res.verdicts[0].justification[:100]}")

        rows.append(
            {
                "id": case["id"],
                "expected_pass": want,
                "got_pass": res.passed,
                "mean_score": res.score,
                "inconclusive": res.inconclusive,
                "error": res.error,
                "justifications": [v.justification for v in res.verdicts],
            }
        )

    scored = agree + disagree
    agreement = agree / scored if scored else 0.0
    print("-" * 78)
    print(f"agreement {agreement:.0%} ({agree}/{scored})   split={inconclusive} errored={errored}")
    print(f"requests  {gw.requests_made}")

    if args.json:
        pathlib.Path(args.json).write_text(
            json.dumps(
                {
                    "model": gw.model,
                    "agreement": agreement,
                    "agree": agree,
                    "disagree": disagree,
                    "inconclusive": inconclusive,
                    "errored": errored,
                    "baseline": baseline,
                    "threshold": AGREEMENT_THRESHOLD,
                    "cases": rows,
                },
                indent=2,
            )
        )

    if errored == len(cases):
        print("\nVERDICT: could not run — every case errored.")
        return 2
    if agreement >= AGREEMENT_THRESHOLD and agreement > baseline:
        print(f"\nVERDICT: CALIBRATED (>= {AGREEMENT_THRESHOLD:.0%} and above baseline).")
        return 0
    if agreement <= baseline:
        # Beating the threshold while merely matching the baseline means the judge has
        # learned the fixture's skew, not the rubric.
        print(f"\nVERDICT: NOT CALIBRATED — {agreement:.0%} is no better than always "
              f"answering the same way ({baseline:.0%}).")
    else:
        print(f"\nVERDICT: NOT CALIBRATED — {agreement:.0%} < {AGREEMENT_THRESHOLD:.0%}.")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
