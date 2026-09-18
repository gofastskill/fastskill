# Eval Setup

FastSkill 0.9.235

Source: https://docs.gofastskill.com/evals-quality/setup

Release revision: a5edf1ff0ba99c5381a99bcaf78efac64137214f

Documentation revision: a5edf1ff0ba99c5381a99bcaf78efac64137214f



# Eval Setup

This page is the reference for **authoring** an eval suite: the config table in `skill-project.toml`, the prompts CSV a suite is made of, and the optional checks TOML that actually decides pass/fail. Work through it top to bottom once, then use [Run evals](/evals-quality/run-evals) for the day-to-day commands.

For guided suite design, use the [FastSkill skill](https://github.com/gofastskill/skill/tree/main/fastskill).
Its [eval-authoring workflow](https://github.com/gofastskill/skill/blob/main/fastskill/references/eval-authoring.md)
and copyable invoice example are maintained and packaged in the skills repository.

## Prerequisites

* `skill-project.toml` exists (project root or skill directory — `fastskill` walks up the tree to find it).
* `SKILL.md` exists alongside it if you're authoring a single skill's suite.
* For `fastskill eval run`: a supported agent CLI on `PATH`. Agent keys come from aikit-sdk: `aikit`, `claude`, `codex`, `cursor`, `gemini`, and `pi`. A key is accepted only when its CLI is detected on the current machine; a typo and an uninstalled agent both error with `RUNTIME_UNKNOWN_ID`. `fastskill eval validate --all` reports which keys are detected in *your* environment.

Deterministic validation and scoring need no model credential. Target execution needs the
selected runtime's authentication; native judges separately need their configured endpoint,
model, and credential environment variable. Embedding configuration is unrelated.

## The `[tool.fastskill.eval]` table

Add this table to `skill-project.toml`. Every path is resolved relative to the **skill project root** (the directory containing `skill-project.toml`) unless it's absolute.

```toml
[tool.fastskill.eval]
prompts = "evals/prompts.csv"     # required — path to the cases CSV
checks = "evals/checks.toml"      # optional — deterministic checks
timeout_seconds = 900             # per-case timeout (default 900)
trials_per_case = 1               # default 1
parallel = 4                      # max concurrent trials per case; omit for CPU core count
pass_threshold = 1.0              # default 1.0
fail_on_missing_agent = true      # default true
```

| Field                   | Type    | Required | Default                | Meaning                                                                                                                           |
| ----------------------- | ------- | -------- | ---------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `prompts`               | path    | **yes**  | —                      | Path to the prompts CSV.                                                                                                          |
| `checks`                | path    | no       | none                   | Path to the checks TOML. Even when omitted, `should_trigger` produces a per-case trigger expectation — see the callout below.     |
| `timeout_seconds`       | integer | no       | `900`                  | Per-case timeout passed to the agent run.                                                                                         |
| `trials_per_case`       | integer | no       | `1`                    | Trials run per case; validated to `[1, 1000]` (`EVAL_INVALID_TRIALS_CONFIG` otherwise).                                           |
| `parallel`              | integer | no       | unset (CPU core count) | Max concurrent trials for one case.                                                                                               |
| `pass_threshold`        | float   | no       | `1.0`                  | Fraction of trials that must pass for a case to be marked passed; validated to `[0.0, 1.0]` (`EVAL_INVALID_THRESHOLD` otherwise). |
| `fail_on_missing_agent` | bool    | no       | **`true`**             | If true, `eval run` and `eval validate --agent <key>` error with `EVAL_AGENT_UNAVAILABLE` when the chosen agent isn't installed.  |

**Defaults verified against source** (`crates/fastskill-core/src/core/manifest.rs::EvalConfigToml`): `timeout_seconds` defaults to **900**, and `fail_on_missing_agent` defaults to **true** — the CLI refuses to run against a missing agent unless you explicitly set it to `false`. Don't assume otherwise from older examples.


With no `checks` file configured, each case still receives the required skill-consultation expectation implied by `should_trigger`. A successful process exit alone is not enough. Add a checks file for further deterministic outcome or adherence evidence.


### Validate the config

```bash
fastskill eval validate
fastskill eval validate --agent codex
fastskill eval validate --all
fastskill eval validate --json
```

Real output against a working config (this one set `timeout_seconds = 120` and `fail_on_missing_agent = false`):

```
eval configuration: valid
  prompts: /path/to/project/evals/prompts.csv
  cases: 2
  checks: /path/to/project/evals/checks.toml
  check count: 2
  timeout: 120s
  trials_per_case: 1
  parallel: 0
  pass_threshold: 1
  fail_on_missing_agent: false
```

(`parallel: 0` here just means "unset" in the table renderer — the runner still falls back to CPU core count.) A broken `checks.toml` fails validation immediately, e.g.:

```
Error: Configuration error: EVAL_CHECKS_INVALID: Failed to parse checks TOML: TOML parse error at line 1, column 5
  |
1 | not valid toml [[[
  |     ^
key with no value, expected `=`
```

A prompts CSV missing a required column fails the same way:

```
Error: Configuration error: EVAL_INVALID_CSV: Missing required column: should_trigger
```


## The prompts CSV

The suite loader (`aikit-evals::suite`) reads a header row with **required** columns `id`, `prompt`, `should_trigger`, plus optional `tags` and `workspace_subdir`.

```csv
id,prompt,should_trigger,tags,workspace_subdir
greet-1,Write a friendly greeting for an email to a new customer,true,smoke,
greet-2,What is 2+2?,false,smoke,
```

| Column             | Required | Meaning                                                                                                                                                                                                                                                                                                                                  |
| ------------------ | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`               | yes      | Non-empty identifier per row. Used for `--case`, artifact folder names, and reports.                                                                                                                                                                                                                                                     |
| `prompt`           | yes      | Text sent to the agent. Use CSV quoting (`"..."`, doubled `""` for an embedded quote) for commas or newlines.                                                                                                                                                                                                                            |
| `should_trigger`   | yes      | `true`/`1` (case-insensitive `true`) or anything else counts as `false`. It creates a required per-case skill-consultation expectation; see below.                                                                                                                                                                                       |
| `tags`             | no       | Comma-separated tags inside the cell (e.g. `"smoke,basic"`). Used by `eval run --tag <name>`.                                                                                                                                                                                                                                            |
| `workspace_subdir` | no       | Relative path that becomes the agent's working directory for the case. Under the default isolation it is created **inside the case's scratch workspace**, with fixture files copied in from the same path under the project root; under `--no-isolation` it resolves directly under the project root. Empty or omitted → workspace root. |

Empty lines are skipped.

**`should_trigger` affects pass/fail.** The engine creates an implicit required `skill_invoked` check for each case, with `expected` equal to the CSV value. An explicit `skill_invoked` check that applies to the case replaces the implicit one; `eval validate` rejects a contradictory polarity. Therefore `false` tests non-consultation and `true` tests consultation, subject to the selected backend's observability.

This is adherence/selection evidence, not outcome correctness. A structured `Skill` call is direct evidence; supported decoders may also recognize a tool input that references the staged skill-document path. The path proxy indicates consultation, not that the instructions were read or followed. Pair positive cases with outcome checks or judges.



## Checks: `checks.toml` (optional)

Point `checks` at a TOML file with one or more `[[check]]` tables, keyed by the required `name` field. There are exactly five check types (`aikit_evals::checks::CheckDefinition`):

**Checks score the canonical trace, never raw stdout.** Every check below matches against the run's `trace.jsonl` — the parsed, structured event stream. Raw stdout is deliberately excluded: with the `claude` agent it opens with a `system`/`project init` event listing **every skill installed in the environment**, which used to make any skill-name pattern pass vacuously. Assistant text still reaches the trace as message events, so matching on what the agent actually said continues to work.


### `skill_invoked`

The check you almost certainly want for "did my skill fire?".

```toml
[[check]]
name = "skill_invoked"
skill = "my-skill"
expected = true
```

| Field      | Type         | Required | Default                                            |
| ---------- | ------------ | -------- | -------------------------------------------------- |
| `skill`    | string       | no       | — (omit to match *any* skill invocation)           |
| `expected` | bool         | no       | `true` (`false` asserts the skill did **not** run) |
| `required` | bool         | no       | `true`                                             |
| `cases`    | string array | no       | all cases (exact case IDs)                         |

Matches a structured `Skill` invocation or, where the decoder supplies it, a tool input referring to the staged skill-document path. When `skill` is given, a structured invocation must exactly match its identifying field. A skill name merely mentioned in unrelated prose does not count. Path-reference matching improves portability but remains a consultation proxy rather than proof of adherence.

Every check type supports `cases = ["case-id", ...]`, selecting exact case IDs. An absent selector applies to every case. Use selectors for checks that should not run across a mixed suite.

### `trigger_expectation`

```toml
[[check]]
name = "trigger_expectation"
pattern = "Launching skill: my-skill"
expected = true
```

| Field      | Type   | Required | Default                                                     |
| ---------- | ------ | -------- | ----------------------------------------------------------- |
| `pattern`  | string | yes      | —                                                           |
| `expected` | bool   | yes      | — (`true` = pattern must appear, `false` = must NOT appear) |
| `required` | bool   | no       | `true`                                                      |

Matches `pattern` as a **plain substring** against the JSONL trace. Prefer [`skill_invoked`](#skill_invoked) for asserting that a skill ran; treat `trigger_expectation` as the escape hatch for matching arbitrary text the agent produced.

### `command_contains`

```toml
[[check]]
name = "command_contains"
pattern = "validate"
required = true
```

| Field      | Type   | Required | Default |
| ---------- | ------ | -------- | ------- |
| `pattern`  | string | yes      | —       |
| `required` | bool   | no       | `true`  |

Same trace substring search as `trigger_expectation`, without the pass/fail inversion — passes if the pattern is found anywhere in the trace.

### `file_exists`

```toml
[[check]]
name = "file_exists"
path = "report.txt"
required = true
```

| Field      | Type | Required | Default |
| ---------- | ---- | -------- | ------- |
| `path`     | path | yes      | —       |
| `required` | bool | no       | `true`  |

Passes if `path` exists under the case's working directory — the case's scratch workspace under the default isolation (plus `workspace_subdir` when set), or the project root under `--no-isolation`.

### `max_tool_calls` (alias: `max_command_count`)

```toml
[[check]]
name = "max_tool_calls"
limit = 20
required = true
```

| Field      | Type    | Required | Default |
| ---------- | ------- | -------- | ------- |
| `limit`    | integer | yes      | —       |
| `required` | bool    | no       | `true`  |

Passes if the number of **tool invocations** the agent made is `<= limit`.

A tool invocation is a structured `tool_use` trace event, plus any `raw_json` trace line — the
latter covers backends that still emit tool calls as raw JSON instead of decoded tool events.
Assistant text output, token-usage events, and unmodelled SDK event variants are **not** counted.

Results use `"check_name": "max_tool_calls"`. The per-case artifact field counting
these events is `command_count`.


### Making a check advisory with `required`

Every check type accepts `required` (default `true`). Setting `required = false` makes the check **advisory**: it still runs, and its result still appears in the run artifacts with its own `passed` value, but a failure does not fail the case.

```toml
[[check]]
name = "command_contains"
pattern = "cache hit"
required = false   # nice to have; don't fail the case over it
```

Use it for signal you want visible in reports without gating the suite on it.

### How a case's pass/fail is decided

* No `checks` file configured (or it loads zero explicit checks): the case still receives the required trigger expectation generated from `should_trigger`.
* Any checks loaded: the case **passes** only if every **required** check's `passed` is `true`. Checks marked `required = false` are reported but never fail the case. Among required checks there is no partial credit and no threshold — they are all-or-nothing per case.
* A case whose checks are **all** advisory (`required = false`) has nothing that can fail it, so it passes as long as it does not time out. That is intentional, but it means an all-optional `checks.toml` is not a safety net.
* A trial that times out is always `error`, independent of checks.
* Across trials, `pass_threshold` decides the case's aggregated status: pass rate across trials must be `>= pass_threshold`.


## Validate, then run, then report

Once the manifest, CSV, and checks file exist:

```bash
fastskill eval validate --agent codex
fastskill eval run --agent codex --output-dir ./eval-runs
fastskill eval report --run-dir ./eval-runs/<timestamp>/codex
```

`--output-dir` is **required** on `fastskill eval run` — there is no default. Omitting it fails argument parsing before anything else runs:

```
error[E003]: missing required argument --output-dir <output-dir>
  hint: Use --help to see required arguments
```

`--agent <key>` (repeatable) and `--all` are mutually exclusive on both `eval run` and `eval validate` — passing both errors with `RUNTIME_CONFLICTING_FLAGS`. `eval run` additionally requires one of them (passing neither errors with `RUNTIME_NO_SELECTION`); on `eval validate` both are optional — omit them to validate config without checking any agent's availability.


See [Run evals](/evals-quality/run-evals) for the full day-to-day command set, artifact layout, and CI gating pattern.



## Setup checklist

* [ ] `[tool.fastskill.eval]` present with a `prompts` path that resolves.
* [ ] Prompts CSV has `id`, `prompt`, `should_trigger` headers (plus `tags` / `workspace_subdir` if you use them).
* [ ] Each `should_trigger` value reflects the required consultation/non-consultation expectation; explicit `skill_invoked` checks do not contradict it.
* [ ] Outcome correctness is measured separately where consultation alone is insufficient.
* [ ] `fastskill eval validate --all` passes and lists the agent you intend to run against as available.
* [ ] Output directory convention agreed for `--output-dir` (local + CI).

## See also

* [Run evals](/evals-quality/run-evals)
* [Cluster analysis](/evals-quality/cluster-analysis)
* [eval command](/cli-reference/eval-command)

