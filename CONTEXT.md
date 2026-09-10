# FastSkill

FastSkill is a package manager and operational toolkit for Claude Code-compatible **skills**. It
installs them from origins, reconciles them against a manifest, and exposes them through
`skill list`, `skill read`, and `skill search`.

## Language

### Command taxonomy

**Command path**:
An explicit `<namespace> <action>` pair. Namespaces identify a resource or operational area;
actions name the operation. The canonical namespaces are `skill`, `bundle`, `project`, `repo`,
`marketplace`, `analysis`, `eval`, `optimization`, `index`, `cache`, `server`, `mcp`, and `cli`.
A bare action or skill ID is not a command path. See
[ADR-0010](./docs/adr/0010-command-taxonomy.md).

**Skill lifecycle**:
`skill add`, `skill update`, and `skill remove` manage explicitly selected individual skills.

**Bundle lifecycle**:
`bundle build`, `bundle add`, `bundle list`, `bundle update`, `bundle remove`, and
`bundle override` manage bundle artifacts and ownership. Bundle artifacts are not accepted by
`skill add`.

**Project restoration**:
`project init` creates `skill-project.toml`; `project install` restores the complete selected
project, including skills and bundles.

### Core entities

**Skill**:
A unit of agent capability defined by a `SKILL.md` (frontmatter + body) plus an optional base directory of resources. Identified by a **Skill ID** derived from its directory name.

**Manifest**:
`skill-project.toml` — the *desired* set of skills and bundles, with their dependencies, groups, and repositories. The declarative source of intent.
_Avoid_: project file, config.

**Lock**:
`skills.lock` — the *pinned* exact skill versions and bundle releases resolved from the Manifest.
Used by `project install --lock` for reproducible installs.
_Avoid_: lockfile (in prose), pin file.

**Installed skill**:
A skill physically present in the **skills directory** (`.claude/skills/` by default). The skills
directory — not the Manifest — is the source of truth for what `skill list` reports.

**Reconciliation**:
The comparison of the three states — Manifest (desired), Lock (pinned), skills directory
(actual) — producing a status per skill: `ok`, `missing`, `extraneous`, `mismatch`. Owned by
`skill list`.

**Installation scope**:
The project or global environment whose requirements and installed contents an
operation manages. Distinct from the local/remote discovery scope below.

**Dependency root**:
An explicitly selected individual skill, bundle, or personal replacement that
requires a skill and its dependencies to remain in the environment.

**Dependency edge**:
A recorded requirement from one skill to another. A shared dependency can have
multiple incoming edges and remain required after one root is removed.

**Operation plan**:
A proposed change to selected revisions, ownership, and installed contents,
including unchanged selections and conflicts. A preview renders this plan.

**Version constraint**:
The *allowed range* a Manifest dependency accepts, used only to filter candidate versions during resolution — distinct from the Lock, which pins the one chosen version. A **bare version (`1.2.3`) means exactly that version**, not a compatible range; ranges are opt-in via explicit `^`/`~`/`>=`/`<=`/comma operators. See [ADR-0004](./docs/adr/0004-bare-version-is-exact.md).
_Avoid_: version requirement, semver range (a bare version is *not* a range here).

### Team presets

**Bundle**:
A named, versioned collection distributed as a self-contained ZIP package containing a `skill-project.toml` and the exact contents of its skills and their skill dependencies.
_Avoid_: skill (when referring to the whole package).

**Installed bundle**:
A bundle recorded on a target with its identity, version, and skill membership, so its skills can be managed together while remaining individually inspectable.

**Bundle release**:
An immutable set of packaged contents identified by a bundle identity and version. Its version is independent of the versions of its member skills.

**Skill ownership**:
The recorded bundle memberships, individual selections, and transitive requirements
that require an installed skill to remain present. Multiple owners may share a
skill when their selected contents agree, subject to the personal override rules.

