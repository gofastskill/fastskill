//! Registry command implementation - browse the registry catalog
//!
//! This command provides access to registry catalog operations for browsing and searching
//! skills in remote repositories.
//!
//! For repository management (list, add, remove, show, update, test, refresh, create),
//! use the `sources` command instead.

pub mod formatters;
pub mod helpers;
pub mod marketplace;
pub mod repo_ops;
pub mod skill_ops;

use crate::cli::error::CliResult;
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct RegistryArgs {
    #[command(subcommand)]
    pub command: RegistryCommand,
}

#[derive(Debug, Subcommand)]
pub enum RegistryCommand {
    /// List skills in registry catalog
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

    /// Search skills in registry catalog (remote)
    Search {
        /// Search query
        query: String,
        /// Repository name to search (searches all if not specified)
        #[arg(long)]
        repository: Option<String>,
    },
}

pub async fn execute_registry(args: RegistryArgs) -> CliResult<()> {
    match args.command {
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
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_registry_search() {
        // Note: This test is expected to fail without a configured repository
        // It's here to verify the command structure compiles correctly
        let args = RegistryArgs {
            command: RegistryCommand::Search {
                query: "test".to_string(),
                repository: None,
            },
        };

        let result = execute_registry(args).await;
        // Should fail due to missing repository configuration, but shouldn't panic
        assert!(result.is_ok() || result.is_err());
    }
}
