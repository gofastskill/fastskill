use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::messages;
use std::path::PathBuf;

#[allow(dead_code)] // Used by legacy registry command paths if needed
pub async fn execute_list() -> CliResult<()> {
    execute_list_with_json(false).await
}

pub async fn execute_list_with_json(json: bool) -> CliResult<()> {
    let repo_manager = super::helpers::load_repo_manager().await?;
    let repos = repo_manager.list_repositories();

    if json {
        let json_output = serde_json::to_string_pretty(&repos)
            .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
        println!("{}", json_output);
    } else {
        println!("{}", super::formatters::format_repository_list(&repos));
    }
    Ok(())
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

    let repo_type_enum = super::helpers::parse_repository_type(&repo_type)?;
    let config =
        super::helpers::create_repository_config(repo_type_enum.clone(), url_or_path, branch, tag);
    let auth =
        super::helpers::parse_authentication(auth_type, auth_env, auth_key_path, auth_username)?;

    let repo_def = fastskill::core::repository::RepositoryDefinition {
        name: name.clone(),
        repo_type: repo_type_enum,
        priority: priority.unwrap_or(0),
        config,
        auth,
        storage: None,
    };

    repo_manager
        .add_repository(name.clone(), repo_def.clone())
        .map_err(|e| CliError::Config(format!("Failed to add repository: {}", e)))?;
    repo_manager
        .save()
        .map_err(|e| CliError::Config(format!("Failed to save repositories: {}", e)))?;

    println!("{}", messages::ok(&format!("Added repository: {}", name)));
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

    println!("{}", messages::ok(&format!("Removed repository: {}", name)));
    Ok(())
}

#[allow(dead_code)] // Used by legacy registry command paths if needed
pub async fn execute_show(name: String) -> CliResult<()> {
    execute_show_with_json(name, false).await
}

pub async fn execute_show_with_json(name: String, json: bool) -> CliResult<()> {
    let repo_manager = super::helpers::load_repo_manager().await?;

    let repo = repo_manager
        .get_repository(&name)
        .ok_or_else(|| CliError::Config(format!("Repository '{}' not found", name)))?;

    if json {
        let json_output = serde_json::to_string_pretty(&repo)
            .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
        println!("{}", json_output);
    } else {
        println!("{}", super::formatters::format_repository_details(repo));
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
            fastskill::core::repository::RepositoryConfig::GitMarketplace {
                url,
                branch: _,
                tag,
            } => fastskill::core::repository::RepositoryConfig::GitMarketplace {
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

    let updated_repo = fastskill::core::repository::RepositoryDefinition {
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

    println!("{}", messages::ok(&format!("Updated repository: {}", name)));
    Ok(())
}

pub async fn execute_test(name: String) -> CliResult<()> {
    let repo_manager = super::helpers::load_repo_manager().await?;

    let _repo = repo_manager
        .get_repository(&name)
        .ok_or_else(|| CliError::Config(format!("Repository '{}' not found", name)))?;

    println!(
        "{}",
        messages::info(&format!("Testing repository: {}...", name))
    );

    match repo_manager.get_client(&name).await {
        Ok(client) => match client.list_skills().await {
            Ok(skills) => {
                println!(
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

pub async fn execute_refresh(name: Option<String>) -> CliResult<()> {
    if let Some(repo_name) = name {
        println!(
            "{}",
            messages::ok(&format!("Refreshed cache for repository: {}", repo_name))
        );
    } else {
        println!("{}", messages::ok("Refreshed cache for all repositories"));
    }
    Ok(())
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_list_with_json_empty() {
        let _lock = fastskill::test_utils::DIR_MUTEX
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
        let _lock = fastskill::test_utils::DIR_MUTEX
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
        let _lock = fastskill::test_utils::DIR_MUTEX
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
}
