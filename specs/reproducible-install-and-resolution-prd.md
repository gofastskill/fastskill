# PRD: Reproducible installation and complete resolution

Status: draft product requirements; implementation pending.
Date: 2026-09-08. Depends on the state/graph boundary in the
[state and ownership PRD](lifecycle-state-and-ownership-prd.md).

Strict locked restoration and complete installation repair existing promises.
Default lock preference, `@latest`, automatic metadata refresh, and `--offline`
are proposed changes recorded in [ADR-0009](../docs/adr/0009-resolution-and-restoration-policy.md).

## Problem and outcome

At audited main `2b649bf`, `install --lock` installs changed local-origin contents
and rewrites the Lock. A cold install can miss transitive dependencies. Local ZIPs,
HTTP registries, and skill manifests accepted on one path fail on another. Users
cannot reliably restore what they previously added.

The outcome is one complete first installation and a later restoration of the
same resolved contents, regardless of supported origin or interface.

## User stories

1. As a new user, I add a repository skill without specifying a version and receive
   the newest stable selection and its complete skill dependencies.
2. As a teammate, I restore committed requirements without moving locked versions.
3. As an offline user, I can install available verified artifacts without catalog
   access and receive precise errors for missing inputs.
4. As a maintainer, I install selected groups while preserving the requirements
   and locked selections of other groups.

## Requirements

### Origin support and references

- **R-01:** Add, install and update MUST share origin inference, fetching, metadata
  validation and content verification. Support local directory, editable directory,
  local single-skill ZIP, ZIP URL, Git ref/subdirectory, and each advertised
  Repository type. A supported source MUST survive add → Lock → remove installed
  files → install → update. Bundle artifacts retain their separate validated format.
- **R-02:** Repository adapters MUST support their advertised discovery and
  acquisition capabilities. A successful catalog lookup MUST yield an installable
  canonical ID and origin. Logical repository identity MUST remain in Manifest and
  Lock provenance; resolving a download URL MUST NOT replace that intent.
- **R-03:** A folder reference MUST designate exactly that folder. Version comes
  from validated metadata, not neighboring names. Editable is directory-only.
  Metadata requirements and fallback rules MUST agree on every install path;
  legacy author metadata and SKILL.md fallback MUST not disagree between add and restore.

### Version selection and freshness

- **R-04:** `ID@1.2.0` MUST remain exact. Explicit ranges retain ADR-0004 semantics.
  `ID` and proposed `ID@latest` MUST normalize to the same floating repository intent
  and choose newest stable. A prerelease requires explicit intent admitting it;
  absence of a stable candidate MUST fail. `==` MUST NOT become the documented
  convention. These version selectors do not apply to arbitrary folders or Git refs.
- **R-05:** Online resolution of floating/ranged selections MUST refresh relevant
  catalog metadata once per repository per operation. Exact pins and locked
  restoration MUST NOT depend on listing versions. The result MUST report whether
  selection used refreshed or cached metadata. Refresh failure MUST NOT silently
  substitute stale data while claiming a current online selection.
- **R-06:** Add/install/update MUST offer an explicit `--offline` mode. It MUST
  prohibit network acquisition, metadata refresh and automatic embedding calls.
  It may resolve floating intent from cached metadata, clearly identifying that
  freshness limitation. Missing inputs MUST name the repository, revision or path
  needed. `--offline --reindex` MUST fail argument validation before mutation.

### Dependency resolution

- **R-07:** Resolution MUST fetch/inspect required manifests while traversing
  missing skills, rather than infer that an uninstalled skill has no dependencies.
  It MUST retain all edges, merge compatible requirements and reject incompatible
  versions/origins/content under one ID. Required dependencies MUST be present on
  the first successful complete install. Traversal order MUST NOT choose a winner.
- **R-08:** IDs, version constraints, cycles, missing dependencies and depth limits
  MUST be validated before apply. Roots are depth zero; public `--depth 1` permits
  roots only, and `--depth N` permits N levels including roots. Values below one or
  beyond the supported integer range MUST be rejected before conversion. A required
  dependency beyond the limit MUST produce an incomplete-closure error with its
  chain. Depth/skip-transitive settings MUST NOT report a complete installation
  while leaving required dependencies absent; already available required contents
  still require verification.

### Lock and restoration contract

- **R-09:** The Lock MUST retain origin intent separately from resolved version,
  Git commit where applicable, canonical content digest, dependency edges and root
  coverage. A single parent pointer is insufficient for shared requirements.
  Project Lock content MUST remain deterministic and timestamp-free.
- **R-10:** Strict `install --lock` MUST require compatible locked coverage for all
  selected roots. For immutable selections it MUST fetch the recorded revision and
  verify identity and content, with the explicit editable exception in R-13,
  and leave Lock bytes unchanged on both success and failure. It MUST NOT replace
  a locked SHA with a branch head or rewrite a changed local digest as a restoration.
- **R-11:** Ordinary install MUST prefer compatible locked selections. With no
  Lock, it resolves selected roots and records them. It MAY extend coverage for
  previously unresolved selected roots while preserving compatible existing pins.
  If an already-recorded origin/constraint/required graph no longer matches desired
  intent, it MUST stop and direct the user to update. It MUST NOT silently prune
  unrelated installed skills; explicit removal owns pruning.
- **R-12:** A Lock MUST represent both desired-intent compatibility and which
  roots have complete resolved coverage. A group-limited first install MUST NOT
  claim unresolved excluded roots are locked. Later ordinary installation may
  complete that coverage; strict installation MUST fail if selected coverage is
  missing. Unselected existing entries MUST be preserved.
