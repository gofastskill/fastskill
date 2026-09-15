# Manifest schema v2: normalize git origins, fix `/tree/` URL regression

Status: proposed — 2026-09-15
Owner: unassigned
Related: PR #323 (regression origin), PR #327 / #332 (docs that noted it), `docs/requirements/two-scope-lock-model.md`

## 1. Problem

`fastskill project install` fails on any `skill-project.toml` git dependency whose `url` is a
GitHub browser link (`https://github.com/org/repo/tree/<branch>/<subdir>`):

```
Error: Service error: Custom error: Failed to resolve ref for https://github.com/goagwiki/agwiki/tree/main/skill:
Custom error: Git command failed: fatal: repository 'https://github.com/goagwiki/agwiki/tree/main/skill/' not found
```

### How it broke

- Up to **v0.9.221**, `project install` used a CLI-owned git path
  (`crates/fastskill-cli/src/utils/install_utils.rs::install_from_git → clone_and_find_skill`)
  that ran `parse_git_url()` on the manifest `url`, cloned the extracted `repo_url`, and used
  the extracted branch/subdir when the manifest did not set them. Manifests written with
  `/tree/` links therefore worked, and older `skill add` wrote them that way.
- **PR #323** (merged 2026-09-09, shipped in **v0.9.222**) deleted that path (and its copies
  in `add/mod.rs` and `add/sources.rs`) and routed every install through
  `fastskill-core::core::install::InstallService::fetch_git`, which passes the manifest `url`
  verbatim to `git ls-remote` (`storage/git.rs::ls_remote`). The `/tree/` split now happens
  only in `skill add` (`commands/add/origin.rs::build_origin`), which stores the split form.
  Nothing was added on the manifest-read side for files already on disk.
- No test caught it: the only `/tree/` tests are unit tests of `parse_git_url` itself and the
  new `build_origin` coverage test. No integration/e2e test installs a `/tree/` URL from a
  manifest, and no fixture manifest contains one.

### What the fixed layout looks like

```toml
# before (v1, written by <= 0.9.221 `skill add`, or by hand)
[dependencies.agwiki.origin]
type = "git"
url = "https://github.com/goagwiki/agwiki/tree/main/skill"
[dependencies.agwiki.origin.ref]
branch = "main"

# after (v2)
[dependencies.agwiki.origin]
type = "git"
url = "https://github.com/goagwiki/agwiki.git"
subdir = "skill"
[dependencies.agwiki.origin.ref]
branch = "main"
```

## 2. Decision

Bump `MANIFEST_SCHEMA_VERSION` from `"1"` to `"2"`.

- **v2 contract:** every `Origin::Git.url` is a plain clone URL. Branch/tag live only in
  `ref`; the subdirectory lives only in `subdir`. A `/tree/` URL in a v2 file is an error.
- **Reading:** absent, `"1"`, and `"2"` are all accepted. Absent/`"1"` files are upgraded in
  memory (legacy pre-Origin → v1 → v2 chain). `"2"` is parsed strictly. Anything else is
  refused (existing behaviour).
- **Writing:** every save stamps `"2"`. Nothing writes v1 any more. Reading never writes
  (existing rule); the upgrade lands on disk the next time something saves the manifest.

Known consequence, accepted: once a new FastSkill saves a manifest, FastSkill ≤ 0.9.231
refuses to read it ("declares schema_version '2', which this FastSkill does not
understand"). Users on mixed versions must upgrade. Call this out in the release notes.

## 3. Design

### 3.1 Version dispatch (`crates/fastskill-core/src/core/manifest.rs`)

`SkillProjectToml::from_toml_str` currently matches `declared` against
`Some(MANIFEST_SCHEMA_VERSION) | Some(unknown) | None`. Change to:

| declared      | action                                                              |
|---------------|---------------------------------------------------------------------|
| `Some("2")`   | `parse_current` (strict). `validate_v2` rejects `/tree/` git URLs.  |
| `Some("1")`   | `parse_current`, then `upgrade_v1_to_v2`.                            |
| `None`        | existing current-first / legacy-fallback logic, then `upgrade_v1_to_v2`. |
| other         | refuse (unchanged message, mention both known versions).             |

Introduce a private `const MANIFEST_SCHEMA_V1: &str = "1"` so the `"1"` arm is not a magic
string and the doc comment on `MANIFEST_SCHEMA_VERSION` can name the upgrade chain.

### 3.2 `upgrade_v1_to_v2`

Iterates `dependencies` and rewrites each `DependencySpec::Inline { origin: Origin::Git {..} }`
whose `url` is a GitHub `/tree/` link. Reuse `core::origin_infer::parse_git_url` for the
split; do not write a second parser.

Rules per entry (`url`, explicit `ref`, explicit `subdir`):

1. If `url` has no `/tree/` segment: leave the entry untouched. Do **not** append `.git` or
   otherwise normalise plain URLs; v1 → v2 must be a no-op for already-correct files.
2. Otherwise let `tree_path` = everything after `/tree/`, and `repo_url` =
   `parse_git_url(url).repo_url`.
3. Determine branch and subdir:
   - If `ref` is `Branch(b)` or `Tag(t)`: the ref name `n` must equal `tree_path` or be a
     prefix `n + "/"` of it. If it is, keep the explicit ref and `subdir` = remainder (or
     `None`). If it is not, return `ManifestError::Parse` naming the dependency, the URL, and
     the explicit ref ("ref does not match the /tree/ path; fix one of them").
     This rule handles branch names containing `/` correctly when the author declared the
     branch — the only case where it can be known.
   - If `ref` is `Default`: use `parse_git_url`'s split (first segment = branch, rest =
     subdir). `ref` becomes `Branch(first_segment)`. Emit a `tracing::warn!` that the branch
     was inferred from the URL and that a branch containing `/` must be declared explicitly.
   - If `ref` is `Commit(_)`: keep the commit; derive subdir as in the `Default` case but do
     not touch `ref`. Warn.
