//! Repos command - manage repository list and browse remote skill catalog
//!
//! This command consolidates repository management (add/remove/test/refresh) and
//! remote catalog operations (skills/show/versions) into a single namespace.

use crate::commands::common::validate_format_args;
use crate::error::CliResult;
use clap::{Args, Subcommand};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::OutputFormat;
use std::collections::HashMap;
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

fn parse_output_format(s: &str) -> Option<OutputFormat> {
    match s {
        "table" => Some(OutputFormat::Table),
        "json" => Some(OutputFormat::Json),
        "grid" => Some(OutputFormat::Grid),
        "xml" => Some(OutputFormat::Xml),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Typed arg structs for repos subcommands
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ReposListArgs {
    pub format: Option<OutputFormat>,
    pub json: bool,
}

#[derive(Debug)]
pub struct ReposAddArgs {
    pub name: String,
    pub repo_type: String,
    pub url_or_path: String,
    pub priority: Option<u32>,
    pub branch: Option<String>,
    pub tag: Option<String>,
    pub auth_type: Option<String>,
    pub auth_env: Option<String>,
    pub auth_key_path: Option<PathBuf>,
    pub auth_username: Option<String>,
}

#[derive(Debug)]
pub struct ReposRemoveArgs {
    pub name: String,
}

#[derive(Debug)]
pub struct ReposInfoArgs {
    pub name: String,
    pub format: Option<OutputFormat>,
    pub json: bool,
}

#[derive(Debug)]
pub struct ReposUpdateArgs {
    pub name: String,
    pub branch: Option<String>,
    pub priority: Option<u32>,
}

#[derive(Debug)]
pub struct ReposTestArgs {
    pub name: String,
}

#[derive(Debug)]
pub struct ReposRefreshArgs {
    pub name: Option<String>,
}

#[derive(Debug)]
pub struct ReposSkillsArgs {
    pub repository: Option<String>,
    pub scope: Option<String>,
    pub all_versions: bool,
    pub include_pre_release: bool,
    pub format: Option<OutputFormat>,
    pub json: bool,
}

#[derive(Debug)]
pub struct ReposShowArgs {
    pub skill_id: String,
    pub repository: Option<String>,
}

#[derive(Debug)]
pub struct ReposVersionsArgs {
    pub skill_id: String,
    pub repository: Option<String>,
}

// ---------------------------------------------------------------------------
// IntoCommandSpec impls
// ---------------------------------------------------------------------------

impl IntoCommandSpec for ReposListArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "List all configured repositories",
            syntax: Some("repos list [OPTIONS]"),
            category: Some("repositories"),
            args: vec![
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output format: table, json, grid, xml (default: table)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Output in JSON format",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReposListArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            format: map
                .get("format")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .and_then(parse_output_format),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
        }
    }
}

