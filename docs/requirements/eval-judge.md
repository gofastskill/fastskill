# Eval Judge

**Version**: 1.0
**Last Updated**: 2026-09-03
**Spans**: `goaikit/aikit` (`aikit-agent`, `aikit-sdk`, `aikit-evals`) → `aroff/cli-framework` → `gofastskill/fastskill` → `gofastskill/skill`

## Overview

A check answers a yes-or-no question the engine can read off a trace: was the skill opened, did the command carry `--tag v2.1.0`, were there fewer than 25 tool calls. It cannot say whether the answer the agent gave was *good*. The correctness suite in `gofastskill/skill` asks twelve questions of the form "give me the exact command", and every one of them is scored today by substring, so an answer that names the right flag inside a wrong or dangerous command passes.

This document specifies the **judge**: a named, user-defined assessment of a trial by a model, rendering a **Judgment** with a **Score**. The vocabulary — Judge, Rubric, Criterion, Judgment, Score, Trial view — is defined in the Skill Evaluation context, `aikit-evals/CONTEXT.md` in `goaikit/aikit`, and this document uses it without redefining it. In particular a **Verdict** stays binary: a judgment carries scores, a check yields pass, fail or not observable, and only a judge that declares a minimum score ever turns a score into a verdict.

### The hazard, stated once

A score is the first number in this pipeline that is not a count. Every downstream artifact — a case verdict, a scorecard metric, a progress line across weeks — inherits whatever the judgment did not record. Two things make a judgment worthless as a measurement, and both are silent:

- **Something the user did not write reached the model.** Every agent harness injects a system prompt, tools, hooks or hidden behaviour. A judge run through `claude`, `codex` or `pi` scores the trial *and* the harness, and nothing in the artifact says which. aikit's own in-process loop does the same in a smaller way: `build_system_instructions` prepends a persona and "You are a helpful AI agent" and pins temperature to the provider default.
- **The identity that produced the number is not part of the number.** The same rubric under a different model, temperature or endpoint is a different judge. A reduction that averages two identities into one figure without saying so has manufactured a measurement.

So a judge is composed in full by the engine, from the user's declarations and nothing else, and every judgment records the request exactly as it was sent. The cost is accepted up front: subscription-only agent CLIs cannot judge, because they cannot be driven as a raw completion.

---

## R1 — A judge is one native completion

A judge is exactly one call to `aikit_agent::llm::LlmGateway::complete(LlmRequest)`: an OpenAI-compatible chat completion carrying the messages the engine rendered, the declared sampling fields, no tools and `stream: false`. `aikit-sdk` re-exports the gateway so `aikit-evals` reaches it without a new dependency edge.

A judge is never an agent run. There is no `agent` key on a judge and no `--judge-agent` flag; the loop runner, the harness backends and every decoder are out of the judge's path by construction. The wire body is the request struct the gateway already sends — `model`, `messages`, `temperature`, `top_p`, `max_tokens`, `stream` — with `Authorization: Bearer` and `Content-Type` as the only headers. A judge adds nothing to that surface; if the gateway ever grows a field a judge does not declare, the judge does not send it.

`LlmResponse` gains an optional `model` field carrying what the provider reported, so a judgment can record the model that answered beside the model that was asked for. This is additive.

## R2 — The user authors the whole conversation

The engine renders exactly what the judge declares and sends exactly what it rendered:

- a system message **only** when `system_prompt` or `system_prompt_file` is declared; absent means no system message, not a default one
- one user message rendered from `prompt` or `prompt_file`
- on a corrective retry (R7), the model's rejected reply as an assistant message and the rendered `retry_prompt` as a further user message

Nothing else is injected. There is no persona, no "answer in JSON" suffix, no tool list. If a judge author wants the model told anything, they write it into a template.

Templates are plain text with `{{variable}}` substitution and these variables only:

| Variable | Renders |
|---|---|
| `{{case.prompt}}` | the case prompt |
| `{{case.<column>}}` | any other `prompts.csv` column, by header name |
| `{{trial.final_answer}}` | the agent's final assistant message, per R10 |
| `{{trial.tool_calls}}` | one JSON object per line: `{seq, tool_name, input, output, is_error}` in call order |
| `{{trial.transcript}}` | every message and tool event in order, as `role: text` blocks separated by blank lines |
| `{{trial.workspace_diff}}` | the trial's `workspace.diff`, per R10 |
| `{{skill.body}}` | the skill's `SKILL.md` as run |
| `{{rubric}}` | the criteria as text, per R5 |
| `{{output_contract}}` | the JSON schema of the reply envelope, per R6 |

