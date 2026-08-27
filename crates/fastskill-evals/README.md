# fastskill-evals

Thin adapter crate: re-exports the [aikit-evals](https://github.com/goaikit/aikit) evaluation engine and adds FastSkill-specific configuration resolution from `skill-project.toml`.

The actual eval machinery — suite loading (CSV), deterministic checks (`skill_invoked`, `trigger_expectation`, `command_contains`, `file_exists`, `max_tool_calls`), case execution with per-case environment isolation, trace normalization, and artifact persistence — lives upstream in `aikit-evals`. This crate exists so the rest of the workspace has one import path and one place where `[tool.fastskill.eval]` config becomes an `EvalConfig`.

## Install

Add the crate from this workspace:

```toml
[dependencies]
fastskill-evals = { path = "../fastskill-evals" }
```

## What this crate adds

- `config_adapter::resolve_eval_config` — resolves eval configuration (suite CSV path, checks TOML path, timeout, trials, threshold, parallelism) from a project's `skill-project.toml`.
- Everything else is a re-export: the `artifacts`, `checks`, `config`, `runner`, `suite`, and `trace` modules and their public items come verbatim from `aikit-evals`, so path-based callers (`fastskill-cli`) work unchanged.

## Crate boundaries

- `fastskill-evals` MAY depend on `fastskill-core` (for `SkillProjectToml`).
- `fastskill-core` MUST NOT depend on `fastskill-evals`.
- `fastskill-agent-runtime` MUST NOT depend on `fastskill-evals`.

## Related documentation

- Workspace overview: [`../../README.md`](../../README.md)
- Workspace contribution guide: [`../../CONTRIBUTING.md`](../../CONTRIBUTING.md)
- Crate contribution guide: [`CONTRIBUTING.md`](CONTRIBUTING.md)
- End-user eval docs: [`../../webdocs/evals-quality/`](../../webdocs/evals-quality/)
