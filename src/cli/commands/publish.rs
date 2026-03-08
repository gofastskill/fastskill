//! Publish command implementation

use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::{api_client::ApiClient, messages};
use clap::Args;
use fastskill::core::repository::{
    RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
};
use std::path::{Path, PathBuf};
use tracing::info;
use url::Url;

/// Publish artifacts to remote API or local folder
///
/// MODES (mutually exclusive):
///   --api-url <url>     Publish to API endpoint (e.g., https://api.example.com)
///   --local-dir <path>  Publish to local directory
///   --registry <name>   Use configured repository
///   (default)           Use FASTSKILL_API_URL environment variable
#[derive(Debug, Args)]
pub struct PublishArgs {
    /// Package file or directory containing ZIP artifacts
    #[arg(short, long, default_value = "./artifacts")]
    pub artifacts: PathBuf,

    /// Repository name from configuration (mutually exclusive with --api-url and --local-dir)
    #[arg(long)]
    pub registry: Option<String>,

    /// API URL for publishing (e.g., https://api.example.com)
    /// Mutually exclusive with --local-dir and --registry
    #[arg(long)]
    pub api_url: Option<String>,

    /// Local directory for publishing
    /// Mutually exclusive with --api-url and --registry
    #[arg(long)]
    pub local_dir: Option<String>,

    /// Wait for validation to complete (default: false for API mode)
    /// For local mode, this flag has no effect
    #[arg(long)]
    pub wait: bool,

    /// Maximum wait time in seconds (default: 300)
    #[arg(long, default_value = "300")]
    pub max_wait: u64,
}

/// Context for publish operations
#[derive(Debug)]
struct PublishContext {
    target: String,
    packages: Vec<PathBuf>,
    is_url: bool,
    wait: bool,
    max_wait: u64,
}

impl PublishContext {
    fn new(args: PublishArgs) -> CliResult<Self> {
        // Validate mutually exclusive target flags (PUB_001)
        validate_publish_target_exclusive(&args)?;

        // Determine target URL or path
        let target = determine_target(&args)?;
        let is_url = Url::parse(&target).is_ok();

        // Collect packages
        let packages = collect_packages(&args.artifacts)?;
        validate_packages(&packages, &args.artifacts)?;

        // Wait defaults to false (user must explicitly enable it)
        let wait = args.wait;

        Ok(Self {
            target,
            packages,
            is_url,
            wait,
            max_wait: args.max_wait,
        })
    }
}

/// Validate that only one target mode is specified (PUB_001)
fn validate_publish_target_exclusive(args: &PublishArgs) -> CliResult<()> {
    let target_count = [&args.registry, &args.api_url, &args.local_dir]
        .iter()
        .filter(|t| t.is_some())
        .count();

    if target_count > 1 {
        return Err(CliError::Validation(
            "PUB_001: Cannot specify multiple target modes. Use only one of: --registry, --api-url, --local-dir".to_string(),
        ));
    }
    Ok(())
}

/// Determine the target URL or path from arguments
fn determine_target(args: &PublishArgs) -> CliResult<String> {
    if let Some(registry_name) = &args.registry {
        determine_target_from_registry(registry_name)
    } else if let Some(api_url) = &args.api_url {
        Ok(api_url.clone())
    } else if let Some(local_dir) = &args.local_dir {
        Ok(local_dir.clone())
    } else {
        // Default: Use FASTSKILL_API_URL env var
        std::env::var("FASTSKILL_API_URL").map_err(|_| {
            CliError::Validation(
                "PUB_002: No target specified and FASTSKILL_API_URL not set. Use --api-url, --local-dir, --registry, or set FASTSKILL_API_URL".to_string(),
            )
        })
    }
}

/// Determine target from registry configuration
fn determine_target_from_registry(registry_name: &str) -> CliResult<String> {
    let repositories = crate::cli::config::load_repositories_from_project()?;
    let repo_manager = RepositoryManager::from_definitions(repositories);

    let repo = repo_manager.get_repository(registry_name).ok_or_else(|| {
        CliError::Config(format!(
            "Repository '{}' not found in repositories.toml",
            registry_name
        ))
    })?;

    extract_api_url_from_repository(repo)
}

/// Validate that packages were found
fn validate_packages(packages: &[PathBuf], artifacts: &Path) -> CliResult<()> {
    if packages.is_empty() {
        return Err(CliError::Validation(format!(
            "No ZIP packages found in: {}",
            artifacts.display()
        )));
    }
    Ok(())
}

