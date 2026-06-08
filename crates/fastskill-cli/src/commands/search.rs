//! Search command implementation
//!
//! Unified search command that can search both local installed skills and remote
//! skill catalogs with explicit scope control via flags.

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::output;
use fastskill_core::{
    ContentMode, FastSkillService, OutputFormat, ResolveContextRequest, ResolveScope, SearchQuery,
    SearchScope,
};
use serde_json;
use std::collections::HashMap;
use std::path::PathBuf;

/// Search for skills by query string with scope control
#[derive(Debug)]
pub struct SearchArgs {
    /// Search query string
    pub query: String,

    /// Search local/installed skills only
    pub local: bool,

    /// Search remote skill catalogs (default, can be explicit for scripts)
    #[allow(dead_code)]
    pub remote: bool,

    /// Limit remote search to specific repository (remote scope only)
    pub repository: Option<String>,

    /// Maximum number of results (default: 10)
    pub limit: usize,

    /// Output format: table, json, grid, xml (default: table)
    pub format: Option<String>,

    /// Shorthand for --format json
    pub json: bool,

    /// Use embedding search: true, false, or auto
    /// Only applies to local search; ignored for remote search
    pub embedding: Option<String>,

    /// Skills directory path (overrides default discovery)
    #[allow(dead_code)]
    pub skills_dir: Option<std::path::PathBuf>,

    /// Output resolved file paths instead of search results (--local only)
    pub paths: bool,

    /// Content to include in JSON output: none, preview, full (--local --paths only)
    pub content: Option<String>,
}

impl IntoCommandSpec for SearchArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Search skills by query with explicit scope flags",
            syntax: Some("search <QUERY> [--local|--remote] [OPTIONS]"),
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
                    name: "local",
                    long: Some("local"),
                    short: None,
                    help: "Search local/installed skills only",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "remote",
                    long: Some("remote"),
                    short: None,
                    help: "Search remote skill catalogs",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "repository",
                    long: Some("repository"),
                    short: None,
                    help: "Limit remote search to specific repository",
                    kind: ArgKind::Option,
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
                    default: Some(ArgValue::Int(10)),
                    ..Default::default()
                },
                ArgSpec {
                    name: "format",
                    long: Some("format"),
                    short: Some('f'),
                    help: "Output format: table, json, grid, xml",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    long: Some("json"),
                    short: None,
                    help: "Shorthand for --format json",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "embedding",
                    long: Some("embedding"),
                    short: None,
                    help: "Use embedding search: true, false, or auto",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
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
                ArgSpec {
                    name: "paths",
                    long: Some("paths"),
                    short: None,
                    help: "Output resolved file paths instead of search results (--local only)",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "content",
                    long: Some("content"),
                    short: None,
                    help: "Content to include in JSON output: none, preview, full (--local --paths only)",
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

impl FromArgValueMap for SearchArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        SearchArgs {
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
            local: matches!(map.get("local"), Some(ArgValue::Bool(true))),
            remote: matches!(map.get("remote"), Some(ArgValue::Bool(true))),
            repository: map.get("repository").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            limit: map
                .get("limit")
                .and_then(|v| {
                    if let ArgValue::Int(n) = v {
                        Some(*n as usize)
                    } else {
                        None
                    }
                })
                .unwrap_or(10),
            format: map.get("format").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
            embedding: map.get("embedding").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            skills_dir: map.get("skills-dir").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(PathBuf::from(s))
                } else {
                    None
                }
            }),
            paths: matches!(map.get("paths"), Some(ArgValue::Bool(true))),
            content: map.get("content").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        }
    }
}

pub async fn execute_search(service: &FastSkillService, args: SearchArgs) -> CliResult<()> {
    // Validate arguments
    validate_search_args(&args)?;

    // When --paths is set, use context_resolver for canonical path output
    if args.paths {
        return execute_search_with_paths(service, &args).await;
    }

    // Determine search scope
    let scope = determine_search_scope(&args)?;

    // Determine output format
    let format = determine_output_format(&args)?;

    // Warn when local semantic search is requested but no embedding provider is configured
    let embedding_mode = determine_embedding_mode(&args);
    if args.local
        && service.config().embedding.is_none()
        && args.embedding.as_deref() != Some("false")
    {
        eprintln!(
            "Warning: semantic search requires an embedding provider. Falling back to text search."
        );
    }

    // Create search query
    let query = SearchQuery {
        query: args.query.clone(),
        scope,
        limit: args.limit,
        embedding: embedding_mode,
    };

    // Execute search
    let results = fastskill_core::execute(query, service).await?;

    // Format and output results
    if results.is_empty() {
        println!("No skills found matching '{}'", args.query);
        return Ok(());
    }

    let formatted_output = output::format_search_results(&results, format, &args.query)
        .map_err(CliError::Validation)?;

    println!("{}", formatted_output);
    Ok(())
}

