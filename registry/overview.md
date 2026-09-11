# Sources and catalogs

FastSkill 0.9.229

Source: https://docs.gofastskill.com/registry/overview

Release revision: 68975dfaca15508bc5cf2fb761d6218405c954ea

Documentation revision: 68975dfaca15508bc5cf2fb761d6218405c954ea



A repository configuration gives FastSkill a named source for discovering and
installing skills. It is separate from the installed skill directory and from the
[local browser console](/registry/web-ui).

## Choose a source

* **Git marketplace**: a Git repository with a marketplace document.
* **HTTP registry**: an HTTP index and downloadable skill artifacts.
* **ZIP catalog**: a base URL exposing marketplace metadata and ZIP downloads.
* **Local catalog**: skill content and catalog metadata on disk.

See [source configuration](/registry/sources) for the manifest fields, authentication,
and direct Git origins. For an exact portable team setup, use
[bundles](/cli-reference/bundle-command).

## Discover and install

The following commands assume a configured repository named `team`; replace the
example skill ID with a result from that repository:

```bash
fastskill repo list
fastskill repo skills team
fastskill skill search "review" --repository team
fastskill skill add review-notes --repository team
fastskill skill list --check
```

Search discovers candidates; installation records a selected origin and content.
Commit the manifest and lock together so teammates can restore the intended state.
See [repository commands](/cli-reference/repository-command) for management actions
and [marketplace.json](/registry/marketplace-json) for catalog authoring.

