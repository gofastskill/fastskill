# `fastskill serve` is read-only by default; deployment security is external

## Status

accepted

## Context & decision

At the time of this decision, `fastskill serve` exposed destructive, state-mutating
endpoints: skill deletion removed directories, the older upgrade handler shelled out to
`fastskill update`, and manifest/reindex/refresh routes rewrote state or spawned work. The
audit in [spec 002](../../specs/002-codebase-issues-audit.md) found these routes carried **no in-app
authentication** (SEC-1, SEC-2). ADR-0005 subsequently moved installation/update orchestration
into core; the exposure boundary here remains applicable.

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
   read endpoints are usable (list/get skills, `search`, `resolve`, `status`, dashboard, registry
   browse, manifest reads). Write routes remain registered but return 403 before dispatch.
   `--enable-write` enables **all** state-changing operations in one
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
server (`fastskill mcp serve`, kept separate from `serve` by design) exports the CLI's commands as
tools — mutating commands included — so it needs the same gate, and now has one: `mcp serve` takes
the same `--enable-write` flag, spelled identically because it means the same thing. Without it the
mutating tools are absent from `tools/list`, and a `tools/call` naming one is refused with JSON-RPC
error `-32005` (`MCP_TOOL_DENIED`) quoting the flag, without dispatching the command. With it they
are listed and dispatched normally.

An earlier revision of this section recorded that MCP exposed no mutating tools and so needed no
gate. That was already false when written: every mutating command was registered with
`register_out`, which exports it as an MCP tool, so an `initialize` + `tools/call fastskill_remove`
over stdio deleted an installed skill. The claim is corrected here rather than dropped so the drift
stays on the record.

Both gates read **one** definition — `fastskill_core::write_ops::WRITE_OPERATIONS` — which names
each mutating operation once and carries its HTTP routes and its command path. `serve` mounts its
write routes from that table and `mcp serve` derives blocked tool names from it. A shared table
avoids divergent gate lists. Registration coverage requires every exported command to have an
explicit classification and prevents an unknown command from becoming a read-only tool.

### Effect classification clarification (2026-09-08)

Every exposed command MUST have an explicit effect classification. When exposed through
HTTP or MCP, commands that write artifacts, edit client configuration, execute agents/scripts,
or invoke evaluation/judging providers MUST require `--enable-write`, just as lifecycle,
indexing, and refresh operations do. This does not add a gate to direct CLI invocation.
An output path outside the skills directory does not make an artifact writer a pure read.
If a command has both read-only and mutating argument variants, its entire MCP tool MUST be
gated until argument-aware enforcement is implemented and tested before dispatch.

Pure reads may return protocol/stdout results and perform their documented read requests;
they MUST NOT use a read-only tool call to dispatch an artifact writer or lifecycle mutation.
Internal diagnostic logging is not a user-requested mutation. The local
[API and MCP consistency PRD](../../specs/api-mcp-lifecycle-parity-prd.md) defines coverage
for artifact producers, evaluations, unknown classifications, and direct denied calls.

Enabling writes authorizes invocation; it MUST NOT bypass dependency, ownership, local-edit,
or integrity checks. HTTP handlers and MCP tools MUST call the same domain operations as the CLI.

## Consequences

- The gate reduces the original SEC-1/SEC-2 exposure when the effect inventory is complete.
  Mutating handlers are inaccessible unless the operator enables writes. External request
  authentication remains the deployment boundary; the gate requires no token machinery.
- **A read-only exposed instance still discloses skill data** (and is subject to SEC-7 dashboard
  XSS). That residual is the operator's call when they expose the port; it is not a mutation risk.
- Introducing default-read-only serving changed the original invocation contract. Server
  references MUST document the flag and the effects it enables; registration coverage MUST keep
  newly added tools classified.
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
