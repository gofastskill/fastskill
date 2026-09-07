# Publishable skill bundles and team presets

Status: implemented. The user confirmed the consolidated product design and shared understanding after decisions Q1–Q24. This document defines the released bundle behavior.

## Confirmed need

The user supports a team whose members use incorrect or outdated skills. Keeping the team current requires repeatedly asking individuals to update. FastSkill should reduce that manual follow-up.

## Confirmed initial feature

Create a publishable ZIP bundle containing a `skill-project.toml` and multiple skill files/directories. A recipient uses `fastskill add` for the entire bundle to install its skills on the target. Initial distribution uses local ZIP files and public HTTPS downloads from existing artifact storage. Private downloads and uploads use existing external tools. The accepted command surface and archive contract are recorded below.

Q4: A managed launcher may be offered later as an optional convenience. It cannot be required or relied on for enforcement. Bundle creation, publication, and installation must work independently of managed agent sessions. Installation does not establish that an independently launched agent used or followed the installed skills.

## Agreed customization model

Presets distinguish required skills, overridable defaults, and permitted additions. Personal customization must respect those rules. Execution restrictions belong in the runtime and its permissions. The earlier proposal for managed CI checks and developer drift reporting is a future integration concern, not a prerequisite for the initial bundle feature.

Q19: Bundle members are required unless explicitly marked overridable. Unrelated personal additions are allowed. Every owning bundle MUST permit a shared skill override. An update that invalidates an existing override MUST stop and explain the conflict. These rules govern FastSkill operations and do not establish enforcement over independently launched agents.

## Agreed bundle semantics

- Q5: Resolve skill dependencies when building the bundle and include their exact contents. Once the ZIP is available locally, installation must not fetch additional skills or require access to their original repositories. External tools required to use skills remain separate.
- Q6: Retain an installed bundle's identity, version, and skill membership for coordinated updates and removal. Each skill remains individually inspectable. Bundle updates must preserve unrelated personal additions.
- Q7: Identical contents under the same skill ID may be shared, with all bundle owners recorded. Different contents MUST cause a conflict before installation changes occur. Removing a bundle MUST preserve skills still owned by another bundle or explicitly installed by the user.
- Q8: An update or removal that would replace or delete a locally modified member MUST stop before changing the installation. The user must explicitly discard the modification or retain it as a permitted personal override. Local changes MUST NOT be silently overwritten or deleted.
- Q10: Bundle identity and version identify immutable packaged contents, independently of member skill versions. Changed packaged contents require a new bundle version. FastSkill MUST record content digests and reject different artifacts claiming an already-known identity/version.
- Q11: Initial updates explicitly take a replacement ZIP or HTTPS URL, verify bundle identity, preview changes, and apply that release. `add` MUST NOT silently upgrade an installed bundle. Automatic update discovery is deferred until an update feed or catalog is defined.
- Q12: Stage and validate the complete bundle before applying it. Retain recovery information sufficient to restore the previous setup if application fails. Skill files, bundle membership, Manifest, and Lock MUST remain consistent; a partial installation MUST NOT be reported as successful.
- Q13: Build from skills declared in `skill-project.toml` and their resolved dependency closure, including each skill's required resource files. Unrelated project files MUST NOT be included merely because they exist in the author's directory.
- Q15: The recipient chooses the destination through project configuration or an explicit CLI override. Bundle-internal paths MUST be relative. The author's installation directory MUST NOT control installation on the target. The same artifact can be installed into different agents' skill directories.
- Q16: Package verified, explicitly selected contents. Local working edits MUST be deliberately incorporated into the build inputs before packaging. Packaging MUST NOT silently upgrade dependencies or substitute an ambient installed copy. Dependency resolution during bundle preparation must be explicit; producing the artifact preserves that selection.
- Q17: Record bundle dependencies in the recipient's Manifest and exact bundle releases in the Lock. Normal `fastskill install` restores declared bundles alongside individually declared skills, provided their artifacts are available. Private artifact acquisition still follows Q14.
- Q18: The initial bundle declares its complete skill selection and per-skill customization rules. Users may install multiple compatible bundles and add permitted personal skills. Organization/team/project preset inheritance and bundles containing other bundles are deferred.
- See [ADR-0007](../adr/0007-self-contained-tracked-skill-bundles.md) and [ADR-0008](../adr/0008-bundle-ownership-and-local-changes.md).

## Agreed distribution scope

Q9 and Q14: FastSkill builds the ZIP; existing CI or storage tooling uploads it. FastSkill supports local ZIP installation and direct public HTTPS downloads. For private storage in the initial feature, an existing authenticated tool downloads the ZIP before local installation. Cloud login, credential management, and uploads remain outside FastSkill. Building and installing bundles remain independent of the hosting service; a dedicated FastSkill catalog is deferred.

The existing direct single-skill ZIP implementation has no private authentication hook and can persist complete signed URLs. It MUST NOT be assumed to provide a credential-safe private bundle download path. Signed-URL handling is not part of the approved initial private-download workflow.

## Agreed product boundary

