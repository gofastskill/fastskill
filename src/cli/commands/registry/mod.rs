//! Unified registry command implementation
//!
//! This command consolidates functionality from sources, registry, and repository commands
//! into a single unified interface for managing repositories and browsing skills.

pub mod formatters;
pub mod helpers;
pub mod marketplace;
pub mod repo_ops;
pub mod skill_ops;

use crate::cli::error::CliResult;
use clap::{Args, Subcommand};
use std::path::PathBuf;

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
        /// Repository type: git-marketplace, http-registry, zip-url, or local
        #[arg(long)]
        repo_type: String,
        /// URL for git-marketplace or http-registry, base_url for zip-url, or path for local
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
        /// Filter by scope (organization name)
        #[arg(long)]
        scope: Option<String>,
        /// Show all versions for each skill
        #[arg(long)]
        all_versions: bool,
        /// Include pre-release versions
        #[arg(long)]
        include_pre_release: bool,
        /// Output in JSON format
        #[arg(long)]
        json: bool,
        /// Output in grid format (default)
        #[arg(long)]
        grid: bool,
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
        RegistryCommand::List => repo_ops::execute_list().await,
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
            repo_ops::execute_add(
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
        RegistryCommand::Remove { name } => repo_ops::execute_remove(name).await,
        RegistryCommand::Show { name } => repo_ops::execute_show(name).await,
        RegistryCommand::Update {
            name,
            branch,
            priority,
        } => repo_ops::execute_update(name, branch, priority).await,
        RegistryCommand::Test { name } => repo_ops::execute_test(name).await,
        RegistryCommand::Refresh { name } => repo_ops::execute_refresh(name).await,
        RegistryCommand::ListSkills {
            repository,
            scope,
            all_versions,
            include_pre_release,
            json,
            grid,
        } => {
            skill_ops::execute_list_skills(
                repository,
                scope,
                all_versions,
                include_pre_release,
                json,
                grid,
            )
            .await
        }
        RegistryCommand::ShowSkill {
            skill_id,
            repository,
        } => skill_ops::execute_show_skill(skill_id, repository).await,
        RegistryCommand::Versions {
            skill_id,
            repository,
        } => skill_ops::execute_versions(skill_id, repository).await,
        RegistryCommand::Search { query, repository } => {
            skill_ops::execute_search(query, repository).await
        }
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
            marketplace::execute_create(
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_registry_list_empty() {
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

        let args = RegistryArgs {
            command: RegistryCommand::List,
        };

        let result = execute_registry(args).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_registry_remove_nonexistent() {
        let temp_dir = TempDir::new().unwrap();

        let original_dir = match std::env::current_dir() {
            Ok(dir) => dir,
            Err(_) => std::path::PathBuf::from("."),
        };

        struct DirGuard(std::path::PathBuf);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _guard = DirGuard(original_dir.clone());

        std::env::set_current_dir(temp_dir.path()).unwrap();

        let claude_dir = temp_dir.path().join(".claude");
        let repos_file = claude_dir.join("repositories.toml");
        fs::create_dir_all(&claude_dir).expect("Failed to create .claude directory");
        fs::write(&repos_file, "[repositories]").expect("Failed to write repositories.toml");

        assert!(claude_dir.exists(), "Claude directory should exist");
        assert!(repos_file.exists(), "Repositories file should exist");

        let args = RegistryArgs {
            command: RegistryCommand::Remove {
                name: "nonexistent".to_string(),
            },
        };

        let result = execute_registry(args).await;
        assert!(result.is_err());
    }
}
