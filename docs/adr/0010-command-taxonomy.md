# Use explicit command namespaces

Status: accepted. Date: 2026-09-09.

Supersedes the command-family retention clause in
[C-01](../../specs/command-consistency-and-discovery-prd.md). The lifecycle, reconciliation,
output, and failure contracts in that PRD remain in force.

## Context

FastSkill exposed unrelated root commands, nested command groups, and an implicit bare-skill-ID
read path. Bundle installation, listing, updating, and removal were selected through flags on the
skill lifecycle commands. Root help grew into a long catalog and `bundle --help` did not expose the
complete bundle lifecycle.

The project is greenfield. The accepted migration can improve the command model without aliases or
a deprecation period. The parser, help, command specification, completion output, MCP tools,
scripts, agent guidance, and documentation need one authoritative tree.

## Decision

Every operation uses an explicit resource or operational namespace followed by an action.

| Area | Namespace | Actions |
| --- | --- | --- |
| Skills and projects | `skill` | `add`, `remove`, `update`, `list`, `read`, `search` |
| Skills and projects | `bundle` | `build`, `add`, `list`, `update`, `remove`, `override` |
| Skills and projects | `project` | `init`, `install` |
| Sources and distribution | `repo` | `add`, `list`, `info`, `update`, `remove`, `test`, `refresh`, `skills`, `show`, `versions` |
| Sources and distribution | `marketplace` | `create` |
| Quality | `analysis` | `matrix`, `cluster`, `duplicates` |
| Quality | `eval` | `validate`, `run`, `judge`, `report`, `score`, `scorecard` |
| Quality | `optimization` | `run`, `resume`, `status`, `inspect`, `export` |
| Operations | `index` | `rebuild` |
| Operations | `cache` | `info`, `clean` |
| Operations | `server` | `serve` |
| Operations | `mcp` | `serve`, `install`, `list` |
| Operations | `cli` | `doctor`, `completion`, `spec` |

This tree has 49 leaf commands. The command registry remains authoritative for parser dispatch,
help, completion output, specification export, and MCP tool names. MCP names include the complete
path, such as `fastskill_skill_add` and `fastskill_bundle_remove`.

`skill add` accepts individual skills and rejects bundle artifacts before mutation. `bundle add`
accepts bundle artifacts and rejects individual skills. `project install` restores all selected
manifest declarations, including both skills and bundles. No `bundle install` command is added.

Root help is a compact, ordered table of contents. Namespace help lists its complete lifecycle.
Leaf help retains arguments, flags, scope limits, and examples. `cli completion` and `cli spec`
are framework built-ins placed under the `cli` namespace.

The migration is intentionally breaking. Old root actions, plural `repos`, implicit
`fastskill SKILL_ID` reads, and `mcp register` fail as unknown commands. They do not forward,
mutate state, or remain as hidden aliases. An error may suggest the replacement path.

## Migration map

| Removed invocation | Replacement |
| --- | --- |
| `fastskill init` | `fastskill project init` |
| `fastskill install` | `fastskill project install` |
| `fastskill add SOURCE` | `fastskill skill add SOURCE` |
| `fastskill remove ID` | `fastskill skill remove ID` |
| `fastskill update [ID]` | `fastskill skill update [ID]` |
| `fastskill list` | `fastskill skill list` |
| `fastskill read ID` | `fastskill skill read ID` |
| `fastskill search QUERY` | `fastskill skill search QUERY` |
| `fastskill add BUNDLE.zip` | `fastskill bundle add BUNDLE.zip` |
| `fastskill list --bundles` | `fastskill bundle list` |
| `fastskill update --bundle ID --from FILE` | `fastskill bundle update ID --from FILE` |
| `fastskill remove --bundle ID` | `fastskill bundle remove ID` |
| `fastskill repos ACTION` | `fastskill repo ACTION` |
| `fastskill analyze ACTION` | `fastskill analysis ACTION` |
| `fastskill optimize ACTION` | `fastskill optimization ACTION` |
| `fastskill reindex` | `fastskill index rebuild` |
| `fastskill serve` | `fastskill server serve` |
| `fastskill doctor` | `fastskill cli doctor` |
| `fastskill completion` | `fastskill cli completion` |
| `fastskill spec` | `fastskill cli spec` |
| `fastskill mcp register` | `fastskill mcp install` |
| `fastskill SKILL_ID` | `fastskill skill read SKILL_ID` |

## Consequences

- Root help stays short as new actions are added under existing namespaces.
- Skill and bundle lifecycle intent is explicit in commands, help, tests, and MCP authorization.
- Scripts and users must migrate all old invocations at the breaking release.
- Current documentation must not teach an old path outside an explicit migration or historical
  record.
- Command-document parity tests compare maintained documentation and the website with
  `fastskill cli spec`.