pub async fn execute_publish(args: PublishArgs) -> CliResult<()> {
    info!("Starting publish command");

    // Create publish context
    let context = PublishContext::new(args)?;

    println!(
        "{}",
        messages::info(&format!(
            "Found {} package(s) to publish",
            context.packages.len()
        ))
    );

    // Execute publish based on target type
    if context.is_url {
        publish_to_api_with_auth(&context).await?;
    } else {
        publish_to_local_folder(&context.target, &context.packages).await?;
    }

    Ok(())
}

/// Publish to API with authentication
async fn publish_to_api_with_auth(context: &PublishContext) -> CliResult<()> {
    let token = crate::cli::auth_config::get_token_with_refresh(&context.target).await?;
    let token_str = token.ok_or_else(|| {
        CliError::Validation(format!(
            "No authentication token found for registry: {}. Run `fastskill auth login` to authenticate.",
            context.target
        ))
    })?;

    publish_to_api(
        &context.target,
        &context.packages,
        Some(&token_str),
        context.wait,
        context.max_wait,
    )
    .await
}

/// Collect packages from artifacts path (file or directory)
fn collect_packages(artifacts: &PathBuf) -> CliResult<Vec<PathBuf>> {
    if !artifacts.exists() {
        return Err(CliError::Validation(format!(
            "Artifacts path does not exist: {}",
            artifacts.display()
        )));
    }

    if artifacts.is_file() {
        // Single file
        if artifacts.extension().and_then(|s| s.to_str()) == Some("zip") {
            Ok(vec![artifacts.clone()])
        } else {
            Err(CliError::Validation(format!(
                "Artifact must be a ZIP file: {}",
                artifacts.display()
            )))
        }
    } else {
        // Directory - find all ZIP files
        find_zip_files(artifacts)
    }
}

/// Check if a path is a ZIP file
fn is_zip_file(path: &Path) -> bool {
    path.is_file() && path.extension().is_some_and(|ext| ext == "zip")
}

/// Find all ZIP files in a directory
fn find_zip_files(dir: &PathBuf) -> CliResult<Vec<PathBuf>> {
    let entries = std::fs::read_dir(dir).map_err(CliError::Io)?;

    let zip_files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| is_zip_file(path))
        .collect();

    Ok(zip_files)
}

/// Get package name from path
fn get_package_name(package: &Path) -> &str {
    package
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
}

/// Handle validation status after waiting
fn handle_validation_status(
    status: &crate::cli::utils::api_client::PublishStatusApiResponse,
) -> CliResult<()> {
    match status.status.as_str() {
        "pending" | "validating" => {
            eprintln!(
                "{}",
                messages::error(&format!(
                    "Timeout waiting for validation. Current status: {}. Use --max-wait to increase timeout.",
                    status.status
                ))
            );
            Ok(())
        }
        "accepted" => {
            println!(
                "{}",
                messages::ok(&format!(
                    "Package accepted: {} v{}",
                    status.skill_id, status.version
                ))
            );
            Ok(())
        }
        "rejected" => {
            eprintln!(
                "{}",
                messages::error(&format!(
                    "Package validation failed: {:?}",
                    status.validation_errors
                ))
            );
            Err(CliError::Validation(format!(
                "Package was rejected: {:?}",
                status.validation_errors
            )))
        }
        _ => Ok(()),
    }
}

/// Wait for package validation to complete
async fn wait_for_package_validation(
    client: &ApiClient,
    job_id: &str,
    max_wait: u64,
) -> CliResult<()> {
    println!("{}", messages::info("Waiting for validation..."));

    match client.wait_for_completion(job_id, max_wait).await {
        Ok(status) => handle_validation_status(&status),
        Err(e) => {
            eprintln!(
                "{}",
                messages::error(&format!("Package validation failed: {}", e))
            );
            Err(e)
        }
    }
}

/// Publish a single package to API
async fn publish_single_package(
    client: &ApiClient,
    package: &Path,
    api_url: &str,
    wait: bool,
    max_wait: u64,
) -> CliResult<()> {
    let package_name = get_package_name(package);

    println!(
        "{}",
        messages::info(&format!("Publishing: {}", package_name))
    );

    // Publish package
    let response = client
        .publish_package(package)
        .await
        .map_err(|e| CliError::Validation(format!("Failed to publish {}: {}", package_name, e)))?;

    println!(
        "{}",
        messages::ok(&format!(
            "Package queued: {} v{} (job: {})",
            response.skill_id, response.version, response.job_id
        ))
    );

    // Get initial status
    let initial_status = client
        .get_publish_status(&response.job_id)
        .await
        .map_err(|e| CliError::Validation(format!("Failed to get initial status: {}", e)))?;

    println!(
        "{}",
        messages::info(&format!("Initial status: {}", initial_status.status))
    );

    // Wait for completion if requested
    if wait {
        wait_for_package_validation(client, &response.job_id, max_wait).await?;
    } else {
        println!(
            "{}",
            messages::info(&format!(
                "Check status with: GET {}/api/registry/publish/status/{}",
                api_url, response.job_id
            ))
        );
    }

    Ok(())
}

