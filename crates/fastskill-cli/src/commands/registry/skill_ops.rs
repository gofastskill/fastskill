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
        return Err(CliError::Config(
            format!(
                "Repository '{}' is not an HTTP registry. This command only works with HTTP registries.",
                repo_name
            )
        ));
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
        println!(
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
                println!("[]");
            }
            OutputFormat::Xml => {
                super::formatters::format_xml_output(&summaries)?;
            }
            OutputFormat::Table | OutputFormat::Grid => {
                println!("{}", messages::warning("No skills found in repository"));
            }
        }
        return Ok(());
    }

    match resolved_format {
        OutputFormat::Json => {
            let json_output = serde_json::to_string_pretty(&summaries)
                .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
            println!("{}", json_output);
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

    println!(
        "{}",
        messages::info(&format!("Fetching skill: {} from {}", skill_id, repo_name))
    );

    let client = repo_manager
        .get_client(&repo_name)
        .await
        .map_err(|e| CliError::Config(format!("Failed to get repository client: {}", e)))?;

    match client.get_skill(&skill_id, None).await {
        Ok(Some(skill)) => {
            println!("\nSkill: {}", skill.name);
            println!("Version: {}", skill.version);
            if !skill.description.is_empty() {
                println!("Description: {}", skill.description);
            }
            if let Some(author) = &skill.author {
                println!("Author: {}", author);
            }
        }
        Ok(None) => {
            println!(
                "{}",
                messages::warning(&format!("Skill '{}' not found in repository", skill_id))
            );
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

    println!(
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
                println!(
                    "{}",
                    messages::warning(&format!("No versions found for skill: {}", skill_id))
                );
                return Ok(());
            }

            println!("\nAvailable versions:");
            for version in versions {
                println!("  - {}", version);
            }
            Ok(())
        }
        Err(e) => Err(CliError::Config(format!("Failed to get versions: {}", e))),
    }
}
