# FastSkill Manual Smoke Test Plan

> **Purpose.** Exercise every fastskill command surface and runtime environment by hand,
> the way a real user meets them, to validate that features work **and** that the
> documentation matches the implementation. Automated tests already prove the code paths;
> this plan exists to catch what they can't — confusing errors, doc-vs-reality drift, and
> rough edges. A confused tester is itself a finding.

**Audience.** A technical tester who is comfortable in a terminal but new to fastskill.
Every step gives the exact command and what "good" looks like. You are *not* expected to
know fastskill's mental model — if a step doesn't make sense from the docs alone, that's a
GAP, record it.

**This is the blind document.** Do not go hunting for a list of "known issues" — there
isn't one in this file on purpose. Report what you actually observe. Maintainers hold a
separate triage key.

---

## How to use this plan

Each step is one row:

| Field | Meaning |
|---|---|
| **Command** | Type it exactly. `$FSREPO`, `$SKILLREPO`, `$SBX` are set in Section 0. |
| **Expected observable** | The concrete, checkable result: an exit code, an output substring, or a file that should exist. |
| **Mode** | 🧑 human-judgment · 🤖 scriptable · ⚙️ automation-gap (see legend). |
| **Src** | Where the behavior is documented — the "source of truth" you are checking reality against. |
| **Result** | Mark `P` (pass), `F` (fail), or `G` (gap) and add a note. |

**Result codes:**
- **P — Pass:** actual matches expected.
- **F — Fail:** actual ≠ expected. A bug. Capture the full command + output.
- **G — Gap:** the command works but the **docs/help are wrong** — they describe something
  that doesn't exist, is spelled differently, behaves differently, or is missing entirely.
  This is the primary thing we're hunting. Note *which doc* and *what it said* vs reality.

> A step can be both: the command fails **and** the docs are misleading → mark `F` and note the doc issue too.

**Execution-mode legend:**
- 🧑 **Human-judgment** — needs a person to decide "is this error message actually helpful?
  does this doc match what I see?" Automation can't judge these.
- 🤖 **Scriptable** — a pure check (exit code / substring / file exists). Runnable now, and
  the exact step an agent would inherit if this plan is later delegated.
- ⚙️ **Automation-gap** — this really ought to be an automated integration test. Run it now,
  but it also feeds the **Automation-Gap Backlog** at the end of this document.

