# FastSkill lifecycle coherence: PRD index and delivery plan

Status: implemented.
Date: 2026-09-08. Implemented: 2026-09-09.

These PRDs turn the command-coherence audit into local repository specifications and record the
implemented lifecycle contract.
They belong in `specs/`, which this repository intentionally Git-ignores. They MUST
NOT be created or mirrored as GitHub issues. Existing ADR clarifications retain
accepted architectural boundaries; new resolution defaults are explicitly proposed.

## Baseline and evidence

The audit checked `origin/main` at `2b649bf` (v0.9.221). Its Rust source matched the
existing v0.9.220 development binary used for disposable CLI, localhost HTTP and
stdio MCP probes. All 45 exported command help paths succeeded. Targeted lifecycle
probes, rather than those help checks, established the defects below. Evaluation
and optimization runs were inspected for command roles/effects, not executed with
paid providers. These findings are a baseline, not a permanent claim about main.

## Documents and order

| Order | PRD | Responsibility |
| --- | --- | --- |
| 1 | [State and ownership](lifecycle-state-and-ownership-prd.md) | Complete Manifest persistence, installation scope, shared graph, ownership, safe application/removal, override reset. |
| 2 | [Installation and resolution](reproducible-install-and-resolution-prd.md) | Supported-origin round trips, first-install closure, locked integrity, groups, exact/latest semantics and offline operation. |
| 3 | [Commands and discovery](command-consistency-and-discovery-prd.md) | Real updates, selectors, previews, reconciliation, usable search, honest outcomes and JSON. |
| 4 | [HTTP and MCP](api-mcp-lifecycle-parity-prd.md) | Shared API mutations, persisted edits, effect classification and transport parity. |

Each PRD is bounded by a shared contract. Foundational state work precedes callers;
surface fixes may proceed against that agreed boundary without waiting for every
UX extension. Shipping a document or a passing help test does not complete a PRD.

## Audit-to-requirement coverage

| Audit finding | Reproduced or source-backed behavior | Owning requirements |
| --- | --- | --- |
| F01 | Unrelated add erases bundle Manifest tables | S-01; P-01 |
| F02 | Force-add, HTTP delete, and bundle update bypass retained owners | S-04–S-06; P-01 |
| F03 | Ordinary install --lock installs changed local bytes and rewrites pins | R-09–R-14 |
| F04 | Cold install omits dependencies discovered on a second run | R-07–R-08 |
| F05 | Invalid replacement fails after old installed files are deleted | S-11–S-13 |
| F06 | ZIP/registry paths disagree; searchable catalog cannot download; no selected-repo add | R-01–R-03; C-02–C-03; C-14 |
| F07 | Update flags ignored, global timestamps-only, failure exits zero | C-03–C-06; C-09 |
| F08 | Removal breaks retained dependency; missing declaration cannot be removed | S-04; S-06–S-10 |
| F09 | Global/storage flags and nested Lock roots disagree | S-02–S-03; C-13; P-02 |
| F10 | Read-only MCP `optimization export` writes its destination | P-08–P-11 |
| F11 | HTTP edits echo unpersisted values or discard origin/groups | P-03–P-06 |
| F12 | List compares presence and misses actual revision/content drift | C-11–C-12 |
| F13 | Locked group filters ignored; only includes ungrouped roots | R-12; R-15–R-16 |
| F14 | Bundle preview reports success despite apply conflict; indexing flags diverge | S-14; C-07–C-10 |
| F15 | Wrong fetched ID, missing-query panic, invalid depth, mixed JSON, misleading search IDs/errors | S-11; R-08; C-13–C-16 |
| F16 | Manifest-only removal deletes an existing destination without Lock integrity evidence | S-05–S-07; S-11 |
| F17 | Global add/update overwrites retained shared selections or locally edited managed content; group-only add is ignored | S-09–S-11; R-07; R-15–R-16; C-06–C-07 |
| F18 | Registry artifact version can disagree with the catalog selection; repository Git tags are discarded on save | R-01–R-04 |
| F19 | Final bundle override promotion can retain a dangling dependency graph; failed rollback deletes its recovery backup | S-10–S-13 |
| F20 | HTTP updates plan roots independently and report unchanged selections as updates; duplicate rejection may refresh persistent catalog state first | P-01; P-07; P-12; C-07; C-09 |

Small extensions from the review are assigned explicitly: repository selection
(C-02), override reset (S-08), previews/JSON/check (C-07/C-12), latest-stable/offline
(R-04–R-06), and lock-first defaults (R-10–R-14). Folder references remain exact
locations (R-03). No separate status/why/sync command family is planned.

