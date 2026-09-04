# Eval Scorecard Report

**Version**: 1.0
**Last Updated**: 2026-09-03
**Spans**: `goaikit/aikit` (`aikit-evals`) → `aroff/cli-framework` → `gofastskill/fastskill` → `gofastskill/skill`

## Overview

`fastskill eval scorecard` answers the question a **Benchmark** asks — across every run under a root, what is the rate of each named **Metric** and does it clear its gate — and prints the answer as a table or as JSON. That JSON is the whole of what survives a benchmark: the run directories are large, live in scratch space and are deleted; the scorecard is what gets committed, pasted into a pull request and compared with last month's.

Today it cannot carry that weight. It has metrics and totals and nothing else: no per-case rows, no record of which skill revision or which benchmark file produced it, no room for a judgment. The committed baseline in `gofastskill/skill` with its per-case detail was printed by a Python aggregator that no longer exists, so the binary cannot reproduce the one report anyone has actually read.

This document makes the scorecard JSON complete and self-identifying, adds the **judge_score** metric over the judgments specified in [Eval Judge](eval-judge.md), refuses the comparisons that would be lies, and renders the result as a single static HTML file. Benchmark, Metric, Scorecard and Report are defined in this repository's `CONTEXT.md` under Evaluation; Judge, Judgment and Score are upstream in `aikit-evals/CONTEXT.md`.

### The hazard, stated once

A scorecard that does not say what it measured is a number without a question. Every way this goes wrong is a comparison that looks valid:

- two scorecards from different benchmark files, plotted on one line
- one metric folding judgments from two judge identities, reported as one score
- one root holding two runs of the same case, counted twice
- a run against a skill revision nobody recorded, so the number cannot be tied to a change

None of these fails today. Each is a mixed measurement presented as a clean one, and the fix is the same for all: record the identity, compare identities before comparing numbers, and refuse — loudly, with an override that names what it is overriding — when they differ.

---

## R1 — The scorecard JSON is complete

`fastskill eval scorecard --format json` (and `--json`) emits:

```
schema             "fastskill.scorecard/1"
generated_at       RFC 3339
targets[]          {agent, model, runs}   — one entry per distinct target
agent, model       set only when targets has one entry
skill              {path, git_sha, dirty}
benchmark          {path, sha256}
runs[]             {run_dir, started_at, agent, model}
fastskill_version, aikit_evals_version
judges[]           {name, judge_hash, identity}   — every distinct judge_hash seen
metrics[]          as today, plus mixed_judges / mixed_targets flags when overridden
totals             as today, plus judge_errors, judge_excluded_trials and judge token totals
unclaimed_checks   as today
cases[]            one row per (run, case)
```

Each `cases[]` row carries the case id, its run directory, the case verdict, trial and scored-trial counts, `error_count`, `judge_excluded_count`, every check result observed (name, passed, observed, not-observable) and, per judge, the latest judgment per trial with its overall score, every criterion's normalised score, answer and **full** reasoning, and the judge error when there was one. Nothing in a row is summarised; the HTML (R6) decides what to fold, the JSON keeps everything.

Every key the scorecard emits today keeps its name, type and meaning. New keys are added beside them. A reader written against the current shape keeps working, which is ADR 0020's rule applied to this artifact.

## R2 — The scorecard identifies what it measured

Four identities, each recorded where it is known and copied, not recomputed, downstream:

- **Target**: `agent` and `model` from each run's `summary.json`. The scorecard lists every distinct pair in `targets[]`.
- **Skill**: `eval run` in `aikit-evals` records `skill_git_sha` and `skill_dirty` in `summary.json` at run time, additively, by reading the skill project root's git state. The scorecard copies them. A run that predates the fields yields `null`, never a value resolved later — the skill on disk at scorecard time is not the skill that ran.
- **Benchmark**: `benchmark.sha256` is the sha256 over the metrics file and every suite file it selects. The metrics file gains a top-level `suites` list of directories relative to itself; for each, `checks.toml`, `prompts.csv` and every file a `[[judge]]` references (`prompt_file`, `system_prompt_file`, `retry_prompt_file`) enter the hash in path order. A metrics file without `suites` yields `benchmark.sha256: null`, and R5's progress section refuses to compare it. A benchmark whose gates or selections changed is a different question, and the hash is what says so.
- **Judges**: every distinct `judge_hash` across the selected judgments, with its identity block, in `judges[]`.

## R3 — `judge_score` is a metric

```toml
[[metric]]
name = "Command correctness"
kind = "judge_score"
cases = ["c-*"]
judges = ["command-correctness"]
criterion = "correct_flags"        # optional; absent means overall
min_score = 0.8
```

The value is the mean of `overall` (or of the named criterion) over the latest judgment per trial across the selected cases and judges; the verdict passes when the value clears `min_score`. A trial with a judge error contributes nothing and is counted in `judge_excluded_trials`. A metric matching no judgment **fails the command**, and `--no-fail` does not suppress it, for the reason R13 of Eval Measurement Integrity gives for checks: a gate that silently matches nothing is not a gate.

`check_rate` skips advisory judge rows (`judge:<name>` results with `required: false`) and counts gated ones, so the two metric kinds never double-count and an advisory judge never moves a rate.

## R4 — Mixed measurements are refused by name

Three guards, each an error naming the offending values, each with an override flag that records itself in the output:

| Condition | Error unless | Recorded as |
|---|---|---|
| a `judge_score` metric's judgments carry more than one `judge_hash` | `--allow-mixed-judges` | `metrics[].mixed_judges: true`, and `judges[]` lists every hash with its identity |
| runs under `--root` carry more than one `(agent, model)` pair | `--allow-mixed-targets` | `metrics[].mixed_targets: true`; `agent`/`model` absent, `targets[]` complete |
| the same case id appears in more than one run under `--root` | `--allow-duplicate-cases` | every row keeps its `run_dir`; totals count each occurrence |

