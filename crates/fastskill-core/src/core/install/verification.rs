use super::support::{git_ref_cache_key, resolve_registry_version, resolve_repo_name};
use crate::core::origin::{GitRef, Origin, Resolved};
use crate::core::service::{FastSkillService, ServiceError};

pub(super) fn offline_resolution(
    service: &FastSkillService,
    origin: &Origin,
) -> Result<Resolved, ServiceError> {
    match origin {
        Origin::Git { url, r#ref, .. } => {
            let sha = match r#ref {
                GitRef::Commit(commit) => commit.clone(),
                _ => service
                    .skill_cache()
                    .read_git_resolutions()?
                    .get(url, &git_ref_cache_key(r#ref))
                    .map(|entry| entry.sha.clone())
                    .ok_or_else(|| {
                        ServiceError::Config(format!(
                            "offline cache has no resolved revision for Git ref '{url}'; run an online install first"
                        ))
                    })?,
            };
            Ok(Resolved {
                version: String::new(),
                commit_hash: Some(sha),
                checksum: None,
            })
        }
        Origin::Repository {
            repo,
            skill,
            version,
        } => {
            let manager = service
                .repository_manager()
                .ok_or_else(|| ServiceError::Config("No repositories configured".to_string()))?;
            let repo_name = resolve_repo_name(manager, repo)?;
            let selected = resolve_registry_version(
                service.skill_cache(),
                &repo_name,
                skill,
                version.as_ref(),
            )?;
            Ok(Resolved {
                version: selected,
                commit_hash: None,
                checksum: None,
            })
        }
        Origin::ZipUrl { .. } => Ok(Resolved {
            version: String::new(),
            commit_hash: None,
            checksum: None,
        }),
        Origin::Local { .. } => Err(ServiceError::InvalidOperation(
            "local origins do not require an offline cache resolution".to_string(),
        )),
    }
}

pub(super) fn verify_resolved_facts(
    id: &str,
    origin: &Origin,
    expected: &Resolved,
    actual: &Resolved,
) -> Result<(), ServiceError> {
    if expected.version != actual.version {
        return Err(ServiceError::Validation(format!(
            "locked version for '{id}' is {}, but the origin produced {}",
            expected.version, actual.version
        )));
    }
    if expected.commit_hash.is_some() && expected.commit_hash != actual.commit_hash {
        return Err(ServiceError::Validation(format!(
            "locked commit for '{id}' does not match the fetched commit"
        )));
    }
    if !matches!(origin, Origin::Local { editable: true, .. }) {
        let expected_checksum = expected.checksum.as_ref().ok_or_else(|| {
            ServiceError::Validation(format!(
                "locked skill '{id}' has no checksum; run an explicit update to establish verified contents"
            ))
        })?;
        if actual.checksum.as_ref() != Some(expected_checksum) {
            return Err(ServiceError::Validation(format!(
                "locked checksum for '{id}' does not match the fetched contents"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn resolved(version: &str, commit: Option<&str>, checksum: Option<&str>) -> Resolved {
        Resolved {
            version: version.to_string(),
            commit_hash: commit.map(str::to_string),
            checksum: checksum.map(str::to_string),
        }
    }

    #[test]
    fn immutable_facts_must_all_match() {
        let origin = Origin::ZipUrl { url: "u".into() };
        let expected = resolved("1.0.0", None, Some("abc"));
        assert!(verify_resolved_facts("x", &origin, &expected, &expected).is_ok());
        assert!(verify_resolved_facts(
            "x",
            &origin,
            &expected,
            &resolved("2.0.0", None, Some("abc"))
        )
        .is_err());
        assert!(verify_resolved_facts(
            "x",
            &origin,
            &expected,
            &resolved("1.0.0", None, Some("bad"))
        )
        .is_err());
        assert!(
            verify_resolved_facts("x", &origin, &resolved("1.0.0", None, None), &expected).is_err()
        );
    }

    #[test]
    fn commit_mismatch_fails_and_editable_skips_digest() {
        let git = Origin::Git {
            url: "u".into(),
            r#ref: GitRef::Default,
            subdir: None,
        };
        assert!(verify_resolved_facts(
            "x",
            &git,
            &resolved("1.0.0", Some("a"), Some("c")),
            &resolved("1.0.0", Some("b"), Some("c"))
        )
        .is_err());
        let editable = Origin::Local {
            path: PathBuf::from("x"),
            editable: true,
        };
        assert!(verify_resolved_facts(
            "x",
            &editable,
            &resolved("1.0.0", None, None),
            &resolved("1.0.0", None, None)
        )
        .is_ok());
    }

    #[tokio::test]
    async fn offline_resolution_handles_direct_and_missing_cache_origins() {
        let root = TempDir::new().expect("temp root");
        let service = FastSkillService::new(crate::ServiceConfig {
            skill_storage_path: root.path().join("skills"),
            skill_cache_root: Some(root.path().join("cache")),
            ..Default::default()
        })
        .await
        .expect("service");

        let commit = offline_resolution(
            &service,
            &Origin::Git {
                url: "https://example.test/repo.git".into(),
                r#ref: GitRef::Commit("abc123".into()),
                subdir: None,
            },
        )
        .expect("commit requires no lookup");
        assert_eq!(commit.commit_hash.as_deref(), Some("abc123"));

        let missing = offline_resolution(
            &service,
            &Origin::Git {
                url: "https://example.test/repo.git".into(),
                r#ref: GitRef::Branch("main".into()),
                subdir: None,
            },
        )
        .expect_err("floating Git ref must be cached");
        assert!(missing.to_string().contains("offline cache"));

        let zip = offline_resolution(&service, &Origin::ZipUrl { url: "u".into() })
            .expect("ZIP resolution is content-addressed later");
        assert!(zip.version.is_empty());
        let local = offline_resolution(
            &service,
            &Origin::Local {
                path: "local".into(),
                editable: false,
            },
        )
        .expect_err("local content does not use the remote cache resolver");
        assert!(local.to_string().contains("do not require"));

        let no_repositories = offline_resolution(
            &service,
            &Origin::Repository {
                repo: "team".into(),
                skill: "demo".into(),
                version: None,
            },
        )
        .expect_err("repository resolution needs a manager");
        assert!(no_repositories.to_string().contains("No repositories"));
    }
}