`{{case.<column>}}` requires that extra CSV columns survive parsing: `EvalCase` gains `extra: BTreeMap<String, String>` holding every column that is not `id`, `prompt`, `should_trigger`, `tags` or `workspace_subdir`. An unknown variable is a validation error (R14). A variable the run directory cannot supply is a judge error at judging time (R10), never a silent blank.

Each rendered variable is capped at `max_var_bytes` (default 32 KiB). A capped variable is cut at a character boundary, ends with `[truncated N bytes]`, and is listed by name in the judgment's `truncated` array, so a judgment made over a shortened transcript says so.

## R3 — Identity is declared, resolved once, and never falls back silently

A judge's identity is its `model`, `base_url`, `api_key_env`, `temperature`, `top_p` and `max_tokens`. Each is read from the `[[judge]]` entry, then from `[judge_defaults]`, then — for the endpoint only — from `AIKIT_LLM_URL` when that variable is set explicitly. A `--judge-model` flag overrides `model` for every judge in the invocation and is recorded in each judgment as the model actually used, so the artifact never disagrees with what ran.

There is no default endpoint. The gateway's fallback to `api.openai.com/v1` when `AIKIT_LLM_URL` is unset does not apply to judges: a judge with no resolvable `base_url` is an error. The API key is read from the environment variable named by `api_key_env`, defaulting to the gateway's own order (`OPENAI_API_KEY`, then `AIKIT_API_KEY`); a judge whose variable is absent is an error naming the variable. Both errors are raised at the **start** of `eval judge` or `eval run --judge`, before any trial runs and before any request is sent. Paying for a run and then failing to judge it is the expensive way to discover a typo.

Unknown keys on `[[judge]]`, `[judge_defaults]` or `[[judge.criterion]]` are errors. A misspelled `temprature` that parsed as nothing would run the judge under a different identity than the one written down.

## R4 — Sampling belongs to the judge

| Field | Default | Note |
|---|---|---|
| `temperature` | `0.0` | sent explicitly, never left to the provider |
| `top_p` | unset | sent only when declared |
| `max_tokens` | `4096` | a reply with `finish_reason == "length"` is a judge error, not a truncated judgment |
| seed | none | not offered; providers disagree on its meaning and a recorded temperature is the honest reproducibility claim |
| `timeout` | 120 s | in `[judge_defaults]`; a timed-out request is a transport failure under R7 |

## R5 — A rubric is a list of criteria, and the engine computes the score

```toml
[[judge]]
name = "command-correctness"
cases = ["c-tag-pin", "c-branch-pin"]      # exact ids; absent means every case
prompt_file = "judge-prompt.md"            # relative to checks.toml; exactly one of prompt / prompt_file
system_prompt = "You are reviewing a command-line answer."   # optional
retry_prompt = "Your reply was rejected: {{validation_error}}. Reply again with only the JSON."
model = "gpt-4.1"
min_score = 0.8                            # gate; absent means advisory

[[judge.criterion]]
name = "correct_flags"
kind = "scale"                             # "scale" or "bool"
scale = 5                                  # scale only; default 5, at least 2
description = "The command uses the flags the prompt asked for, with the right values."

[[judge.criterion]]
name = "would_run"
kind = "bool"
description = "The command as written would execute without error."

[judge_defaults]
base_url = "https://llm.internal/v1"
api_key_env = "JUDGE_API_KEY"
temperature = 0.0
max_tokens = 4096
max_var_bytes = 32768
max_retries = 2
timeout = 120
```

`{{rubric}}` renders one block per criterion — its name, its range (`1–5`) or `yes / no`, and its description — as plain text with no prose around it. `eval validate` warns when a judge's prompt does not use `{{rubric}}`, because a model asked to score criteria it was never shown is guessing.

The model answers each criterion; the **engine** computes every score. A scale answer *a* on scale *k* normalises to `(a − 1) / (k − 1)`; a bool answer is `1.0` for yes and `0.0` for no. `overall` is the unweighted mean of the normalised criterion scores. Weights, non-linear rubrics and model-reported totals are all out of scope: a number the engine did not compute from recorded answers is a number it cannot explain.

