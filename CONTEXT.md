# FastSkill

FastSkill is a package manager and operational toolkit for Claude Code-compatible **skills**: it installs them from sources, reconciles them against a manifest, and exposes them for discovery (search/read) by humans and agents.

## Language

### Core entities

**Skill**:
A unit of agent capability defined by a `SKILL.md` (frontmatter + body) plus an optional base directory of resources. Identified by a **Skill ID** derived from its directory name.

**Manifest**:
`skill-project.toml` — the *desired* set of skills (dependencies, groups, repositories). The declarative source of intent.
_Avoid_: project file, config.

**Lock**:
`skills.lock` — the *pinned* exact versions resolved from the Manifest. Used by `install --lock` for reproducible installs.
_Avoid_: lockfile (in prose), pin file.

**Installed skill**:
A skill physically present in the **skills directory** (`.claude/skills/` by default). The skills directory — not the Manifest — is the source of truth for what `list` reports.

**Reconciliation**:
The comparison of the three states — Manifest (desired), Lock (pinned), skills directory (actual) — producing a status per skill: `ok`, `missing`, `extraneous`, `mismatch`. Owned by `list`.

### Discovery axes

These two axes — not the verb names — are what actually distinguish the read-side commands. The verbs should expose them, not hide them.

**Selector**:
*How* a skill is named for a read operation. Three values, one verb each: **all** (enumerate every installed skill — `list`), **by-id** (one exact skill — `read`, with `--meta`/`--tree` for the metadata/dependency view), **by-query** (semantic match — `search`). `show` is removed; its metadata/tree view is folded into `read --meta`.

**Scope**:
*Where* skills are read from. **local** = installed skills (+ the local vector index); **remote** = repository catalogs. `search` is the only command that spans both (`--local` / `--remote`, remote is the default).
_Avoid_: source (means a repository elsewhere), location.

**Audience / depth**:
Whether output is a human summary or machine-consumable detail. `read` and the `--json`/`--paths` flags on `search` serve agents; `list`/`show` tables serve humans. (Resolved: there is no distinct *resolve* concept — machine-readable query results are `search --local --json --paths`.)

### Indexing

**Embedding provider**:
The LLM/embeddings backend (e.g. OpenAI) used to vectorize `SKILL.md` for semantic `search --local`. It is **optional and may be absent**: FastSkill must work with no embedding provider configured (keyword search only). Configuration presence is a first-class, inspectable state — `doctor` reports whether it is enabled.
_Avoid_: "the LLM" (too broad), AI backend.

**Vector index**:
The local SQLite store of embeddings produced by `reindex`, consumed by `search --local` and by `analyze` (matrix/cluster/duplicates). Only meaningful when an **Embedding provider** is configured. Rebuilding it is therefore a *conditional* step, never an unconditional one — and every consumer (`search --local`, `analyze`) inherits the same provider precondition and `doctor` visibility.

**doctor** (to introduce):
A diagnostic command that reports environment readiness — chiefly whether an **Embedding provider** is configured, so users know if semantic `reindex`/`search --local` will work before they run them.

### Distribution

The distribution commands form an orthogonal pipeline, not overlapping verbs:

**Registry index**:
The on-disk NDJSON catalog read by `fastskill serve` and `registry search`; populated externally (e.g. by the platform operator) for an **http-registry** repository; consumed by `repos`/`search --remote`. FastSkill's *native* catalog format.

**marketplace.json**:
A *distinct, first-class* catalog produced by `marketplace create`, consumed by plugin-marketplace tooling (e.g. Claude Code plugin marketplaces). **Not** interchangeable with the **Registry index** — two real formats for two different consumers; do not conflate or collapse them.

### Repositories

**Repository**:
A configured remote (or local) source of skills, managed by `repos`. Types: `git-marketplace`, `http-registry`, `zip-url`, `local`. Conflicts resolved by **priority** (lower number = higher precedence).
_Avoid_: **source**, **registry** — both are deprecated command aliases now folded into `repos`; do not reintroduce them as concepts.

### Serving surfaces

Two orthogonal, first-class ways to expose skills to a client — distinguished by *protocol/consumer*, not redundant:

**serve**:
The HTTP REST API + bundled web UI. Consumers: humans (browser), CI, REST clients.

**mcp**:
The Model Context Protocol server. Consumer: agents speaking MCP. Kept separate from `serve` on purpose — different transport, different audience. Do not fold one into the other.

## Resolved decisions

- **`sync` is removed.** It wrote skills into an agent metadata file for *older agents that lacked native skill support*. Modern targets (Claude Code) read skills directly, so the command is obsolete. Propagation now has exactly two members: `install` (Manifest → skills dir) and `reindex` (skills dir → Vector index, conditional on an Embedding provider).
- **`reindex` is conditional, never unconditional.** It runs only when an **Embedding provider** is configured. After mutating commands it may auto-run *only if* embeddings are enabled (config flag / `--reindex`/`--no-reindex` to override); with no provider it is skipped silently rather than failing. `doctor` surfaces the provider state.
- **`disable` is removed (and `enable` is not added).** The enabled/disabled flag is vestigial — disabling a skill in place is not a real workflow; the lifecycle is install ↔ remove. Drop the `disable` command; do not expose the dormant `enable_skill` core method. (The `enabled` field/filter in core becomes dead weight pending a deeper cleanup.)

## Open / pending

- _none currently — all flagged ambiguities resolved._
