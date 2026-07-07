# Spec 003 — Browser Skill Management

**Status:** PROPOSED
**Date:** 2026-07-07
**Scope:** `fastskill-core` (HTTP handlers, web UI assets), `fastskill-cli` (`serve`)
**Depends on:** [ADR-0003](../docs/adr/0003-serve-trust-boundary-and-edge-auth.md) (WRITE-GATE),
[spec 002](./002-codebase-issues-audit.md) items WRITE-GATE, PARTIAL-1, PARTIAL-3, PARTIAL-10, SEC-7.

---

## Background

`fastskill serve` ships a bundled web UI. Today it is **read-mostly**: it browses installed skills
and the project, and calls `POST /skills/upgrade` and `DELETE /skills/{id}`, but it cannot create or
edit skills, and it cannot add skills from a source. The `create_skill` / `update_skill` handlers
exist only as stubs that return 500 (spec 002 PARTIAL-1).

Product decision (2026-07-07): make the browser a **first-class, easy way for a local user to manage
their own skills** — create, edit, add-from-any-source, delete, upgrade — mirroring what the CLI can
do. This fits `serve`'s local-first, single-user identity (ADR-0003): the same operator who runs
`serve` on their machine manages their skills through the browser instead of memorizing CLI verbs.

This is deliberately a **local management** surface, not a multi-tenant authoring platform. It rides
WRITE-GATE, so an exposed/appliance instance stays read-only unless the operator opts in.

---

## Vocabulary (aligned with CONTEXT.md)

- **Skill**, **Manifest**, **Lock**, **Installed skill**, **Version constraint** — as defined in
  `CONTEXT.md`. No new nouns are introduced by this spec.
- **Editable skill** — a skill whose Manifest dependency has `editable = true` (installed from a
  `local` path, in-place). Only editable skills may be edited in the browser (see §4).
- **Source type** — `local` | `git` | `zip-url` | `registry` (the `Repository` types). "Add from
  source" spans all four.

---

## Two distinct write actions (do not conflate)

The UI exposes **two different ways** to get a skill into the project; they map to different backends:

| UI action | What it does | Backend |
|---|---|---|
| **Create** | Author a brand-new skill from scratch (a `SKILL.md`) | `POST /skills` (`create_skill`) |
| **Add from source** | Pull an existing skill from local/git/zip-url/registry | the **install** path (Manifest + resolve + install), not `create_skill` |

Conflating these is the mistake the original stub made (a JSON `POST /skills` cannot carry a
multi-file skill from a source). Keep them separate: **Create authors bytes; Add installs from a
Repository.**

---

## 1. Create (`POST /skills`)

Authors a new **`SKILL.md`-based** skill. Scoped to the common case: most skills *are* just a
`SKILL.md` (frontmatter + body). Multi-file/resource skills are the province of *Add from source*, so
`create` does **not** need resource upload in v1.

- **Request:** `{ name, description, body?, groups?[] }` (frontmatter fields + optional Markdown body).
- **Behavior:** materialize a skill directory under the skills directory with a `SKILL.md`; add it to
  the Manifest as an **`editable` local dependency** (it lives on disk, authored in place); update the
  Lock; register it.
- **Response:** `201 Created` with the new skill's id + metadata. `400` on validation failure;
  `409` if the id already exists.
- **Gating:** write-gated (WRITE-GATE) → `403` when `--enable-write` is off.

Open question O1: id derivation — slug from `name`, or explicit `id` in the request? (Recommend: slug
from `name`, reject on collision, allow explicit override.)

## 2. Edit (`PUT /skills/{id}`)

Edits an existing skill's `SKILL.md` (frontmatter fields + body).

- **Request:** partial `{ name?, description?, body?, groups?[] }`.
- **Constraint — editable-only:** **only `editable` skills may be edited.** A skill installed from a
  `git`/`registry`/`zip-url` source is pinned to that source; editing its `SKILL.md` in place drifts
  it from source-of-truth and the next `install`/reconcile would clobber the edit. For non-editable
  skills the endpoint returns **`409 Conflict`** with a message explaining the skill is
  source-managed (the UI should disable/warn rather than offer edit).
- **Response:** `200 OK` with updated metadata. Write-gated.

## 3. Add from source (install)

The UI's "Add skill" offers all four **source types**, matching the CLI:

- `local` (a folder path — already works), `git` (repo URL, optional `#subdir`), `zip-url` (remote
  zip — **depends on spec 002 PARTIAL-3**, currently unimplemented), `registry` (`scope/id`).
