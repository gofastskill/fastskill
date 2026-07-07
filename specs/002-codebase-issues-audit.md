# Spec 002 — Codebase Issues Audit

**Status:** PROPOSED
**Date:** 2026-07-07
**Scope:** `fastskill-core`, `fastskill-cli`, `fastskill-evals`
**Type:** Backlog / triage — catalogs security vulnerabilities, correctness bugs, and
partially implemented functionality found in a **targeted audit of the security- and logic-critical
modules** (not a full-codebase sweep — see **Coverage boundary** in the Method note). Each item is a
candidate for its own fix PR; nothing here is implemented yet.

---

## How to read this

Every finding lists: **severity**, **location(s)**, **what's wrong**, a **concrete failure
scenario**, a **Recommended approach** (the real fix, grounded in the actual code — for
partial-impl items this leads with a **Decision**: implement / return 501 / remove / reword), and,
where one exists, an **Interim workaround** (a mitigation an operator or user can apply *today*
without a code change). Findings are grouped:

1. Security vulnerabilities
2. Correctness bugs
3. Partially implemented / stubbed functionality

Line numbers are from the audited tree and may drift; treat the file + symbol as the anchor.

Severity legend: **Critical** (remote/unauth compromise or data loss), **High** (exploitable
or wrong results in common use), **Medium** (exploitable under specific config or misbehaves
on edge input), **Low** (defense-in-depth / cosmetic / narrow impact).

---

## Deployment / exposure model (governs SEC-1 / SEC-2)

Per [ADR-0003](../docs/adr/0003-serve-trust-boundary-and-edge-auth.md), the security model is:

- `fastskill serve` is **local-first single-user by default**; deployed mode is a secondary
  **single-purpose appliance** (one trust domain per instance), never a shared multi-user platform.
- **FastSkill is not a security boundary** and handles no tokens/identity. In shared deployments,
  request auth is enforced **entirely externally** by a sidecar or reverse proxy. That is the
  operator's responsibility — fastskill neither ships nor verifies it.
- **`serve` is read-only by default.** Mutating operations are mounted only when the operator passes
  **`--enable-write`** (see WRITE-GATE below). There is deliberately **no** in-app token and **no**
  network/bind policing — gating is on the *capability*, not the address.

Consequences for this spec:

- **SEC-1 and SEC-2 are Critical as-is** (destructive routes are mounted with no auth in the current
  code) and **downgrade to Low/Medium only once WRITE-GATE ships** — after which destructive
  endpoints are unreachable unless the operator both passes `--enable-write` *and* exposes the port
  with no fronting sidecar (a deliberate double opt-out, not a silent default). The severity headers
  carry both numbers; triage on the as-is Critical until WRITE-GATE lands.
- This model covers the **mutation-exposure** surface only. Every content-handling and logic finding
  (SEC-3–SEC-8, all BUGs) is orthogonal — it triggers for a legitimate caller and most of it runs in
  the **CLI** with no server/proxy in the path. Those are in scope regardless.

---

## 1. Security vulnerabilities

### SEC-1 — HTTP server has no authentication on any endpoint, including destructive ones — **Critical (as-is)** → **Low/Medium** once WRITE-GATE ships (writes unmounted by default; residual Critical only if writes enabled *and* port exposed without a sidecar)

**Where:** `crates/fastskill-core/src/http/server.rs:228-299` (route registration);
handlers in `http/handlers/skills.rs`, `manifest.rs`, `reindex.rs`, `registry.rs`.

Every route — `POST/PUT/DELETE /api/v1/skills`, `POST /api/v1/skills/upgrade`,
`POST/PUT/DELETE /api/v1/manifest/skills`, `POST /api/v1/reindex`,
`POST /api/v1/registry/refresh` — is mounted with **no auth layer**. The builder chain
(`server.rs:343-355`) adds CORS, compression, and a fallback but never an authentication
middleware. Handler bodies carry empty `// Check permissions (write access required)` comments
(`skills.rs:73,102,142`) that were never implemented.

**Failure scenario:** any client that can reach the port can call
`DELETE /api/v1/skills/{id}`, which runs `tokio::fs::remove_dir_all` on the skill directory
(`skills.rs:~223`), or rewrite `skill-project.toml` through the manifest endpoints. Default bind
is `localhost` (safe), but `--host 0.0.0.0` (`commands/serve.rs:36`) is fully supported and
exposes all of this to the network with zero credentials.

**Deployment context:** **as-is this is Critical** — in the current code every destructive route is
mounted with no auth, so a fresh `serve` (or any deploy before WRITE-GATE lands) is fully exposed.
Per [ADR-0003](../docs/adr/0003-serve-trust-boundary-and-edge-auth.md), fastskill is deliberately not
a security boundary; shared-deployment auth is an external sidecar's job. Once WRITE-GATE ships,
destructive routes are unmounted-by-default and this drops to **Low/Medium**, with residual
**Critical** only when the operator enables writes *and* exposes the port with no fronting sidecar.

**Recommended approach:** implement **WRITE-GATE** (see the dedicated section below for the concrete
route-split + flag-threading plan); do **not** add in-app tokens or bind policing. Independently,
remove the misleading empty `// Check permissions` comments so the code doesn't imply an auth check
that will never exist.

**Interim workaround:** run `serve` on `localhost` (the default) and never expose the port without an
authenticating reverse proxy in front; don't enable CORS `allowed_origins` for untrusted sites.

---

### SEC-2 — Unauthenticated remote trigger of `fastskill update` subprocess — **Critical (as-is)** → **Low/Medium** once WRITE-GATE ships (route unmounted by default; residual Critical only if writes enabled *and* port exposed without a sidecar)

**Where:** `crates/fastskill-core/src/http/handlers/skills.rs:241-282`.

`upgrade_skills` shells out to the server's own binary:
`Command::new(current_exe).arg("update").arg(id)`, where `id` is the request-body `skillId`
filtered only for empty/`"all"` (`skills.rs:246-248`). No shell is involved (so not classic
shell injection), but combined with SEC-1 an unauthenticated caller can drive a full
update cycle — git clone / zip download / filesystem writes — on the host, and a crafted
`skillId` is passed straight through as a child-process CLI argument.

**Failure scenario:** `POST /api/v1/skills/upgrade {"skillId":"..."}` makes the server fetch and
install skills from configured sources on demand; a leading-dash `skillId` is forwarded verbatim
as an argument to `fastskill update`.

**Deployment context:** same as SEC-1 — **as-is Critical** (the route is mounted with no auth today);
`/skills/upgrade` is a mutating route, so once WRITE-GATE ships it is unmounted unless the operator
passes `--enable-write`, dropping this to **Low/Medium**, with residual **Critical** only when writes
are enabled *and* the port is exposed without a sidecar.

**Recommended approach:** WRITE-GATE covers the caller side (route unmounted without
`--enable-write`). Independently — and this holds even for an authorized caller — in `upgrade_skills`
(`skills.rs:241-282`) validate `filter_id` before spawning: call
`state.service.skill_manager().list_skills()` (the handler already has `state`) and reject with
`HttpError::BadRequest` if the id is not a known dependency. Then build the command as
`cmd.arg("update"); cmd.arg("--"); cmd.arg(id);` so a validated id can never be read as a flag
(defense in depth; `Command` uses no shell).

