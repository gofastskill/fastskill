# Contributing

## Finding ways to help

We label issues that would be good for a first time contributor as
[`good first issue`](https://github.com/gofastskill/fastskill/issues?q=is%3Aopen+is%3Aissue+label%3A%22good+first+issue%22).
These usually do not require significant experience with Rust or the fastskill code base.

We label issues that we think are a good opportunity for subsequent contributions as
[`help wanted`](https://github.com/gofastskill/fastskill/issues?q=is%3Aopen+is%3Aissue+label%3A%22help+wanted%22).
These require varying levels of experience with Rust and fastskill. Often, we want to accomplish these
tasks but do not have the resources to do so ourselves.

You don't need our permission to start on an issue we have labeled as appropriate for community
contribution as described above. However, it's a good idea to indicate that you are going to work on
an issue to avoid concurrent attempts to solve the same problem.

Please check in with us before starting work on an issue that has not been labeled as appropriate
for community contribution. We're happy to receive contributions for other issues, but it's
important to make sure we have consensus on the solution to the problem first.

Outside of issues with the labels above, issues labeled as
[`bug`](https://github.com/gofastskill/fastskill/issues?q=is%3Aopen+is%3Aissue+label%3A%22bug%22) are the
best candidates for contribution. In contrast, issues labeled with `needs-decision` or
`needs-design` are _not_ good candidates for contribution. Please do not open pull requests for
issues with these labels.

Please do not open pull requests for new features without prior discussion. While we appreciate
exploration of new features, we will almost always close these pull requests immediately. Adding a
new feature to fastskill creates a long-term maintenance burden and requires strong consensus from the fastskill
team before it is appropriate to begin work on an implementation.

## Quickstart for Contributors

Welcome! Here's the fastest path to get fastskill building and running locally:

### Prerequisites

- **Rust nightly** (MSRV defined in `rust-toolchain.toml` - required for latest dependencies)

### Get Started

```shell
# Clone and enter the repository
git clone https://github.com/gofastskill/fastskill.git
cd fastskill

# Build fastskill (rust-toolchain.toml ensures consistent Rust version)
cargo build

# Run tests
cargo nextest run

# Format and lint your code
cargo fmt --all
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Run fastskill locally
cargo run --bin fastskill -- --help

# Run snapshot tests (if you modify output)
cargo insta test --accept --test-runner nextest
```

### Common Development Commands

```shell
# Add a skill from local path
cargo run --bin fastskill -- add ./path/to/skill

# Search for skills
cargo run --bin fastskill -- search "query"

# Start the web server
cargo run --bin fastskill -- serve
```

This quickstart reduces contributor drop-off by getting you productive in under 10 minutes.

## Setup

[Rust](https://rustup.rs/) nightly is required to build fastskill. The exact version and components are pinned in `rust-toolchain.toml` to ensure consistent builds across contributors, CI, and releases.

fastskill uses pure Rust dependencies and does not require a C compiler for building. The SQLite dependency (rusqlite) uses the `bundled` feature, which compiles SQLite from source using the Rust compiler.

### Testing Tools

To run tests effectively, install the recommended testing tools:

```shell
# Install nextest for fast, parallel test execution
cargo install cargo-nextest

# Install insta for snapshot testing
cargo install cargo-insta
```

## Testing

For running tests, we recommend [nextest](https://nexte.st/), a fast, parallel test runner with excellent output and caching.

To run all tests:

```shell
cargo nextest run
```

To run a specific test by name:

```shell
cargo nextest run -E 'test(test_name)'
```

To run tests in a specific file:

```shell
cargo nextest run --run-ignored --package <test_package>
```

To run tests with different output formats:

```shell
cargo nextest run --verbose  # Detailed output
cargo nextest run --quiet    # Minimal output
```

### Snapshot testing

fastskill uses [insta](https://insta.rs/) for snapshot testing CLI output. This provides reliable validation of command output by capturing expected results.

**Key benefits:**
- Catches unintended output changes automatically
- Easier maintenance than brittle `.contains()` assertions
- Clear diff visualization when outputs change
- Supports content normalization (paths, timestamps, etc.)

**Helper utilities are available** in `tests/cli/snapshot_helpers.rs` for consistent snapshot testing:

```rust
use super::snapshot_helpers::{
    run_fastskill_command, cli_snapshot_settings, assert_snapshot_with_settings
};

#[test]
fn test_command_output() {
    let result = run_fastskill_command(&["--help"], None);
    assert_snapshot_with_settings("help_output", &result.stdout, &cli_snapshot_settings());
}
```

**Snapshot workflow:**
- Run `cargo insta review` to review and accept changes
- Use `cargo insta accept` to accept all pending changes
- Snapshots are stored in `tests/cli/snapshots/` directory and committed to git

### Feature flags and test matrix

fastskill supports several feature flags that affect available functionality:

- `filesystem-storage` (default): Enables local filesystem storage for skills
- `registry-publish` (default): Enables publishing skills to registries
- `hot-reload` (optional): Enables file watching for automatic skill reloading during development

#### Test matrix

| Test Type              | Command                                   | Description                                         |
| ---------------------- | ----------------------------------------- | --------------------------------------------------- |
| **Fast local checks**  | `cargo nextest run`                       | Unit tests, integration tests with default features |
| **Full CI equivalent** | `cargo nextest run --all-features`        | All tests with optional features enabled            |
| **Feature-specific**   | `cargo nextest run --features hot-reload` | Tests requiring specific optional features          |

Tests requiring optional features will be skipped if those features are not enabled.

### Local testing

You can invoke your development version of fastskill with `cargo run --bin fastskill -- <args>`. For example:

```shell
# Add a skill from a local path
cargo run --bin fastskill -- add ./path/to/skill

# Search for skills
cargo run --bin fastskill -- search "data processing"

# Start the HTTP API server
cargo run --bin fastskill -- serve

# List installed skills
cargo run --bin fastskill -- show
```

#### Running Tests with nextest

```shell
# Run all tests (recommended - much faster than cargo test)
cargo nextest run

# Run specific test suites
cargo nextest run --package cli_tests
cargo nextest run --package integration_tests

# Run tests matching a pattern
cargo nextest run search

# Run with different feature flags
cargo nextest run --features hot-reload
cargo nextest run --all-features

# Run tests in CI mode (fail fast, different retry settings)
cargo nextest run --profile ci
```

#### Working with Snapshots

Some tests use [insta](https://insta.rs/) for snapshot testing, which captures expected output for validation:

```shell
# Review snapshot changes (run this when tests fail due to output changes)
cargo insta review

# Accept all snapshot changes (use carefully!)
cargo insta accept

# Accept changes for specific test
cargo insta accept --test test_name

# Check snapshots without running tests (for CI validation)
cargo insta test --check
```

**Snapshot Review Process:**
1. When a test fails with snapshot differences, run `cargo insta review`
2. Review the diff between expected and actual output
3. Accept legitimate changes with `cargo insta accept`
4. Reject unexpected changes and fix the underlying issue

**Snapshot Best Practices:**
- Snapshots are committed to version control
- Review snapshot changes carefully - they represent contract changes
- Use snapshot redactions for dynamic content (paths, timestamps, etc.)
- Keep snapshots focused on the essential output being tested

## Formatting

```shell
# Rust
cargo fmt --all

# Markdown, YAML, and other files (requires Node.js)
npx prettier --write .
# or in Docker
docker run --rm -v .:/src/ -w /src/ node:alpine npx prettier --write .
```

## Linting

Linting requires [shellcheck](https://github.com/koalaman/shellcheck) and
[cargo-shear](https://github.com/Boshen/cargo-shear) to be installed separately.

```shell
# Rust
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Shell scripts
shellcheck <script>

# Spell checking
typos

# Unused Rust dependencies
cargo shear
```

### Compiling for Windows from Unix

To run clippy for a Windows target from Linux or macOS, you can use
[cargo-xwin](https://github.com/rust-cross/cargo-xwin):

```shell
# Install cargo-xwin
cargo install cargo-xwin --locked

# Add the Windows target
rustup target add x86_64-pc-windows-msvc

# Run clippy for Windows
cargo xwin clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Note: fastskill's release process uses cross-compilation for the Windows target, but this is typically only needed for CI/release builds, not local development.

## Crate structure

fastskill is a single-crate project. The main binary is in `src/bin/fastskill.rs` and the library code is in `src/lib.rs`.

Key modules include:

- `cli/`: Command-line interface and argument parsing
- `core/`: Core business logic (skill management, registry, etc.)
- `storage/`: Data persistence layer
- `http/`: HTTP API server
- `execution/`: Skill execution engine

For dependency visualization, you can use cargo-tree:

```shell
cargo install cargo-tree
cargo tree
```

## Running inside a Docker container

Skills can potentially execute arbitrary code when loaded. For security when testing skills from untrusted sources, consider running fastskill in a container:

```shell
# Build a containerized version for testing
docker build -t fastskill-testing -f Dockerfile .
docker run --rm -it -v $(pwd)/skills:/app/skills fastskill-testing

# Or use the musl binary in a minimal container for maximum security
docker run --rm -it -v $(pwd)/skills:/app/skills \
  -v $(pwd)/target/x86_64-unknown-linux-musl/release:/app/bin \
  alpine:latest /app/bin/fastskill
```

We recommend using containers when testing skills from untrusted sources or when you want to isolate skill execution from your development environment.

## Tracing and Logging

fastskill uses [tracing](https://github.com/tokio-rs/tracing) for structured logging. You can enable detailed logging using the `RUST_LOG` environment variable:

```shell
# Enable trace-level logging for fastskill
RUST_LOG=fastskill=trace cargo run --bin fastskill -- <command>

# Enable debug logging for all components
RUST_LOG=debug cargo run --bin fastskill -- <command>

# Enable info logging for specific components
RUST_LOG=fastskill::core=info,fastskill::http=debug cargo run --bin fastskill -- <command>
```

## Code Quality Conventions

### Error Handling

fastskill uses `thiserror` for structured error types and `anyhow` for error propagation:

```rust
use thiserror::Error;
use anyhow::{Context, Result};

#[derive(Error, Debug)]
pub enum SkillError {
    #[error("Skill not found: {id}")]
    NotFound { id: String },
    #[error("Invalid skill format: {reason}")]
    InvalidFormat { reason: String },
}

pub fn load_skill(id: &str) -> Result<SkillDefinition> {
    // Use ? for propagation, Context for additional info
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read skill file: {}", path))?;
    // ...
}
```

### Logging and Tracing

Use structured logging with appropriate levels:

```rust
use tracing::{info, warn, error, debug, instrument};

#[instrument(skip(skill))]
pub async fn execute_skill(skill: &SkillDefinition) -> Result<()> {
    info!(skill_id = %skill.id, "Executing skill");

    if let Err(e) = do_execution(skill).await {
        error!(error = %e, "Skill execution failed");
        return Err(e);
    }

    debug!("Skill executed successfully");
    Ok(())
}
```

### Module Organization

- Keep modules focused on single responsibilities
- Use `pub(crate)` for internal APIs, `pub` only for public interfaces
- Prefer associated functions over free functions where appropriate

### Performance Considerations

- Avoid allocations in hot paths
- Use `Arc` for shared ownership when needed
- Prefer stack allocation for small, temporary data

### Testing Guidelines

- Unit tests for individual functions
- Integration tests for CLI commands and HTTP endpoints
- Snapshot tests for command output and API responses
- Add tests when fixing bugs or adding features

### PR Guidelines

- Keep PRs focused and small (under 500 lines preferred)
- Include tests for new functionality
- Update documentation for API changes
- Squash commits when merging (GitHub's "Squash and merge" is preferred)

For messaging conventions, see [`STYLE.md`](STYLE.md).

## Releases

fastskill uses automated release workflows to ensure consistent builds across platforms.

### Release Process

#### Automatic Releases (main branch)

- Pushes to `main` trigger the auto-release workflow
- Version is auto-bumped (patch increment) if not already updated in the PR
- Can be skipped with commit messages containing `[skip release]`, `[no release]`, or `[skip ci]`
- Can also be skipped by adding a `no-release` label to the PR

#### Manual Releases

- Create a version tag (`v1.2.3`) or use workflow dispatch
- Triggers the release workflow which:
  - Verifies version matches Cargo.toml
  - Builds binaries for 3 targets:
    - `x86_64-unknown-linux-gnu` (glibc/dynamic linking)
    - `x86_64-unknown-linux-musl` (static linking, maximum compatibility)
    - `x86_64-pc-windows-msvc` (Windows)
  - Creates GitHub release with artifacts
  - Updates Homebrew (macOS) and Scoop (Windows) package managers

### Toolchain Consistency

The `rust-toolchain.toml` file ensures all builds (local development, CI, releases) use the same Rust version and components for reproducible builds.

### Binary Distribution

| Binary                                       | Target        | Best For                | Compatibility                         |
| -------------------------------------------- | ------------- | ----------------------- | ------------------------------------- |
| `fastskill-x86_64-unknown-linux-musl.tar.gz` | musl static   | Universal compatibility | Works on any Linux, containers, CI/CD |
| `fastskill-x86_64-unknown-linux-gnu.tar.gz`  | glibc dynamic | FIPS/compliance         | Requires glibc 2.38+                  |
| `fastskill-x86_64-pc-windows-msvc.zip`       | Windows MSVC  | Windows users           | Windows 10+                           |

### Release Artifacts

All releases include:

- Pre-built binaries for Linux (2 variants) and Windows
- SHA256 checksums
- Installation instructions in release notes
