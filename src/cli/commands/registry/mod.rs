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
    // Print deprecation warning
    eprintln!("⚠️  Warning: The 'registry' command is deprecated and will be removed in a future version.");
    eprintln!("   Please use 'fastskill repos' for catalog browsing and 'fastskill search' for searching:");
    eprintln!("   - 'registry list-skills' → 'repos skills'");
    eprintln!("   - 'registry show-skill' → 'repos show'");
    eprintln!("   - 'registry versions' → 'repos versions'");
    eprintln!("   - 'registry search' → 'search' (remote search is the default)");
    eprintln!();

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