## R6 — The reply is a validated envelope

The model replies with one JSON object:

```json
{
  "criteria": [
    { "name": "correct_flags", "reasoning": "…", "answer": 4 },
    { "name": "would_run",     "reasoning": "…", "answer": true }
  ],
  "notes": "optional free text"
}
```

`reasoning` precedes `answer` in every criterion so the model commits to its reasons before its number. The reply is validated against a JSON schema in which `criteria` must contain every declared criterion exactly once, by name, with `answer` an integer in `[1, scale]` for a scale criterion and a boolean for a bool criterion. Extra criteria, missing criteria, a scale answer out of range and anything that is not one JSON object are all validation failures.

`{{output_contract}}` renders that schema — criterion names enumerated, no surrounding prose. A judge whose prompt does not contain `{{output_contract}}` fails `eval validate`: the engine will reject a reply that does not match a contract the model was never shown, and every judgment would be a retry.

## R7 — Retries are recorded turns, not hidden loops

Two failure classes, two counters, both recorded:

- **Validation failure** (not JSON, schema mismatch, `finish_reason == "length"`): the engine appends the rejected reply as an assistant message and the rendered `retry_prompt` as a user message, and asks again. `retry_prompt` may use `{{validation_error}}` and no other variable. Up to `max_retries` (default 2) corrective turns; then the judge errors for that trial. A judge with no `retry_prompt` gets no corrective turns.
- **Transport failure** (HTTP 429, any 5xx, timeout): retried with exponential backoff under a separate count that `max_retries` does not govern. HTTP 401, 403 and 404 fail at once — a wrong key or a wrong path does not get better on the third try.

Every attempt, successful or not, is recorded in the judgment's `attempts` array (R11) with the full request as sent, the raw reply, `finish_reason`, `usage` and its `kind`.

## R8 — A judge sees trials that were measured

A judge runs on trials with outcome `passed` or `failed` and on no others. A trial with outcome `error` — including a timeout, which the glossary defines as `error` — produced no measurement, and asking a model to score an empty transcript would produce a number where there is nothing to measure. Each skipped trial is recorded per judge, so the count of judgments never quietly differs from the count of trials.

A trial whose final answer is empty but whose outcome is `passed` or `failed` is judged, with `{{trial.final_answer}}` rendered as the literal marker `[no final answer]`. An empty answer is a fact about the agent and a model may score it.

## R9 — Gated or advisory, and what a gate does to the verdict

A judge with `min_score` **gates**: its latest judgment on a trial flattens into `result.json` as a required check result and participates in the case verdict like any required check. A judge without `min_score` is **advisory**: its judgment is recorded and reported, and moves no verdict.

The flattened row is a `CheckResult` with `check_name = "judge:<name>"`, a new `score: Option<f64>` field carrying `overall`, `passed = overall ≥ min_score` when gated and `true` when advisory, `required = gated`, and `message` naming the judge and its overall score. `score` is `#[serde(default)]` and `None` on every check that is not a judge, per ADR 0020. Rate metrics that fold check results (`check_rate` in `fastskill eval scorecard`) skip advisory judge rows, so an advisory judge never dilutes a rate.

**A gated judge that renders no judgment excludes the trial from the case's scored trials.** The trial's `result.json` still carries the `judge:<name>` row with `score: None`, `passed: false` and the error in `message`, and gains `judge_excluded: true`. The case's `scored_trials` does not count it, `judge_excluded_count` records how many were dropped this way beside `error_count`, and a case every one of whose trials is excluded has verdict `error` — the R4 shape from Eval Measurement Integrity, applied one tier up. This is deliberately **not** `not_observable`: that field means the decoder could not produce the evidence, and reusing it for a judge outage would make an outage look like a backend limitation. An advisory judge's error is recorded and changes nothing.

## R10 — The trace must carry what a judge needs

Two artifact gaps stop `{{trial.final_answer}}` and `{{trial.workspace_diff}}` from being renderable today, and both are fixed at write time, additively:

