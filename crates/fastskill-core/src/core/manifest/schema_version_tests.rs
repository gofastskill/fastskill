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