**Interim workaround:** run `serve` read-only (don't pass `--enable-write`) so the route is absent;
if writes are needed, keep the port behind an auth proxy that blocks `/skills/upgrade`.

---

### SEC-3 — ZIP bomb / decompression DoS: no size, ratio, or entry-count limit — **High**

**Where:** `crates/fastskill-core/src/storage/zip.rs:48` (loop) and `:134`
(`io::copy(&mut file, &mut outfile)`).

`extract_to_dir` streams every entry to disk with **no** per-file uncompressed cap, total-size
cap, compression-ratio check, or entry-count cap. The two hooks meant to guard this are empty
stubs: `ZipHandler::validate_package` (`zip.rs:16`) and
`ZipValidator::validate_zip_package` (`validation/zip_validator.rs:20`) both `return Ok(())`.
The registry download path additionally buffers the entire archive into memory as a `Vec<u8>`
before writing (`add/sources.rs:~302`).

**Failure scenario:** a 1 MB zip that inflates to hundreds of GB (or contains millions of tiny
entries) is fed via `fastskill add evil.zip` or a registry source → fills the disk / exhausts
inodes → host DoS. This is the only fully remote-triggerable filesystem issue.

**Recommended approach:** add a **single fixed ceiling** (no config knob) enforced inside
`extract_to_dir`, grounded in a measurement of 1311 real skills (package size: p99 1.27 MB, max
5.66 MB; largest single file: max 4.0 MB; file count: max 316 — only one skill exceeds 5 MB, none
exceed 10 MB). Define constants sized comfortably above the whole real corpus while still bounding a
bomb: `MAX_TOTAL_UNCOMPRESSED = 50 MiB` (~9× the largest real skill), `MAX_ENTRY_UNCOMPRESSED = 10 MiB`
(~2.5× the largest real file), `MAX_ENTRIES = 10_000` (~30× the busiest real skill), `MAX_RATIO = 100`.
These are a **DoS ceiling** (protect the host), intentionally distinct from the content-validation
limits that define a *valid* skill (`MAX_CONTENT_SIZE = 512_000` in `context_resolver.rs`, the
`SKILL.md` cap in `file_structure.rs`) — cross-reference those so the two regimes read as related, not
contradictory. Reject before the loop if `archive.len() > MAX_ENTRIES`. Inside the loop, use
zip 0.6's `ZipFile::size()` / `compressed_size()`: reject if declared `size()` exceeds the
per-entry cap or `size()/compressed_size() > MAX_RATIO`. Because the declared `size()` can lie,
also enforce the real budget at the `io::copy` (zip.rs:134) — copy through a running counter (or
`Read::take(remaining_budget)`) and fail with `ServiceError::Validation` when the cumulative
uncompressed total would exceed `MAX_TOTAL_UNCOMPRESSED`. Wire the same entry-count/declared-size
pre-flight into the empty `ZipHandler::validate_package` and `ZipValidator::validate_zip_package`
stubs. Separately, cap the in-memory download in `download_registry_package` (`add/sources.rs:302`)
— `repo_client.download()` returns the whole archive as a `Vec<u8>` in RAM before writing, so
reject early if `zip_data.len()` exceeds a compressed-download cap (~50 MiB).

**Interim workaround:** install only from trusted registries/repos; run installs in a container or
under a filesystem quota / `ulimit -f` so a decompression bomb cannot exhaust the disk.

---

### SEC-4 — Symlinks in git/local skill installs are dereferenced (content exfiltration) — **Medium**

**Where:** `crates/fastskill-cli/src/commands/add/install.rs:216-238` (`copy_dir_recursive`).

`entry.file_type()` does not follow symlinks, so a symlinked file reports neither dir nor
regular-file and falls into the `else` branch, where `tokio::fs::copy` (`:232`) **follows** the
link and copies the target's contents. Git-clone and local-folder installs use this path with no
symlink rejection — unlike zip extraction, which does reject symlink entries.

**Failure scenario:** a malicious git repo or local folder contains `creds -> /etc/passwd` (or
`-> ~/.ssh/id_rsa`); after install the victim's skill-storage copy holds the dereferenced secret
file contents, which can then be re-shared/published. Writes stay inside `dst` (only
`entry.file_name()` is joined), so this is content exfiltration, not an out-of-tree write.

**Recommended approach:** in `copy_dir_recursive` (`add/install.rs:216-238`), the `entry.file_type()`
from `read_dir` does not follow symlinks, so test it — if `ty.is_symlink()` (or
`std::fs::symlink_metadata(&src_path)` reports a link), reject with
`CliError::Validation("refusing to copy symlink: …")` rather than letting it fall into the
`tokio::fs::copy` branch (which follows the link and copies the target's contents). This matches
the zip extractor's existing symlink-rejection stance. Apply the same guard to any sibling
copy-into-storage path.

**Interim workaround:** scan untrusted skill directories with `find <dir> -type l` before
`fastskill add`, and install only from trusted sources.

---

### SEC-5 — Unvalidated `subdir` from manifest escapes the clone directory — **Medium**

**Where:** `crates/fastskill-cli/src/utils/install_utils.rs:108-119`
(`temp_dir.path().join(subdir)`); same shape at `add/sources.rs:196-208`.

`subdir` from `SkillSource::Git { subdir }` is a `PathBuf` taken directly from the (untrusted,
shareable) `skill-project.toml` / lockfile and is `join`ed onto the clone temp dir with no
normalization or containment check. A `..`-laden value escapes the clone; the resulting path is
then passed to `validate_cloned_skill` + `copy_dir_recursive`.

**Failure scenario:** a committed manifest with `subdir = "../../../../home/user/.ssh"` (or any
directory containing a `SKILL.md`) makes the installer read/copy files from outside the cloned
repo into skill storage. (The `subdir` parsed from a GitHub *tree URL* at `utils.rs:129` is safe
because `Url::parse` collapses `..`; the raw manifest field is the unprotected one.)

**Recommended approach:** in both `clone_and_find_skill` (`install_utils.rs:108-119`) and
`clone_and_validate_skill` (`add/sources.rs:196-208`), route `subdir` through the existing
`security::path::validate_path_component` per component (or reuse `safe_join(clone_root, subdir)`
from `security/path.rs:155`, which already splits on `/` and validates each part). Reject absolute
paths and `..`. After joining, canonicalize and assert
`joined.canonicalize()?.starts_with(&temp_dir.canonicalize()?)` before the `.exists()` check, so a
`subdir` escaping the clone (via `..` or a symlink) is rejected with `CliError::InvalidSource`.

**Interim workaround:** clone only from trusted git repos; do not accept untrusted `#subdir=`
fragments / `--subdir` values in manifests you install.

---

### SEC-6 — Unvalidated registry `scope` used in storage path — **Medium**

**Where:** `crates/fastskill-cli/src/commands/add/sources.rs:251-268` (`parse_registry_scope_id`)
and `:391-393` (`.join(&scope).join(...)`).

The `scope` half of a `scope/id` registry reference comes from a raw `find('/')` split with no
`validate_path_component` (contrast `registry_index.rs:53-55`, which validates). It is then
`join`ed into the storage path. The skill-ID-mismatch guard (`:380`) constrains only the `id`
component, not `scope`.

**Failure scenario:** `fastskill add "../evilskill"` → `scope = ".."`, `expected_id = "evilskill"`;
if the registry serves a skill whose validated id is `evilskill`, the storage dir becomes
`skill_storage_path.join("..").join("evilskill")`, writing one directory above the storage root.

**Verification note:** confirmed, with a scope-narrowing correction — because `scope` is everything
*before the first `/`*, it is a single path segment and **cannot** contain a `/`, so the escape is
limited to exactly **one** parent level (e.g. `..`), not arbitrary-depth traversal. Still a real
out-of-root write; the fix is unchanged.

**Recommended approach:** run `scope` through `security::path::validate_path_component` — the same
function used at `registry_index.rs:53-55` — either inside `parse_registry_scope_id` (returning
`CliError::Config` on failure) or immediately before the `.join(&scope)` at `add/sources.rs:391`.
This rejects `..`, `/`, `\`, and absolute components, so a crafted `scope` like `../../etc` cannot
redirect the install outside `skill_storage_path`. Validate `expected_id` the same way for symmetry.

**Interim workaround:** configure only trusted `http-registry` repositories and install skills whose
`scope/id` you control.

---

### SEC-7 — Stored/reflected XSS in the `/dashboard` HTML page — **High**

**Where:** `crates/fastskill-core/src/http/handlers/status.rs:77` (route `/dashboard`,
`server.rs:294`).

Skill `name` and `description` are interpolated into HTML via
`format!("<li><strong>{}</strong> - {}</li>", name, desc)` with no escaping.

**Failure scenario:** a skill whose name/description contains `<script>…</script>` (installable
through the unauthenticated manifest/upgrade path in SEC-1/2, or from any marketplace source)
executes JavaScript in the browser of anyone viewing `/dashboard`.

**Recommended approach:** HTML-escape `name` and `description` before interpolation in
`status::root` (`status.rs:77`). Simplest is a small helper escaping `& < > " '`, applied in the
`.map(...)` closure; or pull in an escaping templating crate (`askama`) / `html-escape` and render
through it. Because the dashboard is an **always-on read route** (served even in read-only mode),
this is the highest-exposure item in the set and must be fixed regardless of WRITE-GATE.

**Interim workaround:** don't expose `/dashboard` publicly (front it with an auth proxy); vet skill
`name`/`description` for HTML/script content before adding.

---

### SEC-8 — Weak, trivially bypassed content-safety scanning — **Medium**

**Where:** `crates/fastskill-core/src/validation/content_safety.rs:36-41`.

The dangerous-pattern check is naive `content.contains(pattern)` against literals like
`"import os"`, `"exec("`, `"sudo"`, `"rm -rf"`. It is trivially bypassed (`exec (` with a space,
`from os import`, `import  os` with a double space, any base64/obfuscation) and simultaneously
prone to false positives (any legitimate Python skill importing `os`/`subprocess`, or a
description mentioning "sudo" gets flagged **Critical**).

**Failure scenario:** a malicious skill evades detection with whitespace variation while a benign
skill is rejected — the check provides little real security and harms usability.

**Recommended approach:** reframe as **advisory** — downgrade the `contains()` matches in
`add_dangerous_pattern_errors` (`content_safety.rs:36-41`) from `ErrorSeverity::Critical` to a
warning (`with_warning`), so they inform without blocking, and document that this is a heuristic,
not a sandbox. If genuine detection is wanted, tokenize per language instead of substring-matching
— only scan files whose extension marks them as scripts, and match at token boundaries (e.g.
`(?m)^\s*import\s+subprocess\b`, `\bsubprocess\.(Popen|call|run)\b`) rather than bare `contains`.

**Behavior change (call out in the PR):** this loosens installs — a skill that trips the pattern
list *fails validation today* and will *install with a warning* after the fix. That is intended (the
gate stopped nothing and blocked legitimate shell-automation skills), but it is a change in
observable behavior, not a silent refactor.

**Required docs deliverable:** state plainly in the security/validation docs that **fastskill does
not vet skill safety** — the pattern scan is an advisory signal, not a sandbox, and untrusted skills
must be run in a sandboxed/containerized environment. Downgrading the severity without this note
would silently weaken a guarantee users may believe they have.

**Interim workaround:** treat all third-party skills as untrusted code — review scripts manually and
run skills in a sandboxed/containerized environment rather than relying on this validator.

---

### SEC-9 — Predictable shared temp-file paths (local race / symlink) — **Low**

**Where:** `crates/fastskill-core/src/http/handlers/registry.rs:126`
(`temp_dir().join("fastskill-sources-temp.toml")`) and
`crates/fastskill-core/src/core/repository/client.rs:129`
(`temp_dir().join("fastskill-repo-{name}")`).

Fixed, world-readable temp paths reused across requests/instances. A local user who pre-creates
or symlinks these paths can influence source resolution or cause cross-request interference.

**Recommended approach:** replace both fixed paths with unique per-operation dirs via `tempfile`
(already a dependency). At `registry.rs:126` use `tempfile::Builder::new().prefix("fastskill-sources-")
.tempdir()?` and build the sources file inside it, keeping the `TempDir` alive for the manager's
lifetime; likewise at `repository/client.rs:129` swap the predictable `fastskill-repo-{name}` path
for a `TempDir`. This also removes the cross-run collision/race.

**Interim workaround:** run on a single-user host, or set `TMPDIR` to a private `0700` per-user dir
so the predictable filenames can't be pre-created or read by others.

---

### SEC-10 — CORS `allow_credentials(true)` with an unguarded origin list — **REFUTED (verification pass)** → config-hygiene nit only

**Where:** `crates/fastskill-core/src/http/server.rs:116-121`.

Original claim: `.allow_credentials(true)` with no guard against a `"*"` entry in `allowed_origins` is
an exploitable CORS foot-gun. **Verification refuted the exploitability.** It is factually true that
nothing rejects `"*"`, but the dangerous tower-http combination is `AllowOrigin::any()` + credentials
(which *panics at construction*), and that variant is never used here. A literal `"*"` string placed
in an `AllowOrigin::list` is compared as an *exact* origin value — it only matches a request whose
`Origin` header is literally `*` (browsers never send that), so it does **not** reflect arbitrary
origins. A misconfigured `allowed_origins = ["*"]` yields non-functional CORS (browsers reject `*` +
credentials), not a credentialed cross-origin bypass.

**Disposition:** not a vulnerability. At most, optionally reject/warn on a `"*"` entry as config
hygiene so the misconfiguration fails loudly instead of silently not working. No security fix needed.

---

### SEC-11 — `git clone` runs with no `--` and no protocol allowlist (transport RCE / local-file access) — **High** *(coverage pass: storage/git.rs)*

**Where:** `crates/fastskill-core/src/storage/git.rs:342-388` (`clone_repository`).

The clone argv is built as `["clone","--depth=1","--quiet",(--branch,ref)?,"--single-branch",
"--no-tags", url, dest]` — **no `--` end-of-options separator** and **no restriction on git transport
protocols** (no `-c protocol.ext.allow=never` / `-c protocol.file.allow=never` / `GIT_ALLOW_PROTOCOL`).
`clone_repository` is a **public, exported** fastskill-core API that does zero URL validation itself.

**Failure scenario:** a caller (or a registry/marketplace-sourced skill URL) passing
`ext::sh -c '<cmd>'` gets **arbitrary command execution** during clone; `file:///…` gives local-path
read/exfil. **Mitigation nuance:** the CLI funnels URLs through `parse_git_url` → `Url::parse`
(`utils.rs:106`), which rejects the space-containing `ext::` form and re-serializes so the URL can't
start with `-` — so the *CLI* path is largely shielded, but **library consumers and `file://` are
not**. Also (Low, same file): the full URL and raw git stderr are logged/emitted (`git.rs:364,395`),
leaking any `user:token@host` credentials embedded in a URL.

**Recommended approach:** in `clone_repository` itself (not just callers), insert `--` before the
positional `url`/`dest`, and pass `-c protocol.ext.allow=never -c protocol.file.allow=never` (plus an
allowlist of `https`/`ssh`/`git` as desired). Redact credentials from the logged URL. Do this in the
core function so it holds regardless of caller.

**Interim workaround:** only clone from trusted `https`/`ssh` URLs; never pass a user-supplied string
to `clone_repository` from library code without your own scheme validation.

---

### SEC-12 — `git checkout` flag injection from a URL-derived branch/ref — **Medium** *(coverage pass: storage/git.rs)*

**Where:** `crates/fastskill-core/src/storage/git.rs:438-462` (`checkout_branch_or_tag`).

`let args = vec!["checkout", ref_name];` puts `ref_name` as a positional with **no `--`**. `ref_name`
is `branch.or(tag)`, and `branch` can derive from an attacker-controllable GitHub tree-URL path
segment (`utils.rs:127`, `path_segments[3]`) — e.g. `github.com/o/r/tree/--foo`. A ref beginning with
`-` is parsed by git as a flag (flag injection into `git checkout`).

**Failure scenario:** a crafted tree URL yields a `--`-prefixed ref that git interprets as an option
rather than a branch name. **Recommended approach:** `vec!["checkout", "--", ref_name]`.
**Interim workaround:** don't install from untrusted GitHub tree URLs.

---

**Confirmed NOT vulnerable** (checked and cleared, recorded to prevent re-investigation):
`serve_index_file` path traversal is properly mitigated (`registry.rs` canonicalize +
`starts_with`); no SSRF — all outbound fetches use server-side config URLs, never request bodies;
TLS verification is on everywhere (no `danger_accept_invalid_*`); auth tokens/API keys are only
placed in outbound `Authorization` headers and are not logged; `SkillId::new` rejects `/` and
non-`[alnum-_]` chars, blocking id-based traversal; `git clone` URLs pass through `Url::parse`,
preventing argument-injection via `-`/`ext::`.

---

### WRITE-GATE — read-only `serve` by default; mutations behind `--enable-write` (from ADR-0003)

This is the single in-app control the security model relies on. It is not a "found vulnerability"
but the app-side commitment that downgrades SEC-1/SEC-2. Deliberately **no** tokens and **no**
bind/network policing — the gate is on the capability.

**Where:** `crates/fastskill-cli/src/commands/serve.rs` (add `--enable-write` flag);
`crates/fastskill-core/src/http/server.rs:228-299` (conditionally mount mutating routes);
handlers behind it in `skills.rs`, `manifest.rs`, `reindex.rs`, `registry.rs`.

**Behavior:**
- Default (`fastskill serve`): mount **read** routes only — list/get skills, `POST /search`,
  `POST /resolve`, `/status`, dashboard, registry browse, manifest reads.
- `--enable-write`: additionally mount **all** state-changing routes in one switch —
  `POST /skills` (create), `PUT`/`DELETE /skills/{id}` (update/delete), `/skills/upgrade`, manifest
  writes (`POST/PUT/DELETE`), `/reindex`, `/registry/refresh`. Rule: "not a pure read → gated"
  (reindex/refresh included because they are side-effecting: disk, network, embedding-API cost).
  `create`/`update` are gated here per PARTIAL-1 (now implement, not remove).
- Gated routes when write is disabled return **403** with a plain message pointing at
  `--enable-write` (discoverable, not a mystery 404).
- Default bind stays `localhost`; binding non-loopback needs no special flag (it is the correct
  configuration behind a sidecar).

**Recommended implementation:** add an `enable-write` `ArgSpec` (`ArgKind::Flag`, default `false`) to
`ServeArgs::command_spec()` + a field in `from_arg_value_map`, thread it through `execute_serve` →
`FastSkillServer` (store on the struct, leave `AppState` untouched) → `serve()`. Split the existing
route builders into read vs write sets; in `serve()` only `.merge()` the write set when the flag is
on. Concretely the **read** set is `list_skills`/`get_skill`, `manifest::get_project`,
`list_manifest_skills`, `search`, `resolve`, `status`, the GET registry + index routes, and the
dashboard/UI fallback; the **write** set is `POST /skills` (create), `PUT`/`DELETE /skills/{id}`,
`/skills/upgrade`, `/reindex` + `/reindex/{id}`, `/registry/refresh`, and the manifest
`add`/`update`/`remove` mutators (`create`/`update` implemented + gated per PARTIAL-1). **Always**
register the write paths and wrap them in an
`axum::middleware::from_fn` (or `from_fn_with_state`) that, when the flag is off, short-circuits with
`(StatusCode::FORBIDDEN, Json(ApiResponse::error("write operations disabled; start server with --enable-write")))`.
Do **not** use conditional `.merge()` to drop the routes — that yields a bare 404/405 and defeats the
discoverability the 403 exists for (see Behavior above). The write paths appearing in the route table
even when disabled (returning 403) is intentional and more honest, not a leak.

**Note:** this is a **breaking change** to the current `serve` surface (today all routes are always
mounted). Update `webdocs/cli-reference/serve-command.mdx`, whose "Authentication Model" section
currently only says "use a reverse proxy" and shows `--host 0.0.0.0` with no mention of write-gating.

---

## 2. Correctness bugs

### BUG-1 — Version-bump wipes `[metadata].id` and `[tool]` sections (data loss) — **High**

**Where:** `crates/fastskill-core/src/core/version_bump.rs:82-124`.

`update_skill_version` deserializes into a local `SkillProjectToml`/`SkillProjectMetadata` that
declares only `version/name/description/author/tags/capabilities/download_url` — **no `id`** and
no `[tool]` — then re-serializes the truncated struct back over the file. Unknown TOML fields are
dropped on the round-trip.

**Failure scenario:** after `fastskill version bump`, the skill's required `[metadata].id` and any
`[tool.fastskill]` config (eval settings, `skills_directory`, etc.) are gone; the skill then fails
`validate_for_context` for a missing id. Silent data loss.

**Recommended approach:** replace the deserialize-into-partial-struct → `toml::to_string_pretty`
round-trip with `toml_edit`. Parse `content` into a `toml_edit::DocumentMut`, set only
`doc["metadata"]["version"] = value(new_version.to_string())` (creating the `metadata` table if
absent), and write the document back. This preserves `id`, `[tool]`, comments, and formatting —
strictly cleaner than the serde-`flatten` alternative, which still silently drops any field not
enumerated and forces you to mirror the whole schema. Keep the "file doesn't exist" branch as-is.

**Interim workaround:** hand-edit the `version` field instead of running `version bump`; or after a
bump, manually re-add the lost `id` and `[tool]` section.

---

### BUG-2 — Caret constraint ignores the semver 0.x rule — **High**

**Where:** `crates/fastskill-core/src/core/version.rs:163-168`.

`Caret` is satisfied when `ver >= base && ver.major == base.major`. Semver treats `^0.2.3` as
`>=0.2.3, <0.3.0` and `^0.0.3` as exactly `0.0.3`. The 0.x special-case is missing.

**Failure scenario:** `foo@^0.2.3` wrongly accepts `0.9.0` — a breaking pre-1.0 change — pulling
an incompatible skill. (Verified: `0.9.0` satisfies because both have major `0`.)

**Recommended approach (covers BUG-2, BUG-3, BUG-4, BUG-5 — one fix):** delete the hand-rolled
`VersionConstraint` enum, `parse`, and `satisfies`, and back the type with the already-present
`semver` crate. Store a `VersionReq`; implement `parse` as `VersionReq::parse(constraint)` (mapping
errors to `VersionError::InvalidConstraint`) and `satisfies` as
`Ok(req.matches(&Version::parse(version)?))`. `VersionReq` handles all four defects correctly:
caret 0.x (`^0.2.3` → `>=0.2.3,<0.3.0`), strict `<` upper bounds, bare single `<`/`>`, and
two-component `^1.2`/`~1.2` — and it is exactly what `get_latest_version` already uses.

**⚠ Migration risk (must preserve) — see [ADR-0004](../docs/adr/0004-bare-version-is-exact.md):**
the current parser treats a bare `"1.2.3"` as **Exact** (equality), and per ADR-0004 that is the
intended product semantics (a bare version is an exact pin, not a range). But
`VersionReq::parse("1.2.3")` applies Cargo caret semantics (`>=1.2.3,<2.0.0`), so the migration
**must** normalize a bare `MAJOR.MINOR.PATCH` (no operator, no comma) to `=MAJOR.MINOR.PATCH` before
handing it to `VersionReq::parse` — and that normalization must not be "cleaned up" later (removing
it silently widens every committed bare pin). Also preserve: empty/`"*"` → `VersionReq::STAR`, comma
ranges, and `>=`/`<=`/`<`/`>`/`^`/`~` (all native). Keep the existing `VersionError` variants so
`?`-sites still compile. Add regression tests for bare-pin equality (ADR-0004) and `^0.x`.

**Interim workaround:** use only the forms the current parser handles correctly — exact `1.2.3`,
`>=x`, `<=x`, or explicit two-sided ranges like `>=1.0.0,<=1.9.9`; avoid `^` on `0.x`, bare `<`/`>`,
and two-component `^1.2`.

---

### BUG-3 — Range upper bound `<` is treated as `<=` — **High**

**Where:** `crates/fastskill-core/src/core/version.rs:112-116` (parse) and `:200-207` (`satisfies`).

A strict `<` upper bound is stored as a bare `Range.max` string with no inclusive/exclusive flag,
and `satisfies` always applies `ver <= max` (`version.rs:207`).

**Failure scenario:** `>=1.0.0,<2.0.0` incorrectly reports `2.0.0` as satisfying. (Verified.)

**Recommended approach:** subsumed by the unified `semver::VersionReq` fix under **BUG-2** (a real
`VersionReq` distinguishes `<` from `<=` natively). **Interim workaround:** spell the upper bound as
`<=` with the exact predecessor version.

---

### BUG-4 — Bare strict `<` / `>` single constraints fail to parse — **Medium**

**Where:** `crates/fastskill-core/src/core/version.rs` `parse` (~`:100-147`).

`<2.0.0` or `>1.0.0` (no comma) match none of the `^ ~ >= <=` branches, fall through to
`VersionReq::parse` (which succeeds), but the resulting `req_str` matches none of the
re-conversion prefixes, so `parse` returns `InvalidConstraint` (`version.rs:147`).

**Failure scenario:** `fastskill add foo@>1.0.0` errors out as an invalid constraint.

**Recommended approach:** subsumed by the unified `semver::VersionReq` fix under **BUG-2** (bare
`<`/`>` parse natively). **Interim workaround:** rephrase as a two-sided comma range, e.g.
`>=1.0.1,<=9.9.9` instead of `>1.0.0`.

---

### BUG-5 — Two-component caret/tilde constraints always error at match time — **Medium**

**Where:** `crates/fastskill-core/src/core/version.rs:132-143` (parse) and `:164,171` (satisfies).

`^1.2` / `~1.2` are stored as `Caret("1.2")` / `Tilde("1.2")`, but `satisfies` then calls
`Version::parse("1.2")`, which fails (needs three components), yielding `ParseError` on every
check.

**Failure scenario:** the common constraint `^1.2` never matches anything and surfaces a parse
error.

**Recommended approach:** subsumed by the unified `semver::VersionReq` fix under **BUG-2**
(`^1.2`/`~1.2` are valid `VersionReq` inputs). **Interim workaround:** write the third component,
e.g. `^1.2.0`.

---

### BUG-6 — `topological_sort` can underflow/panic on duplicate `add_skill` — **Low (latent — not currently reachable)** *(was Medium; downgraded in verification)*

**Where:** `crates/fastskill-core/src/core/dependencies.rs:82-93` (`add_skill`) and `:212`
(Kahn loop `*degree -= 1`).

`add_skill` overwrites the forward graph but **appends** to `reverse_graph`, so re-adding an
existing skill leaves duplicate reverse edges while `in_degree` is computed from the deduped
forward graph. The decrement can then run more times than the initial degree, underflowing
`usize` (debug panic / release wrap).

**Failure scenario:** rebuilding a graph entry for an existing skill panics during install-order
computation.

**Verification note (reachability corrected):** the code defect is real, but the "panics during
install-order computation" scenario is **not currently reachable** — `DependencyGraph::build_graph`
has **no production callers**, `add_skill` is only called by `build_graph` and by unit tests (none of
which add a duplicate id), and the `topological_sort` used in production is a *different* struct
(`DependencyResolver`, see PARTIAL-8). So this is a latent defect that only bites a future caller that
passes a duplicate `skill_id`. Fix it (cheap) as hardening, but it is not an active bug — hence the
downgrade to Low. (`DependencyGraph` being entirely unused in production is itself worth a dead-code
review.)

**Recommended approach:** in `add_skill`, before the forward-graph `insert` overwrites an existing
entry, remove `skill_id` from each old dependency's `reverse_graph[dep]` vec, then insert and
rebuild the reverse edges (de-duping per target). As defense, change `*degree -= 1` (dependencies.rs:212)
to `*degree = degree.saturating_sub(1)` so a stray duplicate edge can't panic via usize underflow.

**Interim workaround:** build the graph once via `DependencyGraph::build_graph` from a de-duplicated
skill list; never call `add_skill` twice for the same id.

---

### BUG-7 — Git-diff skill detection uses naive string prefix — **Medium**

**Where:** `crates/fastskill-core/src/core/change_detection.rs:42,47`.

`path_str.starts_with(&skills_dir_str)` is a raw string prefix; the `.or_else` fallback strips the
prefix without the trailing `/`. With `skills_dir = "skills"`, a changed file under
`skills-extra/foo` passes and is parsed as skill id `-extra`. On Windows, backslash-vs-slash means
the prefix never matches at all.

**Failure scenario:** unrelated sibling directories sharing a prefix get flagged as changed
skills.

**Recommended approach:** use `Path::new(path_str).strip_prefix(skills_dir)` (which strips on whole
path components, not raw bytes) and take the first remaining `Component` as the skill id, skipping
the line when `strip_prefix` returns `Err`. This also removes the fragile manual `/`-trimming
`.or_else` chain.

**Interim workaround:** avoid sibling directories that share the skills-dir name as a prefix (e.g. no
`skills-archive/` next to `skills/`).

---

### BUG-8 — Atomic-write truncate-on-open races ahead of the advisory lock (corruption) — **Medium**

**Where:** `crates/fastskill-core/src/utils.rs:36-64` (the shared `atomic_write` helper).

**Corrected mechanism (an earlier draft of this finding described it wrongly):** `tmp_path` is
*deterministic* (`path + ".tmp"`), shared by all writers to the same target, and
`try_lock_exclusive` *does* mutually exclude — so "the lock is on an unstable path" is **not** the
bug. The real defect is that `OpenOptions::truncate(true).open(&tmp_path)` truncates the tmp file
**at open time, before the lock is checked**. fs2 advisory locks are cooperative, so a second writer
arriving while the first holds the lock still truncates the first writer's tmp (via its own
`open(truncate)`) — and with `unlock()` happening *before* `fs::rename()` (utils.rs:61 then :64),
the first writer can then rename a truncated/empty file over the target.

**Failure scenario:** two processes running `save_to_file` on the same `skills.lock` concurrently:
writer B's `open(truncate)` blows away writer A's synced tmp between A's `sync_all` and A's `rename`,
so A publishes an empty/corrupt lock.

**Recommended approach:** drop the advisory lock and use a **per-writer unique tmp** (random suffix)
+ atomic rename — the standard robust pattern (`tempfile::NamedTempFile::persist`). Each writer owns
its tmp, so no writer can truncate another's; `fs::rename` (atomic on POSIX) then publishes a
*complete* file. This yields clean **last-writer-wins** semantics — correct for a byte-level writer,
since the file is always some writer's full content, never truncated. Do **not** try to fix the
existing lock: getting fs2 flock right (lock before open, no truncate-race, hold across rename,
cross-platform) is fiddly and buys only a fail-fast "another writer active" error, which is of
dubious value. If true concurrent-writer coordination is ever needed (e.g. two `install` runs), put
a project-level lock around the whole resolve+write, not inside `atomic_write`.

**Interim workaround:** serialize `fastskill` invocations that mutate the same lock/registry file;
don't run two writers against the same target concurrently.

---

### BUG-9 — Reconciliation ignores the project version constraint — **Low**

**Where:** `crates/fastskill-core/src/core/reconciliation.rs:83,95-105`.

A skill present in the project but absent from the lock is unconditionally `Ok`, and the
`project_deps` constraint is never evaluated; `missing` entries always emit `version: None`
despite the constraint being available.

**Failure scenario:** an installed skill that violates its declared project constraint reconciles
as `Ok`.

**Recommended approach:** in the `is_in_project` branch of `build_reconciliation_report`, when a
constraint string is present (`project_deps.get(&skill_id)`), parse it and call `.satisfies(&skill.version)`
(using the BUG-2 `VersionReq`-backed type); if it doesn't satisfy, mark
`ReconciliationStatus::Mismatch` and record a `VersionMismatch`. Keep the lock-equality check as an
additional signal, but let the constraint check drive `Mismatch`.

**Interim workaround:** keep `skills.lock` regenerated in sync with the project constraints, and
manually verify installed versions against declared ranges.

---

### BUG-10 — String version sort picks the wrong version — **Medium** *(was Low; escalated in verification)*

**Where:** `crates/fastskill-core/src/core/registry/client.rs:154-164` (`get_versions`, `// TODO: Use
semver`) **and — found in verification — `crates/fastskill-cli/src/commands/add/sources.rs:271-294`
(`resolve_registry_version`).**

Reverse **string** comparison orders multi-digit versions wrong (`"1.9.0"` sorts newer than
`"1.10.0"`). `get_versions` alone would be display-only (`get_latest_version` re-sorts with `semver`).
**But verification found a second site that is *not* cosmetic:** `resolve_registry_version` does its
own `sorted.sort()` (lexical) then `.last()` to choose the version **to install** — so with versions
`1.9.0` and `1.10.0` present it installs `1.9.0`, the wrong one. That is a functional resolution bug,
which is why this is Medium, not Low.

**Recommended approach:** fix **both** sites to sort by `semver::Version` (mirroring
`get_latest_version`), e.g. `sort_by(|a, b| Version::parse(b).ok().cmp(&Version::parse(a).ok()))`,
unparseable strings lowest; remove the stale `TODO`. **Interim workaround:** pin the exact version in
the manifest (per ADR-0004, a bare version is an exact match) so resolution doesn't rank a candidate
set.

---

### BUG-11 — Directory-only zip entries written as empty files — **Low (cosmetic)**

**Where:** `crates/fastskill-core/src/storage/zip.rs:88`.

`normalized_entry_name.ends_with("/")` is always `false` after `PathBuf` normalization (no
trailing-slash component), so the directory branch (`:99-107`) is dead and directory-only entries
are created as empty files. No security impact.

**Recommended approach:** determine directory-ness from the source before normalizing — use the zip
crate's `ZipFile::is_dir()`, or check the raw `entry_name.ends_with('/')` captured at zip.rs:53, and
bind that into `path_is_directory` instead of testing the normalized `PathBuf`. **Interim workaround:**
re-package archives so directories are implied by file paths (no standalone dir entries); parent dirs
are still created when files are written.

---

### BUG-12 — Structured `CloneFailed` error is dead code; clone failures lose URL context — **Low** *(coverage pass: storage/git.rs)*

**Where:** `crates/fastskill-core/src/storage/git.rs:390-398`.

`execute_git_command_with_retry` already returns `Err` on any non-zero exit (git.rs:254/272), so on
the `Ok` path `output.exit_code` is always 0. The `if output.exit_code != 0 { return
Err(CloneFailed{url,stderr}) }` block is therefore **unreachable**, and the structured
`CloneFailed{url,stderr}` error is never produced — clone failures surface as the generic
`"Git command failed: {stderr}"`, losing the URL/structured context. (Related Low: the git-version
cache only stores `Ok`, so a too-old/parse-fail git re-runs `git --version` on every clone —
git.rs:86-136.)

**Recommended approach:** drop the dead exit-code branch and instead map the `Err` from
`execute_git_command_with_retry` into `CloneFailed{url,stderr}` so the structured error actually fires.

---

### BUG-13 — Event history keeps the *oldest* events and silently drops all new ones — **Medium** *(coverage pass: events/event_bus.rs)*

**Where:** `crates/fastskill-core/src/events/event_bus.rs:165-170`.

The history buffer does `history.push(event)` then `if history.len() > max { history.truncate(max) }`.
`Vec::truncate(n)` retains the **first** `n` elements — so once history reaches `max_history_size`
(100), every subsequent push is immediately discarded (the just-appended newest entry is the one
thrown away). `get_event_history()` returns the first 100 events ever published and never reflects
recent activity.

**Failure scenario:** after 100 events, any `optimize`/debug tooling reading history sees a
permanently frozen, stale snapshot. **Recommended approach:** use a `VecDeque` with
`pop_front()` when over capacity (or `history.remove(0)`) — a real ring buffer that drops the oldest.

---

### BUG-14 — Event dispatch holds a read-lock across every handler `await` (deadlock / serialization) — **Medium (suspected)** *(coverage pass: events/event_bus.rs)*

**Where:** `crates/fastskill-core/src/events/event_bus.rs:189-215` (`notify_handlers`).

`notify_handlers` holds `handlers.read().await` for the whole loop while awaiting each
`handler.handle_event(...).await`. Tokio's `RwLock` is write-preferring and non-reentrant, so a
handler body that calls `register_handler`/`unregister_handler` (which take `write().await`) — or a
concurrent registration queued while a long handler awaits — **stalls/deadlocks**. It also serializes
all handler execution under a lock held across arbitrary user async code.

**Failure scenario:** a custom `EventHandler` that (re)subscribes in response to an event deadlocks the
bus. **Recommended approach:** clone the handler list (they are `Arc`s) under a short read-lock, drop
the guard, then await the handlers outside the lock. (Related, lower: a `panic!` in a third-party
handler unwinds the publish task, event_bus.rs:206; and `MetricsEventHandler` grows an unbounded map
on distinct `Custom` event types, :341.)

---

### BUG-15 — Skill-content hash concatenates path+content with no separator (cache collision) — **Low** *(coverage pass: change_detection.rs)*

**Where:** `crates/fastskill-core/src/core/change_detection.rs:146-147` (`calculate_skill_hash`,
consumed by `core/build_cache.rs`).

The hash does `hasher.update(relative_path)` then `hasher.update(content)` with no length prefix or
delimiter, so `("ab","c")` and `("a","bc")` hash identically. A crafted rename+edit can produce a
**stale cache hit** (a changed skill judged unchanged → missed rebuild/reindex).

**Failure scenario:** two files whose (path, content) byte-streams concatenate to the same sequence
collide, so `build_cache` reports "unchanged" for a skill that did change. **Recommended approach:**
hash a length-prefixed / delimited encoding of each field (e.g. update with `path.len()` as fixed
bytes, then path, then `content.len()`, then content). (`build_cache.rs` itself is otherwise clean;
one Low: `save` is a non-atomic `fs::write`, so a crash mid-save loses the cache — surfaced as a parse
error next load, not silent.)

---

## 3. Partially implemented / stubbed functionality

### PARTIAL-1 — REST `create_skill` and `update_skill` return "not implemented" — **High**

**Where:** `crates/fastskill-core/src/http/handlers/skills.rs:98-134,137-166`.

`POST /api/v1/skills` and `PUT /api/v1/skills/{id}` validate the request, build a throwaway JSON
value, then always return `HttpError::InternalServerError("… not yet implemented")`. Two
advertised REST endpoints are non-functional, and they return **500** rather than **501** for an
unimplemented operation. `delete_skill` on the same resource IS fully implemented (see SEC-1).

**Decision: IMPLEMENT — as the write half of a browser-based local skill manager, write-gated.**
(Reversed from an earlier "remove" call after a product decision.) Create/edit skills from the web UI
is a genuinely useful, low-friction way for a local user to manage their own skills, and it fits
serve's local-first identity (ADR-0003). Design constraints so it doesn't repeat the stub's mistakes:

- Scope `create_skill` to authoring a **`SKILL.md`-based** skill (frontmatter + body) — the common
  case, since most skills are just a `SKILL.md`. Multi-file/resource skills come via the
  *add-from-source* (install) flow, not a JSON create; do **not** block create on resource upload.
- `update_skill` edits an existing skill's `SKILL.md`. **Only allow create/edit for `editable`/local
  skills** (the existing `editable` manifest flag) — editing a skill installed from a git/registry
  source drifts from its source-of-truth and the next `install`/reconcile would clobber it, so the UI
  must block or warn for non-editable skills.
- Both sit behind **WRITE-GATE** (`--enable-write`), alongside delete/upgrade — a local user managing
  skills runs `serve --enable-write`; an exposed instance stays read-only.
- Return proper status codes (201/200, 403 when write is disabled), not the current 500.

**Note — this is a feature, not just a bug-fix.** "Expose create/edit + add-from-any-source in the
browser" is real new work (UI + the constraints above) that deserves **its own short spec**, not just
un-stubbing two handlers. The *add-from-source* part ("same source types in the browser":
local/git/zip-url/registry) maps to the **install** path — a separate UI action from
create-from-scratch. Recorded here as the decision to build; the design lives in that spec.

**Interim workaround:** manage skills via the CLI (`fastskill add`/`update`) until the UI ships.

---

### PARTIAL-2 — `eval run` flags `--no-fail`, `--trials`, `--ci`, `--threshold` are unreachable — **High**

**Where:** `crates/fastskill-cli/src/commands/eval/run.rs:56-67,85-159,224-227`.

`RunArgs` documents these four flags and the execute logic fully uses them (`:290-304,564-568,645`),
but `command_spec()` registers none of them and `from_arg_value_map` hardcodes
`no_fail:false, trials:None, ci:false, threshold:None`. They cannot be passed from the CLI, so
CI-mode gating and per-run trial/threshold overrides are dead. (`eval score` registers `--no-fail`
correctly, confirming the omission is accidental.)

**Decision: IMPLEMENT (wire through).** Add each `ArgSpec` in `command_spec()` (`Flag` for
`no-fail`/`ci`, `Option` `U32`/`F64` for `trials`/`threshold`) and read them in `from_arg_value_map`
— `eval score` (`score.rs:75-83,114`) already demonstrates the exact pattern for `no-fail`.
Low-risk; the consuming logic already exists.

**Interim workaround:** none — the flags are unreachable from the CLI until wired (scripts must rely
on defaults).

---

### PARTIAL-3 — ZIP-URL install is unimplemented but the repo type is configurable — **Medium**

**Where:** `crates/fastskill-cli/src/utils/install_utils.rs:293-303`.

`install_from_zip_url` returns `CliError::Config("ZIP URL installation not yet implemented")`,
yet `RepositoryType::ZipUrl` is fully parseable and config-convertible (`config.rs:64,80-82`). A
user can configure a `zip_url` repository that then fails only at install time.

**Decision: IMPLEMENT (product decision — wanted capability).** Note the scope: `zip-url` is a
**remote HTTP source** — a URL hosting skill zip archives (+ a `marketplace.json` catalog), e.g. a
GitHub release asset / S3 / CDN. It is distinct from the `local` source type (a local folder as a
repository), which already works. `RepositoryType::ZipUrl` is first-class everywhere else — the
sources manager already loads `marketplace.json` from a ZipUrl (`manager.rs:161`) — so only the
*install* leg is missing; rejecting the type at parse time would break discovery. Have
`install_from_zip_url` download the archive and route it through the existing safe extractor
(`storage/zip.rs` + `zip_validator.rs`, with the SEC-3 size caps).

**Interim workaround:** install ZipUrl skills out-of-band — download and unzip manually into the
skills dir, or use a `git`/`local` source instead.

---

### PARTIAL-4 — Tool-calling subsystem always reports zero tools — **Medium**

**Where:** `crates/fastskill-core/src/core/tool_calling.rs:26-45`.

`ToolCallingServiceImpl::get_available_tools` always returns `Ok(vec![])`; `ToolResult` /
`AvailableTool` and the trait are scaffolding (`// Add other fields/methods as needed`). The
entire subsystem is a placeholder.

**Decision: REMOVE (dead surface).** Confirmed orphaned: the only references are a crate-doc example
(`lib.rs:44`), the `Service::tool_service()` accessor (`service.rs:427`), and the re-exports
(`lib.rs:75`, `core/mod.rs:110`) — **no code consumes `get_available_tools()`**. It is **not** the MCP
seam: fastskill has no MCP server of its own (no `ServerCapabilities`/`tools/call` anywhere); the
`mcp` command is cli-framework's built-in, which exposes registered *commands* as tools, not this
`ToolCallingService`. So removing it does not touch the `mcp` capability in CONTEXT.md. **Removal
scope:** delete `core/tool_calling.rs`, the `tool_service()` accessor, both re-exports, and the
crate-doc example. Reintroduce behind a concrete spec if skills-as-tools is ever actually built.
**Interim workaround:** n/a (no user-visible behavior today).

---

### PARTIAL-5 — Runtime "discovery" is a hardcoded list, not real detection — **Medium**

**Where:** `crates/fastskill-cli/src/runtime_selector.rs:51,58-64`.

`RUNNABLE_AGENTS` is a static `["codex","claude","gemini","opencode","agent","aikit"]`. `--all`
returns this constant regardless of what is installed, despite docs claiming "runtimes discovered
by aikit." The `EmptyRuntimeSet` error branch (`:78-84`) is therefore dead (the list is never
empty).