- `trace.jsonl`'s `message` payload gains `kind` and `phase` (both `#[serde(default)]`), copied from the SDK's `StreamMessage`. The final answer is the last assistant message with `phase = final`; today the SDK's delta and final frames both land as bare `{text, role}` and every trial's final answer appears twice.
- Every trial writes `trial-N/workspace.diff`: a unified diff of the scratch workspace against its seeded state, taken before the workspace is discarded. It is an empty file when nothing changed and names binary files without their contents. Today the workspace survives only for failed cases, so a judge could never see what a passing trial wrote.

A judge asked to render a variable from a run directory that predates these fields — a trace without `phase`, a trial without `workspace.diff` — errors for that trial, naming the variable. It does not render a best guess, because the best guess (the duplicated final answer, an empty diff) is precisely the wrong evidence.

## R11 — Every judgment is recorded whole

Judgments live in `trial-N/judgments.json`, a JSON array that is append-only: elements are added, never modified or removed. Each element:

| Field | Meaning |
|---|---|
| `schema` | `"aikit.judgment/1"` |
| `judge` | the judge's `name` |
| `judge_hash` | sha256 over the judge's definition and resolved identity: criteria, prompt, system and retry text, model, endpoint host, sampling fields, `max_var_bytes`. Excludes `cases` and `min_score` (those belong to the suite) and excludes `model_reported` |
| `cache_key` | sha256 over `judge_hash` and the rendered messages |
| `identity` | `{model, model_reported, endpoint_host, temperature, top_p, max_tokens}` — the host only, never the path, never the key |
| `attempts` | every attempt in order: `{kind, request, response_text, finish_reason, usage}`; `request` is the body as sent with the bearer value redacted |
| `scores` | `{<criterion>: normalised score, overall}`; absent when the judge errored |
| `error` | present when no judgment was rendered; the reason |
| `usage` | token totals across attempts |
| `cost_usd` | only when the gateway reported one; never estimated (ADR 0020) |
| `truncated` | names of variables capped under `max_var_bytes` |
| `judged_at` | RFC 3339 |

The latest element per judge name is the one that flattens (R9) and reduces (R12). The full record exists so that a judgment can be audited without the endpoint: what was asked, what came back, under which identity. `aikit.judgment/1` and its flattening into `result.json`, `aggregated.json` and `summary.json` are covered by ADR 0020's additive-only rule; ADR 0021 in `goaikit/aikit` records that extension.

## R12 — Reduction across trials names what it averages

`aggregated.json` and the per-case entry in `summary.json` gain `scores`, keyed by judge name: `overall` as the mean of the latest judgment's `overall` across judged trials, each criterion likewise, bool criteria by majority, and `judged_trials` beside the numbers. A case with no judgment for a judge has no entry for it, rather than a zero.

Run-level totals gain `judge_errors` and judge token totals (`input`, `output`, `total`). Judge cost everywhere is tokens; a money figure appears only where a gateway reported one, and it is never summed with an estimate.

## R13 — One judging function, two entry points

`fastskill eval judge --run-dir <dir> [--checks <file>] [--judge-model <m>] [--judge-parallel <n>] [--rejudge]` judges a completed run in place. `fastskill eval run --judge` runs the same function after the run's own scoring. Both read judge declarations from the checks file recorded in the run's `summary.json` unless `--checks` overrides it.

`eval judge` rewrites `result.json`, `aggregated.json` and `summary.json` additively: every existing field keeps its value except those R9 and R12 define, which are recomputed. It exits non-zero when any judge errored or when the rewritten suite verdict fails; `--no-fail` suppresses the second reason only.

A trial whose latest judgment for a judge already carries the same `cache_key` is skipped — the same identity over the same rendered messages is the same question — unless `--rejudge`, which appends a new judgment regardless. `--judge-parallel` bounds concurrent judge requests and defaults to the run's recorded `parallel`. The schema-validated call reuses `aikit_sdk::Pipeline`, generalised from a single prompt to a message list; it is blocking and runs under `spawn_blocking`.

`aikit-evals` has no command line of its own: the run, judge, validate and reduce functions land there as library API, and `fastskill-evals` re-exports them for `fastskill-cli`, as it does today for the rest of the engine. The `aikit-textgrad` and `aikit-skillopt` consumers are untouched — a judge is not a Scorer.

## R14 — `eval validate` reads files, not the network

`fastskill eval validate` checks judge declarations from file content alone, so it gives the same answer on a laptop and in CI:

