//! Resolve command implementation
//!
//! Machine-first skill resolution that returns canonical paths, metadata, and optional content.

use crate::error::{CliError, CliResult};
use clap::Args;
use fastskill_core::core::context_resolver::{ContentMode, ResolveContextRequest, ResolveScope};
use fastskill_core::FastSkillService;
use std::path::PathBuf;

#[derive(Debug, Args)]
#[command(
    about = "Resolve skills as machine-readable JSON with canonical paths and optional content",
    after_help = "Examples:\n  fastskill resolve \"text processing\"\n  fastskill resolve \"pptx\" --limit 3 --include-content preview\n  fastskill resolve \"query\" --include-content full --no-resolve-paths"
)]
pub struct ResolveArgs {
    /// Search query string
    pub query: String,

    /// Maximum number of results (default: 5)
    #[arg(short, long, default_value = "5")]
    pub limit: usize,

    /// Content to include: none, preview (frontmatter + 20 lines), full
    #[arg(long, default_value = "none", value_parser = ["none", "preview", "full"])]
    pub include_content: String,

    /// Resolve and validate canonical absolute paths (default: true)
    #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
    pub resolve_paths: bool,

    /// Skills directory path (overrides default discovery)
    #[arg(long, help = "Skills directory path")]
    pub skills_dir: Option<PathBuf>,
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
