# PRD: FastSkill Hub — managed skill profiles for organizations

Status: draft. Date: 2026-09-23.

This PRD records the design settled in the 2026-09-23 design review. It depends on two ADRs,
proposed alongside it:

- [ADR-0016](../docs/adr/0016-hub-mode-is-a-security-boundary.md): Hub mode. It amends
  [ADR-0003](../docs/adr/0003-serve-trust-boundary-and-edge-auth.md).
- [ADR-0017](../docs/adr/0017-managed-profile-and-hub-policy.md): Managed profile and policy
  model.

It builds on:

- [ADR-0014](../docs/adr/0014-skill-identity-comes-from-skill-content.md): content digests identify skills.
- [ADR-0010](../docs/adr/0010-command-taxonomy.md): command paths.
- [ADR-0005](../docs/adr/0005-install-seam-and-origin-model.md): the install seam.
- The preset vocabulary in [team-skill-presets](../docs/requirements/team-skill-presets.md).

It is aligned with the Rudaia platform (aroff/rudaia, `specs/concept.md` and
`specs/contracts.md`, both 2026-09-23). There, fastskill is **Rudaia Skills**, the pilot
primitive at `skills.rudaia.com`, and it must pass Rudaia Contracts v0.1 C1 to C4 (identity,
resource names, telemetry, HTTP API). The Hub is that pilot. FastSkill stays a general
product: every Rudaia-specific value below (issuers, audience, claim paths, host, product
title) is configuration, so another organization can run the Hub against its own identity
provider.

MUST, MUST NOT, SHOULD and MAY express requirement strength.

## Problem Statement

An organization whose developers use Claude Code, Cursor and other agents has no control over,
and no view of, which skills those agents load.

- **Developers** get skills from anywhere: git URLs, zips, copies from colleagues. They have no
  single place to find the skills their organization recommends. When a team fixes a skill,
  nobody's machine gets the fix until each person reinstalls it.
- **Team leads** have to chase each member to install or update a team setup. This is the
  problem the bundle feature partly addressed, and it still depends on someone running a
  command.
- **Security teams** can't answer basic questions:
  - Which skills are on which machines?
  - Who has the skill that was just found to carry a prompt injection?
  - Can we make sure only reviewed skills are installed?

  A skill is executable instructions plus scripts, so this is a real supply-chain exposure.

FastSkill today can't close this gap. [ADR-0003](../docs/adr/0003-serve-trust-boundary-and-edge-auth.md)
makes `fastskill server serve` a single-trust-domain appliance with no identity. Repository
credentials are environment-variable tokens. No component knows who a user is or what that user
should have.

## Solution

FastSkill gains a **Hub mode**: `fastskill server serve --hub`, built from the same code base
behind a `hub` build feature and shipped as a separate release file and container image. The Hub:

- authenticates people and machines through the organization's OIDC identity provider
- stores an organization-wide **catalog** of approved skill contents
- resolves each user's **Managed profile** from their identity provider groups
- publishes a **signed Hub policy**
- collects machine **inventory reports**

It serves a web **portal**: a catalog for everyone, plus an admin area for curation, approval,
fleet visibility and audit.

The first deployment is Rudaia Skills at `skills.rudaia.com`. It trusts the `rudaia` Keycloak
realm and the Rudaia Agent STS, names its resources with Rudaia resource names (rrn), and
exports OpenTelemetry to the platform Collector.

On each machine the existing CLI stays the only client:

1. The developer (or a device-managed config file) points FastSkill at the Hub and signs in with
   `fastskill auth login`.
2. `fastskill profile sync` runs explicitly and through an agent **SessionStart hook**. Each
   run:
   - installs the user's Required skills plus the Permitted additions they chose
   - makes those skills visible to every configured **Agent target** (Claude Code and Cursor
     first)
   - quarantines skills the policy doesn't allow
   - reports an inventory back to the Hub
3. `project install` and `skill add` refuse skill contents whose digest the Hub hasn't approved,
   so a repository can't be used to get around the policy.
4. A developer who wants a skill that isn't in the catalog runs `fastskill profile request`. The
   request goes to the approval queue.

Enforcement follows the preset principle in
[team-skill-presets](../docs/requirements/team-skill-presets.md) Q4: FastSkill enforces through
its own operations. Hard runtime enforcement belongs to the agent runtime, e.g. managed agent
settings pushed by device management. The Hub documents that setup but doesn't implement it.

Delivery follows the organization's priorities (discovery, then visibility, then governance) in
four milestones:

| Milestone | Scope |
|---|---|
| **M0** | Foundations |
| **M1** | Discovery: catalog, search, portal |
| **M2** | Visibility: profile sync and inventory |
| **M3** | Governance: policy, quarantine, install gate, approvals, audit |

## User Stories

### Developer (end user)

1. As a developer, I want to run `fastskill auth login`, so that FastSkill knows who I am
   without me handling tokens. It uses the browser when one is available and a device code over
   SSH.
2. As a developer, I want `fastskill auth status`, so that I can see which Hub I'm connected to,
   as whom, and when my session expires.
3. As a developer, I want `fastskill auth logout`, so that I can clear my credentials on a
   shared machine.
