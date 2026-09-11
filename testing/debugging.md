# Debugging Guide

FastSkill 0.9.230

Source: https://docs.gofastskill.com/testing/debugging

Release revision: dd983e89e5fee977325b77ae386b879e8310ed26

Documentation revision: dd983e89e5fee977325b77ae386b879e8310ed26



## Overview

The two tools for debugging FastSkill are `fastskill cli doctor` — an environment/configuration health
check — and the global `--verbose` / `-v` flag for detailed command output.

## `fastskill cli doctor`

`cli doctor` inspects your environment and reports what is and isn't set up:

```bash
fastskill cli doctor
```

It runs these checks:

| Check              | What it verifies                                                                                      |
| ------------------ | ----------------------------------------------------------------------------------------------------- |
| `skills_dir`       | The skills directory exists and is a directory                                                        |
| `project_toml`     | `skill-project.toml` is present (walk-up from cwd)                                                    |
| `embedding_config` | `[tool.fastskill.embedding]` is configured (semantic search / reindex enabled)                        |
| `api_key`          | `OPENAI_API_KEY` is set (required for embeddings)                                                     |
| `auth_token`       | An auth token is present (`FASTSKILL_AUTH_TOKEN` or `FASTSKILL_TOKEN`) for remote registry operations |

For machine-readable output (CI, scripts):

```bash
fastskill cli doctor --json
```

Common findings and fixes:

* **`project_toml` failing** — run `fastskill project init` to create `skill-project.toml`.
* **`embedding_config` / `api_key` failing** — add `[tool.fastskill.embedding]` and set
  `OPENAI_API_KEY`; without them semantic search and reindex are disabled (they skip silently, they
  don't error).
* **`auth_token` failing** — set `FASTSKILL_AUTH_TOKEN` (or `FASTSKILL_TOKEN`) if you use a private
  registry.

## Verbose output

Add `--verbose` (or `-v`) to any command to see detailed diagnostic output about what FastSkill is
doing — useful for understanding install resolution, indexing, and registry access:

```bash
fastskill project install --verbose
fastskill skill search "pdf" -v
```

Start with `fastskill cli doctor` to confirm the environment, then re-run the failing command with
`--verbose` to see the detail.


## See also

* [Troubleshooting](/troubleshooting) — common issues and resolutions.