**Decision: IMPLEMENT real detection.** aikit-sdk already ships the discovery API this needs —
`get_installed_agents() -> Vec<String>` (probes each binary via `is_agent_available` → `--version`).
The code is already structured for it: `resolve_with_discovery` takes a discovery closure, and only
the public `resolve_runtime_selection` hardcodes `RUNNABLE_AGENTS`. Swap that closure to call
`get_installed_agents()`, which makes `--all` reflect what's installed and makes the `EmptyRuntimeSet`
branch reachable; the `mock_runtimes`/`mock_empty` test seams stay intact.

**Interim workaround:** pass explicit runtime ids (e.g. `--runtime claude`) instead of `--all` so you
only target agents you know are installed.

---

### PARTIAL-6 — `reindex` HTTP endpoints are no-ops returning mock responses — **Medium**

**Where:** `crates/fastskill-core/src/http/handlers/reindex.rs:13-47`.

`POST /reindex` and `POST /reindex/{id}` return mock success without doing anything. Currently
harmless, but they lie to callers (`reindex_all` reports `total_processed: 0`; `reindex_skill`
reports a fake `success_count: 1`).

**Decision: IMPLEMENT, write-gated.** The real path exists and is proven: CLI
`commands::reindex::execute_reindex(service, ReindexArgs{…})`, already used by
`utils/reindex_utils::maybe_auto_reindex`. Call it against `state.service` and return the true
counts. Reindex mutates the embedding index, so it sits behind `--enable-write` (WRITE-GATE); until
wired, return **501** rather than a mock **200**. **Interim workaround:** run `fastskill reindex` from
the CLI instead of the HTTP endpoint.

