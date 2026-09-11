# optimization commands

FastSkill 0.9.228

Source: https://docs.gofastskill.com/cli-reference/optimize-command

Release revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d

Documentation revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d



## Overview

`fastskill optimization` improves a skill document iteratively: each step scores the current `SKILL.md` against your eval suite, proposes patches, and keeps the best candidate. Subcommands are `run`, `resume`, `status`, `inspect`, and `export`. For the config file format and a walkthrough, see [Optimization](/optimize/overview).

`optimization run` scores candidates with the same agent runtimes as `fastskill eval run`, so the
agent named in the configuration must be on `PATH`. Use
`fastskill eval validate --agent <key>` to verify availability.


## Usage

```bash
fastskill optimization <SUBCOMMAND> [OPTIONS]
```

## optimization run

Starts an optimization run from a config file.

```bash
fastskill optimization run --config ./optimize.toml
fastskill optimization run --config ./optimize.toml --out-dir ./optimize-runs
fastskill optimization run --config ./optimize.toml --resume ./optimize-runs/run-1
```

| Option               | Description                                                                                             |
| -------------------- | ------------------------------------------------------------------------------------------------------- |
| `--config <PATH>`    | **Required.** Path to the optimize config file                                                          |
| `--out-dir <DIR>`    | Override `out_dir` from the config file                                                                 |
| `--resume <RUN-DIR>` | Resume from this run directory instead of starting fresh                                                |
| `--no-isolation`     | Score in a shared workspace against the ambient agent environment (disables per-case scoring isolation) |

## optimization resume

Resumes an interrupted run in place.

```bash
fastskill optimization resume ./optimize-runs/run-1
```

| Argument    | Description                                       |
| ----------- | ------------------------------------------------- |
| `<run-dir>` | **Required.** Path to the run directory to resume |

## optimization status

Shows the state of a run: current step, scores, and whether it is still in progress.

```bash
fastskill optimization status ./optimize-runs/run-1
fastskill optimization status ./optimize-runs/run-1 --watch
```

| Argument / Option | Description                             |
| ----------------- | --------------------------------------- |
| `<run-dir>`       | **Required.** Path to the run directory |
| `--watch`         | Poll and re-render every \~2 seconds    |

## optimization inspect

Shows the per-step artifacts of a run.

```bash
fastskill optimization inspect ./optimize-runs/run-1 --step 3
fastskill optimization inspect ./optimize-runs/run-1 --step 3 --show diffs
```

| Argument / Option | Description                                                                    |
| ----------------- | ------------------------------------------------------------------------------ |
| `<run-dir>`       | **Required.** Path to the run directory                                        |
| `--step <N>`      | **Required.** Step number to inspect (0-based, matching `optimization status`) |
| `--show <MODE>`   | `patches`, `diffs`, `gate`, `skips`, or `all` (default `all`)                  |

## optimization export

Writes the best skill document from a completed run to a file.

```bash
fastskill optimization export ./optimize-runs/run-1 --out ./SKILL.md
```

| Argument / Option | Description                                                    |
| ----------------- | -------------------------------------------------------------- |
| `<run-dir>`       | **Required.** Path to the run directory                        |
| `--out <PATH>`    | **Required.** Destination path for the exported skill document |

## See also

* [Optimization overview](/optimize/overview), [configuration](/optimize/configuration), [running](/optimize/running), and [results](/optimize/results)
* [eval Command](/cli-reference/eval-command) for the eval suite the optimizer scores against

