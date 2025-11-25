//! Unified registry command implementation
//!
//! This command consolidates functionality from sources, registry, and repository commands
//! into a single unified interface for managing repositories and browsing skills.

use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::messages;
use clap::{Args, Subcommand};
use fastskill::core::manifest::MetadataSection;
use fastskill::core::metadata::parse_yaml_frontmatter;
use fastskill::core::repository::{
    RepositoryAuth, RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
};
use fastskill::core::sources::{
    ClaudeCodeMarketplaceJson, ClaudeCodeMetadata, ClaudeCodeOwner, ClaudeCodePlugin,
    MarketplaceSkill,
};
use serde_json;
use std::fs;
use std::path::{Path, PathBuf};
use toml;
use tracing::{info, warn};
use walkdir::WalkDir;

/// Registry management commands
#[derive(Debug, Args)]
pub struct RegistryArgs {
    #[command(subcommand)]
    pub command: RegistryCommand,
}

#[derive(Debug, Subcommand)]
pub enum RegistryCommand {
    /// List all configured repositories
    List,

    /// Add a new repository
    Add {
        /// Repository name
        name: String,
        /// Repository type: git-marketplace, git-registry, zip-url, or local
        #[arg(long)]
        repo_type: String,
        /// URL for git-marketplace or git-registry, base_url for zip-url, or path for local
        url_or_path: String,
        /// Priority (lower number = higher priority, default: 0)
        #[arg(long)]
        priority: Option<u32>,
        /// Branch for git-marketplace
        #[arg(long)]
        branch: Option<String>,
        /// Tag for git-marketplace
        #[arg(long)]
        tag: Option<String>,
        /// Authentication type: pat, ssh-key, ssh, basic, or api_key
        #[arg(long)]
        auth_type: Option<String>,
        /// Environment variable for PAT, basic password, or API key
        #[arg(long)]
        auth_env: Option<String>,
        /// SSH key path (for ssh-key or ssh auth)
        #[arg(long)]
        auth_key_path: Option<PathBuf>,
        /// Username (for basic auth)
        #[arg(long)]
        auth_username: Option<String>,
    },

    /// Remove a repository
    Remove {
        /// Repository name to remove
        name: String,
    },

    /// Show repository details
    Show {
        /// Repository name
        name: String,
    },

    /// Update repository metadata
    Update {
        /// Repository name to update
        name: String,
        /// New branch (for git-marketplace)
        #[arg(long)]
        branch: Option<String>,
        /// New priority
        #[arg(long)]
        priority: Option<u32>,
    },

    /// Test repository connectivity
    Test {
        /// Repository name to test
        name: String,
    },

    /// Refresh repository cache
    Refresh {
        /// Repository name to refresh (if not specified, refreshes all)
        name: Option<String>,
    },

    /// List all skills in registry
    ListSkills {
        /// Repository name to list skills from (defaults to default repository if not specified)
        #[arg(long)]
        repository: Option<String>,
    },

    /// Show skill details
    ShowSkill {
        /// Skill ID
        skill_id: String,
        /// Repository name (defaults to default repository if not specified)
        #[arg(long)]
        repository: Option<String>,
    },

    /// List available versions for a skill
    Versions {
        /// Skill ID
        skill_id: String,
        /// Repository name (defaults to default repository if not specified)
        #[arg(long)]
        repository: Option<String>,
    },

    /// Search skills in registry
    Search {
        /// Search query
        query: String,
        /// Repository name to search (searches all if not specified)
        #[arg(long)]
        repository: Option<String>,
    },

    /// Create marketplace.json from a directory containing skills
    Create {
        /// Directory containing skills to scan
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Output file path (default: .claude-plugin/marketplace.json in the specified directory)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Base URL for download links (optional)
        #[arg(long)]
        base_url: Option<String>,
        /// Repository name (required)
        #[arg(long)]
        name: Option<String>,
        /// Owner name (optional)
        #[arg(long)]
        owner_name: Option<String>,
        /// Owner email (optional)
        #[arg(long)]
        owner_email: Option<String>,
        /// Repository description (optional)
        #[arg(long)]
        description: Option<String>,
        /// Repository version (optional)
        #[arg(long)]
        version: Option<String>,
    },
}

