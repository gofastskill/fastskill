# Eval Measurement Integrity

**Version**: 1.0
**Last Updated**: 2026-09-03
**Spans**: `goaikit/aikit` (`aikit-sdk`, `aikit-evals`) → `aroff/cli-framework` → `gofastskill/fastskill` → `gofastskill/skill`

## Overview

An eval is worthless if its checks can pass regardless of what the agent did. This document specifies the changes that close the ways the current engine can report a number that is not a measurement.

The work started from a real observation. In a 210-trial baseline sweep on the `pi` backend, four trials produced a complete-looking artifact set and zero model output. The provider had timed out, `pi` retried three times, gave up, and exited zero. Those four trials were the entire difference between a reported 96.4% and the true 100%.

The engine had the evidence and discarded it. The agent's own stdout carried `"stopReason":"error"` sixteen times and `"errorMessage":"Request timed out."`, and trace normalization kept none of it. What reached the trace was one system message reading `Connection error.` A Python post-processor in the skill repository worked around this by treating "no assistant text in the trace" as death. That heuristic is a workaround for information the engine threw away, and this specification replaces it.

### The hazard, stated once

Over an empty or truncated trace:

- a negative expectation passes, because an absent pattern is absent
- a tool-call ceiling passes, because zero is under every limit
- a positive expectation fails

So an outage scores as perfect restraint and perfect budget compliance, while the same outage on a positive suite scores as total failure. The direction of the lie is set by the polarity of the check, which means no single default is safe. A run that produced no valid measurement must be recorded as such and excluded, never reduced to a pass and never averaged in as though it were a wrong answer.

---

## R1 — A trial that produced no measurement is `error`, not `failed`

`error` means **no valid measurement exists**. It is decided on transport and terminal signal, not inferred from the content of the output:

1. the process exited non-zero, or
2. the run timed out, or
3. the agent's own terminal event reports failure, or
4. the stream ended with no terminal event, on a backend that declares it emits one (see R3).

An agent that exits cleanly having answered with nothing is **`failed`**, not `error`. That is a real skill failure and must score as one. Text absence is never the primary discriminator; it is a fallback only where no transport signal exists.

`skipped` stays reserved and unconstructed.

## R2 — Every decoder emits a typed terminal event

Each backend already sends its outcome on the wire and every decoder drops it. Each now decodes it into one canonical frame carrying an outcome, a machine reason, and an optional message.

| Backend | Terminal line | Outcome from |
|---|---|---|
| claude | `result` | `is_error`, `subtype`, `stop_reason` |
| codex | `turn.completed` / `turn.failed` / `error` | event type, `error.message` |
| pi | `turn_end` | `message.stopReason`, `message.errorMessage` |
| gemini, opencode, cursor | not decoded | — (see R3) |
| aikit (in-process) | not applicable | completion is the call returning |

**The last status-bearing terminal event decides the run.** `pi` emits one per turn, and a run that errors and then recovers must not be marked `error`. `pi`'s `agent_settled` is an end-of-stream marker with no status of its own and does not override a preceding outcome.

## R3 — A capability flag says whether a terminal event is expected

`BackendCapabilities` gains `terminal_event`. Rule 4 of R1 — stream ended without a terminal event means `error` — applies **only** to backends whose flag is set.

Three backends are wrapped as text-only and emit no structured frames at all. Under a strict rule every one of their trials would become `error` in the same release that changes the schema. The flag makes the leniency explicit and per-backend, and each backend flips to strict the day its decoder is fixed.

Following ADR 0019, a capability flag describes the decoder as it is. A flag that outruns its decoder is a bug. `opencode` currently declares structured tools while its decoder discards the tool name; that claim is corrected to match reality in the same change that makes these flags load-bearing.

## R4 — Errored trials are excluded from the rate; a case with none left is `error`

`pass_rate` is passing trials over **scored** trials, where scored excludes `error`. The count of excluded trials is recorded on the case, never silently dropped.

A case whose every trial errored has no scored trials. It takes the case verdict `error`, is excluded from the split score, and `fastskill eval score` **exits non-zero naming it**. Silent exclusion would move the vacuity hazard up one level, where a total outage scores 100% over zero cases.

There is no retry layer in the engine. `pi` already retries internally, and a second layer would hide the outage rate, which is itself a signal the report should carry.

## R5 — Artifacts gain the fields the runner already has

