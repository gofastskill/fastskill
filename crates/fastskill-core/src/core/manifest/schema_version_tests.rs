use super::*;

fn origin_of<'a>(p: &'a SkillProjectToml, id: &str) -> &'a Origin {
    match p.dependencies.as_ref().unwrap().dependencies.get(id) {
        Some(DependencySpec::Inline { origin, .. }) => origin,
        other => panic!("expected inline origin for {id}, got {other:?}"),
    }
}

/// The whole point: a manifest written before this feature existed must keep working.
#[test]
fn legacy_manifest_without_schema_version_is_upgraded_in_memory() {
    let legacy = r#"
[metadata]
id = "ws"
version = "1.0.0"

[dependencies.codescene]
source = "git"
url = "https://github.com/org/repo"
branch = "main"

[dependencies.paper-trail]
source = "local"
path = "/srv/skills/paper-trail"
editable = true

[dependencies.newton]
source = "git"
url = "https://github.com/gonewton/skill"

[dependencies.bundled]
source = "zip-url"
zip_url = "https://example.com/s.zip"

[dependencies.from-repo]
source = "source"
name = "internal"
skill = "helper"
"#;
    let parsed = SkillProjectToml::from_toml_str(legacy).expect("legacy manifest must parse");

    assert_eq!(
        parsed.schema_version.as_deref(),
        Some(MANIFEST_SCHEMA_VERSION),
        "upgrading must stamp the current schema version"
    );

    match origin_of(&parsed, "codescene") {
        Origin::Git { url, r#ref, subdir } => {
            assert_eq!(url, "https://github.com/org/repo");
            assert_eq!(*r#ref, GitRef::Branch("main".to_string()));
            assert!(subdir.is_none());
        }
        other => panic!("expected git origin, got {other:?}"),
    }

    // No branch in legacy meant "the repository default", not a branch literally named
    // something. Getting this wrong would silently re-point a dependency.
    match origin_of(&parsed, "newton") {
        Origin::Git { r#ref, .. } => assert_eq!(*r#ref, GitRef::Default),
        other => panic!("expected git origin, got {other:?}"),
    }

    // `editable` moved from beside `source` into `Origin::Local`.
    match origin_of(&parsed, "paper-trail") {
        Origin::Local { path, editable } => {
            assert_eq!(path, &PathBuf::from("/srv/skills/paper-trail"));
            assert!(*editable, "editable = true must survive the upgrade");
        }
        other => panic!("expected local origin, got {other:?}"),
    }

    match origin_of(&parsed, "bundled") {
        Origin::ZipUrl { url } => assert_eq!(url, "https://example.com/s.zip"),
        other => panic!("expected zip-url origin, got {other:?}"),
    }

    // Legacy `source = "source"` meant "from a configured repository".
    match origin_of(&parsed, "from-repo") {
        Origin::Repository { repo, skill, .. } => {
            assert_eq!(repo, "internal");
            assert_eq!(skill, "helper");
        }
        other => panic!("expected repository origin, got {other:?}"),
    }
}

/// The v1 → v2 regression: `skill add` up to 0.9.221 wrote the GitHub browser link
/// verbatim, and git cannot clone it. Reading such a manifest must produce the split
/// form instead, or `project install` fails with "repository not found".
#[test]
fn v1_tree_url_is_split_into_clone_url_subdir_and_branch() {
    let v1 = r#"
schema_version = "1"

[dependencies.agwiki.origin]
type = "git"
url = "https://github.com/goagwiki/agwiki/tree/main/skill"
"#;
    let parsed = SkillProjectToml::from_toml_str(v1).expect("a v1 manifest must still parse");
    match origin_of(&parsed, "agwiki") {
        Origin::Git { url, r#ref, subdir } => {
            assert_eq!(url, "https://github.com/goagwiki/agwiki.git");
            assert_eq!(*r#ref, GitRef::Branch("main".to_string()));
            assert_eq!(subdir.as_deref(), Some(Path::new("skill")));
        }
        other => panic!("expected git origin, got {other:?}"),
    }
}

/// A v1 manifest whose single dependency `dep` is spelled by `entry`.
fn v1_with(entry: &str) -> String {
    format!("schema_version = \"1\"\n\n[dependencies.dep]\n{entry}\n")
}

fn git_origin_of(parsed: &SkillProjectToml) -> (&str, &GitRef, Option<&Path>) {
    match origin_of(parsed, "dep") {
        Origin::Git { url, r#ref, subdir } => (url, r#ref, subdir.as_deref()),
        other => panic!("expected git origin, got {other:?}"),
    }
}

/// An explicitly declared branch is kept as-is; only the subdirectory is recovered.
#[test]
fn v1_tree_url_keeps_a_matching_explicit_branch() {
    let parsed = SkillProjectToml::from_toml_str(&v1_with(
        "origin = { type = \"git\", url = \"https://github.com/org/repo/tree/main/skill\", \
         ref = { branch = \"main\" } }",
    ))
    .unwrap();
    assert_eq!(
        git_origin_of(&parsed),
        (
            "https://github.com/org/repo.git",
            &GitRef::Branch("main".to_string()),
            Some(Path::new("skill"))
        )
    );
}

/// A branch containing `/` can only be split correctly when the author declared it, which
/// is exactly why the declared ref — not the URL's first path segment — drives the split.
#[test]
fn v1_tree_url_splits_on_a_declared_branch_containing_a_slash() {
    let parsed = SkillProjectToml::from_toml_str(&v1_with(
        "origin = { type = \"git\", url = \"https://github.com/org/repo/tree/feature/x/skills/foo\", \
         ref = { branch = \"feature/x\" } }",
    ))
    .unwrap();
    assert_eq!(
        git_origin_of(&parsed),
        (
            "https://github.com/org/repo.git",
            &GitRef::Branch("feature/x".to_string()),
            Some(Path::new("skills/foo"))
        )
    );
}

/// A tag is a ref too, and must survive the split untouched.
#[test]
fn v1_tree_url_keeps_a_declared_tag() {
    let parsed = SkillProjectToml::from_toml_str(&v1_with(
        "origin = { type = \"git\", url = \"https://github.com/org/repo/tree/v1.2.0/skill\", \
         ref = { tag = \"v1.2.0\" } }",
    ))
    .unwrap();
    assert_eq!(
        git_origin_of(&parsed),
        (
            "https://github.com/org/repo.git",
            &GitRef::Tag("v1.2.0".to_string()),
            Some(Path::new("skill"))
        )
    );
}

/// A tree URL naming only a branch leaves `subdir` unset — the repository root is the skill.
#[test]
fn v1_tree_url_without_a_subdirectory_leaves_subdir_unset() {
    let parsed = SkillProjectToml::from_toml_str(&v1_with(
        "origin = { type = \"git\", url = \"https://github.com/org/repo/tree/main\" }",
    ))
    .unwrap();
    assert_eq!(
        git_origin_of(&parsed),
        (
            "https://github.com/org/repo.git",
            &GitRef::Branch("main".to_string()),
            None
        )
    );
}

/// An explicit `subdir` that agrees with the URL is redundant, not wrong.
#[test]
fn v1_tree_url_accepts_an_agreeing_explicit_subdir() {
    let parsed = SkillProjectToml::from_toml_str(&v1_with(
        "origin = { type = \"git\", url = \"https://github.com/org/repo/tree/main/skill\", \
         subdir = \"skill\" }",
    ))
    .unwrap();
    assert_eq!(
        git_origin_of(&parsed),
        (
            "https://github.com/org/repo.git",
            &GitRef::Branch("main".to_string()),
            Some(Path::new("skill"))
        )
    );
}

/// Two sources of truth that disagree are reported, never resolved by guessing: a
/// dependency silently pointing somewhere else is the failure mode worth avoiding.
#[test]
fn v1_tree_url_conflicting_with_its_declared_fields_is_an_error() {
    for (entry, expected) in [
        (
            "origin = { type = \"git\", url = \"https://github.com/org/repo/tree/main/skill\", \
             ref = { branch = \"release\" } }",
            "release",
        ),
        (
            "origin = { type = \"git\", url = \"https://github.com/org/repo/tree/main/skill\", \
             subdir = \"other\" }",
            "other",
        ),
    ] {
        let error = SkillProjectToml::from_toml_str(&v1_with(entry))
            .expect_err("a conflict must not be resolved by guessing")
            .to_string();
        assert!(
            error.contains("dep"),
            "error must name the dependency: {error}"
        );
        assert!(
            error.contains(expected),
            "error must show the conflict: {error}"
        );
    }
}

/// v1 → v2 must be a no-op for files that were already correct — including not "helpfully"
/// appending `.git`, which would make the manifest differ from what the lock recorded.
#[test]
fn v1_origins_without_a_tree_url_are_left_exactly_as_they_are() {
    for entry in [
        "origin = { type = \"git\", url = \"https://github.com/org/repo\" }",
        "origin = { type = \"git\", url = \"https://gitlab.com/org/repo/-/tree/main/skill\" }",
        "origin = { type = \"local\", path = \"./skills/demo\" }",
        "origin = { type = \"zip-url\", url = \"https://example.com/s.zip\" }",
        "origin = { type = \"repository\", repo = \"team\", skill = \"demo\" }",
    ] {
        let source = v1_with(entry);
        let upgraded = SkillProjectToml::from_toml_str(&source).unwrap();
        let untouched = SkillProjectToml::from_toml_str(&source.replace(
            "schema_version = \"1\"",
            &format!("schema_version = \"{MANIFEST_SCHEMA_VERSION}\""),
        ))
        .unwrap();
        assert_eq!(
            origin_of(&upgraded, "dep"),
            origin_of(&untouched, "dep"),
            "upgrading must not rewrite: {entry}"
        );
    }
}

/// Unstamped and pre-`Origin` files reach v2 through the same upgrade, so a manifest that
/// nothing ever stamped is fixed too.
#[test]
fn unstamped_and_legacy_tree_urls_are_upgraded_the_same_way() {
    let expected = (
        "https://github.com/org/repo.git",
        &GitRef::Branch("main".to_string()),
        Some(Path::new("skill")),
    );
    let unstamped = SkillProjectToml::from_toml_str(
        "[dependencies.dep]\norigin = { type = \"git\", \
         url = \"https://github.com/org/repo/tree/main/skill\" }\n",
    )
    .unwrap();
    assert_eq!(git_origin_of(&unstamped), expected);

    let legacy = SkillProjectToml::from_toml_str(
        "[dependencies.dep]\nsource = \"git\"\n\
         url = \"https://github.com/org/repo/tree/main/skill\"\nbranch = \"main\"\n",
    )
    .unwrap();
    assert_eq!(git_origin_of(&legacy), expected);
}

/// A file that declares v2 is taken at its word: a tree URL in it is an authoring mistake,
/// and the error carries the TOML that fixes it.
#[test]
fn a_declared_v2_manifest_rejects_a_tree_url_and_shows_the_fix() {
    let source = format!(
        "schema_version = \"{MANIFEST_SCHEMA_VERSION}\"\n\n[dependencies.dep]\n\
         origin = {{ type = \"git\", url = \"https://github.com/org/repo/tree/main/skill\" }}\n"
    );
    let error = SkillProjectToml::from_toml_str(&source)
        .expect_err("a declared v2 manifest must not carry a browser url")
        .to_string();
    assert!(error.contains("dep"), "{error}");
    assert!(
        error.contains("url = \"https://github.com/org/repo.git\""),
        "the error must show the corrected url: {error}"
    );
    assert!(
        error.contains("subdir = \"skill\"") && error.contains("branch = \"main\""),
        "the error must show the split fields: {error}"
    );
}

/// Saving a loaded v1 manifest is what lands the upgrade on disk — through either writer.
#[test]
fn saving_an_upgraded_v1_manifest_writes_the_v2_form() {
    let dir = tempfile::tempdir().unwrap();
    for (name, save) in [
        (
            "plain.toml",
            &(|path: &Path, project: &SkillProjectToml| project.save_to_file(path))
                as &dyn Fn(&Path, &SkillProjectToml) -> Result<(), ManifestError>,
        ),
        ("preserving.toml", &|path, project| {
            crate::core::project_state::save_project_preserving(path, project)
        }),
    ] {
        let path = dir.path().join(name);
        std::fs::write(
            &path,
            v1_with(
                "origin = { type = \"git\", url = \"https://github.com/org/repo/tree/main/skill\" }",
            ) + "\n[extension]\nkeep = true\n",
        )
        .unwrap();

        let loaded = SkillProjectToml::load_from_file(&path).unwrap();
        save(&path, &loaded).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            written.contains(&format!("schema_version = \"{MANIFEST_SCHEMA_VERSION}\"")),
            "{name} must be stamped v2:\n{written}"
        );
        assert!(
            !written.contains("/tree/"),
            "{name} must no longer carry a browser url:\n{written}"
        );
        assert!(
            written.contains("https://github.com/org/repo.git") && written.contains("skill"),
            "{name} must carry the split fields:\n{written}"
        );
        // Re-reading the rewritten file must take the strict v2 path without complaint.
        SkillProjectToml::load_from_file(&path).unwrap();
    }

    let preserved = std::fs::read_to_string(dir.path().join("preserving.toml")).unwrap();
    assert!(
        preserved.contains("[extension]"),
        "the preserving writer must keep unknown tables:\n{preserved}"
    );
}

/// A modern file that nobody has stamped yet must NOT be mistaken for legacy.
#[test]
fn unstamped_current_format_is_parsed_as_current() {
    let current = r#"
[metadata]
id = "ws"
version = "1.0.0"

[dependencies.a]
origin = { type = "git", url = "https://github.com/org/repo" }
"#;
    let parsed = SkillProjectToml::from_toml_str(current).expect("current format must parse");
    match origin_of(&parsed, "a") {
        Origin::Git { url, .. } => assert_eq!(url, "https://github.com/org/repo"),
        other => panic!("expected git origin, got {other:?}"),
    }
}

#[test]
fn a_stamped_current_manifest_round_trips() {
    let src = format!(
        "schema_version = \"{MANIFEST_SCHEMA_VERSION}\"\n\n\
             [metadata]\nid = \"ws\"\nversion = \"1.0.0\"\n\n\
             [dependencies.a]\norigin = {{ type = \"local\", path = \"/x\" }}\n"
    );
    let parsed = SkillProjectToml::from_toml_str(&src).unwrap();
    assert_eq!(
        parsed.schema_version.as_deref(),
        Some(MANIFEST_SCHEMA_VERSION)
    );
}

/// A newer file must be refused, not guessed at — misreading it would corrupt it on save.
#[test]
fn unknown_schema_version_is_refused() {
    let future = r#"
schema_version = "99"

[metadata]
id = "ws"
version = "1.0.0"
"#;
    let err = SkillProjectToml::from_toml_str(future)
        .expect_err("a future schema version must not be silently accepted");
    let msg = err.to_string();
    assert!(msg.contains("99"), "error should name the version: {msg}");
    assert!(
        msg.contains("newer FastSkill"),
        "error should explain the likely cause: {msg}"
    );
    assert!(
        msg.contains("'1'") && msg.contains("'2'"),
        "error should list the versions this build knows: {msg}"
    );
}

/// Saving stamps the version even when the in-memory value never had one, so no writer
/// can forget to migrate.
#[test]
fn saving_stamps_the_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skill-project.toml");

    let unstamped = SkillProjectToml {
        schema_version: None,
        metadata: None,
        dependencies: None,
        tool: None,
    };
    unstamped.save_to_file(&path).unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.contains(&format!("schema_version = \"{MANIFEST_SCHEMA_VERSION}\"")),
        "save must stamp the schema version, got:\n{written}"
    );
    // And the scalar must precede the tables, or TOML would nest it inside one.
    assert!(
        written.trim_start().starts_with("schema_version"),
        "schema_version must be written before any table, got:\n{written}"
    );
}

/// End-to-end: read legacy -> save -> re-read yields the current format. This is the
/// "migrate at first write" behaviour as a user would experience it.
#[test]
fn legacy_file_becomes_current_after_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skill-project.toml");
    std::fs::write(
            &path,
            "[metadata]\nid = \"ws\"\nversion = \"1.0.0\"\n\n\
             [dependencies.codescene]\nsource = \"git\"\nurl = \"https://github.com/o/r\"\nbranch = \"main\"\n",
        )
        .unwrap();

    let loaded = SkillProjectToml::load_from_file(&path).unwrap();
    loaded.save_to_file(&path).unwrap();

    let rewritten = std::fs::read_to_string(&path).unwrap();
    assert!(
        !rewritten.contains("source = \"git\""),
        "legacy spelling must be gone:\n{rewritten}"
    );
    assert!(
        rewritten.contains("schema_version"),
        "must be stamped:\n{rewritten}"
    );

    // Re-reading the rewritten file must take the current path and preserve the origin.
    let reloaded = SkillProjectToml::load_from_file(&path).unwrap();
    match origin_of(&reloaded, "codescene") {
        Origin::Git { url, r#ref, .. } => {
            assert_eq!(url, "https://github.com/o/r");
            assert_eq!(*r#ref, GitRef::Branch("main".to_string()));
        }
        other => panic!("expected git origin, got {other:?}"),
    }
}

/// A legacy entry missing its required field must fail loudly. Silently resolving a
/// dependency to the wrong place is worse than refusing to migrate it.
#[test]
fn legacy_entry_missing_required_field_is_reported_not_guessed() {
    let broken = r#"
[metadata]
id = "ws"
version = "1.0.0"

[dependencies.oops]
source = "git"
branch = "main"
"#;
    let err = SkillProjectToml::from_toml_str(broken).expect_err("must not silently migrate");
    let msg = err.to_string();
    assert!(
        msg.contains("oops") || msg.contains("url"),
        "unhelpful error: {msg}"
    );
}

/// A bare version string means the same in both formats and must pass through.
#[test]
fn bare_version_string_dependencies_survive() {
    let legacy = "[metadata]\nid = \"ws\"\nversion = \"1.0.0\"\n\n[dependencies]\na = \"1.2.3\"\n";
    let parsed = SkillProjectToml::from_toml_str(legacy).unwrap();
    match parsed.dependencies.as_ref().unwrap().dependencies.get("a") {
        Some(DependencySpec::Version(v)) => assert_eq!(v, "1.2.3"),
        other => panic!("expected a bare version, got {other:?}"),
    }
}
