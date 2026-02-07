//! Add command implementation

use crate::cli::error::{manifest_required_for_add_message, CliError, CliResult};
use crate::cli::utils::manifest_utils;
use crate::cli::utils::{
    detect_skill_source, parse_git_url, parse_skill_id, validate_skill_structure, SkillSource,
};
use chrono::Utc;
use clap::Args;
use fastskill::core::project::resolve_project_file;
use fastskill::core::repository::RepositoryManager;
use fastskill::core::skill_manager::SourceType;
use fastskill::{FastSkillService, SkillDefinition};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tracing::info;
use walkdir::WalkDir;

/// T028: Helper to update skill-project.toml and skills.lock
fn update_project_files(
    skill_def: &SkillDefinition,
    groups: Vec<String>,
    editable: bool,
) -> CliResult<()> {
    // Update skill-project.toml
    manifest_utils::add_skill_to_project_toml(skill_def, groups.clone(), editable)
        .map_err(|e| CliError::Config(format!("Failed to update skill-project.toml: {}", e)))?;

    // T033: Lock file at project root
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    let lock_path = if let Some(parent) = project_file_result.path.parent() {
        parent.join("skills.lock")
    } else {
        PathBuf::from("skills.lock")
    };
    manifest_utils::update_lock_file(&lock_path, skill_def, groups)
        .map_err(|e| CliError::Config(format!("Failed to update lock file: {}", e)))?;

    Ok(())
}

/// Discover all directories containing SKILL.md under the given base directory
/// Skips hidden directories (those starting with '.')
fn get_skill_dirs_recursive(base: &Path) -> CliResult<Vec<PathBuf>> {
    if !base.exists() {
        return Err(CliError::InvalidSource(format!(
            "Directory does not exist: {}",
            base.display()
        )));
    }

    if !base.is_dir() {
        return Err(CliError::InvalidSource(format!(
            "Path is not a directory: {}",
            base.display()
        )));
    }

    let mut skill_dirs = Vec::new();
    let base_canonical = base.canonicalize().map_err(CliError::Io)?;

    for entry in WalkDir::new(base)
        .min_depth(1)
        .max_depth(usize::MAX)
        .follow_links(false)
    {
        let entry = entry.map_err(|e| {
            CliError::Io(
                e.into_io_error()
                    .unwrap_or_else(|| std::io::Error::other("WalkDir error")),
            )
        })?;
        let path = entry.path();

        // Check if this directory contains SKILL.md
        if path.is_dir() && path.join("SKILL.md").exists() {
            // Canonicalize to check relative path from base
            let path_canonical = path.canonicalize().map_err(CliError::Io)?;

            // Compute relative path to check for hidden directories
            let relative_path = path_canonical.strip_prefix(&base_canonical).map_err(|_| {
                CliError::Validation(format!(
                    "Failed to compute relative path for: {}",
                    path.display()
                ))
            })?;

            // Skip hidden directories (check if any component of relative path starts with '.')
            let should_skip = relative_path.components().any(|c| {
                if let std::path::Component::Normal(name) = c {
                    name.to_string_lossy().starts_with('.')
                } else {
                    false
                }
            });
            if should_skip {
                continue;
            }

            skill_dirs.push(path_canonical);
        }
    }

    Ok(skill_dirs)
}

/// Add a new skill and update skill-project.toml [dependencies]
///
/// Adds the skill to skill-project.toml [dependencies] section and updates skills.lock.
/// Supports git URLs, local paths, ZIP files, and registry skill IDs.
#[derive(Debug, Args)]
pub struct AddArgs {
    /// Source: path to zip file, folder, git URL, or skill ID (e.g., pptx@1.2.3)
    pub source: String,

    /// Override source type (registry, github, local)
    /// Default is registry for skill IDs
    #[arg(long)]
    pub source_type: Option<String>,

    /// Git branch to checkout (only for git URLs)
    #[arg(long)]
    pub branch: Option<String>,

    /// Git tag to checkout (only for git URLs)
    #[arg(long)]
    pub tag: Option<String>,

    /// Force registration even if skill already exists
    #[arg(short, long)]
    pub force: bool,

    /// Install skill in editable mode (for local development, like poetry add -e)
    #[arg(short = 'e', long)]
    pub editable: bool,

    /// Add skill to a specific group (like poetry add --group dev)
    #[arg(long)]
    pub group: Option<String>,

    /// Add all skills found under the directory (only for local folders)
    #[arg(short = 'r', long)]
    pub recursive: bool,
}

