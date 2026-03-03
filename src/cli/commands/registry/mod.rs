//! Registry command implementation - browse the registry catalog
//!
//! This command is deprecated. Use `repos` for catalog browsing and `search` for searching.

pub mod formatters;
pub mod helpers;
pub mod marketplace;
pub mod repo_ops;
pub mod skill_ops;

use crate::cli::commands::common::emit_deprecation_warning;
use crate::cli::error::CliResult;
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(
    after_help = "Examples:\n  fastskill registry list-skills\n  fastskill search \"query\" (use the unified search command)"
)]
pub struct RegistryArgs {
    #[command(subcommand)]
    pub command: RegistryCommand,
}

#[derive(Debug, Subcommand)]
pub enum RegistryCommand {
    /// List skills in registry catalog
    #[command(
        after_help = "Examples:\n  fastskill registry list-skills\n  fastskill registry list-skills --json"
    )]
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
    #[command(after_help = "Examples:\n  fastskill registry show-skill pptx")]
    ShowSkill {
        /// Skill ID
        skill_id: String,
        /// Repository name (defaults to default repository if not specified)
        #[arg(long)]
        repository: Option<String>,
    },

    /// List available versions for a skill
    #[command(after_help = "Examples:\n  fastskill registry versions pptx")]
    Versions {
        /// Skill ID
        skill_id: String,
        /// Repository name (defaults to default repository if not specified)
        #[arg(long)]
        repository: Option<String>,
    },
}

pub async fn execute_registry(args: RegistryArgs) -> CliResult<()> {
    // Print standardized deprecation warning
    let mappings = vec![
        ("registry list-skills", "repos skills"),
        ("registry show-skill", "repos show"),
        ("registry versions", "repos versions"),
    ];
    emit_deprecation_warning("registry", "repos", "catalog browsing", &mappings);

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
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
}
