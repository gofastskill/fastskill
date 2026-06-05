# Contributing to evals-core

`evals-core` is the reusable evaluation engine crate in this workspace. Keep changes focused on deterministic eval execution, stable artifacts, and clear public APIs.

## Scope

- In scope: suite/check parsing, runner interfaces, artifact generation, trace normalization, eval config resolution.
- Out of scope: CLI UX, HTTP handlers, registry publishing, workspace-level command wiring.

## Crate layout

- `src/lib.rs`: public exports and crate-level API surface.
- `src/suite.rs`: suite/case loading and validation.
- `src/checks.rs`: check definitions and scoring logic.
- `src/runner.rs`: runner traits and case execution.
- `src/artifacts.rs`: filesystem outputs for results and summaries.
- `src/trace.rs`: trace conversion and serialization helpers.
- `src/config.rs`: config input resolution.

## Development workflow

From workspace root:

```bash
cargo fmt --all
cargo clippy -p evals-core --all-targets --all-features -- -D warnings
cargo test -p evals-core
```

## Contribution rules

- Keep crate responsibilities narrow and independent from CLI concerns.
- Favor explicit, typed errors (`thiserror`) over opaque failures.
- Preserve artifact and trace formats unless a versioned migration is introduced.
- Add or update tests for parser, scoring, and artifact behavior changes.
- Re-export new public items from `src/lib.rs` when they are part of the intended public API.

## Pull requests

- Describe behavior changes and why they are needed.
- Include tests for new checks, config parsing, or runner behavior.
- Update `README.md` when public usage patterns change.
- Run crate-local checks before requesting review.
