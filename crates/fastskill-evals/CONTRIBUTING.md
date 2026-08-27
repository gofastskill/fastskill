# Contributing to fastskill-evals

`fastskill-evals` is a thin adapter over the upstream `aikit-evals` engine. Keep it thin.

## Scope

- In scope: config resolution from `skill-project.toml` (`config_adapter`), and the re-export surface that the rest of the workspace imports.
- Out of scope: the eval engine itself. Suite parsing, checks, the runner, isolation, traces, and artifacts live upstream in [`aikit-evals`](https://github.com/goaikit/aikit) — change them there, then bump the pin here (note: the pin travels aikit → cli-framework → fastskill; all three must move together).
- Also out of scope: CLI UX, HTTP handlers, registry publishing (those live in `fastskill-cli` / `fastskill-core`).

## Crate layout

- `src/lib.rs`: re-exports from `aikit_evals` plus the crate-boundary rules.
- `src/config_adapter.rs`: `[tool.fastskill.eval]` → `EvalConfig` resolution.

## Development workflow

From workspace root:

```bash
cargo fmt --all
cargo clippy -p fastskill-evals --all-targets --all-features -- -D warnings
cargo test -p fastskill-evals
```

## Pull requests

- Describe behavior changes and why they are needed.
- Config-resolution changes need tests here; engine changes need tests upstream.
- Update `README.md` when public usage patterns change.