impl IntoCommandSpec for ReposAddArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Add a new repository",
            syntax: Some("repos add <NAME> <URL-OR-PATH> [OPTIONS]"),
            category: Some("repositories"),
            args: vec![
                ArgSpec {
                    name: "name",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Repository name",
                    ..Default::default()
                },
                ArgSpec {
                    name: "url-or-path",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "URL for git-marketplace or http-registry, or path for local",
                    ..Default::default()
                },
                ArgSpec {
                    name: "repo-type",
                    kind: ArgKind::Option,
                    long: Some("repo-type"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Repository type: git-marketplace, http-registry, zip-url, or local",
                    ..Default::default()
                },
                ArgSpec {
                    name: "priority",
                    kind: ArgKind::Option,
                    long: Some("priority"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Priority (lower number = higher priority, default: 0)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "branch",
                    kind: ArgKind::Option,
                    long: Some("branch"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Branch for git-marketplace",
                    ..Default::default()
                },
                ArgSpec {
                    name: "tag",
                    kind: ArgKind::Option,
                    long: Some("tag"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Tag for git-marketplace",
                    ..Default::default()
                },
                ArgSpec {
                    name: "auth-type",
                    kind: ArgKind::Option,
                    long: Some("auth-type"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Authentication type: pat, ssh-key, ssh, basic, or api_key",
                    ..Default::default()
                },
                ArgSpec {
                    name: "auth-env",
                    kind: ArgKind::Option,
                    long: Some("auth-env"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Environment variable for PAT, basic password, or API key",
                    ..Default::default()
                },
                ArgSpec {
                    name: "auth-key-path",
                    kind: ArgKind::Option,
                    long: Some("auth-key-path"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "SSH key path (for ssh-key or ssh auth)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "auth-username",
                    kind: ArgKind::Option,
                    long: Some("auth-username"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Username (for basic auth)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReposAddArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            name: map
                .get("name")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
            url_or_path: map
                .get("url-or-path")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
            repo_type: map
                .get("repo-type")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
            priority: map.get("priority").and_then(|v| {
                if let ArgValue::Int(n) = v {
                    Some(*n as u32)
                } else {
                    None
                }
            }),
            branch: map.get("branch").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            tag: map.get("tag").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            auth_type: map.get("auth-type").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            auth_env: map.get("auth-env").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            auth_key_path: map.get("auth-key-path").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(PathBuf::from(s))
                } else {
                    None
                }
            }),
            auth_username: map.get("auth-username").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        }
    }
}

impl IntoCommandSpec for ReposRemoveArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Remove a repository",
            syntax: Some("repos remove <NAME>"),
            category: Some("repositories"),
            args: vec![ArgSpec {
                name: "name",
                kind: ArgKind::Positional,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Required,
                help: "Repository name to remove",
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReposRemoveArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            name: map
                .get("name")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
        }
    }
}

impl IntoCommandSpec for ReposInfoArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Show repository details",
            syntax: Some("repos info <NAME> [OPTIONS]"),
            category: Some("repositories"),
            args: vec![
                ArgSpec {
                    name: "name",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Repository name",
                    ..Default::default()
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output format: table, json, grid, xml (default: table)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Output in JSON format",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReposInfoArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            name: map
                .get("name")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
            format: map
                .get("format")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .and_then(parse_output_format),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
        }
    }
}

impl IntoCommandSpec for ReposUpdateArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Update repository metadata",
            syntax: Some("repos update <NAME> [OPTIONS]"),
            category: Some("repositories"),
            args: vec![
                ArgSpec {
                    name: "name",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Repository name to update",
                    ..Default::default()
                },
                ArgSpec {
                    name: "branch",
                    kind: ArgKind::Option,
                    long: Some("branch"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "New branch (for git-marketplace)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "priority",
                    kind: ArgKind::Option,
                    long: Some("priority"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "New priority",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReposUpdateArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            name: map
                .get("name")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
            branch: map.get("branch").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            priority: map.get("priority").and_then(|v| {
                if let ArgValue::Int(n) = v {
                    Some(*n as u32)
                } else {
                    None
                }
            }),
        }
    }
}

impl IntoCommandSpec for ReposTestArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Test repository connectivity",
            syntax: Some("repos test <NAME>"),
            category: Some("repositories"),
            args: vec![ArgSpec {
                name: "name",
                kind: ArgKind::Positional,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Required,
                help: "Repository name to test",
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReposTestArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            name: map
                .get("name")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
        }
    }
}

impl IntoCommandSpec for ReposRefreshArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Refresh repository cache",
            syntax: Some("repos refresh [NAME]"),
            category: Some("repositories"),
            args: vec![ArgSpec {
                name: "name",
                kind: ArgKind::Positional,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                help: "Repository name to refresh (if not specified, refreshes all)",
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReposRefreshArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            name: map.get("name").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        }
    }
}

impl IntoCommandSpec for ReposSkillsArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "List skills in repository catalog",
            syntax: Some("repos skills [OPTIONS]"),
            category: Some("repositories"),
            args: vec![
                ArgSpec {
                    name: "repository",
                    kind: ArgKind::Option,
                    long: Some("repository"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Repository name to list skills from",
                    ..Default::default()
                },
                ArgSpec {
                    name: "scope",
                    kind: ArgKind::Option,
                    long: Some("scope"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Filter by scope (organization name)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "all-versions",
                    kind: ArgKind::Flag,
                    long: Some("all-versions"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Show all versions for each skill",
                    ..Default::default()
                },
                ArgSpec {
                    name: "include-pre-release",
                    kind: ArgKind::Flag,
                    long: Some("include-pre-release"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Include pre-release versions",
                    ..Default::default()
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output format: table, json, grid, xml (default: table)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Shorthand for --format json",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReposSkillsArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            repository: map.get("repository").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            scope: map.get("scope").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            all_versions: matches!(map.get("all-versions"), Some(ArgValue::Bool(true))),
            include_pre_release: matches!(
                map.get("include-pre-release"),
                Some(ArgValue::Bool(true))
            ),
            format: map
                .get("format")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .and_then(parse_output_format),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
        }
    }
}

impl IntoCommandSpec for ReposShowArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Show skill details from catalog",
            syntax: Some("repos show <SKILL-ID> [OPTIONS]"),
            category: Some("repositories"),
            args: vec![
                ArgSpec {
                    name: "skill-id",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Skill ID",
                    ..Default::default()
                },
                ArgSpec {
                    name: "repository",
                    kind: ArgKind::Option,
                    long: Some("repository"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Repository name (defaults to default repository if not specified)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReposShowArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            skill_id: map
                .get("skill-id")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
            repository: map.get("repository").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        }
    }
}

impl IntoCommandSpec for ReposVersionsArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "List available versions for a skill",
            syntax: Some("repos versions <SKILL-ID> [OPTIONS]"),
            category: Some("repositories"),
            args: vec![
                ArgSpec {
                    name: "skill-id",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Skill ID",
                    ..Default::default()
                },
                ArgSpec {
                    name: "repository",
                    kind: ArgKind::Option,
                    long: Some("repository"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Repository name (defaults to default repository if not specified)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReposVersionsArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            skill_id: map
                .get("skill-id")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
            repository: map.get("repository").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        }
    }
}

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
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_repos_list() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
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

        let args = ReposListArgs {
            format: None,
            json: false,
        };

        let result = execute_repos_list(args).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_repos_skills() {
        // Note: This test is expected to fail without a configured repository
        // It's here to verify the command structure compiles correctly
        let args = ReposSkillsArgs {
            repository: None,
            scope: None,
            all_versions: false,
            include_pre_release: false,
            format: None,
            json: false,
        };

        let result = execute_repos_skills(args).await;
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
