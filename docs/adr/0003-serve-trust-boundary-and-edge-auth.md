# `fastskill serve` is read-only by default; deployment security is external

## Status

accepted

## Context & decision

`fastskill serve` exposes an HTTP API that includes destructive, state-mutating endpoints
(`DELETE /skills/{id}` runs `remove_dir_all`, `POST /skills/upgrade` shells out to
`fastskill update`, the manifest/reindex/refresh routes rewrite project state or spawn work). The
audit in [spec 002](../../specs/002-codebase-issues-audit.md) found these routes carry **no in-app
authentication** (SEC-1, SEC-2).

`fastskill serve` is **local-first and single-user by default**: an operator runs it on their own
machine for a web UI / REST view over their own skills. A **deployed mode** (container behind an
edge proxy) is supported but *secondary*, and is scoped as a **single-purpose appliance** — one
instance per trust domain (one team / one purpose), **not** a shared multi-user platform.
"Single-tenant" here means *one trust domain per instance*, not "all humans are equal": any caller
that legitimately reaches an instance is authorized to do whatever that instance permits.

We decide two things:

1. **FastSkill is not a security boundary and will not become one.** It handles no tokens, no
   identity, no per-user authorization. In shared/enterprise deployments, request authentication
   and authorization are enforced **entirely externally** — by a sidecar or reverse proxy that
   fronts the port. FastSkill is a lightweight tool; owning auth would contradict that. If a future
   requirement needs multiple distinct users with *different* permissions against one instance,
   that is a different product and this ADR does not cover it.

2. **`serve` is read-only by default; mutation is opt-in via `--enable-write`.** With no flag, only
   read endpoints are mounted (list/get skills, `search`, `resolve`, `status`, dashboard, registry
   browse, manifest reads). `--enable-write` enables **all** state-changing operations in one
   switch: create/update/delete skill, `/skills/upgrade`, manifest writes, `/reindex`, and
   `/registry/refresh`. The rule is "anything that is not a pure read is gated" — reindex and
   refresh are folded in because they are side-effecting (disk, network, embedding-API cost), even
   though they are not destructive in the delete sense. Gated routes return **403** with a plain
   message directing the operator to restart with `--enable-write`.

We explicitly **rejected** gating on the *network* (an `--insecure` flag or a non-loopback bind
check). Binding a non-loopback address is the *normal, required* configuration behind a sidecar —
flagging it as "insecure" cries wolf on the correct setup and says nothing about the real risk. The
real risk is not *where* you bind but *what an arbitrary caller can do once connected*, so the
guard belongs on the capability, not the address. The default bind stays `localhost`; binding
elsewhere needs no special flag.

## Scope

This ADR covers the **exposure model** of mutating operations. It does not discharge the rest of
the audit. The content-handling and logic findings (SEC-3 zip bomb, SEC-4 symlink deref, SEC-5/6
traversal, SEC-7 dashboard XSS, SEC-8 weak content scan, and all correctness bugs) are orthogonal —
they trigger for a legitimate caller, and most run in the **CLI** with no server involved. They are
fixed independently.

The write-gate principle applies to **any** surface that mutates state, not just HTTP. The MCP
server (`fastskill mcp`, kept separate from `serve` by design) currently exposes **no** mutating
tools (`ToolCallingService` returns an empty tool list — see spec 002 PARTIAL-4), so there is no
gate to build there today; if it ever exposes writes, the same read-only-by-default rule applies.

## Consequences

- **SEC-1 and SEC-2 are downgraded.** Destructive endpoints are not mounted unless the operator
  passes `--enable-write`. Reaching them unauthenticated now requires the operator to have *both*
  enabled writes *and* exposed the port with no fronting sidecar — a deliberate double opt-out, not
  a silent default. This is an in-app, verifiable control and needs no token machinery.
- **A read-only exposed instance still discloses skill data** (and is subject to SEC-7 dashboard
  XSS). That residual is the operator's call when they expose the port; it is not a mutation risk.
- The `--enable-write` default-read-only behavior is a **breaking change** to the current `serve`
  surface (today all routes are always mounted) and must be documented in the serve reference,
  which currently only says "use a reverse proxy."
- No `FASTSKILL_API_TOKEN`, no `--insecure`, no bind-address policing — the app stays thin.

## Considered alternatives

- *In-app auth (identity + per-user authZ)* — rejected: fastskill is single-tenant tooling;
  duplicates what the edge proxy already does and adds session/secret-management surface.
- *Optional shared-secret token as an in-app backstop* — considered and rejected: even an optional
  token drags token handling into a tool whose whole value is being lightweight. Shared-deployment
  security is the sidecar's job.
- *Gate on the network (`--insecure` / refuse non-loopback)* — rejected: binding non-loopback is
  the correct behavior behind a sidecar, so a "danger" flag on it is a false signal; it also fails
  to protect a non-loopback bind that *is* legitimately fronted. Gating the capability is both
  safer and more honest.
- *Two-tier write flags (ordinary writes vs. destructive delete/upgrade)* — rejected: a permission
  matrix is more surface than a lightweight tool warrants; one boolean the operator can reason
  about is better.
