# Contributing to fastskill-core

`fastskill-core` contains the shared library logic for FastSkill. Changes here should prioritize stable APIs, predictable behavior, and clear separation between domain logic and integration surfaces.

## Scope

- In scope: core services, metadata/manifest handling, validation, storage/search abstractions, HTTP service internals, eval integration.
- Out of scope: CLI argument UX and command orchestration (handled in `fastskill-cli`).

## Crate layout

- `src/core/`: main domain services (project config, dependency resolution, routing, registry, skill management).
- `src/storage/`: filesystem, git, vector index, and hot-reload integration.
- `src/search/`: local and remote search behavior.
- `src/validation/`: validators for skill/package shape and safety.
- `src/http/`: server and handler modules for API surfaces.
- `src/eval_config_adapter.rs` and `src/lib.rs` re-exports: eval integration.

## Development workflow

From workspace root:

```bash
cargo fmt --all
cargo clippy -p fastskill-core --all-targets --all-features -- -D warnings
cargo test -p fastskill-core --all-features
```

## Contribution rules

- Keep modules focused and avoid cross-layer coupling.
- Prefer extending existing service abstractions before adding new entry points.
- Use `thiserror` for domain errors and `anyhow` for top-level propagation only.
- Add tests for changed behavior in validation, storage, search, or service orchestration.
- When changing public exports in `src/lib.rs`, update crate docs and usage examples.

## Pull requests

- Explain user-visible behavior changes and integration impact.
- Include tests for new logic and edge cases directly related to the change.
- Keep API changes deliberate; avoid broad refactors mixed with feature work.
