# Skill bundles are self-contained and retain installed identity

Status: accepted

Team skill distribution needs predictable installation and coordinated updates. Bundle builds resolve skill dependencies and include their exact contents in the ZIP alongside `skill-project.toml`. Installing a locally available bundle requires no additional skill downloads; external runtime tools remain separately managed.

We chose complete artifacts over manifest-only packages that resolve dependencies on each target, so recipients need no access to the original skill repositories and receive the packaged contents. This trades larger artifacts for predictable, offline skill installation.

Installation retains bundle identity, version, and skill membership rather than only expanding independent skills. This supports coordinated lifecycle operations while keeping individual skills inspectable. Updates preserve unrelated personal additions. [ADR-0008](0008-bundle-ownership-and-local-changes.md) defines shared ownership, conflicts, and local modifications.

A bundle release is immutable and has its own identity and version, independent of member skill versions. FastSkill records content digests and rejects a different artifact claiming an already-known identity/version. Changed packaged contents require a new bundle version.

Generated and published artifact filenames follow `<bundle-id>-<version>.zip` so recipients can distinguish releases without extracting them. Installation accepts renamed downloads when their embedded metadata and contents validate. Embedded identity/version and content verification are authoritative; changing a filename cannot change a release identity or bypass immutable-release checks.

Updates explicitly receive a replacement ZIP or HTTPS URL, verify identity, preview changes, and
apply the selected release. `bundle add` does not silently upgrade an installed bundle. Automatic
discovery of newer releases is deferred until an update feed or catalog is defined. ADR-0010 moves
the complete lifecycle under `bundle`.

Bundle contents come from the author's declared Manifest skill set and its resolved dependencies, including required skill resources rather than unrelated project files. Internal paths are relative. The recipient's project configuration or explicit CLI override selects the installation directory, so the author's filesystem layout does not constrain the target.

Packaging uses verified, explicitly selected contents. Authors must deliberately incorporate local edits and changed dependency resolutions into build inputs; packaging cannot silently upgrade dependencies or substitute ambient installed copies. Initial bundles declare their complete skill selections and customization rules. Preset inheritance and bundles containing bundles are deferred.

Recipient Manifests declare bundle dependencies and Locks pin exact bundle releases. Normal project installation restores these alongside individually declared skills when the referenced artifacts are available. This keeps bundle distribution in the existing project installation workflow rather than requiring a separate manual bootstrap step.

Build validation covers structure, dependency completeness, identities, and content digests. Agent evaluations are separately invoked and optional to FastSkill packaging; publishing pipelines may impose their own evaluation requirements. Neither packaging nor installation automatically launches agents or executes packaged scripts.
