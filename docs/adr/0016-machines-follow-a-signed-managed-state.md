# Machines can follow a signed managed state; FastSkill applies it and never decides it

Status: proposed. Date: 2026-09-24.

Related: [ADR-0001](0001-remove-sync-command.md),
[ADR-0003](0003-serve-trust-boundary-and-edge-auth.md),
[ADR-0006](0006-skill-deployment-boundary.md),
[ADR-0008](0008-bundle-ownership-and-local-changes.md),
[ADR-0010](0010-command-taxonomy.md),
[ADR-0014](0014-skill-identity-comes-from-skill-content.md), and
[team-skill-presets](../requirements/team-skill-presets.md).

## Context

team-skill-presets started from one need: a team keeps using incorrect or outdated skills, and
keeping it current means asking each person to update. Bundles and Manifest composition reduced
that work, but a skill still reaches a machine only when someone there runs a command. ADR-0006
gave FastSkill the desired skill setup, its installation, verification, updates and rollback, and
explicitly left "fleet rollout mechanisms" unsettled.

Three things are still missing:

- **Rollout.** A set of skills decided somewhere else doesn't reach machines by itself.
- **Withdrawal.** When a skill turns out to be harmful, nothing stops it being used or installed
  again, and nobody can tell which machines have it.
- **Agent visibility.** Agents read skills from per-user folders such as `~/.claude/skills` and
  `~/.cursor/skills`. The global installation scope doesn't install into them, so a globally
  installed skill is invisible to every agent.

Earlier decisions constrain the answer:

- ADR-0003: FastSkill holds no identity and is not a security boundary.
- team-skill-presets Q9 and Q14: login and credential management stay outside FastSkill.
- team-skill-presets Q4: a managed launcher can't be relied on for enforcement, and execution
  restrictions belong to the agent runtime.

Who decides what a machine should have varies a lot. It may be one person editing a file in git,
a CI job, device management, or a service that knows every user. FastSkill shouldn't need to know
which, and shouldn't have to change when that decision-maker's rules change.

## Decision

### The managed state

1. **A managed state is a signed, resolved description of what one machine should have.** It
   contains:
   - a format version
   - `issued_at` and `expires_at`
   - `subject`: an opaque label that FastSkill only displays, never interprets
   - `skills`: each entry has an id, a content digest (the checksum a Lock pins) and an
     artifact location
   - `allowed`: either `listed`, meaning only digests in `skills` or `also_allowed` may be
     installed, or `any`
   - `also_allowed`: extra digests that may be installed in projects but aren't installed by
     default
   - `blocked`: digests that must not be present, each with an optional message for the user
   - `report_url`: optional (decision 9)
   - `exclusive_targets`: a flag (decision 7)

   Entries are resolved content, like a Lock, not intent, like a Manifest. Applying a managed
   state never resolves versions, reads repositories or runs dependency resolution. FastSkill
   refuses a managed state whose format version it doesn't support.
2. **Every managed state is signed, and the keys are pinned locally.** FastSkill accepts a
   managed state only if it verifies against a key pinned in configuration. Verification keys are
   never fetched from the managed source. Several keys may be pinned at once, so keys can be
   rotated. The signature is what lets a state be cached, carried offline, or served from a git
   repository or object storage without trusting the transport.
3. **A managed state expires.** FastSkill keeps the last valid state and uses it offline until it
   expires. After expiry, FastSkill refuses operations that add or change skill content. It never
   removes installed skills because of expiry alone.

### Getting it

4. **A managed source is one location.** It is an `https://` URL or a local file path, configured
   under `[tool.fastskill.managed]`. FastSkill fetches it and nothing more. Whatever produces the
   state (a static signed file, a CI job, a service) is outside FastSkill.
5. **Credentials come from an external command.** When the source needs authorization,
   configuration names a credential command. FastSkill runs it and uses the bearer token it prints
   for that one request. This is how cloud CLIs, identity provider CLIs and CI already supply
   credentials to other tools.
   - FastSkill never obtains, refreshes, stores or inspects the token.
   - It sends the token only to the managed source's origin. It never sends it to artifact
     locations on another origin, which must carry their own authorization, such as a
     pre-signed URL.
   - FastSkill gains no login command.
6. **Machine-wide configuration wins.** A system configuration file at a fixed platform location,
   deployed by device management, can set:
   - the source
   - the pinned keys
   - the credential command
   - `required = true`

   For these keys it overrides user configuration, environment variables and flags. With
   `required = true`, commands that read or change skills refuse to run until a valid, unexpired
   managed state is cached.

### Applying it

7. **`managed apply` makes the machine match the state.**
   - It downloads each listed skill whose digest isn't already present into a user-level
     **managed store**, verifying the digest before anything is used.
   - It deploys each skill into every **Agent target**, meaning each installed agent's per-user
     skills folder. It links where the platform allows and falls back to a verified copy.
   - Managed skills that are no longer listed are removed from the managed store and the Agent
     targets. The managed store belongs to FastSkill.
   - A managed copy that was modified locally is quarantined before it's replaced, never
     silently overwritten. This follows ADR-0008.
   - Content in an Agent target, or in the current project's skills directory, is quarantined if
     its digest is `blocked`. With `allowed = listed` and `exclusive_targets = true`, content
     whose digest isn't allowed is quarantined too. Editable local skills (ADR-0005) are
     reported, never quarantined.
   - Quarantine moves content into a timestamped folder with a small record of the digest and the
     reason. It never deletes anything.