4. If an explicit `subdir` is present and differs from the derived one: `ManifestError::Parse`
   (conflict). If equal: fine.
5. Replace `url` with `repo_url` (this carries a `.git` suffix, matching what `skill add`
   writes today — see `add/origin/coverage_tests.rs`).
6. `tracing::warn!` one line per rewritten entry:
   `manifest: rewrote git origin for '<id>': url=<new> subdir=<s> ref=<r> (schema 1 -> 2; will be saved on next write)`.

Non-GitHub hosts (`gitlab.com/-/tree/`, Gitea `src/branch/`) are out of scope; they were never
supported by `parse_git_url` and never worked.

### 3.3 `validate_v2`

Called only for declared `"2"`. Any `Origin::Git.url` with host `github.com` and a `/tree/`
path segment → `ManifestError::Parse` with the fixed TOML form in the message. Keep it to that
one check; do not add a general URL validator.

### 3.4 Writers

All writers already go through the constant, so the bump alone switches them to `"2"`:

- `SkillProjectToml::save_to_file` (`manifest.rs`)
- `project_state::save_project_preserving` (`project_state.rs:20`)
- `commands/init.rs:399`, `core/repository.rs:224` (construct with the constant)
- `http/handlers/manifest.rs:280` (reports the constant)

Verify nothing else hard-codes `"1"` (`grep -rn '"1"' crates --include=*.rs` around
`schema_version`).

### 3.5 Defensive install-time check

In `core/install.rs::fetch_git` (or at the top of `storage/git.rs::ls_remote`), if `url`
is a GitHub `/tree/` link, fail before shelling out with a message that shows the
`url` + `subdir` + `ref` form. This covers hand-edited v2 files and any caller that builds an
`Origin::Git` without going through the manifest loader. It replaces the opaque
"repository not found" error.

### 3.6 Lock / reconciliation interaction — must be verified

`skills.lock` and the reconciliation checks compare the manifest origin against what was
locked/installed. After the upgrade, a v1 manifest's `url` changes in memory
(`.../repo/tree/main/skill` → `.../repo.git` + `subdir`). Confirm, with a test, what
`project install` does when the lock was written from the v1 form:

- acceptable: one-time reinstall/relock with a clear message;
- not acceptable: a persistent "drift" report, or an install that silently keeps the old
  entry.

Look at `core/project_apply.rs`, `core/resolution/`, and `commands/list/reconciliation_tests.rs`
for where origins are compared. If the comparison is by full `Origin` equality, either
normalise both sides through the same function or treat a v1-form lock entry as matching its
upgraded manifest entry.

## 4. Tasks

Do them in this order; each step should leave `cargo test --workspace` green.

1. **Regression test first** (`crates/fastskill-core/src/core/manifest/schema_version_tests.rs`):
   a v1 manifest with a `/tree/main/skill` git origin parses to `Origin::Git { url: ".../repo.git",
   subdir: Some("skill"), ref: Branch("main") }`. It fails today.
2. **Bump** `MANIFEST_SCHEMA_VERSION` to `"2"`, add `MANIFEST_SCHEMA_V1`, update the doc
   comment (§3.1). Fix the existing schema tests that assert the literal `"1"`
   (`schema_version_tests.rs`, `manifest/tests.rs:28`, `project_state.rs` tests).
3. **Implement** `upgrade_v1_to_v2` (§3.2) and wire the dispatch (§3.1).
4. **Implement** `validate_v2` (§3.3).
5. **Install-time guard** (§3.5).
6. **Lock/reconciliation check** (§3.6), as two tests, because tests must not touch the
   network and the local git daemon (`crates/fastskill-core/tests/common/mod.rs`) serves
   `git://127.0.0.1:<port>/...`, which has no `/tree/` form:
   - CLI-level: a v1 manifest with an `https://github.com/.../tree/main/skill` origin; run
     a command that loads and saves the manifest without installing (e.g. `skill remove` of
     another entry, or `project init`-style rewrite); assert the file is now v2 with the
     split fields and the tree entry's `url` no longer contains `/tree/`.
   - Daemon-backed: manifest + lock for a daemon repo written in the v1 *shape*
     (`url` + `ref.branch`, subdir given explicitly), then `project install`; assert success
     and that the lock is either unchanged or relocked once, with no lingering drift report.
     Put it in `crates/fastskill-cli/tests/lifecycle_install_test.rs` or
     `tests/cli/install_e2e_tests.rs`, whichever already has the daemon fixture wired.
