# ADR-0015: Compose Manifests and export projects as bundles

Status: Accepted

## Context

Bundle authors had to repeat project dependencies under `[bundle.members]`, repeat identity under
`[bundle]`, and decide a per-member `overridable` policy. A references-only bundle would contain no
meaningful information beyond a reusable `skill-project.toml`.

## Decision

`fastskill bundle build` derives bundle identity from `[metadata].id` and `[metadata].version` and
packages every `[dependencies]` root plus its installed dependency closure. The ZIP remains
self-contained and embeds a compatibility descriptor for bundle lifecycle validation. Existing
source manifests with `[bundle]` continue to build during migration.

Projects reuse reference-only collections through Manifest composition:

```toml
[tool.fastskill.manifests]
platform = "../platform-team/skill-project.toml"
```

Composition is recursive. Each relative path is evaluated from the Manifest that contains it.
Repeated identical dependency declarations are deduplicated; different origins or groups for the
same skill ID are rejected. Missing references and cycles are rejected before installation.

The consuming project's `skills.lock` pins the resolved skill contents for the combined roots.
The shared Manifest may declare Git, repository, ZIP URL, or shared-filesystem origins normally.

## Consequences

Bundle authors maintain one dependency list and one identity. A bundle is always the offline-ready
artifact; a reusable online setup remains a Manifest. The old member policy remains readable for
installed legacy archives, but new builds make every member required.

Manifest composition is supported in author-controlled project manifests. Installed or fetched
skill packages must declare dependencies directly; their manifest references are rejected before
any referenced file is read. This prevents packages from importing host project files.
