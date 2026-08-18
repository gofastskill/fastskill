# FastSkill Smoke Test — Known-Issues Triage Key (MAINTAINER ONLY)

> **Do NOT give this file to the tester.** The smoke-test plan is deliberately *blind* so the
> tester discovers doc-vs-implementation gaps without being primed. This file is the triage
> key: it lists gaps we already know about (found during plan authoring, 2026-08) so that when
> findings come back you can separate **genuinely new** issues from **already-known** ones.
>
> When a returned finding matches an entry here → tag it "known", link the entry.
> When it doesn't → it's new signal; that's the whole point of the exercise.

These were surfaced by source-reading the repo while designing the plan; they have **not**
all been reproduced against a released binary. Treat them as "expected to be found", and
confirm/date them as the tester (or an agent) hits them.

---

## Known gaps & rough edges

### K1 — `scripts/install.sh` hard-fails on macOS despite README billing (Distribution / A.6)
- **What:** `install.sh` detects OS/arch but errors `"Currently only Linux x86_64 is supported"`
  on macOS, while the README advertises the script as "Linux & macOS".
- **Source:** `scripts/install.sh:159-161`; README install section.
- **Severity:** S2 (major — documented install path doesn't work on a claimed platform).
- **Expect the tester to hit it at:** Appendix A.6 (and D.2 doc walk on a Mac).

### K2 — Port 8080 collision between `serve` and `mcp serve` (Serve / MCP, S5 / S6)
- **What:** `fastskill serve` defaults to `localhost:8080`; `fastskill mcp serve --transport http`
  *also* defaults to `127.0.0.1:8080`. Running both with defaults collides.
- **Source:** `serve.rs` defaults; `mcp/commands.rs` defaults.
- **Severity:** S3 (minor — UX footgun; plan already warns about it in §5/§6).
- **Note:** This is called out *in the plan* as a setup note (not a blind gap), because it
  would otherwise wedge the tester. If a tester reports it as a bug, it's known.

### K3 — Write routes return 403, not 404, when `--enable-write` is off (Serve, 5.7)
- **What:** Write endpoints are always mounted but gated; without `--enable-write` they return
  **403** with body "write operations disabled; start server with --enable-write".
- **Source:** `http/server.rs:313-339`.
- **Severity:** informational — this is **correct/intended** behavior. Listed so that a tester
  reporting "I expected 404" is triaged as *not a bug*. The plan asserts 403 explicitly.

### K4 — Embedding config is asymmetric: base URL is TOML-only, API key is env-only (Config, S8)
- **What:** `openai_base_url` and `embedding_model` are set **only** in `skill-project.toml`
  `[tool.fastskill.embedding]`; the API key is read **only** from `OPENAI_API_KEY` (no
  `OPENAI_BASE_URL` env override, no TOML key for the secret).
- **Source:** `core/embedding.rs`, `core/manifest.rs:276-278`, `cli/config_file.rs`.
- **Severity:** S3 (minor design friction) — surfaces when pointing at a non-OpenAI gateway
  (Q15 variant B). Not a bug, but a real ergonomic gap worth a decision (add an env override?).
- **Expect the tester to notice at:** §8 variant B, D.5 config walk.

### K5 — Docs may reference commands that don't exist (Doc walk, D.*, 12.4)
- **What:** Prior docs audit (2026-08-03) found `SKILL.md` / webdocs referencing commands that
  are **not** in the CLI: `publish`, `auth`, `package`, and a `registry show` variant, plus a
  non-existent `.fastskill/config.yaml`.
- **Source:** 2026-08-03 fastskill docs-vs-source audit.
- **Severity:** S2 (major doc gap) — this is exactly the class of finding the plan hunts.
- **Expect the tester to hit it at:** 12.4 (spec-vs-docs diff), D.1/D.3/D.4 doc walks.
- **Caveat:** confirm against the *tested released version* — some may already be fixed.

### K6 — `optimize` subcommand flag names need help-vs-docs confirmation (Optimize, S10)
- **What:** No real `optimize.toml` ships in-repo, and the exact flags for `status` / `inspect`
  / `export` / `resume` weren't pinned during authoring. The plan tells the tester to confirm
  each against `--help`.
- **Source:** `cli/commands/skillopt/*`.
- **Severity:** unknown until run — potential S3 doc drift.
- **Expect at:** §10 (10.2–10.5).

### K7 — `repos skills` errors for local repos ("is not an HTTP registry") (Repos, 7.8)
- **What:** `repos skills <local-repo>` is expected to error because catalog listing requires
  an http-registry. This is **intended**; asserted by integration tests.
- **Source:** `tests/cli/repos_integration_tests.rs:131`.
- **Severity:** informational — triage "clear error" reports as not-a-bug; but if the *message*
  is confusing, that's a legit S3.

### K8 — `api.fastskill.io` registry reachability unverified (Repos, network)
- **What:** The documented http-registry `https://api.fastskill.io/index` has real DNS
  (→ Hetzner IP) but did not respond from the authoring sandbox (likely egress block). The plan
  deliberately avoids depending on it (local-repo path, Q8=a).
- **Severity:** N/A for the plan; flagged so that if a tester *does* try the http-registry path
  and it times out, it's a known-inconclusive, not necessarily a product bug.

### K9 — `search` defaults to Remote scope, silently missing indexed local skills (Search)
- **What:** `fastskill search "<query>"` with neither `--local` nor `--repository` defaults to
  **`SearchScope::Remote`**, per the code comment "Default to remote search (even if --remote is
  not explicit)". In a project with skills indexed locally (`.fastskill/index.db`) but no
  registry/repository configured, this means `search` never consults the local index at all —
  it prints `No skills found matching '<query>'` with no warning, no fallback to local, and no
  hint to try `--local`. Confirmed 2026-08-18 against v0.9.176: with two skills indexed
  (`k8s-debug`, `pdf-tools`, real embeddings), `search "why is my container restarting"` →
  "No skills found matching...". `search --local --embedding true "why is my container
  restarting"` → correct semantic ranking, `k8s-debug` (0.504), `pdf-tools` (0.311). The failure
  is silent and misattributable: a user with no registry configured has every reason to assume
  their skills aren't indexed or embeddings are broken, when the actual cause is scope
  defaulting away from the only backend that has data.