## Command coverage

| Command paths | Count | Primary PRD concerns |
| --- | ---: | --- |
| skill add, remove, update, list, read, search | 6 | State, reconciliation, resolution, discovery, output |
| bundle build, add, list, update, remove, override | 6 | State/ownership; artifact integrity; MCP effects |
| project init, install | 2 | Context and project restoration |
| repo add, remove, update, list, info, test, refresh, show, skills, versions | 10 | Repository configuration/capabilities, acquisition and canonical references |
| marketplace create | 1 | Distinct catalog role; artifact effects |
| analysis matrix, cluster, duplicates | 3 | Selected index, output and actual effect classification |
| eval validate, run, judge, score, scorecard, report | 6 | Preserve pipeline; output and actual effect classification |
| optimization run, resume, status, inspect, export | 5 | Preserve workflow; explicit execution/export effects |
| index rebuild | 1 | Readiness, validation and indexing |
| cache info, clean | 2 | Cache versus managed state; effects |
| server serve | 1 | HTTP API parity, context and effect classification |
| mcp serve, install, list | 3 | MCP parity, context and effect classification |
| cli doctor, completion, spec | 3 | Readiness, authoritative schema, discoverability |
| Total | 49 | All canonical command paths under ADR-0010 |

## Decisions and scope

- [ADR-0003](../docs/adr/0003-serve-trust-boundary-and-edge-auth.md): retain external
  authentication; clarify effect classification and gate completeness.
- [ADR-0005](../docs/adr/0005-install-seam-and-origin-model.md): extend its existing
  shared-core requirement explicitly to context, planning and all lifecycle callers.
- [ADR-0008](../docs/adr/0008-bundle-ownership-and-local-changes.md): clarify transitive
  requirements, removing intent, ordinary-path protection and override lifecycle.
- [ADR-0009](../docs/adr/0009-resolution-and-restoration-policy.md): accepted
  lock-first installation, latest-stable resolution and explicit offline behavior.
- ADR-0001/0002/0004/0006/0007 retain their command-removal, indexing, exact-pin,
  deployment and self-contained-bundle decisions. The
  [bundle requirements](../docs/requirements/team-skill-presets.md) remain the original product scope;
  its historical implementation-status prose is not current evidence of feature absence.

ADRs express decisions, not proof of implementation. The PRDs distinguish current
defects from proposed UX changes. Code readiness requires the schema/recovery
design details called out in the owning PRDs and passing behavioral acceptance.
No GitHub issues, new publishing service, private auth workflow, runtime enforcement,
fleet orchestration or automatic bundle catalog is included.

## Completion criteria

1. All referenced acceptance scenarios pass against the implemented binary/core
   seams with valid setup fixtures and isolated state.
2. The origin × scope × ownership × group matrix verifies Manifest, Lock, installed
   bytes, outcome and subsequent restore, including failure and preview paths.
3. CLI/HTTP/MCP report equivalent domain results, and write-gate coverage includes
   every registered command's actual effects.
4. README examples, command help, relevant webdocs, contributing guidance and
   shipped skills match implemented behavior. External skills-repository work is
   tracked separately rather than silently assumed complete.
5. Exact pins, bundle release immutability, protected local changes and external
   deployment/authentication boundaries remain intact.

## Implementation evidence

- The complete all-features workspace suite completed 1,810 tests with no failures and one skipped
  environment-specific test.
- The CI coverage profile reports 91.16% to 99.04% full-file line coverage for every production Rust
  file changed by the F16-F20 follow-up.
- Focused CLI, core, HTTP, and real stdio MCP scenarios cover the 55 PRD acceptance cases, including
  lock integrity, first-install closure, shared ownership, rollback, JSON output, scope routing, bundle
  preview/reset, repository selection, and read-only write denial.
- Windows validation covers immutable Git checkout line endings, isolated global state through
  `XDG_CONFIG_HOME`, JSON path rendering, and repeated editable installs without a second symlink
  privilege during transaction capture.
- A follow-up adversarial review at `089b4b9` found the F16–F20 gaps. Regression scenarios now cover
  untracked destinations, global local edits and shared graphs, selected registry versions, retained
  override closure and recovery data, and coordinated truthful HTTP updates.
- README, contributor guidance, ADRs, command help, webdocs, and the external FastSkill skill describe
  the implemented command and lifecycle behavior.
