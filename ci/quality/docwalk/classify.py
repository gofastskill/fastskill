#!/usr/bin/env python3
"""LLM classifier for the blind doc-walk harness (spec 005, P3).

This is the ONLY place an LLM appears in the whole doc-walk pipeline, and its job is
narrow on purpose: compare what a doc *claims* the output will be against what the
sandboxed run actually produced, and label the divergence. It never sees a broken
command and suggests a fix — there is no "corrected command" field anywhere in the
schema below, and the prompt tells the model outright not to try.

Reuses `judge.py`'s `Gateway`, `strip_reasoning`, and `extract_json` — the gateway
client, `<think>`-block stripping, and tolerant JSON parsing are identical concerns to
the error-quality judge (same gateway, same reasoning-model quirks) and are not
reimplemented here.
"""

from __future__ import annotations

from collections import Counter
from dataclasses import dataclass, field

import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "error_quality"))
from judge import Gateway, extract_json  # noqa: E402

VERDICTS = ("match", "drift", "broken")

RUBRIC = """\
You are checking whether a documentation example's CLAIMED output matches what
ACTUALLY happened when the exact commands from the doc were run verbatim in a fresh
sandbox.

You are NOT being asked whether the command is well-written, idiomatic, or how you
would fix it. Do not propose a different command, a corrected command, or any kind of
fix. Only compare the two texts you are given and classify the divergence between
them.

Classify as exactly one of:
  match  - the actual output is consistent with what the doc claims. Minor
           formatting, whitespace, ordering, or timestamp differences still count as
           a match.
  drift  - the command(s) ran, but the actual output differs in SUBSTANCE from what
           the doc claims (different values, different fields, wording that changes
           the meaning) — the workflow basically works but the doc's example is
           stale.
  broken - the actual output shows the documented command failing, erroring, or
           crashing in a way the doc's claim does not anticipate — the doc's
           instructions do not work as written.

Reply with ONLY a JSON object, no prose and no code fence:
{"verdict": "match"|"drift"|"broken", "quote": "<the exact actual-output text that diverges from the claim, or empty string if match>"}
"""


@dataclass
class Trial:
    verdict: str
    quote: str


@dataclass
class ClassifyResult:
    block_index: int
    trials: list[Trial] = field(default_factory=list)
    verdict: str | None = None      # majority verdict; None when no majority at all
    inconclusive: bool = False
    quote: str = ""
    error: str | None = None


def build_prompt(command_text: str, claimed: str, actual: str, exit_code: int | None) -> str:
    return (
        f"{RUBRIC}\n"
        f"Command(s) exactly as written in the doc:\n---\n{command_text}\n---\n\n"
        f"Doc's claimed output:\n---\n{claimed}\n---\n\n"
        f"Actual output (exit code {exit_code}):\n---\n{actual}\n---\n"
    )


def classify_block(
    gw: Gateway, block_index: int, command_text: str, claimed: str, actual: str,
    exit_code: int | None, trials: int = 3,
) -> ClassifyResult:
    """Classify one block over N trials and take the majority verdict.

    Same reasoning as `judge.judge_case`: the pinned model reasons
    non-deterministically, so a single call is not reproducible. Unlike the binary
    pass/fail judge, this is a 3-way classification, so "majority" is stricter here —
    only a unanimous 3/3 counts as decisive. A 2-1 split is flagged `inconclusive`
    (spec 005 Q5's "bare 2-1" rule, generalized from 2 categories to 3) but still
    reports the leading label so a human has a lead to follow, not just a shrug.
    """
    result = ClassifyResult(block_index=block_index)
    prompt = build_prompt(command_text, claimed, actual, exit_code)
    for _ in range(trials):
        try:
            raw = gw.complete(prompt, max_tokens=1200)
            data = extract_json(raw)
            verdict = str(data["verdict"]).strip().lower()
            if verdict not in VERDICTS:
                raise ValueError(f"unexpected verdict {verdict!r}")
            result.trials.append(Trial(verdict=verdict, quote=str(data.get("quote", "")).strip()))
        except (RuntimeError, ValueError, KeyError, TypeError) as exc:
            result.error = f"{type(exc).__name__}: {exc}"

    if not result.trials:
        return result

    votes = Counter(t.verdict for t in result.trials)
    top_verdict, top_count = votes.most_common(1)[0]
    quotes = [t.quote for t in result.trials if t.verdict == top_verdict and t.quote]

    if top_count == len(result.trials) and len(votes) == 1:
        result.verdict = top_verdict
        result.quote = quotes[0] if quotes else ""
    else:
        result.inconclusive = True
        result.verdict = top_verdict  # leaning, not decisive — caller must check flag
        result.quote = quotes[0] if quotes else ""
    return result
