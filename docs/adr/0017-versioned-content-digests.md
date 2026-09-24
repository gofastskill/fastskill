# Content digests are versioned and frame every field

Status: accepted. Date: 2026-09-24. Implemented: 2026-09-24.

Related: [ADR-0007](0007-self-contained-tracked-skill-bundles.md),
[ADR-0008](0008-bundle-ownership-and-local-changes.md),
[ADR-0009](0009-resolution-and-restoration-policy.md),
[ADR-0015](0015-manifest-composition-and-bundle-exports.md), and
[ADR-0016](0016-machines-follow-a-signed-managed-state.md).

## Context

A **content digest** is the one string fastskill uses to say "these exact files". Locks pin it
as a skill's `checksum`, bundle records pin it per member and per release, bundle artifacts carry
it in their embedded `skills.lock`, and install, update, restore and remove compare it with the
directory on disk before they change anything. Whether a locked restore installs the reviewed
content, and whether a bundle update overwrites local edits, both rest on it.

Until now the digest was SHA-256 over, for each file in name order, the relative path with its
length and then the file's bytes *without* their length. The resulting byte stream does not
decode to a single directory tree: the end of one file's contents and the start of the next
file's path cannot be told apart. Two different skill directories could therefore share a
digest, and a directory that differed from the one a Lock pinned could pass verification.

The digest had no version marker either, so the algorithm could not be changed without making
old and new values indistinguishable.

## Decision

### The current form

A content digest is written as `sha256-tree-v2:` followed by 64 lowercase hex characters: SHA-256
over

1. the domain tag `fastskill content digest v2`, and then
2. for every regular file, in byte order of its path: the path relative to the skill directory
   and then the file's contents.

Every field is preceded by its length as a big-endian `u64`, so the stream decodes to exactly one
tree. In addition:

- A path is its UTF-8 components joined with `/`. It is built from components, not by rewriting
  separators, so a Unix file name containing `\` stays distinct from a nested path.
- A file name that is not valid UTF-8 is refused: it has no spelling that means the same thing on
  every platform.
- Symbolic links are refused, as before.
- A file whose length changes while it is hashed is an error, not a digest of torn content.

The digest covers file paths and contents only. It does not cover empty directories, file modes
(including the executable bit) or timestamps; see Out of scope.

### Compatibility with digests written by older releases

A **legacy digest** is 64 bare lowercase hex characters. fastskill never writes one, but it still
accepts one that matches, so existing Locks and bundles keep installing:

- A recorded digest is checked with the algorithm its form names. A legacy value is checked with
  the old algorithm; any other value must equal the current-form digest. Unreadable content is an
  error whatever the form.
- Accepting a legacy value prints one warning per process saying the record is weaker and will be
  replaced.
- Every record fastskill writes uses the current form. When fastskill rewrites a record that held
  a legacy value, the rewrite stores the current form.
- Two recorded digests of the same form that differ are a conflict, as before. A legacy and a
  current value cannot be compared with each other, so each is checked against the content.

`project install --lock` never changes the Lock, so it verifies legacy values and keeps them. Where
digests are stored, and when a legacy value is replaced:

| Record | Written by | Legacy value replaced when |
|---|---|---|
| `skills.lock` `[[skills]].checksum` | `skill add`, `skill update`, `project install` for a newly resolved root | That entry is re-resolved. `project install --lock` verifies against the Lock and does not rewrite existing entries. |
| `global-skills.lock` `checksum` | `skill add --global`, `skill update --global` | That entry is reinstalled or updated. |
| `skills.lock` `[[bundles]]` release and member digests | `bundle add`, `bundle update`, `project install` | The bundle is added, updated or restored by `project install`. An unchanged `bundle add` leaves the entry alone. |
| `skills.lock` `[[overrides]].digest` | `bundle override --from` | The override is recorded again, or removing its last owning bundle turns it into an individual skill. |
| `.fastskill/bundle-history.toml` | every recorded bundle release | That release is recorded again. |
| `skills.lock` inside a bundle artifact, and its copy under `.fastskill/bundles/` | `bundle build` | Never: an artifact is immutable. Rebuild it with `bundle build`. |

A bundle artifact is accepted when its members and its release digest are all current or all
legacy. An artifact that mixes forms was not produced by any release and is refused. The release
digest algorithm is unchanged, but it hashes the member digests, so a rebuilt artifact has a new
release digest; fastskill treats both as the same release, because both name the same contents.

The bundle archive format (`fastskill-bundle-lock-v1`) and the Lock format version are not
changed. Changing either would make older releases refuse to read files they could otherwise
report on, with a message suggesting that the Lock be deleted.

## Tradeoffs

- **Legacy values are still trusted until they are rewritten.** A tree crafted to collide with a
  legacy digest passes where that digest is still recorded. Refusing legacy digests would close
  this, but every committed Lock and every bundle built by an earlier release would stop
  installing, including `project install --lock` in CI, with no way to verify the content they
  pinned. The warning makes the weaker record visible, and a rewrite removes it. A later release
  can refuse legacy digests once Locks have had time to be rewritten.
- **Older releases cannot verify current-form digests.** A Lock or artifact written by this
  release reports a checksum mismatch on an older fastskill. Every machine and CI job that shares
  a project must be upgraded together, as with manifest `schema_version = "2"`.
- **Non-UTF-8 file names are no longer installable.** They were rare, and their digest depended
  on how the platform converted them.

## Out of scope

- **The executable bit.** Windows cannot represent it, and a Lock written on one platform must
  verify on every other. Including it would need a platform-independent source for the mode
  (for example, the archive or git tree), which fastskill does not have for every Origin.
- **Empty directories.** Neither ZIP artifacts nor git preserve them consistently, so they cannot
  be part of what a digest promises.
- **Removing legacy acceptance.** That is a later decision, with its own migration notice.