Per trial, all additive under [aikit ADR 0020](https://github.com/goaikit/aikit/blob/main/docs/adr/0020-eval-artifacts-are-an-additive-only-contract.md):

- `exit_code` — captured today and never persisted
- `terminal` — outcome, reason and message from R2
- `cost_usd` — **vendor-reported only.** `None` when the backend reports nothing. Never estimated from a local price table: a stale estimate is indistinguishable from a real number once written to an artifact, and a metric reading "unknown" is honest where one reading a wrong number is not.
- the full token breakdown — total, cache read, cache creation and reasoning tokens, which the runner receives and currently narrows away to input and output only

Per case, `error_count` and `scored_trials` alongside the existing counts.

## R6 — Checks may target named cases

`[[checks]]` entries gain an optional `cases` list. Absent means every case, so existing files parse unchanged.

Today a suite's checks are global by construction: the suite type holds cases and nothing else, and one slice of checks is applied to all of them. Two cases needing different assertions therefore cannot share a suite. The v2 suite works around this with one directory per correctness assertion, which is a workaround for a missing selector and collapses once the selector exists.

## R7 — `should_trigger` is scored

`should_trigger` is parsed into the case and read by nothing. A case marked `false` asserts nothing at all. Everyone reading the CSV assumes otherwise, which makes it an explicit input that silently does nothing.

It now generates an implicit per-case check: **skill invocation, with polarity matching the column**. Not a substring expectation, which would need a pattern nobody supplies. Validation rejects a case whose explicit checks contradict the column rather than silently letting one win.

## R8 — Skill invocation is a path match, not a tool name

`skill_invoked` keys on a tool-use event whose tool name is literally `Skill`. Only Claude Code emits that name. On `pi` a skill read arrives as `read_file` with the path in its arguments; on `codex` every call is `shell` or `file_change`.

The check now matches **any tool-use whose input references the skill document's path**, and still accepts the typed `Skill` tool where it exists. The path is what `fastskill` stages into the rollout workspace, so it is known without configuration, and `checks.toml` may override it.

The check reads the trace only. Nothing derived from the agent's environment may feed a verdict: the capability listing an agent prints at startup produced false passes once already, and the isolation report that records it is documented as report-only for that reason.

## R9 — A check that cannot be observed says so

A check whose evidence the target backend cannot produce returns a typed **not observable** result. It does not pass and does not fail. Both would be lies, and a vacuous pass is the failure this whole document is about.

Decoder repairs for `opencode` and `cursor` are deliberately **out of scope** here. They are separate bugs with their own tests, and this change is one bounded batch. What must land now is the not-observable result, or the vacuous pass simply relocates.

## R10 — Refuse a suite the backend cannot score

When a **required** check is not observable on the chosen backend, `fastskill eval validate` and `fastskill eval run` refuse the suite-and-backend combination before spending a single token. Paying for trials that are known in advance to be unscoreable is waste, and refusing early is the loud failure. `skipped` is not used for this.

## R11 — `eval score` is read-only

`eval score` currently backfills `command_count` and token counts into the run's `summary.json` while reading it. Scoring the committed fixtures therefore dirties the working tree.

The backfill moves to `eval run`, at write time. The writer owns the artifact; the scorer reads it. Every number `eval score` reports is a pure function of `result.json` and `trace.jsonl`.

## R12 — The vocabulary is upstream

Case, Trial, Check, Suite, Trial outcome and Case verdict are defined in aikit's `CONTEXT.md`. `fastskill` references them and does not define its own copies. The informal coinages used during this investigation — "dead trial", "oracle", "family" — retire in favour of "trial with outcome `error`", "check", and "suite".

Trial outcome and Case verdict are named separately although one type, `CaseStatus`, serves both at four sites. The shared type is recorded debt. The vocabulary leads the refactor rather than ratifying the collision.

---

## Ownership and sequence

Each step waits for the previous to merge.

| # | Repo | Contains | Landed as |
|---|---|---|---|
| 1 | `goaikit/aikit` | R1–R9, ADR 0020, glossary follow-up | `532c5ee` (#167) |
| 2 | `aroff/cli-framework` | rev bump only | `c1c813c` (#138) |
| 3 | `gofastskill/fastskill` | rev bump, R10, R11, this document | this change |
| 4 | `gofastskill/skill` | collapse the v2 layout onto R6, delete the Python aggregator | pending |

The `aikit-sdk` git dependency is pinned by exact revision in both `cli-framework` and `fastskill`, and the two must match character for character or cargo resolves two incompatible copies of the crate. The middle link is not optional.

## Verification

Every requirement lands with a test that fails before the change and passes after. Specifically:

- a recorded `pi` trial whose stream carries `stopReason: error` scores `error`, and the same trial with that field removed does not
- a case whose trials all error yields verdict `error` and a non-zero exit from `eval score`
- `pass_rate` moves when an errored trial is counted and holds when it is excluded
- the committed pass and fail fixtures score identically across the version bump, which is what ADR 0020 exists to guarantee
