use crate::commands::common::validate_format_args;
use crate::error::{CliError, CliResult};
use crate::utils::messages;
use fastskill_core::core::registry_index::ListSkillsOptions;
use fastskill_core::core::repository::{CratesRegistryClient, RepositoryType};
use fastskill_core::OutputFormat;

pub async fn execute_list_skills(
    repository: Option<String>,
    scope: Option<String>,
    all_versions: bool,
    include_pre_release: bool,
    format: Option<OutputFormat>,
    json: bool,
) -> CliResult<()> {
    let resolved_format = validate_format_args(&format, json)?;

    if let Some(ref scope) = scope {
        if scope.is_empty() {
            return Err(CliError::Config(
                "Scope cannot be empty. Use a valid organization name.".to_string(),
            ));
        }
        if scope.contains('/') || scope.contains('\\') || scope.contains("..") {
            return Err(CliError::Config(
                format!(
                    "Invalid scope format: '{}'. Scope must be a valid organization name without path separators.",
                    scope
                )
            ));
        }
        if !scope
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(CliError::Config(
                format!(
                    "Invalid scope format: '{}'. Scope must contain only alphanumeric characters, hyphens, and underscores.",
                    scope
                )
            ));
        }
    }

    let repo_manager = super::helpers::load_repo_manager().await?;

    let repo_name = super::helpers::resolve_repository_name(&repo_manager, repository)?;

    let repo_def = repo_manager
        .get_repository(&repo_name)
        .ok_or_else(|| CliError::Config(format!("Repository '{}' not found", repo_name)))?;

    if repo_def.repo_type != RepositoryType::HttpRegistry {
        if scope.is_some() || all_versions || include_pre_release {
            return Err(CliError::Config(format!(
                "Repository '{}' supports catalog listing, but --scope, --all-versions, and --include-pre-release require an HTTP registry index",
                repo_name
            )));
        }
        let client = repo_manager
            .get_client(&repo_name)
            .await
            .map_err(|error| CliError::Config(format!("Failed to open repository: {error}")))?;
        let skills = client
            .list_skills()
            .await
            .map_err(|error| CliError::Config(format!("Failed to list repository: {error}")))?;
        match resolved_format {
            OutputFormat::Json => crate::outln!(
                "{}",
                serde_json::to_string_pretty(&skills).map_err(|error| CliError::Config(
                    format!("Failed to serialize JSON: {error}")
                ))?
            ),
            OutputFormat::Xml => {
                crate::outln!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
                crate::outln!("<skills>");
                for skill in &skills {
                    crate::outln!(
                        "  <skill id=\"{}\" version=\"{}\" />",
                        fastskill_core::output::escape_xml(skill.id.as_str()),
                        fastskill_core::output::escape_xml(&skill.version)
                    );
                }
                crate::outln!("</skills>");
            }
            OutputFormat::Table | OutputFormat::Grid => {
                if skills.is_empty() {
                    crate::outln!("{}", messages::warning("No skills found in repository"));
                } else {
                    for skill in skills {
                        crate::outln!("{} {} {}", skill.id, skill.version, skill.name);
                    }
                }
            }
        }
        return Ok(());
    }

    let _index_url = match &repo_def.config {
        fastskill_core::core::repository::RepositoryConfig::HttpRegistry { index_url } => {
            index_url.clone()
        }
        _ => {
            return Err(CliError::Config(
                "Repository does not have index_url configured".to_string(),
            ));
        }
    };

    if matches!(resolved_format, OutputFormat::Table | OutputFormat::Grid) {
        crate::outln!(
            "{}",
            messages::info(&format!("Listing skills from repository: {}", repo_name))
        );
    }

    let http_client = CratesRegistryClient::new(repo_def)
        .map_err(|e| CliError::Config(format!("Failed to create HTTP registry client: {}", e)))?;

    let options = ListSkillsOptions {
        scope,
        all_versions,
        include_pre_release,
    };

    let summaries = http_client
        .fetch_skills(&options)
        .await
        .map_err(|e| CliError::Config(format!("Failed to fetch skills from registry: {}", e)))?;

    if summaries.is_empty() {
        match resolved_format {
            OutputFormat::Json => {
                crate::outln!("[]");
            }
            OutputFormat::Xml => {
                super::formatters::format_xml_output(&summaries)?;
            }
            OutputFormat::Table | OutputFormat::Grid => {
                crate::outln!("{}", messages::warning("No skills found in repository"));
            }
        }
        return Ok(());
    }

    match resolved_format {
        OutputFormat::Json => {
            let json_output = serde_json::to_string_pretty(&summaries)
                .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
            crate::outln!("{}", json_output);
        }
        OutputFormat::Table => {
            super::formatters::format_table_output(&summaries, all_versions)?;
        }
        OutputFormat::Grid => {
            super::formatters::format_grid_output(&summaries, all_versions)?;
        }
        OutputFormat::Xml => super::formatters::format_xml_output(&summaries)?,
    }

    Ok(())
}

