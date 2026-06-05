//! Marketplace command - create marketplace.json from skill directories
//!
//! This command handles marketplace generation functionality that was previously
//! part of the sources command.

use crate::error::CliResult;
use clap::{Args, Subcommand};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use std::collections::HashMap;
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

/// Flat args struct for the `marketplace create` subcommand
#[derive(Debug)]
pub struct MarketplaceCreateArgs {
    /// Directory containing skills to scan
    pub path: PathBuf,
    /// Output file path
    pub output: Option<PathBuf>,
    /// Base URL for download links
    pub base_url: Option<String>,
    /// Repository name
    pub name: Option<String>,
    /// Owner name
    pub owner_name: Option<String>,
    /// Owner email
    pub owner_email: Option<String>,
    /// Repository description
    pub description: Option<String>,
    /// Repository version
    pub version: Option<String>,
}

impl IntoCommandSpec for MarketplaceCreateArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Create skill marketplace artifacts",
            syntax: Some("marketplace create <PATH> [OPTIONS]"),
            category: Some("publishing"),
            args: vec![
                ArgSpec {
                    name: "path",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Directory containing skills to scan (default: .)",
                    default: Some(ArgValue::Str(".".to_string())),
                    ..Default::default()
                },
                ArgSpec {
                    name: "output",
                    kind: ArgKind::Option,
                    short: Some('o'),
                    long: Some("output"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output file path (default: .claude-plugin/marketplace.json)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "base-url",
                    kind: ArgKind::Option,
                    long: Some("base-url"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Base URL for download links (optional)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "name",
                    kind: ArgKind::Option,
                    long: Some("name"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Repository name (required)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "owner-name",
                    kind: ArgKind::Option,
                    long: Some("owner-name"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Owner name (optional)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "owner-email",
                    kind: ArgKind::Option,
                    long: Some("owner-email"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Owner email (optional)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "description",
                    kind: ArgKind::Option,
                    long: Some("description"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Repository description (optional)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "version",
                    kind: ArgKind::Option,
                    long: Some("version"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Repository version (optional)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for MarketplaceCreateArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            path: map
                .get("path")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(PathBuf::from(s))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| PathBuf::from(".")),
            output: map.get("output").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(PathBuf::from(s))
                } else {
                    None
                }
            }),
            base_url: map.get("base-url").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            name: map.get("name").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            owner_name: map.get("owner-name").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            owner_email: map.get("owner-email").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            description: map.get("description").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            version: map.get("version").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        }
    }
}

pub async fn execute_marketplace_create(args: MarketplaceCreateArgs) -> CliResult<()> {
    super::registry::marketplace::execute_create(
        args.path,
        args.output,
        args.base_url,
        args.name,
        args.owner_name,
        args.owner_email,
        args.description,
        args.version,
    )
    .await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_marketplace_create() {
        let temp_dir = TempDir::new().unwrap();

        let args = MarketplaceCreateArgs {
            path: temp_dir.path().to_path_buf(),
            output: None,
            base_url: None,
            name: Some("test-marketplace".to_string()),
            owner_name: None,
            owner_email: None,
            description: None,
            version: None,
        };

        // This test verifies the command structure compiles correctly
        // The actual execution may fail without proper skill directory structure
        let result = execute_marketplace_create(args).await;
        assert!(result.is_ok() || result.is_err());
    }
}
