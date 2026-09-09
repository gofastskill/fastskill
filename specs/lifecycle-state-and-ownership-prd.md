# PRD: Consistent lifecycle state and ownership

Status: accepted and implemented.
Date: 2026-09-08. Implemented: 2026-09-09.

This PRD defines the implemented shared-core and ownership contracts in
[ADR-0005](../docs/adr/0005-install-seam-and-origin-model.md) and
[ADR-0008](../docs/adr/0008-bundle-ownership-and-local-changes.md). The new override
reset and scope restrictions below are proposed command behavior. MUST, MUST NOT,
SHOULD, and MAY express requirement strength.

## Problem and outcome

Routine operations can lose desired state or break another installed skill. The
2026-09-08 audit of `origin/main` at `2b649bf` reproduced these cases:

- Install a bundle, then add an unrelated skill: `[bundles]` disappears from the
  Manifest while the Lock still records it.
- Force-add different contents under a bundle member's ID: replacement succeeds
  without an approved override. HTTP deletion also removes required members.
- Remove a dependency or its owning bundle: an ordinary skill that still requires
  it remains installed but broken.
- Reject an invalid replacement after deleting the previous installed directory.

A maintainer must be able to combine bundles and personal skills, remove one
requirement, and recover from failure without losing another requirement or edits.

## User stories

1. As a recipient, I can combine two bundles and individual skills with identical
   shared contents, then remove owners independently.
2. As an author, I can edit ordinary dependencies without losing bundle settings.
3. As a user, I can remove a missing dependency declaration or reset a permitted
   override without editing internal state by hand.
4. As an operator, a failed change preserves the previous usable state of its
   affected dependency group and reports any earlier completed work accurately.

## Requirements

### State and scope

- **S-01:** Every writer MUST preserve all supported Manifest sections, including
  metadata, dependencies, tool/repository configuration, `[bundle]`, `[bundles]`,
  and `[overrides]`. Unrelated values MUST survive semantically unchanged. Unknown
  extension tables MUST be preserved or rejected before writing, never dropped.
- **S-02:** One resolved installation context MUST govern root, Manifest, Lock,
  repositories, and skills directory. Project commands MUST work identically from
  the project root and a nested directory. Project `--skills-dir` changes the
  destination, not the owning Manifest or relative-origin base.
- **S-03:** Global ordinary-skill add/install/update/remove/list/read MUST use
  global selection and operational Lock state without consulting an ambient
  project Manifest. Global installation restores the global recorded selections;
  this PRD does not introduce a global Manifest. Bundle operations and
  `--global --skills-dir` MUST be rejected before mutation until those combinations
  have a defined storage/ownership model. Project bundle destinations remain supported.

### Requirements and ownership

- **S-04:** The shared graph MUST retain all roots and dependency edges, including
  multiple parents of a shared skill. Direct selection, transitive requirement,
  bundle membership, and personal replacement MUST remain distinguishable. Fetching
  a transitive skill MUST NOT promote it to a direct Manifest dependency.
- **S-05:** Every add/install/update/remove path MUST check all retained owners.
  Selections under one ID MUST agree on verified contents and constraints unless
  an explicit override is permitted. Bundle updates MUST check direct and
  transitive owners; ordinary writes MUST check bundle owners. Last writer wins is
  prohibited. `--force` MUST NOT bypass these checks or discard untracked edits.
- **S-06:** Removing a direct root MUST remove its declaration and retain files
  reachable from any other root. Removing a bundle follows the same rule. Only
  unreachable managed dependencies may be pruned, subject to local-edit protection.
  Removing an ID that is only a required dependency MUST fail and name its roots.
  A declared or locked root whose files are missing MUST still be removable.
- **S-07:** An absent ID with no declaration, Lock entry, or files MUST produce an
  explicit unchanged result. Extraneous unmanaged files MUST NOT be silently
  adopted or pruned. Removing an explicitly named extraneous skill MAY use the
  existing force confirmation, but MUST NOT follow links outside the target.

### Overrides and local changes

- **S-08:** Add `bundle override ID --reset` as the inverse of setting an override.
  `--reset` and `--from` MUST be mutually exclusive. Reset MUST restore the packaged
  selection agreed by every retained owner and clear the override records in the
  same operation. An already-reset ID is unchanged. A conflict between remaining
  owners MUST block reset and explain the conflicting selections.
- **S-09:** Setting, updating, removing, or resetting a selection MUST protect
  untracked installed-file edits. An editable origin is explicitly mutable; it
  MUST NOT be presented as a verified immutable snapshot. Removing an editable
  installation MUST remove only its installed link and records, never origin files.
- **S-10:** When removing the last bundle owner of a personal replacement, the plan
  MUST retain that explicit personal selection as an individual requirement with
  its origin and dependencies, and clear its obsolete override relationship.