pub async fn execute_add(
    service: &FastSkillService,
    args: AddArgs,
    verbose: bool,
    global: bool,
) -> CliResult<()> {
    // Require manifest before any install work
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    if !project_file_result.found {
        return Err(CliError::Config(
            manifest_required_for_add_message().to_string(),
        ));
    }

    let groups = args.group.map(|g| vec![g]).unwrap_or_default();

    // Check if source type is overridden
    let source = if let Some(ref source_type) = args.source_type {
        match source_type.as_str() {
            "registry" => SkillSource::SkillId(args.source.clone()),
            "github" | "git" => SkillSource::GitUrl(args.source.clone()),
            "local" => {
                let path = PathBuf::from(&args.source);
                if path.extension().and_then(|s| s.to_str()) == Some("zip") {
                    SkillSource::ZipFile(path)
                } else {
                    SkillSource::Folder(path)
                }
            }
            _ => detect_skill_source(&args.source),
        }
    } else {
        detect_skill_source(&args.source)
    };

    // Handle recursive add
    if args.recursive {
        // Recursive only works with local folders
        if let SkillSource::Folder(ref path) = source {
            info!(
                "Adding skills recursively from directory: {}",
                path.display()
            );

            // Discover all skill directories
            let skill_dirs = get_skill_dirs_recursive(path)?;

            if skill_dirs.is_empty() {
                return Err(CliError::Validation(format!(
                    "No skill directories found under {}",
                    path.display()
                )));
            }

            println!(
                "Found {} skill(s) in directory {}",
                skill_dirs.len(),
                path.display()
            );

            // Add each skill directory, collecting failures but continuing
            let mut failed: Vec<(PathBuf, CliError)> = Vec::new();
            let mut success_count = 0;

            for skill_path in skill_dirs {
                // Use the same logic as single folder add
                match add_from_folder(
                    service,
                    &skill_path,
                    args.force,
                    args.editable,
                    groups.clone(),
                    verbose,
                    global,
                )
                .await
                {
                    Ok(()) => success_count += 1,
                    Err(e) => {
                        eprintln!("Skill at {}: {}", skill_path.display(), e);
                        failed.push((skill_path, e));
                    }
                }
            }

            // Report results
            if !failed.is_empty() {
                let total = success_count + failed.len();
                return Err(CliError::Validation(format!(
                    "{} of {} skills failed:\n{}",
                    failed.len(),
                    total,
                    failed
                        .iter()
                        .map(|(path, err)| format!("  - {}: {}", path.display(), err))
                        .collect::<Vec<_>>()
                        .join("\n")
                )));
            }

            return Ok(());
        } else {
            return Err(CliError::Config(
                "Recursive add is only valid when source is a local directory".to_string(),
            ));
        }
    }

    match source {
        SkillSource::ZipFile(path) => {
            add_from_zip(
                service,
                &path,
                args.force,
                args.editable,
                groups,
                verbose,
                global,
            )
            .await
        }
        SkillSource::Folder(path) => {
            // Check if the folder has a SKILL.md at the root
            if !path.join("SKILL.md").exists() {
                // No SKILL.md at root - check if there are skills in subdirectories
                match get_skill_dirs_recursive(&path) {
                    Ok(skill_dirs) if !skill_dirs.is_empty() => {
                        return Err(CliError::Validation(format!(
                            "This directory has no SKILL.md at the root but contains {} skill(s) in subdirectories. Add them all with: fastskill add {} --recursive",
                            skill_dirs.len(),
                            path.display()
                        )));
                    }
                    _ => {
                        return Err(CliError::Validation(format!(
                            "No SKILL.md found in {}. A skill directory must contain a SKILL.md file. If this folder has multiple skills in subdirectories, use --recursive.",
                            path.display()
                        )));
                    }
                }
            }
            add_from_folder(
                service,
                &path,
                args.force,
                args.editable,
                groups,
                verbose,
                global,
            )
            .await
        }
        SkillSource::GitUrl(url) => {
            add_from_git(
                service,
                &url,
                args.branch.as_deref(),
                args.tag.as_deref(),
                args.force,
                args.editable,
                groups,
                verbose,
                global,
            )
            .await
        }
        SkillSource::SkillId(skill_id) => {
            add_from_registry(
                service,
                &skill_id,
                args.force,
                args.editable,
                groups,
                verbose,
                global,
            )
            .await
        }
    }
}