---

### PARTIAL-7 — `eval` `--format grid` and `--format xml` silently fall back to table — **Low/Medium**

**Where:** `eval/run.rs:601-641`, `eval/score.rs:122,260`, `eval/report.rs:108,124`,
`eval/validate.rs`.

All four eval commands advertise and parse `--format table|json|grid|xml`, but their output code
only branches on `use_json`, so `grid`/`xml` produce identical plain-table output. (Grid/Xml ARE
implemented for `search`/`list`/`read`/`analyze`/`registry` via `output/mod.rs`; the eval
omission is a real gap.)

**Decision: DROP the choices (reject `grid`/`xml` for eval) unless a formatter is actually written.**
Correction from the fix pass: the shared `output/mod.rs` formatters are **not** generic — they take
domain-specific row types (search/list/show results), none of which accept an eval `RunSummary`, so
"route eval through the shared formatter" is not free reuse; it requires writing eval-specific
grid/xml emitters. Cheapest honest fix: make `validate_format_args` reject `grid`/`xml` for eval and
drop them from the help text; implement eval grid/xml only if that output is genuinely wanted.

**Interim workaround:** use `--format json` (fully implemented) for machine-readable eval output;
`table` for humans.

---

### PARTIAL-8 — Dependency resolution / sort naming is misleading — **Low**