8. **Installs are gated by digest.** While a managed state is active, `skill add`,
   `skill update`, `bundle add`, `bundle update` and `project install` refuse content whose
   digest is `blocked`, or not allowed under `allowed = listed`. They refuse before changing any
   state and name the digest and its message. Committed Manifests and Locks keep working
   unchanged. The gate only decides whether their pinned content may be installed on this
   machine.
9. **Reporting is fixed and inspectable.** If the state names a `report_url`, `managed apply`
   sends it one report and nothing else. The report contains:
   - a random machine id created at enrollment, not derived from hardware
   - the Agent targets
   - each installed skill's id, digest, Origin kind and reconciliation status
   - the quarantine actions taken

   Prompts, file contents and usage are never sent. `managed status --json --report` prints the
   exact report body without sending it. The report is authorized the same way as the source
   (decision 5).
10. **The reconciliation vocabulary grows by two statuses.** They are emitted only while a
    managed state is active: `managed-blocked` and `managed-not-allowed`. `skill list --check`
    fails on both. With no managed state, the eleven existing statuses and their meanings don't
    change.
11. **Session start is a convenience, not an enforcement point.** `managed enroll` records the
    source, creates the machine id, and registers a session-start hook in each installed agent
    that supports one. The hook runs `managed apply` within a short time budget, always exits
    successfully, and writes no agent metadata files. ADR-0001's removed `sync` is not coming
    back. Agents without hooks get the state on explicit `managed apply` or a scheduled run.

### Commands

12. A `managed` namespace joins ADR-0010's tree with four actions:
    - `enroll`
    - `apply`
    - `status`, which shows the source, subject, expiry, Agent targets, pending quarantines and,
      with `--report`, the report body
    - `unenroll`, which system configuration with `required = true` refuses

    All of them are classified for `server serve` and `mcp serve` under ADR-0003.
    `apply`, `enroll` and `unenroll` are write operations.

### What FastSkill does not do

13. **FastSkill never decides.** It doesn't know why a skill is listed, allowed or blocked. It
    has no users, roles or groups, and `subject` is only displayed.
14. **FastSkill ships no managed source.** `server serve` doesn't serve managed states and stays
    as ADR-0003 describes. ADR-0003 is unchanged: no FastSkill component holds identity, and the
    CLI passes along a token it was handed.

### Where the code lives

15. Knowledge about agents belongs in `aikit-sdk`:
    - each agent's per-user skills folder, alongside the project folder it already knows
    - link-or-copy deployment into an agent's skills folder
    - registering a session-start hook in each agent that supports one

    FastSkill owns the rest: the managed state format and its verification, `managed apply`, the
    install gate, quarantine, and the report. Both are libraries that any tool producing managed
    states can depend on.

## Consequences

- **Enforcement has limits.** FastSkill enforces at its own operations: `managed apply`, installs
  and `--check`. A file copied into an agent folder between applies is caught at the next apply.
  Hard enforcement means configuring the agent runtime, through device management, to load only
  managed folders. That is documented, not implemented, consistent with team-skill-presets Q4.
- **How fast a block arrives.** A blocked digest reaches a machine at its next apply, which is at
  least every agent session start where hooks exist. An offline machine is covered within the
  expiry window at the latest.
- **Nothing changes unless configured.** With no managed source, FastSkill does no new network
  activity, emits no new statuses, and every current command behaves as before.
- **A static file is enough.** A signed managed state in a git repository or a storage bucket is
  a complete managed source. Tools to build and sign a state from a Manifest are out of scope
  here and may follow.
- **The format is a compatibility surface.** Like the reconciliation vocabulary, the managed
  state and report formats are versioned and documented. Fields are added only in
  backward-compatible ways within a format version.
- **ADR-0006's open question is settled** for fleet rollout. Preset composition and session
  verification stay open.
- **CONTEXT.md gains** managed state, managed source, managed store, Agent target, quarantine,
  and the two new reconciliation statuses.
- **aikit changes come first.** The per-user skills folders, deployment, and hook registration
  land in `aikit-sdk` before `managed apply` can deploy to agents.

## Considered alternatives

- *A built-in login command* was rejected. It would pull credential handling into FastSkill,
  against ADR-0003 and team-skill-presets Q14. A credential command covers identity provider
  CLIs, cloud CLIs and CI tokens without FastSkill holding any of them.
- *Selection rules in the client*, such as which skills are mandatory or optional for whom,
  were rejected. Each rule change on the deciding side would need a FastSkill release and a
  format change. A resolved state keeps the client the same whatever produced it.
- *A remote Manifest applied with `project install`* was rejected. A Manifest carries intent
  (ranges, repositories) that each machine would resolve differently over time, and it has no
  blocked list, expiry or signature. A managed state carries resolved content, like a Lock, so
  every machine applies the same thing.
- *An unsigned state trusted over HTTPS* was rejected. The cached copy, file sources and git
  sources need integrity that doesn't depend on the transport.
- *Deleting disallowed content* was rejected. Quarantine loses nothing and can be reversed.
- *Reviving `sync`* was rejected. ADR-0001 removed a command of that name with a different
  meaning, so the new actions are `managed apply` and `managed enroll`.
