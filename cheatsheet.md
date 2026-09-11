# Cheatsheet

FastSkill 0.9.229

Source: https://docs.gofastskill.com/cheatsheet

Release revision: 68975dfaca15508bc5cf2fb761d6218405c954ea

Documentation revision: 68975dfaca15508bc5cf2fb761d6218405c954ea



# FastSkill Cheatsheet

Quick reference for common FastSkill operations, ordered from everyday manifest workflow to advanced setup.

## Manifest and workspace

| Operation             | Command                                      | What It Does                                                                                                  |
| --------------------- | -------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Install from manifest | `fastskill project install`                  | Restores compatible pins, resolves uncovered roots, and records complete dependency closures                  |
| Reproducible install  | `fastskill project install --lock --offline` | Verifies and restores exact locked revisions and content without network access                               |
| Update skills         | `fastskill skill update`                     | Resolves deliberate changes within recorded constraints (see [update command](/cli-reference/update-command)) |
| Check updates only    | `fastskill skill update --check`             | Reports newer versions without changing files                                                                 |
| List installed        | `fastskill skill list`                       | Lists locally installed skills with reconciliation status (table)                                             |
| List installed (JSON) | `fastskill skill list --json`                | Same as list, machine-readable                                                                                |
| Verify selected state | `fastskill skill list --check`               | Returns nonzero for missing, mismatched, unverifiable, or conflicted required contents                        |
| Read skill content    | `fastskill skill read my-skill-id`           | Full `SKILL.md` and base path in an agent-oriented format                                                     |
| Read skill metadata   | `fastskill skill read my-skill-id --meta`    | ID, version, description, source (add `--json` for machine output)                                            |
| Dependency tree       | `fastskill skill read my-skill-id --tree`    | Dependency relationships for the skill                                                                        |
| Remove skill          | `fastskill skill remove my-skill-id`         | Removes from skills dir, updates manifest and lock; prompts for confirmation                                  |
| Force remove          | `fastskill skill remove my-skill-id --force` | Removes without confirmation                                                                                  |

## Team bundles

| Operation         | Command                                                   | What It Does                                                                         |
| ----------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Build release     | `fastskill bundle build --output dist`                    | Validates declared member versions and creates a self-contained `<id>-<version>.zip` |
| Install bundle    | `fastskill bundle add ./team-1.0.0.zip`                   | Installs members and records bundle ownership in the manifest and lock               |
| List bundles      | `fastskill bundle list`                                   | Shows installed bundle identities, versions, and members                             |
| Restore locked    | `fastskill project install --lock`                        | Restores the exact bundle releases and digests in `skills.lock`                      |
| Update bundle     | `fastskill bundle update team --from ./team-1.1.0.zip`    | Verifies and applies an explicitly selected release                                  |
| Remove bundle     | `fastskill bundle remove team`                            | Removes bundle ownership and only deletes members with no other owner                |
| Personal override | `fastskill bundle override member --from ./custom-member` | Declares a permitted replacement for an overridable member                           |
| Reset override    | `fastskill bundle override member --reset`                | Restores agreed packaged bytes and clears the personal replacement                   |

See [bundle command](/cli-reference/bundle-command) for the manifest shape and ownership rules.

## Add skills

| Operation               | Command                                                       | What It Does                                                                    |
| ----------------------- | ------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| Latest stable           | `fastskill skill add pptx@latest --repository team`           | Selects the newest stable release from the named repository                     |
| Pinned version          | `fastskill skill add pptx@1.2.3 --repository team`            | Selects that exact version and records the repository identity                  |
| From Git                | `fastskill skill add https://github.com/org/skill.git`        | Clones, validates layout, registers and installs                                |
| Editable local path     | `fastskill skill add ./dev-skill -e`                          | Symlink or reference; edits apply without reinstall                             |
| Private registry        | `fastskill skill add team-skill`                              | Uses auth from `skill-project.toml` / environment                               |
| Folder of skills        | `fastskill skill add ./skills -r`                             | Adds each subdirectory that contains `SKILL.md` (local trees only)              |
| Network share (Windows) | `fastskill skill add \\\\fileserver\\corp-skills\\team-tools` | Copies from a corporate share into the local skills directory                   |
| Offline add             | `fastskill skill add pptx@1.2.3 --repository team --offline`  | Uses a verified cached artifact without refresh, network, or automatic indexing |