4. As a developer, I want to browse the organization's catalog in the portal, so that I can find
   skills my organization has approved without asking around.
5. As a developer, I want each skill page to show its description, versions, publisher scope,
   approval state, scorecard and dependents, so that I can judge whether to use it.
6. As a developer, I want `fastskill skill search` to query the Hub catalog, so that I can find
   skills by meaning from the terminal.
7. As a developer, I want search to keep working with plain text matching when no embedding
   provider is configured, so that discovery never depends on an AI key.
8. As a developer, I want search results to include only skills I'm allowed to see, so that
   team-private skills stay private.
9. As a developer, I want to add a Permitted addition to my Managed profile from the portal or the
   CLI, so that I can opt into useful skills without a ticket.
10. As a developer, I want to remove a Permitted addition from my profile, so that I can keep my
    agent's context lean.
11. As a developer, I want `fastskill profile sync` to install my Required skills and chosen
    additions, so that my machine matches my profile in one command.
12. As a developer, I want sync to run automatically when my agent session starts, so that my
    skills are current without me remembering to sync.
13. As a developer, I want the session-start sync to be quiet and fast when nothing changed, so
    that it never slows down starting my agent.
14. As a developer, I want synced skills to appear in both Claude Code and Cursor, so that I get
    the same capabilities in every agent I use.
15. As a developer, I want `fastskill profile status` to show my profile, what's installed, what's
    drifting and what was quarantined, so that I understand my machine's state.
16. As a developer, I want a skill I copied in by hand to be moved to quarantine and never
    deleted, so that I don't lose work when policy disallows it.
17. As a developer, I want `fastskill profile request <skill>` to submit a quarantined or local
    skill for approval, so that I can get a useful skill into the catalog instead of hiding it.
18. As a developer, I want an approved request to be restored by my next sync, so that approval
    takes effect without manual steps.
19. As a developer, I want to be told clearly when a project's skill is refused because its
    content isn't approved, and which skill and digest caused it, so that I can ask for approval
    or fix the project.
20. As a developer, I want FastSkill to keep working offline with the policy cached by my last
    sync, so that planes and Hub outages don't block my work.
21. As a developer, I want a clear message when my cached policy has expired, telling me how to
    refresh it, so that I know why installs are refused.
22. As a developer, I want my installed skills to keep working after the cached policy expires,
    so that an outage never breaks an agent session in progress.
23. As a developer, I want to see which skills were revoked since my last sync and why, so that
    I understand changes to my agent's behavior.
24. As a developer, I want my project's committed `skill-project.toml` and `skills.lock` to keep
    working unchanged, so that the Hub doesn't break reproducible installs.

### Skill author

25. As an author holding the `author` role, I want to keep editable (`-e`) skills in my projects
    without them being quarantined, so that I can iterate on a skill under a managed profile.
26. As an author, I want my editable skills reported to the Hub as unmanaged editable content,
    so that security can see unreviewed content without blocking me.
27. As an author, I want to register a git repository in my team's publisher scope, so that the
    Hub imports new versions of my skills automatically.
28. As an author, I want each imported version to land in the approval queue with its digest,
    diff against the previous version and scorecard, so that reviewers have what they need.
29. As an author, I want an eval suite I ship with my skill to have its scorecard shown on the
    approval page, so that evidence of quality speeds approval.
30. As an author, I want to see my skill's approval state and reviewer comments in the portal, so
    that I know what to fix.
31. As an author, I want to see how many machines have each version of my skill, so that I know
    my change's reach before deprecating a version.

### Team maintainer

32. As a team maintainer, I want to curate my team's Preset (Required skills and Permitted
    additions), so that every team member gets the team setup automatically.
33. As a team maintainer, I want the team Preset linked to an identity provider group, so that
    joining or leaving the group updates the member's profile with no manual step.
34. As a team maintainer, I want to approve versions inside my team's publisher scope, so that my
    team can move without waiting on security for team-only skills.
35. As a team maintainer, I want to propose a team skill for org-wide availability, so that good
    skills spread past my team.
36. As a team maintainer, I want to see drift across my team's machines, so that I know who is
    on an old or modified version.

### Security approver

37. As a security approver, I want an approval queue of content waiting for org-wide
    availability, so that nothing reaches every machine without review.
38. As a security approver, I want each queued item to show its digest, origin, files, scripts,
    diff from the previous version and scorecard, so that I can review efficiently.
39. As a security approver, I want to approve or reject with a comment, so that authors get
    actionable feedback and the decision is recorded.
40. As a security approver, I want to block a skill or specific content digests, so that known-bad
    content is quarantined on every machine at its next sync.
41. As a security approver, I want to attach an advisory to a blocked digest, so that developers
    and admins see why it was revoked.
42. As a security approver, I want to see every machine and user that reported a given digest, so
    that I can gauge exposure when an issue is found.
43. As a security approver, I want to see machines that haven't synced within the policy expiry
    window, so that I know where revocations may not have landed.
44. As a security approver, I want to review incoming skill requests from developers, so that
    shadow skills become visible and reviewable.
45. As a security approver, I want every approval, rejection, block and policy change recorded in
    an append-only audit log, so that I can answer "who allowed this, and when".
