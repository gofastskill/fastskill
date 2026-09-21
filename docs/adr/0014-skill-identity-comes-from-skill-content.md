# Skill identity comes from skill content, never from a catalog path

Status: accepted. Date: 2026-09-19. Implemented: 2026-09-21.

Related: [ADR-0004](0004-bare-version-is-exact.md),
[ADR-0005](0005-install-seam-and-origin-model.md),
[ADR-0009](0009-resolution-and-restoration-policy.md), and
[ADR-0010](0010-command-taxonomy.md).

`marketplace create` publishes a repository of skills as a catalog. Today three parts of
the system disagree about what a catalogued skill *is*:

| Part | Skill id | Skill version |
|---|---|---|
| Writer (`marketplace create`) | `skill-project.toml`, then discarded: it writes `./{id}` as the path | not written per skill |
| Reader (listing) | guessed from the last folder of that path | catalog-wide version, else `1.0.0` |
| Git and local acquisition | the repository's own `skill-project.toml`; the catalog is ignored | same, matched exactly |

The consequences were reproduced on 0.9.241:

- A skill at `workspace/cli-rust-dev` is catalogued as `./cli-rust-dev`, which does not exist.
  `marketplace create ./skills --output .claude-plugin/marketplace.json` writes paths relative
  to `./skills` although readers resolve them from the repository root.
- The listing only shows correct ids *because* the writer fakes the path. Writing real paths
  alone would rename any skill whose folder differs from its id.
- A skill whose own version differs from the catalog-wide version is listed at a version that
  acquisition then refuses to find.
- The generated file is not a valid Claude Code marketplace. `claude plugin validate` (2.1.273)
  reports three errors on a freshly generated catalog, because absent optional values are
  serialized as `null` rather than omitted: `owner.email`, `plugins.0.description`, and
  `metadata`. With no `--owner-name` at all, required `owner` is `null` too.

Three adjacent defects found alongside these were fixed while this decision was being drafted,
and their fixes are assumed below rather than restated: listing at the configured ref instead of
an assumed `main` (#343), resolving bare skill ids across repositories (#344), and reading a git
source's catalog through `git clone`, which gave listing and acquisition one credential model and
made private and non-GitHub git catalogs work (#346).

#346 is what makes this decision cheap. Listing a git source now happens with the whole
repository already checked out, so identity can be read from each skill's own files instead of
being inferred from the catalog. No side-car index and no new file format is required to stop
guessing, and a third-party Claude Code catalog that will never carry FastSkill metadata is
listed correctly on its content alone.

## Decision

- A skill's id and version MUST be read from the skill's own content, in this order:
  `skill-project.toml`, then `SKILL.md` frontmatter (`name`, and `version` when present),
  then an error naming the folder. A reader MUST NOT derive an id from a folder name, nor a
  version from a catalog-wide default. A folder name need not equal the skill id.
- A catalog's `skills` entries are **pointers**: which folders to inspect, and the curated set
  a publisher chose to advertise. They MUST NOT be treated as identity.
- Listing a git or local source MUST resolve identity from the checkout that the catalog was
  read from, in the same pass and at the same commit, so a listing and a later acquisition of
  the same skill cannot disagree. ADR-0009's offline and cache rules are unchanged; the
  on-disk `SourceIndex` records resolved values rather than guessed ones.
- `marketplace create` MUST write each skill's folder path relative to the **catalog root**
  (the directory containing `.claude-plugin/`), with `/` separators and no `..`, regardless of
  which directory was scanned.
- The generated `marketplace.json` MUST be a valid Claude Code marketplace: it MUST pass
  `claude plugin validate`, and absent optional values MUST be omitted rather than written as
  `null`. `owner` is required input.
- Generation MUST be deterministic: entries sorted, and no timestamps or other run-varying
  content, so a committed catalog has a stable diff.
- Generation MUST fail, naming the folders involved, on a duplicate `id@version`, a missing id
  or version, a skill outside the catalog root, or an unreadable `SKILL.md`. Skipping such a
  folder with a warning is not allowed. `.git` and hidden folders are not scanned.
- `marketplace create --check` MUST write nothing and exit non-zero when the committed catalog
  differs from what generation would produce, so CI can reject a stale catalog.
- Acquisition for git and local sources keeps resolving a skill by identity. Where a catalog
  entry and the resolved identity disagree, the failure MUST name the stale catalog rather
  than silently installing a different skill.

## Tradeoffs

Reading identity per skill costs N small file reads from a clone that #346 already makes. It
adds no network round trips. In exchange, listing and acquisition answer the identity question
with the same inputs, so the two cannot drift.

Requiring `owner` is a new mandatory input for `marketplace create`. Claude Code requires it,
and a catalog that Claude Code rejects has no reason to exist.

Refusing to skip a malformed skill folder means one bad skill fails the whole catalog. That is
deliberate: a partially generated catalog is how a skill silently disappears from a listing.

This changes the generated catalog's contents with no compatibility shim, consistent with
ADR-0005. No published catalog can depend on the current output, which Claude Code rejects.

## Out of scope

Two related problems are deliberately left undecided here, because each needs its own
investigation and neither blocks the above:

- **Zip-url identity and integrity.** A zip-url entry's URL names a downloadable archive, not
  a folder in a checkout, so there is nothing to read identity from and this decision's ladder
  does not reach it. Content checksums and a FastSkill-owned index are likely part of that
  answer; `marketplace create` does not currently publish archives at all.
- **Version discovery for git marketplaces.** `list_skills()` advertises exactly one version
  per skill for every repository type, so a git marketplace read at one commit can only
  advertise the tip. What ADR-0004 pins and ADR-0009 resolves ranges over therefore has a
  single candidate today. Whether versions should come from git tags is a resolution-policy
  question, not a catalog-format one.

Signing the catalog, and per-skill Claude Code plugin entries, are also out of scope. The
lockfile already records `commit_hash`; this decision does not change the Lock.