/// Execute search with --paths flag: uses context_resolver to include canonical paths
async fn execute_search_with_paths(service: &FastSkillService, args: &SearchArgs) -> CliResult<()> {
    let content_mode = parse_content_mode(args.content.as_deref().unwrap_or("none"))?;

    let request = ResolveContextRequest {
        prompt: args.query.clone(),
        limit: args.limit,
        scope: ResolveScope::Local,
        include_content: content_mode,
        resolve_paths: true,
    };

    let response = service
        .context_resolver()
        .resolve_context(request)
        .await
        .map_err(CliError::Service)?;

    let json_output = serde_json::to_string_pretty(&response)
        .map_err(|e| CliError::Validation(format!("Failed to serialize results to JSON: {}", e)))?;

    println!("{}", json_output);
    Ok(())
}

/// Parse content mode string into ContentMode enum
fn parse_content_mode(s: &str) -> CliResult<ContentMode> {
    match s.to_lowercase().as_str() {
        "none" => Ok(ContentMode::None),
        "preview" => Ok(ContentMode::Preview),
        "full" => Ok(ContentMode::Full),
        other => Err(CliError::Config(format!(
            "Invalid --content value '{}': must be none, preview, or full",
            other
        ))),
    }
}

/// Validate search arguments for conflicting options
fn validate_search_args(args: &SearchArgs) -> CliResult<()> {
    // Validate --repository flag only works with remote search
    if args.repository.is_some() && args.local {
        return Err(CliError::Config(
            "Error: --repository is only valid for remote search. Omit --local or --repository."
                .to_string(),
        ));
    }

    if args.json && args.format.is_some() {
        return Err(CliError::Config(
            "Error: --json and --format cannot be used together. Use one output selector."
                .to_string(),
        ));
    }

    if args.paths && !args.local {
        return Err(CliError::Config(
            "Error: --paths requires --local. Use 'fastskill search --local --paths <query>'."
                .to_string(),
        ));
    }

    if args.content.is_some() && !args.local {
        return Err(CliError::Config(
            "--content is only valid with --local. Use 'fastskill search --local --paths --content <mode> <query>'.".to_string(),
        ));
    }

    // Validate the content mode value eagerly so an invalid value is rejected
    // regardless of whether --paths is also present.
    if let Some(content) = args.content.as_deref() {
        parse_content_mode(content)?;
    }

    Ok(())
}

/// Determine search scope from CLI arguments
fn determine_search_scope(args: &SearchArgs) -> CliResult<SearchScope> {
    if args.local {
        Ok(SearchScope::Local)
    } else if let Some(repo_name) = &args.repository {
        Ok(SearchScope::RemoteRepo(repo_name.clone()))
    } else {
        // Default to remote search (even if --remote is not explicit)
        Ok(SearchScope::Remote)
    }
}

/// Determine output format from CLI arguments
fn determine_output_format(args: &SearchArgs) -> CliResult<OutputFormat> {
    if args.json {
        Ok(OutputFormat::Json)
    } else {
        args.format
            .as_deref()
            .unwrap_or("table")
            .parse::<OutputFormat>()
            .map_err(CliError::Config)
    }
}