## Search

| Operation              | Command                                                         | What It Does                                                       |
| ---------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------ |
| Catalog search         | `fastskill skill search "query"`                                | Searches remote catalogs by default                                |
| Installed only         | `fastskill skill search "query" --local`                        | Searches skills already on disk                                    |
| Resolve paths (agents) | `fastskill skill search "query" --local --paths`                | Emits canonical skill paths instead of result rows                 |
| Paths with content     | `fastskill skill search "query" --local --paths --content full` | Includes `SKILL.md` content (`none`, `preview`, or `full`) in JSON |

## Diagnostics

| Operation         | Command                       | What It Does                                                                                                               |
| ----------------- | ----------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Environment check | `fastskill cli doctor`        | Reports configuration and environment readiness, including whether an embedding provider is configured for semantic search |
| Machine-readable  | `fastskill cli doctor --json` | Same checks as JSON                                                                                                        |

Modern agents (Claude Code, Cursor, …) read skills directly from the skills directory — there is no metadata-file sync step. Just `fastskill project install` (or `fastskill skill add`) and the agent discovers them.


## Skill authoring

| Operation            | Command                                      | What It Does                                                                |
| -------------------- | -------------------------------------------- | --------------------------------------------------------------------------- |
| Initialize project   | `fastskill project init`                     | Reads `SKILL.md` frontmatter, prompts for gaps, writes `skill-project.toml` |
| Init with version    | `fastskill project init --set-version 1.2.3` | Sets version without prompting                                              |
| Init non-interactive | `fastskill project init --yes`               | Defaults from `SKILL.md`, no prompts                                        |

## Repositories and catalog

| Operation           | Command                                                          | What it does                                                           |
| ------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------------- |
| List repositories   | `fastskill repo list`                                            | Entries from `skill-project.toml`                                      |
| Repository details  | `fastskill repo info repo-name`                                  | Type, URL or path, priority, auth hints                                |
| Add repository      | `fastskill repo add repo-name --repo-type git-marketplace <url>` | Adds a source (`git-marketplace`, `http-registry`, `zip-url`, `local`) |
| Remove repository   | `fastskill repo remove repo-name`                                | Drops the entry                                                        |
| Update repository   | `fastskill repo update repo-name --branch main --priority 1`     | Branch or priority changes                                             |
| Test repository     | `fastskill repo test repo-name`                                  | Connectivity check                                                     |
| Refresh cache       | `fastskill repo refresh`                                         | Refreshes cached catalog metadata                                      |
| List catalog skills | `fastskill repo skills`                                          | Skills advertised by registries                                        |
| Show catalog skill  | `fastskill repo show skill-id`                                   | One skill from the catalog                                             |
| List versions       | `fastskill repo versions skill-id`                               | Published versions for an id                                           |

See [Registry overview](/registry/overview) and [`repo` commands](/cli-reference/repository-command).

## Local server

| Operation         | Command                                                                                                      | What it does                                                                                                   |
| ----------------- | ------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| HTTP server       | `fastskill server serve`                                                                                     | Web UI and HTTP API under `/api/v1/…` (`--host`, `--port`; see [`server serve`](/cli-reference/serve-command)) |
| Liveness probe    | `curl http://localhost:8080/healthz`                                                                         | Returns JSON with fastskill version; suitable for container health checks                                      |
| Readiness probe   | `curl http://localhost:8080/readyz`                                                                          | Returns 200 when ready, 503 during graceful shutdown                                                           |
| List skills (API) | `curl http://localhost:8080/api/v1/skills`                                                                   | JSON list of installed skills                                                                                  |
| Search (API)      | `curl -X POST http://localhost:8080/api/v1/search -d '{"query":"text"}' -H 'Content-Type: application/json'` | Search skills via HTTP                                                                                         |

