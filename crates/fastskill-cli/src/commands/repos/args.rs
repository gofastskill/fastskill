use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::OutputFormat;
use std::collections::HashMap;
use std::path::PathBuf;

pub(super) fn parse_output_format(s: &str) -> Option<OutputFormat> {
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
            examples: vec!["fastskill repos list", "fastskill repos list --json"],
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
            examples: vec![
                "fastskill repos add my-repo https://github.com/org/skills.git --repo-type git-marketplace",
                "fastskill repos add local-skills ./skills --repo-type local",
            ],
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
            examples: vec!["fastskill repos remove my-repo"],
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
            examples: vec!["fastskill repos info my-repo"],
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
            examples: vec!["fastskill repos update my-repo --branch main --priority 1"],
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
            examples: vec!["fastskill repos test my-repo"],
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
            examples: vec!["fastskill repos refresh", "fastskill repos refresh my-repo"],
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
            syntax: Some("repos skills [REPOSITORY] [OPTIONS]"),
            category: Some("repositories"),
            examples: vec![
                "fastskill repos skills",
                "fastskill repos skills my-repo --all-versions",
                "fastskill repos skills --repository my-repo --all-versions",
            ],
            args: vec![
                ArgSpec {
                    name: "repo",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    conflicts_with: vec!["repository"],
                    help: "Repository name to list skills from (positional shorthand for --repository)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "repository",
                    kind: ArgKind::Option,
                    long: Some("repository"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    // Not declared on this side too: `validate_typed_args` walks every
                    // arg's `conflicts_with` independently, so a two-way declaration
                    // would emit the E005 diagnostic twice (once per direction) for a
                    // single conflict. The positional `repo` arg above owns the check.
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
            // `--repository <name>` and the positional `<REPOSITORY>` shorthand are
            // mutually exclusive (see `conflicts_with` above), so at most one is ever
            // present; either satisfies the `repository` field.
            repository: map
                .get("repository")
                .or_else(|| map.get("repo"))
                .and_then(|v| {
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
            examples: vec![
                "fastskill repos show pptx",
                "fastskill repos show pptx --repository my-repo",
            ],
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
            examples: vec!["fastskill repos versions pptx"],
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