7. **Docs**: `README.md:204`, `webdocs/cli-reference/init-command.mdx:95` (`schema_version = "2"`),
   `webdocs/registry/sources.mdx` (add a short "migrating from `/tree/` URLs" note stating the
   auto-upgrade and the ≤ 0.9.231 compatibility break), and a changelog/release-notes entry
   if the repo keeps one.
8. **Release note** text (see §2, known consequence).

## 5. Tests (minimum)

Unit (`manifest/schema_version_tests.rs`):

- v1 + `/tree/<b>/<sub>` + no ref → Branch(b), subdir sub, url `.git`, warn.
- v1 + `/tree/<b>/<sub>` + `ref.branch = b` → unchanged ref, subdir sub.
- v1 + `/tree/feature/x/skills/foo` + `ref.branch = "feature/x"` → subdir `skills/foo`.
- v1 + `/tree/<b>/<sub>` + `ref.branch = other` → `ManifestError::Parse` naming the id.
- v1 + `/tree/<b>/<sub>` + `subdir = <sub>` → ok; `subdir = different` → error.
- v1 + `/tree/<b>` (no subdir) → Branch(b), subdir None.
- v1 + `ref.tag = t`, url `/tree/t/sub` → Tag(t) kept, subdir sub.
- v1 + plain `https://…/repo` (no `.git`) → **untouched** (no `.git` appended).
- v1 + local / zip-url / repository origins → untouched.
- absent version + `/tree/` URL → upgraded the same way (unstamped files).
- legacy pre-Origin + `source = "git"` + `/tree/` url → upgraded through both steps.
- declared `"2"` + `/tree/` URL → error with the fixed form in the message.
- declared `"3"` → refused; message lists known versions.
- save after loading v1 → file contains `schema_version = "2"` and the split fields;
  `save_project_preserving` behaves the same and keeps unknown tables.

Integration (§4 step 6). Also add one `fetch_git`/`ls_remote` unit test for the §3.5 guard
(no network: assert the error is produced before any git invocation, e.g. by checking the
message and that `CLONE_INVOCATIONS` is unchanged).

## 6. Acceptance criteria

- The manifest in the problem statement installs with no edits on the new binary.
- A v2 file written by the new binary round-trips through `load → save` byte-stable
  (modulo the existing serializer's formatting).
- Clippy, tests, and the coverage gate pass on Linux and Windows CI (the repo enforces
  coverage on changed code; keep the new code unit-tested).
- No `"1"` literal remains for the manifest schema outside the `MANIFEST_SCHEMA_V1` const
  and its tests.

## 7. Out of scope / follow-ups (file separately)

- `core/sources/manager.rs:334` builds `https://github.com/{repo}/tree/main/{path}` as a
  marketplace `download_url`, and `repository/client/marketplace_acquisition.rs:47` does a
  plain HTTP GET on it, which returns GitHub HTML, not an archive. Same family of bug,
  different code path.
- A workspace manifest in the wild contained
  `cached-git-skill = { origin = { type = "git", url = "git://127.0.0.1:43081/moving" } }`,
  which is the shape of the test git-daemon URLs. Some test appears to write into the
  real project's `skill-project.toml` when run from a checkout inside a FastSkill project.
  Find it and make it use a temp project root.
- Supporting non-GitHub browser URLs (`/-/tree/`, `/src/branch/`) in `parse_git_url`.

## 8. Pointers

- Regression origin: `git show b0e7d750` (PR #323); last good tag `v0.9.221`, first bad `v0.9.222`.
- Manifest loader/dispatch: `crates/fastskill-core/src/core/manifest.rs` (`from_toml_str`,
  `parse_current`, `save_to_file`, `LegacySkillProjectToml::upgrade`).
- URL split: `crates/fastskill-core/src/core/origin_infer.rs::parse_git_url` (+ `GitUrlInfo`).
- Preserving writer: `crates/fastskill-core/src/core/project_state.rs::save_project_preserving`.
- Install path: `crates/fastskill-core/src/core/install.rs::fetch_git` →
  `install/support.rs::resolve_git_sha` → `storage/git.rs::ls_remote`.
- `skill add` split (reference behaviour): `crates/fastskill-cli/src/commands/add/origin.rs::build_origin`
  and `add/origin/coverage_tests.rs`.
- Existing schema tests: `crates/fastskill-core/src/core/manifest/schema_version_tests.rs`.
- Repo rules: `CLAUDE.md` (branch off `origin/main`, arm auto-merge, never bypass checks).