- **Backend:** this is the existing add/install flow — add to the Manifest (`POST /manifest/skills`),
  resolve, install into the skills directory, update the Lock. It is **not** `create_skill`.
- Reuses the security-hardened install path (spec 002 SEC-3 zip caps, SEC-4 symlink reject, SEC-5
  subdir, SEC-6 scope, SEC-11/12 git). Those fixes are prerequisites for exposing add-from-source to
  a browser caller.
- Write-gated.

## 4. Delete / Upgrade (already implemented)

`DELETE /skills/{id}` and `POST /skills/upgrade` already work and are already called by the UI. They
move under the WRITE-GATE like the rest of the write set; no behavior change beyond gating.

## 5. Live refresh (hot-reload)

With **PARTIAL-10** (hot-reload watcher) implemented, edits made outside the browser (or by the
browser) trigger an auto-reindex so the running `serve` reflects changes without a manual reindex.
The UI should also reflect its own writes immediately (optimistic update / refetch after a successful
write), independent of hot-reload.

---

## WRITE-GATE interaction

All of §1–§4's writes are in the WRITE-GATE write set (spec 002). Consequences for the UI:

- When `serve` runs **without** `--enable-write`, every write route returns `403` with the
  "start with `--enable-write`" message. The UI must **detect read-only mode** (e.g. probe `/status`
  or read the 403) and **hide/disable** the create/edit/add/delete/upgrade controls, showing a
  "read-only — start with `--enable-write` to manage skills" banner rather than offering buttons that
  will 403.
- A local user managing skills runs `fastskill serve --enable-write`. This is the intended everyday
  flow and should be documented as such in `serve-command.mdx`.

---

## Security considerations

- **SEC-7 (XSS) is a hard prerequisite.** The moment the UI renders user-authored skill `name`/
  `description`/`body`, unescaped rendering is stored XSS. Spec 002 SEC-7 (HTML-escape) must land
  with or before this feature. Rendering authored **Markdown `body`** additionally needs a
  safe/ sanitizing renderer (no raw HTML passthrough).
- **Untrusted skills are still not vetted.** Adding from a git/registry/zip source runs the same
  content that the CLI would install; per spec 002 SEC-8, fastskill does not sandbox skills. The UI
  must not imply that browser-added skills are "safe."
- **No new network exposure.** This feature adds no auth of its own; it relies entirely on WRITE-GATE
  + the external-sidecar model (ADR-0003). It does not change the trust boundary.

---

## Open questions

- **O1 — id derivation** on create (slug vs explicit; collision handling). Recommend slug-from-name.
- **O2 — Markdown body renderer**: which sanitizing renderer for the `body` preview, and do we render
  Markdown at all in v1 or store/edit raw text only? (Leaning raw-text edit + escaped preview in v1.)
- **O3 — Add-from-source endpoint shape**: reuse `POST /manifest/skills` + a follow-up install call,
  or a single `POST /skills/install` that does add+resolve+install atomically? (Leaning the latter for
  a clean UI action, but it overlaps existing manifest routes — needs a call.)
- **O4 — Resource files on create**: v1 is `SKILL.md`-only. Is a later "upload resources" flow wanted,
  or do resource-bearing skills always come via add-from-source? (Assume the latter unless told.)

---

## Implementation phases

1. **Prerequisites (spec 002):** WRITE-GATE, SEC-7 (XSS escape), and — for add-from-source — SEC-3/4/5/6/11/12
   and PARTIAL-3 (zip-url install). These are audit fixes, tracked in spec 002; this feature builds on them.
2. **Create + Edit endpoints** (`create_skill`/`update_skill`), editable-only constraint, proper status
   codes; behind WRITE-GATE.
3. **UI: manage panel** — read-only-mode detection + banner; Create form; Edit form (editable skills
   only); Delete/Upgrade already wired.
4. **UI: Add-from-source** — the four source types (needs O3 resolved).
5. **Hot-reload live refresh** (PARTIAL-10) — optional polish; the UI refetch-after-write works without it.

---

## Non-goals

- Multi-user authoring, per-user permissions, or any auth beyond WRITE-GATE + external sidecar (ADR-0003).
- Editing skills that are pinned to a remote source (blocked by the editable-only rule).
- Resource/multi-file authoring via `create` in v1 (use add-from-source).
- A REST-CRUD API contract for third-party integrations — this surface exists to back the local UI.
