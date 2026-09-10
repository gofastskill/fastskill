# PRD: Predictable commands, discovery, and reconciliation

Status: accepted and implemented.
Date: 2026-09-08. Implemented: 2026-09-09. Depends on the shared operations in the
[state PRD](lifecycle-state-and-ownership-prd.md) and
[resolution PRD](reproducible-install-and-resolution-prd.md). The command-family part of C-01 is
superseded by [ADR-0010](../docs/adr/0010-command-taxonomy.md).

## Problem and outcome

The pre-namespace command audit at `2b649bf` found that advertised update controls were ignored,
global updates only changed timestamps, update failures returned exit zero, and list did not
detect version drift. Search could return results that add could not select;
some JSON modes return prose or mixed formats. These inconsistencies prevent users
and agents from knowing whether a requested operation actually happened.

The outcome is a small, stable command vocabulary with meaningful options,
installable discovery results, and accurate state and failure reporting.

## User stories

1. As a user, I install the exact repository result I selected, without guessing which default
   repository `skill add` will use.
2. As a maintainer, I preview current-to-target revisions and trust that applying
   the same request checks the same constraints and owners.
3. As a CI author, I detect missing/modified state and partial failures from stable
   JSON and exit codes rather than interpreting human progress text.
4. As a recipient, I can explain why a skill is present and which owners require it.

## Requirements

### Command meanings and selectors

- **C-01 (command paths superseded by ADR-0010):** Preserve these behavioral families under the
  explicit namespaces. `skill add` records individual-skill intent and installs its closure;
  `project install` restores selections; `skill update` deliberately resolves changes;
  `skill remove` removes individual ownership; `skill list` and `skill read` reconcile and explain
  state. `bundle add/list/update/remove` own the corresponding bundle lifecycle. A repeated
  identical add MAY return unchanged, but MUST NOT upgrade a bundle or silently adopt a
  conflicting origin. Identity and ownership use the shared core.
- **C-02:** `skill add` and `skill update` MUST accept `--repository NAME` for
  repository-backed targets. `skill search` results MUST contain canonical ID, repository identity, available version,
  and a directly usable installation reference/command. Display names MUST NOT
  replace IDs. Default repository choice MUST be visible in results and persisted
  as the resolved repository identity in intent.
- **C-03:** The existing ambiguous `skill update --source` MUST become a documented
  deprecated alias of `--repository`; different values for both MUST fail. Changing
  repository through `skill update` requires one explicitly named repository-backed skill
  and MUST appear as a provenance change in the plan. It MUST NOT silently migrate
  every installed skill or interpret an unknown repository as a URL. Other origin
  changes continue through explicit `skill add` replacement with ownership checks.
- **C-04:** `skill update ID --to-version VERSION` MUST require one named repository-backed
  target and a valid exact version. It deliberately changes that root's Manifest
  pin, then resolves and verifies its closure. It may downgrade only through this
  explicit target selection, and the preview MUST label the direction. Combining
  an explicitly provided strategy with `--to-version` MUST fail. Local/Git/ZIP
  origins MUST reject inapplicable version or repository controls before mutation.
- **C-05:** Without `--to-version`, `skill update` MUST preserve recorded constraints.
  `patch` selects the newest allowed stable version within the current major/minor;
  `minor` stays within the current major; `latest` permits any newer version already
  allowed by the constraint. Existing `major` is a documented compatibility alias
  of `latest`. Exact pins remain pinned; strategies MUST NOT widen them. Explicit
  prerelease constraints retain their admitted candidates under R-04. A missing
  compatible candidate is an error, not proof that the installation is current.
- **C-06:** Global `skill update` MUST fetch, validate and apply actual content changes
  through the same operation policies, then update operational timestamps only for
  completed actions. A named target absent from desired/recorded global selection
  MUST fail. Updating dependencies MUST not promote them to independent roots.

### Plans, outcomes, and indexing

- **C-07:** `skill add`, `project install`, `skill update`, `skill remove`, and bundle mutation
  operations MUST offer consistent `--dry-run` and `--json` results. `skill update --check` renders a concise version of
  the same validated plan. Outcomes MUST distinguish changed, unchanged, blocked,
  failed and partial failure, and show current/target revisions, owner/dependent
  effects and retained files. No-change output MUST be explicit, never blank.
