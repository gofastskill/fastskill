# Configure skill sources

FastSkill 0.9.229

Source: https://docs.gofastskill.com/registry/sources

Release revision: 68975dfaca15508bc5cf2fb761d6218405c954ea

Documentation revision: 68975dfaca15508bc5cf2fb761d6218405c954ea



Complete the [quickstart](/quickstart) first. Commands below use example URLs and
IDs: replace them with a repository you can access and a skill it actually contains.

## Install directly from Git

```bash
fastskill skill add https://github.com/your-org/your-skill.git --branch main
fastskill skill add https://github.com/your-org/your-skill.git --tag v1.0.0
```

Use one ref selector for a source. For a skill in a subdirectory, declare the clean
clone URL and `subdir` in the manifest, then install it:

```toml
[dependencies]
review-notes = { origin = { type = "git", url = "https://github.com/your-org/skills.git", subdir = "review-notes", ref = { branch = "main" } } }
```

```bash
fastskill project install --no-reindex
```

Use this manifest form for subdirectories: GitHub browser `/tree/…` URLs fail ref
resolution in the current release.
FastSkill uses system Git; install Git and ensure it can access the repository before
adding private sources. Git credentials come from your Git configuration.

The manifest records the requested origin/ref; the lock records the selected commit
and integrity evidence. `project install --lock` restores that selection even when
a branch advances. `skill update` deliberately refreshes the origin. Repository
version/strategy controls do not apply to Git origins.

```bash
fastskill skill update review-notes --dry-run
fastskill skill update review-notes
fastskill project install --lock
```

Offline restoration needs the verified source content in the local cache. A lockfile
alone does not contain the source files. Run an online install first, or distribute
self-contained [bundles](/cli-reference/bundle-command).

## Configure a catalog

A catalog lets you discover skills and add them by their canonical IDs. Each
repository is an entry in `[[tool.fastskill.repositories]]` in `skill-project.toml`.
Lower priority numbers are searched first. Explicit `--repository NAME` selection
avoids ambiguity when several catalogs offer the same ID.

```toml
[tool.fastskill]
skills_directory = ".claude/skills"

[[tool.fastskill.repositories]]
name = "team"
type = "git-marketplace"
priority = 0
url = "https://github.com/your-org/skills.git"
branch = "main"

[[tool.fastskill.repositories]]
name = "local"
type = "local"
priority = 1
path = "./catalog"

[[tool.fastskill.repositories]]
name = "registry"
type = "http-registry"
priority = 2
index_url = "https://example.com/index.json"
auth = { type = "pat", env_var = "PAT_TOKEN" }

[[tool.fastskill.repositories]]
name = "archive"
type = "zip-url"
priority = 3
zip_url = "https://example.com/skills/"
```

`zip_url` is the manifest field for a ZIP catalog's base URL. Direct ZIP dependency
origins use `origin.type = "zip-url"` and `origin.url` instead. These are different
configuration objects.

The manifest's repository authentication supports `pat` with a token environment
variable. Omit `auth` for public catalogs. Do not put credentials in a committed
manifest. This setting does not replace system Git authentication.

You can also manage sources through the CLI:

```bash
fastskill repo add team --repo-type git-marketplace https://github.com/your-org/skills.git
fastskill repo list
fastskill repo info team
fastskill repo test team
fastskill repo skills team
fastskill skill search "review" --repository team
fastskill skill add review-notes --repository team
```

Search results identify the repository, canonical skill ID, version, and install
command. Remote search can fail partially; inspect the exit status and diagnostics
instead of treating partial results as a complete catalog response.

## Publish a catalog

Git marketplaces and ZIP catalogs describe skills using
[marketplace.json](/registry/marketplace-json). HTTP registries expose an
[index](/registry/index-system). FastSkill does not provide a CLI publish service;
host catalog files and artifacts using your distribution infrastructure.

For sharing an already installed set without access to its original sources, use
[bundles](/cli-reference/bundle-command). For all repository actions and flags, see
[repository reference](/cli-reference/repository-command).

