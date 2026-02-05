# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

FastSkill is a Rust-based package manager and operational toolkit for Claude Code-compatible skills. It provides registry services, semantic search, version management, and deployment tooling for AI agent skills at scale.

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

FastSkill follows a **layered architecture** with clear separation of concerns:

```
CLI Layer (src/cli/)
    ↓
Service Layer (FastSkillService)
    ↓
Core/Business Layer (src/core/)
    ↓
Storage Layer (src/storage/)

HTTP Layer (src/http/) - independent of CLI, uses same service layer
```

### Key Modules

- **`src/cli/`** - Command-line interface and argument parsing (14 files)
  - `cli.rs` - Main CLI entry point and command dispatch
  - `commands/` - Individual command implementations (add, search, serve, registry, etc.)
  - `config.rs` - Configuration file handling
  - `utils/` - CLI utilities (API client, messages, install/manifest helpers)

- **`src/core/`** - Core business logic (27 files)
  - `skill_manager.rs` - Skill lifecycle management (register, update, enable/disable)
  - `metadata.rs` - Skill metadata extraction and discovery
  - `vector_index.rs` - Semantic search using OpenAI embeddings + SQLite
  - `registry/` - Registry client, configuration, and authentication
  - `repository.rs` - Unified repository system (git, HTTP, local, ZIP)
  - `manifest.rs` - Project manifest (skills.toml) and lockfile handling
  - `service.rs` - `FastSkillService` orchestrator that initializes all sub-services

- **`src/http/`** - HTTP API server (13 files)
  - `server.rs` - Axum server setup and router configuration
  - `handlers/` - API endpoint handlers (skills, search, registry, auth, etc.)
  - `models.rs` - Request/response types (`ApiResponse<T>`, error handling)
  - `auth/` - JWT authentication middleware and role-based access

- **`src/storage/`** - Storage backends (5 files)
  - `filesystem.rs` - File-based skill storage with metadata caching
  - `git_storage.rs` - Git operations for skill sources
  - `zip_handler.rs` - ZIP package extraction and creation
  - `vector_index_storage.rs` - SQLite persistence for embeddings

- **`src/validation/`** - Skill validation (3 files)
  - `skill_validator.rs` - Validates skill structure and metadata
  - `standard_validator.rs` - Standard SKILL.md format validation
  - `zip_validator.rs` - ZIP package integrity validation

- **`src/events/`** - Event bus for skill lifecycle tracking (2 files)

- **`src/execution.rs`** - Script execution sandboxing and security policies

### Critical Data Structures

#### SkillDefinition
The core data structure representing a skill. Located in `src/core/skill_definition.rs`.

