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
        /// Repository name (required)
        #[arg(long)]
        name: Option<String>,
        /// Owner name (required by Claude Code)
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
        /// Verify the existing marketplace.json matches the skills on disk, writing nothing
        #[arg(long)]
        check: bool,
    },
}

/// Flat args struct for the `marketplace create` subcommand
#[derive(Debug)]
pub struct MarketplaceCreateArgs {
    /// Directory containing skills to scan
    pub path: PathBuf,
    /// Output file path
    pub output: Option<PathBuf>,
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
    /// Verify the existing catalog instead of writing one
    pub check: bool,
}

impl IntoCommandSpec for MarketplaceCreateArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Create skill marketplace artifacts",
            syntax: Some("marketplace create <PATH> [OPTIONS]"),
            category: Some("publishing"),
            examples: vec![
                "fastskill marketplace create . --name my-skills",
                "fastskill marketplace create ./skills --name my-skills --owner-name \"Team\" --output .claude-plugin/marketplace.json",
                "fastskill marketplace create . --name my-skills --owner-name \"Team\" --check",
            ],
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
                    help: "Owner name (required)",
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
                    name: "repo-version",
                    kind: ArgKind::Option,
                    long: Some("repo-version"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Repository version (optional)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "check",
                    kind: ArgKind::Flag,
                    long: Some("check"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Verify the existing marketplace.json matches the skills on disk, writing nothing",
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
            version: map.get("repo-version").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            check: matches!(map.get("check"), Some(ArgValue::Bool(true))),
        }
    }
}

pub async fn execute_marketplace_create(args: MarketplaceCreateArgs) -> CliResult<()> {
    super::repos::marketplace::execute_create(
        args.path,
        args.output,
        args.name,
        args.owner_name,
        args.owner_email,
        args.description,
        args.version,
        args.check,
    )
    .await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_skill(root: &std::path::Path, rel: &str, id: &str, version: &str) {
        let dir = root.join(rel);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {id}\ndescription: d\nversion: {version}\n---\n\n# {id}\n"),
        )
        .unwrap();
        fs::write(
            dir.join("skill-project.toml"),
            format!("[metadata]\nid = \"{id}\"\nversion = \"{version}\"\n"),
        )
        .unwrap();
    }

    fn args(path: &std::path::Path) -> MarketplaceCreateArgs {
        MarketplaceCreateArgs {
            path: path.to_path_buf(),
            output: None,
            name: Some("test-marketplace".to_string()),
            owner_name: Some("Test Owner".to_string()),
            owner_email: None,
            description: None,
            version: None,
            check: false,
        }
    }

    /// The path a catalog entry carries is the skill's own folder. This is the
    /// regression the command existed to get wrong: it used to write `./{id}`,
    /// which names a real folder only when a skill sits at the root under its
    /// own id.
    #[tokio::test]
    async fn create_writes_each_skill_real_path() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write_skill(root, "workspace/cli-rust-dev", "cli-rust-dev", "1.0.0");
        write_skill(root, "skills/nested/deep", "deep-skill", "2.0.0");

        execute_marketplace_create(args(root)).await.unwrap();

        let written =
            fs::read_to_string(root.join(".claude-plugin").join("marketplace.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();
        let skills = parsed["plugins"][0]["skills"].as_array().unwrap();
        assert_eq!(
            skills
                .iter()
                .map(|s| s.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["./skills/nested/deep", "./workspace/cli-rust-dev"],
        );
    }

    /// Claude Code rejects a catalog without an owner, so the command refuses to
    /// write one rather than producing a file that fails `claude plugin validate`.
    #[tokio::test]
    async fn create_requires_an_owner_name() {
        let temp = TempDir::new().unwrap();
        write_skill(temp.path(), "a", "a", "1.0.0");

        let mut args = args(temp.path());
        args.owner_name = None;
        let err = execute_marketplace_create(args).await.unwrap_err();

        assert!(err.to_string().contains("Owner name is required"), "{err}");
        assert!(!temp.path().join(".claude-plugin").exists());
    }

    /// `--check` is the CI gate: it must report a stale catalog as a failure and
    /// leave the file untouched.
    #[tokio::test]
    async fn check_fails_on_a_stale_catalog_without_writing() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write_skill(root, "a", "a", "1.0.0");
        execute_marketplace_create(args(root)).await.unwrap();

        write_skill(root, "b", "b", "1.0.0");
        let mut stale = args(root);
        stale.check = true;
        let err = execute_marketplace_create(stale).await.unwrap_err();
        assert!(err.to_string().contains("out of date"), "{err}");

        let written =
            fs::read_to_string(root.join(".claude-plugin").join("marketplace.json")).unwrap();
        assert!(!written.contains("./b"));

        execute_marketplace_create(args(root)).await.unwrap();
        let mut fresh = args(root);
        fresh.check = true;
        execute_marketplace_create(fresh).await.unwrap();
    }

    #[test]
    fn check_flag_is_parsed_and_base_url_is_gone() {
        let spec = MarketplaceCreateArgs::command_spec();
        assert!(spec.args.iter().any(|a| a.name == "check"));
        assert!(!spec.args.iter().any(|a| a.name == "base-url"));

        let mut map = HashMap::new();
        map.insert("check".to_string(), ArgValue::Bool(true));
        assert!(MarketplaceCreateArgs::from_arg_value_map(&map).check);
        assert!(!MarketplaceCreateArgs::from_arg_value_map(&HashMap::new()).check);
    }
}