**Personal override**:
An explicitly declared personal skill selection with its own origin that replaces bundled contents where every owning bundle permits it. An untracked edit is not an approved override.

**Preset**:
A named selection of skills and customization rules for a team or project.

**Required skill**:
A skill that a setup must contain at an allowed revision to satisfy its preset.

**Overridable default**:
A preset skill selection that a user may replace within the preset's customization rules.

**Permitted addition**:
A skill outside the preset's selections that its customization rules allow a user to add.

### Discovery axes

These two axes — not the verb names — are what actually distinguish the read-side commands. The verbs should expose them, not hide them.

**Selector**:
*How* a skill is named for a read operation. Three values, one action each: **all** (enumerate every
installed skill — `skill list`), **by-id** (one exact skill — `skill read`, with
`--meta`/`--tree`), and **by-query** (semantic match — `skill search`). The historical `show`
command was removed; its metadata/tree view moved to `skill read --meta`.

**Scope**:
*Where* skills are read from. **local** = installed skills plus the local vector index;
**remote** = repository catalogs. `skill search` is the only command that spans both
(`--local` / `--remote`; remote is the default).
_Avoid_: source (means a repository elsewhere), location.

**Audience / depth**:
Whether output is a human summary or machine-consumable detail. `skill read` and the
`--json`/`--paths` flags on `skill search` serve agents; `skill list` serves humans. There is no
distinct *resolve* command; machine-readable query results use
`skill search --local --json --paths`.

### Indexing

**Embedding provider**:
The LLM/embeddings backend (e.g. OpenAI) used to vectorize `SKILL.md` for semantic
`skill search --local`. It is **optional and may be absent**: FastSkill must work with no embedding
provider configured. `cli doctor` reports whether it is enabled.
_Avoid_: "the LLM" (too broad), AI backend.

**Vector index**:
The local SQLite store of embeddings produced by `index rebuild`, consumed by
`skill search --local` and `analysis matrix/cluster/duplicates`. It is meaningful only when an
**Embedding provider** is configured. Rebuilding is conditional, and every semantic consumer
inherits the same provider precondition and `cli doctor` visibility.

**cli doctor**:
A diagnostic command that reports environment readiness, chiefly whether an **Embedding provider**
is configured, so users know if semantic `index rebuild` and `skill search --local` will work.

### Distribution

The distribution commands form an orthogonal pipeline, not overlapping verbs:

**Registry index**:
The on-disk NDJSON catalog read by `fastskill server serve`; populated externally for an
**http-registry** repository; consumed by `repo skills` and `skill search --remote`. FastSkill's
native catalog format.

**marketplace.json**:
A *distinct, first-class* catalog produced by `marketplace create`, consumed by plugin-marketplace
tooling. It is not interchangeable with the **Registry index**.

### Repositories

**Repository**:
A configured remote or local origin catalog, managed by `repo`. Types: `git-marketplace`,
`http-registry`, `zip-url`, and `local`. Conflicts resolve by **priority** (lower number = higher
precedence).
_Avoid_: **source**, **registry**, **repos** — historical command names must not be reintroduced as
current concepts.

**Origin**:
Where a single installed skill came from — the install **intent** (what the user asked for), recorded as provenance on the installed skill. Variants: `git` (url + ref + subdir), `local` (a filesystem path — directory *or* `.zip` — plus `editable`, dir-only), `zip-url` (a remote zip), and `repository` (a *reference into* a configured **Repository**: `{repo, skill, version?}`). The `repository` variant is the only one **Version constraint** / ADR-0004 governs; `git`/`local`/`zip-url` are ref-based and versionless. `Origin` is intent only: the **resolved** facts (exact commit, resolved version, checksum, timestamps) live in the **Lock**, not in `Origin`. It is the single canonical model — replacing the former `SkillSource` (two colliding types), `SourceType`, `SourceSpecificFields`, and the flat `source_*` fields on the manifest/lock/skill records.
_Avoid_: **source** (banned, see above); do not blur `Origin::repository` (a reference; always names a concrete Repository) with **Repository** (the configured place itself).