46. As a security approver, I want to export the audit log as JSON Lines, so that I can load it
    into our SIEM.
47. As a security approver, I want an org rule restricting which origins projects may install
    from, so that I can go further than digest approval when needed.

### Organization admin

48. As an org admin, I want to connect the Hub to our identity provider with configurable
    issuers, audience and groups claim, so that the `rudaia` realm today and another identity
    provider later work without code changes.
49. As an org admin, I want to map identity provider groups to Hub roles (`user`, `author`,
    `team-maintainer`, `security-approver`, `org-admin`), so that permissions follow our
    directory.
50. As an org admin, I want to curate the org baseline Preset, so that every user gets the
    mandatory skills.
51. As an org admin, I want to configure which Agent targets sync writes to, so that we cover the
    agents we actually use.
52. As an org admin, I want to set the policy expiry window (7 days by default), so that I can
    trade offline tolerance against revocation speed.
53. As an org admin, I want to require a Hub connection on managed machines via a
    device-deployed config file the user can't override, so that joining isn't optional where it
    matters.
54. As an org admin, I want a fleet overview (machines, users, last sync, drift counts,
    quarantine counts), so that I can judge rollout health.
55. As an org admin, I want adoption numbers per skill and per Preset, so that I know what's used.
56. As an org admin, I want embedding and search costs to go through our LLM gateway, so that
    spend is on one bill I already monitor.
57. As an org admin, I want every admin action in the portal to require an admin role from the
    identity provider, so that the admin area is protected by the same identity rules as
    everything else.

### CI pipeline

58. As a CI pipeline, I want to authenticate to the Hub with a service login (client
    credentials), so that `project install --lock` works without a person present.
59. As a CI pipeline, I want the digest gate to apply to project installs in CI, so that
    unapproved content can't enter builds.
60. As a CI pipeline, I want service logins kept in memory and never written to disk, so that
    runner caches don't leak credentials.

### Operator (deployer)

61. As an operator, I want to deploy the Hub as a container backed by Postgres and S3-compatible
    storage, so that it runs like our other services.
62. As an operator, I want to run the Hub on SQLite with a local content store for development
    and small installs, so that trying it needs no infrastructure.
63. As an operator, I want the normal CLI release to exclude Hub code, so that developer machines
    don't carry server dependencies or attack surface.
64. As an operator, I want the Hub to refuse to start on invalid identity provider, storage or
    policy configuration, so that misconfiguration is caught before users see it.
65. As an operator, I want the Hub to serve the existing `http-registry` index protocol, so that
    FastSkill versions without Hub support can still install from it with a token.

### Agent

66. As an agent, I want skills in the folder I already read, so that no agent-side integration
    is needed for skills to appear.
67. As an agent runtime, I want the SessionStart hook to exit successfully and quickly when the
    Hub is unreachable, so that a Hub outage never stops a session from starting.
68. As an agent acting in a Rudaia agent run, I want to read the catalog and download approved
    content with an Agent STS token whose capabilities cover it, so that agents get skills
    without borrowing a person's or a workload's credentials.

### Rudaia platform

69. As a Rudaia platform operator, I want the Hub to pass the Rudaia conformance kit for C1 to
    C4, so that it can be listed in the catalog as the Rudaia Skills primitive.
70. As a Rudaia platform operator, I want the Hub's traces and metrics in the platform Collector
    with the standard Rudaia resource and request attributes, so that Hub requests appear in the
    same dashboards and traces as every other primitive.
71. As another Rudaia primitive, I want every Hub resource to carry its resource name (rrn), so
    that I can reference a skill or a pinned digest without knowing Hub internals.
72. As a developer already signed in with `rudaia login`, I want FastSkill to reuse the shared
    Rudaia token cache, so that I don't sign in twice.

## Implementation Decisions

### Product boundary and packaging

- The Hub is a mode of the existing binary: `fastskill server serve --hub`. It's compiled only
  under a `hub` build feature.
  - The standard CLI release does not include it.
  - A separate `fastskill-hub` release file and container image are built from the same
    workspace.
- **ADR-0016** amends ADR-0003:
  - ADR-0003 still holds unchanged for `server serve` without `--hub` and for the CLI: no
    identity in process, and read-only by default.
  - Hub mode is the "different product" ADR-0003 anticipated. It is a security boundary, and it
    enforces identity and roles in-process.
- **ADR-0017** records the Managed profile, tiers, Hub policy, quarantine and digest gate.
- `server serve` and `mcp serve` keep their current contracts. Remote MCP from the Hub is out of
  scope (see Out of Scope).

### Identity

- Identity uses `cli-framework-oidc`:
  - **Client side:** browser (PKCE) and device-code login, token refresh, and service login
    (client credentials) for CI.
  - **Hub side:** bearer-token validation for the API, and the browser cookie session for the
    portal.
- The CLI registers `cli-framework-oidc` as its token provider, which gives it the
  `auth login|logout|status|token` commands. When the shared Rudaia token cache written by
  `rudaia login` holds a valid token for the Hub's issuer and audience, the CLI uses it instead
  of running its own login (Rudaia C6.2). `auth login` stays for everyone else.
