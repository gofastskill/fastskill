# Bundle commands

FastSkill 0.9.250

Source: https://docs.gofastskill.com/cli-reference/bundle-command

Release revision: 98b75151bba19f2131ab4223f72f0d3befbde3d4

Documentation revision: 98b75151bba19f2131ab4223f72f0d3befbde3d4



# `bundle` commands

Bundles turn a skill project into one versioned ZIP. The artifact contains every declared skill,
its installed dependency closure, resource files, and content digests. A recipient can
install it without access to the original repositories. On Unix systems, executable files remain
executable after bundle installation; other permission bits are normalized for portability.

## Build a bundle

Complete the [quickstart](/quickstart), then give the project an identity and version in its existing
`skill-project.toml`:

```toml
[metadata]
id = "notes-team"
version = "1.0.0"

[dependencies]
review-notes = "2.1.0"
```

Every dependency becomes a bundle member. Its installed version must satisfy the declaration.
FastSkill stops before writing an artifact when a declared dependency is
missing, has invalid frontmatter, or has a different version.

Build the release:

```bash
fastskill project install
fastskill bundle build --output dist
```

The result is `dist/notes-team-1.0.0.zip`. Its embedded identity, version, membership, and
digests are authoritative, so renaming the download does not change the release.

There is no `[bundle.members]` list to maintain. A source Manifest using the earlier `[bundle]`
format remains buildable for migration, while all new bundles should use `[metadata]`.

## Install and inspect

In a second project initialized with `project init --yes --skills-dir .claude/skills`,
install the artifact with `bundle add`. Replace the local path with the artifact you
built. The HTTPS URL below is an example for an artifact hosted by your team:

```bash
fastskill bundle add ./notes-team-1.0.0.zip
fastskill bundle add https://releases.example.com/notes-team-1.0.0.zip
fastskill bundle list
```

Installation copies the artifact into `.fastskill/bundles/`, adds its declaration to
`skill-project.toml`, and records the exact release and members in `skills.lock`.

Commit these files when teammates and CI need the same setup:

```bash
git add skill-project.toml skills.lock .fastskill/bundles/
```

Then restore the exact locked release:

```bash
fastskill project install --lock --offline
```

`--lock` uses the bundle artifact, version, digest, and membership recorded in `skills.lock`.
It does not switch to a different release if the manifest has drifted. `--offline` additionally
guarantees that restore does not contact a repository or embedding provider.

## Update a bundle

Bundle updates are explicit. After building version `1.1.0` of your bundle, preview
and apply that replacement:

```bash
fastskill bundle update notes-team --from ./notes-team-1.1.0.zip --dry-run
fastskill bundle update notes-team --from ./notes-team-1.1.0.zip
```

FastSkill checks the new artifact, shared ownership, local modifications, and release immutability
before it changes installed members. The preview runs the same validation as apply and reports
membership changes even when retained member bytes are unchanged.

## Remove a bundle

Remove the bundle identity and the members that have no other owner:

```bash
fastskill bundle remove notes-team
fastskill bundle remove notes-team --force
```

Removing a direct declaration detaches that owner. FastSkill retains the installed member while a
bundle or another dependency root still requires it. An ID that is only a transitive requirement
cannot be removed directly; the error names the roots that require it. A member is deleted only
after its final direct, transitive, or bundle owner is gone.

## Reuse a setup without packaging it

A references-only bundle would duplicate a Manifest, so FastSkill uses Manifest composition for
that case. A consuming project references the shared file once:

```toml
[tool.fastskill.manifests]
notes-team = "../notes-team/skill-project.toml"
```

The shared Manifest can declare skills from multiple Git repositories, configured repositories,
ZIP URLs, or shared paths. `project install` resolves the combined roots and the consuming project
writes the exact results to its own `skills.lock`. References may be nested. FastSkill rejects
missing files, reference cycles, and incompatible declarations of the same skill ID.

Use a bundle when recipients need an offline-ready artifact. Use a composed Manifest when they
have access to the declared sources and should obtain the skills from those sources.

## Legacy member overrides

Bundles created with the earlier `[bundle.members]` format may permit a personal replacement.
Those existing artifacts remain supported through `fastskill bundle override <skill-id> --from <directory>` and `fastskill bundle override <skill-id> --reset`. Manifest-driven builds make all
members required and do not create new override policies.

Every bundle action except `bundle list` changes files. The MCP server hides and refuses these
tools unless it starts with `fastskill mcp serve --enable-write`.

