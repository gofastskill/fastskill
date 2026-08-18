# Error-message-quality judge (spec 005, P2)

Scores fastskill's error messages against a fixed rubric using an LLM, and — first —
checks whether the LLM can do that job at all.

```
run a known error-producing command  →  capture (command, output, exit_code)
     →  gateway LLM + fixed rubric  →  {score, pass, justification}  →  3-trial majority
```

The **capture is deterministic**; only the **scoring** is LLM. Per spec 005 Q3 this can
never become a PR gate: anything with an LLM in the verdict path stays in the nightly
soft tier. If a check becomes stable enough to gate on, rewrite it as a deterministic
assertion instead.

## Files

| | |
|---|---|
| `judge.py` | Rubric, gateway client, majority vote |
| `cases.json` | Calibration fixture — human verdicts on real fastskill errors |
| `calibrate.py` | Scores the judge against the fixture. The go/no-go gate. |
| `suite.json` | The live suite — **commands**, not captured output |
| `run_suite.py` | Runs each command in a sandbox and judges what comes back |

## Running the live suite

```bash
LLM_GATEWAY_URL=... LLM_GATEWAY_KEY=... LLM_GATEWAY_MODEL=... \
    python3 ci/quality/error_quality/run_suite.py --json report.json
```

`--capture-only` runs the commands and prints what they emit **without calling the
gateway** — use it to check the harness, add cases, or see what changed, for free.

Each case runs in a disposable sandbox with a bare environment (`PATH` + `HOME` only), so
a stray `OPENAI_API_KEY` or `FASTSKILL_*` on the machine cannot change which error path
runs. Sandbox paths are rewritten to `<project>` so captures are stable across runs.

**A case that exits 0 is reported as broken, not judged.** A command that succeeded did
not produce the error the case exists to grade, and scoring its success output would
quietly weaken the suite while still looking green.

**This tier is soft.** A poor message exits 0 and appears in the report; only a broken
harness (no binary, unreachable gateway) fails the run. Past `--max-requests` the
remaining cases are skipped *and named* — silent truncation would read as full coverage.

### First live run (2026-08-18, judge `claude-haiku-4-5`, 24 requests, ~$0.017)

7 of 8 messages passed. The one flagged poor was `repos_skills_unknown_repo` —
independently rediscovering the known K-class defect with no hard-coded knowledge of
which case was bad:

> The error is vague and generic — it doesn't specify which argument is unknown, what the
> valid arguments are, or how to correct it.

## Running calibration

```bash
LLM_GATEWAY_URL=... LLM_GATEWAY_KEY=... LLM_GATEWAY_MODEL=... \
    python3 ci/quality/error_quality/calibrate.py --trials 3
```

Exit 0 = calibrated, 1 = below threshold, 2 = could not run. 10 cases × 3 trials = 30
gateway requests, well inside the 300-request cap.

**Run this before trusting any judge output, and again on every model change** (spec 005
Q4). It is cheap and it is the only thing standing between "we have a judge" and "we have
a plausible-sounding random number generator".

## Calibration results (2026-08-18)

Measured, not assumed:

| Model | Backing | Agreement | Verdict |
|---|---|---|---|
| `claude-haiku-4-5` | real Anthropic | **100%** (10/10) | CALIBRATED |
| `claude-sonnet-4-6` | local gemma4 on gx10 | **70%** (7/10) | not calibrated |

Baseline for the fixture is 50% (it is deliberately balanced 5 pass / 5 fail, so a judge
that always answers the same way scores 50%).

### Why the local model failed, specifically

It got every *good* message right and both *raw panics* right. It failed on exactly one
class: **messages that are clean but useless** — `repos_skills_unknown_argument`,
`generic_invalid_input`, `serde_error_leak`. It conflates "no stack trace" with "good".

The clearest symptom is `generic_invalid_input` (`Error: invalid input`), where the model
wrote *"does not provide specific details or actionable steps"* and then returned
`pass: true` with a mean score of 2.7 — contradicting both its own reasoning and the
rubric's rule that pass requires ≥ 3.

**No prompt tuning was attempted before switching models.** Tuning against the same
fixture that measures success would fit the judge to these ten cases and inflate the
number without improving the judgement.

### Cost

From the gateway's own spend log:

| Model | per call | per 30-call run |
|---|---|---|
| `claude-haiku-4-5` | $0.00070 | ~$0.02 |
| `claude-sonnet-4-6` | $0.00251 | ~$0.08 |

The local model is billed **3.5× more**, because the alias is named `claude-sonnet-4-6`
and LiteLLM prices by alias regardless of the gx10 route. So the usual "use the local
model to save money" argument does not apply here — it is both less accurate *and* more
expensive in the ledger.

## Two implementation details that are not incidental

**`<think>` stripping.** The pinned local model emits reasoning preambles
*non-deterministically* — the same prompt produced a 143-token `<think>` block in one call
and a bare 2-token answer in another. Stripping cannot be conditioned on the model name or
assumed away; it runs on every response. An unterminated `<think>` means the budget ran
out mid-reasoning, so there is no JSON to recover.

**Majority vote is not padding.** Because reasoning is non-deterministic, single-shot
verdicts are not reproducible. A bare 2-1 majority is flagged `inconclusive` (spec 005 Q5)
rather than forced into a verdict — a coin flip recorded as a result silently poisons the
calibration number.

## Extending the fixture

Keep it **balanced**. A fixture that is mostly `pass` can be gamed by a judge that always
answers `pass`, and would report high agreement while being useless — `calibrate.py`
prints the always-answer-the-same-way baseline and refuses to certify a judge that merely
matches it.

Prefer **real captured output** over invented examples. The judge has to agree with humans
about messages the product actually emits.