- Stored credentials use cli-framework's `SecretStore`: the OS-keychain backend where available,
  the 0600 file store otherwise.
- **Trusted issuers and audience** (Rudaia C1.1, C1.2):
  - The Hub accepts bearer tokens from a configured list of issuers and rejects every other
    issuer with `401`.
  - The Rudaia deployment trusts exactly the `rudaia` realm
    (`https://auth.faseinfra.net/realms/rudaia`) and the Agent STS (`https://api.rudaia.com`).
  - Every token MUST carry the configured audience, `rudaia:skills` in the Rudaia deployment.
  - Issuers and audience are read from configuration, never from constants.
- **Upstream changes (M0)** land in cli-framework, not in FastSkill:
  - `cli-framework-oidc` validates tokens from several issuers. Rudaia ADR-0004 already commits
    to this change.
  - `cli-framework-oidc` gains a configurable **groups claim path**. Today it reads roles only
    from Keycloak's `realm_access.roles`.
  - The shared Rudaia authorization and resource-name libraries (Rudaia C1.4, C2.6).
- **Two layers of authorization:**
  1. **Platform gate** (Rudaia C1.4, the shared authorization library). A human or workload needs
     a coarse role on the Hub's scope, read from the `rudaia.roles` claim. Proposed role →
     action matrix for the Hub, which feeds the open v0.2 contracts item:
     - `viewer`: read the catalog and the caller's own profile.
     - `developer`: also sync, submit inventory, submit requests, and change their own
       Permitted additions.
     - `admin`: also every admin route.
     - `service`: what the workload's descriptor grants. CI service logins normally get
       catalog reads and downloads.
  2. **Hub governance roles** (`author`, `team-maintainer`, `security-approver`, `org-admin`).
     Each is granted by a configured mapping from the groups claim to a role. These roles refine
     what a caller may do inside the platform gate:
     - `author`: requires `developer`.
     - `team-maintainer` and `security-approver`: require `developer`.
     - `org-admin`: requires `admin`.

     A team maintainer's rights are limited to the publisher scopes owned by the teams they
     maintain. Assigning users to groups manually in the Hub is deferred.
  - **Agent principals** (Agent STS tokens) are allowed only through capabilities of the form
    `skills:<action>:<rrn pattern>`. In v1 the only agent actions are `read` (catalog and index)
    and `download` (content by digest). Profile, inventory, request and admin routes refuse
    agents.
  - A deployment without a Rudaia issuer (another organization's identity provider) turns off
    the platform gate. The governance roles then map directly from groups, with `user` as the
    baseline role.
- **Deployment:** Keycloak `rudaia` realm, with one confidential client for the portal and one
  public client for the CLI. Both have an audience mapper for `rudaia:skills` and a groups
  mapper. Both are provisioned declaratively in the platform GitOps repository, like the realm's
  other clients.

### Managed configuration of the CLI

- The CLI reads a **Managed config file** from a fixed, OS-specific system location.
- It takes precedence over user and project configuration and can't be overridden by
  environment variables or flags.
- It carries:
  - the Hub URL
  - the OIDC issuer and client id
  - `required`: when true, commands that read or change skills refuse to run until a Hub
    session exists
- Without the file, users can opt in with `auth login --hub <url>`, which writes user
  configuration.
- Organization-wide CLI settings are delivered through cli-framework's `config-managed` client,
  with the Hub mounting the matching `config-service` routes. These settings are:
  - Agent targets
  - the policy expiry window
  - whether editable skills are allowed for the `author` role
  - the org origin restriction

  They are separate from the skill-level Hub policy below. `config-service` already provides
  claim-based assignment, inheritance, admin writes and an append-only mutation log. This reuse
  was decided while writing this spec; see Further Notes.

### Hub domain model

- **Organization and tenancy:** one Hub deployment serves one organization. In Rudaia terms
  (ADR-0002), the organization is an **account**, `aroff` today, and the Hub's resources sit in
  one configured **product scope**. Teams are identity provider groups, not Rudaia products.
  Whether team scopes should become product scopes is an open item (Further Notes).
- **Resource names** (Rudaia C2): the Hub keeps its internal ids. At its edges (API responses,
  telemetry, audit exports, capabilities) it renders
  `rrn:skills:<account>:<scope>:<kind>/<id>[@ref]` using the shared cli-framework library.
  - Catalog entries are `skill/<publisher scope>/<name>`, pinned `@<version>` or
    `@sha256:<digest>`. A `@sha256:` reference is verified against content before use, which the
    ADR-0014 digest already guarantees.
  - The other kinds the Hub owns are declared in its `primitive:` block: `skill`, `scorecard`,
    `preset`, `request`, `machine`, `advisory` and `audit-event`.
  - New internal ids are ULIDs.
- **Catalog entry:** a skill identity plus a version, bound to exactly one **content digest**
  (ADR-0014). It also records:
  - publisher scope
  - origin and import provenance
  - approval state
  - optional scorecard
  - optional advisory

  Content is immutable: new content means a new catalog entry.
- **Publisher scope:** the `http-registry` scope (e.g. `platform/…`), owned by exactly one team.
  Team maintainers approve within their own scope.
- **Approval state machine:**
  - For each digest: `imported → team-approved → org-approved`.
  - From any state, content can be `rejected` or `blocked`. `blocked` is terminal for that
    digest and carries an optional advisory.
  - `team-approved` content is visible only to that team's members.
  - Only `org-approved` content can appear in the org baseline or in other teams' Presets.
- **Preset:** reuses the existing glossary term, a named selection of Required skills and
  Permitted additions.
  - The **org baseline Preset** applies to everyone.
  - **Team Presets** apply to members of the linked identity provider group.
- **Managed profile:** the per-user resolution of the org baseline plus every team Preset the
  user belongs to, plus the user's chosen Permitted additions.
  - Resolution is a union.
  - Blocked content wins over everything. Required wins over Permitted.
  - Two Presets requiring different content for the same skill id is an **ownership conflict**
    for that user, reported rather than guessed. The same principle applies to shared skills
    under ADR-0008.
- **Skill request:** content submitted by a user via `profile request`, including its digest and
  files, and queued for approval in the requester's team scope or org-wide.
- **Inventory report:** one document per sync, per machine. It records:
  - user
  - machine id (generated at first login and stored locally)
  - Agent targets
  - installed skills and their digests
  - reconciliation statuses
  - quarantine actions
  - unmanaged editable skills
  - sync time

  It MUST NOT include prompts, conversation content or file contents other than the skill
  contents the user explicitly submits with a request.
- **Audit event:** an append-only record for:
  - imports
  - approvals, rejections and blocks
  - Preset and policy changes
  - role-mapping changes
  - requests
  - quarantine actions reported by machines

  Events are never updated or deleted.

### Hub policy

- The **Hub policy** is the per-user document the CLI enforces. It contains:
  - the Managed profile (Required and chosen Permitted entries, as skill id plus approved
    digest)
  - the set of approved digests visible to the user
  - blocked digests with advisories
  - the org origin restriction
  - issue time and expiry
- The Hub signs it with a Hub key, and the CLI verifies the signature before use.
- The CLI caches the last valid policy. Within its expiry (org setting, default 7 days) it's used
  offline. After expiry:
  - installs and syncs that would add content are refused
  - installed skills are left in place
- A blocked digest in a fresh policy triggers quarantine at the next sync, even if the digest is
  installed.
- Signing individual skill contents is out of scope for v1. The signed digest list gives
  Hub-managed machines the same guarantee.

### Wire contracts

- **Registry index protocol:** unchanged. The Hub serves the existing per-skill NDJSON registry
  index (one line per version, with `cksum` and `yanked`) beneath a `/v1` base path, for the
  catalog visible to the caller, and downloads by digest.
  - `blocked` content is served with `yanked = true`.
  - FastSkill versions without Hub support can use the Hub as an `http-registry` repository with
    a bearer token.
- **Hub API** (Rudaia C4): public routes live under `/v1`, separate from the existing
  `/api/v1` local routes.
  - Hub mode does not mount the local `/api` routes or the local dashboard.
  - Its surfaces are `/v1` (the Hub API, and the registry index beneath it), `/healthz` and
    `/readyz` without auth, and the portal.
  - It serves an OpenAPI 3.1 document at `/v1/openapi.json`.
  - Errors are `application/problem+json` carrying a stable `code` and a `trace_id`.
  - Lists paginate with `limit` and `page_token` and return `next_page_token`. Offset
    pagination is not used.
  - Every resource carries its `rrn`.
  - Request submission and inventory reports accept an `Idempotency-Key`.
  - Every `/v1` route requires a valid bearer token or portal session. No anonymous reads are
    declared (Rudaia C1.6). Missing or invalid tokens get `401`; forbidden actions get `403`.

  Resource groups:
  - **Profile:**
    - read my resolved Managed profile and signed Hub policy (ETag revalidation)
    - add or remove my Permitted additions
  - **Inventory:** submit an inventory report.
  - **Requests:** submit a skill request with its contents; read my requests.
  - **Catalog:** list and read catalog entries, versions, scorecards, advisories and dependents;
    search (semantic with text fallback), filtered by what the caller may see.
  - **Admin:** role-gated routes for:
    - tracked repositories and imports
    - the approval queue and decisions
    - blocks and advisories
    - Presets
    - role mapping
    - fleet and inventory queries
    - audit query and JSON Lines export
- Mutating Hub routes are authorized by role and publisher scope, not by `--enable-write`.
  `--enable-write` keeps its meaning only for the non-Hub local surfaces.

### CLI changes

- **New command namespaces** following ADR-0010:
  - `auth`: `login`, `logout`, `status`, `token`. Supplied by cli-framework.
  - `profile`: `sync`, `status`, `request`, `hook-install`.

  `profile sync` is unrelated to the `sync` command removed by ADR-0001. It doesn't write agent
  metadata files.
- **Agent targets:**
  - Managed skills are installed once into a FastSkill-owned **managed store**.
  - Each Agent target's user-level skills folder (Claude Code and Cursor first) gets one link
    per managed skill.
  - Where links aren't available (e.g. Windows without developer mode), sync falls back to
    verified copies.
  - Today's `--global` store is not what agents read. The managed store MUST therefore be
    exposed through Agent targets and not only through the global scope.
- **`profile sync` steps:**
  1. Refresh the Hub policy. Fall back to the cached valid policy.
  2. Resolve the Managed profile.
  3. Install missing content through the existing install seam (ADR-0005), verifying digests.
  4. Link it into each Agent target.
  5. Quarantine disallowed content in each Agent target folder, and in the current project's
     skills directory when a project is detected.
  6. Report the inventory.
  7. Print a summary.
- Sync is idempotent. A no-op sync performs one conditional policy request and no writes.
- **Disallowed content** means content in a managed folder whose digest is:
  - blocked, or
  - not approved for the user and not an allowed editable skill of an `author`.
- **Quarantine:**
  - Content is moved, never deleted, into a timestamped quarantine folder in the FastSkill data
    directory.
  - A quarantine manifest records the skill id, digest, original path and reason.
  - `profile status` lists quarantined content.
  - `profile request` can submit it.
  - Approval makes the next sync restore it.
- **SessionStart hook:**
  - `profile hook-install` registers a quiet `profile sync` as a session-start hook in each
    Agent target that supports hooks (Claude Code first).
  - The hook MUST always exit successfully and within a short time budget. Failures are
    reported in `profile status` rather than blocking the session.
- **Digest gate:**
  - When a Hub policy is active, `project install`, `skill add` and `skill update` refuse any
    skill whose content digest isn't approved for the user. The check runs before the install
    changes state, and the org origin restriction is applied too.
  - Editable local skills are exempt for users with the `author` role.
  - Without a Hub policy, behavior is unchanged.
- **Reconciliation statuses:** the closed vocabulary gains two policy statuses, emitted only when
  a Hub policy is active:
  - `policy-blocked`: installed content is blocked.
  - `policy-unapproved`: installed content isn't approved for the user.

  `skill list --check` fails on both. This changes a compatibility surface and is recorded in
  ADR-0017 and CONTEXT.md.

### Hub storage

- **Relational store:** SQLite for development and small installs, Postgres for production.
  Queries follow cli-framework's precedent of using the sqlx core and driver crates directly,
  not the facade.
- **Content store:** a content-addressed blob store behind one trait, keyed by digest. It has a
  filesystem backend and an S3-compatible backend.
- **Signing key:** loaded from configuration (in production, from the secret manager).
- **Migrations** run at startup. The Hub refuses to start on a failed migration.

### Import

- Tracked git repositories are registered per publisher scope.
- The Hub fetches them on a schedule and on demand. It computes digests with the same code the
  CLI uses, and creates `imported` catalog entries for new content.
- Direct upload (`skill publish`) is deferred to after v1. It will feed the same approval queue.

### Search

- The Hub keeps one semantic index over catalog entries, using an OpenAI-compatible embedding
  endpoint configured to go through the organization's LLM gateway. In Rudaia that gateway is
  Rudaia Models (LiteLLM). The Hub is a workload holding a long-lived virtual key in OpenBao, as
  Rudaia ADR-0009 describes.
- Plain text search is always available and serves as the fallback.
- Results are filtered by caller visibility before ranking.
- When connected, `skill search` without `--local` queries the Hub.

### Telemetry (Rudaia C3, C5)

- The Hub exports OTLP to the configured Collector through cli-framework's `telemetry`
  feature. In Rudaia that is the platform Collector, and the namespace carries the
  `faseinfra.net/otlp-access=true` label.
- **Resource attributes:**
  - `service.name`: configurable, `rudaia-skills` in the Rudaia deployment
  - `service.version`
  - `rudaia.primitive=skills`
  - `rudaia.contracts.version=0.1`
- **Request attributes:** every server span carries the Rudaia request attributes (account,
  product, env, principal, principal kind, and run for agents), set by the authorization library.
- The Hub propagates W3C `traceparent` on outgoing calls, including embedding calls. The CLI
  sends one with its Hub requests, so a sync appears as one trace.
- **Usage records** (SHOULD): one `rudaia.usage` log record per sync and per content download.
  - Meters: `skills.syncs` and `skills.downloads`, declared in the `primitive:` block.
  - Each record's idempotency key is stable per fact.
- **Inventory reports are not telemetry.** They are Hub data that admins query, stored in the
  Hub and kept to the fields ADR-0017 allows. The OTel pipeline carries the Hub's operational
  signals only, never inventory contents, prompts or skill contents.

### Portal

- **Stack:** one single-page app in React 19, built with Vite and pnpm and embedded in the Hub
  build.
- **Access:** it authenticates through the cookie session from `cli-framework-oidc`. The admin
  area is shown and served only to admin roles, and the server enforces the same checks.
- **Look:** built on the Rudaia design system. In M0, Rudaia gains a component package
  (components, tokens, Tailwind preset) with proper entry points. The Hub consumes it as a pnpm
  git dependency pinned to a commit. The portal is styled with Rudaia, in light and dark themes.
  Its product title is configuration: "FastSkill Hub" by default, and "Rudaia Skills" in the
  Rudaia deployment.
- **End-user pages:**
  - catalog and search
  - skill detail
  - my profile (Required, Permitted additions, add/remove)
  - my machines (last sync, drift, quarantine)
  - my requests
- **Admin pages:**
  - approval queue and review
  - tracked repositories
  - Presets
  - blocks and advisories
  - fleet overview
  - exposure lookup by digest
  - role mapping
  - audit log with export

### Deployment (first deployment: Rudaia Skills)

- The Hub runs as a service in the platform GitOps repository on the Rudaia cluster, backed by
  cluster Postgres and S3-compatible storage.
- Its service descriptor carries the Rudaia `primitive:` block:
  - `name: skills`
  - `title: Rudaia Skills`
  - `contracts: "0.1"`
  - `host: skills.rudaia.com`
  - `openapi: /v1/openapi.json`
  - the kinds listed in the domain model
  - no anonymous reads
  - the two meters
- It's served publicly at `https://skills.rudaia.com` (Rudaia C4.7), through its own Ingress and
  a publicly trusted certificate. Every route except health checks requires identity, so public
  exposure doesn't mean anonymous access.
- Following Rudaia ADR-0008, the deployment's declaration lives in git. The Hub's own state
  (catalog, approvals, Presets, inventory, audit) lives in its database.

### Milestones

- **M0, foundations** (the two ADRs are drafted in this PR):
  - groups-claim and multi-issuer support in `cli-framework-oidc`
  - the shared Rudaia authorization and resource-name libraries in cli-framework, which are
    Rudaia M1 deliverables that the Hub consumes rather than builds
  - the `rudaia` realm clients for the Hub
  - the Rudaia component package
  - ADR-0016 and ADR-0017
  - CONTEXT.md glossary additions: Hub, Managed profile, Hub policy, Agent target, managed
    store, quarantine, publisher scope ownership, inventory report
- **M1, discovery:**
  - Hub mode skeleton and packaging
  - storage
  - OIDC login (CLI and portal)
  - tracked-repository import
  - registry index serving
  - catalog and search API, with the C4 conventions and resource names
  - OTLP telemetry with the Rudaia attributes
  - the portal catalog, skill pages and search
  - deployment at `skills.rudaia.com`, passing the Rudaia conformance kit. This is the Rudaia
    pilot exit criterion.
- **M2, visibility:**
  - `profile sync|status|hook-install`
  - Agent targets, the managed store and links
  - org baseline and team Presets (Required and Permitted)
  - inventory reports
  - portal "my machines" and admin fleet overview
- **M3, governance:**
  - signed Hub policy and expiry
  - blocks, advisories and quarantine
  - `profile request`
  - the digest gate and policy reconciliation statuses
  - the Managed config file with `required`
  - the `author` exemption
  - the approval queue with scorecards
  - the audit log and export
  - the org origin restriction

## Testing Decisions

- **What makes a good test:**
  - A good test drives FastSkill only through its external surfaces: the CLI binary, the Hub
    HTTP API, and the portal in a browser.
  - It asserts on observable results: exit codes, JSON output, files present or quarantined on
    disk, and HTTP responses.
  - It never asserts on internal types, SQL or module structure.
  - Tests MUST run without a real identity provider, network access or an embedding key.
- **Seam 1, the Hub HTTP API** (primary for Hub behavior):
  - Hub behavior is tested at the level of the whole library router: an in-process Hub on
    SQLite with a filesystem content store, called over HTTP with tokens minted by a synthesized
    issuer.
  - This covers catalog visibility, the approval state machine, Preset and profile resolution
    (including conflicts and blocks), policy signing and expiry, request intake, inventory
    ingestion, role and scope authorization, and the audit log.
- **Seam 2, the CLI binary against a running Hub** (primary for machine behavior):
  - End-to-end tests start `server serve --hub` on a free local port in a temp directory. They
    point a temp-HOME CLI at it through a Managed config file, and sign in with service login
    against the synthesized issuer.
  - This covers `auth`, `profile sync|status|request|hook-install`, Agent target links, copy
    fallback, quarantine and restore, the digest gate on `project install` and `skill add`,
    offline use within and past expiry, the new reconciliation statuses, and `skill search`
    through the Hub.
  - Agent targets are temp directories, so no agent is installed.
- **Seam 3, the portal:**
  - Playwright tests run against the embedded app served by a test Hub with a synthesized
    session. They cover the main flows (browse, add to profile, approve, block) and run axe in
    light and dark themes.
  - These tests stay thin, because behavior is already covered at seam 1.
- **Rudaia contract checks at seam 1:**
  - `401` as problem+json for a missing token, a foreign issuer and a wrong audience
  - `403` for roles on another product scope, and for an agent token without a matching
    capability
  - `rrn` on every listed resource
  - `/healthz`, `/readyz` and `/v1/openapi.json`
  - the Rudaia span attributes, captured with an in-memory exporter
- **Conformance kit:** once the Rudaia conformance kit image exists, CI also runs it against a
  seam-2 Hub. The in-repo checks above remain, so the kit is a second opinion rather than the
  only guard.
- **Identity provider stand-in:**
  - `cli-framework-oidc`'s `test-support` feature synthesizes an issuer (a wiremock discovery
    and JWKS endpoint) and mints ES256 tokens with chosen claims, including the groups claim.
  - Tests use two synthesized issuers, one standing in for the realm and one for the Agent
    STS, plus a third, untrusted issuer.
  - One optional test against a real Keycloak MAY exist behind an opt-in flag, as in that crate's
    own Keycloak end-to-end test.
