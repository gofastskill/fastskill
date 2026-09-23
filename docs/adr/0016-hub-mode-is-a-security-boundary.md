# ADR-0016: Hub mode is a separate security boundary; the CLI stays identity-free

Status: proposed. Date: 2026-09-23.

Amends: [ADR-0003](0003-serve-trust-boundary-and-edge-auth.md).
Related: [ADR-0010](0010-command-taxonomy.md),
[ADR-0014](0014-skill-identity-comes-from-skill-content.md),
[ADR-0017](0017-managed-profile-and-hub-policy.md), and the
[Hub PRD](../../specs/hub-prd.md).

## Context

ADR-0003 decided that FastSkill "is not a security boundary and will not become one". Authentication
and authorization belong to an external proxy, with one instance per trust domain. It also named the
exception: if many users need different permissions against one instance, "that is a different
product and this ADR does not cover it."

That product is now needed. An organization wants a server that knows who each user is. It
approves skill content per team and org-wide, resolves a Managed profile per user, and serves a
portal with an admin area. An external proxy can't express any of this, because the
authorization decisions depend on FastSkill's own data: publisher scopes, approval states, Presets.

We considered three options:

1. Grow `server serve` itself into a multi-user service.
2. Build the Hub on another platform.
3. Add a separately built Hub mode.

## Decision

1. **Hub mode is a separate build of the same code base.** `fastskill server serve --hub` exists
   only when the binary is built with the `hub` feature. The standard CLI release (install
   script, Homebrew, Scoop) is built without it. A separate `fastskill-hub` release file and
   container image are built with it.
2. **Hub mode is a security boundary.** It authenticates every Hub request in-process with OIDC,
   using `cli-framework-oidc`:
   - bearer tokens for the API and the CLI
   - a cookie session for the portal

   It authorizes each request by role and by publisher scope. Roles come from a configurable
   identity provider groups claim.
3. **Everything else in ADR-0003 still holds.** Without `--hub`, `server serve` and `mcp serve`
   stay local-first, carry no identity, and remain read-only by default behind `--enable-write`.
   `--enable-write` has no meaning for Hub routes, which are gated by role instead.
4. **The CLI stays a client and never becomes a boundary.**
   - It gains `auth login|logout|status|token`, provided by cli-framework's token provider.
   - It keeps credentials in cli-framework's `SecretStore`: the OS keychain where available,
     otherwise a 0600 file. Service logins used in CI are kept only in memory.
   - It enforces the policy the Hub signs for its user (ADR-0017), but it doesn't authorize
     other callers.
5. **The Hub keeps the existing registry index protocol.** It serves the NDJSON registry index
   for the catalog visible to the caller, so an unchanged `http-registry` client with a bearer
   token can install from it. New capabilities (profile, policy, inventory, requests, admin)
   live in a separate, versioned Hub API.

## Consequences

- ADR-0003's status line points here. Its rejection of network-based gating (`--insecure`, bind
  checks) still applies to Hub mode: the guard stays on capability and identity, not address.
- Developer machines don't carry Hub dependencies (database drivers, S3, the embedded portal) or
  their attack surface. CI must build and test the `hub` feature as its own configuration.
- `fastskill-core` must keep Hub-only code behind the feature, so the default build is unchanged.
- Hub mode inherits cli-framework's server stack and OIDC crate. A needed change, the
  configurable groups claim, lands in `cli-framework-oidc` rather than in FastSkill.
- Building the Hub on an external platform was rejected. It would be quicker to start, but it
  would tie a FastSkill product to that platform for good, and the audit and authorization needs
  here are small enough to build directly.
- Growing plain `server serve` into a multi-user service was rejected. It would put an
  identity-bearing server into every developer's install, which contradicts ADR-0003's intent for
  the default path.