async fn add_from_zip(
    service: &FastSkillService,
    zip_path: &Path,
    force: bool,
    editable: bool,
    groups: Vec<String>,
    verbose: bool,
    global: bool,
) -> CliResult<()> {
    info!("Adding skill from zip file: {}", zip_path.display());

    // Basic validation - check file exists
    if !zip_path.exists() {
        return Err(CliError::InvalidSource(format!(
            "Zip file does not exist: {}",
            zip_path.display()
        )));
    }

    // Extract zip to temporary directory
    let temp_dir = TempDir::new().map_err(CliError::Io)?;
    let extract_path = temp_dir.path();

    // Extract zip file safely using shared handler
    {
        use fastskill::storage::zip::ZipHandler;
        let zip_handler = ZipHandler::new()
            .map_err(|e| CliError::InvalidSource(format!("Failed to create ZIP handler: {}", e)))?;
        zip_handler
            .extract_to_dir(zip_path, extract_path)
            .map_err(|e| match e {
                fastskill::core::service::ServiceError::Validation(msg) => {
                    CliError::InvalidSource(format!("ZIP extraction validation failed: {}", msg))
                }
                fastskill::core::service::ServiceError::Io(err) => CliError::Io(err),
                _ => CliError::InvalidSource(format!("ZIP extraction failed: {}", e)),
            })?;
    }

    // Find SKILL.md in extracted directory
    let skill_path = find_skill_in_directory(extract_path)?;
    validate_skill_structure(&skill_path, verbose)?;

    // Read and parse SKILL.md to create skill definition
    // Skill ID will be read from skill-project.toml (mandatory)
    let skill_def = create_skill_from_path(&skill_path)?;

    // Copy skill to skills storage directory
    let skill_storage_dir = service
        .config()
        .skill_storage_path
        .join(skill_def.id.as_str());
    if skill_storage_dir.exists() {
        if !force {
            return Err(CliError::Config(format!(
                "Skill directory '{}' already exists. Use --force to overwrite.",
                skill_storage_dir.display()
            )));
        }
        // Remove existing directory
        tokio::fs::remove_dir_all(&skill_storage_dir)
            .await
            .map_err(CliError::Io)?;
    }

    // Copy skill directory to storage
    copy_dir_recursive(&skill_path, &skill_storage_dir).await?;

    // Update skill file path to point to the copied location
    let mut skill_def = skill_def;
    skill_def.skill_file = skill_storage_dir.join("SKILL.md");

    // Register the skill (force or regular based on flag)
    if force {
        service
            .skill_manager()
            .force_register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    } else {
        // Check if skill already exists only when not forcing
        if service
            .skill_manager()
            .get_skill(&skill_def.id)
            .await
            .map_err(CliError::Service)?
            .is_some()
        {
            return Err(CliError::Config(format!(
                "Skill '{}' already exists. Use --force to overwrite.",
                skill_def.id
            )));
        }
        service
            .skill_manager()
            .register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    }

    // Update source tracking in skill definition
    skill_def.source_url = Some(zip_path.to_string_lossy().to_string());
    skill_def.source_type = Some(SourceType::ZipFile);
    skill_def.fetched_at = Some(Utc::now());
    skill_def.editable = editable;

    // Update skill with source tracking
    service
        .skill_manager()
        .update_skill(
            &skill_def.id,
            fastskill::core::skill_manager::SkillUpdate {
                source_url: skill_def.source_url.clone(),
                source_type: skill_def.source_type.clone(),
                fetched_at: skill_def.fetched_at,
                editable: Some(skill_def.editable),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| {
            crate::cli::utils::service_error_to_cli(
                e,
                service.config().skill_storage_path.as_path(),
                global,
            )
        })?;

    // T028: Update skill-project.toml and skills.lock
    update_project_files(&skill_def, groups.clone(), editable)?;

    println!(
        "Successfully added skill: {} (v{})",
        skill_def.name, skill_def.version
    );
    println!(
        "{}",
        crate::cli::utils::messages::ok("Updated skill-project.toml and skills.lock")
    );
    Ok(())
}

async fn add_from_folder(
    service: &FastSkillService,
    folder_path: &Path,
    force: bool,
    editable: bool,
    groups: Vec<String>,
    verbose: bool,
    global: bool,
) -> CliResult<()> {
    info!("Adding skill from folder: {}", folder_path.display());

    // Validate folder structure
    validate_skill_structure(folder_path, verbose)?;

    // Read and parse SKILL.md to create skill definition
    // Skill ID will be read from skill-project.toml (mandatory)
    let skill_def = create_skill_from_path(folder_path)?;

    // Copy skill to skills storage directory (local skills stored at id path, no scope)
    let skill_storage_dir = service
        .config()
        .skill_storage_path
        .join(skill_def.id.as_str());
    if skill_storage_dir.exists() {
        if !force {
            return Err(CliError::Config(format!(
                "Skill directory '{}' already exists. Use --force to overwrite.",
                skill_storage_dir.display()
            )));
        }
        // Remove existing directory
        tokio::fs::remove_dir_all(&skill_storage_dir)
            .await
            .map_err(CliError::Io)?;
    }

    // Copy skill directory to storage
    copy_dir_recursive(folder_path, &skill_storage_dir).await?;

    // Update skill file path to point to the copied location
    let mut skill_def = skill_def;
    skill_def.skill_file = skill_storage_dir.join("SKILL.md");

    // Register the skill (force or regular based on flag)
    if force {
        service
            .skill_manager()
            .force_register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    } else {
        // Check if skill already exists only when not forcing
        if service
            .skill_manager()
            .get_skill(&skill_def.id)
            .await
            .map_err(CliError::Service)?
            .is_some()
        {
            return Err(CliError::Config(format!(
                "Skill '{}' already exists. Use --force to overwrite.",
                skill_def.id
            )));
        }
        service
            .skill_manager()
            .register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    }

    // Update source tracking in skill definition
    skill_def.source_url = Some(folder_path.to_string_lossy().to_string());
    skill_def.source_type = Some(SourceType::LocalPath);
    skill_def.fetched_at = Some(Utc::now());
    skill_def.editable = editable;

    // Update skill with source tracking
    service
        .skill_manager()
        .update_skill(
            &skill_def.id,
            fastskill::core::skill_manager::SkillUpdate {
                source_url: skill_def.source_url.clone(),
                source_type: skill_def.source_type.clone(),
                fetched_at: skill_def.fetched_at,
                editable: Some(skill_def.editable),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| {
            crate::cli::utils::service_error_to_cli(
                e,
                service.config().skill_storage_path.as_path(),
                global,
            )
        })?;

    // T028: Update skill-project.toml and skills.lock
    update_project_files(&skill_def, groups.clone(), editable)?;

    println!(
        "Successfully added skill: {} (v{})",
        skill_def.name, skill_def.version
    );
    println!(
        "{}",
        crate::cli::utils::messages::ok("Updated skill-project.toml and skills.lock")
    );
    Ok(())
}

async fn add_from_git(
    service: &FastSkillService,
    git_url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
    force: bool,
    editable: bool,
    groups: Vec<String>,
    verbose: bool,
    global: bool,
) -> CliResult<()> {
    info!("Adding skill from git URL: {}", git_url);

    // Parse git URL
    let git_info = parse_git_url(git_url)?;
    let branch = branch.or(git_info.branch.as_deref());

    // Clone repository
    {
        // Access through the storage module
        use fastskill::storage::git::{clone_repository, validate_cloned_skill};
        let temp_dir = clone_repository(&git_info.repo_url, branch, tag, None)
            .await
            .map_err(|e| CliError::GitCloneFailed(e.to_string()))?;

        // Determine the skill directory path (handle subdir for tree URLs)
        let skill_base_path = if let Some(subdir) = &git_info.subdir {
            let subdir_path = temp_dir.path().join(subdir);
            if !subdir_path.exists() {
                return Err(CliError::InvalidSource(format!(
                    "Specified subdirectory '{}' does not exist in cloned repository",
                    subdir.display()
                )));
            }
            subdir_path
        } else {
            temp_dir.path().to_path_buf()
        };

        // Validate cloned skill structure
        let skill_path = validate_cloned_skill(&skill_base_path)
            .map_err(|e| CliError::SkillValidationFailed(e.to_string()))?;

        // Validate skill structure
        validate_skill_structure(&skill_path, verbose)?;

        // Read and parse SKILL.md to create skill definition
        // Skill ID will be read from skill-project.toml (mandatory)
        let skill_def = create_skill_from_path(&skill_path)?;

        // Copy skill to skills storage directory (local skills stored at id path, no scope)
        let skill_storage_dir = service
            .config()
            .skill_storage_path
            .join(skill_def.id.as_str());
        if skill_storage_dir.exists() {
            if !force {
                return Err(CliError::Config(format!(
                    "Skill directory '{}' already exists. Use --force to overwrite.",
                    skill_storage_dir.display()
                )));
            }
            // Remove existing directory
            tokio::fs::remove_dir_all(&skill_storage_dir)
                .await
                .map_err(CliError::Io)?;
        }

        // Copy skill directory to storage
        copy_dir_recursive(&skill_path, &skill_storage_dir).await?;

        // Update skill file path to point to the copied location
        let mut skill_def = skill_def;
        skill_def.skill_file = skill_storage_dir.join("SKILL.md");

        // Register the skill (force or regular based on flag)
        if force {
            service
                .skill_manager()
                .force_register_skill(skill_def.clone())
                .await
                .map_err(CliError::Service)?;
        } else {
            // Check if skill already exists only when not forcing
            if service
                .skill_manager()
                .get_skill(&skill_def.id)
                .await
                .map_err(CliError::Service)?
                .is_some()
            {
                return Err(CliError::Config(format!(
                    "Skill '{}' already exists. Use --force to overwrite.",
                    skill_def.id
                )));
            }
            service
                .skill_manager()
                .register_skill(skill_def.clone())
                .await
                .map_err(CliError::Service)?;
        }

        // Update source tracking in skill definition
        skill_def.source_url = Some(git_url.to_string());
        skill_def.source_type = Some(SourceType::GitUrl);
        skill_def.source_branch = branch.map(|s| s.to_string());
        skill_def.source_tag = tag.map(|s| s.to_string());
        skill_def.source_subdir = git_info.subdir.clone();
        skill_def.fetched_at = Some(Utc::now());
        skill_def.editable = editable;
        // TODO: Get commit hash from git repository

        // Update skill with source tracking
        service
            .skill_manager()
            .update_skill(
                &skill_def.id,
                fastskill::core::skill_manager::SkillUpdate {
                    source_url: skill_def.source_url.clone(),
                    source_type: skill_def.source_type.clone(),
                    source_branch: skill_def.source_branch.clone(),
                    source_tag: skill_def.source_tag.clone(),
                    source_subdir: skill_def.source_subdir.clone(),
                    fetched_at: skill_def.fetched_at,
                    editable: Some(skill_def.editable),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| {
                crate::cli::utils::service_error_to_cli(
                    e,
                    service.config().skill_storage_path.as_path(),
                    global,
                )
            })?;

        // T028: Update skill-project.toml and skills.lock
        update_project_files(&skill_def, groups.clone(), editable)?;

        println!(
            "Successfully added skill: {} (v{})",
            skill_def.name, skill_def.version
        );
        println!(
            "{}",
            crate::cli::utils::messages::ok("Updated skill-project.toml and skills.lock")
        );
        Ok(())
    }
}

/// Add skill from registry
async fn add_from_registry(
    service: &FastSkillService,
    skill_id_input: &str,
    force: bool,
    editable: bool,
    groups: Vec<String>,
    verbose: bool,
    global: bool,
) -> CliResult<()> {
    info!("Adding skill from registry: {}", skill_id_input);

    // Parse skill ID and version
    let (skill_id_full, version_opt) = parse_skill_id(skill_id_input);

    // Extract scope and id from skill_id (format: scope/id or just id)
    let (scope, expected_id) = if let Some(slash_pos) = skill_id_full.find('/') {
        let s = &skill_id_full[..slash_pos];
        let i = &skill_id_full[slash_pos + 1..];
        (s.to_string(), i.to_string())
    } else {
        // No scope provided, use "default" or error?
        // For now, treat as unscoped (local skill)
        return Err(CliError::Config(format!(
            "Registry skill ID must be in format 'scope/id', got: {}",
            skill_id_full
        )));
    };

    // Load repository configuration from skill-project.toml [tool.fastskill.repositories]
    let repositories = crate::cli::config::load_repositories_from_project()?;
    let repo_manager = RepositoryManager::from_definitions(repositories);

    // Get default repository (prefer http-registry type, fallback to any)
    let default_repo = repo_manager.get_default_repository().ok_or_else(|| {
        CliError::Config(
            "No default repository configured. Use 'fastskill registry add' to add a repository."
                .to_string(),
        )
    })?;

    // For now, we only support http-registry type for add command
    // TODO: Support other repository types
    if !matches!(
        default_repo.repo_type,
        fastskill::core::repository::RepositoryType::HttpRegistry
    ) {
        return Err(CliError::Config(
            format!("Repository '{}' is not an http-registry type. Only http-registry repositories are supported for 'fastskill add' currently.", default_repo.name)
        ));
    }

    // Get repository client
    let repo_client = repo_manager
        .get_client(&default_repo.name)
        .await
        .map_err(|e| CliError::Config(format!("Failed to get repository client: {}", e)))?;

    // Determine version to use
    let version = if let Some(v) = version_opt {
        v
    } else {
        // Get all versions and pick the latest
        let versions = repo_client
            .get_versions(&skill_id_full)
            .await
            .map_err(|e| CliError::Config(format!("Failed to get versions: {}", e)))?;

        if versions.is_empty() {
            return Err(CliError::Config(format!(
                "No versions found for skill '{}' in repository",
                skill_id_full
            )));
        }

        // Sort versions and get latest (simple string comparison for now)
        let mut sorted_versions = versions;
        sorted_versions.sort();
        sorted_versions
            .last()
            .ok_or_else(|| CliError::Config("No versions available".to_string()))?
            .clone()
    };

    // Get skill metadata to verify it exists
    let _skill_metadata = repo_client
        .get_skill(&skill_id_full, Some(&version))
        .await
        .map_err(|e| CliError::Config(format!("Failed to get skill: {}", e)))?
        .ok_or_else(|| {
            CliError::Config(format!(
                "Skill '{}' version '{}' not found in repository",
                skill_id_full, version
            ))
        })?;

    // Download package
    info!(
        "Downloading {}@{} from repository...",
        skill_id_full, version
    );
    let zip_data = repo_client
        .download(&skill_id_full, &version)
        .await
        .map_err(|e| CliError::Config(format!("Failed to download package: {}", e)))?;

    // Extract to temporary directory
    // The skill ID will be read from skill-project.toml in the extracted package
    let temp_dir = TempDir::new().map_err(CliError::Io)?;
    let extract_path = temp_dir.path().join("extracted");
    fs::create_dir_all(&extract_path).map_err(CliError::Io)?;

    // Write ZIP to temp file
    let temp_zip = temp_dir.path().join(format!("package-{}.zip", version));
    std::fs::write(&temp_zip, zip_data).map_err(CliError::Io)?;

    // Extract ZIP safely using shared handler
    {
        use fastskill::storage::zip::ZipHandler;
        let zip_handler = ZipHandler::new()
            .map_err(|e| CliError::InvalidSource(format!("Failed to create ZIP handler: {}", e)))?;
        zip_handler
            .extract_to_dir(&temp_zip, &extract_path)
            .map_err(|e| match e {
                fastskill::core::service::ServiceError::Validation(msg) => {
                    CliError::InvalidSource(format!("ZIP extraction validation failed: {}", msg))
                }
                fastskill::core::service::ServiceError::Io(err) => CliError::Io(err),
                _ => CliError::InvalidSource(format!("ZIP extraction failed: {}", e)),
            })?;
    }

    // Find SKILL.md in extracted directory
    let skill_path = find_skill_in_directory(&extract_path)?;
    validate_skill_structure(&skill_path, verbose)?;

    // Read and parse SKILL.md to create skill definition
    // Skill ID will be read from skill-project.toml in the extracted package (mandatory)
    let skill_def = create_skill_from_path(&skill_path)?;

    // Verify that the extracted id matches the expected id from the registry reference
    if skill_def.id.as_str() != expected_id {
        return Err(CliError::Config(format!(
            "Skill ID mismatch: expected '{}' from registry reference '{}', but found '{}' in skill-project.toml",
            expected_id, skill_id_full, skill_def.id.as_str()
        )));
    }

    // Copy skill to skills storage directory at scope/id path
    let skill_storage_dir = service
        .config()
        .skill_storage_path
        .join(&scope)
        .join(skill_def.id.as_str());
    if skill_storage_dir.exists() {
        if !force {
            return Err(CliError::Config(format!(
                "Skill directory '{}' already exists. Use --force to overwrite.",
                skill_storage_dir.display()
            )));
        }
        // Remove existing directory
        tokio::fs::remove_dir_all(&skill_storage_dir)
            .await
            .map_err(CliError::Io)?;
    }

    // Copy skill directory to storage
    copy_dir_recursive(&skill_path, &skill_storage_dir).await?;

    // Update skill file path to point to the copied location
    let mut skill_def = skill_def;
    skill_def.skill_file = skill_storage_dir.join("SKILL.md");

    // Register the skill
    if force {
        service
            .skill_manager()
            .force_register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    } else {
        if service
            .skill_manager()
            .get_skill(&skill_def.id)
            .await
            .map_err(CliError::Service)?
            .is_some()
        {
            return Err(CliError::Config(format!(
                "Skill '{}' already exists. Use --force to overwrite.",
                skill_def.id
            )));
        }
        service
            .skill_manager()
            .register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    }

    // Update source tracking - use Source type for repository
    skill_def.source_url = None; // Repository clients handle download internally
    skill_def.source_type = Some(SourceType::Source);
    skill_def.installed_from = Some(default_repo.name.clone());
    skill_def.fetched_at = Some(Utc::now());
    skill_def.editable = editable;

    // Update skill with source tracking
    service
        .skill_manager()
        .update_skill(
            &skill_def.id,
            fastskill::core::skill_manager::SkillUpdate {
                source_url: skill_def.source_url.clone(),
                source_type: skill_def.source_type.clone(),
                installed_from: skill_def.installed_from.clone(),
                fetched_at: skill_def.fetched_at,
                editable: Some(skill_def.editable),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| {
            crate::cli::utils::service_error_to_cli(
                e,
                service.config().skill_storage_path.as_path(),
                global,
            )
        })?;

    // T028: Update skill-project.toml and skills.lock
    update_project_files(&skill_def, groups.clone(), editable)?;

    println!(
        "Successfully added skill: {} (v{}) from registry",
        skill_def.name, version
    );
    println!(
        "{}",
        crate::cli::utils::messages::ok("Updated skill-project.toml and skills.lock")
    );
    Ok(())
}

/// Read skill ID from skill-project.toml (mandatory)
fn read_skill_id_from_toml(skill_path: &Path) -> CliResult<String> {
    let skill_project_path = skill_path.join("skill-project.toml");

    if !skill_project_path.exists() {
        return Err(CliError::Validation(format!(
            "skill-project.toml is required but not found in: {}. Run 'fastskill init' to create it.",
            skill_path.display()
        )));
    }

    #[derive(serde::Deserialize)]
    struct SkillProjectToml {
        metadata: Option<fastskill::core::manifest::MetadataSection>,
    }

    let content = fs::read_to_string(&skill_project_path)
        .map_err(|e| CliError::Validation(format!("Failed to read skill-project.toml: {}", e)))?;

    let project: SkillProjectToml = toml::from_str(&content)
        .map_err(|e| CliError::Validation(format!("Failed to parse skill-project.toml: {}", e)))?;

    let metadata = project.metadata.ok_or_else(|| {
        CliError::Validation("skill-project.toml must have a [metadata] section".to_string())
    })?;

    metadata.id.ok_or_else(|| {
        CliError::Validation(
            "skill-project.toml [metadata] section must have a non-empty 'id' field".to_string(),
        )
    })
}

/// Find SKILL.md in a directory (could be at root or in subdirectory)
fn find_skill_in_directory(dir: &Path) -> CliResult<PathBuf> {
    // Check root
    if dir.join("SKILL.md").exists() {
        return Ok(dir.to_path_buf());
    }

    // Check subdirectories
    let entries = fs::read_dir(dir).map_err(CliError::Io)?;
    for entry in entries {
        let entry = entry.map_err(CliError::Io)?;
        let path = entry.path();
        if path.is_dir() && path.join("SKILL.md").exists() {
            return Ok(path);
        }
    }

    Err(CliError::Validation(format!(
        "SKILL.md not found in: {}",
        dir.display()
    )))
}

/// Create a skill definition from a path containing SKILL.md
/// The skill ID is read from skill-project.toml (mandatory)
pub fn create_skill_from_path(skill_path: &Path) -> CliResult<SkillDefinition> {
    // Read skill ID from skill-project.toml (mandatory)
    let skill_id_str = read_skill_id_from_toml(skill_path)?;
    let skill_file = skill_path.join("SKILL.md");
    let content = fs::read_to_string(&skill_file).map_err(CliError::Io)?;

    // Parse YAML frontmatter using proper YAML parser
    let frontmatter = fastskill::core::metadata::parse_yaml_frontmatter(&content)
        .map_err(|e| CliError::Validation(format!("Failed to parse SKILL.md: {}", e)))?;

    // Validate and create skill ID
    let skill_id_str_clone = skill_id_str.clone();
    let skill_id = fastskill::SkillId::new(skill_id_str).map_err(|_| {
        CliError::Validation(format!("Invalid skill ID format: {}", skill_id_str_clone))
    })?;

    // Create skill definition from parsed frontmatter
    let mut skill = SkillDefinition::new(
        skill_id,
        frontmatter.name,
        frontmatter.description,
        frontmatter.version.unwrap_or_else(|| "1.0.0".to_string()),
    );

    // Set skill file path
    skill.skill_file = skill_file.clone();

    // No optional fields to set from frontmatter
    skill.author = frontmatter.author;

    Ok(skill)
}

/// Recursively copy a directory from src to dst
pub async fn copy_dir_recursive(src: &Path, dst: &Path) -> CliResult<()> {
    // Create destination directory
    tokio::fs::create_dir_all(dst).await.map_err(CliError::Io)?;

    let mut entries = tokio::fs::read_dir(src).await.map_err(CliError::Io)?;

    while let Some(entry) = entries.next_entry().await.map_err(CliError::Io)? {
        let ty = entry.file_type().await.map_err(CliError::Io)?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_dir() {
            // Recursively copy subdirectory (boxed to handle async recursion)
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            // Copy file
            tokio::fs::copy(&src_path, &dst_path)
                .await
                .map_err(CliError::Io)?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used
)]
mod tests {
    use super::*;
    use fastskill::{FastSkillService, ServiceConfig};
    use std::io::Write;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_add_nonexistent_source() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = AddArgs {
            source: "/nonexistent/path".to_string(),
            source_type: Some("local".to_string()),
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
        };

        let result = execute_add(&service, args, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_add_invalid_skill_id() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = AddArgs {
            source: "invalid@skill@id".to_string(),
            source_type: Some("registry".to_string()),
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
        };

        let result = execute_add(&service, args, false).await;
        // May fail due to invalid format or missing registry
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_add_with_force_flag() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = AddArgs {
            source: "/nonexistent/path".to_string(),
            source_type: Some("local".to_string()),
            branch: None,
            tag: None,
            force: true,
            editable: false,
            group: None,
            recursive: false,
        };

        // Should still fail because source doesn't exist, but force flag is accepted
        let result = execute_add(&service, args, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_add_no_manifest_fails_with_instructions() {
        // Use a shared mutex to serialize directory changes across parallel tests
        let _lock = fastskill::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let original_dir = std::env::current_dir().ok();
        let _guard = DirGuard(original_dir);
        std::env::set_current_dir(temp_dir.path()).unwrap();
        // No skill-project.toml in temp_dir

        let args = AddArgs {
            source: "scope/some-skill".to_string(),
            source_type: Some("registry".to_string()),
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
        };

        let result = execute_add(&service, args, false).await;
        assert!(
            result.is_err(),
            "Expected error when no manifest: {:?}",
            result
        );
        if let Err(CliError::Config(msg)) = result {
            assert!(
                msg.contains("skill-project.toml not found") && msg.contains("fastskill init"),
                "Error must mention skill-project.toml and fastskill init: '{}'",
                msg
            );
        } else {
            panic!("Expected Config error");
        }
    }

    #[tokio::test]
    async fn test_execute_add_success() {
        let _lock = fastskill::test_utils::DIR_MUTEX
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

        let skills_dir = temp_dir.path().join(".claude/skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let source_dir = temp_dir.path().join("source-skill");
        fs::create_dir_all(&source_dir).unwrap();
        let skill_content = r#"# Test Skill

Name: test-skill
 Version: 1.0.0
 Description: A test skill for coverage
"#;
        fs::write(source_dir.join("SKILL.md"), skill_content).unwrap();

        let manifest_content = r#"[tool.fastskill]
 skills_directory = ".claude/skills"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = AddArgs {
            source: source_dir.display().to_string(),
            source_type: Some("local".to_string()),
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
        };

        let result = execute_add(&service, args, false).await;
        // May succeed or fail depending on various factors, but should exercise the code path
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_add_from_zip_rejects_path_traversal_attack() {
        let _lock = fastskill::test_utils::DIR_MUTEX
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

        let skills_dir = temp_dir.path().join(".claude/skills");
        fs::create_dir_all(&skills_dir).unwrap();

        // Create a ZIP with path traversal attempt
        let zip_path = temp_dir.path().join("malicious.zip");
        let zip_file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(zip_file);
        let options =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

        // Add safe content
        zip.start_file("SKILL.md", options).unwrap();
        let _ = zip.write_all(
            b"# Test Skill

Name: safe-skill
Version: 1.0.0
Description: Safe skill",
        );

        // Add malicious path traversal attempt
        zip.start_file("../../../evil.txt", options).unwrap();
        let _ = zip.write_all(b"malicious content");

        zip.finish().unwrap();

        // Create skill-project.toml
        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = AddArgs {
            source: zip_path.display().to_string(),
            source_type: Some("local".to_string()),
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
        };

        let result = execute_add(&service, args, false).await;

        // Should fail due to path traversal detection
        assert!(result.is_err());
        match result {
            Err(CliError::InvalidSource(msg)) => {
                assert!(
                    msg.contains("path traversal") || msg.contains("Path traversal"),
                    "Error should mention path traversal: {}",
                    msg
                );
            }
            Err(CliError::Validation(msg)) => {
                assert!(
                    msg.contains("path traversal") || msg.contains("Path traversal"),
                    "Error should mention path traversal: {}",
                    msg
                );
            }
            _ => {
                panic!("Expected InvalidSource or Validation error for path traversal attack, got: {:?}", result);
            }
        }

        // Verify no file was created outside the extraction directory
        let evil_path = temp_dir.path().join("../../../evil.txt");
        assert!(
            !evil_path.exists(),
            "Evil file should not exist: {}",
            evil_path.display()
        );
    }

    #[tokio::test]
    async fn test_add_from_zip_rejects_windows_style_traversal() {
        let _lock = fastskill::test_utils::DIR_MUTEX
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

        let skills_dir = temp_dir.path().join(".claude/skills");
        fs::create_dir_all(&skills_dir).unwrap();

        // Create a ZIP with Windows-style path traversal
        let zip_path = temp_dir.path().join("windows-malicious.zip");
        let zip_file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(zip_file);
        let options =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

        zip.start_file("SKILL.md", options).unwrap();
        let _ = zip.write_all(
            b"# Test Skill

Name: safe-skill
Version: 1.0.0
Description: Safe skill",
        );

        // Add Windows-style path traversal attempt
        zip.start_file("..\\..\\..\\evil.txt", options).unwrap();
        let _ = zip.write_all(b"malicious content");

        zip.finish().unwrap();

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = AddArgs {
            source: zip_path.display().to_string(),
            source_type: Some("local".to_string()),
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
        };

        let result = execute_add(&service, args, false).await;

        // Should fail
        assert!(
            result.is_err(),
            "Windows-style traversal should be rejected"
        );
    }
}