**Where:** `crates/fastskill-core/src/core/resolver.rs:264-300`;
`dependency_resolver.rs:125-140,170-174`.

`PackageResolver::resolve_dependencies_recursive` resolves only one level despite its name — it
never fetches resolved skills' transitive deps (real traversal lives in `DependencyResolver`).
`DependencyResolver::topological_sort` (`:170-174`) is a documented no-op passthrough relying on
BFS order. Cycle detection (`:125-140`) conflates diamonds with cycles, emitting a spurious
"Circular dependency detected" warning for shared deps (A→B, A→C, B→D, C→D).

**Decision: RENAME + FIX the false warning (priority is the warning).** Rename
`resolve_dependencies_recursive` → `resolve_dependency_list` (or make it genuinely recurse). Replace
the global-`visited_skills` cycle heuristic with path-based detection: keep a separate in-progress /
ancestor set and only warn on a back-edge to an ancestor, so legitimate diamonds stop emitting
"Circular dependency detected." `topological_sort` (`:170-174`) is an admitted no-op — either delete
it or make it enforce the BFS ordering it claims.

**Interim workaround:** ignore the spurious "circular dependency" warning when your graph has shared
(diamond) dependencies — resolution/dedup itself is correct; only the warning is wrong.

---

### PARTIAL-9 — Misc unfinished spots — **Low**

