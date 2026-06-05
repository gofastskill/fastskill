# Contributing to fastskill-cli

`fastskill-cli` is the user-facing command layer for FastSkill. Keep changes focused on command UX, argument modeling, and clear command-to-service wiring.

## Scope

- In scope: command definitions, argument parsing, output formatting, command execution flow, CLI-specific config handling.
- Out of scope: core domain behavior and storage implementations (handled in `fastskill-core`).

## Crate layout

- `src/main.rs`: process entry point, parse/exit handling.
- `src/cli.rs`: top-level CLI structure and command dispatch.
- `src/commands/`: subcommand implementations.
- `src/utils/`: shared command helpers and formatting helpers.
- `src/error.rs`: CLI-specific error model and exit codes.

## Development workflow

From workspace root:

```bash
cargo fmt --all
cargo clippy -p fastskill-cli --all-targets --all-features -- -D warnings
cargo test -p fastskill-cli
```

## Contribution rules

- Keep command behavior explicit and consistent across subcommands.
- Do not duplicate core business logic in command handlers.
- Return actionable errors with stable exit semantics.
- Add tests for output or behavior changes, especially for argument validation and command flow.
- Keep user-facing text concise and consistent with existing command style.

## Pull requests

- Describe changed command behavior and expected user impact.
- Include tests for modified commands.
- Update this crate README if command examples or invocation patterns change.