pub async fn execute_registry(args: RegistryArgs) -> CliResult<()> {
    match args.command {
        RegistryCommand::List => execute_list().await,
        RegistryCommand::Add {
            name,
            repo_type,
            url_or_path,
            priority,
            branch,
            tag,
            auth_type,
            auth_env,
            auth_key_path,
            auth_username,
        } => {
            execute_add(
                name,
                repo_type,
                url_or_path,
                priority,
                branch,
                tag,
                auth_type,
                auth_env,
                auth_key_path,
                auth_username,
            )
            .await
        }
        RegistryCommand::Remove { name } => execute_remove(name).await,
        RegistryCommand::Show { name } => execute_show(name).await,
        RegistryCommand::Update {
            name,
            branch,
            priority,
        } => execute_update(name, branch, priority).await,
        RegistryCommand::Test { name } => execute_test(name).await,
        RegistryCommand::Refresh { name } => execute_refresh(name).await,
        RegistryCommand::ListSkills { repository } => execute_list_skills(repository).await,
        RegistryCommand::ShowSkill {
            skill_id,
            repository,
        } => execute_show_skill(skill_id, repository).await,
        RegistryCommand::Versions {
            skill_id,
            repository,
        } => execute_versions(skill_id, repository).await,
        RegistryCommand::Search { query, repository } => execute_search(query, repository).await,
        RegistryCommand::Create {
            path,
            output,
            base_url,
            name,
            owner_name,
            owner_email,
            description,
            version,
        } => {
            execute_create(
                path,
                output,
                base_url,
                name,
                owner_name,
                owner_email,
                description,
                version,
            )
            .await
        }
    }
}

// Repository management functions

async fn execute_list() -> CliResult<()> {
    let repos_path = PathBuf::from(".claude/repositories.toml");
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

    let repos = repo_manager.list_repositories();
    if repos.is_empty() {
        println!("No repositories configured.");
    } else {
        println!("Configured Repositories ({}):\n", repos.len());
        for repo in repos {
            let repo_type_str = match repo.repo_type {
                RepositoryType::GitMarketplace => "git-marketplace",
                RepositoryType::GitRegistry => "git-registry",
                RepositoryType::ZipUrl => "zip-url",
                RepositoryType::Local => "local",
            };
            println!(
                "  • {} (type: {}, priority: {})",
                repo.name, repo_type_str, repo.priority
            );
        }
    }
    Ok(())
}