- **Source:** `determine_search_scope`, `crates/fastskill-cli/src/commands/search.rs:392-401`
  (comment at line 398); dispatch confirmed at `crates/fastskill-core/src/search/mod.rs:91`
  (`SearchScope::Remote => remote::execute_remote_search(...)`) — Remote scope never touches the
  local index regardless of whether a registry is configured.
- **Severity:** S2 (major) — this is not a doc-vs-implementation gap like K1/K5, it's a
  functional dead end in the single most common zero-config workflow (index locally, search
  locally, no registry ever set up). Falls short of S1 because there's a working workaround
  (`--local`) and no data loss/corruption; still worse than the S3 friction items (K2/K4) because
  the default behavior actively contradicts the tool's most-likely usage pattern and gives zero
  signal that scope — not indexing/embeddings — is the cause.
- **Expect the tester to notice at:** any local-index-only search flow (e.g. §Search / §Embedding
  walk without a registry configured); most likely to surface as a false "search is broken" or
  "embeddings aren't working" report before the tester thinks to check `--local`.

---

## Triage workflow for returned findings

1. For each returned `F`/`G`, scan K1–K9 for a match.
2. **Match** → tag "known-<Kn>", update that entry with "confirmed on <version> <date>".
3. **No match** → new finding. Assign severity (S1/S2/S3), file/track it.
4. Pay special attention to `G`s from the **Doc walk** (§D) and **12.4 spec-diff** — that's the
   richest new-signal source and the reason the plan exists.
5. Feed all ⚙️ automation-gap items into the integration-test backlog (see plan's roll-up).
