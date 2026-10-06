# Configure skill sources

FastSkill 0.9.264

Source: https://docs.gofastskill.com/registry/sources

Release revision: 77e1ecdf9d221ea023b0d92e10be7bb2554e1481

Documentation revision: 77e1ecdf9d221ea023b0d92e10be7bb2554e1481



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

Use this manifest form as the portable source of truth for subdirectories. It keeps the clone URL,
branch, and subdirectory explicit in committed project state.
FastSkill uses system Git; install Git and ensure it can access the repository before
adding private sources. Git credentials come from your Git configuration.

### Migrating from `/tree/` URLs

Manifests written by fastskill 0.9.221 and earlier recorded a GitHub browser link as the
git `url`, e.g., `https://github.com/your-org/skills/tree/main/review-notes`. Git cannot
clone such a URL. Manifest `schema_version = "2"` splits it into the three fields shown
above, and fastskill performs that split while reading — no manual edit is needed. The
rewritten form reaches disk the next time a command saves the manifest, e.g.,
`skill add` or `skill remove` (`project install` reads it and updates only `skills.lock`). A `/tree/` URL in a manifest that
already declares `schema_version = "2"` is an error, and the message shows the corrected
TOML.

One consequence is worth planning for: a manifest saved by a current fastskill declares
`schema_version = "2"`, and fastskill 0.9.232 and earlier refuse to read it. Upgrade every
machine and CI job that shares the project before the first save.

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

Relative local repository paths are resolved from the directory containing
`skill-project.toml`. They therefore keep the same meaning when FastSkill is invoked from a
nested project directory.

`zip_url` is the manifest field for a ZIP catalog's base URL. Direct ZIP dependency
origins use `origin.type = "zip-url"` and `origin.url` instead. These are different
configuration objects.

Omit `auth` for public catalogs. Never put a token in a committed manifest:
`auth` names where the token comes from, not the token itself. Repository auth does
not replace system Git authentication.

### Registry authentication

| `auth.type` | Token source                    | Header sent               | Where it may be configured        |
| ----------- | ------------------------------- | ------------------------- | --------------------------------- |
| `pat`       | `env_var`                       | `Authorization: token …`  | `skill-project.toml` or user file |
| `bearer`    | `env_var`                       | `Authorization: Bearer …` | `skill-project.toml` or user file |
| `command`   | first line printed by `command` | `Authorization: Bearer …` | user file only                    |

`bearer` and `command` apply to `http-registry` repositories. With either one, a
missing variable or a failing command is an error; FastSkill never retries the
request without credentials. `pat` keeps its existing behaviour and sends no header
when its variable is unset.

For a registry that requires a bearer token, name the variable in the manifest:

```toml
[[tool.fastskill.repositories]]
name = "private"
type = "http-registry"
priority = 0
index_url = "https://registry.example.com/index"
auth = { type = "bearer", env_var = "REGISTRY_TOKEN" }
```

When the token is short-lived, let a program print it, such as an identity provider
CLI or a CI job's token exchange. A credential command can only be configured in your
user repositories file, `repositories.toml` in FastSkill's configuration directory
(`$XDG_CONFIG_HOME/fastskill/` when set, otherwise the platform config directory). A
`skill-project.toml` that names a command is refused, so cloning a project can never
make FastSkill run a program.

```toml
# ~/.config/fastskill/repositories.toml
[[repositories]]
name = "private"
type = "http-registry"
priority = 0
index_url = "https://registry.example.com/index"
auth = { type = "command", command = ["my-login", "token"] }
```

```bash
fastskill repo add private https://registry.example.com/index --repo-type http-registry \
  --user --auth-type command --credential-command my-login --credential-arg token
```

A user entry replaces a project entry with the same name, for listing, resolution
and install alike. How the command runs:

* It runs directly, without a shell, from FastSkill's configuration directory, and is
  stopped after 30 seconds.
* The token is the first line of standard output with the trailing line ending
  removed. Output over 16 KiB, an empty token, or a non-zero exit is an error. Errors
  name the program and its exit status, never the token.
* Standard error reaches your terminal only when there is one, so the command can
  prompt you to sign in. When standard input or standard error is not a terminal,
  FastSkill sets `FASTSKILL_INTERACTIVE=0`: the command must then print a token
  without prompting, or exit non-zero.
* It runs at most once per FastSkill process for each repository. The token stays in
  memory; FastSkill never writes it to disk or prints it.

The token is sent only to the origin (scheme, host and port) of `index_url`. A
download on another origin gets no token and must carry its own authorization, such
as a pre-signed URL. A redirect to another origin is followed without the token, and
a redirect from `https` to `http` is refused.

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