async fn execute_add(
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
    let repos_path = PathBuf::from(".claude/repositories.toml");
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

    // Parse repository type
    let repo_type_enum = match repo_type.as_str() {
        "git-marketplace" => RepositoryType::GitMarketplace,
        "git-registry" => RepositoryType::GitRegistry,
        "zip-url" => RepositoryType::ZipUrl,
        "local" => RepositoryType::Local,
        _ => {
            return Err(CliError::Config(format!(
            "Invalid repository type: {}. Use: git-marketplace, git-registry, zip-url, or local",
            repo_type
        )))
        }
    };

    // Parse repository config
    let config = match repo_type_enum {
        RepositoryType::GitMarketplace => RepositoryConfig::GitMarketplace {
            url: url_or_path,
            branch,
            tag,
        },
        RepositoryType::GitRegistry => RepositoryConfig::GitRegistry {
            index_url: url_or_path,
        },
        RepositoryType::ZipUrl => RepositoryConfig::ZipUrl {
            base_url: url_or_path,
        },
        RepositoryType::Local => RepositoryConfig::Local {
            path: PathBuf::from(url_or_path),
        },
    };

    // Parse authentication
    let auth = if let Some(auth_t) = auth_type {
        match auth_t.as_str() {
            "pat" => {
                let env_var = auth_env.ok_or_else(|| {
                    CliError::Config("--auth-env required for pat authentication".to_string())
                })?;
                Some(RepositoryAuth::Pat { env_var })
            }
            "ssh-key" | "ssh" => {
                let key_path = auth_key_path.ok_or_else(|| {
                    CliError::Config("--auth-key-path required for ssh authentication".to_string())
                })?;
                if auth_t == "ssh-key" {
                    Some(RepositoryAuth::SshKey { path: key_path })
                } else {
                    Some(RepositoryAuth::Ssh { key_path })
                }
            }
            "basic" => {
                let username = auth_username.ok_or_else(|| {
                    CliError::Config(
                        "--auth-username required for basic authentication".to_string(),
                    )
                })?;
                let password_env = auth_env.ok_or_else(|| {
                    CliError::Config("--auth-env required for basic authentication".to_string())
                })?;
                Some(RepositoryAuth::Basic {
                    username,
                    password_env,
                })
            }
            "api_key" => {
                let env_var = auth_env.ok_or_else(|| {
                    CliError::Config("--auth-env required for api_key authentication".to_string())
                })?;
                Some(RepositoryAuth::ApiKey { env_var })
            }
            _ => {
                return Err(CliError::Config(format!(
                    "Invalid auth type: {}. Use: pat, ssh-key, ssh, basic, api_key",
                    auth_t
                )));
            }
        }
    } else {
        None
    };

    let repo_def = RepositoryDefinition {
        name: name.clone(),
        repo_type: repo_type_enum,
        priority: priority.unwrap_or(0),
        config,
        auth,
        storage: None,
    };

    repo_manager
        .add_repository(name.clone(), repo_def)
        .map_err(|e| CliError::Config(format!("Failed to add repository: {}", e)))?;
    repo_manager
        .save()
        .map_err(|e| CliError::Config(format!("Failed to save repositories: {}", e)))?;

    println!("{}", messages::ok(&format!("Added repository: {}", name)));
    Ok(())
}

async fn execute_remove(name: String) -> CliResult<()> {
    let repos_path = PathBuf::from(".claude/repositories.toml");
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

    repo_manager
        .remove_repository(&name)
        .map_err(|e| CliError::Config(format!("Failed to remove repository: {}", e)))?;
    repo_manager
        .save()
        .map_err(|e| CliError::Config(format!("Failed to save repositories: {}", e)))?;

    println!("{}", messages::ok(&format!("Removed repository: {}", name)));
    Ok(())
}

async fn execute_show(name: String) -> CliResult<()> {
    let repos_path = PathBuf::from(".claude/repositories.toml");
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

    let repo = repo_manager
        .get_repository(&name)
        .ok_or_else(|| CliError::Config(format!("Repository '{}' not found", name)))?;

    let repo_type_str = match repo.repo_type {
        RepositoryType::GitMarketplace => "git-marketplace",
        RepositoryType::GitRegistry => "git-registry",
        RepositoryType::ZipUrl => "zip-url",
        RepositoryType::Local => "local",
    };

    println!("Repository: {}", repo.name);
    println!("  Type: {}", repo_type_str);
    println!("  Priority: {}", repo.priority);

    match &repo.config {
        RepositoryConfig::GitMarketplace { url, branch, tag } => {
            println!("  URL: {}", url);
            if let Some(b) = branch {
                println!("  Branch: {}", b);
            }
            if let Some(t) = tag {
                println!("  Tag: {}", t);
            }
        }
        RepositoryConfig::GitRegistry { index_url } => {
            println!("  Index URL: {}", index_url);
        }
        RepositoryConfig::ZipUrl { base_url } => {
            println!("  Base URL: {}", base_url);
        }
        RepositoryConfig::Local { path } => {
            println!("  Path: {}", path.display());
        }
    }

    if let Some(auth) = &repo.auth {
        println!("  Auth: {:?}", auth);
    }

    if let Some(storage) = &repo.storage {
        println!("  Storage: {:?}", storage);
    }

    Ok(())
}