- **R-13:** Restoration of a mutable snapshot origin MUST use saved verified bytes
  or verify the origin against the recorded digest and fail on mismatch. Editable
  links remain explicitly mutable: restore validates link target, identity and
  constraints, reports mutable content, and MUST NOT claim byte-for-byte integrity.
  A moved or unavailable source is an actionable failure, not a guessed replacement.
- **R-14:** Older locks with recoverable exact facts MUST migrate without changing
  selections. Missing integrity evidence MUST remain distinguishable from verified
  evidence. Strict mode MUST fail on insufficient evidence and leave files alone;
  explicit update can establish a new verified selection. It MUST NOT present a
  newly observed mutable source as proof of the old locked bytes.
  Ordinary restoration MUST also stop rather than silently treating missing
  integrity evidence as a newly approved snapshot.

### Groups and bundles

- **R-15:** Ungrouped individual roots belong to the implicit `default` group.
  Plain install selects all declared groups. `--only GROUP` selects roots in those
  groups only; `--without GROUP` excludes matching roots. Unknown groups and combining
  `--only` with `--without` MUST fail before mutation. Multiple values within one
  selector form a union. Selection occurs at roots, then includes their required
  closure even when a child has different group labels.
- **R-16:** Group semantics MUST match with and without a Lock. Selection controls
  materialization, not deletion of previous files or declarations. Bundles have no
  group selector in this release and remain selected; their required members and
  active overrides MUST NOT be filtered out by individual-skill group flags.
- **R-17:** Locally available bundles MUST install their verified embedded closure
  without fetching original member origins. Normal restoration MUST combine bundles
  and individual requirements under shared ownership rules. Bundle updates remain
  explicit selected artifacts, never implicit `@latest` bundle discovery.

## Proposed command behavior

```sh
fastskill add reviewer@1.2.0 --repository team   # exact intent
fastskill add reviewer@latest --repository team # floating stable intent, exact Lock
fastskill install                              # prefer compatible pins
fastskill install --lock --without dev         # strict selected restoration
fastskill install --lock --offline             # verified local inputs only
fastskill update reviewer                      # deliberate resolution within intent
```

These examples describe the target contract, including flags absent in the audited
binary. Repository selection and update controls are specified in the
[command PRD](command-consistency-and-discovery-prd.md).

## Acceptance scenarios

| ID | Scenario | Required observable result |
| --- | --- | --- |
| R-A01 | Cold root → child → grandchild install | All required bytes and all edges exist after the first success. |
| R-A02 | Diamond graph, compatible and conflicting requirements | Compatible shared node installed once; conflict independent of traversal order. |
| R-A03 | Each supported origin through add/restore/update | Same validation, canonical ID, provenance and content guarantees throughout. |
| R-A04 | Lock local v1, change origin to v2, restore strictly | Original verified snapshot restored or explicit mismatch; Lock unchanged. |
| R-A05 | Lock a Git branch, advance it, restore with cache cleared | Recorded commit restored; branch movement does not alter pins. |
| R-A06 | Corrupt/reuse a ZIP or repository artifact at a pinned location | Digest rejection before replacement; old files and Lock survive. |
| R-A07 | Catalog stable 1.2.0 plus 2.0.0-beta.1 | Omitted/latest/wildcard choose stable; explicit prerelease can select beta. |
| R-A08 | Exact cached pin with repository unavailable | Installs offline without querying version listings. |
| R-A09 | Floating online/offline with empty, old, or failed metadata | Online refreshes or fails; offline uses available cache with disclosed freshness or fails precisely. |
| R-A10 | default/dev roots share a child; install with only/without and Lock | Selected closure matches in both modes; no required child is dropped. |
| R-A11 | First install one group, then strict/ordinary install another | Strict fails incomplete coverage; ordinary extends coverage without moving old pins. |
| R-A12 | Change already-locked Manifest intent or load an old incomplete Lock | Strict preserves state and diagnoses mismatch/evidence; update is explicit. |
| R-A13 | Wrong fetched ID, cycle, negative/overflow depth, missing child | Validation error; no Installed message or destination mutation. |
| R-A14 | Local bundle plus individual root sharing a member, no member network access | Embedded closure restores and shared ownership remains valid. |
| R-A15 | Same compatible installation twice | Same pins and bytes; unchanged result; deterministic Lock. |

Test the public CLI with isolated local directories, valid ZIPs, local Git fixtures,
and controlled catalog/download servers. Compare bytes and Lock content, not only
reported versions. Existing extraction safety fixtures remain required. No real
provider credentials or paid evaluations are needed.

## Implementation entry points and exclusions

Start with [legacy install dispatch](../crates/fastskill-cli/src/utils/install_utils.rs),
[install orchestration](../crates/fastskill-cli/src/commands/install.rs),
[dependency resolver](../crates/fastskill-core/src/core/dependency_resolver.rs),
[core acquisition](../crates/fastskill-core/src/core/install.rs),
[version selection](../crates/fastskill-core/src/core/install/support.rs),
[repository clients](../crates/fastskill-core/src/core/repository/client.rs), and
[Lock model](../crates/fastskill-core/src/core/lock.rs).

Before format edits, the implementation design MUST define canonical digest rules,
intent compatibility/coverage representation and migration fixtures. Reuse bundle
digest rules where their content definition applies; do not invent competing hashes.
Cross-platform metadata and link behavior MUST be explicit.

No new publishing service, bundle release feed, private credential flow, side-by-side
versions under one ID, sibling-folder version discovery, or automatic script execution.
