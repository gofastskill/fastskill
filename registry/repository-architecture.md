# Choose a distribution model

FastSkill 0.9.230

Source: https://docs.gofastskill.com/registry/repository-architecture

Release revision: dd983e89e5fee977325b77ae386b879e8310ed26

Documentation revision: dd983e89e5fee977325b77ae386b879e8310ed26



| Need                                                    | Use                                                             |
| ------------------------------------------------------- | --------------------------------------------------------------- |
| Iterate on one skill locally                            | A local dependency, optionally editable                         |
| Install a skill from a source repository                | A Git origin with a branch or tag                               |
| Discover and select skills across a team                | A Git marketplace, local catalog, HTTP registry, or ZIP catalog |
| Share an exact skill set without original source access | A self-contained bundle                                         |

## Discovery and installation

Catalog search returns metadata and an install reference. It does not install a
skill. Adding a result records its origin and materializes the selected content.
Locks record the resolved selection so restoration does not silently pick a newer
catalog version or Git commit.

A project's manifest defines its repository configuration. Give each catalog a
unique name and use explicit repository selection where IDs overlap. See
[configuration examples](/registry/sources).

## Team reproducibility

Commit the manifest and lock together. Preserve any local source files and bundled
artifacts required for restoration. A fresh machine still needs network access for
uncached remote dependencies; `--offline` cannot download missing content.

For an artifact recipients can install without access to private source repositories,
[build a bundle](/cli-reference/bundle-command). Bundles retain ownership so removing
one package does not remove a skill that another package still requires.

## Trust

A digest proves that selected content has not changed; it does not establish that
the instructions or scripts are safe. Review sources before making them available
to agents. See the [security model](/security/model).

