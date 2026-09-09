# ADR-0005 — Install/update/reindex live in core; one `Origin` model

**Status:** Accepted
**Date:** 2026-07-08
**Supersedes/relates:** [ADR-0003](0003-serve-trust-boundary-and-edge-auth.md) (WRITE-GATE), [ADR-0004](0004-bare-version-is-exact.md) (bare version = exact pin, registry-scoped), [ADR-0002](0002-conditional-semantic-indexing.md) (reindex conditional on an Embedding provider). Motivated by [spec 003](../../specs/003-browser-skill-management.md).

## Context

`fastskill serve` ships a bundled web UI. To let a local user *manage* skills from the browser (install from a source, update, remove), the HTTP handlers need to run the same install/update/reindex work the CLI does. Two facts, surfaced while auditing the codebase (PR #201), make this non-trivial:

1. **The orchestration lives in the CLI crate.** `execute_add` (install), the update apply path, and `execute_reindex` are in `fastskill-cli`. An HTTP handler in `fastskill-core` cannot call *up* into the CLI crate. The `reindex` handler already returns `501` for exactly this reason, and the `upgrade` handler *shells out* to the `fastskill` binary — a request-body-to-subprocess path we do not want (against the SEC-2 hardening direction).

2. **"Where a skill comes from" is modelled four different ways.** There are two distinct types both named `SkillSource` (one in `core::manifest`, one in the CLI `add` command), plus `SourceType`, plus nine flat `source_*` fields on `SkillDefinition` (`source_url`, `source_type`, `source_branch`, `source_tag`, `source_subdir`, `installed_from`, `commit_hash`, `fetched_at`, `editable`). The glossary (CONTEXT.md) additionally **bans "source"** as a concept (it is a deprecated command alias folded into `repos`) and reserves **Repository** for a *configured* place skills are fetched from.

We must decide (a) how the browser write-handlers obtain install/update/reindex capability, and (b) what the canonical vocabulary/type for a skill's provenance is.

## Decision

### 1. The domain logic moves into `fastskill-core`; the CLI becomes a thin caller

Introduce core service methods on `FastSkillService`:

- `add_from_origin(origin, mode)` where `mode ∈ {Fresh, Update}` — resolve the `Origin` → fetch → validate → write skills dir → update Manifest + Lock → reindex-if-provider. **Add and update are one operation**, differing only in policy (see below). (Named `add_from_origin`, *not* `install`: in this codebase `install` already means the manifest-reconcile-with-dependencies flow — CONTEXT.md "install = Manifest → skills dir". The per-`Origin`, single-skill operation is `add` semantics.)
- a **core reindex seam** — reindex is domain logic (skills dir → Vector index) and belongs in core.

Both the HTTP handlers **and** the CLI verbs (`execute_add`/`execute_update`/`execute_reindex`) call these methods. The CLI commands become thin wrappers (arg parsing + human output) over the shared core path.

The genuinely CLI-flavoured concerns are **construction-time dependencies, not logic**, and are supplied at `serve`/CLI startup:

- **Embedding provider** — reading the API key from env/config happens at startup (in the CLI, where config resolution belongs); the constructed provider (already a core abstraction, `OpenAIEmbeddingService`) is handed to the service. Reindex then runs iff a provider is present, else **skips silently** (ADR-0002).
- **Progress reporting** — a UI concern. The HTTP path returns a structured `Result`; it does not need live progress. The CLI keeps its progress output in its wrapper.

`UpdateService` is **retained but narrowed** to what it already is: the read-only *"is anything newer?"* query (`check_updates`/`resolve_updates`) that backs `check`/`--dry-run` (the update *preflight*). The *apply* half is `add_from_origin(origin, Update)`, gated by a `preflight(origin) → {UpToDate | Immutable | Updatable}` step.

**Rejected — dependency-inversion / trait injection (core defines `SkillInstaller`/`Reindexer` traits, CLI implements and injects them):** this keeps the orchestration physically in the CLI and adds an indirection layer, but the logic (resolve → fetch → write → lock → reindex) is domain logic that has no reason to live in the CLI. Injection is warranted for the *dependencies* (the embedding provider), not for the *logic*. We inject the provider; we move the logic.

**Rejected — keep shelling out:** re-introduces the request-body → `Command::new` subprocess surface (SEC-2), and cannot return structured results.

**Rejected — leave the CLI on its own separate install path, add a parallel core method only for HTTP:** ships *two* install orchestrations guaranteed to drift. The whole point is one path.

### 2. One canonical `Origin` type replaces the four provenance models

**`Origin`** (in `fastskill-core`) is *where a single installed skill came from* — serving both the **input** to an install and the **provenance** recorded on the installed skill (one type, one truth). Variants:

| Variant | Fields | Versioning |
|---|---|---|
| `git` | `url`, ref (`branch`\|`tag`\|`commit`), `subdir?` | ref-based, versionless |
| `local` | `path`, `editable` | ref-based, versionless |
| `zip-url` | `url` | ref-based, versionless |
| `repository` | `repo`, `skill`, `version: Option<Version>` | **the only variant ADR-0004 governs** |

- `Origin` **replaces** `core::manifest::SkillSource`, the CLI `SkillSource`, `SourceType`, and the nine flat `SkillDefinition.source_*` fields (which become a single `origin: Origin`).
- `Origin::repository` is a *reference into* a configured **Repository** — the two are kept distinct (a typed-in GitHub URL is an `Origin::git`, **not** a `git-marketplace` Repository).
- The persisted manifest/lock serde representation changes outright (`"source"` → `"repository"`, flat fields → nested `origin`). This is a **greenfield break with no back-compat shim** — consistent with the project's break-for-better-design stance.

### 3. Lifecycle consistency clarification (2026-09-08)

The shared-core decision applies to ordinary skills, bundles, recursive installation,
project/global operations, and every CLI, HTTP, or MCP caller. Separate entry points
may select different policies; they MUST NOT implement separate ownership, origin,
validation, or persistence rules. A capability that is unsupported in a scope MUST
be rejected before mutation, rather than silently using another scope.

Each operation MUST resolve one installation context: the project root or global
selection, its Manifest where applicable, its Lock, and its skills directory.
Discovery scope (local/remote search) is a separate concept. Relative references
MUST resolve against the owning Manifest or Lock, including when called from a
nested working directory or an HTTP server.

The core MUST produce a validated operation plan before applying changes. The plan
records selected revisions, dependency and ownership changes, conflicts, and
affected files. Preview and apply MUST use the same rules; apply MUST revalidate
state that could have changed since planning. A refetchable origin alone is not
evidence that its installed contents are outdated.

All writers MUST preserve supported Manifest sections, including bundle authoring,
bundle dependencies, and personal overrides. A writer MUST NOT serialize a partial
model over a complete document. Fetching and validation MUST finish before a
working installation is replaced, and persistence failures MUST NOT be reported as
successful installation. The recovery unit and acceptance scenarios are specified
in the local [state and ownership PRD](../../specs/lifecycle-state-and-ownership-prd.md).

The September 2026 lifecycle implementation applies this contract to the CLI and
HTTP paths identified by the audit. Lock-first defaults and floating-version freshness are
defined in
[ADR-0009](0009-resolution-and-restoration-policy.md).

## Intended consequences

- The browser install/update flow works in-process with no subprocess and no fake `200`s; reindex is reachable from core (runs or skips-silently per provider state).
- `install`/`update`/`reindex` have exactly one implementation each, shared by CLI and HTTP — the four-enum drift and the two-`SkillSource` name collision are deleted, not layered over.
- **Original migration decision:** the Origin redesign permitted a format break
  rather than retaining competing provenance models. This does not require later
  readers to discard recoverable pins. Future lifecycle migrations MUST preserve
  verified selections, as specified in the local resolution PRD.
- Skill identity follows the canonical Skill ID rules in [CONTEXT.md](../../CONTEXT.md).
  A requested dependency ID MUST match the resolved skill ID before committing.
  A conflicting existing installation MUST expose its Origin and owners so a
  provenance change is deliberate and subject to ownership checks.
- `FastSkillService` now carries an optional embedding provider; the `serve` binary must construct and pass it (or `None`).
