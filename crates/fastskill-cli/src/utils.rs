//! Utility functions for CLI operations

pub mod install_utils;
pub mod manifest_utils;
pub mod messages;
pub mod reindex_utils;

use crate::config::get_skill_search_locations_for_display;
use crate::error::{CliError, CliResult, SkillNotFoundMessage};
use std::path::{Path, PathBuf};
use url::Url;

/// Convert ServiceError to CliError, mapping SkillNotFound to the rich message with searched paths and Try suggestions.
pub fn service_error_to_cli(
    e: fastskill_core::ServiceError,
    skill_storage_path: &Path,
    _global: bool,
) -> CliError {
    if let fastskill_core::ServiceError::SkillNotFound(id) = e {
        let searched_paths = get_skill_search_locations_for_display(_global).unwrap_or_else(|_| {
            vec![(
                skill_storage_path.to_path_buf(),
                if _global { "global" } else { "project" }.to_string(),
            )]
        });
        return CliError::SkillNotFound(SkillNotFoundMessage::new(id, searched_paths));
    }
    CliError::Service(e)
}

/// Git repository information parsed from URL
#[derive(Debug, Clone)]
pub struct GitUrlInfo {
    pub repo_url: String,
    pub branch: Option<String>,
    pub subdir: Option<PathBuf>,
}

/// Detect if input is a skill ID (format: skillid@version, skillid, or scope/skillid@version)
pub fn is_skill_id(input: &str) -> bool {
    // Skill ID format: alphanumeric with dashes/underscores, optionally followed by @version or @tag
    // Can also be scoped: scope/skillid or scope/skillid@version
    // Examples: pptx@1.2.3, web-scraper, my_skill@2.0.0, aroff@teste, dev-user/test-skill, org/my-skill@1.0.0
    // Must not contain backslashes or colons (URL schemes)
    if input.contains('\\') || input.contains(':') {
        return false;
    }

    // Check if it looks like a skill ID (not a file path)
    // Supports both unscoped (skillid) and scoped (scope/skillid) formats
    // The @suffix can be a version (1.2.3) or any identifier (teste, label, etc.)
    // This regex is a compile-time constant pattern, so it should never fail
    #[allow(clippy::expect_used)]
    let skill_id_pattern =
        regex::Regex::new(r"^[a-zA-Z0-9_-]+(/[a-zA-Z0-9_-]+)?(@[a-zA-Z0-9_.-]+)?$")
            .expect("Invalid regex pattern");
    skill_id_pattern.is_match(input)
}

/// Parse skill ID and version from input (format: skillid@version)
pub fn parse_skill_id(input: &str) -> (String, Option<String>) {
    if let Some(at_pos) = input.find('@') {
        let skill_id = input[..at_pos].to_string();
        let version = input[at_pos + 1..].to_string();
        (skill_id, Some(version))
    } else {
        (input.to_string(), None)
    }
}

/// Detect the type of skill source (zip, folder, git URL, remote zip URL, or skill ID)
pub fn detect_skill_source(path: &str) -> SkillSource {
    // Check if it's a skill ID first
    if is_skill_id(path) {
        return SkillSource::SkillId(path.to_string());
    }

    // Check if it's a URL (git or http)
    if let Ok(url) = Url::parse(path) {
        if url.scheme() == "git" || url.scheme() == "https" || url.scheme() == "http" {
            // A URL whose path ends in `.zip` is a remote archive to download, not
            // a git repository to clone. Previously this was misclassified as
            // `GitUrl` (a `.zip`-URL add would fail with a git-clone error) — fixed
            // as part of the ADR-0005 Origin/add_from_origin seam, which now
            // supports `Origin::ZipUrl` natively.
            if url.path().to_ascii_lowercase().ends_with(".zip") {
                return SkillSource::RemoteZipUrl(path.to_string());
            }
            return SkillSource::GitUrl(path.to_string());
        }
    }

    let path_buf = PathBuf::from(path);

    // Check if it's a zip file
    if path_buf.extension().and_then(|s| s.to_str()) == Some("zip") {
        return SkillSource::ZipFile(path_buf);
    }

    // Otherwise, treat as folder
    SkillSource::Folder(path_buf)
}

