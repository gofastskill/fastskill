# AGENTS.md

This file provides guidance to AI coding agents when working with code in this repository.

## Overview

FastSkill is a Rust-based package manager and operational toolkit for Claude Code-compatible skills. It provides repository services, semantic search, version management, and deployment tooling for AI agent skills at scale.

## Development Commands

### Building and Running

```bash
# Build the project
cargo build

# Run fastskill locally with arguments
cargo run --bin fastskill -- <command>

# Run in release mode (optimized)
cargo build --release
```

### Testing

```bash
# Run all tests with nextest (recommended - faster than cargo test)
cargo nextest run

# Run specific test by name
cargo nextest run -E 'test(test_name)'

# Run tests with all features enabled
cargo nextest run --all-features

# Run tests with specific features
cargo nextest run --features hot-reload
```

### Snapshot Testing

FastSkill uses [insta](https://insta.rs/) for snapshot testing CLI output:

```bash
# Review snapshot changes when tests fail
cargo insta review

# Accept all snapshot changes
cargo insta accept

# Run tests and accept snapshots in one command
cargo insta test --accept --test-runner nextest
```

### Code Quality

```bash
# Format code
cargo fmt --all

# Run clippy linter
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Check for typos
typos

# Check for unused dependencies
cargo shear
```

### Logging and Debugging

```bash
# Enable trace-level logging for fastskill
RUST_LOG=fastskill=trace cargo run --bin fastskill -- <command>

# Enable debug logging for specific modules
RUST_LOG=fastskill::core=info,fastskill::http=debug cargo run --bin fastskill -- <command>
```

## Architecture Overview

FastSkill is a Cargo workspace of three crates:

```
crates/fastskill-cli    - command-line interface (cli-framework AppBuilder), HTTP-server wiring
    ↓ depends on
crates/fastskill-core   - domain logic, storage, HTTP handlers, repositories, vector index
crates/fastskill-evals  - thin adapter over aikit-evals (suite/check/runner/artifact infra)
```

`fastskill-cli` registers its commands and its `serve`/`mcp serve` HTTP surfaces against
`fastskill-core` services; the HTTP layer (`crates/fastskill-core/src/http/`) is not a separate
process — it's mounted by the `serve` command and uses the same core services as every other
command.

### Key Modules

- **`crates/fastskill-cli/src/`** - CLI entry point and command handlers
  - `main.rs` - Builds the `cli_framework::AppBuilder` app: registers every command with
    `path![...]` (e.g. `path!["repos", "add"]`) via `register_out`/`register_out_no_mcp`/`register_group`
  - `registration.rs` - `AppBuilderExt` trait (`register_out`, `register_out_no_mcp`) that routes
    buffered `outln!` output through the MCP-safe context
  - `commands/` - one module (or subdirectory) per command: `add/`, `analyze/`, `cache.rs`,
    `doctor.rs`, `eval/`, `init.rs`, `install.rs`, `list.rs`, `marketplace.rs`, `mcp.rs`,
    `read.rs`, `reindex.rs`, `remove.rs`, `repos/`, `search.rs`, `serve.rs`, `skillopt/`, `update.rs`
  - `config.rs` - Loads `skill-project.toml`, including `load_repositories_from_project()`

- **`crates/fastskill-core/src/core/`** - Core business logic
  - `skill_manager.rs` - `SkillDefinition` and skill lifecycle management (register, update, enable/disable)
  - `metadata.rs` - Skill metadata extraction and discovery
  - `vector_index.rs` - Semantic search using OpenAI embeddings + SQLite
  - `registry/` - Registry client, configuration, and authentication (`auth.rs`, `client.rs`, `config.rs`)
  - `repository.rs` + `repository/client.rs` - Unified repository system (`RepositoryType`: `GitMarketplace`, `HttpRegistry`, `ZipUrl`, `Local`; `RepositoryClient` trait)
  - `manifest.rs` / `manifest/` - Project manifest (`skill-project.toml`) parsing
  - `lock.rs` / `lock/` - `ProjectSkillsLock` (`skills.lock`) and `GlobalSkillsLock` (`global-skills.lock`)
  - `service.rs` - `FastSkillService` orchestrator that initializes all sub-services

- **`crates/fastskill-core/src/storage/`** - Storage backends
  - `filesystem.rs` - File-based skill storage with metadata caching (`StorageBackend` trait)
  - `git.rs` / `git/` - Git operations for skill sources
  - `zip.rs` - ZIP package extraction and creation
  - `hot_reload.rs` - File watching behind the `hot-reload` feature
  - `vector_index.rs` - SQLite persistence for embeddings

- **`crates/fastskill-core/src/validation/`** - Skill validation
  - `skill_validator.rs` - Validates skill structure and metadata
  - `standard_validator.rs` - Standard SKILL.md format validation
  - `zip_validator.rs` - ZIP package integrity validation
  - `field_validation.rs` - Field-level validation helpers

- **`crates/fastskill-core/src/events/event_bus.rs`** - Event bus for skill lifecycle tracking

- **`crates/fastskill-core/src/write_ops.rs`** - The single `WriteOperation`/`WriteHttpRoute` table
  that both the HTTP write-gate and the MCP tool gate derive from (ADR-0003). Adding a mutating
  command or route means adding it here; there is no second list to keep in sync.

- **`crates/fastskill-core/src/http/`** - HTTP API server, mounted by `fastskill serve`
  - `server.rs` - Axum server setup and router configuration
  - `handlers/` - API endpoint handlers: `manifest.rs`, `registry.rs`, `reindex.rs`, `resolve.rs`, `search.rs`, `skills.rs`, `status.rs`
  - `models.rs` - Request/response types, error handling

- **`crates/fastskill-cli/src/commands/mcp.rs`** - `fastskill mcp serve|install|list`; `serve` is
  registered with `register_out_no_mcp` (never returns, and cli-framework's default MCP
  auto-registration has no write gate); `install`/`list` auto-register.

### Critical Data Structures

#### SkillDefinition
The core data structure representing a skill. Located in `crates/fastskill-core/src/core/skill_manager.rs`.

**Key fields:**
- `id: SkillId`, `name`, `description`, `version`, `author` - Identity and metadata
- `skill_file: PathBuf` - Path to SKILL.md
- `reference_files` / `script_files` / `asset_files` - Optional file references
- Execution config: `execution_environment`, `dependencies`, `timeout`
- Provenance fields tracking install intent (origin, editable, etc.)

#### FastSkillService
The main service orchestrator in `crates/fastskill-core/src/core/service.rs`. Initializes and coordinates all sub-services:
- `SkillManagementService` (skill lifecycle)
- `MetadataService` (metadata extraction)
- `VectorIndexService` (semantic search, optional)
- `EmbeddingService` (optional, injected at the CLI/serve edge)
- `RepositoryManager` (optional, multi-source skill discovery)
- `StorageBackend` (file operations)

Used by both CLI commands and HTTP handlers.

#### Repository System
Multi-source skill repository support in `crates/fastskill-core/src/core/repository.rs` and
`repository/client.rs`:

**Repository Types (`RepositoryType`):**
- `GitMarketplace` - Git repos with marketplace.json for skill discovery
- `HttpRegistry` - HTTP-based registries with flat index
- `ZipUrl` - ZIP file downloads from base URL
- `Local` - Local filesystem paths

Configured in `skill-project.toml` under `[tool.fastskill.repositories]`, loaded via
`crates/fastskill-cli/src/config.rs::load_repositories_from_project()`, with priority-based
conflict resolution.

### Command Dispatch Pattern

`crates/fastskill-cli/src/main.rs` builds a `cli_framework::AppBuilder` app and registers every
command explicitly against a `path![...]` (e.g. `path!["repos", "add"]`):

- `register_out` - the default: buffers `outln!` output and drains it into the MCP-safe context
  after the handler resolves, so the command works both as a CLI subcommand and as an MCP tool.
- `register_out_no_mcp` - for commands that never return (`serve`) or make no sense as a
  request/response MCP tool call (`mcp serve`).
- `register_group` - registers a command group's shared metadata/help; some groups (`mcp`) leave
  individual leaves (`install`, `list`) to cli-framework's default auto-registration.

The `repos` command has its own modular structure in `crates/fastskill-cli/src/commands/repos/`.

### Vector Search Implementation

Located in `crates/fastskill-core/src/core/vector_index.rs`:

1. Skills are embedded using OpenAI's `text-embedding-3-small` model
2. Embeddings stored in a SQLite database at `<skills_dir>/.fastskill/index.db`
3. Search uses cosine similarity to rank results
4. Files are content-addressed (SHA256 hashing) to detect changes

**Key trait:** `VectorIndexService` with methods: `add_or_update_skill()`, `search_similar()`, `remove_skill()`

**Note:** The `remove` and `reindex` commands automatically keep the vector index in sync by removing entries for deleted skills or skills no longer on disk.

### Event System

Event-driven architecture in `crates/fastskill-core/src/events/event_bus.rs`:

**Event types:** `SkillRegistered`, `SkillUpdated`, `SkillUnregistered`, `SkillReloaded`, `SkillEnabled`, `SkillDisabled`

Enables decoupled components to react to skill lifecycle changes (e.g., hot-reload, cache invalidation).

## Configuration Resolution

FastSkill resolves configuration in priority order:

1. CLI arguments
2. Environment variables (e.g., `OPENAI_API_KEY`, `RUST_LOG`)
3. `skill-project.toml` `[tool.fastskill]` section (walks up directory tree to find it)
4. Default to `./.claude/skills` as the skills directory if none is configured

### Key Configuration Files

- **`skill-project.toml`** - Project manifest (dependencies, `[tool.fastskill]` settings including
  `skills_directory`, `[tool.fastskill.repositories]`, `[tool.fastskill.embedding]`)
- **`<project_root>/skills.lock`** - `ProjectSkillsLock`, deterministic lockfile for reproducible installations
- **`global-skills.lock`** - `GlobalSkillsLock`, operational lockfile with timestamps for global installs
- **`<skills_dir>/.fastskill/index.db`** - SQLite vector index

Lock file structures are defined in `crates/fastskill-core/src/core/lock.rs` (`LOCK_FORMAT_VERSION = "3.0"`).

## Feature Flags

Defined in `Cargo.toml`:

- `filesystem-storage` (default) - Local filesystem storage for skills
- `hot-reload` (optional) - File watching for automatic skill reloading

Tests requiring optional features are skipped if features not enabled.

## Error Handling

FastSkill uses structured error handling:

- **`thiserror`** for domain-specific error types (e.g., `SkillError`, `RegistryError`)
- **`anyhow`** for error propagation with context
- Use `.with_context(|| format!("..."))` for adding context to errors
- Use `?` operator for propagation

## Testing Guidelines

1. **Unit tests** - Test individual functions and modules
2. **Integration tests** - Test CLI commands and HTTP endpoints
3. **Snapshot tests** - Validate CLI output using insta (in `tests/cli/`)
4. **Helper utilities** - Use `tests/cli/snapshot_helpers.rs` for consistent snapshot testing

When adding tests that modify CLI output, run `cargo insta review` to accept snapshot changes.

## Async Patterns

FastSkill is **async-first** using Tokio:

- All I/O operations are async
- Service traits use `#[async_trait]`
- Use `Arc<dyn Trait>` for shared service references across async tasks
- Main service orchestrator (`FastSkillService`) uses `Arc<RwLock<_>>` for thread-safe state

## Toolchain and Dependencies

- **Rust nightly** required (MSRV defined in `rust-toolchain.toml`)
- **Pure Rust dependencies** - No C compiler needed
- **SQLite bundled** - Uses `rusqlite` with `bundled` feature
- **System git** - Git operations use system `git` binary (not libgit2)

## Common Development Workflows

### Adding a new CLI command

1. Create a handler module (or subdirectory) under `crates/fastskill-cli/src/commands/` with a
   `TypedArgs` struct and an `execute_*()` async function.
2. Register it in `crates/fastskill-cli/src/main.rs` against a `path![...]`, using
   `register_out` (default), `register_out_no_mcp` (never returns, or meaningless as an MCP tool),
   or `register_group` for a command group's shared metadata.
3. Wire through `FastSkillService` methods if service-dependent.
4. If the command mutates FastSkill-managed state, add it to the `WriteOperation` table in
   `crates/fastskill-core/src/write_ops.rs` — that is the sole gate for both the HTTP write-gate
   and the MCP tool gate; there is no separate list to update.
5. Update the documented command/flag tables (README.md, this file) — `docs_subcommand_lists_test.rs`
   and `spec_docs_parity_test.rs` fail `cargo nextest run` if they drift from `fastskill spec`.

### Adding a new HTTP endpoint

1. Create a handler module in `crates/fastskill-core/src/http/handlers/`.
2. Define request/response types in `crates/fastskill-core/src/http/models.rs`.
3. Add the route to the Axum router in `crates/fastskill-core/src/http/server.rs`.
4. If the route mutates state, add it to `WriteHttpRoute` in `crates/fastskill-core/src/write_ops.rs`
   so it's mounted behind the `--enable-write` middleware.
5. Keep handlers consistent with local `fastskill serve` unauthenticated API behavior.

### Extending repository support

1. Add the new repository type to the `RepositoryType` enum in `crates/fastskill-core/src/core/repository.rs`.
2. Implement the `RepositoryClient` trait (`crates/fastskill-core/src/core/repository/client.rs`) for the new type.
3. Add CLI subcommand wiring in `crates/fastskill-cli/src/commands/repos/`.
4. Update formatters in `crates/fastskill-cli/src/commands/repos/formatters.rs` if needed.

## Style and Conventions

Refer to `STYLE.md` for detailed style guidelines. Key points:

- Use "fastskill" (lowercase) in code and commands, not "FastSkill"
- CLI messages use "headline style" (terse, no trailing periods for single sentences)
- Use structured logging with `tracing` crate (`debug!`, `info!`, `warn!`, `error!`)
- User-facing messages go to stdout/stderr directly, not through tracing
- Follow error template: `error: <summary>` with optional hints
- Avoid `.unwrap()` and `.expect()` in production code (enabled as Clippy warnings)

## Security

When handling untrusted input or archives, agents MUST follow these rules:

- **Path traversal**  
  Code that writes files from archive entry names or other untrusted path-like strings MUST validate that the resolved output path stays under the intended base directory. The resolved path MUST be normalized (e.g. without `..` or redundant segments) and MUST have the base directory as a prefix before any filesystem write; otherwise the entry MUST be rejected.

- **Archive extraction**  
  Any ZIP (or similar) extraction MUST use the same rule: never join entry names to a base path and write without checking that the result is under the extraction root. This applies to `crates/fastskill-core/src/storage/zip.rs`, `crates/fastskill-core/src/validation/zip_validator.rs`, and any future extraction code.

- **Tests**  
  For code that extracts archives or resolves untrusted paths, tests MUST include at least one case that uses malicious path components (e.g. `../`, `..\\`, or segments that escape the base). The test MUST assert that no file is created outside the intended directory (or that the operation fails). Shared safe-extraction helpers SHOULD have dedicated tests so reuse does not regress.

## Commit Conventions

Per RFC 2119, the following rules apply:

- **MUST** use conventional commit-style messages: a type (e.g. `feat`, `fix`, `docs`, `chore`, `refactor`, `test`), optional scope in parentheses, and a short description (e.g. `feat(cli): add --dry-run to install`).
- **MUST NOT** add co-author trailers (e.g. `Co-authored-by: Cursor`, `Co-authored-by: Claude`) or any AI/agent attribution in commit messages or footers.
- **MUST NOT** include author-style lines or footers in commit messages; commits MUST NOT contain `Author:` or similar trailers that attribute the change to an AI or tool.
- Commit messages MUST refer only to the change itself and MUST NOT state or imply that the commit was co-authored by Cursor, Claude, or any other agent.

## Release Process

- Automatic releases on pushes to `main` (patch version bump)
- Skip with `[skip release]`, `[no release]`, or `[skip ci]` in commit message
- Manual releases via version tags (`v1.2.3`) or workflow dispatch
- Builds 5 binary variants: `x86_64-unknown-linux-musl` (static), `x86_64-unknown-linux-gnu` (glibc), `x86_64-pc-windows-msvc`, `aarch64-apple-darwin` (Apple Silicon), `x86_64-apple-darwin` (Intel Macs)

## Additional Resources

- **CONTRIBUTING.md** - Full contributor guidelines
- **STYLE.md** - Comprehensive style guide for CLI output and documentation
- **README.md** - User-facing documentation and installation instructions
- **SECURITY.md** - Security policy and vulnerability reporting
