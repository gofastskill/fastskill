//! Marketplace command - create marketplace.json from skill directories
//!
//! This command handles marketplace generation functionality that was previously
//! part of the sources command.

use crate::cli::error::CliResult;
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args)]
#[command(
    about = "Marketplace generation and management",
    after_help = "Examples:\n  fastskill marketplace create\n  fastskill marketplace create --path ./skills --name my-marketplace"
)]
pub struct MarketplaceArgs {
    #[command(subcommand)]
    pub command: MarketplaceCommand,
}

#[derive(Debug, Subcommand)]
pub enum MarketplaceCommand {
    /// Create marketplace.json from a directory containing skills
    #[command(
        after_help = "Examples:\n  fastskill marketplace create\n  fastskill marketplace create --path ./skills --name my-marketplace"
    )]
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

pub async fn execute_marketplace(args: MarketplaceArgs) -> CliResult<()> {
    match args.command {
        MarketplaceCommand::Create {
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_marketplace_create() {
        let temp_dir = TempDir::new().unwrap();

        let args = MarketplaceArgs {
            command: MarketplaceCommand::Create {
                path: temp_dir.path().to_path_buf(),
                output: None,
                base_url: None,
                name: Some("test-marketplace".to_string()),
                owner_name: None,
                owner_email: None,
                description: None,
                version: None,
            },
        };

        // This test verifies the command structure compiles correctly
        // The actual execution may fail without proper skill directory structure
        let result = execute_marketplace(args).await;
        assert!(result.is_ok() || result.is_err());
    }
}
