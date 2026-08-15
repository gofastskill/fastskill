use crate::error::{CliError, CliResult};
use crate::utils::messages;
use fastskill_core::core::repository::RepositoryDefinition;
use fastskill_core::OutputFormat;
use std::path::PathBuf;

#[allow(dead_code)] // Used by legacy registry command paths if needed
pub async fn execute_list() -> CliResult<()> {
    execute_list_with_format(OutputFormat::Table).await
}

#[allow(dead_code)]
pub async fn execute_list_with_json(json: bool) -> CliResult<()> {
    let format = if json {
        OutputFormat::Json
    } else {
        OutputFormat::Table
    };
    execute_list_with_format(format).await
}

pub async fn execute_list_with_format(format: OutputFormat) -> CliResult<()> {
    let repo_manager = super::helpers::load_repo_manager().await?;
    let repos = repo_manager.list_repositories();

    match format {
        OutputFormat::Json => {
            let json_output = serde_json::to_string_pretty(&repos)
                .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
            crate::outln!("{}", json_output);
        }
        OutputFormat::Table => {
            crate::outln!("{}", super::formatters::format_repository_list(&repos))
        }
        OutputFormat::Grid => {
            crate::outln!("{}", super::formatters::format_repository_list_grid(&repos))
        }
        OutputFormat::Xml => {
            crate::outln!("{}", super::formatters::format_repository_list_xml(&repos))
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn execute_show_with_json(name: String, json: bool) -> CliResult<()> {
    let format = if json {
        OutputFormat::Json
    } else {
        OutputFormat::Table
    };
    execute_show_with_format(name, format).await
}

pub async fn execute_show_with_format(name: String, format: OutputFormat) -> CliResult<()> {
    let repo_manager = super::helpers::load_repo_manager().await?;

    let repo = repo_manager
        .get_repository(&name)
        .ok_or_else(|| CliError::Config(format!("Repository '{}' not found", name)))?;

    match format {
        OutputFormat::Json => {
            let json_output = serde_json::to_string_pretty(&repo)
                .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
            crate::outln!("{}", json_output);
        }
        OutputFormat::Table => {
            crate::outln!("{}", super::formatters::format_repository_details(repo))
        }
        OutputFormat::Grid => {
            crate::outln!(
                "{}",
                super::formatters::format_repository_details_grid(repo)
            )
        }
        OutputFormat::Xml => {
            crate::outln!("{}", super::formatters::format_repository_details_xml(repo))
        }
    }

    Ok(())
}

pub async fn execute_update(
    name: String,
    branch: Option<String>,
    priority: Option<u32>,
) -> CliResult<()> {
    let mut repo_manager = super::helpers::load_repo_manager().await?;

    let repo = repo_manager
        .get_repository(&name)
        .ok_or_else(|| CliError::Config(format!("Repository '{}' not found", name)))?
        .clone();

    let updated_config = if let Some(new_branch) = branch {
        match &repo.config {
            fastskill_core::core::repository::RepositoryConfig::GitMarketplace {
                url,
                branch: _,
                tag,
            } => fastskill_core::core::repository::RepositoryConfig::GitMarketplace {
                url: url.clone(),
                branch: Some(new_branch),
                tag: tag.clone(),
            },
            _ => repo.config.clone(),
        }
    } else {
        repo.config.clone()
    };

    let updated_priority = priority.unwrap_or(repo.priority);

    repo_manager
        .remove_repository(&name)
        .map_err(|e| CliError::Config(format!("Failed to remove repository: {}", e)))?;

    let updated_repo = fastskill_core::core::repository::RepositoryDefinition {
        name: repo.name.clone(),
        repo_type: repo.repo_type,
        priority: updated_priority,
        config: updated_config,
        auth: repo.auth,
        storage: repo.storage,
    };

    repo_manager
        .add_repository(name.clone(), updated_repo)
        .map_err(|e| CliError::Config(format!("Failed to add repository: {}", e)))?;
    repo_manager
        .save()
        .map_err(|e| CliError::Config(format!("Failed to save repositories: {}", e)))?;

    crate::outln!("{}", messages::ok(&format!("Updated repository: {}", name)));
    Ok(())
}

pub async fn execute_test(name: String) -> CliResult<()> {
    let repo_manager = super::helpers::load_repo_manager().await?;

    let _repo = repo_manager
        .get_repository(&name)
        .ok_or_else(|| CliError::Config(format!("Repository '{}' not found", name)))?;

    crate::outln!(
        "{}",
        messages::info(&format!("Testing repository: {}...", name))
    );

    match repo_manager.get_client(&name).await {
        Ok(client) => match client.list_skills().await {
            Ok(skills) => {
                crate::outln!(
                    "{}",
                    messages::ok(&format!(
                        "Repository '{}' is accessible ({} skills found)",
                        name,
                        skills.len()
                    ))
                );
            }
            Err(e) => {
                return Err(CliError::Config(format!(
                    "Repository '{}' test failed: {}",
                    name, e
                )));
            }
        },
        Err(e) => {
            return Err(CliError::Config(format!(
                "Repository '{}' test failed: {}",
                name, e
            )));
        }
    }

    Ok(())
}

/// `repos refresh [name]` (PRD 006 "Local Skill Cache", US-005): refresh the
/// on-disk index cache for one repository, or every configured repository,
/// via [`fastskill_core::core::repository::RepositoryManager::refresh_index`].
///
/// An explicit `name` for a repository that does not exist is a fast, honest
/// error — never fake success. When refreshing "all" repositories, one
/// source's failure does not stop the rest: every source is attempted, each
/// prints its own outcome, and the command exits non-zero if any failed
/// (FR-5). Index only — this never fetches or caches skill content.
pub async fn execute_refresh(name: Option<String>) -> CliResult<()> {
    let repo_manager = super::helpers::load_repo_manager().await?;
    let cache = fastskill_core::core::cache::SkillCache::from_env()?;

    let targets: Vec<String> = match name {
        Some(repo_name) => {
            repo_manager
                .get_repository(&repo_name)
                .ok_or_else(|| CliError::Config(format!("Repository '{}' not found", repo_name)))?;
            vec![repo_name]
        }
        None => repo_manager
            .list_repositories()
            .into_iter()
            .map(|r| r.name.clone())
            .collect(),
    };

    if targets.is_empty() {
        crate::outln!(
            "{}",
            messages::info("No repositories configured to refresh")
        );
        return Ok(());
    }

    let mut failed: Vec<(String, String)> = Vec::new();
    for target in &targets {
        match repo_manager.refresh_index(&cache, target).await {
            Ok(count) => {
                crate::outln!(
                    "{}",
                    messages::ok(&format!(
                        "Refreshed {}: {} skill{}",
                        target,
                        count,
                        if count == 1 { "" } else { "s" }
                    ))
                );
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    messages::error(&format!("Failed to refresh {}: {}", target, e))
                );
                failed.push((target.clone(), e.to_string()));
            }
        }
    }

    if failed.is_empty() {
        return Ok(());
    }

    Err(CliError::Validation(format!(
        "{} of {} repositor{} failed to refresh:\n{}",
        failed.len(),
        targets.len(),
        if targets.len() == 1 { "y" } else { "ies" },
        failed
            .iter()
            .map(|(n, e)| format!("  - {}: {}", n, e))
            .collect::<Vec<_>>()
            .join("\n")
    )))
}

pub async fn execute_add(
    name: String,
    repo_type: String,
    url_or_path: String,
    priority: Option<u32>,
    branch: Option<String>,
    tag: Option<String>,
    auth_type: Option<String>,
    auth_env: Option<String>,
    auth_key_path: Option<PathBuf>,
    auth_username: Option<String>,
) -> CliResult<()> {
    let mut repo_manager = super::helpers::load_repo_manager().await?;

    let repo_type = super::helpers::parse_repository_type(&repo_type)?;
    let config =
        super::helpers::create_repository_config(repo_type.clone(), url_or_path, branch, tag);
    let auth =
        super::helpers::parse_authentication(auth_type, auth_env, auth_key_path, auth_username)?;

    let repo = RepositoryDefinition {
        name: name.clone(),
        repo_type,
        priority: priority.unwrap_or(0),
        config,
        auth,
        storage: None,
    };

    repo_manager
        .add_repository(name.clone(), repo)
        .map_err(|e| CliError::Config(format!("Failed to add repository: {}", e)))?;
    repo_manager
        .save()
        .map_err(|e| CliError::Config(format!("Failed to save repositories: {}", e)))?;

    crate::outln!("{}", messages::ok(&format!("Added repository: {}", name)));
    Ok(())
}

pub async fn execute_remove(name: String) -> CliResult<()> {
    let mut repo_manager = super::helpers::load_repo_manager().await?;

    repo_manager
        .remove_repository(&name)
        .map_err(|e| CliError::Config(format!("Failed to remove repository: {}", e)))?;
    repo_manager
        .save()
        .map_err(|e| CliError::Config(format!("Failed to save repositories: {}", e)))?;

    crate::outln!("{}", messages::ok(&format!("Removed repository: {}", name)));
    Ok(())
}

#[allow(dead_code)]
pub async fn execute_show(name: String) -> CliResult<()> {
    execute_show_with_format(name, OutputFormat::Table).await
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::await_holding_lock)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_list_with_json_empty() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let result = execute_list_with_json(false).await;
        assert!(result.is_ok());

        let result_json = execute_list_with_json(true).await;
        assert!(result_json.is_ok());
    }

    #[tokio::test]
    async fn test_execute_add_then_list() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        let repo_path = temp_dir.path().join("test-repo-path");
        fs::create_dir_all(&repo_path).unwrap();

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let result = execute_add(
            "test-repo".to_string(),
            "local".to_string(),
            repo_path.display().to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(result.is_ok());

        let list_result = execute_list_with_json(false).await;
        assert!(list_result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_show_then_remove() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        let repo_path = temp_dir.path().join("test-repo-path");
        fs::create_dir_all(&repo_path).unwrap();

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let add_result = execute_add(
            "test-repo".to_string(),
            "local".to_string(),
            repo_path.display().to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(add_result.is_ok());

        let show_result = execute_show("test-repo".to_string()).await;
        assert!(show_result.is_ok());

        let remove_result = execute_remove("test-repo".to_string()).await;
        assert!(remove_result.is_ok());

        let list_result = execute_list_with_json(false).await;
        assert!(list_result.is_ok());
    }

    // ── PRD 006 "Local Skill Cache", US-005: `repos refresh` real semantics ──

    /// RAII guard restoring `FASTSKILL_CACHE_DIR` to whatever it was before
    /// the test set it, so these tests never leak into the real platform
    /// cache dir nor into other tests sharing this process.
    struct CacheDirEnvGuard(Option<String>);
    impl Drop for CacheDirEnvGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("FASTSKILL_CACHE_DIR", v),
                None => std::env::remove_var("FASTSKILL_CACHE_DIR"),
            }
        }
    }

    fn write_skill(dir: &std::path::Path, id: &str, version: &str) {
        let skill_dir = dir.join(id);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {id}\nversion: \"{version}\"\ndescription: a skill\n---\nBody\n"),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn test_execute_refresh_unknown_repository_fails() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(original_dir);
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let manifest_content = "[tool.fastskill]\nskills_directory = \".claude/skills\"\n";
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let cache_dir = TempDir::new().unwrap();
        let _env_guard = CacheDirEnvGuard(std::env::var("FASTSKILL_CACHE_DIR").ok());
        std::env::set_var("FASTSKILL_CACHE_DIR", cache_dir.path());

        let err = execute_refresh(Some("does-not-exist".to_string()))
            .await
            .expect_err("refresh of an unknown repository must fail, not fake success");
        let message = err.to_string();
        assert!(message.contains("does-not-exist"));
        assert!(message.contains("not found"));
    }

    #[tokio::test]
    async fn test_execute_refresh_writes_index_and_reports_skill_count() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(original_dir);
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let manifest_content = "[tool.fastskill]\nskills_directory = \".claude/skills\"\n";
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let repo_path = temp_dir.path().join("indexed-repo");
        write_skill(&repo_path, "indexed-skill", "2.3.4");

        let add_result = execute_add(
            "idx-repo".to_string(),
            "local".to_string(),
            repo_path.display().to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(add_result.is_ok());

        let cache_dir = TempDir::new().unwrap();
        let _env_guard = CacheDirEnvGuard(std::env::var("FASTSKILL_CACHE_DIR").ok());
        std::env::set_var("FASTSKILL_CACHE_DIR", cache_dir.path());

        let result = execute_refresh(Some("idx-repo".to_string())).await;
        assert!(result.is_ok(), "refresh should succeed: {:?}", result.err());

        let index_path = cache_dir.path().join("index").join("idx-repo.json");
        let index_contents = fs::read_to_string(&index_path)
            .unwrap_or_else(|e| panic!("expected index file at {}: {e}", index_path.display()));
        assert!(index_contents.contains("indexed-skill"));
        assert!(index_contents.contains("2.3.4"));
    }

    #[tokio::test]
    async fn test_execute_refresh_all_partial_failure_refreshes_others_and_errors() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(original_dir);
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let manifest_content = "[tool.fastskill]\nskills_directory = \".claude/skills\"\n";
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let good_repo_path = temp_dir.path().join("good-repo");
        write_skill(&good_repo_path, "healthy-skill", "1.0.0");
        let add_good = execute_add(
            "healthy".to_string(),
            "local".to_string(),
            good_repo_path.display().to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(add_good.is_ok());

        let broken_repo_path = temp_dir.path().join("this-path-does-not-exist");
        let add_bad = execute_add(
            "broken".to_string(),
            "local".to_string(),
            broken_repo_path.display().to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(add_bad.is_ok());

        let cache_dir = TempDir::new().unwrap();
        let _env_guard = CacheDirEnvGuard(std::env::var("FASTSKILL_CACHE_DIR").ok());
        std::env::set_var("FASTSKILL_CACHE_DIR", cache_dir.path());

        let result = execute_refresh(None).await;
        assert!(
            result.is_err(),
            "overall refresh must fail non-zero when any source fails"
        );
        let message = result.unwrap_err().to_string();
        assert!(message.contains("broken"));

        // The healthy source still refreshed despite the other's failure.
        assert!(cache_dir
            .path()
            .join("index")
            .join("healthy.json")
            .is_file());
        assert!(!cache_dir.path().join("index").join("broken.json").exists());
    }
}