- errors: no `model` resolvable from the judge or `[judge_defaults]`; neither or both of `prompt` / `prompt_file` (likewise for the system and retry pairs); an unknown template variable; a `cases` id that matches no case; duplicate criterion names within a judge; `scale < 2`; `min_score` outside `[0, 1]`; a prompt without `{{output_contract}}`; an unknown key
- warnings: a prompt without `{{rubric}}`; a judge whose `model` equals the target agent's model, which is self-preference and worth a line in the report

Endpoint reachability and key presence are R3's concern and are checked when judging starts, not here.

---

## Ownership and sequence

Each step waits for the previous to merge. `aikit-evals` is a library; the commands are fastskill's.

| # | Repo | Contains | Landed as |
|---|---|---|---|
| 1 | `goaikit/aikit` | R10: trace `kind`/`phase`, `workspace.diff` for every trial | [aikit#170](https://github.com/goaikit/aikit/pull/170), `cc569cc` |
| 2 | `goaikit/aikit` | R1–R9, R11, R12, the library half of R13 and R14; `LlmResponse.model`; `EvalCase.extra`; ADR 0021 | [aikit#171](https://github.com/goaikit/aikit/pull/171), `eb50f2c` |
| 3 | `aroff/cli-framework` | rev bump only | [cli-framework#140](https://github.com/aroff/cli-framework/pull/140), `f389b51` |
| 4 | `gofastskill/fastskill` | rev bump; `eval judge`, `eval run --judge`, the validate rules; `webdocs/cli-reference/eval-command.mdx` gains the command, which `spec_docs_parity_test` requires; this document | [fastskill#303](https://github.com/gofastskill/fastskill/pull/303) |
| 5 | `gofastskill/skill` | the first `[[judge]]`, on the correctness suite: `build.py` emits it from `patterns.json`, `prompts.csv` gains an `expected` column, the prompt lives in a hand-written `judge-prompt.md` the generator references | [skill#17](https://github.com/gofastskill/skill/pull/17), `5fb2861` |

The `aikit-sdk` git dependency is pinned by exact revision in both `cli-framework` and `fastskill`, and the two must match character for character or cargo resolves two incompatible copies of the crate. The middle link is not optional.

The judge that landed in step 5 is advisory: it declares no `min_score`, so it scores without moving a verdict, and its metrics report at `min_score = 0.0`. A gate is ratified against the first judged sweep, not asserted ahead of one.

## Verification

Every requirement lands with a test that fails before the change and passes after. Specifically:

- against a recording HTTP stub, a judge with no `system_prompt` sends exactly one message, with one sends exactly two, and the body carries no `tools`, `temperature: 0.0` and `stream: false`; reintroducing any injected text makes the body assertion fail
- a `[[judge]]` with an `agent` key, or any unknown key, fails to parse
- with `base_url` unresolvable, `eval judge` exits non-zero and the stub records zero requests
- a reply missing a declared criterion produces a second attempt whose request has the rejected reply as an assistant message and the rendered `retry_prompt` after it; the judgment's `attempts` has two elements of kind `validation`
- a stub returning HTTP 401 produces exactly one attempt; one returning 503 then 200 produces two, the first of kind `transport`
- a scale answer of 4 on scale 5 and a bool `true` yield `scores` of `0.75` and `1.0` with `overall = 0.875`, computed by the engine and not read from the reply
- an `error` trial receives no request and is listed as skipped for the judge; a `failed` trial with an empty final answer is judged with the `[no final answer]` marker in the sent body
- a run directory whose trace lacks `phase` makes `{{trial.final_answer}}` a judge error naming the variable, and the same directory with `phase` present renders it
- a gated judge erroring on every trial of a case yields `judge_excluded_count == total_trials`, `scored_trials == 0` and case verdict `error`; the same failure on an advisory judge leaves the verdict unchanged
- the serialised `attempts[].request` never contains the bearer value, tested by grepping the artifact for the stub's key
- judging the same run twice appends nothing; `--rejudge` appends one judgment per trial
- `eval validate` on a prompt without `{{output_contract}}` exits non-zero and on one without `{{rubric}}` exits zero with a warning, in both cases with no network and no key in the environment
- the committed pass and fail fixtures in `gofastskill/skill` score identically across the version bump, which is what ADR 0020 exists to guarantee
