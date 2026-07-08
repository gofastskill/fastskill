# Spec 003 — Browser Skill Management

**Status:** PROPOSED
**Date:** 2026-07-07
**Scope:** `fastskill-core` (HTTP handlers, web UI assets, a new core install seam), `fastskill-cli` (`serve`)
**Depends on:** [ADR-0003](../docs/adr/0003-serve-trust-boundary-and-edge-auth.md) (WRITE-GATE),
[ADR-0004](../docs/adr/0004-bare-version-is-exact.md) (version constraints, registry-scoped),
[spec 002](./002-codebase-issues-audit.md) items WRITE-GATE, PARTIAL-1, PARTIAL-3, SEC-3/4/5/6/7/11/12.

---

## Background

`fastskill serve` ships a bundled web UI that today is read-mostly (browse + a couple of write pokes).
Product decision (2026-07-07): make the browser an **easy way for a local user to manage their skills
as install-units** — install from a source, update to a newer version, remove — mirroring the CLI's
`add`/`install`/`update`/`remove`. This fits `serve`'s local-first, single-user identity (ADR-0003).

**This is management, not authoring.** A Skill is a *multi-file* directory (`SKILL.md` + scripts /
references / resources), so editing skill *content* through a web form is the wrong tool — that
belongs in an editor on the filesystem. The browser therefore does **not** create-from-a-form or
edit `SKILL.md` fields. It **installs, updates, and removes** whole skills from sources.

Corrected vocabulary (this replaces an earlier draft that read "create"/"edit" as form authoring):

| User-facing action | Means | CLI analogue |
|---|---|---|
| **Install** ("add a skill") | Install a whole skill from a source | `fastskill add <source>` / `install` |
| **Update** | Re-fetch from the recorded source and overwrite | `fastskill update [id]` |
| **Remove** | Delete an installed skill | `fastskill remove` (`DELETE /skills/{id}`) |

---

## Vocabulary (aligned with CONTEXT.md)

- **Skill**, **Manifest**, **Lock**, **Installed skill**, **Version constraint** — as in `CONTEXT.md`.
  No new nouns.
- **Source type** — `local` | `git` | `zip-url` | `registry` (the `Repository` types).

---

## Two resolution models (do not conflate)

A settled insight from grilling, worth stating because the UI must respect it:

1. **Ref/source-based** (`local` / `git` / `zip-url`) — the common case. A skill's identity is its
   *source location + ref* (a folder, a git branch, a URL). Skills here are typically **versionless**
   (`SKILL.md` version is optional and usually absent). There is **no list of versions**; "update"
   means **re-fetch from that source and overwrite**.
2. **Version-based** (`registry`) — the registry maintains a version index (`VersionEntry`), so skills
   carry versions and **version constraints** apply. **ADR-0004** (a bare version = exact pin) is
   scoped to *this* model only; it never governed ref-based sources.

Consequence: **update is versionless by default** (re-fetch). A version *picker* is a registry-only
refinement (only a registry can enumerate versions), not part of the core update operation, and is
out of scope for v1.

---

## 1. Install from source

Install a whole skill from any of the four source types — e.g. point at a local folder, a GitHub
URL, a zip URL, or a `scope/id` registry ref, and install.

- **Endpoint:** `POST /skills/install`, body `{ source_type, source_ref, options? }` (e.g. git
  `subdir`/`ref`, groups, editable). Write-gated.
- **Backend (O3, resolved — lift to core):** the add/install orchestration currently lives in the CLI
  crate (`commands::add::execute_add`) and the HTTP handler can't call up into it. **Lift it into a
  `fastskill-core` service method** (e.g. `service.install_from_source(spec)`) called by *both*
  `execute_add` and this handler — in-process, structured `Result` → HTTP status, **no subprocess,
  no argument-injection**. Do **not** shell out (that would add a second `Command::new(request_input)`
  path, against the SEC-2 direction). Reuses the security-hardened install path (SEC-3 zip caps, SEC-4
  symlink reject, SEC-5 subdir, SEC-6 scope, SEC-11/12 git, and PARTIAL-3 for zip-url).
- **Behavior:** resolve the source → add to the Manifest → install into the skills directory → update
  the Lock → (auto-)reindex. Response `201`/`200` with the installed skill's id + resolved
  ref/version; `400` on a bad source; `409` if already installed.

This **replaces** the stubbed form-based `create_skill` — see spec 002 PARTIAL-1 (**REMOVE** the
`POST /skills` create handler; the install action is a *different* endpoint with a source body).

## 2. Update (re-fetch from source)

Resembles **`fastskill update [id]`**: re-resolve from the recorded source and overwrite the installed
copy; versionless for ref-based sources (§Two resolution models).

- **Endpoint:** refactor the existing `POST /skills/upgrade` (rename to `/skills/update` for parity
  with the CLI verb, keeping `upgrade` as an alias if convenient). Body `{ skill_id? }` — one skill or
  all. Optional `check` / `dry_run` mirroring the CLI. Write-gated.
