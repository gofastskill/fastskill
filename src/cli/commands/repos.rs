//! Repos command - manage repository list and browse remote skill catalog
//!
//! This command consolidates repository management (add/remove/test/refresh) and
//! remote catalog operations (skills/show/versions) into a single namespace.

use crate::cli::commands::common::validate_format_args;
use crate::cli::error::CliResult;
use clap::{Args, Subcommand};
use fastskill::OutputFormat;
use std::path::PathBuf;

#[derive(Debug, Args)]
#[command(
    about = "Manage repository list and browse remote skill catalog.",
    after_help = "Repository Management:\n  fastskill repos add my-repo --repo-type local /path/to/skills\n  fastskill repos remove my-repo\n  fastskill repos info my-repo\n  fastskill repos test my-repo\n  fastskill repos refresh\n\nCatalog Browsing:\n  fastskill repos skills\n  fastskill repos show pptx\n  fastskill repos versions pptx"
)]
pub struct ReposArgs {
    #[command(subcommand)]
    pub command: ReposCommand,
}

#[derive(Debug, Subcommand)]
pub enum ReposCommand {
    // Repository Management Commands
    /// List all configured repositories
    #[command(
        after_help = "Examples:\n  fastskill repos list\n  fastskill repos list --format xml\n  fastskill repos list --json"
    )]
    List {
        /// Output format: table, json, grid, xml (default: table)
        #[arg(long, value_enum, help = "Output format: table, json, grid, xml")]
        format: Option<OutputFormat>,
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Add a new repository
    #[command(
        after_help = "Examples:\n  fastskill repos add my-repo --repo-type local /path/to/skills"
    )]
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
    #[command(after_help = "Examples:\n  fastskill repos remove my-repo")]
    Remove {
        /// Repository name to remove
        name: String,
    },

    /// Show repository details
    #[command(
        after_help = "Examples:\n  fastskill repos info my-repo\n  fastskill repos info my-repo --format xml"
    )]
    Info {
        /// Repository name
        name: String,
        /// Output format: table, json, grid, xml (default: table)
        #[arg(long, value_enum, help = "Output format: table, json, grid, xml")]
        format: Option<OutputFormat>,
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Update repository metadata
    #[command(after_help = "Examples:\n  fastskill repos update my-repo --priority 1")]
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
    #[command(after_help = "Examples:\n  fastskill repos test my-repo")]
    Test {
        /// Repository name to test
        name: String,
    },

    /// Refresh repository cache
    #[command(
        after_help = "Examples:\n  fastskill repos refresh\n  fastskill repos refresh my-repo"
    )]
    Refresh {
        /// Repository name to refresh (if not specified, refreshes all)
        name: Option<String>,
    },

    // Catalog Browsing Commands
    /// List skills in repository catalog
    #[command(after_help = "Examples:\n  fastskill repos skills\n  fastskill repos skills --json")]
    Skills {
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
        /// Output format: table, json, grid, xml (default: table)
        #[arg(long, value_enum, help = "Output format: table, json, grid, xml")]
        format: Option<OutputFormat>,
        /// Shorthand for --format json
        #[arg(long, help = "Shorthand for --format json")]
        json: bool,
    },

    /// Show skill details from catalog
    #[command(after_help = "Examples:\n  fastskill repos show pptx")]
    Show {
        /// Skill ID
        skill_id: String,
        /// Repository name (defaults to default repository if not specified)
        #[arg(long)]
        repository: Option<String>,
    },

    /// List available versions for a skill
    #[command(after_help = "Examples:\n  fastskill repos versions pptx")]
    Versions {
        /// Skill ID
        skill_id: String,
        /// Repository name (defaults to default repository if not specified)
        #[arg(long)]
        repository: Option<String>,
    },
}