- **Prior art:**
  - the library-level route tests for the existing HTTP surface
  - the `server serve` and MCP write-gate end-to-end tests, which start the binary on a free
    port and wait for it
  - the CLI snapshot helpers
  - the registry end-to-end tests, for index consumption
  - `cli-framework`'s managed-config tests, which already mint tokens against a synthesized
    issuer
  - the Rudaia Playwright and axe suite, for the portal
- **Gate:** the repository's `scripts/run-tests.sh`, including its source-size check, applies
  to Hub code unchanged. Hub tests run under the `hub` feature in CI.

## Out of Scope

- Remote MCP delivery of skills from the Hub. Files on disk stay the delivery channel in v1.
- Usage telemetry (which skills trigger in agent sessions) and any collection of prompts or
  conversation content.
- Direct upload publishing (`skill publish`). It's the first post-v1 addition.
- Signing individual skill contents, and provenance attestations.
- SCIM provisioning and team membership managed in the Hub. v1 uses identity provider groups
  only.
- Managed eval runners and blocking eval thresholds. Scorecards are informational in v1.
- Enforcing policy inside agent runtimes. It's documented as a device-management setup, not
  implemented.
- Scanning every project on a machine. Only the current project is checked, at sync.
- Multi-organization SaaS (one deployment serving several Rudaia accounts), white-label
  branding beyond the product title, SAML, and identity providers without OIDC.