/// Determine embedding mode from CLI arguments.
/// Returns:
/// - Some(true): embedding only
/// - Some(false): text only
/// - None: auto-detect/fallback behavior
fn determine_embedding_mode(args: &SearchArgs) -> Option<bool> {
    match args.embedding.as_deref() {
        Some("true") => Some(true),
        Some("false") => Some(false),
        Some("auto") | None => None,
        Some(_) => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use fastskill_core::ServiceConfig;
    use tempfile::TempDir;

    #[test]
    fn test_validate_search_args_local_and_repository_conflict() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: true,
            remote: false,
            repository: Some("my-repo".to_string()),
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: None,
            skills_dir: None,
            paths: false,
            content: None,
        };

        let result = validate_search_args(&args);
        assert!(result.is_err());
        if let Err(CliError::Config(msg)) = result {
            assert!(msg.contains("--repository is only valid for remote search"));
        }
    }

    #[test]
    fn test_validate_search_args_paths_without_local() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: false,
            remote: false,
            repository: None,
            limit: 10,
            format: None,
            json: false,
            embedding: None,
            skills_dir: None,
            paths: true,
            content: None,
        };
        let result = validate_search_args(&args);
        assert!(result.is_err());
        match result {
            Err(CliError::Config(msg)) => {
                assert!(msg.contains("--paths"));
                assert!(msg.contains("--local"));
            }
            other => panic!("expected CliError::Config, got {:?}", other),
        }
    }

    #[test]
    fn test_validate_search_args_content_without_local() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: false,
            remote: false,
            repository: None,
            limit: 10,
            format: None,
            json: false,
            embedding: None,
            skills_dir: None,
            paths: false,
            content: Some("full".to_string()),
        };
        let result = validate_search_args(&args);
        assert!(matches!(result, Err(CliError::Config(_))));
    }

    #[test]
    fn test_validate_search_args_invalid_content_value() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: true,
            remote: false,
            repository: None,
            limit: 10,
            format: None,
            json: false,
            embedding: None,
            skills_dir: None,
            paths: false,
            content: Some("bogus".to_string()),
        };
        let result = validate_search_args(&args);
        match result {
            Err(CliError::Config(msg)) => assert!(msg.contains("must be none, preview, or full")),
            other => panic!("expected CliError::Config, got {:?}", other),
        }
    }

    #[test]
    fn test_validate_search_args_local_paths_content_full_ok() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: true,
            remote: false,
            repository: None,
            limit: 10,
            format: None,
            json: true,
            embedding: None,
            skills_dir: None,
            paths: true,
            content: Some("full".to_string()),
        };
        assert!(validate_search_args(&args).is_ok());
    }

    #[test]
    fn test_validate_search_args_valid_local() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: true,
            remote: false,
            repository: None,
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: Some("false".to_string()),
            skills_dir: None,
            paths: false,
            content: None,
        };

        let result = validate_search_args(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_search_args_valid_remote_with_repository() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: false,
            remote: true,
            repository: Some("my-repo".to_string()),
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: None,
            skills_dir: None,
            paths: false,
            content: None,
        };

        let result = validate_search_args(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_determine_search_scope_local() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: true,
            remote: false,
            repository: None,
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: None,
            skills_dir: None,
            paths: false,
            content: None,
        };

        let scope = determine_search_scope(&args).unwrap();
        assert_eq!(scope, SearchScope::Local);
    }

    #[test]
    fn test_determine_search_scope_remote_with_repository() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: false,
            remote: false,
            repository: Some("my-repo".to_string()),
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: None,
            skills_dir: None,
            paths: false,
            content: None,
        };

        let scope = determine_search_scope(&args).unwrap();
        assert_eq!(scope, SearchScope::RemoteRepo("my-repo".to_string()));
    }

    #[test]
    fn test_determine_search_scope_remote_default() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: false,
            remote: false,
            repository: None,
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: None,
            skills_dir: None,
            paths: false,
            content: None,
        };

        let scope = determine_search_scope(&args).unwrap();
        assert_eq!(scope, SearchScope::Remote);
    }

    #[test]
    fn test_determine_output_format_json_flag() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: false,
            remote: false,
            repository: None,
            limit: 10,
            format: Some("table".to_string()),
            json: true,
            embedding: None,
            skills_dir: None,
            paths: false,
            content: None,
        };

        let format = determine_output_format(&args).unwrap();
        assert_eq!(format, OutputFormat::Json);
    }

    #[test]
    fn test_determine_output_format_from_string() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: false,
            remote: false,
            repository: None,
            limit: 10,
            format: Some("xml".to_string()),
            json: false,
            embedding: None,
            skills_dir: None,
            paths: false,
            content: None,
        };

        let format = determine_output_format(&args).unwrap();
        assert_eq!(format, OutputFormat::Xml);
    }

    #[tokio::test]
    async fn test_execute_search_local_with_empty_results() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            embedding: None,
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = SearchArgs {
            query: "nonexistent".to_string(),
            local: true,
            remote: false,
            repository: None,
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: Some("false".to_string()), // Force text search
            skills_dir: None,
            paths: false,
            content: None,
        };

        let result = execute_search(&service, args).await;
        // Should succeed with no results message
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_search_args_json_and_format_conflict() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: false,
            remote: true,
            repository: None,
            limit: 10,
            format: Some("table".to_string()),
            json: true,
            embedding: None,
            skills_dir: None,
            paths: false,
            content: None,
        };

        let result = validate_search_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_determine_embedding_mode_auto() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: true,
            remote: false,
            repository: None,
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: Some("auto".to_string()),
            skills_dir: None,
            paths: false,
            content: None,
        };

        let mode = determine_embedding_mode(&args);
        assert_eq!(mode, None);
    }
}