The mixed-judges guard applies again across scorecards in a progress report (R5). An override is a statement the reader will see in the report, not a way to make the check disappear.

## R5 — Progress compares one benchmark with itself

`fastskill eval scorecard --format html --from a.json --from b.json [--from …] -o report.html` renders a progress section when given two or more scorecard files. Every input must carry the same non-null `benchmark.sha256`; otherwise the command errors, with no override, because two benchmarks are two questions and there is no honest way to draw them on one axis. Series are keyed by target (`agent` + `model`); the x axis is `generated_at`; each metric is one chart, with its gate drawn as a line and every point where the verdict flipped highlighted.

## R6 — The report is one static file

`--format html` writes a single self-contained HTML file. Its properties are requirements, not preferences:

- **No JavaScript.** Folding uses `<details>`; charts are inline SVG written by Rust; there is no script element except one `<script type="application/json">` carrying the source scorecard(s) verbatim, so the report is its own data file and a reader can recover the JSON with a text editor.
- **No network.** Fonts are IBM Plex Sans (400, 600) and IBM Plex Mono (400) as base64 woff2 in `@font-face`, included at build time with `include_bytes!` from `crates/fastskill-cli/assets/fonts/`, with the OFL licence text beside them and reproduced in the file. There is no `<link>` to anywhere.
- **Sections, in order:** identity (R2, plus versions and the run list); gates (every metric, its value, its threshold, its verdict); notes (unclaimed checks, unmeasured cases, judge errors, every override that was used); judges (identity table and token totals); one table per suite listing each case's verdict, checks and judgment scores; progress when R5 applies.
- **Reasoning is shown, then folded.** Each criterion's reasoning shows its first 600 characters; the remainder sits under `<details>`. `--no-reasoning` removes reasoning from the rendered tables **and** from the embedded JSON, so a report that hides reasoning does not ship it in a place the reader cannot see.
- **Links only where they resolve.** A case row links to its run directory when that path exists at render time and is plain text otherwise. A link to a deleted scratch directory is worse than none.

Rendering from `--from` reads scorecard JSON and touches no run directory. Without `--from`, `--format html` computes the scorecard from `--root` and `--metrics` as today and renders that.

## R7 — Nothing in the crate says *sweep*

`Sweep`, `SweepTotals`, `absorb`'s doc comment and the module header in `commands/eval/scorecard.rs` use a word the glossary bans. They are renamed to the vocabulary: the accumulator becomes `Observations`, its totals `ScorecardTotals`, and no user-facing string, doc comment or identifier in the eval commands says sweep or grader after this lands. The internal rename is invisible in the artifact: `totals` keeps its key.

## Out of scope

Pairwise judgments, judge panels with agreement statistics and `--labels` calibration against human scores are the next phase. They need the identity and completeness this document provides and are not designed here.

---

## Ownership and sequence

Each step waits for the previous to merge and follows the [Eval Judge](eval-judge.md) sequence, since every judge field here is read from artifacts that spec creates.

| # | Repo | Contains | Landed as |
|---|---|---|---|
| 1 | `goaikit/aikit` | `skill_git_sha` and `skill_dirty` in `summary.json` (rides with Eval Judge step 2) | goaikit/aikit#171 `eb50f2c` |
| 2 | `aroff/cli-framework` | rev bump only | aroff/cli-framework#140 `f389b51` |
| 3 | `gofastskill/fastskill` | rev bump; R1–R4, R7; this document | #303 `964d65f` |
| 4 | `gofastskill/fastskill` | R5, R6: `--format html`, `--from`, `-o`, `--no-reasoning`, the font assets; `webdocs/cli-reference/eval-command.mdx` | #304 |
| 5 | `gofastskill/skill` | `suites` in `evals/v2/metrics.toml`; a `judge_score` metric over the correctness judge; a regenerated baseline scorecard JSON replacing the text file no tool can reproduce | |

## Verification

Every requirement lands with a test that fails before the change and passes after. Specifically:

- the JSON output of a two-run fixture contains a `cases[]` row per (run, case) with full reasoning, and deserialises with a reader written against today's `{metrics, totals, unclaimed_checks}` shape
- editing one byte of a selected suite's `prompts.csv` changes `benchmark.sha256`; editing a file outside `suites` does not; removing `suites` yields `null`
- a fixture with two judgment rows differing only in `judge_hash` makes a `judge_score` metric exit non-zero naming both hashes, and exit zero with `mixed_judges: true` under `--allow-mixed-judges`
- two runs with different `model` values exit non-zero, and under `--allow-mixed-targets` emit no top-level `model` and a two-entry `targets[]`
- the same case id in two run directories exits non-zero, and under `--allow-duplicate-cases` yields two rows with distinct `run_dir`
- a `judge_score` metric whose pattern matches no judgment exits non-zero even with `--no-fail`
- the rendered HTML contains exactly one `<script` and it is `type="application/json"`; contains no `<link`; contains no `http` inside `src` or `href` except the run-directory links, which are only present when the directory exists in the test's temp root
- `--no-reasoning` produces an HTML file in which the string of a known reasoning text appears nowhere, including inside the embedded JSON
- `--from a.json --from b.json` with differing `benchmark.sha256` exits non-zero; with equal hashes the SVG contains one highlighted point per verdict flip in the fixture
- `grep -i sweep crates/fastskill-cli/src/commands/eval` returns nothing
