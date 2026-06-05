//! Resolve command implementation
//!
//! Machine-first skill resolution that returns canonical paths, metadata, and optional content.

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::context_resolver::{ContentMode, ResolveContextRequest, ResolveScope};
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug)]
pub struct ResolveArgs {
    /// Search query string
    pub query: String,

    /// Maximum number of results (default: 5)
    pub limit: usize,

    /// Content to include: none, preview (frontmatter + 20 lines), full
    pub include_content: String,

    /// Resolve and validate canonical absolute paths (default: true)
    pub resolve_paths: bool,

    /// Skills directory path (overrides default discovery)
    #[allow(dead_code)]
    pub skills_dir: Option<PathBuf>,
}

impl IntoCommandSpec for ResolveArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Resolve skills as machine-readable JSON with canonical paths",
            syntax: Some("resolve <QUERY> [OPTIONS]"),
            category: Some("discovery"),
            args: vec![
                ArgSpec {
                    name: "query",
                    long: None,
                    short: None,
                    help: "Search query string",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "limit",
                    long: Some("limit"),
                    short: Some('l'),
                    help: "Maximum number of results",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Int(5)),
                    ..Default::default()
                },
                ArgSpec {
                    name: "include-content",
                    long: Some("include-content"),
                    short: None,
                    help: "Content to include: none, preview, full",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str("none".to_string())),
                    ..Default::default()
                },
                ArgSpec {
                    name: "resolve-paths",
                    long: Some("resolve-paths"),
                    short: None,
                    help: "Resolve and validate canonical absolute paths",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Bool(true)),
                    ..Default::default()
                },
                ArgSpec {
                    name: "skills-dir",
                    long: Some("skills-dir"),
                    short: None,
                    help: "Skills directory path (overrides default discovery)",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ResolveArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        ResolveArgs {
            query: map
                .get("query")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("fw bug: missing query")),
            limit: map
                .get("limit")
                .and_then(|v| {
                    if let ArgValue::Int(n) = v {
                        Some(*n as usize)
                    } else {
                        None
                    }
                })
                .unwrap_or(5),
            include_content: map
                .get("include-content")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "none".to_string()),
            resolve_paths: map
                .get("resolve-paths")
                .and_then(|v| {
                    if let ArgValue::Bool(b) = v {
                        Some(*b)
                    } else {
                        None
                    }
                })
                .unwrap_or(true),
            skills_dir: map.get("skills-dir").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(PathBuf::from(s))
                } else {
                    None
                }
            }),
        }
    }
}

pub async fn execute_resolve(service: &FastSkillService, args: ResolveArgs) -> CliResult<()> {
    let include_content = parse_content_mode(&args.include_content)?;

    let request = ResolveContextRequest {
        prompt: args.query,
        limit: args.limit,
        scope: ResolveScope::Local,
        include_content,
        resolve_paths: args.resolve_paths,
    };

    let resolver = service.context_resolver();
    let response = resolver
        .resolve_context(request)
        .await
        .map_err(CliError::Service)?;

    let json = serde_json::to_string_pretty(&response)
        .map_err(|e| CliError::Config(format!("Failed to serialize response: {}", e)))?;

    println!("{}", json);
    Ok(())
}

fn parse_content_mode(s: &str) -> CliResult<ContentMode> {
    match s {
        "none" => Ok(ContentMode::None),
        "preview" => Ok(ContentMode::Preview),
        "full" => Ok(ContentMode::Full),
        other => Err(CliError::Config(format!(
            "Invalid --include-content value '{}': must be none, preview, or full",
            other
        ))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_content_mode_none() {
        assert_eq!(parse_content_mode("none").unwrap(), ContentMode::None);
    }

    #[test]
    fn test_parse_content_mode_preview() {
        assert_eq!(parse_content_mode("preview").unwrap(), ContentMode::Preview);
    }

    #[test]
    fn test_parse_content_mode_full() {
        assert_eq!(parse_content_mode("full").unwrap(), ContentMode::Full);
    }

    #[test]
    fn test_parse_content_mode_invalid() {
        assert!(parse_content_mode("bad").is_err());
    }

    #[tokio::test]
    async fn test_execute_resolve_returns_valid_json() {
        use fastskill_core::ServiceConfig;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            embedding: None,
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ResolveArgs {
            query: "test query".to_string(),
            limit: 5,
            include_content: "none".to_string(),
            resolve_paths: true,
            skills_dir: None,
        };

        let result = execute_resolve(&service, args).await;
        assert!(result.is_ok());
    }
}
