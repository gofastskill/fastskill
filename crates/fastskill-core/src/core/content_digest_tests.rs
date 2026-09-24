use super::*;
use tempfile::TempDir;

fn skill(root: &Path, name: &str, files: &[(&str, &[u8])]) -> std::path::PathBuf {
    let directory = root.join(name);
    for (relative, contents) in files {
        let path = directory.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    fs::create_dir_all(&directory).unwrap();
    directory
}

/// A one-file tree whose bytes spell out a second file, as the legacy digest saw it.
fn boundary_pair(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let split = skill(root, "split", &[("a.bin", b"head"), ("z.sh", b"tail")]);
    let mut joined = b"head".to_vec();
    joined.extend_from_slice(&(b"z.sh".len() as u64).to_be_bytes());
    joined.extend_from_slice(b"z.sh");
    joined.extend_from_slice(b"tail");
    let merged = skill(root, "merged", &[("a.bin", &joined)]);
    (split, merged)
}

#[test]
fn current_form_is_prefixed_and_stable() {
    let root = TempDir::new().unwrap();
    let first = skill(root.path(), "first", &[("SKILL.md", b"x"), ("sub/b", b"y")]);
    let second = skill(
        root.path(),
        "second",
        &[("sub/b", b"y"), ("SKILL.md", b"x")],
    );
    let digest = content_digest(&first).unwrap();
    let hex = digest.strip_prefix(CONTENT_DIGEST_PREFIX).unwrap();
    assert_eq!(hex.len(), 64);
    assert!(
        is_legacy_digest(hex),
        "the hex part is 64 lowercase hex characters"
    );
    assert!(!is_legacy_digest(&digest));
    assert_eq!(digest, content_digest(&second).unwrap());
    assert_ne!(digest, legacy_content_digest(&first).unwrap());
}

#[test]
fn legacy_form_collides_where_the_current_form_does_not() {
    let root = TempDir::new().unwrap();
    let (split, merged) = boundary_pair(root.path());
    assert_eq!(
        legacy_content_digest(&split).unwrap(),
        legacy_content_digest(&merged).unwrap()
    );
    assert_ne!(
        content_digest(&split).unwrap(),
        content_digest(&merged).unwrap()
    );
}

#[test]
fn contents_moving_between_files_change_the_digest() {
    let root = TempDir::new().unwrap();
    let left = skill(root.path(), "left", &[("a", b"xy"), ("b", b"z")]);
    let right = skill(root.path(), "right", &[("a", b"x"), ("b", b"yz")]);
    assert_ne!(
        content_digest(&left).unwrap(),
        content_digest(&right).unwrap()
    );
}

#[test]
fn empty_directories_do_not_contribute() {
    let root = TempDir::new().unwrap();
    let plain = skill(root.path(), "plain", &[("SKILL.md", b"x")]);
    let with_empty = skill(root.path(), "with-empty", &[("SKILL.md", b"x")]);
    fs::create_dir_all(with_empty.join("empty/nested")).unwrap();
    assert_eq!(
        content_digest(&plain).unwrap(),
        content_digest(&with_empty).unwrap()
    );
}

#[test]
fn missing_directory_is_refused() {
    let root = TempDir::new().unwrap();
    let missing = root.path().join("missing");
    assert!(content_digest(&missing).is_err());
    assert!(legacy_content_digest(&missing).is_err());
    assert!(content_digest_matches(&"0".repeat(64), &missing).is_err());
    assert!(content_digest_matches("unknown", &missing).is_err());
}

#[test]
fn framed_reader_must_yield_exactly_its_length() {
    let path = Path::new("file");
    let mut hasher = Sha256::new();
    assert!(hash_framed(&mut hasher, &b"abc"[..], 3, path).is_ok());
    let longer = hash_framed(&mut hasher, &b"abc"[..], 2, path).unwrap_err();
    assert!(longer.to_string().contains("changed while"));
    assert!(hash_framed(&mut hasher, &b"abc"[..], 4, path).is_err());
}

#[test]
fn matching_dispatches_on_the_recorded_form() {
    let root = TempDir::new().unwrap();
    let (split, merged) = boundary_pair(root.path());
    let current = content_digest(&split).unwrap();
    let legacy = legacy_content_digest(&split).unwrap();

    assert!(content_digest_matches(&current, &split).unwrap());
    assert!(!content_digest_matches(&current, &merged).unwrap());
    assert!(content_digest_matches(&legacy, &split).unwrap());
    // The legacy form cannot tell these trees apart; that is the residual risk it carries.
    assert!(content_digest_matches(&legacy, &merged).unwrap());
    let other = skill(root.path(), "other", &[("SKILL.md", b"other")]);
    assert!(!content_digest_matches(&legacy, &other).unwrap());

    for unknown in [
        "",
        "abc",
        "sha256:0000",
        &legacy.to_uppercase(),
        &format!("sha256-tree-v3:{legacy}"),
    ] {
        assert!(
            !content_digest_matches(unknown, &split).unwrap(),
            "{unknown}"
        );
    }
}

#[test]
fn upgrade_replaces_only_a_matching_legacy_digest() {
    let root = TempDir::new().unwrap();
    let first = skill(root.path(), "first", &[("SKILL.md", b"one")]);
    let second = skill(root.path(), "second", &[("SKILL.md", b"two")]);
    let current = content_digest(&first).unwrap();
    let legacy = legacy_content_digest(&first).unwrap();

    assert_eq!(upgraded_digest(&legacy, &first).unwrap(), current);
    assert_eq!(upgraded_digest(&legacy, &second).unwrap(), legacy);
    assert_eq!(upgraded_digest(&current, &second).unwrap(), current);
}

#[test]
fn recorded_digests_conflict_only_within_one_form() {
    let current_a = format!("{CONTENT_DIGEST_PREFIX}{}", "a".repeat(64));
    let current_b = format!("{CONTENT_DIGEST_PREFIX}{}", "b".repeat(64));
    let legacy_a = "a".repeat(64);
    let legacy_b = "b".repeat(64);
    assert!(!recorded_digests_conflict([]));
    assert!(!recorded_digests_conflict([
        current_a.as_str(),
        current_a.as_str()
    ]));
    assert!(recorded_digests_conflict([
        current_a.as_str(),
        current_b.as_str()
    ]));
    assert!(recorded_digests_conflict([
        legacy_a.as_str(),
        legacy_b.as_str()
    ]));
    assert!(!recorded_digests_conflict([
        current_a.as_str(),
        legacy_b.as_str()
    ]));
    assert!(recorded_digests_conflict([
        legacy_a.as_str(),
        current_a.as_str(),
        legacy_b.as_str()
    ]));
}

#[test]
fn all_recorded_digests_must_name_the_content() {
    let root = TempDir::new().unwrap();
    let first = skill(root.path(), "first", &[("SKILL.md", b"one")]);
    let second = skill(root.path(), "second", &[("SKILL.md", b"two")]);
    let current = content_digest(&first).unwrap();
    let legacy = legacy_content_digest(&first).unwrap();
    assert!(all_match(&[], &first).unwrap());
    assert!(all_match(&[&current, &legacy], &first).unwrap());
    assert!(!all_match(&[&current, &legacy], &second).unwrap());
}

#[test]
fn digest_forms_accept_either_form_of_their_own_content() {
    let root = TempDir::new().unwrap();
    let first = skill(root.path(), "first", &[("SKILL.md", b"one")]);
    let second = skill(root.path(), "second", &[("SKILL.md", b"two")]);
    let forms = DigestForms::of_directory(&first).unwrap();
    assert_eq!(forms.current, content_digest(&first).unwrap());
    assert!(forms.matches(&forms.current));
    assert!(forms.matches(&forms.legacy));
    assert!(!forms.matches(&content_digest(&second).unwrap()));
    assert!(!forms.matches(&legacy_content_digest(&second).unwrap()));
}

#[cfg(unix)]
#[test]
fn backslash_in_a_file_name_is_not_a_directory_separator() {
    let root = TempDir::new().unwrap();
    let flat = skill(root.path(), "flat", &[("a\\b", b"x")]);
    let nested = skill(root.path(), "nested", &[("a/b", b"x")]);
    assert_eq!(
        legacy_content_digest(&flat).unwrap(),
        legacy_content_digest(&nested).unwrap()
    );
    assert_ne!(
        content_digest(&flat).unwrap(),
        content_digest(&nested).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_file_names_are_refused() {
    use std::os::unix::ffi::OsStrExt;
    let root = TempDir::new().unwrap();
    let directory = skill(root.path(), "skill", &[("SKILL.md", b"x")]);
    let name = std::ffi::OsStr::from_bytes(b"bad-\xff");
    if fs::write(directory.join(name), b"x").is_err() {
        return; // The filesystem itself refuses non-UTF-8 names.
    }
    let error = content_digest(&directory).unwrap_err();
    assert!(error.to_string().contains("not valid UTF-8"));
    assert!(legacy_content_digest(&directory).is_ok());
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_refused() {
    let root = TempDir::new().unwrap();
    let directory = skill(root.path(), "skill", &[("SKILL.md", b"x")]);
    std::os::unix::fs::symlink(directory.join("SKILL.md"), directory.join("link")).unwrap();
    assert!(content_digest(&directory).is_err());
    assert!(legacy_content_digest(&directory).is_err());
}

#[cfg(unix)]
#[test]
fn unreadable_subdirectories_are_reported() {
    use std::os::unix::fs::PermissionsExt;
    let root = TempDir::new().unwrap();
    let directory = skill(root.path(), "skill", &[("SKILL.md", b"x"), ("sub/f", b"y")]);
    let sub = directory.join("sub");
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(&sub).is_ok(); // true when running with elevated privileges
    let current = content_digest(&directory);
    let legacy = legacy_content_digest(&directory);
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
    if !readable {
        assert!(current.is_err());
        assert!(legacy.is_err());
    }
}