### Planning, application, and recovery

- **S-11:** The core MUST validate a complete operation plan before mutation:
  selected identities/content, dependency closure, scopes, ownership, local edits,
  Manifest/Lock validity, and destination safety. Expected skill IDs MUST match
  fetched IDs. Preview MUST run the same validation without changing managed state.
- **S-12:** A replacement MUST be staged and validated before deleting working
  files. The recovery unit MUST include a changed root and its affected dependency
  closure; overlapping units MUST be combined. Files, ownership, Manifest and Lock
  changes for that unit MUST commit together or restore their prior state on an
  application error. Failure in a later independent unit may leave earlier units
  committed, but the command MUST return a partial-failure result and nonzero status.
- **S-13:** FastSkill writers affecting the same state or destination MUST be
  coordinated. Apply MUST reject or re-plan stale inputs. Recovery failure MUST
  identify affected paths and block further mutations of that state until recovery;
  it MUST NOT masquerade as success. An interrupted operation MUST be detectable
  before a later mutation. No atomic view to independently running agents or
  guarantee against loss of storage is implied.
- **S-14:** Reindexing is derived work after a successful state change. All paths,
  including bundles and override reset, MUST honor ADR-0002 and the indexing flags.
  Index failure MUST be reported separately from committed package changes.
  Cache cleanup MUST NOT delete ownership, retained bundle releases, or recovery data.

## Acceptance scenarios

| ID | Scenario | Required observable result |
| --- | --- | --- |
| S-A01 | Add/update/remove an unrelated skill beside all supported Manifest tables | Unrelated values, bundles and overrides survive each write and subsequent restore. |
| S-A02 | Two bundles and a direct root share identical contents; remove each owner | Files remain until no owner requires them; list and Lock agree after each step. |
| S-A03 | A root depends on a shared child through two parents | Both edges remain; removing one parent does not delete the child. |
| S-A04 | Force-add or HTTP-delete a required bundle member | Conflict; Manifest, Lock and installed bytes remain unchanged. |
| S-A05 | Update a bundle to content conflicting with a direct or transitive owner | Conflict identifies both owners before any mutation. |
| S-A06 | Remove a direct root with missing installed files | Declaration and no-longer-required Lock records are removed successfully. |
| S-A07 | Set/reset an allowed override, then remove its final bundle | Reset restores packaged bytes; separate final-bundle removal retains personal intent. |
| S-A08 | Reset with conflicting owners or untracked installed edits | Actionable conflict; previous bytes and records survive. |
| S-A09 | Replacement contains a rejected symlink or invalid metadata | Previous installed marker, Manifest and Lock remain unchanged. |
| S-A10 | Inject failure at file, Manifest, ownership or Lock persistence | Affected unit is restored; earlier independent successes are explicitly reported. |
| S-A11 | Run supported operations from nested/project/global contexts | Only the selected state and destination change; unsupported combinations fail first. |
| S-A12 | Remove an editable installation | Origin files and sentinel locations outside the destination remain intact. |
| S-A13 | Competing writer or interrupted application precedes apply | No stale plan commits; pending recovery is detected and handled or blocks mutation. |
| S-A14 | Bundle change with indexing on/off/absent provider | Same provider/flag semantics as an ordinary skill change; state success is truthful. |

Use subprocess CLI scenarios and public core operations with controlled persistence
faults. Assertions MUST compare installed bytes, desired declarations, Lock facts,
owners, exit status and a subsequent read/list/restore. A command failing during
fixture setup is not evidence for a conflict test.

## Implementation boundaries and dependencies

Primary entry points: [Manifest persistence](../crates/fastskill-core/src/core/manifest.rs),
[core install](../crates/fastskill-core/src/core/install.rs),
[bundle lifecycle](../crates/fastskill-core/src/core/bundle.rs),
[ordinary removal](../crates/fastskill-cli/src/commands/remove.rs), and
[manifest helpers](../crates/fastskill-cli/src/utils/manifest_utils.rs).

Implement the state/graph/planning boundary first. The
[resolution PRD](reproducible-install-and-resolution-prd.md) populates that graph;
the [command PRD](command-consistency-and-discovery-prd.md) exposes outcomes; the
[API/MCP PRD](api-mcp-lifecycle-parity-prd.md) removes alternate policy paths.
An implementation design MUST specify graph persistence, schema versions, writer
coordination and recovery markers before code changes to those formats. Existing
valid pins MUST NOT be discarded merely to migrate the schema.

## Out of scope

No fleet management, environment provisioning, runtime enforcement, global bundle
model, standalone rollback command, new identity service, or simultaneous differing
versions under one skill ID. Existing external publishing and bundle artifacts remain.
