# Resolve floating intent deliberately; restore pinned contents

Status: proposed; implementation pending. Date: 2026-09-08.

Related: [ADR-0004](0004-bare-version-is-exact.md),
[ADR-0005](0005-install-seam-and-origin-model.md), and
[ADR-0007](0007-self-contained-tracked-skill-bundles.md).

Users usually want a current skill when adding it, and the same environment when
restoring a project. We propose separating those moments: `add` and `update` may
resolve new selections; `install` prefers compatible locked selections. This
changes current defaults and remains a proposal rather than a claim about the CLI.

## Proposed decision

- A repository reference `skill@1.2.0` MUST remain an exact pin under ADR-0004.
  Explicit operators retain their range meaning. Neither install nor an update
  strategy may silently widen a recorded constraint.
- An omitted repository version and `@latest` MUST mean newest stable. Both MUST
  normalize to the same floating intent; the Lock records the exact selection.
  Prereleases require an explicit prerelease version or range that admits them.
  No stable candidate is an error, not permission to choose a beta.
- Online resolution of floating or ranged intent MUST refresh relevant repository
  metadata once per repository per operation. Exact pins and locked restoration
  MUST NOT require a version listing. An explicit offline mode MUST use only
  available verified inputs and identify missing metadata or artifacts.
- `install` MUST reuse a Lock compatible with the selected Manifest requirements.
  With no Lock, it resolves the selected closure. It may fill previously unresolved
  selected roots while preserving existing compatible pins. Changed recorded intent
  requires an explicit update, rather than silent re-resolution during restoration.
- `install --lock` MUST require complete compatible coverage of the selected roots,
  restore their recorded revisions, and leave the Lock unchanged. Missing revisions,
  insufficient integrity evidence, and changed immutable contents MUST fail clearly.
- A local folder means exactly that folder. Snapshot installation verifies its
  locked contents; editable installation is an explicitly mutable link and MUST be
  reported as such. Neither mode searches sibling folders for a newer version.
- Bundles retain explicitly selected artifact updates. This decision does not add
  bundle catalogs, release channels, or automatic bundle version discovery.

## Tradeoffs

We prefer reproducible restoration over resolving newest on every install. Users
must use update after changing already-locked intent. We prefer current online
floating resolution over the current manual-refresh-only policy; this adds a
metadata request and makes offline behavior explicit. Exact pins and verified
cached artifacts remain usable without catalog freshness checks.

The Lock MUST preserve exact revisions and content evidence, including dependency
edges and selected-root coverage. It MUST NOT acquire volatile freshness timestamps;
repository cache metadata owns those timestamps. Existing locks without enough
evidence MUST receive an actionable error in strict mode rather than an invented
pin or an unverified integrity claim.

The local [installation and resolution PRD](../../specs/reproducible-install-and-resolution-prd.md)
defines selection, groups, partial coverage, migration, and acceptance scenarios.
