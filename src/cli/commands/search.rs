//! Search command implementation
//!
//! Unified search command that can search both local installed skills and remote
//! skill catalogs with explicit scope control via flags.

use crate::cli::error::{CliError, CliResult};
use crate::cli::output::{self, OutputFormat};
use crate::cli::search::{self, SearchQuery, SearchScope};
use clap::Args;
use fastskill::FastSkillService;

/// Search for skills by query string with scope control
#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search query string
    pub query: String,

    /// Search local/installed skills only
    #[arg(long, conflicts_with = "remote")]
    pub local: bool,

    /// Search remote skill catalogs (default, can be explicit for scripts)
    #[arg(long, conflicts_with = "local")]
    pub remote: bool,

    /// Limit remote search to specific repository (remote scope only)
    #[arg(long)]
    pub repo: Option<String>,

    /// Maximum number of results (default: 10)
    #[arg(short, long, default_value = "10")]
    pub limit: usize,

    /// Output format: table, json, grid, xml (default: table)
    #[arg(short, long)]
    pub format: Option<String>,

    /// Shorthand for --format json
    #[arg(long)]
    pub json: bool,

    /// Use embedding search: true, false, or auto
    /// Only applies to local search; ignored for remote search
    #[arg(long, value_parser = ["true", "false", "auto"])]
    pub embedding: Option<String>,
}

pub async fn execute_search(service: &FastSkillService, args: SearchArgs) -> CliResult<()> {
    // Validate arguments
    validate_search_args(&args)?;

    // Determine search scope
    let scope = determine_search_scope(&args)?;

    // Determine output format
    let format = determine_output_format(&args)?;

    // Create search query
    let query = SearchQuery {
        query: args.query.clone(),
        scope,
        limit: args.limit,
        embedding: determine_embedding_mode(&args),
    };

    // Execute search
    let results = search::execute(query, service).await?;

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

/// Validate search arguments for conflicting options
fn validate_search_args(args: &SearchArgs) -> CliResult<()> {
    // Validate --repo flag only works with remote search
    if args.repo.is_some() && args.local {
        return Err(CliError::Config(
            "Error: --repo is only valid for remote search. Omit --local or --repo.".to_string(),
        ));
    }

    if args.json && args.format.is_some() {
        return Err(CliError::Config(
            "Error: --json and --format cannot be used together. Use one output selector."
                .to_string(),
        ));
    }

    Ok(())
}

/// Determine search scope from CLI arguments
fn determine_search_scope(args: &SearchArgs) -> CliResult<SearchScope> {
    if args.local {
        Ok(SearchScope::Local)
    } else if let Some(repo_name) = &args.repo {
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
            .parse()
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
    use fastskill::ServiceConfig;
    use tempfile::TempDir;

    #[test]
    fn test_validate_search_args_local_and_repo_conflict() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: true,
            remote: false,
            repo: Some("my-repo".to_string()),
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: None,
        };

        let result = validate_search_args(&args);
        assert!(result.is_err());
        if let Err(CliError::Config(msg)) = result {
            assert!(msg.contains("--repo is only valid for remote search"));
        }
    }

    #[test]
    fn test_validate_search_args_valid_local() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: true,
            remote: false,
            repo: None,
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: Some("false".to_string()),
        };

        let result = validate_search_args(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_search_args_valid_remote_with_repo() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: false,
            remote: true,
            repo: Some("my-repo".to_string()),
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: None,
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
            repo: None,
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: None,
        };

        let scope = determine_search_scope(&args).unwrap();
        assert_eq!(scope, SearchScope::Local);
    }

    #[test]
    fn test_determine_search_scope_remote_with_repo() {
        let args = SearchArgs {
            query: "test".to_string(),
            local: false,
            remote: false,
            repo: Some("my-repo".to_string()),
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: None,
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
            repo: None,
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: None,
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
            repo: None,
            limit: 10,
            format: Some("table".to_string()),
            json: true,
            embedding: None,
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
            repo: None,
            limit: 10,
            format: Some("xml".to_string()),
            json: false,
            embedding: None,
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
            repo: None,
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: Some("false".to_string()), // Force text search
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
            repo: None,
            limit: 10,
            format: Some("table".to_string()),
            json: true,
            embedding: None,
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
            repo: None,
            limit: 10,
            format: Some("table".to_string()),
            json: false,
            embedding: Some("auto".to_string()),
        };

        let mode = determine_embedding_mode(&args);
        assert_eq!(mode, None);
    }
}