**Key fields:**
- `id: SkillId` - Validated skill identifier
- `skill_file: PathBuf` - Path to SKILL.md
- `source_url/source_type/source_branch` - Tracks skill origin
- `editable: bool` - For local development (like Poetry's `-e` flag)
- `enabled: bool` - Runtime enable/disable without uninstalling
- Execution config: `execution_environment`, `dependencies`, `timeout`

#### FastSkillService
The main service orchestrator in `src/core/service.rs`. Initializes and coordinates all sub-services:
- `SkillManagementService` (skill lifecycle)
- `MetadataService` (metadata extraction)
- `VectorIndexService` (semantic search)
- `StorageBackend` (file operations)
- `RepositoryManager` (multi-source skill discovery)

Used by both CLI commands and HTTP handlers.

#### Repository System
Multi-source skill repository support in `src/core/repository.rs`:

**Repository Types:**
- `GitMarketplace` - Git repos with marketplace.json for skill discovery
- `HttpRegistry` - HTTP-based registries with flat index
- `ZipUrl` - ZIP file downloads from base URL
- `Local` - Local filesystem paths

Configured in `.claude/repositories.toml` with priority-based conflict resolution.

### Command Dispatch Pattern

Commands are divided into two categories:

1. **Service-dependent commands** (add, search, serve, show, update, etc.)
   - Initialize `FastSkillService` first
   - Use shared service layer for operations

2. **Standalone commands** (init, install, publish, auth, registry)
   - Execute without full service initialization
   - Avoid circular dependencies and overhead
   - Registry command has its own modular structure in `src/cli/commands/registry/`

### Vector Search Implementation

Located in `src/core/vector_index.rs`:

1. Skills are embedded using OpenAI's `text-embedding-3-small` model
2. Embeddings stored in SQLite database (`.claude/.fastskill/index.db`)
3. Search uses cosine similarity to rank results
4. Files are content-addressed (SHA256 hashing) to detect changes

**Key trait:** `VectorIndexService` with methods: `add_or_update_skill()`, `search_similar()`, `remove_skill()`

### Event System

Event-driven architecture in `src/events/event_bus.rs`:

**Event types:** `SkillRegistered`, `SkillUpdated`, `SkillUnregistered`, `SkillReloaded`, `SkillEnabled`, `SkillDisabled`

Enables decoupled components to react to skill lifecycle changes (e.g., hot-reload, cache invalidation).

## Configuration Resolution

FastSkill resolves configuration in priority order:

1. CLI arguments
2. Environment variables (e.g., `OPENAI_API_KEY`, `RUST_LOG`)
3. `.fastskill.yaml` in current directory
4. Walk up directory tree to find existing `.claude/skills/`
5. Default to `./.claude/skills/`

### Key Configuration Files

- **`.fastskill.yaml`** - Project configuration (embedding settings, skills directory)
- **`.claude/repositories.toml`** - Multi-repository configuration
- **`.claude/skills.toml`** - Project manifest (like package.json or Cargo.toml)
- **`.claude/skills.lock`** - Lockfile for reproducible installations
- **`.claude/.fastskill/index.db`** - SQLite vector index

## Feature Flags

Defined in `Cargo.toml`:

- `filesystem-storage` (default) - Local filesystem storage for skills
- `registry-publish` (default) - Publishing to registries with AWS S3
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

1. Add command variant to `Commands` enum in `src/cli/commands/mod.rs`
2. Create handler module in `src/cli/commands/`
3. Implement `execute_*()` async function
4. Add dispatch logic in `src/cli/cli.rs`
5. Wire through `FastSkillService` methods if service-dependent

### Adding a new HTTP endpoint

1. Create handler module in `src/http/handlers/`
2. Define request/response types in `src/http/models.rs`
3. Add route to Axum router in `src/http/server.rs`
4. Apply authentication middleware if needed via `src/http/auth/`

### Extending repository support

1. Add new repository type to `RepositoryType` enum in `src/core/repository.rs`
2. Implement `RepositoryClient` trait for new type
3. Add CLI subcommand in `src/cli/commands/registry/`
4. Update formatters in `src/cli/commands/registry/formatters.rs` if needed

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
  Any ZIP (or similar) extraction MUST use the same rule: never join entry names to a base path and write without checking that the result is under the extraction root. This applies to CLI add-from-zip, registry validation worker extraction, and any future extraction code.

- **Tests**  
  For code that extracts archives or resolves untrusted paths, tests MUST include at least one case that uses malicious path components (e.g. `../`, `..\\`, or segments that escape the base). The test MUST assert that no file is created outside the intended directory (or that the operation fails). Shared safe-extraction helpers SHOULD have dedicated tests so reuse does not regress.

## Commit Conventions

Per RFC 2119, the following rule applies:

- **MUST NOT** add any authoring messages for commits suggesting that Claude is a co-author of that commit

Commits should represent human-authored work without AI co-attribution.

## Release Process

- Automatic releases on pushes to `main` (patch version bump)
- Skip with `[skip release]`, `[no release]`, or `[skip ci]` in commit message
- Manual releases via version tags (`v1.2.3`) or workflow dispatch
- Builds 3 binary variants: `x86_64-unknown-linux-musl` (static), `x86_64-unknown-linux-gnu` (glibc), `x86_64-pc-windows-msvc`

## Additional Resources

- **CONTRIBUTING.md** - Full contributor guidelines
- **STYLE.md** - Comprehensive style guide for CLI output and documentation
- **README.md** - User-facing documentation and installation instructions
- **SECURITY.md** - Security policy and vulnerability reporting
