# Discover and inspect skills

FastSkill 0.9.246

Source: https://docs.gofastskill.com/cli-reference/discovery-commands

Release revision: 2b2672fc852933d9a91bb0c7a8f76577e7aef5ed

Documentation revision: 2b2672fc852933d9a91bb0c7a8f76577e7aef5ed



Complete the [quickstart](/quickstart) for local skills, or configure a real
[catalog](/registry/sources). Examples below use placeholder repository and skill IDs.

## Search installed skills

```bash
fastskill skill search "meeting" --local --embedding false --json
fastskill skill list --check
fastskill skill read review-notes --meta
```

Local keyword search needs no API key. Semantic search is optional and requires an
[index and embedding provider](/cli-reference/reindex-command).

## Search a catalog

```bash
fastskill skill search "review" --repository team --json
fastskill repo skills team
fastskill repo show review-notes --repository team
fastskill repo versions review-notes --repository team
fastskill skill add review-notes --repository team
```

Remote search is the default when `--local` is absent. Results identify the
canonical skill ID, repository, version, and installation command. Empty catalogs,
missing configuration, and partial failures are distinct outcomes; inspect both
JSON diagnostics and exit status.

See [search reference](/cli-reference/search-command) for filters and output formats,
and [repository reference](/cli-reference/repository-command) for catalog operations.