**Origin ref**:
The *textual* form of an **Origin** — the single string a user types to name where a skill comes
from. It is resolved into a typed `Origin` by one seam, `Origin::infer(&str)`, which is the only
place ref-to-`Origin` inference lives. Both `skill add` and the HTTP install route call it. An
Origin ref is unresolved intent as text; the `Origin` is typed intent; the **Lock** holds resolved
facts.

### Serving surfaces

Two orthogonal, first-class ways to expose skills to a client — distinguished by *protocol/consumer*, not redundant:

**server serve**:
The HTTP REST API + bundled web UI. Consumers: humans (browser), CI, REST clients.

**mcp serve**:
The Model Context Protocol server. Its consumers are agents speaking MCP. It remains separate from
`server serve` because the protocol and audience differ.

### Evaluation

The eval commands consume the **Skill Evaluation** vocabulary of `goaikit/aikit` (`aikit-evals/CONTEXT.md`) — Case, Trial, Check, Suite, Trial outcome, Case verdict, Judge, Judgment, Score, Run directory — and add only the terms below, which exist because fastskill is where many run directories are read together.

**Benchmark**:
The environment a scorecard measures: the suites — cases, checks, judges — that a metrics file selects over, and the metrics themselves. Identified by the content hash of that file and of the suite files it selects; a benchmark whose gates or selections changed is a different question, not a later answer to the same one, so scorecards of different benchmarks are never drawn on one line.
_Avoid_: Sweep, spec, eval config, test plan

**Metric**:
One question a benchmark asks over a set of run directories — a rate of check results, a percentile of tool calls, or a mean judge score — selected by case-id pattern and by check or judge name. A gated metric carries a threshold and a **verdict**, PASS or FAIL; an ungated metric is reported and never decides. A metric that selects nothing is an error, never an absence.
_Avoid_: KPI, stat, measure, gate (the threshold is the gate; the metric is the question), check (per trial, upstream)

**Scorecard**:
One evaluation of a benchmark against a set of run directories, carrying enough of its own identity — when, the target agent and model, the skill revision, the benchmark hash, the judge identities — to be rendered or compared later without those directories. One scorecard is one point on a progress line.
_Avoid_: Sweep, report (a rendering), summary (one run directory's, upstream), results

**Report**:
A rendering of measurements already made — one run directory's summary, or one or more scorecards — as a table, JSON, or one self-contained HTML file with every asset embedded. Producing a report never runs an agent and never re-scores; it reads scorecard files, not run directories, so it can be made anywhere the files are.
_Avoid_: Dashboard, page, export, scorecard (the measurement it renders)

## Resolved decisions

- **Historical `sync` is removed.** Modern targets read skills directly. Propagation has exactly two
  members: `project install` (Manifest → skills directory) and `index rebuild` (skills directory →
  Vector index, conditional on an Embedding provider).
- **`index rebuild` is conditional.** It runs only when an **Embedding provider** is configured.
  Mutating commands may auto-run it only when embeddings are enabled; `--reindex` and
  `--no-reindex` override configuration. `cli doctor` surfaces provider state.
- **Historical `disable` is removed.** The lifecycle is `skill add` ↔ `skill remove`. Do not expose
  the dormant `enable_skill` core method.
- **Command paths use explicit namespaces.** Old root actions, plural `repo`, and implicit skill-ID
  reads are removed without aliases. See [ADR-0010](./docs/adr/0010-command-taxonomy.md).

## Related decisions

- [ADR-0009](./docs/adr/0009-resolution-and-restoration-policy.md) defines lock-first restoration
  and explicit latest-stable resolution.
- [ADR-0010](./docs/adr/0010-command-taxonomy.md) defines the command namespaces and breaking
  migration policy.
