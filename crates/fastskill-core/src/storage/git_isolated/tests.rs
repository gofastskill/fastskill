use super::*;

fn args_for(branch: Option<&str>, options: &IsolatedCloneOptions) -> Vec<String> {
    build_isolated_clone_args(
        "https://example.com/repo.git",
        Path::new("/w/tree"),
        Path::new("/w/home"),
        branch,
        options,
    )
}

fn settings(args: &[String]) -> Vec<&str> {
    args.windows(2)
        .filter(|pair| pair[0] == "-c")
        .map(|pair| pair[1].as_str())
        .collect()
}

#[test]
fn clone_args_disable_hooks_helpers_and_unsafe_transports() {
    let args = args_for(None, &IsolatedCloneOptions::default());
    let settings = settings(&args);
    for expected in [
        "protocol.ext.allow=never",
        "protocol.file.allow=never",
        "core.fsmonitor=false",
        "credential.helper=",
        "http.followRedirects=false",
    ] {
        assert!(settings.contains(&expected), "{expected} in {settings:?}");
    }
    let hooks = format!("core.hooksPath={}", Path::new("/w/home/hooks").display());
    assert!(settings.contains(&hooks.as_str()), "{settings:?}");
    for flag in [
        "--depth=1",
        "--single-branch",
        "--no-tags",
        "--no-recurse-submodules",
    ] {
        assert!(args.iter().any(|arg| arg == flag), "{flag} in {args:?}");
    }
    let template = format!("--template={}", Path::new("/w/home/template").display());
    assert!(args.contains(&template), "{args:?}");
    assert!(!args.iter().any(|arg| arg.starts_with("--branch")));
}

#[test]
fn clone_args_end_options_before_the_url() {
    let args = args_for(Some("-evil"), &IsolatedCloneOptions::default());
    let end = args.iter().position(|arg| arg == "--").unwrap();
    assert_eq!(args[end + 1], "https://example.com/repo.git");
    assert_eq!(args.len(), end + 3);
    // The branch is one `--branch=<name>` argument, never a flag of its own.
    assert!(args[..end].contains(&"--branch=-evil".to_string()));
    assert!(!args.contains(&"-evil".to_string()));
}

#[test]
fn redirects_are_followed_only_when_asked() {
    let options = IsolatedCloneOptions {
        follow_redirects: true,
        ..IsolatedCloneOptions::default()
    };
    assert!(settings(&args_for(None, &options)).contains(&"http.followRedirects=initial"));
}

#[test]
fn allowed_protocols_refuse_ext_file_and_malformed_names() {
    let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(allowed_protocols(&names(&["https"])).unwrap(), "https");
    assert_eq!(
        allowed_protocols(&names(&["https", "git"])).unwrap(),
        "https:git"
    );
    for bad in [
        &[][..],
        &["ext"],
        &["https", "file"],
        &[""],
        &["https:file"],
        &["HTTPS"],
        &["git;rm"],
    ] {
        assert!(allowed_protocols(&names(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn isolated_git_passes_only_listed_variables_and_points_config_at_home() {
    let home = Path::new("/w/home");
    let cmd = isolated_git(home, "https");
    let envs: Vec<(String, Option<String>)> = cmd
        .as_std()
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    let value = |name: &str| {
        envs.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .and_then(|(_, value)| value.clone())
    };
    assert_eq!(value("GIT_CONFIG_NOSYSTEM").as_deref(), Some("1"));
    assert_eq!(value("GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
    assert_eq!(value("GIT_ALLOW_PROTOCOL").as_deref(), Some("https"));
    assert_eq!(value("HOME"), Some(home.display().to_string()));
    assert_eq!(value("XDG_CONFIG_HOME"), Some(home.display().to_string()));
    assert_eq!(
        value("GIT_CONFIG_GLOBAL"),
        Some(home.join("gitconfig").display().to_string())
    );
    let set_here = [
        "HOME",
        "XDG_CONFIG_HOME",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_GLOBAL",
        "GIT_TERMINAL_PROMPT",
        "GIT_ALLOW_PROTOCOL",
    ];
    for (key, value) in &envs {
        // Removals (`None`) are the scrubbed repository variables.
        if value.is_some() {
            assert!(
                set_here.iter().any(|name| name.eq_ignore_ascii_case(key))
                    || PASSED_ENV_VARS
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(key)),
                "unexpected variable {key}"
            );
        }
    }
}

#[test]
fn default_options_allow_https_only_without_redirects() {
    let options = IsolatedCloneOptions::default();
    assert_eq!(options.allowed_protocols, vec!["https".to_string()]);
    assert!(!options.follow_redirects);
    assert!(options.max_bytes > 0 && options.max_files > 0);
}

#[test]
fn disk_usage_counts_entries_and_bytes() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/a"), [0u8; 100]).unwrap();
    std::fs::write(dir.path().join("b"), [0u8; 20]).unwrap();
    let (bytes, files) = disk_usage(dir.path());
    assert_eq!(files, 3);
    assert!(bytes >= 120, "{bytes}");
    assert_eq!(disk_usage(&dir.path().join("missing")), (0, 0));
}

#[cfg(unix)]
#[test]
fn disk_usage_does_not_follow_links() {
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("big"), vec![0u8; 1 << 20]).unwrap();
    let dir = TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
    let (bytes, files) = disk_usage(dir.path());
    assert_eq!(files, 1);
    assert!(bytes < 1 << 20, "{bytes}");
}
