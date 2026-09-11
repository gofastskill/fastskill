# Security Model

FastSkill 0.9.228

Source: https://docs.gofastskill.com/security/model

Release revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d

Documentation revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d



## Overview

FastSkill is a skill **package manager**, not a runtime. It installs, indexes, and serves skill
directories; it does **not** execute skills, so there is no sandbox, no resource governor, and no
per-request audit log to speak of. Its security model is about two things: what the `fastskill server serve`
HTTP surface lets a caller do, and how repository credentials are handled.

## `fastskill server serve` is read-only by default

Per ADR-0003, `fastskill server serve` mounts **only read endpoints** unless you start it with
`--enable-write`. Reads (list/get skills, `POST /api/v1/search`, `POST /api/v1/resolve`,
`GET /api/v1/status`, registry browse, manifest reads, the dashboard) are always available.

Every state-changing endpoint — install, update, delete, reindex, registry refresh, manifest
writes — returns **HTTP 403** when writes are disabled:

```
HTTP 403 Forbidden
{
  "success": false,
  "error": {
    "code": "FORBIDDEN",
    "message": "write operations disabled; start server with --enable-write",
    "details": null
  }
}
```

"Anything that is not a pure read is gated" — reindex and registry refresh are included because they
are side-effecting (disk, network, embedding-API cost), not just because they are destructive. This
is one switch: `--enable-write` enables all mutations at once. See the
[`server serve` reference](/cli-reference/serve-command).

## `server serve` is not a security boundary

FastSkill enforces **no authentication of its own**. There is no token endpoint, and API routes
require no `Authorization` or `x-api-key` header. It handles no identity and no per-user
authorization by design.

* **Run it local-first.** The default bind is `localhost` — a single operator viewing their own
  skills on their own machine.
* **Front it with an authenticating proxy if you expose it.** If the port is reachable on a shared
  or untrusted network, put a reverse proxy or sidecar in front that owns request authentication,
  and ensure the app port is not directly reachable. FastSkill will not police the bind address for
  you, and binding a non-loopback address is the normal, expected configuration behind such a proxy.

Combined with the read-only default, an exposed instance started **without** `--enable-write` cannot
be used to mutate state even before the proxy is considered. Reaching a destructive endpoint
unauthenticated requires the operator to have *both* enabled writes *and* exposed the port with no
fronting proxy — a deliberate double opt-out.

A read-only exposed instance still **discloses skill data** to anyone who reaches it. That is the
operator's call when they open the port; it is a disclosure consideration, not a mutation risk.


## Repository credentials: env-var indirection

HTTP registries can authenticate with a **PAT referenced by environment variable**.
Git sources use system Git credentials. Tokens are not written into the manifest:

```toml
[[tool.fastskill.repositories]]
name = "team-registry"
priority = 0
type = "http-registry"
index_url = "https://registry.example.com/index.json"
auth = { type = "pat", env_var = "TEAM_REGISTRY_TOKEN" }
```

FastSkill reads the token from `TEAM_REGISTRY_TOKEN` at runtime. Commit `skill-project.toml`; keep
the token in your shell profile, CI secrets, or a secrets manager. Never commit a plaintext token.

## Reproducible, verifiable installs

`fastskill project install` resolves the manifest and writes a lockfile (`skills.lock`, or
`global-skills.lock` with `--global`). `fastskill project install --lock` installs the **exact** versions
recorded in the lock, giving reproducible and verifiable installs for CI and locked environments.
Commit both `skill-project.toml` and `skills.lock`.

## What FastSkill does not provide

To set expectations honestly, FastSkill has **no**:

* sandboxed execution environment (it does not run skills),
* resource limits or execution monitoring,
* built-in request audit log,
* in-app authentication, identity, or per-user authorization.

For deployed exposure, those responsibilities live in the surrounding infrastructure (proxy,
network policy, secrets management).

