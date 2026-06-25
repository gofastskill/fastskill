# Spec 001 — Remove Publish Functionality

**Status:** PROPOSED  
**Branch convention:** `chore/remove-publish`

---

## Background

The `registry-publish` Cargo feature gate (introduced in PR #194) controls a self-hosted skill registry pipeline: a client uploads a skill package, a server-side validation worker checks it, the index is updated on disk, and the artifact is stored in S3 or local blob storage. The feature is **off by default** and has never appeared in a release binary.

Decision: remove the publish subsystem entirely rather than continue maintaining it behind a flag.

**Reasons:**
- The gofastskill marketplace uses a platform-managed deploy workflow (CRD + Operator, spec 003) — not a self-hosted FastSkill server write path.
- `aws-sdk-s3` + `aws-config` add meaningful compile-time and binary weight with no current payoff.
- Dead feature flags compound maintenance: dual code paths, dual CI runs, documentation, onboarding confusion.

**Also removed in this spec:** `fastskill package` (creates a distributable zip). Its only material use case was feeding into publish; without publish it has no place in the product.

---

## Before / After

### Before

```
fastskill serve            → HTTP server including:
                             - skill CRUD + search + MCP  (core purpose)
                             - POST /registry/publish              ← GOES
                             - GET  /registry/publish/status/{id} ← GOES

fastskill auth login       ← GOES (exists only to acquire tokens for publish)
fastskill auth logout      ← GOES
fastskill auth whoami      ← GOES
fastskill publish          ← GOES
fastskill package          ← GOES (packaging step before publish)

Cargo features:
  fastskill-core:  registry-publish = ["aws-sdk-s3", "aws-config"]
  fastskill-cli:   registry-publish = ["fastskill-core/registry-publish", ...]

Dependencies pulled by publish:
  aws-sdk-s3, aws-config  (optional, both crates)
  multer                  (fastskill-core, unconditional)
  base64                  (fastskill-cli, unconditional — JWT parsing in auth_config.rs)
  zip                     (fastskill-cli, unconditional — packaging.rs + publish.rs)
```

### After

```
fastskill serve            → HTTP server:
                             - skill CRUD + search + MCP    (unchanged)
                             - GET /registry/index/skills   (unchanged)
                             - GET /registry/sources        (unchanged)
                             - GET /registry/skills         (unchanged)
                             - GET /registry/refresh        (unchanged)
                             - read-only index file serving (unchanged)
                             ← no publish or package endpoints

fastskill auth             ← command group gone
fastskill publish          ← gone
fastskill package          ← gone

Cargo features:
  fastskill-core:  (registry-publish feature removed)
  fastskill-cli:   (registry-publish feature removed)

Dependencies removed:
  aws-sdk-s3, aws-config  (both crates)
  multer                  (fastskill-core)
  base64                  (fastskill-cli)
  zip                     (fastskill-cli only; fastskill-core keeps it for storage/zip.rs)
```

**Commands that survive unchanged:**
`add`, `analyze`, `doctor`, `eval`, `init`, `install`, `list`, `marketplace`, `read`, `reindex`, `registry`, `repos`, `search`, `serve`, `skillopt`, `update`

---

## What is NOT removed

| Item | Why kept |
|---|---|
| `crates/fastskill-core/src/core/registry/client.rs` | Downloads skills from a registry for `install` / `add` |
| `crates/fastskill-core/src/core/registry/auth.rs` | `Auth` trait + GitHub PAT / SSH key / API key — used by `registry/client.rs` and `repository/client.rs` for authenticated downloads |
| `crates/fastskill-core/src/core/registry/config.rs` | Registry URL config for `fastskill registry add/remove/list` |
| `crates/fastskill-core/src/core/registry_index.rs` | Read path types: `scan_registry_index`, `SkillSummary`, `ListSkillsOptions`, `VersionEntry` (on-disk format), `read_skill_versions`, `IndexMetadata` — all used by `GET /registry/index/skills` and CLI search. Some dead write-path symbols are pruned (see below). |
| `crates/fastskill-core/src/http/handlers/registry.rs` | Serves read-only registry browse/search endpoints |
| `crates/fastskill-core/src/storage/zip.rs` + `ZipHandler` | Used by `add/sources.rs` to unpack downloaded skill zips |
| `crates/fastskill-core/src/events/event_bus.rs` | Fires local skill-store events (`SkillRegistered`, `HotReloadEnabled`, etc.) — unrelated to publish |

---

## Complete file inventory

### Files to DELETE

#### fastskill-core — server-side publish pipeline (5 files)

| File | Lines | What it does |
|---|---|---|
| `crates/fastskill-core/src/core/blob_storage.rs` | 357 | `BlobStorage` trait + `LocalBlobStorage` + S3 impl. Only consumed by the publish server path and `ServiceConfig`. |
| `crates/fastskill-core/src/core/registry/index_manager.rs` | 523 | `IndexManager::atomic_update` — writes `VersionEntry` NDJSON to the on-disk registry index during publish. |
| `crates/fastskill-core/src/core/registry/staging.rs` | 359 | `StagingManager` — holds uploaded packages in a temp directory pending validation. |
| `crates/fastskill-core/src/core/registry/validation_worker.rs` | 649 | `ValidationWorker` — async background task that unzips and validates staged packages. |
| `crates/fastskill-core/src/http/handlers/registry_publish.rs` | 377 | Axum handlers for `POST /registry/publish` and `GET /registry/publish/status/{job_id}`. |

#### fastskill-core — publish tests (2 files)

| File | What it tests |
|---|---|
| `crates/fastskill-core/tests/index_manager_test.rs` | `IndexManager` write path |
| `crates/fastskill-core/tests/registry_atomic_test.rs` | Concurrent / sequential `IndexManager::atomic_update` |

#### fastskill-cli — client-side publish + package pipeline (5 files)

| File | Lines | What it does |
|---|---|---|
| `crates/fastskill-cli/src/commands/publish.rs` | 837 | `fastskill publish` — packages skill, authenticates, POSTs to `/registry/publish`, polls status. |
| `crates/fastskill-cli/src/commands/package.rs` | 732 | `fastskill package` — creates a distributable `.zip` artifact locally. Primary use case was feeding into publish. |
| `crates/fastskill-cli/src/commands/auth.rs` | 303 | `fastskill auth login/logout/whoami` — manages per-registry JWT tokens. Exists solely to support publish. |
| `crates/fastskill-cli/src/auth_config.rs` | 386 | Reads/writes `~/.config/fastskill/auth.toml`. Only callers: `publish.rs` and `auth.rs`. |
| `crates/fastskill-cli/src/utils/api_client.rs` | ~240 | `ApiClient::publish_package`, `get_publish_status`, `PublishApiResponse`, `PublishStatusApiResponse`. Entire file is gated via `#[cfg(feature = "registry-publish")]` in `utils.rs`. |

#### fastskill-core — packaging (1 file)

| File | Lines | What it does |
|---|---|---|
| `crates/fastskill-core/src/core/packaging.rs` | 369 | `package_skill`, `package_skill_with_id`, `calculate_checksum`, `create_build_metadata`. Only called from `commands/package.rs` (being deleted) and `commands/publish.rs` (being deleted). |

#### Root integration tests and fixtures

| Path | Reason |
|---|---|
| `tests/integration_registry_publish_test.rs` | End-to-end publish flow test |
| `tests/fixtures/test-skill-registry-publish/` | Fixture directory (4 files) used only by the above test |

#### CLI e2e tests and snapshots

| Path |
|---|
| `tests/cli/auth_e2e_tests.rs` |
| `tests/cli/package_tests.rs` |
| `tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_login_custom_role.snap` |
| `tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_login_invalid_credentials.snap` |
| `tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_login_invalid_port.snap` |
| `tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_login_success.snap` |
| `tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_login_unreachable.snap` |
| `tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_logout_nonexistent.snap` |
| `tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_logout_success.snap` |

---

### Files to MODIFY (15 files)

---

#### `crates/fastskill-core/Cargo.toml`

Remove:
```toml
# line 52 — only used in registry_publish.rs handler
multer.workspace = true

# lines 95–96
aws-sdk-s3 = { workspace = true, optional = true }
aws-config = { workspace = true, optional = true }

# line 113
registry-publish = ["aws-sdk-s3", "aws-config"]
```

Keep `zip.workspace = true` — still required by `storage/zip.rs` for unpacking downloaded skills.

---

#### `crates/fastskill-core/src/core/registry.rs`

Remove three submodule declarations and two re-export lines:

```rust
// REMOVE:
pub mod index_manager;
pub mod staging;
pub mod validation_worker;

// REMOVE:
pub use staging::{StagingManager, StagingMetadata, StagingStatus};
pub use validation_worker::{ValidationWorker, ValidationWorkerConfig};
```

Keep `pub mod auth`, `pub mod client`, `pub mod config` and their `pub use` lines unchanged.

---

#### `crates/fastskill-core/src/core/mod.rs`

Remove `blob_storage` and `packaging` module declarations and their re-export blocks:

```rust
// REMOVE line 4:
pub mod blob_storage;

// REMOVE line 15:
pub mod packaging;

// REMOVE line 36:
pub use blob_storage::{create_blob_storage, BlobStorage, BlobStorageConfig, LocalBlobStorage};

// REMOVE lines 70–72:
pub use packaging::{
    calculate_checksum, create_build_metadata, package_skill, package_skill_with_id,
};
```

Also trim the `registry_index` re-export block (currently lines 85–88) to remove the six symbols that are only used by the files being deleted:

```rust
// BEFORE:
pub use registry_index::{
    create_registry_structure, get_skill_index_path, get_version_metadata, migrate_index_format,
    read_skill_versions, IndexMetadata, VersionEntry, VersionMetadata,
};

// AFTER (keep only what live code references):
pub use registry_index::{read_skill_versions, IndexMetadata, VersionEntry, VersionMetadata};
```

Symbols removed from re-export: `create_registry_structure`, `get_skill_index_path`, `get_version_metadata`, `migrate_index_format`. Note `get_skill_index_lock_path` and `ScopedSkillName` are not in this re-export block but must also be pruned from `registry_index.rs` itself (see below).

---

#### `crates/fastskill-core/src/core/registry_index.rs`

Remove six publish-write-path symbols. Their only callers were `index_manager.rs` and `validation_worker.rs` (both being deleted):

| Symbol | Approx. line | Remove |
|---|---|---|
| `ScopedSkillName` struct + impl | 16–37 | yes |
| `create_registry_structure` fn | 38–45 | yes |
| `get_skill_index_path` fn | 46–70 | yes |
| `get_skill_index_lock_path` fn | 72–83 | yes |
| `get_version_metadata` fn | 184–222 | yes |
| `migrate_index_format` fn | 342–end area | yes |

Keep everything else: `VersionEntry`, `VersionMetadata`, `IndexMetadata`, `read_skill_versions`, `scan_registry_index`, `SkillSummary`, `ListSkillsOptions`, and any helpers they call internally.

---

#### `crates/fastskill-core/src/core/service.rs`

Remove the `BlobStorageConfig` import and two fields from `ServiceConfig`:

```rust
// REMOVE (line 3):
use crate::core::blob_storage::BlobStorageConfig;

// REMOVE from struct body (lines 42, 45):
/// Staging directory for registry publishing
pub staging_dir: Option<PathBuf>,
pub registry_blob_storage: Option<BlobStorageConfig>,

// REMOVE from Default impl (lines 66–67):
staging_dir: None,
registry_blob_storage: None,
```

---

#### `crates/fastskill-core/src/http/handlers/mod.rs`

Remove two lines:

```rust
// REMOVE:
#[cfg(feature = "registry-publish")]
pub mod registry_publish;
```

---

#### `crates/fastskill-core/src/http/server.rs`

Remove every `#[cfg(feature = "registry-publish")]` block. Confirmed occurrences at lines 3–9, 25, 64, 88, 113, 118, 357–362, 455, 459. Specifically:

- The `use crate::http::handlers::registry_publish;` import.
- Any `AppState` fields holding `StagingManager`, `ValidationWorker`, or `Arc<dyn BlobStorage>`.
- S3 / staging startup validation (`error_msg.push_str("S3 configuration is required...")`).
- The two route registrations in `create_registry_api_routes_v1`:
  ```rust
  .route("/registry/publish", post(registry_publish::publish_package))
  .route("/registry/publish/status/{job_id}", get(registry_publish::get_publish_status))
  ```

---

#### `crates/fastskill-cli/Cargo.toml`

Remove:

```toml
# line 71 — only used in auth_config.rs for JWT decoding
base64.workspace = true

# lines 72-ish — only used in commands/package.rs and commands/publish.rs
zip.workspace = true

# lines 92–93
aws-sdk-s3 = { workspace = true, optional = true }
aws-config = { workspace = true, optional = true }

# line 103
registry-publish = ["fastskill-core/registry-publish", "dep:aws-sdk-s3", "dep:aws-config"]
```

Keep `sha2.workspace = true` — used in `commands/reindex.rs`. Keep `vendored-openssl` — still required for musl cross-compilation.

---

#### `crates/fastskill-cli/src/commands/mod.rs`

Remove three lines:

```rust
// REMOVE:
pub mod auth;

// REMOVE:
pub mod package;

// REMOVE:
#[cfg(feature = "registry-publish")]
pub mod publish;
```

---

#### `crates/fastskill-cli/src/mod.rs`

Remove:

```rust
// REMOVE line 4:
pub mod auth_config;
```

---

#### `crates/fastskill-cli/src/utils.rs`

Remove:

```rust
// REMOVE lines 3–4:
#[cfg(feature = "registry-publish")]
pub mod api_client;
```

---

#### `crates/fastskill-cli/src/main.rs`

Five changes:

1. Remove `mod auth_config;` (line 14).

2. Remove `auth` and `package` from the `use commands::{...}` import (line 67):
   ```rust
   // BEFORE:
   use commands::{add, analyze, auth, doctor, eval, init, install, list, marketplace, package, read, reindex, ...};
   // AFTER:
   use commands::{add, analyze, doctor, eval, init, install, list, marketplace, read, reindex, ...};
   ```

3. Remove the feature-gated publish import (lines 64–65):
   ```rust
   // REMOVE:
   #[cfg(feature = "registry-publish")]
   use commands::publish;
   ```

4. Remove the entire `auth` command registration block (~lines 368–401) that registers `path!["auth"]`, `path!["auth", "login"]`, `path!["auth", "logout"]`, `path!["auth", "whoami"]`.

5. Remove the `package` command registration block (~lines 205–212):
   ```rust
   // REMOVE:
   builder.register(
       path!["package"],
       |_ctx, args: package::PackageArgs| async move {
           package::execute_package(args) ...
       },
   )?
   ```

6. Remove the `#[cfg(feature = "registry-publish")]` publish registration block (~lines 215–220).

---

#### `crates/fastskill-cli/src/config.rs`

Three changes:

1. Update import on line 8:
   ```rust
   // BEFORE:
   use fastskill_core::{core::BlobStorageConfig, ServiceConfig};
   // AFTER:
   use fastskill_core::ServiceConfig;
   ```

2. Remove the `registry_blob_storage` block (lines 206–229) — the `if let (Ok(bucket), Ok(region)) = ...` block that reads `S3_BUCKET`, `S3_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `S3_ENDPOINT`, `BLOB_BASE_URL`.

3. Remove `staging_dir` and its references in the `ServiceConfig` initialiser (lines 235, 241, 243):
   ```rust
   // REMOVE:
   let staging_dir = env::var("REGISTRY_STAGING_DIR").ok().map(PathBuf::from);

   // REMOVE from ServiceConfig { ... }:
   registry_blob_storage,
   staging_dir,
   ```

---

#### `tests/cli/mod.rs`

Remove two lines:

```rust
// REMOVE:
pub mod auth_e2e_tests;
pub mod package_tests;
```

---

#### `.github/workflows/test.yml`

Remove the second `cargo clippy` step (lines 31–33) and its comment. It was added to lint the default-feature config with `registry-publish` off — that concern disappears when the feature is gone:

```yaml
# REMOVE:
# Also lint the default-feature config (registry-publish OFF) — this is what
# release binaries are built from, so it must stay clean too.
- run: cargo clippy --workspace --all-targets
```

One clippy pass remains (`--all-features`).

---

## Documentation changes

### webdocs — DELETE (5 files)

These pages document removed commands or features entirely. Delete the files and remove their nav entries from `mint.json`.

| File | Why deleted |
|---|---|
| `webdocs/cli-reference/auth-command.mdx` | Documents `fastskill auth login/logout/whoami` |
| `webdocs/cli-reference/package-command.mdx` | Documents `fastskill package` |
| `webdocs/cli-reference/publish-command.mdx` | Documents `fastskill publish` |
| `webdocs/integration/blob-storage.mdx` | Documents S3/blob storage config for registry-publish |
| `webdocs/integration/cicd-pipelines.mdx` | Entire page is `fastskill package` + `fastskill publish` CI patterns; no content remains after removal |

### webdocs — MODIFY (9 files)

#### `webdocs/mint.json`

Remove five entries from the navigation array:

```json
// REMOVE from cli-reference group:
"cli-reference/auth-command",
"cli-reference/package-command",
"cli-reference/publish-command",

// REMOVE from integration group:
"integration/blob-storage",
"integration/cicd-pipelines"
```

#### `webdocs/cli-reference/overview.mdx`

- Remove the `<Card title="fastskill auth" ...>` card block (lines 105–107).
- Remove the `<Card title="fastskill package" ...>` card block (lines 113–115).
- Remove the `<Card title="fastskill publish" ...>` card block (lines 116–118).
- Remove the "opt-in registry publishing builds" config section at the bottom (~lines 192–204) that shows `.fastskill/publish.toml` with `[blob_storage]`.
- In the skill-authoring table (~lines 65–67), remove the three `fastskill package …` rows.

#### `webdocs/cheatsheet.mdx`

Remove any rows or sections covering `fastskill auth`, `fastskill package`, or `fastskill publish`.

#### `webdocs/quickstart.mdx`

Remove the `fastskill package` step (line 112). If it is part of a "ship your skill" section, remove the entire section or replace with a note that distribution is handled by the platform.

#### `webdocs/registry/index-system.mdx`

The page documents the on-disk NDJSON format (still valid) but frames it entirely around the publish write path. Rewrite to:
- Keep: format description, directory structure, `VersionEntry` JSON schema, how to read the index via `GET /registry/index/skills`.
- Remove: all "publishing process" walkthroughs, `fastskill package` + `fastskill publish` code blocks, JWT-scope extraction copy, blob storage URLs, "The registry index is maintained automatically during publishing" paragraph, "How Versions Are Added" section that uses `fastskill publish`.

#### `webdocs/registry/overview.mdx`

Remove any references to `fastskill publish`, `fastskill package`, or `registry-publish` feature. Keep registry browsing, `fastskill registry add/remove/list`, and install workflows.

#### `webdocs/registry/sources.mdx`

Remove any `fastskill auth` or publish-related content. Registry sources (git, local, http) are unaffected — keep those.

#### `webdocs/welcome.mdx`

- Line 22: remove "then discovery and optional operator publishing when you distribute outward" — stop at "validation and `fastskill eval` for quality gates".
- Line 243: remove "Optional private sources and operator publish flows when you distribute".
- Line 279: remove the link to registry overview "when you are ready to publish or browse catalogs" — drop the "publish" qualifier.

#### `webdocs/index.mdx`

- Line 16: remove "and **publish** flows when you ship skills to others" from the description sentence.
- Line 44: remove "`package` and `publish` when you distribute" from the registry card copy.
- Line 68: remove the "Authors" bullet that says "package when ready; publish requires an operator build with `registry-publish`."
- Line 71: remove "package, and optional publish in CI" from the Automation bullet.

---

### READMEs — MODIFY (2 files)

#### `README.md`

Remove line 90 from the command reference table:

```markdown
<!-- REMOVE this row: -->
| `fastskill package` | Package skills for distribution |
```

#### `crates/fastskill-core/README.md`

Remove line 104:

```markdown
<!-- REMOVE: -->
- `registry-publish`: opt-in registry publishing support.
```

---

### Project docs — MODIFY (3 files)

#### `CONTEXT.md`

- **Artifact** definition (line 57): remove the sentence "A packaged skill produced by `package` — a ZIP with change-detection/version-bump. Input to `publish`." The Artifact concept itself may be removed or rewritten to reflect that packaging is no longer a first-class CLI operation.
- **Registry Index** definition (line 60): remove "The catalog `publish` writes (alongside blobs)". Reframe as: "The on-disk NDJSON catalog read by `fastskill serve` and `registry search`; populated externally (e.g. by the platform operator)."

#### `AGENTS.md`

- Line 176: remove `publish` and `auth` from the "Standalone commands" list. Updated: `(init, install, registry)`.
- Line 225: remove the `registry-publish (default) - Publishing to registries with AWS S3` feature bullet.

#### `CONTRIBUTING.md`

- Line 169: remove the `registry-publish (default): Enables publishing skills to registries` feature entry.

---

## Complete modified-file list

### Source — deleted (18 items)

```
crates/fastskill-core/src/core/blob_storage.rs
crates/fastskill-core/src/core/packaging.rs
crates/fastskill-core/src/core/registry/index_manager.rs
crates/fastskill-core/src/core/registry/staging.rs
crates/fastskill-core/src/core/registry/validation_worker.rs
crates/fastskill-core/src/http/handlers/registry_publish.rs
crates/fastskill-core/tests/index_manager_test.rs
crates/fastskill-core/tests/registry_atomic_test.rs
crates/fastskill-cli/src/commands/publish.rs
crates/fastskill-cli/src/commands/package.rs
crates/fastskill-cli/src/commands/auth.rs
crates/fastskill-cli/src/auth_config.rs
crates/fastskill-cli/src/utils/api_client.rs
tests/integration_registry_publish_test.rs
tests/fixtures/test-skill-registry-publish/   (directory)
tests/cli/auth_e2e_tests.rs
tests/cli/package_tests.rs
tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_login_custom_role.snap
tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_login_invalid_credentials.snap
tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_login_invalid_port.snap
tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_login_success.snap
tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_login_unreachable.snap
tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_logout_nonexistent.snap
tests/cli/snapshots/cli_tests__cli__snapshot_helpers__auth_logout_success.snap
```

### Source — modified (15 files)

```
crates/fastskill-core/Cargo.toml
crates/fastskill-core/src/core/registry.rs
crates/fastskill-core/src/core/mod.rs
crates/fastskill-core/src/core/registry_index.rs
crates/fastskill-core/src/core/service.rs
crates/fastskill-core/src/http/handlers/mod.rs
crates/fastskill-core/src/http/server.rs
crates/fastskill-cli/Cargo.toml
crates/fastskill-cli/src/commands/mod.rs
crates/fastskill-cli/src/mod.rs
crates/fastskill-cli/src/utils.rs
crates/fastskill-cli/src/main.rs
crates/fastskill-cli/src/config.rs
tests/cli/mod.rs
.github/workflows/test.yml
```

### Docs — deleted (5 files)

```
webdocs/cli-reference/auth-command.mdx
webdocs/cli-reference/package-command.mdx
webdocs/cli-reference/publish-command.mdx
webdocs/integration/blob-storage.mdx
webdocs/integration/cicd-pipelines.mdx
```

### Docs — modified (14 files)

```
webdocs/mint.json
webdocs/cli-reference/overview.mdx
webdocs/cheatsheet.mdx
webdocs/quickstart.mdx
webdocs/registry/index-system.mdx
webdocs/registry/overview.mdx
webdocs/registry/sources.mdx
webdocs/welcome.mdx
webdocs/index.mdx
README.md
crates/fastskill-core/README.md
CONTEXT.md
AGENTS.md
CONTRIBUTING.md
```

---

## Verification checklist

Run in order; each must be green before the next.

```bash
# 1. Clean build — no aws-sdk, no multer, no missing modules
cargo build --workspace

# 2. All tests pass
cargo nextest run --workspace --retries 2

# 3. No warnings
cargo clippy --workspace --all-features -- -D warnings

# 4. Formatting clean
cargo fmt --all -- --check
```

Confirm: `fastskill --help` no longer lists `auth`, `package`, or `publish` subcommands. All other commands and help text are unchanged.

---

## Implementation order

1. **Delete all source files** — removes bulk dead code and surfaces remaining compile errors cleanly.
2. **`registry.rs`** — remove three submodule declarations and their `pub use` lines.
3. **`core/mod.rs`** — remove `blob_storage` and `packaging` modules, their re-exports, and the four dead `registry_index` symbols.
4. **`registry_index.rs`** — remove the six write-path symbols.
5. **`service.rs`** — remove `staging_dir` and `registry_blob_storage` fields.
6. **`http/handlers/mod.rs`** + **`http/server.rs`** — remove all `#[cfg(feature = "registry-publish")]` blocks.
7. **Both `Cargo.toml` files** — drop feature flag, AWS deps, `multer`, `base64`, `zip` (CLI only).
8. **CLI entrypoints** — `main.rs`, `commands/mod.rs`, `mod.rs`, `utils.rs`, `config.rs`.
9. **Test wiring** — `tests/cli/mod.rs`, delete snapshots.
10. **CI** — `.github/workflows/test.yml`, remove second clippy pass.
11. `cargo build --workspace` → iterate until green.
12. `cargo nextest run --workspace` → confirm all passing.
13. **Docs** — delete 5 webdocs files, update 14 docs files as specified above.
