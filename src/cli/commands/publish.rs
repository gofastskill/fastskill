//! Publish command implementation

use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::{api_client::ApiClient, messages};
use clap::Args;
use fastskill::core::repository::{
    RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
};
use std::path::PathBuf;
use tracing::info;
use url::Url;

/// Publish artifacts to registry API or local folder
#[derive(Debug, Args)]
pub struct PublishArgs {
    /// Package file or directory containing ZIP artifacts
    #[arg(short, long, default_value = "./artifacts")]
    pub artifacts: PathBuf,

    /// Registry name from repositories.toml (mutually exclusive with --target)
    #[arg(long)]
    pub registry: Option<String>,

    /// Target: API URL (e.g., https://registry.example.com) or local folder path
    /// If not specified, defaults to API mode using FASTSKILL_API_URL env var
    /// Mutually exclusive with --registry
    #[arg(short, long)]
    pub target: Option<String>,

    /// Wait for validation to complete (only for API mode, default: true)
    #[arg(long, default_value = "true")]
    pub wait: bool,

    /// Don't wait for validation to complete (overrides --wait)
    #[arg(long)]
    pub no_wait: bool,

    /// Maximum wait time in seconds (default: 300)
    #[arg(long, default_value = "300")]
    pub max_wait: u64,
}

pub async fn execute_publish(args: PublishArgs) -> CliResult<()> {
    info!("Starting publish command");

    // Validate mutually exclusive flags
    if args.registry.is_some() && args.target.is_some() {
        return Err(CliError::Config(
            "Cannot specify both --registry and --target. Use one or the other.".to_string(),
        ));
    }

    // Determine target (API URL or local path)
    let target = if let Some(registry_name) = &args.registry {
        // Load from repositories.toml (searches up directory tree)
        let repos_path = crate::cli::config::get_repositories_toml_path()
            .map_err(|e| CliError::Config(format!("Failed to find repositories.toml: {}", e)))?;
        let mut repo_manager = RepositoryManager::new(repos_path);
        repo_manager
            .load()
            .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

        let repo = repo_manager.get_repository(registry_name).ok_or_else(|| {
            CliError::Config(format!(
                "Repository '{}' not found in repositories.toml",
                registry_name
            ))
        })?;

        // Extract API URL from repository configuration
        extract_api_url_from_repository(repo)?
    } else {
        // Existing logic: --target flag, env var, or default
        args.target
            .or_else(|| std::env::var("FASTSKILL_API_URL").ok())
            .unwrap_or_else(|| "http://localhost:8080".to_string())
    };

    // Detect if target is URL or local path
    let is_url = Url::parse(&target).is_ok();

    // Collect packages to publish (validate artifacts path exists first)
    let packages = collect_packages(&args.artifacts)?;

    if packages.is_empty() {
        return Err(CliError::Validation(format!(
            "No ZIP packages found in: {}",
            args.artifacts.display()
        )));
    }

    println!(
        "{}",
        messages::info(&format!("Found {} package(s) to publish", packages.len()))
    );

    // Get token (with automatic refresh if needed)
    let token = if is_url {
        crate::cli::auth_config::get_token_with_refresh(&target).await?
    } else {
        None
    };

    // Apply --no-wait override
    let wait = if args.no_wait { false } else { args.wait };

    if is_url {
        // API mode - check if token is available
        let token_str = token.ok_or_else(|| {
            CliError::Validation(format!(
                "No authentication token found for registry: {}. Run `fastskill auth login` to authenticate.",
                target
            ))
        })?;
        publish_to_api(&target, &packages, Some(&token_str), wait, args.max_wait).await?;
    } else {
        // Local folder mode
        publish_to_local_folder(&target, &packages).await?;
    }

    Ok(())
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

/// Find all ZIP files in a directory
fn find_zip_files(dir: &PathBuf) -> CliResult<Vec<PathBuf>> {
    let mut zip_files = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(CliError::Io)?;

    for entry in entries {
        let entry = entry.map_err(CliError::Io)?;
        let path = entry.path();

        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "zip" {
                    zip_files.push(path);
                }
            }
        }
    }

    Ok(zip_files)
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
        let package_name = package
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        println!(
            "{}",
            messages::info(&format!("Publishing: {}", package_name))
        );

        // Publish package (server will extract scope from JWT token)
        let response = client.publish_package(package).await.map_err(|e| {
            CliError::Validation(format!("Failed to publish {}: {}", package_name, e))
        })?;

        println!(
            "{}",
            messages::ok(&format!(
                "Package queued: {} v{} (job: {})",
                response.skill_id, response.version, response.job_id
            ))
        );

        // Always poll status at least once to show initial status
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
            println!("{}", messages::info("Waiting for validation..."));

            match client.wait_for_completion(&response.job_id, max_wait).await {
                Ok(status) => {
                    // Check if we timed out (status is still pending/validating)
                    if status.status == "pending" || status.status == "validating" {
                        eprintln!("{}", messages::error(&format!(
                            "Timeout waiting for validation. Current status: {}. Use --max-wait to increase timeout.",
                            status.status
                        )));
                        // Don't return error, just show warning and continue
                    } else if status.status == "accepted" {
                        println!(
                            "{}",
                            messages::ok(&format!(
                                "Package accepted: {} v{}",
                                status.skill_id, status.version
                            ))
                        );
                    } else if status.status == "rejected" {
                        eprintln!(
                            "{}",
                            messages::error(&format!(
                                "Package validation failed: {:?}",
                                status.validation_errors
                            ))
                        );
                        return Err(CliError::Validation(format!(
                            "Package was rejected: {:?}",
                            status.validation_errors
                        )));
                    }
                }
                Err(e) => {
                    eprintln!(
                        "{}",
                        messages::error(&format!("Package validation failed: {}", e))
                    );
                    return Err(e);
                }
            }
        } else {
            println!(
                "{}",
                messages::info(&format!(
                    "Check status with: GET {}/api/registry/publish/status/{}",
                    api_url, response.job_id
                ))
            );
        }
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
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_publish_nonexistent_artifacts() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_path = temp_dir.path().join("nonexistent");

        let args = PublishArgs {
            artifacts: nonexistent_path,
            registry: None,
            target: None,
            wait: true,
            no_wait: false,
            max_wait: 300,
        };

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

        let args = PublishArgs {
            artifacts: artifacts_dir,
            registry: None,
            target: Some("/tmp/test-publish".to_string()),
            wait: true,
            no_wait: false,
            max_wait: 300,
        };

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

        let args = PublishArgs {
            artifacts: invalid_file,
            registry: None,
            target: Some("/tmp/test-publish".to_string()),
            wait: true,
            no_wait: false,
            max_wait: 300,
        };

        let result = execute_publish(args).await;
        assert!(result.is_err());
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("ZIP file"));
        } else {
            panic!("Expected Validation error");
        }
    }
}
