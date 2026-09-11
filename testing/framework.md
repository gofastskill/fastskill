# Testing Framework

FastSkill 0.9.229

Source: https://docs.gofastskill.com/testing/framework

Release revision: 68975dfaca15508bc5cf2fb761d6218405c954ea

Documentation revision: 68975dfaca15508bc5cf2fb761d6218405c954ea



## Overview

The way you test a skill in FastSkill is `fastskill eval`. You define eval cases in your project
manifest, run them against an agent, and score the results. There is no separate unit/mock harness —
eval is the testing framework.

## Configure evals

Evals are declared under `[tool.fastskill.eval]` in `skill-project.toml`:

```toml
[tool.fastskill.eval]
prompts = "evals/prompts.csv"     # CSV of prompt cases
checks = "evals/checks.toml"      # TOML of checks/assertions
timeout_seconds = 600             # per-case timeout
fail_on_missing_agent = false     # skip instead of failing when the agent isn't installed (default: true)
```

* `prompts` — path to a CSV of prompt cases to run.
* `checks` — path to a TOML file of checks applied to each result.
* `timeout_seconds` — per-case timeout.
* `fail_on_missing_agent` — when `true`, eval fails instead of skipping if the agent isn't found.

## The eval subcommands

| Command                   | What it does                                                         |
| ------------------------- | -------------------------------------------------------------------- |
| `fastskill eval validate` | Validates the eval configuration and referenced files before running |
| `fastskill eval run`      | Runs the eval cases against an agent                                 |
| `fastskill eval report`   | Shows a report for a completed eval run                              |
| `fastskill eval score`    | Scores results                                                       |

```bash
# 1. Check the config and files are well-formed
fastskill eval validate

# 2. Run the prompt cases against an agent (--agent or --all, plus --output-dir, are required)
fastskill eval run --agent claude --output-dir ./eval-runs

# 3. Review and re-score the per-agent run directory it created
fastskill eval report --run-dir ./eval-runs/<timestamp>/claude
fastskill eval score --run-dir ./eval-runs/<timestamp>/claude
```

Eval output supports `--format table|json` (no grid/xml for eval).

Start by pinning down what "good" looks like in your `checks.toml`, then iterate: `eval run` →
`eval report` → adjust the skill → repeat.


## See also

* [Evals & quality overview](/evals-quality/overview) — the quality workflow and concepts.
* [eval command reference](/cli-reference/eval-command) — full flags and output details.