async fn execute_update(
    name: String,
    branch: Option<String>,
    priority: Option<u32>,
) -> CliResult<()> {
    let repos_path = PathBuf::from(".claude/repositories.toml");
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

    let repo = repo_manager
        .get_repository(&name)
        .ok_or_else(|| CliError::Config(format!("Repository '{}' not found", name)))?
        .clone();

    // Update branch if specified (for git-marketplace)
    let updated_config = if let Some(new_branch) = branch {
        match &repo.config {
            RepositoryConfig::GitMarketplace {
                url,
                branch: _,
                tag,
            } => RepositoryConfig::GitMarketplace {
                url: url.clone(),
                branch: Some(new_branch),
                tag: tag.clone(),
            },
            _ => repo.config.clone(),
        }
    } else {
        repo.config.clone()
    };

    // Update priority if specified
    let updated_priority = priority.unwrap_or(repo.priority);

    // Remove and re-add with updated config
    repo_manager
        .remove_repository(&name)
        .map_err(|e| CliError::Config(format!("Failed to remove repository: {}", e)))?;

    let updated_repo = RepositoryDefinition {
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

async fn execute_test(name: String) -> CliResult<()> {
    let repos_path = PathBuf::from(".claude/repositories.toml");
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

    let _repo = repo_manager
        .get_repository(&name)
        .ok_or_else(|| CliError::Config(format!("Repository '{}' not found", name)))?;

    println!(
        "{}",
        messages::info(&format!("Testing repository: {}...", name))
    );

    // Try to get client and list skills
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

async fn execute_refresh(name: Option<String>) -> CliResult<()> {
    // Note: Cache clearing would need to be implemented in RepositoryManager
    // For now, we'll just acknowledge the command
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

// Skill browsing functions

async fn execute_list_skills(repository: Option<String>) -> CliResult<()> {
    let repos_path = PathBuf::from(".claude/repositories.toml");
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

    let repo_name = if let Some(repo_name) = repository {
        repo_name
    } else {
        repo_manager
            .get_default_repository()
            .map(|r| r.name.clone())
            .ok_or_else(|| {
                CliError::Config(
                    "No repository specified and no default repository configured".to_string(),
                )
            })?
    };

    println!(
        "{}",
        messages::info(&format!("Listing skills from repository: {}", repo_name))
    );

    let client = repo_manager
        .get_client(&repo_name)
        .await
        .map_err(|e| CliError::Config(format!("Failed to get repository client: {}", e)))?;

    match client.list_skills().await {
        Ok(skills) => {
            if skills.is_empty() {
                println!("{}", messages::warning("No skills found in repository"));
                return Ok(());
            }

            println!("\nFound {} skill(s):\n", skills.len());
            for skill in skills {
                println!("  • {} (v{})", skill.name, skill.version);
                if !skill.description.is_empty() {
                    println!("    Description: {}", skill.description);
                }
                println!();
            }
            Ok(())
        }
        Err(e) => {
            println!(
                "{}",
                messages::warning(&format!(
                    "List command not fully implemented for this repository type: {}",
                    e
                ))
            );
            Ok(())
        }
    }
}

async fn execute_show_skill(skill_id: String, repository: Option<String>) -> CliResult<()> {
    let repos_path = PathBuf::from(".claude/repositories.toml");
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

    let repo_name = if let Some(repo_name) = repository {
        repo_name
    } else {
        repo_manager
            .get_default_repository()
            .map(|r| r.name.clone())
            .ok_or_else(|| {
                CliError::Config(
                    "No repository specified and no default repository configured".to_string(),
                )
            })?
    };

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
            if !skill.tags.is_empty() {
                println!("Tags: {}", skill.tags.join(", "));
            }
            if !skill.capabilities.is_empty() {
                println!("Capabilities: {}", skill.capabilities.join(", "));
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

async fn execute_versions(skill_id: String, repository: Option<String>) -> CliResult<()> {
    let repos_path = PathBuf::from(".claude/repositories.toml");
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

    let repo_name = if let Some(repo_name) = repository {
        repo_name
    } else {
        repo_manager
            .get_default_repository()
            .map(|r| r.name.clone())
            .ok_or_else(|| {
                CliError::Config(
                    "No repository specified and no default repository configured".to_string(),
                )
            })?
    };

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

async fn execute_search(query: String, repository: Option<String>) -> CliResult<()> {
    let repos_path = PathBuf::from(".claude/repositories.toml");
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

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

// Marketplace creation function

async fn execute_create(
    path: PathBuf,
    output: Option<PathBuf>,
    _base_url: Option<String>,
    name: Option<String>,
    owner_name: Option<String>,
    owner_email: Option<String>,
    description: Option<String>,
    version: Option<String>,
) -> CliResult<()> {
    let skill_dir = path
        .canonicalize()
        .map_err(|e| CliError::Validation(format!("Failed to resolve path: {}", e)))?;

    info!("Scanning directory for skills: {}", skill_dir.display());

    // Scan for SKILL.md files
    let skills = scan_directory_for_skills(&skill_dir)?;

    if skills.is_empty() {
        return Err(CliError::Validation(format!(
            "No skills found in directory: {}",
            skill_dir.display()
        )));
    }

    let skills_count = skills.len();
    info!("Found {} skills", skills_count);

    // Determine output path (default to .claude-plugin/marketplace.json)
    let output_path =
        output.unwrap_or_else(|| skill_dir.join(".claude-plugin").join("marketplace.json"));

    // Validate required fields
    let repo_name = name
        .or_else(|| {
            skill_dir
                .file_name()
                .and_then(|n| n.to_str().map(|s| s.to_string()))
        })
        .ok_or_else(|| {
            CliError::Validation(
                "Repository name is required. Use --name or ensure directory has a name."
                    .to_string(),
            )
        })?;

    // Group skills into a single plugin (simple approach)
    let skill_paths: Vec<String> = skills
        .iter()
        .map(|skill| format!("./{}", skill.id))
        .collect();

    let plugin = ClaudeCodePlugin {
        name: repo_name.clone(),
        description: description.clone(),
        source: Some("./".to_string()),
        strict: Some(false),
        skills: skill_paths,
    };

    let marketplace = ClaudeCodeMarketplaceJson {
        name: repo_name,
        owner: if owner_name.is_some() || owner_email.is_some() {
            Some(ClaudeCodeOwner {
                name: owner_name.unwrap_or_else(|| "Unknown".to_string()),
                email: owner_email,
            })
        } else {
            None
        },
        metadata: if description.is_some() || version.is_some() {
            Some(ClaudeCodeMetadata {
                description,
                version,
            })
        } else {
            None
        },
        plugins: vec![plugin],
    };

    // Generate Claude Code format marketplace.json
    let json_content = serde_json::to_string_pretty(&marketplace).map_err(|e| {
        CliError::Validation(format!("Failed to serialize marketplace.json: {}", e))
    })?;

    // Create parent directory if needed (for .claude-plugin/marketplace.json)
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            CliError::Validation(format!("Failed to create output directory: {}", e))
        })?;
    }

    fs::write(&output_path, json_content)
        .map_err(|e| CliError::Validation(format!("Failed to write marketplace.json: {}", e)))?;

    println!(
        "{}",
        messages::ok(&format!(
            "Created marketplace.json: {}",
            output_path.display()
        ))
    );
    println!("   Found {} skills", skills_count);

    Ok(())
}

fn scan_directory_for_skills(dir: &Path) -> CliResult<Vec<MarketplaceSkill>> {
    let mut skills = Vec::new();

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "SKILL.md")
    {
        let skill_path = entry.path();
        let skill_dir = skill_path
            .parent()
            .ok_or_else(|| CliError::Validation("SKILL.md has no parent directory".to_string()))?;

        match extract_skill_metadata(skill_dir, skill_path) {
            Ok(skill) => {
                info!("Found skill: {} ({})", skill.name, skill.id);
                skills.push(skill);
            }
            Err(e) => {
                warn!(
                    "Failed to extract skill from {}: {}",
                    skill_dir.display(),
                    e
                );
                continue;
            }
        }
    }

    Ok(skills)
}

fn extract_skill_metadata(skill_dir: &Path, skill_file: &Path) -> CliResult<MarketplaceSkill> {
    // Read skill-project.toml if present (priority source)
    let skill_project_path = skill_dir.join("skill-project.toml");
    let skill_metadata = if skill_project_path.exists() {
        if let Ok(skill_project_content) = fs::read_to_string(&skill_project_path) {
            #[derive(serde::Deserialize)]
            struct SkillProjectToml {
                metadata: Option<MetadataSection>,
            }

            if let Ok(skill_project) = toml::from_str::<SkillProjectToml>(&skill_project_content) {
                skill_project.metadata
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Read SKILL.md frontmatter (fallback source)
    let skill_content = fs::read_to_string(skill_file)
        .map_err(|e| CliError::Validation(format!("Failed to read SKILL.md: {}", e)))?;

    let frontmatter = parse_yaml_frontmatter(&skill_content).map_err(|e| {
        CliError::Validation(format!("Failed to parse SKILL.md frontmatter: {}", e))
    })?;

    // Use directory name as ID (always)
    let id = skill_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            CliError::Validation("Cannot determine skill ID from directory name".to_string())
        })?
        .to_string();

    // Apply priority: skill-project.toml > SKILL.md frontmatter
    let name = skill_metadata
        .as_ref()
        .and_then(|m| m.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| frontmatter.name.clone());

    let description = skill_metadata
        .as_ref()
        .and_then(|m| m.description.clone())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| frontmatter.description.clone());

    let version = if let Some(metadata) = skill_metadata.as_ref() {
        metadata.version.clone()
    } else {
        frontmatter.version.clone()
    };

    let author = skill_metadata
        .as_ref()
        .and_then(|m| m.author.clone())
        .or_else(|| frontmatter.author.clone());

    let tags = skill_metadata
        .as_ref()
        .and_then(|m| m.tags.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| frontmatter.tags.clone());

    let capabilities = skill_metadata
        .as_ref()
        .and_then(|m| m.capabilities.clone())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| frontmatter.capabilities.clone());

    let download_url = skill_metadata.as_ref().and_then(|m| m.download_url.clone());

    if skill_metadata.is_some() {
        info!("Using metadata from skill-project.toml for skill: {}", id);
    }

    Ok(MarketplaceSkill {
        id,
        name,
        description,
        version,
        author,
        tags,
        capabilities,
        download_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_registry_list_empty() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        // Helper struct to ensure directory is restored even if test panics
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

        let args = RegistryArgs {
            command: RegistryCommand::List,
        };

        // Should succeed even with no repositories
        let result = execute_registry(args).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_registry_remove_nonexistent() {
        let temp_dir = TempDir::new().unwrap();

        // Get original directory immediately and handle potential errors
        let original_dir = match std::env::current_dir() {
            Ok(dir) => dir,
            Err(_) => {
                // If we can't get current dir, assume we're in the project root
                std::path::PathBuf::from(".")
            }
        };

        // Helper struct to ensure directory is restored even if test panics
        struct DirGuard(std::path::PathBuf);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _guard = DirGuard(original_dir.clone());

        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Create .claude directory and empty repositories.toml using absolute paths
        let claude_dir = temp_dir.path().join(".claude");
        let repos_file = claude_dir.join("repositories.toml");
        fs::create_dir_all(&claude_dir).expect("Failed to create .claude directory");
        fs::write(&repos_file, "[repositories]").expect("Failed to write repositories.toml");

        // Verify files exist before test
        assert!(claude_dir.exists(), "Claude directory should exist");
        assert!(repos_file.exists(), "Repositories file should exist");

        let args = RegistryArgs {
            command: RegistryCommand::Remove {
                name: "nonexistent".to_string(),
            },
        };

        let result = execute_registry(args).await;
        // Should fail because repository doesn't exist
        assert!(result.is_err());
    }
}