/// Represents different skill source types
#[derive(Debug, Clone)]
pub enum SkillSource {
    /// A `.zip` archive on the local filesystem.
    ZipFile(PathBuf),
    /// A directory on the local filesystem.
    Folder(PathBuf),
    /// A git repository URL (http/https/git scheme, not ending in `.zip`).
    GitUrl(String),
    /// An http(s) URL to a remote `.zip` archive (downloaded, not cloned).
    RemoteZipUrl(String),
    /// Format: `skillid@version`, `skillid`, `scope/skillid`, or `scope/skillid@version`.
    SkillId(String),
}

/// Parse git URL to extract repository information
pub fn parse_git_url(git_url: &str) -> CliResult<GitUrlInfo> {
    let url = Url::parse(git_url)
        .map_err(|e| CliError::InvalidSource(format!("Invalid git URL: {}", e)))?;

    // Extract query parameters for branch
    let query_branch = url
        .query_pairs()
        .find(|(key, _)| key == "branch")
        .map(|(_, value)| value.to_string());

    // For GitHub tree URLs, extract branch and subdir
    let path_segments: Vec<&str> = url.path().split('/').filter(|s| !s.is_empty()).collect();
    let mut branch = None;
    let mut subdir = None;

    if url.host_str() == Some("github.com")
        && path_segments.len() >= 3
        && path_segments.get(2) == Some(&"tree")
        && path_segments.len() >= 4
    {
        // GitHub tree URL: /org/repo/tree/branch[/subdir...]
        branch = Some(path_segments[3].to_string());
        if path_segments.len() > 4 {
            subdir = Some(std::path::PathBuf::from(path_segments[4..].join("/")));
        }
    }

    // Construct clean repo URL
    let mut clean_url = url.clone();
    clean_url.set_query(None);
    let mut path = clean_url.path().to_string();

    // Remove tree/branch/subdir from path for GitHub tree URLs
    if url.host_str() == Some("github.com") && path.contains("/tree/") {
        if let Some(tree_pos) = path.find("/tree/") {
            path = path[..tree_pos].to_string();
        }
    }

    // Ensure .git extension
    if !path.ends_with(".git") {
        path.push_str(".git");
    }

    clean_url.set_path(&path);
    let repo_url = clean_url.to_string();

    Ok(GitUrlInfo {
        repo_url,
        branch: branch.or(query_branch),
        subdir,
    })
}

/// Validate skill structure follows Claude Code standard.
pub fn validate_skill_structure(skill_path: &Path) -> CliResult<()> {
    use fastskill_core::validation::standard_validator::{StandardValidator, ValidationError};

    // Use StandardValidator for comprehensive AI Skill standard validation
    let result = StandardValidator::validate_skill_directory(skill_path);

    match result {
        Ok(validation_result) => {
            let warnings = &validation_result.warnings;

            if !validation_result.is_valid {
                // Format validation errors for CLI display
                let error_messages: Vec<String> = validation_result
                    .errors
                    .iter()
                    .map(|e| match e {
                        ValidationError::InvalidNameFormat(msg) => {
                            format!("✗ Name format invalid: {}", msg)
                        }
                        ValidationError::NameMismatch { expected, actual } => format!(
                            "✗ Name mismatch: Directory '{}' doesn't match skill name '{}'",
                            actual, expected
                        ),
                        ValidationError::InvalidDescriptionLength(len) => format!(
                            "✗ Description length invalid: {} characters (must be 1-1024)",
                            len
                        ),
                        ValidationError::InvalidCompatibilityLength(len) => format!(
                            "✗ Compatibility field too long: {} characters (max 500)",
                            len
                        ),
                        ValidationError::MissingRequiredField(field) => {
                            format!("✗ Missing required field: {}", field)
                        }
                        ValidationError::InvalidFileReference(msg) => {
                            format!("✗ Invalid file reference: {}", msg)
                        }
                        ValidationError::InvalidDirectoryStructure(msg) => {
                            format!("✗ Invalid directory structure: {}", msg)
                        }
                        ValidationError::YamlParseError(msg) => {
                            format!("✗ YAML parsing error: {}", msg)
                        }
                    })
                    .collect();

                let warning_messages: Vec<String> =
                    warnings.iter().map(|w| format!("⚠ {}", w)).collect();

                let mut all_messages = error_messages;
                all_messages.extend(warning_messages);

                return Err(CliError::Validation(all_messages.join("\n")));
            }

            // Log warnings even if validation passes
            for warning in warnings {
                eprintln!("⚠ {}", warning);
            }

            Ok(())
        }
        Err(e) => Err(CliError::Validation(format!("Validation failed: {:?}", e))),
    }
}

