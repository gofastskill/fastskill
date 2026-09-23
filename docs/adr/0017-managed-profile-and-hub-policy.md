# ADR-0017: Managed profiles, the signed Hub policy, and how the CLI enforces it

Status: proposed. Date: 2026-09-23.

Related: [ADR-0016](0016-hub-mode-is-a-security-boundary.md),
[ADR-0014](0014-skill-identity-comes-from-skill-content.md),
[ADR-0005](0005-install-seam-and-origin-model.md),
[ADR-0008](0008-bundle-ownership-and-local-changes.md),
[ADR-0001](0001-remove-sync-command.md),
[team-skill-presets](../requirements/team-skill-presets.md), and the
[Hub PRD](../../specs/hub-prd.md).

## Context

A security team wants to decide which skills can be on user machines. Team leads want team setups
to reach members without chasing them. The Hub (ADR-0016) knows users, teams and approved content.
We need to decide what the Hub tells each machine, how the CLI acts on it, and where enforcement
stops.

team-skill-presets deferred organization and team preset inheritance. It also said that a managed
launcher can't be relied on for enforcement, and that execution restrictions belong to the agent
runtime.

## Decision

### Profile model

1. **Content is approved by digest.** Approval applies to a content digest (ADR-0014), never to a
   skill id or version alone. Each digest moves through these states:
   - `imported → team-approved → org-approved`
   - From any state it can become `rejected` or `blocked`.
   - `blocked` is terminal and may carry an advisory.
   - Team maintainers approve within their team's publisher scope. Security approvers approve
     org-wide.
2. **A Managed profile is a union of Presets.** A user's profile is resolved from:
   - the org baseline Preset
   - every team Preset linked to an identity provider group the user belongs to
   - the user's own chosen Permitted additions

   Precedence: `blocked` beats everything, and a Required skill beats a Permitted addition. Two
   Presets requiring different content for one skill id is an ownership conflict. It is reported
   to the user and admins, never resolved by guessing.
3. **The Hub policy is signed and can expire.** For each user the Hub issues a policy containing:
   - the resolved profile (skill id and digest, which the Hub API also exposes as a
     `@sha256:` resource name)
   - the approved digests visible to the user
   - blocked digests with advisories
   - the org origin restriction
   - issue and expiry times

   The CLI verifies the Hub's signature before use and caches the last valid policy. Within the
   expiry window (org setting, default 7 days) the cached policy works offline. After expiry the
   CLI refuses operations that would add content, but it never removes installed skills because
   of expiry alone.

### What the CLI does

4. **`profile sync` makes the machine match the profile.** The `profile` namespace holds `sync`,
   `status`, `request` and `hook-install`. `profile sync` installs profile content once into a
   managed store and links it into each configured Agent target's user skills folder, falling
   back to verified copies where links are unavailable. It runs:
   - explicitly
   - through a session-start hook that `profile hook-install` registers in each agent that
     supports hooks

   The hook always exits successfully within a short time budget. It is unrelated to the `sync`
   command removed by ADR-0001 and writes no agent metadata files.
5. **Enforcement is quarantine, never deletion.**
   - During sync, content in an Agent target folder, or in the current project's skills
     directory, is quarantined if its digest is blocked or not approved for the user. The
     exception is editable skills of a user with the `author` role, which are reported as
     unmanaged editable content instead.
   - Quarantine moves the content into a timestamped folder with a manifest recording the reason.
   - `profile request` submits quarantined or local content to the approval queue. Approval makes
     a later sync restore it.
6. **Project installs are gated by digest.** While a Hub policy is active, `project install`,
   `skill add` and `skill update` refuse, before changing any state, content whose digest isn't
   approved for the user or whose origin breaks the org origin restriction. Committed
   `skill-project.toml` and `skills.lock` files keep working unchanged. The gate only decides
   whether their pinned content may be installed.
7. **The reconciliation vocabulary grows by two statuses**, emitted only while a Hub policy is
   active:
   - `policy-blocked`: installed content is blocked.
   - `policy-unapproved`: installed content isn't approved for the user.

   `skill list --check` fails on both. With no Hub policy, the eleven existing statuses and their
   meanings are unchanged.
8. **Joining the Hub can be required by the organization.** A Managed config file at a fixed
   system location, deployed by device management, overrides user configuration, environment
   variables and flags. It can set `required`: FastSkill then refuses skill-reading and
   skill-changing commands until a Hub session exists.
   - CLI settings (Agent targets, expiry window, author exemption, origin restriction) come from
     cli-framework's managed-config policy.
   - The skill policy stays Hub-native. The managed-config service assigns one profile per
     caller, while a Managed profile is a union carrying signed digest lists.
9. **Inventory is reported and nothing else.** Each sync reports:
   - user, machine id and Agent targets
   - installed skills and their digests
   - reconciliation statuses
   - quarantine actions
   - unmanaged editable content

   Prompts, conversation content and skill usage are never collected by this mechanism.
   Inventory is Hub data that admins query, not telemetry. It doesn't travel through the
   OpenTelemetry pipeline that carries the Hub's operational signals (Rudaia C3).

## Consequences

- **The limits of enforcement.** FastSkill enforces at its own operations: sync, install and
  check. A file copied into an agent folder between syncs is caught at the next sync, not before.
  Hard enforcement means configuring agent runtimes through device management to load only
  managed folders. That is documented, not implemented, consistent with team-skill-presets.
- **Revocation reach.** A block reaches a machine at its next sync, which happens at least at
  every agent session start. An offline machine gets it within the expiry window at the latest.
  Machines past the window are visible to admins as stale.
- **Signing.** A signed list of digests gives Hub-managed machines the integrity guarantee that
  signing each skill would. Signing individual skills is deferred until content must be verified
  outside a Hub.
- **Scanning.** Only the current project is scanned. Scanning every project on a machine was
  rejected as intrusive and slow.
- **Documentation updates.** CONTEXT.md gains Hub, Managed profile, Hub policy, Agent target,
  managed store, quarantine, publisher scope ownership, inventory report, and the two policy
  reconciliation statuses.
- **Organization.** "Organization" in this ADR means the one organization a Hub deployment
  serves. In the Rudaia deployment that is a Rudaia account. Rudaia's product-level
  "organizations" (Keycloak Organizations in a product's end-user realm) are unrelated.
- **Naming.** cli-framework's auto-registered `config profile` names a *configuration* profile.
  The glossary MUST call this ADR's concept "Managed profile" or "skill profile" in prose, and
  the `profile` command namespace refers only to it.