- **C-08:** Preview MAY fetch inputs into disposable staging and make documented
  read requests, but MUST NOT modify installed files, declarations, Lock bytes,
  persistent indexes, operational timestamps, or run evaluations. Apply MUST
  revalidate the plan against current local state and fetched inputs; a later
  invocation cannot promise an unchanged remote candidate. Policy-only bundle
  changes MUST be included even when member bytes are identical.
- **C-09:** Every failed requested target and every partial application MUST return
  nonzero. Validation errors MUST occur before mutation. A check/dry-run with valid
  available changes returns success; blocked/failed planning returns nonzero.
  Summaries MUST report actual committed outcomes, not just attempted operations.
  Progress and diagnostics MUST not corrupt structured stdout.
- **C-10:** Consistent indexing policy MUST apply to individual and bundle paths.
  With no provider, indexing skips under ADR-0002. With a provider, explicit flags
  override configuration. A derived-index failure after package commit MUST be
  visible as a warning/result field and MUST NOT claim the package rolled back.
  Explicit offline mode follows R-06. Previews MUST NOT run `index rebuild`.

### Reconciliation and discovery

- **C-11:** `skill list` MUST compare desired constraints, locked facts, and actual identity,
  version and content. It MUST report roots, every owner, dependent reasons, groups,
  and explicit mutable/override status. Bundle/transitive members required by roots
  MUST NOT be mislabeled extraneous or missing from the Manifest merely because
  they are not direct dependency keys. Presence alone MUST NOT imply agreement.
- **C-12:** Add `skill list --check`; return nonzero for missing required contents,
  unsupported/insufficient integrity evidence, mismatched revisions/content, or
  unresolved ownership conflicts. Report extraneous unrelated additions separately;
  they are permitted and MUST NOT fail a check by themselves. An intentional
  editable selection may pass structural/constraint checks while explicitly marked
  mutable. Optional `--only`/`--without` selectors MUST match installation semantics;
  without selectors the check covers all declared roots. Excluded selections MUST
  remain distinguishable from missing selected dependencies.
- **C-13:** `skill read` MUST use the selected installation scope and resolved Lock root.
  Installed reads accept an exact canonical ID; unsupported `ID@version` selection
  MUST fail with guidance to repository version discovery, rather than advertise
  historical content retrieval. `--locked` changes metadata provenance, not which
  historical bytes are read. A filesystem tree MUST be labeled as the actual tree;
  it MUST NOT masquerade as a locked snapshot. `--meta --tree --json` MUST return
  one JSON value containing both supported views or fail argument validation.
- **C-14:** `repo skills/show/versions` and `skill search` MUST use each adapter's supported
  capabilities. Listing ordinary local/Git/ZIP catalogs MUST work; options that
  require HTTP-specific index data MUST be rejected specifically for other types.
  `repo test` MUST distinguish connectivity/catalog validity from acquisition
  capability, rather than imply unsupported downloads will work.
- **C-15:** `skill search` MUST distinguish no repositories, zero matches, partial results,
  and total repository failure. Partial results MUST identify failed repositories
  and return nonzero for incomplete requested coverage; total failure MUST not say
  merely no matches. Query is required. Empty JSON results MUST remain valid JSON.
- **C-16:** Command schemas MUST validate required arguments, enumerations, integer
  bounds and mutually exclusive flags before handler dispatch. Every JSON mode MUST
  emit one documented object/array, including empty results and domain errors.
  Existing read fields SHOULD be retained where accurate; any incompatible output
  change MUST be documented. Machine-readable mutation results MUST include scope,
  overall outcome, per-target outcomes, changes, retained items and diagnostics.

## Roles retained across the remaining commands

