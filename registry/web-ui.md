# Local browser console

FastSkill 0.9.229

Source: https://docs.gofastskill.com/registry/web-ui

Release revision: 68975dfaca15508bc5cf2fb761d6218405c954ea

Documentation revision: 68975dfaca15508bc5cf2fb761d6218405c954ea



Complete the [quickstart](/quickstart) or open an existing configured project, then run:

```bash
fastskill server serve
```

Open `http://localhost:8080/`. The console loads project information and the installed
skills table. Type in the search field to filter the loaded rows; select a skill
for its details. Catalog discovery is available through [repository commands](/cli-reference/repository-command)
and the registry HTTP endpoints.

## Write operations

The console reads server capabilities before exposing write controls. Start with
write access only when you intend to change installed skills:

```bash
fastskill server serve --enable-write
```

With writes enabled, the console provides an install form, eligible skill updates,
removal controls, and an update-all action. Reindexing additionally requires an
embedding provider. A successful operation refreshes the displayed state; inspect
errors when a source cannot resolve or ownership prevents removal.

The server has no built-in authentication. Use it locally, or put authentication
and network restrictions in front of it when exposing it to other users. Read-only
access still reveals skill content. See the [security model](/security/model).

## Troubleshooting

* If the table does not load, check that the server is running in the correct project
  and inspect `fastskill cli doctor` output.
* If write controls are absent, verify that you started with `--enable-write`.
* If reindexing is unavailable, configure the optional [embedding provider](/cli-reference/reindex-command).
* If a skill cannot be removed, inspect its [owners and reconciliation state](/skill-management/reconciliation).

For the HTTP contract, health checks, and server flags, see
[server reference](/cli-reference/serve-command).