Q1: FastSkill owns desired skill setup, installation, verification, updates, and rollback. Existing infrastructure tooling supplies environments, agent runtimes, external tools, and credentials. See [ADR-0006](../adr/0006-skill-deployment-boundary.md). This describes intended responsibility, not current feature completeness.

## Agreed command surface

Q20: Bundle lifecycle uses existing commands, with explicit bundle selection where needed.

```bash
fastskill bundle build
fastskill add ./payments-team-1.2.0.zip
fastskill list --bundles
fastskill update --bundle payments-team --from ./payments-team-2.0.0.zip
fastskill remove --bundle payments-team
```

`fastskill add` recognizes bundle archives and existing single-skill archives. Normal `fastskill install` restores both bundle and individual dependencies. Existing single-skill behavior MUST remain unambiguous.

## Agreed validation boundary

Q21: Packaging requires structural validation, complete skill dependencies, valid identities, and content digests. Agent evaluations are optional and separately invoked. Packaging and installation MUST NOT automatically execute bundled scripts or launch agents. Teams may require evaluations in their external publishing pipeline.

## Consolidated archive and override contract

The following captures the closing archive discussion, including Q22's versioned filenames, Q23's explicit overrides, and Q24's renamed-download behavior.

### Versioned artifact filenames

Q22: Generated and published bundle filenames MUST contain the bundle version, so releases can be distinguished without opening the archive. Canonical pattern: `<bundle-id>-<version>.zip`, for example `payments-team-1.2.0.zip`.

Q24: Installation MUST accept renamed downloads when their embedded metadata and contents validate. The embedded bundle identity/version and verified contents are authoritative; filenames are distribution labels. Renaming a downloaded file MUST NOT change its release identity or bypass immutable-release checks.

### Archive identification and layout

A bundle ZIP has a root `skill-project.toml` with an explicit bundle format marker, bundle identity/version, member declarations, and customization rules. It also contains the resolved Lock and skill contents under relative member paths:

```text
payments-team-1.2.0.zip
  skill-project.toml
  skills.lock
  skills/
    code-review/SKILL.md
    code-review/references/...
    payments-debugging/SKILL.md
```

The bundle marker distinguishes this from a single-skill ZIP; invalid bundle metadata is an error rather than a fallback to single-skill interpretation. The recipient's project Manifest is merged with the bundle dependency, never replaced by the archive's Manifest. Exact TOML field spelling and serialization migration belong in the implementation specification.

### Explicit override resolution

Q23 (agreed): An override is a deliberately declared personal skill selection, with an explicit origin, rather than an untracked edit treated as approved. To retain edited contents, the user moves or copies them into a personal skill origin and records the override. Every owning bundle must permit it. Required members cannot be overridden while retaining a claim that the bundle requirements are satisfied. Undeclared modifications remain conflicts.

### Acceptance scenarios

1. Build a bundle containing two declared skills, resource files, and a transitive skill dependency; exclude unrelated project files.
2. Install that local ZIP on a fresh target without network access or the original repositories; verify exact packaged contents and resource paths.
3. Add a bundle and an individual skill, then restore both from the recipient Manifest and Lock with artifacts available; do not use author-machine absolute paths.
4. Update a bundle whose new release changes a member and removes another; preserve unrelated personal additions and shared members.
5. Share identical skill contents between two bundles and an individual declaration; remove owners one at a time and preserve the skill until no owner requires it.
6. Reject differing contents under a shared skill ID before changing files or metadata.
7. Detect local edits before replacement/removal; permit an explicit override only when every owner allows it; reject policy-tightening updates that invalidate an override.
8. Reject changed artifacts under a previously known bundle identity/version; do not silently upgrade an installed bundle through `add`.
9. Inject a failure during application and recover the previous consistent skill files, membership, Manifest, and Lock. Do not report partial success.
10. Reject malformed manifests, missing dependencies, digest mismatches, and malicious archive paths without writing outside the target.
11. Distinguish bundle archives from supported single-skill ZIPs and retain ordinary single-skill lifecycle behavior.
12. Build and install without launching agents or executing packaged scripts; evaluations remain an independent operation.
13. Produce `<bundle-id>-<version>.zip`; rename it to `download.zip` and install successfully under the embedded identity/version. A misleading filename MUST NOT change identity, version, or digest verification.

The implementation validates archives before applying them, preserves recovery data through the transaction, and exposes explicit overrides through `fastskill bundle override <skill-id> --from <directory>`.

## Deferred beyond the initial bundle feature

- Optional managed launch, session verification, and handling updates during active sessions.
- Fleet reporting and runtime enforcement integrations.
- A dedicated FastSkill bundle catalog.
- Automatic discovery of new bundle releases.
- In-app cloud publishing, login, and private-download credential management.
- Inherited organization/team/project presets and bundles containing other bundles.

## Existing architectural boundary

[ADR-0003](../adr/0003-serve-trust-boundary-and-edge-auth.md) places identity and authorization outside FastSkill and scopes served instances to one trust domain. An integrated multi-user enforcement service would require an explicit reconsideration of that decision. This interview has not changed it.
