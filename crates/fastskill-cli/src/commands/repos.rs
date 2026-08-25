//! Repos command - manage repository list and browse remote skill catalog
//!
//! This command consolidates repository management (add/remove/test/refresh) and
//! remote catalog operations (skills/show/versions) into a single namespace.

use crate::commands::common::validate_format_args;
use crate::error::CliResult;
use clap::{Args, Subcommand};
use fastskill_core::OutputFormat;
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

mod args;

pub use args::{
    ReposAddArgs, ReposInfoArgs, ReposListArgs, ReposRefreshArgs, ReposRemoveArgs, ReposShowArgs,
    ReposSkillsArgs, ReposTestArgs, ReposUpdateArgs, ReposVersionsArgs,
};

// ---------------------------------------------------------------------------
// Dispatch helpers for typed repos subcommands
// ---------------------------------------------------------------------------

pub async fn execute_repos_list(args: ReposListArgs) -> CliResult<()> {
    let resolved_format = validate_format_args(&args.format, args.json)?;
    super::registry::repo_ops::execute_list_with_format(resolved_format).await
}

pub async fn execute_repos_add(args: ReposAddArgs) -> CliResult<()> {
    super::registry::repo_ops::execute_add(
        args.name,
        args.repo_type,
        args.url_or_path,
        args.priority,
        args.branch,
        args.tag,
        args.auth_type,
        args.auth_env,
        args.auth_key_path,
        args.auth_username,
    )
    .await
}

pub async fn execute_repos_remove(args: ReposRemoveArgs) -> CliResult<()> {
    super::registry::repo_ops::execute_remove(args.name).await
}

pub async fn execute_repos_info(args: ReposInfoArgs) -> CliResult<()> {
    let resolved_format = validate_format_args(&args.format, args.json)?;
    super::registry::repo_ops::execute_show_with_format(args.name, resolved_format).await
}

pub async fn execute_repos_update(args: ReposUpdateArgs) -> CliResult<()> {
    super::registry::repo_ops::execute_update(args.name, args.branch, args.priority).await
}

pub async fn execute_repos_test(args: ReposTestArgs) -> CliResult<()> {
    super::registry::repo_ops::execute_test(args.name).await
}

pub async fn execute_repos_refresh(args: ReposRefreshArgs) -> CliResult<()> {
    super::registry::repo_ops::execute_refresh(args.name).await
}

pub async fn execute_repos_skills(args: ReposSkillsArgs) -> CliResult<()> {
    super::registry::skill_ops::execute_list_skills(
        args.repository,
        args.scope,
        args.all_versions,
        args.include_pre_release,
        args.format,
        args.json,
    )
    .await
}

pub async fn execute_repos_show(args: ReposShowArgs) -> CliResult<()> {
    super::registry::skill_ops::execute_show_skill(args.skill_id, args.repository).await
}

pub async fn execute_repos_versions(args: ReposVersionsArgs) -> CliResult<()> {
    super::registry::skill_ops::execute_versions(args.skill_id, args.repository).await
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::await_holding_lock)]
#[cfg(test)]
mod tests;