**Version honesty.** You are testing an installed **released** binary. Its behavior is
ground truth. When you check the CLI surface, trust `fastskill spec` (emitted by the binary
you're running) over the prose docs — the docs track the latest development and may describe
things a released binary doesn't have yet. For CLI-shape mismatches, `spec` wins. For prose
and workflow docs, mark the GAP but add "(confirm vs tested version)".

---

## Section 0 — Before you start (Preflight & Setup)

### 0.1 Install the binary under test

Install the **latest released** fastskill for your platform (see **Appendix A** for
Homebrew / Scoop / install.sh / release archive). Do **not** build from source for the main
plan — we test what ships.

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 0.1.1 | `fastskill --version` | Prints `fastskill <X.Y.Z>`, exit 0. **Record this version — every finding is relative to it.** | 🤖 | `--help` | ☐P ☐F ☐G |
| 0.1.2 | `which fastskill` | Resolves to the installed release binary (not a source `target/` build). | 🧑 | — | ☐P ☐F ☐G |
| 0.1.3 | `fastskill --help` | Lists command groups: Discovery, Packages, Project, Server, Setup, and analyze/completion/eval/marketplace/mcp/optimize/repos/spec. Exit 0. | 🧑 | README | ☐P ☐F ☐G |
| 0.1.4 | `fastskill spec --format markdown > /tmp/fs-smoke-spec.md` | Writes the full command surface. **This file is your CLI source-of-truth for the rest of the plan.** | 🤖 | `spec` | ☐P ☐F ☐G |

### 0.2 Set environment variables

Set these for your shell session (adjust paths to your checkouts):

```bash
export FSREPO=<path to gofastskill/fastskill checkout>     # for test fixtures
export SKILLREPO=<path to gofastskill/skill checkout>      # for the proven eval suite
export SBX=/tmp/fs-smoke                                    # throwaway sandbox
export SKILLS_DIR=$SBX/skills                               # isolated skills dir
mkdir -p "$SKILLS_DIR" "$SBX/project"
```

> **Isolation:** every core-path command uses `--skills-dir "$SKILLS_DIR"` so you never
> touch your real skills. Delete `$SBX` at the end (teardown).

### 0.3 Provisioning checklist (full-exercise plan)

Confirm each before the sections that need it; if one is missing, mark those sections
BLOCKED rather than skipping silently.

| Dependency | Needed by | Check |
|---|---|---|
| `codex` CLI installed + authenticated | Eval (S9), Optimize (S10) | `codex --version` |
| `claude` CLI installed + authenticated | Eval (S9), Optimize (S10) | `claude --version` |
| `OPENAI_API_KEY` (real OpenAI) | Analyze/Semantic (S8) variant A | `echo ${OPENAI_API_KEY:+set}` |
| Gateway key + base URL (OpenAI-compatible) | Analyze/Semantic (S8) variant B | your gateway creds |
| Network reachability | Package git-add (S3), Distribution appendix | `git ls-remote https://github.com/gofastskill/skill.git` |

### 0.4 Build the fixtures the repo doesn't ship

Two `add` modes and the optimize command need fixtures that aren't committed. Build them now
(this is also a mini doc-walk — if these steps are unclear, note it).

```bash
# A .zip skill (for zip-add) — zipped from a committed folder fixture
( cd "$FSREPO/tests/cli/fixtures" && zip -r "$SBX/minimal-skill.zip" minimal-skill )

# An optimize config over the sibling skill's proven eval suite
cat > "$SBX/optimize.toml" <<EOF
skill         = "$SKILLREPO/fastskill/SKILL.md"
skill_name    = "fastskill"
suite         = "$SKILLREPO/evals/prompts.csv"
checks        = "$SKILLREPO/evals/checks.toml"
out_dir       = "$SBX/skillopt-runs"

target_agent    = "claude"
optimizer_agent = "claude"

n_epochs                 = 1
batch_size               = 1
accumulation             = 1
aggregate_group_size     = 1
lr_0                     = 1
pass_threshold           = 0.7
gate_metric              = "hard"
gate_trials              = 1
gate_epsilon             = 0.0
slow_update_mode         = "gated"
protected_soft_cap_chars = 2000
timeout_seconds          = 120
EOF
```

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 0.4.1 | (zip command above) | `$SBX/minimal-skill.zip` exists, exit 0. | 🤖 | — | ☐P ☐F ☐G |
| 0.4.2 | (optimize.toml heredoc above) | `$SBX/optimize.toml` exists with real paths. | 🤖 | webdocs `optimize/configuration` | ☐P ☐F ☐G |

### 0.5 Doctor

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 0.5.1 | `fastskill doctor` | Runs a set of named checks, human-readable, exit 0. Read every line — does each check name + message make sense to a newcomer? | 🧑 | `doctor` | ☐P ☐F ☐G |
| 0.5.2 | `fastskill doctor --json` | Same results as valid JSON. | 🤖 | `doctor` | ☐P ☐F ☐G |

---

## Core spine (Sections 1–3 — shared sandbox, run in order)

These build on each other's state in `$SKILLS_DIR`. Run top to bottom.

### Section 1 — Discovery

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 1.1 | `fastskill --skills-dir "$SKILLS_DIR" list` | Empty sandbox → prints "no skills"/empty list, exit 0 (not an error). | 🤖 | webdocs `skill-management` | ☐P ☐F ☐G |
| 1.2 | `fastskill --skills-dir "$SKILLS_DIR" list --format json` | Valid JSON (empty array). Confirm `--format`/`--json` documented. | 🤖 | `spec` | ☐P ☐F ☐G |
| 1.3 | `fastskill --skills-dir "$SKILLS_DIR" search test` | Runs without an index; falls back to text search or empty result, exit 0. | 🧑 | webdocs `skill-management` | ☐P ☐F ☐G |
| 1.4 | `fastskill --skills-dir "$SKILLS_DIR" read nonexistent-skill` | Clean "not found" message, non-zero exit — **is the message helpful?** | 🧑 | `read` help | ☐P ☐F ☐G |

*(1.1–1.2 revisited in Section 3 once skills are installed.)*

### Section 2 — Project init

Work in a scratch skill dir so `init` has somewhere to write.

```bash
mkdir -p "$SBX/newskill" && cd "$SBX/newskill"
```

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 2.1 | `fastskill init` | Creates `skill-project.toml` in cwd, exit 0, validates after writing. Open the file — does it match the documented schema? | 🧑 | webdocs `configuration/init-command` | ☐P ☐F ☐G |
| 2.2 | `cat skill-project.toml` | Contains a `[metadata]` (skill) or `[tool.fastskill]` (project) block as the docs describe. | 🧑 | init docs | ☐P ☐F ☐G |
| 2.3 | `fastskill init` (again, no flag) | Refuses to clobber / warns (file exists). | 🤖 | init help | ☐P ☐F ☐G |
| 2.4 | `fastskill init --force` | Overwrites, exit 0. | 🤖 | init help | ☐P ☐F ☐G |
| 2.5 | `cd -` | Return to prior dir. | 🤖 | — | ☐P ☐F ☐G |

### Section 3 — Packages (add / install / update / remove / reindex)

All commands use `--skills-dir "$SKILLS_DIR"`.

#### 3a — `add` in every mode

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 3.1 | `fastskill --skills-dir "$SKILLS_DIR" add "$FSREPO/tests/cli/fixtures/minimal-skill"` | Adds the skill, exit 0, prints the added id. | 🤖 | webdocs `skill-management` | ☐P ☐F ☐G |
| 3.2 | `fastskill --skills-dir "$SKILLS_DIR" add "$FSREPO/tests/cli/fixtures/complex-skill"` | Adds a second skill, exit 0. | 🤖 | " | ☐P ☐F ☐G |
| 3.3 | `fastskill --skills-dir "$SKILLS_DIR" add "$FSREPO/tests/cli/fixtures/invalid-skill"` | **Fails cleanly** with a validation error (bad name/version). **Is the error specific and actionable?** | 🧑 | " | ☐P ☐F ☐G |
| 3.4 | `fastskill --skills-dir "$SKILLS_DIR" add "$SBX/minimal-skill.zip" --force` | Installs from the zip (`--force` since minimal-skill already added), exit 0. | 🤖 | add help | ☐P ☐F ☐G |
| 3.5 | `fastskill --skills-dir "$SKILLS_DIR" add "$FSREPO/tests/cli/fixtures" --recursive` | Adds all skills under the dir; the invalid one is reported as a failure but others succeed. | 🧑 | add help | ☐P ☐F ☐G |
| 3.6 | `fastskill --skills-dir "$SKILLS_DIR" add "$FSREPO/tests/cli/fixtures/minimal-skill" --editable --force` | Installs in editable mode, exit 0. | 🤖 | add help | ☐P ☐F ☐G |
| 3.7 | `fastskill --skills-dir "$SKILLS_DIR" add "$FSREPO/tests/cli/fixtures/complex-skill" --group dev --force` | Adds to group `dev`, exit 0. | 🤖 | add help | ☐P ☐F ☐G |
| 3.8 | `fastskill --skills-dir "$SKILLS_DIR" add https://github.com/gofastskill/skill.git` | (Network) clones + adds from git URL, exit 0. | 🤖 | `skill/README` | ☐P ☐F ☐G |
| 3.9 | `fastskill --skills-dir "$SKILLS_DIR" add https://github.com/gofastskill/skill.git --branch main --force` | `--branch` honored (git only), exit 0. | 🤖 | add help | ☐P ☐F ☐G |
| 3.10 | `fastskill --skills-dir "$SKILLS_DIR" add ./nope-does-not-exist` | Clean error, non-zero exit. **Helpful?** | 🧑 | — | ☐P ☐F ☐G |

#### 3b — list / read the installed skills

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 3.11 | `fastskill --skills-dir "$SKILLS_DIR" list` | Shows the skills added above, exit 0. | 🤖 | skill-management | ☐P ☐F ☐G |
| 3.12 | `fastskill --skills-dir "$SKILLS_DIR" read <an-installed-id>` | Prints the full SKILL.md content, exit 0. | 🤖 | read help | ☐P ☐F ☐G |

#### 3c — install from manifest

```bash
cp "$FSREPO/tests/cli/fixtures/sample-skill-project.toml" "$SBX/project/skill-project.toml"
cd "$SBX/project"
```

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 3.13 | `fastskill install` | Reads `[dependencies]`, resolves them, installs into `[tool.fastskill].skills_directory`, writes/updates `skills.lock`. **Note:** deps reference a registry/git/zip-url — network-dependent; a resolution failure here may be an environment limit, record which dep failed. | 🧑 | webdocs `skill-management` | ☐P ☐F ☐G |
| 3.14 | `ls skills.lock` | Lockfile exists after install. | 🤖 | install help | ☐P ☐F ☐G |
| 3.15 | `fastskill install --lock` | Installs exact pinned versions from `skills.lock`, exit 0. | 🤖 | install help | ☐P ☐F ☐G |
| 3.16 | `cd -` | Return to sandbox. | 🤖 | — | ☐P ☐F ☐G |

#### 3d — update / remove / reindex

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 3.17 | `fastskill --skills-dir "$SKILLS_DIR" update` | Checks/updates installed skills, exit 0. | 🤖 | update help | ☐P ☐F ☐G |
| 3.18 | `fastskill --skills-dir "$SKILLS_DIR" reindex` | With **no embedding provider** configured here: prints "Reindex skipped: … Run 'fastskill doctor' for setup guidance." and **exits 0** (informational, not an error). | 🧑 | webdocs `configuration` | ☐P ☐F ☐G |
| 3.19 | `fastskill --skills-dir "$SKILLS_DIR" remove <an-installed-id>` | Removes from manifest + disk, exit 0. | 🤖 | remove help | ☐P ☐F ☐G |
| 3.20 | `fastskill --skills-dir "$SKILLS_DIR" list` | Removed skill is gone. | 🤖 | — | ☐P ☐F ☐G |

---

## Independent sections (own setup/teardown — run in any order)

### Section 4 — Global environment (`--global`)

Exercises the global skills dir at `~/.config/fastskill/skills`. **This writes to your real
global dir** — note anything you add so you can remove it.

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 4.1 | `fastskill --global list` | Lists global skills (maybe empty), exit 0. | 🤖 | `--global` flag | ☐P ☐F ☐G |
| 4.2 | `fastskill --global add "$FSREPO/tests/cli/fixtures/minimal-skill" --force` | Installs into the global dir, exit 0. | 🤖 | — | ☐P ☐F ☐G |
| 4.3 | `fastskill --global list` | Shows the just-added skill. | 🤖 | — | ☐P ☐F ☐G |
| 4.4 | `fastskill --global remove test-minimal` | Removes it (cleanup). | 🤖 | — | ☐P ☐F ☐G |

### Section 5 — Serve (HTTP API)

> **Port note:** `serve` defaults to `localhost:8080`. `mcp serve` (Section 6) *also*
> defaults to 8080 — don't run both at once.

**5a — Read-only (default):** start in one terminal:
```bash
cd "$SBX/project" && fastskill serve --skills-dir "$SKILLS_DIR"
```

| # | Command (in a second terminal) | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 5.1 | `curl -s localhost:8080/healthz` | `200`, body `{"status":"ok","version":"…"}`. | 🤖 | webdocs `cli-reference/serve-command` | ☐P ☐F ☐G |
| 5.2 | `curl -s localhost:8080/readyz` | `200` when ready. | 🤖 | serve-command | ☐P ☐F ☐G |
| 5.3 | `curl -s localhost:8080/api/v1/status` | JSON with `writable` and `embeddingProvider` flags. `writable` should be **false** here. | 🤖 | serve-command | ☐P ☐F ☐G |
| 5.4 | `curl -s localhost:8080/api/v1/skills` | JSON list of installed skills, `200`. | 🤖 | serve-command | ☐P ☐F ☐G |
| 5.5 | `curl -s -X POST localhost:8080/api/v1/search -H 'Content-Type: application/json' -d '{"query":"pdf tools"}'` | `200`, JSON results (POST-but-read). | 🤖 | webdocs `search-command` | ☐P ☐F ☐G |
| 5.6 | `curl -s -X POST localhost:8080/api/v1/resolve -H 'Content-Type: application/json' -d '{"prompt":"help me edit a spreadsheet"}'` | `200`, resolved context. | 🤖 | serve-command | ☐P ☐F ☐G |
| 5.7 | `curl -s -o /dev/null -w '%{http_code}' -X DELETE localhost:8080/api/v1/skills/whatever` | **403** (write disabled), body message "write operations disabled; start server with --enable-write". **Confirm 403, not 404.** | 🤖 | serve-command | ☐P ☐F ☐G |
| 5.8 | `curl -s localhost:8080/` | Serves the embedded dashboard UI (HTML), `200`. | 🧑 | serve-command | ☐P ☐F ☐G |
| 5.9 | `curl -s -o /dev/null -w '%{http_code}' localhost:8080/api/skills` | **308** redirect to `/api/v1/skills` (version pinning). | 🤖 | serve-command | ☐P ☐F ☐G |

Stop the server (Ctrl-C). **5b — Write-enabled:**
```bash
fastskill serve --skills-dir "$SKILLS_DIR" --enable-write
```

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 5.10 | `curl -s localhost:8080/api/v1/status` | `writable: true`. | 🤖 | serve-command | ☐P ☐F ☐G |
| 5.11 | `curl -s -X POST localhost:8080/api/v1/skills/install -H 'Content-Type: application/json' -d '{"origin":{"type":"local","path":"'"$FSREPO"'/tests/cli/fixtures/minimal-skill"}}'` | `201` (or `409` if already installed) — **write now allowed**. | 🤖 | serve-command | ☐P ☐F ☐G |
| 5.12 | `curl -s -X POST localhost:8080/api/v1/skills/update -H 'Content-Type: application/json' -d '{"check":true}'` | `200`, dry-run update result. | 🤖 | serve-command | ☐P ☐F ☐G |

Stop the server.

### Section 6 — MCP

> Entire section is ⚙️ **automation-gap** — the handshake/E2E belong in an integration test.
> Run them now and note them for the backlog.

**6a — install/register config writes.** Run in a scratch dir so project-scope configs land there:
```bash
mkdir -p "$SBX/mcp" && cd "$SBX/mcp"
```

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 6.1 | `fastskill mcp list` | Lists the 6 supported agent targets: claude, cursor(-agent), gemini, copilot, opencode, codex. | 🤖 | cli-framework MCP | ☐P ☐F ☐G |
| 6.2 | `fastskill mcp install --agent claude --scope project` | Writes `./.mcp.json` (key `mcpServers`), prints the path. | ⚙️ | " | ☐P ☐F ☐G |
| 6.3 | `fastskill mcp install --agent cursor --scope project` | Writes `./.cursor/mcp.json` (`cursor` is the canonical key; `cursor-agent` was removed, aikit ADR 0015). | ⚙️ | " | ☐P ☐F ☐G |
| 6.4 | `fastskill mcp install --agent gemini --scope project` | Writes `./.gemini/settings.json`. | ⚙️ | " | ☐P ☐F ☐G |
| 6.5 | `fastskill mcp install --agent copilot --scope project` | Writes `./.vscode/mcp.json` (key `servers`, VS Code shape). | ⚙️ | " | ☐P ☐F ☐G |
| 6.6 | `fastskill mcp install --agent opencode --scope project` | Writes `./opencode.json` (root `mcp` map). | ⚙️ | " | ☐P ☐F ☐G |
| 6.7 | `fastskill mcp install --agent codex --stdio` | Writes `./.codex/config.toml` (`[mcp_servers.…]`) with the current executable as a stdio command. | ⚙️ | " | ☐P ☐F ☐G |
| 6.8 | `fastskill mcp register --agent claude --scope project` | Still succeeds and writes the same `./.mcp.json` as 6.2 — `register` is a **withdrawn alias**, not a removed command (cli-framework `d1b1c61`). Also confirm neither `mcp --help` nor `fastskill spec` advertises it any more. | ⚙️ | " | ☐P ☐F ☐G |

**6b — serve + protocol handshake.**

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 6.9 | `fastskill mcp serve --transport stdio` then pipe a JSON-RPC `initialize` + `tools/list` on stdin | Returns a read-only tool list: `list`, `read`, `search`, `repos list`, `eval …`, etc. **`serve` (HTTP) must NOT appear as a tool**, and neither may any mutating tool (`add`, `install`, `remove`, `update`, `reindex`, `repos add/remove/update/refresh`, `marketplace create`, `optimize run/resume`, `cache clean`, `init`) — they are write-gated (ADR-0003). | ⚙️ | main.rs export policy | ☐P ☐F ☐G |
| 6.9a | Same session, `tools/call` `fastskill_remove` on an installed skill **without** `--enable-write` | JSON-RPC error `-32005 MCP_TOOL_DENIED` naming `--enable-write`; **the skill directory is still on disk.** | ⚙️ | commands/mcp.rs | ☐P ☐F ☐G |
| 6.9b | `fastskill mcp serve --transport stdio --enable-write`, then `tools/list` + `tools/call` `fastskill_remove` | `fastskill_remove` is listed and the call succeeds; the skill is removed. | ⚙️ | " | ☐P ☐F ☐G |
| 6.10 | `fastskill mcp serve` (http, default `127.0.0.1:8080/mcp`) then `curl` a JSON-RPC `tools/list` to `/mcp` | Same tool list over HTTP, `200`. | ⚙️ | cli-framework MCP | ☐P ☐F ☐G |
| 6.11 | `fastskill mcp serve --transport stdio --host 0.0.0.0` | **Rejected** with `[E004] … '--host', '--port', '--path' are only valid when --transport=http`. | 🤖 | commands.rs | ☐P ☐F ☐G |

**6c — one real-agent E2E (optional but requested).**

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 6.12 | Point Claude Code at the `./.mcp.json` from 6.2 and invoke one fastskill tool (e.g. `list`) | The agent lists skills through the MCP server end-to-end. | 🧑 | — | ☐P ☐F ☐G |

Cleanup: `rm -rf "$SBX/mcp"`. `cd "$SBX"`.

### Section 7 — Repos (deterministic local path)

> We use a **local** repo (no network). This exercises the full repos command surface
> deterministically. `repos add` writes into `skill-project.toml`, so work in a project dir.

```bash
mkdir -p "$SBX/repo-src/demo-skill"
cp "$FSREPO/tests/cli/fixtures/minimal-skill/SKILL.md" "$SBX/repo-src/demo-skill/SKILL.md"
cd "$SBX/project"   # has a skill-project.toml from Section 3
```

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 7.1 | `fastskill repos list` | Lists configured repos (may be empty if this project has none), exit 0. | 🤖 | webdocs `cli-reference/repository-command` | ☐P ☐F ☐G |
| 7.2 | `fastskill repos add demo-local --repo-type local "$SBX/repo-src"` | Prints `Added repository: demo-local`; writes `[[tool.fastskill.repositories]]` into `skill-project.toml`. | 🤖 | repository-command | ☐P ☐F ☐G |
| 7.3 | `grep -A3 repositories skill-project.toml` | The repo block is persisted in the TOML. | 🤖 | " | ☐P ☐F ☐G |
| 7.4 | `fastskill repos list` | Now shows `demo-local`. | 🤖 | " | ☐P ☐F ☐G |
| 7.5 | `fastskill repos info demo-local` | Shows repo details, exit 0. | 🤖 | " | ☐P ☐F ☐G |
| 7.6 | `fastskill repos test demo-local` | Connectivity/validity check passes for a local repo. | 🤖 | " | ☐P ☐F ☐G |
| 7.7 | `fastskill repos refresh` | Refreshes the catalog cache, exit 0. | 🤖 | " | ☐P ☐F ☐G |
| 7.8 | `fastskill repos skills demo-local` | For a **local** repo this is expected to error with "is not an HTTP registry" (documented limitation). **Is that message clear?** | 🧑 | repos integration tests | ☐P ☐F ☐G |
| 7.9 | `fastskill repos show <scope/id>` | Catalog lookup; note behavior for a local repo. | 🧑 | " | ☐P ☐F ☐G |
| 7.10 | `fastskill repos versions <scope/id>` | Lists versions or a clean "unavailable for local", exit code as documented. | 🧑 | " | ☐P ☐F ☐G |
| 7.11 | `fastskill search --remote something` | With no http-registry configured, returns an **empty** result set (not an error). | 🤖 | search remote | ☐P ☐F ☐G |
| 7.12 | `fastskill add scope/some-id` | Requires an http-registry default repo → **errors** "is not an http-registry type". **Helpful?** | 🧑 | add sources | ☐P ☐F ☐G |
| 7.13 | `fastskill repos update demo-local --priority 5` | Updates metadata, persists, exit 0. | 🤖 | repository-command | ☐P ☐F ☐G |
| 7.14 | `fastskill repos remove demo-local` | Removes from the TOML, exit 0. | 🤖 | " | ☐P ☐F ☐G |
| 7.15 | `cd -` | Return. | 🤖 | — | ☐P ☐F ☐G |

### Section 8 — Analyze & semantic search (embeddings)

Needs an embedding provider. We run **two variants** to prove `openai_base_url` is honored.
Configure a project and reindex.

```bash
cd "$SBX/project"
# ensure several skills are installed into this project's skills dir first
fastskill --skills-dir "$SKILLS_DIR" add "$FSREPO/tests/cli/fixtures/minimal-skill" --force
fastskill --skills-dir "$SKILLS_DIR" add "$FSREPO/tests/cli/fixtures/complex-skill" --force
```

**Variant A — real OpenAI.** Add to `skill-project.toml`:
```toml
[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
```
`export OPENAI_API_KEY=<real key>`

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 8.1 | `fastskill doctor` | `embedding_config` = Pass ("configuration found"); `api_key` = Pass ("OPENAI_API_KEY is set"). | 🤖 | doctor | ☐P ☐F ☐G |
| 8.2 | `fastskill --skills-dir "$SKILLS_DIR" reindex` | Actually builds the index (not skipped), exit 0, reports a count. | 🤖 | configuration | ☐P ☐F ☐G |
| 8.3 | `fastskill --skills-dir "$SKILLS_DIR" search "edit documents" --embedding true` | Returns semantically-ranked results, exit 0. | 🧑 | search-command | ☐P ☐F ☐G |
| 8.4 | `fastskill --skills-dir "$SKILLS_DIR" analyze matrix` | Pairwise similarity output, exit 0. | 🤖 | analyze | ☐P ☐F ☐G |
| 8.5 | `fastskill --skills-dir "$SKILLS_DIR" analyze duplicates` | Duplicate/near-duplicate pairs (or none), exit 0. | 🤖 | analyze | ☐P ☐F ☐G |
| 8.6 | `fastskill --skills-dir "$SKILLS_DIR" analyze cluster --num-clusters 2` | Clusters the skills, exit 0. | 🤖 | analyze | ☐P ☐F ☐G |
| 8.7 | `curl -s localhost:8080/api/v1/status` (with a `serve` running on this project) | `embeddingProvider: true`. | 🤖 | status handler | ☐P ☐F ☐G |

**Variant B — your gateway (proves base_url override).** Change the block:
```toml
[tool.fastskill.embedding]
openai_base_url = "<your gateway>/v1"
embedding_model = "<model your gateway serves>"
```
`export OPENAI_API_KEY=<gateway key>`

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 8.8 | `fastskill --skills-dir "$SKILLS_DIR" reindex` | Reindex succeeds **against the gateway** — confirm via gateway logs that the request hit `<gateway>/v1/embeddings`. **This is the real proof the override works.** | 🧑 | embedding.rs | ☐P ☐F ☐G |
| 8.9 | `fastskill --skills-dir "$SKILLS_DIR" search "edit documents" --embedding true` | Semantic results via the gateway, exit 0. | 🧑 | — | ☐P ☐F ☐G |

**Variant C — graceful skip (no provider).** Temporarily unset the key:
| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 8.10 | `OPENAI_API_KEY= fastskill --skills-dir "$SKILLS_DIR" reindex` | "Reindex skipped: … Run 'fastskill doctor' …", **exit 0**. | 🧑 | reindex | ☐P ☐F ☐G |
| 8.11 | `OPENAI_API_KEY= fastskill --skills-dir "$SKILLS_DIR" search "x"` (no `--embedding`) | Falls back to text/fuzzy search, exit 0. | 🧑 | search local | ☐P ☐F ☐G |

### Section 9 — Eval

Uses the **proven suite** in `$SKILLREPO`. Run from inside the skill project so the config
is discovered.

```bash
cd "$SKILLREPO/fastskill"
```

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 9.1 | `fastskill eval validate` | Config + files validate, exit 0. | 🤖 | references/eval.md | ☐P ☐F ☐G |
| 9.2 | `fastskill eval validate --agent codex` | Validates the codex runtime is available. | 🤖 | eval.md | ☐P ☐F ☐G |
| 9.3 | `fastskill eval run --agent codex --output-dir "$SBX/eval-runs"` | Runs the smoke case via codex; a timestamped run dir appears; case passes. | 🧑 | eval.md | ☐P ☐F ☐G |
| 9.4 | `fastskill eval run --agent claude --output-dir "$SBX/eval-runs"` | Same case via claude — **catches agent-agnostic bugs**; passes. | 🧑 | eval.md | ☐P ☐F ☐G |
| 9.5 | `fastskill eval run --all --output-dir "$SBX/eval-runs"` | Runs against all available runtimes (mutually exclusive with `--agent`). | 🧑 | eval.md | ☐P ☐F ☐G |
| 9.6 | `fastskill eval report --run-dir "$SKILLREPO/fastskill/results/2026-04-08T15-47-51Z"` | **Agent-free** — reports the committed baseline artifacts, exit 0. | 🤖 | eval.md | ☐P ☐F ☐G |
| 9.7 | `fastskill eval score --run-dir "$SKILLREPO/fastskill/results/2026-04-08T15-47-51Z"` | Re-scores saved artifacts without re-running the agent. | 🤖 | eval.md | ☐P ☐F ☐G |
| 9.8 | `cd -` | Return. | 🤖 | — | ☐P ☐F ☐G |

### Section 10 — Optimize

Uses `$SBX/optimize.toml` from Section 0.4. **Token-heavy** (invokes agents many times).

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 10.1 | `fastskill optimize run --config "$SBX/optimize.toml"` | Starts a run, creates a run dir under `out_dir`, copies config to `<run-dir>/optimize.toml`, completes 1 epoch, exit 0. | 🧑 | webdocs `optimize/configuration` | ☐P ☐F ☐G |
| 10.2 | `fastskill optimize status --run-dir <the run dir>` *(check `--help` for exact flag)* | Reports run status. | 🤖 | optimize help | ☐P ☐F ☐G |
| 10.3 | `fastskill optimize inspect …` | Shows per-step artifacts. | 🧑 | optimize help | ☐P ☐F ☐G |
| 10.4 | `fastskill optimize export …` | Exports the best skill document from the run. | 🤖 | optimize help | ☐P ☐F ☐G |
| 10.5 | Interrupt a fresh run (Ctrl-C), then `fastskill optimize resume --run-dir <dir>` *(or `optimize run --config … --resume <dir>`)* | Resumes from the interrupted run rather than starting over. | 🧑 | optimize help | ☐P ☐F ☐G |

> Steps 10.2–10.5: confirm the exact flag names against `fastskill optimize <sub> --help`;
> a mismatch between help and the docs is itself a GAP.

### Section 11 — Marketplace

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 11.1 | `fastskill --skills-dir "$SKILLS_DIR" marketplace create --help` | Shows options for the create command. | 🤖 | marketplace help | ☐P ☐F ☐G |
| 11.2 | `fastskill --skills-dir "$SKILLS_DIR" marketplace create <args per help>` | Produces a marketplace artifact (e.g. `marketplace.json`), exit 0. Open it — does it match the documented shape? | 🧑 | webdocs | ☐P ☐F ☐G |

### Section 12 — Misc surface (completion, spec, mcp list)

| # | Command | Expected observable | Mode | Src | Result |
|---|---|---|---|---|---|
| 12.1 | `for s in bash zsh fish powershell pwsh; do fastskill completion $s >/dev/null && echo "$s ok"; done` | Each shell emits a completion stub, exit 0. | 🤖 | completion | ☐P ☐F ☐G |
| 12.2 | `fastskill spec --format json | head` | Valid JSON surface export. | 🤖 | spec | ☐P ☐F ☐G |
| 12.3 | `fastskill spec --format yaml | head` | Valid YAML surface export. | 🤖 | spec | ☐P ☐F ☐G |
| 12.4 | **Doc diff:** compare `/tmp/fs-smoke-spec.md` (from 0.1.4) against `webdocs/cli-reference/`. | Every command/flag in the docs exists in `spec` and vice-versa. **Any command in docs-not-in-spec (or spec-not-in-docs) is a GAP** — list each. | 🧑 | webdocs `cli-reference` | ☐P ☐F ☐G |

---

## Documentation walk (blind gap-hunt)

Do this section **last**, and do it *literally* — open each doc, follow it top-to-bottom
copying commands exactly as written, and record every divergence. This is where the highest-
value doc gaps surface. Don't reason about whether a command "should" work — do exactly what
the doc says and report what happens.

| # | Doc to walk (in `webdocs/`) | What to do | Result |
|---|---|---|---|
| D.1 | `quickstart.mdx` | Follow the quickstart verbatim from a clean state. Note any command that fails, is spelled differently, or produces different output. | ☐P ☐F ☐G |
| D.2 | `installation.mdx` | Follow install instructions for your platform verbatim. | ☐P ☐F ☐G |
| D.3 | `skill-management/` | Walk the add/list/read/remove/update journeys. | ☐P ☐F ☐G |
| D.4 | `registry/` | Walk the registry/repos setup journey. | ☐P ☐F ☐G |
| D.5 | `configuration/` | Walk the config + embedding setup. | ☐P ☐F ☐G |
| D.6 | `optimize/` | Walk the optimize configuration + run journey. | ☐P ☐F ☐G |
| D.7 | `evals-quality/` + `testing/` | Walk the eval setup journey. | ☐P ☐F ☐G |
| D.8 | `troubleshooting.mdx` | Do the documented commands actually produce the described symptoms/fixes? | ☐P ☐F ☐G |

---

## Teardown

```bash
rm -rf "$SBX"
# Also remove any skills you added to the global dir in Section 4.
```

---

## Findings summary (fill in at the end)

| Section | Pass | Fail | Gap | Notable finding |
|---|---|---|---|---|
| 0 Preflight | | | | |
| 1 Discovery | | | | |
| 2 Init | | | | |
| 3 Packages | | | | |
| 4 Global | | | | |
| 5 Serve | | | | |
| 6 MCP | | | | |
| 7 Repos | | | | |
| 8 Analyze/Semantic | | | | |
| 9 Eval | | | | |
| 10 Optimize | | | | |
| 11 Marketplace | | | | |
| 12 Misc | | | | |
| D Doc walk | | | | |

**Severity rubric for findings:**
- **S1 — Blocker:** a documented core workflow can't be completed at all.
- **S2 — Major:** a command fails, or docs describe something that doesn't exist / is materially wrong.
- **S3 — Minor:** confusing message, cosmetic doc drift, unclear help text.

---

## Automation-Gap Backlog (roll-up)

List every step you marked ⚙️ (or any 🤖 step you think is under-covered by the test suite).
These are candidate integration tests. Starter set (extend as you go):

- [ ] MCP `install` config-write shape per target (6.2–6.7) — assert exact file + JSON/TOML key per agent, plus `register` rejection (6.8).
- [ ] MCP `serve` protocol handshake `tools/list` over stdio and http (6.9–6.10) — assert the tool set and that `serve` is excluded.
- [ ] MCP stdio flag-rejection `[E004]` (6.11).
- [ ] Serve write-gating returns 403 (not 404) without `--enable-write` (5.7).
- [ ] Serve `/api` → `/api/v1` 308 redirect (5.9).
- [ ] `reindex` graceful-skip exit 0 with no provider (3.18 / 8.10).
- [ ] `add scope/id` error path without an http-registry (7.12).

---

## Appendix A — Distribution / install channels (optional, per-platform)

Run only the rows for platforms you actually have (needs clean machines/VMs). Each row:
install via the channel, then confirm `fastskill --version` runs and matches the release.

| # | Channel / platform | Command | Expected | Result |
|---|---|---|---|---|
| A.1 | Homebrew — macOS arm64 | `brew install gofastskill/cli/fastskill` | Installs, `fastskill --version` works. | ☐P ☐F ☐G |
| A.2 | Homebrew — macOS Intel | (same) | Works. | ☐P ☐F ☐G |
| A.3 | Homebrew — Linux x86_64 | (same; picks gnu vs musl by glibc) | Works; confirm glibc≥2.38 → gnu else musl. | ☐P ☐F ☐G |
| A.4 | Scoop — Windows x86_64 | `scoop bucket add gofastskill https://github.com/gofastskill/scoop-bucket; scoop install fastskill` | Installs `fastskill.exe`, `--version` works. | ☐P ☐F ☐G |
| A.5 | install.sh — Linux x86_64 | `curl -fsSL https://raw.githubusercontent.com/gofastskill/fastskill/main/scripts/install.sh \| bash` | Installs, works. | ☐P ☐F ☐G |
| A.6 | install.sh — macOS | (same) | 🧑 Note the README bills this "Linux & macOS" — verify what actually happens on macOS. | ☐P ☐F ☐G |
| A.7 | Release archive — each target triple | Download the `.tar.gz`/`.zip` from GitHub Releases, extract, run | Binary runs on the target platform. | ☐P ☐F ☐G |
| A.8 | Cargo from source | `cargo install --path .` (in `fastskill/`) | Builds + installs, `--version` matches HEAD. | ☐P ☐F ☐G |

**Release matrix (targets that ship):** `x86_64-apple-darwin`, `aarch64-apple-darwin`,
`x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `x86_64-pc-windows-msvc`.
(No aarch64-linux, no aarch64-windows, no Docker image.)