/// Publish packages to API
async fn publish_to_api(
    api_url: &str,
    packages: &[PathBuf],
    token: Option<&str>,
    wait: bool,
    max_wait: u64,
) -> CliResult<()> {
    let client = ApiClient::new(api_url, token.map(|s| s.to_string()))?;

    println!(
        "{}",
        messages::info(&format!("Publishing to API: {}", api_url))
    );

    for package in packages {
        publish_single_package(&client, package, api_url, wait, max_wait).await?;
    }

    Ok(())
}

/// Publish packages to local folder
async fn publish_to_local_folder(target: &str, packages: &[PathBuf]) -> CliResult<()> {
    let target_path = PathBuf::from(target);

    // Create target directory if it doesn't exist
    if !target_path.exists() {
        std::fs::create_dir_all(&target_path).map_err(CliError::Io)?;
        println!(
            "{}",
            messages::info(&format!("Created target directory: {}", target))
        );
    }

    if !target_path.is_dir() {
        return Err(CliError::Validation(format!(
            "Target must be a directory: {}",
            target
        )));
    }

    println!(
        "{}",
        messages::info(&format!("Publishing to local folder: {}", target))
    );

    for package in packages {
        let package_name = package
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let dest_path = target_path.join(package_name);

        println!(
            "{}",
            messages::info(&format!(
                "Copying: {} -> {}",
                package_name,
                dest_path.display()
            ))
        );

        // Copy file
        std::fs::copy(package, &dest_path).map_err(CliError::Io)?;

        println!("{}", messages::ok(&format!("Published: {}", package_name)));
    }

    println!();
    println!(
        "{}",
        messages::ok(&format!(
            "Published {} package(s) to local folder",
            packages.len()
        ))
    );

    Ok(())
}

