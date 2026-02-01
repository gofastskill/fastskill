//! Sources command - manage skill sources (repositories)
//!
//! For browsing the registry catalog, use `registry` command.

use crate::cli::error::CliResult;
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct SourcesArgs {
    #[command(subcommand)]
    pub command: SourcesCommand,
}

#[derive(Debug, Subcommand)]
pub enum SourcesCommand {
    /// List all configured repositories
    List {
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

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
        /// Output in JSON format
        #[arg(long)]
        json: bool,
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

pub async fn execute_sources(args: SourcesArgs) -> CliResult<()> {
    match args.command {
        SourcesCommand::List { json } => {
            // Dispatch to existing registry::repo_ops::execute_list
            super::registry::repo_ops::execute_list_with_json(json).await
        }
        SourcesCommand::Add {
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
        SourcesCommand::Remove { name } => super::registry::repo_ops::execute_remove(name).await,
        SourcesCommand::Show { name, json } => {
            super::registry::repo_ops::execute_show_with_json(name, json).await
        }
        SourcesCommand::Update {
            name,
            branch,
            priority,
        } => super::registry::repo_ops::execute_update(name, branch, priority).await,
        SourcesCommand::Test { name } => super::registry::repo_ops::execute_test(name).await,
        SourcesCommand::Refresh { name } => super::registry::repo_ops::execute_refresh(name).await,
        SourcesCommand::Create {
            path,
            output,
            base_url,
            name,
            owner_name,
            owner_email,
            description,
            version,
        } => {
            super::registry::marketplace::execute_create(
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
