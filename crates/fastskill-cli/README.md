# fastskill-cli

Command-line interface crate for FastSkill.

`fastskill-cli` provides the `fastskill` binary and command routing layer on top of `fastskill-core`. Consumers of this crate are developers who want to run or package the CLI executable.

## Build and run

From the workspace root:

```bash
cargo run -p fastskill-cli -- --help
```

Or build only the CLI package:

```bash
cargo build -p fastskill-cli
```

## Usage examples

```bash
# Initialize a project manifest
fastskill init

# Add and install a local skill
fastskill add ./skills/my-skill -e --group dev
fastskill install

# Search skills
fastskill search "text processing"
```

## What this crate owns

- CLI argument parsing and command definitions (`clap`).
- Command dispatch and user-facing error handling.
- Bridge layer between user commands and `fastskill-core` services.

## Related documentation

- Workspace overview: [`../../README.md`](../../README.md)
- Workspace contribution guide: [`../../CONTRIBUTING.md`](../../CONTRIBUTING.md)
- Crate contribution guide: [`CONTRIBUTING.md`](CONTRIBUTING.md)