pub async fn execute_show_skill(skill_id: String, repository: Option<String>) -> CliResult<()> {
    let repo_manager = super::helpers::load_repo_manager().await?;

    let repo_name = super::helpers::resolve_repository_name(&repo_manager, repository)?;

    crate::outln!(
        "{}",
        messages::info(&format!("Fetching skill: {} from {}", skill_id, repo_name))
    );

    let client = repo_manager
        .get_client(&repo_name)
        .await
        .map_err(|e| CliError::Config(format!("Failed to get repository client: {}", e)))?;

    match client.get_skill(&skill_id, None).await {
        Ok(Some(skill)) => {
            crate::outln!("\nSkill: {}", skill.name);
            crate::outln!("Version: {}", skill.version);
            if !skill.description.is_empty() {
                crate::outln!("Description: {}", skill.description);
            }
            if let Some(author) = &skill.author {
                crate::outln!("Author: {}", author);
            }
        }
        Ok(None) => {
            return Err(CliError::Config(format!(
                "Skill '{}' not found in repository '{}'",
                skill_id, repo_name
            )));
        }
        Err(e) => {
            return Err(CliError::Config(format!("Failed to get skill: {}", e)));
        }
    }

    Ok(())
}

pub async fn execute_versions(skill_id: String, repository: Option<String>) -> CliResult<()> {
    let repo_manager = super::helpers::load_repo_manager().await?;

    let repo_name = super::helpers::resolve_repository_name(&repo_manager, repository)?;

    crate::outln!(
        "{}",
        messages::info(&format!(
            "Fetching versions for: {} from {}",
            skill_id, repo_name
        ))
    );

    let client = repo_manager
        .get_client(&repo_name)
        .await
        .map_err(|e| CliError::Config(format!("Failed to get repository client: {}", e)))?;

    match client.get_versions(&skill_id).await {
        Ok(versions) => {
            if versions.is_empty() {
                crate::outln!(
                    "{}",
                    messages::warning(&format!("No versions found for skill: {}", skill_id))
                );
                return Ok(());
            }

            crate::outln!("\nAvailable versions:");
            for version in versions {
                crate::outln!("  - {}", version);
            }
            Ok(())
        }
        Err(e) => Err(CliError::Config(format!("Failed to get versions: {}", e))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::await_holding_lock)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct RestoreDirectory(std::path::PathBuf);

    impl Drop for RestoreDirectory {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    async fn local_catalog() -> (TempDir, RestoreDirectory) {
        let temp = TempDir::new().unwrap();
        let restore = RestoreDirectory(std::env::current_dir().unwrap());
        std::env::set_current_dir(temp.path()).unwrap();
        std::fs::create_dir_all(temp.path().join(".claude/skills")).unwrap();
        std::fs::write(
            temp.path().join("skill-project.toml"),
            "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
        )
        .unwrap();
        let catalog = temp.path().join("catalog/demo");
        std::fs::create_dir_all(&catalog).unwrap();
        std::fs::write(
            catalog.join("SKILL.md"),
            "---\nname: demo\nversion: 1.2.3\ndescription: catalog fixture\nauthor: FastSkill\n---\n# Demo\n",
        )
        .unwrap();
        crate::commands::repos::repo_ops::execute_add(
            "local".to_string(),
            "local".to_string(),
            temp.path().join("catalog").display().to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        (temp, restore)
    }

    #[tokio::test]
    async fn local_adapter_supports_all_catalog_formats_show_and_versions() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (_temp, _restore) = local_catalog().await;
        for format in [
            Some(OutputFormat::Json),
            Some(OutputFormat::Xml),
            Some(OutputFormat::Table),
            Some(OutputFormat::Grid),
        ] {
            assert!(execute_list_skills(
                Some("local".to_string()),
                None,
                false,
                false,
                format,
                false,
            )
            .await
            .is_ok());
        }
        assert!(execute_list_skills(
            Some("local".to_string()),
            Some("scope".to_string()),
            false,
            false,
            None,
            false,
        )
        .await
        .is_err());
        assert!(
            execute_show_skill("demo".to_string(), Some("local".to_string()))
                .await
                .is_ok()
        );
        assert!(
            execute_show_skill("absent".to_string(), Some("local".to_string()))
                .await
                .is_err()
        );
        assert!(
            execute_versions("demo".to_string(), Some("local".to_string()))
                .await
                .is_ok()
        );
        assert!(
            execute_versions("absent".to_string(), Some("local".to_string()))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn invalid_scope_is_rejected_before_repository_loading() {
        for scope in ["", "../team", "team/name", "team name"] {
            assert!(
                execute_list_skills(None, Some(scope.to_string()), false, false, None, false,)
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn http_registry_lists_nonempty_and_empty_catalogs_in_every_format() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (_temp, _restore) = local_catalog().await;
        let server = MockServer::start().await;
        let body = serde_json::json!([{
            "id": "team/demo",
            "scope": "team",
            "name": "Demo",
            "description": "HTTP fixture",
            "latest_version": "1.2.3",
            "published_at": null,
            "versions": ["1.2.3", "1.1.0"]
        }]);
        Mock::given(method("GET"))
            .and(path("/api/v1/registry/index/skills"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        crate::commands::repos::repo_ops::execute_add(
            "registry".to_string(),
            "http-registry".to_string(),
            server.uri(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        for format in [
            Some(OutputFormat::Json),
            Some(OutputFormat::Xml),
            Some(OutputFormat::Table),
            Some(OutputFormat::Grid),
        ] {
            execute_list_skills(
                Some("registry".to_string()),
                None,
                true,
                true,
                format,
                false,
            )
            .await
            .unwrap();
        }

        Mock::given(method("GET"))
            .and(path("/api/v1/registry/index/skills"))
            .and(query_param("scope", "empty"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .with_priority(1)
            .mount(&server)
            .await;
        for format in [
            Some(OutputFormat::Json),
            Some(OutputFormat::Xml),
            Some(OutputFormat::Table),
        ] {
            execute_list_skills(
                Some("registry".to_string()),
                Some("empty".to_string()),
                false,
                false,
                format,
                false,
            )
            .await
            .unwrap();
        }
    }
}