- **Backend:** call the **already-core** `fastskill_core::core::update::UpdateService` directly (drop
  the current shell-out to the `fastskill update` binary — this also removes the SEC-2 subprocess
  surface for this route). All/one selection, Lock update, and reindex match the CLI's behavior matrix.
- **Version-based sources only:** for a `registry` skill, update follows ADR-0004 — re-pinning to a
  newer version is an explicit act, not a silent widen. (No version picker in v1.)

This is what the earlier draft mislabeled `update_skill` (field editing). Spec 002 PARTIAL-1
**REMOVE**s the `PUT /skills/{id}` field-edit handler; "update" is source re-fetch, not editing.

## 3. Remove

`DELETE /skills/{id}` — already implemented; removes from Manifest + Lock + skills dir. Moves under
WRITE-GATE with the rest. No change beyond gating.

## 4. Browse (read-only)

List/detail views of installed skills, the project, and registry catalogs — all reads, always
available. A skill **detail view may show its `SKILL.md`** as **raw text, HTML-escaped** — **no
Markdown-to-HTML rendering** in v1 (O2). Rendering untrusted skill bodies to HTML is an XSS surface
(add-from-source brings untrusted content), so v1 shows escaped source; a sanitized Markdown preview
is a possible v2 add-on and MUST use a strict allowlist renderer. SEC-7 (escape name/description
everywhere) is required regardless.

---

## WRITE-GATE interaction

All of §1–§3's writes are in the WRITE-GATE write set (spec 002): `POST /skills/install`,
`POST /skills/update` (was `upgrade`), `DELETE /skills/{id}`, plus manifest writes, `/reindex`,
`/registry/refresh`. Consequences:

- Without `--enable-write`, these return `403` ("start with `--enable-write`"). The UI must **detect
  read-only mode** (probe `/status` or read the 403) and **hide/disable** install/update/remove
  controls, showing a "read-only — start with `--enable-write` to manage skills" banner.
- A local user managing skills runs `fastskill serve --enable-write`. Document this in
  `serve-command.mdx` as the everyday management flow.

---

## Security considerations

- **SEC-7 (XSS) is a prerequisite** — the UI renders skill-derived `name`/`description` and (read-only)
  `SKILL.md`; all must be HTML-escaped. No raw-HTML Markdown rendering in v1.
- **Untrusted skills are not vetted** (SEC-8): installing from git/registry/zip runs the same content
  the CLI would; the UI must not imply browser-installed skills are "safe."
- **Install lifted to core, not shelled out** (O3): avoids a request-body-to-subprocess path,
  consistent with the SEC-2 hardening; the update route should likewise stop shelling out.
- **No new trust boundary** — relies entirely on WRITE-GATE + the external-sidecar model (ADR-0003).

---

## Resolved design decisions (from grilling)

- **O1 — moot:** no form-based create, so no id-derivation problem; ids come from the source as today.
- **O2 — resolved:** read-only skill detail shows `SKILL.md` as escaped raw text; no Markdown→HTML in
  v1 (sanitized renderer required if ever added).
- **O3 — resolved:** lift add/install into a `fastskill-core` service method; the handler calls it
  in-process (no shell-out). Update uses the already-core `UpdateService`.
- **O4 — moot:** no create-from-form, so no "resources on create" question; multi-file skills arrive
  whole via install-from-source.
- **O5 — resolved:** update = **re-fetch from source and overwrite**, versionless by default
  (resembles `fastskill update`). Version selection is a registry-only future refinement, not v1.

---

## Implementation phases

1. **Prerequisites (spec 002):** WRITE-GATE, SEC-7 (XSS escaping), the install-path hardening
   (SEC-3/4/5/6/11/12), and PARTIAL-3 (zip-url install). Also **remove** the `create_skill`/`update_skill`
   stubs (PARTIAL-1).
2. **Core install seam:** lift add/install into `fastskill-core` (`install_from_source`); refactor
   `execute_add` to call it; refactor the update route to call `UpdateService` directly (drop shell-out).
3. **Endpoints:** `POST /skills/install`, `POST /skills/update` (+ keep `DELETE`), all write-gated with
   structured results.
4. **UI:** read-only-mode detection + banner; an "Install skill" flow offering the four source types;
   per-skill Update + Remove; read-only detail view (escaped `SKILL.md`).
5. **(v2, optional)** registry version picker; sanitized Markdown preview.

---

## Non-goals

- Form-based skill authoring or `SKILL.md` field editing in the browser (skills are multi-file; author
  in an editor, then install-from-source).
- A version picker for ref-based sources (there are no versions to pick).
- Multi-user authoring / per-user permissions / any auth beyond WRITE-GATE + external sidecar (ADR-0003).
- A general REST-CRUD API for third-party integrations — this surface backs the local UI.