- `core/manifest.rs:579-580` (the `to_skill_entries` hardcode; `handlers/manifest.rs:205-206` is just
  the `#[serde]` field) — **not a bug (correction):** a bare `"1.0.0"` version-string dependency
  legitimately has no group/editable metadata, so `groups=[]`/`editable=false` is correct (the `Inline`
  arm at :582-588 reads real values). *Approach:* keep the behavior, replace the `// TODO` with a
  comment noting it's intentional.
- `config.rs:161-165` — `create_service_config`'s `_sources_path_override` is a dead parameter.
  *Approach:* either wire it into the skills-dir precedence chain it documents, or remove it from the
  signature and callers. (Low risk either way; removing is simplest.)
- `core/routing.rs:13,24,36,90,118` — **not scaffolding (correction):** `RoutingServiceImpl::find_relevant_skills`
  is fully implemented; the cited lines are only stale `// Add other fields/methods as needed`
  comments. *Approach:* delete the stale comments. (The relevance heuristic itself is a separate,
  deliberate design choice, not an unfinished stub.)
- `validation/standard_validator.rs:203-227` — directory-structure validation false-positives on any
  description mentioning "scripts"/"references". *Approach:* replace the `description.contains(...)`
  heuristic with parsing the frontmatter's actual referenced paths, or drop the check rather than
  emit spurious `InvalidDirectoryStructure` errors.

