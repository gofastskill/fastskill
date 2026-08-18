#!/usr/bin/env python3
"""LLM judge for fastskill error-message quality (spec 005, P2).

Scores a captured (command, output, exit_code) against a fixed rubric and returns a
structured verdict. The *capture* is deterministic; only the *scoring* is LLM.

Stdlib only, so this runs before any dependency install.

Two behaviours here exist because of things measured on the real gateway, not from
theory -- see `strip_reasoning` and `judge_case`.
"""

from __future__ import annotations

import json
import os
import re
import urllib.error
import urllib.request
from collections import Counter
from dataclasses import dataclass, field

TIMEOUT = 180

RUBRIC = """\
You are grading the quality of a command-line tool's ERROR MESSAGE.

Grade ONLY the message shown. Do not speculate about the tool's internals, and do not
reward or penalise wording you merely like or dislike.

Four dimensions:

1. Specificity  - does it name the SPECIFIC problem (which file, which value, which
                  argument), rather than a generic "invalid input"?
2. Actionability - does it tell the user what to do next, or make the fix obvious?
3. Cleanliness  - free of stack traces, panics, source paths, and internal type names?
4. Exit code    - non-zero for a real failure. A panic exit (101) is a CRASH, not a
                  handled error, and should be penalised heavily.

Score 1-5:
  5 - specific, actionable, clean
  4 - specific and actionable; minor wording or jargon issues
  3 - identifies the problem but the next step is unclear
  2 - vague, or the user must guess what to change
  1 - useless, or a raw crash/panic/internal leak

`pass` is true only for a score of 3 or higher AND no crash/panic/internal leak.

Reply with ONLY a JSON object, no prose and no code fence:
{"score": <1-5>, "pass": <true|false>, "justification": "<one sentence>"}
"""


@dataclass
class Verdict:
    score: int
    passed: bool
    justification: str


@dataclass
class CaseResult:
    case_id: str
    verdicts: list[Verdict] = field(default_factory=list)
    passed: bool | None = None       # majority verdict; None when inconclusive
    score: float | None = None       # mean score across trials
    inconclusive: bool = False
    error: str | None = None


def strip_reasoning(text: str) -> str:
    """Remove a reasoning preamble before JSON parsing.

    Measured on the pinned gateway model: the SAME prompt produced a `<think>` block of
    143 tokens in one call and a bare 2-token answer in another. Reasoning is emitted
    non-deterministically, so this cannot be conditioned on the model name or assumed
    away -- it has to be stripped defensively on every response.

    Also unwraps a ```json fence, which models add despite being told not to.
    """
    text = re.sub(r"<think\b.*?</think\s*>", "", text, flags=re.DOTALL | re.IGNORECASE)
    # An unterminated <think> means the token budget ran out mid-reasoning; everything
    # after the opening tag is reasoning, so there is no JSON to recover.
    text = re.sub(r"<think\b.*", "", text, flags=re.DOTALL | re.IGNORECASE)
    fence = re.search(r"```(?:json)?\s*(.*?)```", text, flags=re.DOTALL)
    if fence:
        text = fence.group(1)
    return text.strip()


def extract_json(text: str) -> dict:
    """Parse the verdict object, tolerating text around it."""
    cleaned = strip_reasoning(text)
    try:
        return json.loads(cleaned)
    except json.JSONDecodeError:
        pass
    # Fall back to the first {...} block; models sometimes prepend a sentence.
    match = re.search(r"\{.*\}", cleaned, flags=re.DOTALL)
    if not match:
        raise ValueError(f"no JSON object in reply: {cleaned[:200]!r}")
    return json.loads(match.group(0))


class Gateway:
    def __init__(self, base_url: str, api_key: str, model: str):
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.model = model
        self.requests_made = 0

    def complete(self, prompt: str, max_tokens: int = 1200) -> str:
        payload = {
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            # Deterministic scoring (spec 005 rubric: temp=0). Note this does NOT make
            # the pinned model deterministic in practice -- hence the majority vote.
            "temperature": 0,
            # Generous, because a reasoning model spends budget thinking before it
            # answers; too small a budget yields an empty answer, not a short one.
            "max_tokens": max_tokens,
        }
        req = urllib.request.Request(
            f"{self.base_url}/v1/chat/completions",
            data=json.dumps(payload).encode(),
            headers={
                "Authorization": f"Bearer {self.api_key}",
                "Content-Type": "application/json",
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
                body = json.loads(resp.read().decode())
        except urllib.error.HTTPError as exc:
            raise RuntimeError(f"HTTP {exc.code}: {exc.read().decode(errors='replace')[:200]}")
        except urllib.error.URLError as exc:
            raise RuntimeError(f"cannot reach gateway: {exc.reason}")
        self.requests_made += 1
        return (body.get("choices") or [{}])[0].get("message", {}).get("content", "")


def build_prompt(case: dict) -> str:
    return (
        f"{RUBRIC}\n"
        f"Command: {case['command']}\n"
        f"Exit code: {case['exit_code']}\n"
        f"Output:\n---\n{case['output']}\n---\n"
    )


def judge_case(gw: Gateway, case: dict, trials: int = 3) -> CaseResult:
    """Judge one case over N trials and take the majority verdict.

    Majority vote is not defensive padding: the pinned model reasons
    non-deterministically, so single-shot verdicts are not reproducible. A split where
    the minority is a single dissent is reported as `inconclusive` rather than forced
    into a verdict (spec 005, Q5) -- a coin-flip recorded as a result is worse than an
    honest abstention, because it silently poisons the calibration number.
    """
    result = CaseResult(case_id=case["id"])
    for _ in range(trials):
        try:
            raw = gw.complete(build_prompt(case))
            data = extract_json(raw)
            result.verdicts.append(
                Verdict(
                    score=int(data["score"]),
                    passed=bool(data["pass"]),
                    justification=str(data.get("justification", "")).strip(),
                )
            )
        except (RuntimeError, ValueError, KeyError, TypeError) as exc:
            result.error = f"{type(exc).__name__}: {exc}"

    if not result.verdicts:
        return result

    votes = Counter(v.passed for v in result.verdicts)
    top, count = votes.most_common(1)[0]
    result.score = sum(v.score for v in result.verdicts) / len(result.verdicts)

    if len(votes) > 1 and count == len(result.verdicts) - count:
        result.inconclusive = True          # an even split has no majority
    elif len(votes) > 1 and count - (len(result.verdicts) - count) <= 1:
        result.inconclusive = True          # bare 2-1 majority: too weak to trust
        result.passed = top
    else:
        result.passed = top
    return result


def gateway_from_env() -> Gateway:
    missing = [
        name
        for name in ("LLM_GATEWAY_URL", "LLM_GATEWAY_KEY", "LLM_GATEWAY_MODEL")
        if not os.environ.get(name, "").strip()
    ]
    if missing:
        raise SystemExit(f"missing required environment: {', '.join(missing)}")
    return Gateway(
        os.environ["LLM_GATEWAY_URL"],
        os.environ["LLM_GATEWAY_KEY"],
        os.environ["LLM_GATEWAY_MODEL"],
    )