- Anonymous access of any kind, including public skill metadata.
- Rudaia Packages: generalizing the engine over package types other than skills (Rudaia
  ADR-0012). The Hub is Rudaia Skills only.
- Agent targets beyond Claude Code and Cursor. The target model allows more, but none are built
  in v1.
- Changing the behavior of `server serve` without `--hub`, of `mcp serve`, or of any command when
  no Hub policy is active.

## Further Notes

- **Reuse of cli-framework managed config was decided while writing this spec.**
  `cli-framework` already ships:
  - `config-managed`: an org policy client with a cache, `max_cache_age_secs` and enforced-veto
    resolution
  - `config-service`: claim-based assignment rules, profile inheritance, an admin API and an
    append-only mutation log on Postgres

  This PRD uses them for the CLI's *settings* (Hub URL defaults, Agent targets, expiry window,
  origin restriction). It doesn't use them for the skill-level Hub policy, because:
  - `config-service` picks one profile per caller (first matching rule, single-parent
    inheritance)
  - Managed profiles are a *union* of Presets, and they carry signed digest lists

  If that split proves awkward, ADR-0017 should revisit it.
- **Name clash:** cli-framework auto-registers a `config profile` command that names a
  *configuration* profile. `profile` in this PRD names the skill Managed profile. ADR-0017 should
  settle how the glossary tells them apart.