---

### PARTIAL-10 — Hot-reload is a silent no-op that lies when enabled — **High** *(coverage pass: storage/hot_reload.rs)*

**Where:** `crates/fastskill-core/src/storage/hot_reload.rs` (whole file); wired at
`core/service.rs:344-380`.

The entire hot-reload subsystem is a non-functional stub: `enable_hot_reloading` ignores its `_paths`
argument and returns `Ok(())`, `disable_hot_reloading` is a no-op, and the `notify` crate (declared as
the optional `hot-reload` feature dep) is **never imported or used**. But it is wired into service
init: when a user sets `config.hot_reload.enabled = true`, the service calls `enable_hot_reloading`,
which returns `Ok(())` — so the user believes hot reload is active and skill edits will be picked up,
while **nothing is watched and no reindex ever fires**. Silently inert, no warning.

**Decision: IMPLEMENT (product decision — keep the feature).** Build the `notify` watcher: watch the
skills directory, debounce filesystem events, and drive the existing `execute_reindex` path so a
running `serve` picks up skill edits live (no restart / manual reindex). This is a local-dev
convenience for the `serve` loop. Because it triggers reindex (a write-gated operation), it should
only auto-fire when writes are enabled (WRITE-GATE), or be understood as a local-only capability. Until
the watcher is wired, `enable_hot_reloading` must **not** silently return `Ok(())` — return an
error / warn — so a user who enables it isn't misled that it works.