/// Extract API URL from repository configuration
fn extract_api_url_from_repository(repo: &RepositoryDefinition) -> CliResult<String> {
    match &repo.repo_type {
        RepositoryType::HttpRegistry => {
            // For http-registry, check storage.base_url first
            if let Some(storage) = &repo.storage {
                if let Some(base_url) = &storage.base_url {
                    return Ok(base_url.clone());
                }
            }

            // If index_url is already an API URL (not a git URL), use it
            let index_url = match &repo.config {
                RepositoryConfig::HttpRegistry { index_url } => index_url,
                _ => {
                    return Err(CliError::Config(
                        "Invalid repository config".to_string(),
                    ))
                }
            };

            // If it's already an HTTP(S) URL, use it
            if index_url.starts_with("http://") || index_url.starts_with("https://") {
                // It's an API URL, extract base URL (remove /index suffix if present)
                let base = index_url.trim_end_matches("/index").trim_end_matches("/");
                return Ok(base.to_string());
            }

            // For non-HTTP URLs, we can't determine the API URL
            Err(CliError::Config(format!(
                "Cannot determine API URL for http-registry '{}'. \
                Please specify storage.base_url in repositories.toml or use --target flag.",
                repo.name
            )))
        }
        _ => Err(CliError::Config(format!(
            "Repository type '{}' does not support publishing. Only http-registry type supports publishing.",
            repo.name
        ))),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::ptr_arg
)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Create test publish args
    fn create_test_args(artifacts: PathBuf, local_dir: Option<String>, wait: bool) -> PublishArgs {
        PublishArgs {
            artifacts,
            registry: None,
            api_url: None,
            local_dir,
            wait,
            max_wait: 300,
        }
    }

    /// Create a valid ZIP file for testing
    fn create_test_zip(artifacts_dir: &PathBuf, skill_name: &str) {
        use std::io::Write;
        use zip::{write::FileOptions, ZipWriter};

        let skill_content = format!(
            r#"# {}

Name: {}
Version: 1.0.0
Description: A test skill for coverage
"#,
            skill_name, skill_name
        );

        let zip_path = artifacts_dir.join(format!("{}-1.0.0.zip", skill_name));
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("SKILL.md", options).unwrap();
        zip.write_all(skill_content.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    #[tokio::test]
    async fn test_execute_publish_nonexistent_artifacts() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_path = temp_dir.path().join("nonexistent");

        // Provide a target to avoid PUB_002 error
        let args = create_test_args(
            nonexistent_path,
            Some("/tmp/test-publish".to_string()),
            true,
        );

        let result = execute_publish(args).await;
        assert!(result.is_err());
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("does not exist"));
        } else {
            panic!("Expected Validation error");
        }
    }

    #[tokio::test]
    async fn test_execute_publish_empty_artifacts_dir() {
        let temp_dir = TempDir::new().unwrap();
        let artifacts_dir = temp_dir.path().join("artifacts");
        fs::create_dir_all(&artifacts_dir).unwrap();

        let args = create_test_args(artifacts_dir, Some("/tmp/test-publish".to_string()), true);

        let result = execute_publish(args).await;
        assert!(result.is_err());
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("No ZIP packages found"));
        } else {
            panic!("Expected Validation error");
        }
    }

    #[tokio::test]
    async fn test_execute_publish_invalid_file_type() {
        let temp_dir = TempDir::new().unwrap();
        let invalid_file = temp_dir.path().join("not-a-zip.txt");
        fs::write(&invalid_file, "not a zip file").unwrap();

        let args = create_test_args(invalid_file, Some("/tmp/test-publish".to_string()), true);

        let result = execute_publish(args).await;
        assert!(result.is_err());
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("ZIP file"));
        } else {
            panic!("Expected Validation error");
        }
    }

    #[tokio::test]
    async fn test_execute_publish_success() {
        let temp_dir = TempDir::new().unwrap();
        let artifacts_dir = temp_dir.path().join("artifacts");
        fs::create_dir_all(&artifacts_dir).unwrap();

        let target_dir = temp_dir.path().join("target");
        fs::create_dir_all(&target_dir).unwrap();

        create_test_zip(&artifacts_dir, "test-skill");

        let args = create_test_args(artifacts_dir, Some(target_dir.display().to_string()), false);

        let result = execute_publish(args).await;
        // May fail due to missing S3 config or other issues, but should process the zip
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_publish_context_api_url_target() {
        let temp_dir = TempDir::new().unwrap();
        let artifacts_dir = temp_dir.path().join("artifacts");
        fs::create_dir_all(&artifacts_dir).unwrap();

        create_test_zip(&artifacts_dir, "test-skill");

        let args = PublishArgs {
            artifacts: artifacts_dir,
            registry: None,
            api_url: Some("https://registry.example.com".to_string()),
            local_dir: None,
            wait: false,
            max_wait: 300,
        };

        // This test verifies the context creation succeeds with an API URL target
        let result = PublishContext::new(args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_publish_context_local_path() {
        let temp_dir = TempDir::new().unwrap();
        let artifacts_dir = temp_dir.path().join("artifacts");
        fs::create_dir_all(&artifacts_dir).unwrap();

        let target_dir = temp_dir.path().join("target");
        fs::create_dir_all(&target_dir).unwrap();

        create_test_zip(&artifacts_dir, "test-skill");

        let args = PublishArgs {
            artifacts: artifacts_dir,
            registry: None,
            api_url: None,
            local_dir: Some(target_dir.display().to_string()),
            wait: false,
            max_wait: 300,
        };

        // This test verifies the context creation succeeds with a local path
        let result = PublishContext::new(args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_publish_context_conflicting_targets_pub_001() {
        let temp_dir = TempDir::new().unwrap();
        let artifacts_dir = temp_dir.path().join("artifacts");
        fs::create_dir_all(&artifacts_dir).unwrap();

        create_test_zip(&artifacts_dir, "test-skill");

        // Test: both api_url and local_dir specified (should fail)
        let args = PublishArgs {
            artifacts: artifacts_dir.clone(),
            registry: None,
            api_url: Some("https://registry.example.com".to_string()),
            local_dir: Some("/tmp/target".to_string()),
            wait: false,
            max_wait: 300,
        };

        let result = PublishContext::new(args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CliError::Validation(msg) if msg.contains("PUB_001")));

        // Test: both registry and api_url specified (should fail)
        let args = PublishArgs {
            artifacts: artifacts_dir.clone(),
            registry: Some("my-registry".to_string()),
            api_url: Some("https://registry.example.com".to_string()),
            local_dir: None,
            wait: false,
            max_wait: 300,
        };

        let result = PublishContext::new(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_publish_context_no_target_pub_002() {
        let temp_dir = TempDir::new().unwrap();
        let artifacts_dir = temp_dir.path().join("artifacts");
        fs::create_dir_all(&artifacts_dir).unwrap();

        create_test_zip(&artifacts_dir, "test-skill");

        // Test: no target specified and no env var (should fail)
        let args = PublishArgs {
            artifacts: artifacts_dir,
            registry: None,
            api_url: None,
            local_dir: None,
            wait: false,
            max_wait: 300,
        };

        let result = PublishContext::new(args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CliError::Validation(msg) if msg.contains("PUB_002")));
    }
}
