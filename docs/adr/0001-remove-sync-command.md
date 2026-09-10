# Remove the `sync` command

## Status

accepted

## Context & decision

`fastskill sync` wrote the set of installed skills into an agent metadata file (e.g. a Claude Code skills list) so that **agents that lacked native skill support** could discover them. Modern target agents read skills directly from the skills directory, so the metadata file — and the command that maintained it — no longer serve a purpose.

The removed path above is historical. [ADR-0010](0010-command-taxonomy.md) defines the current
command tree.

We **remove `sync` entirely** (breaking change) rather than keep it as a hidden/deprecated alias. Keeping it would imply the metadata-file path is still a supported integration, which it is not.

## Consequences

- Propagation now has exactly two members: `project install` (Manifest → skills directory) and
  `index rebuild` (skills directory → vector index). See
  [ADR-0002](./0002-conditional-semantic-indexing.md).
- Any tooling still reading the old agent metadata file must migrate to reading the skills directory directly.
- The `--agent` / `--agents-file` surface disappears with the command.

## Considered alternatives

- *Hide/deprecate instead of remove* — rejected: leaves a misleading supported surface and dead
  configuration for an integration FastSkill no longer endorses. The earlier `sources`,
  `registry`, and `repos` paths are also historical after ADR-0010; repository operations now use
  `repo`.
