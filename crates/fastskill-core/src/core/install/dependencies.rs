use super::*;

pub(super) fn durable_dependencies(
    service: &FastSkillService,
    manifest: &SkillProjectToml,
    fetched_path: &Path,
    parent: &Origin,
) -> Result<Vec<crate::core::manifest::SkillEntry>, ServiceError> {
    let local_parent = match parent {
        Origin::Local { path, .. } => {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                service
                    .project_root()
                    .cloned()
                    .unwrap_or(std::env::current_dir()?)
                    .join(path)
            };
            path.is_dir().then_some(path)
        }
        _ => None,
    };
    let base = local_parent.as_deref().unwrap_or(fetched_path);
    let mut entries = manifest
        .to_package_skill_entries(base)
        .map_err(ServiceError::Validation)?;
    let Some(specs) = manifest.dependencies.as_ref() else {
        return Ok(entries);
    };
    for entry in &mut entries {
        let Some(DependencySpec::Inline {
            origin:
                Origin::Local {
                    path,
                    editable: false,
                },
            ..
        }) = specs.dependencies.get(&entry.id)
        else {
            continue;
        };
        if path.is_absolute() || local_parent.is_some() {
            continue;
        }
        match parent {
            Origin::Git { url, r#ref, subdir } => {
                entry.origin = Origin::Git {
                    url: url.clone(),
                    r#ref: r#ref.clone(),
                    subdir: Some(normalize_git_subdir(subdir.as_deref(), path)?),
                };
            }
            _ => {
                return Err(ServiceError::Validation(format!(
                    "dependency '{}' uses a relative local path inside an archived package; use a durable Git or repository origin",
                    entry.id
                )));
            }
        }
    }
    Ok(entries)
}

fn normalize_git_subdir(base: Option<&Path>, child: &Path) -> Result<PathBuf, ServiceError> {
    let mut parts = Vec::new();
    for component in base
        .into_iter()
        .flat_map(Path::components)
        .chain(child.components())
    {
        match component {
            std::path::Component::Normal(value) => parts.push(value.to_os_string()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(ServiceError::Validation(
                        "relative dependency path escapes its Git repository".to_string(),
                    ));
                }
            }
            _ => {
                return Err(ServiceError::Validation(
                    "Git dependency subdirectories must be relative".to_string(),
                ));
            }
        }
    }
    Ok(parts.into_iter().collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn git_relative_children_are_normalized_without_escaping_repository() {
        assert_eq!(
            normalize_git_subdir(Some(Path::new("skills/alpha")), Path::new("../beta")).unwrap(),
            PathBuf::from("skills/beta")
        );
        assert!(normalize_git_subdir(None, Path::new("../outside")).is_err());
        assert_eq!(
            normalize_git_subdir(Some(Path::new("./skills")), Path::new("./child")).unwrap(),
            PathBuf::from("skills/child")
        );
        assert!(normalize_git_subdir(None, Path::new("/absolute")).is_err());
    }

    fn dependency_manifest() -> SkillProjectToml {
        toml::from_str("[dependencies.child.origin]\ntype = \"local\"\npath = \"child\"\n").unwrap()
    }

    #[tokio::test]
    async fn skill_packages_reject_manifest_composition_before_reading_host_files() {
        let root = TempDir::new().unwrap();
        let package = root.path().join("package");
        std::fs::create_dir(&package).unwrap();
        let outside = root.path().join("private.toml");
        std::fs::write(&outside, "[dependencies]\nsecret = \"1.0.0\"\n").unwrap();
        let service = FastSkillService::new(crate::ServiceConfig::default())
            .await
            .unwrap();
        for reference in [
            PathBuf::from("../private.toml"),
            outside.clone(),
            PathBuf::from("nested.toml"),
        ] {
            let manifest: SkillProjectToml = toml::from_str(&format!(
                "[tool.fastskill.manifests]\nexternal = {:?}\n",
                reference
            ))
            .unwrap();
            let error = durable_dependencies(
                &service,
                &manifest,
                &package,
                &Origin::ZipUrl {
                    url: "https://example.test/package.zip".into(),
                },
            )
            .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("only supported in project manifests"),
                "{error}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(outside).unwrap(),
            "[dependencies]\nsecret = \"1.0.0\"\n"
        );
    }

    #[tokio::test]
    async fn durable_children_follow_local_and_git_parent_origins() {
        let root = TempDir::new().unwrap();
        let parent = root.path().join("parent");
        std::fs::create_dir_all(parent.join("child")).unwrap();
        let service = FastSkillService::new(crate::ServiceConfig::default())
            .await
            .unwrap()
            .with_project_root(root.path().to_path_buf());
        let entries = durable_dependencies(
            &service,
            &dependency_manifest(),
            root.path(),
            &Origin::Local {
                path: "parent".into(),
                editable: false,
            },
        )
        .unwrap();
        assert_eq!(
            entries[0].origin,
            Origin::Local {
                path: parent.join("child"),
                editable: false,
            }
        );

        let entries = durable_dependencies(
            &service,
            &dependency_manifest(),
            root.path(),
            &Origin::Git {
                url: "https://example.test/repo.git".into(),
                r#ref: crate::core::origin::GitRef::Branch("main".into()),
                subdir: Some("skills/parent".into()),
            },
        )
        .unwrap();
        assert_eq!(
            entries[0].origin,
            Origin::Git {
                url: "https://example.test/repo.git".into(),
                r#ref: crate::core::origin::GitRef::Branch("main".into()),
                subdir: Some("skills/parent/child".into()),
            }
        );
    }

    #[tokio::test]
    async fn archived_relative_child_is_rejected_and_empty_manifest_is_allowed() {
        let root = TempDir::new().unwrap();
        let service = FastSkillService::new(crate::ServiceConfig::default())
            .await
            .unwrap();
        let error = durable_dependencies(
            &service,
            &dependency_manifest(),
            root.path(),
            &Origin::ZipUrl { url: "u".into() },
        )
        .unwrap_err();
        assert!(error.to_string().contains("archived package"));

        let empty: SkillProjectToml = toml::from_str("[metadata]\nname = \"empty\"\n").unwrap();
        assert!(durable_dependencies(
            &service,
            &empty,
            root.path(),
            &Origin::ZipUrl { url: "u".into() },
        )
        .unwrap()
        .is_empty());
    }
}
