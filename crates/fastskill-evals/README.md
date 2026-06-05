# evals-core

Standalone evaluation infrastructure for agent task execution and scoring.

`evals-core` is a Rust library crate used by `fastskill-core` and other agent tooling to run evaluation suites, execute checks, and persist artifacts without pulling the full FastSkill CLI stack.

## Install

Add the crate from this workspace:

```toml
[dependencies]
evals-core = { path = "../evals-core" }
```

## Quick start

```rust
use evals_core::{load_suite, load_checks, run_eval_case, CaseRunOptions, AikitEvalRunner};
use std::path::Path;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let suite = load_suite(Path::new("evals/suite.toml"))?;
let checks = load_checks(Path::new("evals/checks.toml"))?;
let runner = AikitEvalRunner::new();

let case = &suite.cases[0];
let result = run_eval_case(case, &runner, &checks, &CaseRunOptions::default()).await?;
println!("{:?}", result.status);
# Ok(())
# }
```

## Main modules

- `suite`: load and validate suite definitions.
- `checks`: load check definitions and score outputs.
- `runner`: execute eval cases with an `EvalRunner` implementation.
- `artifacts`: write/read run artifacts and summaries.
- `trace`: normalize execution traces and export JSONL.
- `config`: resolve eval configuration from input sources.

## Typical usage flow

1. Load suite and checks from TOML files.
2. Execute cases with `run_eval_case` or your own loop over `EvalSuite`.
3. Persist artifacts with `write_case_artifacts` and `write_summary`.
4. Use trace helpers for downstream analysis pipelines.

## Related documentation

- Workspace overview: [`../../README.md`](../../README.md)
- Workspace contribution guide: [`../../CONTRIBUTING.md`](../../CONTRIBUTING.md)
- Crate contribution guide: [`CONTRIBUTING.md`](CONTRIBUTING.md)