**Interim workaround:** don't set `hot_reload.enabled = true` (it does nothing); re-run `fastskill
reindex` manually after editing skills.

---

### PARTIAL-11 — skillopt `inspect`/`status` swallow JSON parse errors (corrupt run reads as empty) — **Low** *(coverage pass: skillopt)*

**Where:** `crates/fastskill-cli/src/commands/skillopt/inspect.rs:143-144` and `status.rs:119`.

`inspect` does `serde_json::from_slice(...).unwrap_or(Value::Null)` and
`to_string_pretty(...).unwrap_or_default()`; `status` does
`from_slice(&history_bytes).unwrap_or_default()`. A corrupt `patch.json`/`gate_scores.json`/
`history.json` renders as empty/`null` instead of reporting corruption — and `status` is *inconsistent*
(it correctly surfaces `OPTIMIZE_RUN_DIR_CORRUPT` for `runtime_state.json` at :112 but not for
`history.json`).

**Decision: surface the error.** Map parse failures to the existing corrupt-run error path rather than
`unwrap_or_default`, so a truncated run reports corruption instead of a misleadingly empty view.
(`skillopt/config.rs` was checked and is clean — no parsed-but-unused options, no stubs.)

---

## Suggested triage order

The deployment decision (ADR-0003 — fastskill is not a security boundary; read-only by default)
means SEC-1/SEC-2 no longer lead; the WRITE-GATE that downgrades them does, followed by the
content/logic issues the exposure model does nothing for.

1. **WRITE-GATE** — read-only `serve` by default + `--enable-write`. This is what downgrades
   SEC-1/SEC-2 and is a small, self-contained change. Do first.
2. **SEC-11, SEC-12** — `git clone`/`checkout` argument + protocol hardening (add `--`, disable
   `ext`/`file` transports). SEC-11 is `ext::` RCE for any library consumer of `clone_repository`; the
   CLI is partially shielded but the core fix is cheap and belongs in the function itself.
3. **SEC-3** — the only fully remote-triggerable filesystem DoS; unaffected by the exposure model;
   the guard hooks already exist as empty stubs.
4. **BUG-1, BUG-2, BUG-3, BUG-10** — silent data loss and wrong dependency resolution in normal use
   (BUG-10 now installs the *wrong version* via string sort, not just mis-displays it).
5. **SEC-4, SEC-5, SEC-6, SEC-7, SEC-8** — install-path content handling and dashboard XSS; all
   orthogonal to who is calling, so the exposure model does not cover them.
6. **BUG-13/BUG-14** (event bus: frozen history, lock-across-await deadlock) if the event bus is on a
   live path for you. **PARTIAL-10** (hot-reload) until its watcher is built must at least fail loudly
   instead of silently returning `Ok(())`.
7. **Feature work** decided during triage (own specs, not bug-fixes): **PARTIAL-1** (browser skill
   create/edit, `editable`-only, write-gated), **PARTIAL-3** (install from remote zip-url),
   **PARTIAL-10** (hot-reload watcher → reindex). **Remove** decided: **PARTIAL-4** (`tool_calling`
   stub) and **BUG-6**'s unused `DependencyGraph`. **PARTIAL-6** (reindex endpoints) → implement
   (write-gated) or honest `501`. Remaining Low bugs as capacity allows.

Out of scope by decision (ADR-0003): in-app authentication, API tokens, and bind-address policing.
Shared-deployment request auth is an external sidecar/proxy concern, not fastskill's.

---

## Method note

Findings come from a targeted read of the security- and logic-critical modules (zip/path/fs,
HTTP server + handlers, network clients, version/lock/resolve/reconcile logic, CLI/eval/skillopt),
cross-checked against `grep` sweeps for `todo!/unimplemented!/not implemented/placeholder/FIXME`
and `unwrap()/expect()`. The high-severity items (SEC-1, SEC-2, SEC-3, SEC-4, BUG-1, BUG-2, BUG-3,
SEC-8, PARTIAL-1) were independently re-read and confirmed against source. The `unwrap()/expect()`
density (~526 in non-test code) is dominated by `.unwrap_or(...)` fallbacks and framework-invariant
guards; no panic on untrusted input was found in the audited production paths beyond BUG-6.

### Coverage boundary — what "no finding" means

This was a **targeted** audit, not exhaustive, but the originally-unexamined modules have since been
covered (see the **Verification & coverage pass** below). A module with no entry falls into:

- **Examined and cleared** (a clean bill): `serve_index_file` traversal, SSRF surface, TLS config,
  `SkillId` validation (see *Confirmed NOT vulnerable*); `reindex`/`analysis`/`vector_index` (complete
  and internally consistent); `core/build_cache.rs` and `skillopt/config.rs` (checked clean, minor
  notes folded into BUG-15 / PARTIAL-11).
- **Examined and now carrying findings** (the coverage pass): `storage/git.rs` (SEC-11, SEC-12,
  BUG-12), `storage/hot_reload.rs` (PARTIAL-10), `events/event_bus.rs` (BUG-13, BUG-14),
  `change_detection.rs` hashing (BUG-15), `skillopt/inspect.rs`+`status.rs` (PARTIAL-11).
- **Still not deeply examined** (absence of a finding is not a clean bill): the remaining `skillopt`
  execution path delegates to `aikit_skillopt::train_skill`/`resume_skill` (out of this repo's scope —
  its subprocess/argument surface was not audited here), and modules not named anywhere in this doc.

### Verification & coverage pass (added after the initial audit)

Every not-yet-independently-reverified finding was re-read against source. Outcomes:

- **SEC-10 — REFUTED.** A `"*"` in an `AllowOrigin::list` is inert (exact-match, not wildcard); the
  exploitable combo is `AllowOrigin::any()` + credentials, which isn't used. Demoted to a config nit.
- **BUG-6 — downgraded Medium → Low.** Real code defect, but `DependencyGraph`/`add_skill`/`build_graph`
  have **no production callers**, so the underflow isn't currently reachable (latent only).
- **BUG-10 — escalated Low → Medium.** Found a *second, functional* site (`resolve_registry_version`
  string-sorts to pick the version **to install**), so it's not display-only — it installs the wrong
  version.
- **SEC-6 — corrected:** traversal is limited to one parent level (`scope` can't contain `/`).
- **SEC-7 — cross-check:** a verifier suggested the route was `GET /`; that was over-reach (it read
  only `status.rs`). `server.rs:294` mounts `status::root` at `/dashboard` — original location stands.
- All other SEC-4/5/8/9, BUG-4/5/7/9/11, and PARTIAL-2/3/5/6/7/8/9 were **CONFIRMED** as written
  (only minor line-number corrections, e.g. PARTIAL-9's manifest hardcode is `manifest.rs:579-580`).

Caveat that motivated this pass: two findings (BUG-8's mechanism, SEC-10's exploitability) were wrong
in the first draft — so treat any *un-verified* claim here as provisional until re-read.