// Re-export for use in other modules

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_is_skill_id_regex_valid() {
        // Test that the regex compiles and the function doesn't panic
        // These test various valid skill ID formats
        assert!(is_skill_id("valid-skill"));
        assert!(is_skill_id("scope/id"));
        assert!(is_skill_id("scope/id@1.0.0"));
        assert!(is_skill_id("valid-skill@1.2.3"));
        assert!(is_skill_id("my_skill@2.0.0"));

        // Test that invalid formats are rejected
        assert!(!is_skill_id(""));
        assert!(!is_skill_id("invalid:format"));
        assert!(!is_skill_id("invalid\\path"));
        assert!(!is_skill_id("/absolute/path"));
    }

    #[test]
    fn test_is_skill_id_rejects_urls_and_paths() {
        // Colon (URL scheme) and backslash short-circuit to false.
        assert!(!is_skill_id("https://github.com/org/repo"));
        assert!(!is_skill_id("git@github.com:org/repo"));
        assert!(!is_skill_id("a\\b"));
        // Dotted names are not valid skill IDs (dots not in the id charset).
        assert!(!is_skill_id("skill.zip"));
        // Extra path segments beyond scope/id are rejected.
        assert!(!is_skill_id("a/b/c"));
    }

    #[test]
    fn test_parse_skill_id_with_version() {
        let (id, version) = parse_skill_id("pptx@1.2.3");
        assert_eq!(id, "pptx");
        assert_eq!(version, Some("1.2.3".to_string()));
    }

    #[test]
    fn test_parse_skill_id_without_version() {
        let (id, version) = parse_skill_id("web-scraper");
        assert_eq!(id, "web-scraper");
        assert_eq!(version, None);
    }

    #[test]
    fn test_parse_skill_id_scoped_with_version() {
        let (id, version) = parse_skill_id("org/my-skill@2.0.0");
        assert_eq!(id, "org/my-skill");
        assert_eq!(version, Some("2.0.0".to_string()));
    }

    #[test]
    fn test_detect_skill_source_skill_id() {
        match detect_skill_source("pptx@1.2.3") {
            SkillSource::SkillId(id) => assert_eq!(id, "pptx@1.2.3"),
            other => panic!("expected SkillId, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_skill_source_git_url() {
        // A URL with a path segment (not a bare skill id) is a git URL.
        match detect_skill_source("https://github.com/org/repo.git") {
            SkillSource::GitUrl(url) => assert_eq!(url, "https://github.com/org/repo.git"),
            other => panic!("expected GitUrl, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_skill_source_zip_file() {
        match detect_skill_source("./bundle.zip") {
            SkillSource::ZipFile(path) => assert_eq!(path, PathBuf::from("./bundle.zip")),
            other => panic!("expected ZipFile, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_skill_source_remote_zip_url() {
        // ADR-0005 fix: a URL ending in `.zip` must not be classified as git
        // (previously misclassified as `GitUrl`, causing a spurious git-clone
        // failure instead of a zip download).
        match detect_skill_source("https://example.com/skills/bundle.zip") {
            SkillSource::RemoteZipUrl(url) => {
                assert_eq!(url, "https://example.com/skills/bundle.zip")
            }
            other => panic!("expected RemoteZipUrl, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_skill_source_zip_url_query_string_still_detected() {
        match detect_skill_source("https://example.com/download.zip?token=abc") {
            SkillSource::RemoteZipUrl(_) => {}
            other => panic!("expected RemoteZipUrl, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_skill_source_folder() {
        match detect_skill_source("./some/folder") {
            SkillSource::Folder(path) => assert_eq!(path, PathBuf::from("./some/folder")),
            other => panic!("expected Folder, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_git_url_plain_adds_git_suffix() {
        let info = parse_git_url("https://github.com/org/repo").unwrap();
        assert_eq!(info.repo_url, "https://github.com/org/repo.git");
        assert!(info.branch.is_none());
        assert!(info.subdir.is_none());
    }

    #[test]
    fn test_parse_git_url_keeps_existing_git_suffix() {
        let info = parse_git_url("https://github.com/org/repo.git").unwrap();
        assert_eq!(info.repo_url, "https://github.com/org/repo.git");
    }

    #[test]
    fn test_parse_git_url_query_branch() {
        let info = parse_git_url("https://github.com/org/repo?branch=dev").unwrap();
        assert_eq!(info.repo_url, "https://github.com/org/repo.git");
        assert_eq!(info.branch, Some("dev".to_string()));
    }

    #[test]
    fn test_parse_git_url_github_tree_with_branch_and_subdir() {
        let info = parse_git_url("https://github.com/org/repo/tree/main/skills/inner").unwrap();
        assert_eq!(info.repo_url, "https://github.com/org/repo.git");
        assert_eq!(info.branch, Some("main".to_string()));
        assert_eq!(info.subdir, Some(PathBuf::from("skills/inner")));
    }

    #[test]
    fn test_parse_git_url_github_tree_branch_only() {
        let info = parse_git_url("https://github.com/org/repo/tree/feature-x").unwrap();
        assert_eq!(info.repo_url, "https://github.com/org/repo.git");
        assert_eq!(info.branch, Some("feature-x".to_string()));
        assert!(info.subdir.is_none());
    }

    #[test]
    fn test_parse_git_url_invalid() {
        let result = parse_git_url("not a url");
        assert!(matches!(result, Err(CliError::InvalidSource(_))));
    }

    #[test]
    fn test_validate_skill_structure_valid() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let skill_md = r#"---
name: test-skill
version: "1.0.0"
description: A valid test skill
---
Content
"#;
        std::fs::write(temp_dir.path().join("SKILL.md"), skill_md).unwrap();
        assert!(validate_skill_structure(temp_dir.path()).is_ok());
    }

    #[test]
    fn test_validate_skill_structure_invalid() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let skill_md = r#"---
name: ""
description: A test skill
---
"#;
        std::fs::write(temp_dir.path().join("SKILL.md"), skill_md).unwrap();
        let result = validate_skill_structure(temp_dir.path());
        assert!(matches!(result, Err(CliError::Validation(_))));
    }

    #[test]
    fn test_validate_skill_structure_missing_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        // No SKILL.md at all: validator surfaces a Validation error.
        let result = validate_skill_structure(temp_dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_service_error_to_cli_non_not_found_passthrough() {
        let e = fastskill_core::ServiceError::Custom("boom".to_string());
        let cli = service_error_to_cli(e, Path::new("/tmp/storage"), false);
        assert!(matches!(cli, CliError::Service(_)));
    }

    #[test]
    fn test_service_error_to_cli_not_found_maps_to_rich_message() {
        let e = fastskill_core::ServiceError::SkillNotFound("missing-skill".to_string());
        let cli = service_error_to_cli(e, Path::new("/tmp/storage"), false);
        assert!(matches!(cli, CliError::SkillNotFound(_)));
    }
}