| Command family | Product responsibility |
| --- | --- |
| project init | Create project/skill metadata; explicit overwrite behavior for existing files. |
| repo add/remove/update/list/info/test/refresh | Manage and inspect repository configuration and metadata. Removing configuration does not uninstall skills; references to removed configuration remain visible as unresolved provenance. |
| cache info/clean | Inspect/remove reusable acquisition cache; preserve managed ownership and required retained artifacts. |
| index rebuild | Maintain derived embeddings under the optional-provider contract. |
| cli doctor | Report selected-scope and effective provider/tool readiness; do not warn about an unrelated provider's missing token. Dependency drift belongs to `skill list`. |
| analysis matrix/cluster/duplicates | Inspect the selected index through distinct analysis views. |
| eval validate/run/judge/score/scorecard/report | Validate, execute, assess, aggregate and render evaluations; no implicit invocation during installation. |
| optimization run/resume/status/inspect/export | Run and inspect optimization; export remains an explicit artifact write. |
| marketplace create | Generate the distinct plugin catalog format. |
| server serve; mcp serve/install/list | Keep HTTP/browser and MCP/client-configuration roles distinct. |
| cli completion/spec | Generate discoverability and machine schemas from accurate command definitions. |

These families need the applicable validation, output, scope and effect contracts;
this PRD does not introduce new evaluation algorithms. ADR-0010 owns the command-tree names.

## Acceptance scenarios

| ID | Scenario | Required observable result |
| --- | --- | --- |
| C-A01 | Search a nondefault repository and run the returned `skill add` reference | Same canonical skill and repository selected; provenance retained. |
| C-A02 | Global local-origin v1 changes to v2; run `skill update` | Disk changes to verified v2; timestamp-only success is impossible. |
| C-A03 | Patch/minor/latest/major strategies with exact and ranged intent | Targets follow C-05; exact pins never widen. |
| C-A04 | Explicit version/repository change and invalid combinations | Correct intent change shown/applied, or pre-mutation validation error. |
| C-A05 | Missing named target, failed fetch, later-unit failure | Nonzero; accurate unchanged/failed/partial results and preserved prior state. |
| C-A06 | Bundle policy tightens while bytes remain unchanged | Dry-run and apply both identify the override conflict; no blank preview. |
| C-A07 | Change installed v2 to v9 or edit bytes without changing version | `skill list` identifies drift; check returns nonzero with desired/locked/actual evidence. |
| C-A08 | Identical shared bundle/transitive member, personal addition, editable skill | Owners correct; additions allowed; editable status disclosed without a false digest claim. |
| C-A09 | Group-limited `project install` and check with matching selectors | Selected closure passes; unselected groups explicitly excluded. |
| C-A10 | Locked `skill read` from nested/global scope | Same selected state as its root invocation; no ambient project leakage. |
| C-A11 | `skill search` with no query, empty matches, failed repository, and partial results | No panic; distinct statuses, nonzero incomplete/error results, valid JSON. |
| C-A12 | All supported JSON selector combinations, including meta plus tree | One parseable documented JSON result or a validation error, no appended prose. |
| C-A13 | Preview with input fetches; inspect state before/after | No managed state, timestamps or indexes change. |
| C-A14 | Catalog listing across all four Repository types | Generic listing works; only unsupported options are rejected. |
| C-A15 | Run `repo add` for a Git marketplace with `--tag`, restart, and inspect configuration | Tag persists in the Manifest/runtime; combining branch and tag fails before mutation. |
| C-A16 | Re-run global `skill add` with only a different group | Lock groups change and the result reports a change without replacing identical content. |

Use CLI subprocesses and controlled local catalogs. Record command, exit status,
stdout/stderr, JSON parsing, actual files and subsequent inspection. Retain the
generated help/`cli spec` parity tests, but do not treat their success as lifecycle proof.

## Implementation and documentation

Primary paths: [update](../crates/fastskill-cli/src/commands/update.rs),
[list](../crates/fastskill-cli/src/commands/list.rs),
[read](../crates/fastskill-cli/src/commands/read.rs),
[search](../crates/fastskill-cli/src/commands/search.rs),
[repos](../crates/fastskill-cli/src/commands/repos), and
[remote search](../crates/fastskill-core/src/search/remote.rs).

Implementation MUST update command help, README quick starts, relevant webdocs,
contributor guidance and shipped FastSkill skills alongside behavior. References
to an external skills project MUST be tracked as a separate delivery dependency,
not assumed updated by editing this repository. Examples MUST be copy-pasteable
and verified against the implemented binary before being advertised as available.

No new sync, enable/disable, status/why command family, hosted catalog service,
publishing login, automatic bundle release discovery, or fleet dashboard.