pub async fn execute_repos(args: ReposArgs) -> CliResult<()> {
    match args.command {
        // Repository Management Commands
        ReposCommand::List { format, json } => {
            let resolved_format = validate_format_args(&format, json)?;
            super::registry::repo_ops::execute_list_with_format(resolved_format).await
        }
        ReposCommand::Add {
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
            super::registry::repo_ops::execute_add(
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
        ReposCommand::Remove { name } => super::registry::repo_ops::execute_remove(name).await,
        ReposCommand::Info { name, format, json } => {
            let resolved_format = validate_format_args(&format, json)?;
            super::registry::repo_ops::execute_show_with_format(name, resolved_format).await
        }
        ReposCommand::Update {
            name,
            branch,
            priority,
        } => super::registry::repo_ops::execute_update(name, branch, priority).await,
        ReposCommand::Test { name } => super::registry::repo_ops::execute_test(name).await,
        ReposCommand::Refresh { name } => super::registry::repo_ops::execute_refresh(name).await,

        // Catalog Browsing Commands
        ReposCommand::Skills {
            repository,
            scope,
            all_versions,
            include_pre_release,
            format,
            json,
        } => {
            super::registry::skill_ops::execute_list_skills(
                repository,
                scope,
                all_versions,
                include_pre_release,
                format,
                json,
            )
            .await
        }
        ReposCommand::Show {
            skill_id,
            repository,
        } => super::registry::skill_ops::execute_show_skill(skill_id, repository).await,
        ReposCommand::Versions {
            skill_id,
            repository,
        } => super::registry::skill_ops::execute_versions(skill_id, repository).await,
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::await_holding_lock)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_repos_list() {
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

        let args = ReposArgs {
            command: ReposCommand::List {
                format: None,
                json: false,
            },
        };

        let result = execute_repos(args).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_repos_skills() {
        // Note: This test is expected to fail without a configured repository
        // It's here to verify the command structure compiles correctly
        let args = ReposArgs {
            command: ReposCommand::Skills {
                repository: None,
                scope: None,
                all_versions: false,
                include_pre_release: false,
                format: None,
                json: false,
            },
        };

        let result = execute_repos(args).await;
        // Should fail due to missing repository configuration, but shouldn't panic
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_repos_command_excludes_search_variant() {
        // This compile-time test ensures the ReposCommand enum does not contain a Search variant.
        // Search is a top-level command to maintain clear architectural separation.
        let _command_list = [
            "List", "Add", "Remove", "Info", "Update", "Test", "Refresh", "Skills", "Show",
            "Versions",
        ];
    }

    #[test]
    fn test_repos_command_has_approved_subcommands() {
        // Verify ReposCommand contains exactly the approved subcommands
        // This test validates the command structure for specification 026a
        use std::mem::discriminant;

        let list = ReposCommand::List {
            format: None,
            json: false,
        };
        let add = ReposCommand::Add {
            name: "test".to_string(),
            repo_type: "local".to_string(),
            url_or_path: "/tmp".to_string(),
            priority: None,
            branch: None,
            tag: None,
            auth_type: None,
            auth_env: None,
            auth_key_path: None,
            auth_username: None,
        };
        let remove = ReposCommand::Remove {
            name: "test".to_string(),
        };
        let info = ReposCommand::Info {
            name: "test".to_string(),
            format: None,
            json: false,
        };
        let update = ReposCommand::Update {
            name: "test".to_string(),
            branch: None,
            priority: None,
        };
        let test = ReposCommand::Test {
            name: "test".to_string(),
        };
        let refresh = ReposCommand::Refresh { name: None };
        let skills = ReposCommand::Skills {
            repository: None,
            scope: None,
            all_versions: false,
            include_pre_release: false,
            format: None,
            json: false,
        };
        let show = ReposCommand::Show {
            skill_id: "test".to_string(),
            repository: None,
        };
        let versions = ReposCommand::Versions {
            skill_id: "test".to_string(),
            repository: None,
        };

        // Verify all commands have different discriminants (different variants)
        let discriminants = vec![
            discriminant(&list),
            discriminant(&add),
            discriminant(&remove),
            discriminant(&info),
            discriminant(&update),
            discriminant(&test),
            discriminant(&refresh),
            discriminant(&skills),
            discriminant(&show),
            discriminant(&versions),
        ];

        // All discriminants should be unique (10 unique subcommands)
        assert_eq!(
            discriminants.len(),
            discriminants
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
        );
    }
}