- **Two open deployment details for M0:**
  - the exact per-OS paths of the Managed config file
  - whether the machine id survives a user wiping FastSkill's data directory
- **Groups-claim change:** the configurable groups claim in `cli-framework-oidc` also makes
  `config-service`'s default admin rule, which today checks `realm_access.roles` for
  `config-admin`, configurable in practice. The Hub's admin role mapping and that rule SHOULD
  come from one setting.
- **Open items from the Rudaia alignment:**
  - **The catalog and the control plane.** Rudaia's control plane (`aroff/rudaia-control`)
    serves a catalog of *primitives and products* read from git (Rudaia ADR-0008). The Hub's
    catalog of *skill contents* and approvals stays in the Hub. Whether control-plane views
    (usage, principals) should link to Hub resources by rrn is left for when the control plane
    exists.
  - **Governance in git.** Rudaia's GitOps discipline might want the slow-changing Hub settings
    in git: publisher-scope ownership, tracked repositories, the group → role mapping. In v1
    they live in the Hub with an audit trail. Moving them to git would be a later ADR.
  - **Teams versus product scopes.** Hub teams are identity provider groups. If Rudaia products
    turn out to be the natural teams, a team Preset could key on a product scope instead of a
    group.
  - **The role → action matrix** above is this PRD's proposal for the open Rudaia v0.2 item.
  - **Agent STS availability.** Agent tokens depend on the control plane's STS, which isn't
    built yet. The Hub still accepts only the realm issuer until the STS exists. Its
    configuration already takes a list of issuers.
  - **The issuer migration.** The `rudaia` realm issuer moves from `auth.faseinfra.net` to a
    Rudaia hostname in a later, planned migration. The Hub needs only a configuration change for
    that, and so does the CLI's Managed config file.
- **Relationship to presets:** this PRD turns the deferred "organization/team preset inheritance"
  in team-skill-presets Q18 into a Hub-resolved union, without changing bundle semantics. Bundles
  remain a project-level artifact and can be catalog entries of their own in a later milestone.
