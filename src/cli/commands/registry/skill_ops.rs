use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::messages;
use fastskill::core::registry_index::ListSkillsOptions;
use fastskill::core::repository::{CratesRegistryClient, RepositoryType};

pub async fn execute_list_skills(
    repository: Option<String>,
    scope: Option<String>,
    all_versions: bool,
    include_pre_release: bool,
    json: bool,
    grid: bool,
) -> CliResult<()> {
    if json && grid {
        return Err(CliError::Config(
            "Cannot use both --json and --grid flags. Use only one.".to_string(),
        ));
    }

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
        fastskill::core::repository::RepositoryConfig::HttpRegistry { index_url } => {
            index_url.clone()
        }
        _ => {
            return Err(CliError::Config(
                "Repository does not have index_url configured".to_string(),
            ));
        }
    };

    println!(
        "{}",
        messages::info(&format!("Listing skills from repository: {}", repo_name))
    );

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
        println!("{}", messages::warning("No skills found in repository"));
        return Ok(());
    }

    let output_format = if json { "json" } else { "grid" };
    match output_format {
        "json" => {
            let json_output = serde_json::to_string_pretty(&summaries)
                .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
            println!("{}", json_output);
        }
        "grid" => {
            super::formatters::format_grid_output(&summaries, all_versions)?;
        }
        _ => unreachable!(),
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

pub async fn execute_search(query: String, repository: Option<String>) -> CliResult<()> {
    let repo_manager = super::helpers::load_repo_manager().await?;

    println!(
        "{}",
        messages::info(&format!("Searching repositories for: {}", query))
    );

    let repos = if let Some(repo_name) = repository {
        vec![repo_manager
            .get_repository(&repo_name)
            .ok_or_else(|| CliError::Config(format!("Repository '{}' not found", repo_name)))?]
    } else {
        repo_manager.list_repositories()
    };

    let mut all_results = Vec::new();

    for repo in repos {
        match repo_manager.get_client(&repo.name).await {
            Ok(client) => {
                if let Ok(results) = client.search(&query).await {
                    all_results.extend(results);
                }
            }
            Err(_) => continue,
        }
    }

    if all_results.is_empty() {
        println!("{}", messages::warning("No results found"));
    } else {
        println!("\nFound {} result(s):", all_results.len());
        for result in all_results {
            println!("  - {}: {}", result.name, result.description);
        }
    }

    Ok(())
}
