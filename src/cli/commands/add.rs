//! Add command implementation

use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::manifest_utils;
use crate::cli::utils::{
    detect_skill_source, parse_git_url, parse_skill_id, validate_skill_structure, SkillSource,
};
use chrono::Utc;
use clap::Args;
use fastskill::core::repository::RepositoryManager;
use fastskill::core::skill_manager::SourceType;
use fastskill::{FastSkillService, SkillDefinition};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tracing::info;

/// Add a new skill from zip file, folder, git URL, or registry (skill ID)
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
}

pub async fn execute_add(service: &FastSkillService, args: AddArgs) -> CliResult<()> {
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

    match source {
        SkillSource::ZipFile(path) => {
            add_from_zip(service, &path, args.force, args.editable, groups).await
        }
        SkillSource::Folder(path) => {
            add_from_folder(service, &path, args.force, args.editable, groups).await
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
            )
            .await
        }
        SkillSource::SkillId(skill_id) => {
            add_from_registry(service, &skill_id, args.force, args.editable, groups).await
        }
    }
}

async fn add_from_zip(
    service: &FastSkillService,
    zip_path: &Path,
    force: bool,
    editable: bool,
    groups: Vec<String>,
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

    // Extract zip file
    let file = fs::File::open(zip_path).map_err(CliError::Io)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| CliError::InvalidSource(format!("Invalid zip file: {}", e)))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| CliError::InvalidSource(format!("Failed to read zip entry: {}", e)))?;
        let outpath = extract_path.join(file.name());

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath).map_err(CliError::Io)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p).map_err(CliError::Io)?;
                }
            }
            let mut outfile = fs::File::create(&outpath).map_err(CliError::Io)?;
            std::io::copy(&mut file, &mut outfile).map_err(CliError::Io)?;
        }
    }

    // Find SKILL.md in extracted directory
    let skill_path = find_skill_in_directory(extract_path)?;
    validate_skill_structure(&skill_path)?;

    // Read and parse SKILL.md to create skill definition
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
        .map_err(CliError::Service)?;

    // Update manifest and lock files
    let manifest_path = PathBuf::from(".claude/skills.toml");
    let lock_path = PathBuf::from(".claude/skills.lock");
    manifest_utils::add_skill_to_manifest(&manifest_path, &skill_def, groups.clone(), editable)
        .map_err(|e| CliError::Config(format!("Failed to update manifest: {}", e)))?;
    manifest_utils::update_lock_file(&lock_path, &skill_def, groups)
        .map_err(|e| CliError::Config(format!("Failed to update lock file: {}", e)))?;

    println!(
        "Successfully added skill: {} (v{})",
        skill_def.name, skill_def.version
    );
    println!(
        "{}",
        crate::cli::utils::messages::ok("Updated skills.toml and skills.lock")
    );
    Ok(())
}

async fn add_from_folder(
    service: &FastSkillService,
    folder_path: &Path,
    force: bool,
    editable: bool,
    groups: Vec<String>,
) -> CliResult<()> {
    info!("Adding skill from folder: {}", folder_path.display());

    // Validate folder structure
    validate_skill_structure(folder_path)?;

    // Read and parse SKILL.md to create skill definition
    let skill_def = create_skill_from_path(folder_path)?;

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
        .map_err(CliError::Service)?;

    // Update manifest and lock files
    let manifest_path = PathBuf::from(".claude/skills.toml");
    let lock_path = PathBuf::from(".claude/skills.lock");
    manifest_utils::add_skill_to_manifest(&manifest_path, &skill_def, groups.clone(), editable)
        .map_err(|e| CliError::Config(format!("Failed to update manifest: {}", e)))?;
    manifest_utils::update_lock_file(&lock_path, &skill_def, groups)
        .map_err(|e| CliError::Config(format!("Failed to update lock file: {}", e)))?;

    println!(
        "Successfully added skill: {} (v{})",
        skill_def.name, skill_def.version
    );
    println!(
        "{}",
        crate::cli::utils::messages::ok("Updated skills.toml and skills.lock")
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
) -> CliResult<()> {
    info!("Adding skill from git URL: {}", git_url);

    // Parse git URL
    let git_info = parse_git_url(git_url)?;
    let branch = branch.or(git_info.branch.as_deref());

    // Clone repository
    #[cfg(feature = "git-support")]
    {
        // Note: git module is only available when feature is enabled
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
        validate_skill_structure(&skill_path)?;

        // Read and parse SKILL.md to create skill definition
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
            .map_err(CliError::Service)?;

        // Update manifest and lock files
        let manifest_path = PathBuf::from(".claude/skills.toml");
        let lock_path = PathBuf::from(".claude/skills.lock");
        manifest_utils::add_skill_to_manifest(&manifest_path, &skill_def, groups.clone(), editable)
            .map_err(|e| CliError::Config(format!("Failed to update manifest: {}", e)))?;
        manifest_utils::update_lock_file(&lock_path, &skill_def, groups)
            .map_err(|e| CliError::Config(format!("Failed to update lock file: {}", e)))?;

        println!(
            "Successfully added skill: {} (v{})",
            skill_def.name, skill_def.version
        );
        println!(
            "{}",
            crate::cli::utils::messages::ok("Updated skills.toml and skills.lock")
        );
        Ok(())
    }

    #[cfg(not(feature = "git-support"))]
    {
        Err(CliError::Config(
            "Git support is not enabled. Build with --features git-support".to_string(),
        ))
    }
}

/// Add skill from registry
async fn add_from_registry(
    service: &FastSkillService,
    skill_id_input: &str,
    force: bool,
    editable: bool,
    groups: Vec<String>,
) -> CliResult<()> {
    info!("Adding skill from registry: {}", skill_id_input);

    // Parse skill ID and version
    let (skill_id, version_opt) = parse_skill_id(skill_id_input);

    // Load repository configuration from repositories.toml
    let repos_path = PathBuf::from(".claude/repositories.toml");
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

    // Get default repository (prefer git-registry type, fallback to any)
    let default_repo = repo_manager.get_default_repository().ok_or_else(|| {
        CliError::Config(
            "No default repository configured. Use 'fastskill registry add' to add a repository."
                .to_string(),
        )
    })?;

    // For now, we only support git-registry type for add command
    // TODO: Support other repository types
    if !matches!(
        default_repo.repo_type,
        fastskill::core::repository::RepositoryType::GitRegistry
    ) {
        return Err(CliError::Config(
            format!("Repository '{}' is not a git-registry type. Only git-registry repositories are supported for 'fastskill add' currently.", default_repo.name)
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
            .get_versions(&skill_id)
            .await
            .map_err(|e| CliError::Config(format!("Failed to get versions: {}", e)))?;

        if versions.is_empty() {
            return Err(CliError::Config(format!(
                "No versions found for skill '{}' in repository",
                skill_id
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
        .get_skill(&skill_id, Some(&version))
        .await
        .map_err(|e| CliError::Config(format!("Failed to get skill: {}", e)))?
        .ok_or_else(|| {
            CliError::Config(format!(
                "Skill '{}' version '{}' not found in repository",
                skill_id, version
            ))
        })?;

    // Download package
    info!("Downloading {}@{} from repository...", skill_id, version);
    let zip_data = repo_client
        .download(&skill_id, &version)
        .await
        .map_err(|e| CliError::Config(format!("Failed to download package: {}", e)))?;

    // Extract to temporary directory
    let temp_dir = TempDir::new().map_err(CliError::Io)?;
    let extract_path = temp_dir.path();

    // Write ZIP to temp file
    let temp_zip = extract_path.join(format!("{}-{}.zip", skill_id, version));
    std::fs::write(&temp_zip, zip_data).map_err(CliError::Io)?;

    // Extract ZIP
    let file = fs::File::open(&temp_zip).map_err(CliError::Io)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| CliError::InvalidSource(format!("Invalid zip file: {}", e)))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| CliError::InvalidSource(format!("Failed to read zip entry: {}", e)))?;
        let outpath = extract_path.join(file.name());

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath).map_err(CliError::Io)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p).map_err(CliError::Io)?;
                }
            }
            let mut outfile = fs::File::create(&outpath).map_err(CliError::Io)?;
            std::io::copy(&mut file, &mut outfile).map_err(CliError::Io)?;
        }
    }

    // Find SKILL.md in extracted directory
    let skill_path = find_skill_in_directory(extract_path)?;
    validate_skill_structure(&skill_path)?;

    // Read and parse SKILL.md to create skill definition
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
        .map_err(CliError::Service)?;

    // Update manifest and lock files
    let manifest_path = PathBuf::from(".claude/skills.toml");
    let lock_path = PathBuf::from(".claude/skills.lock");

    manifest_utils::add_skill_to_manifest(&manifest_path, &skill_def, groups.clone(), editable)
        .map_err(|e| CliError::Config(format!("Failed to update manifest: {}", e)))?;
    manifest_utils::update_lock_file(&lock_path, &skill_def, groups)
        .map_err(|e| CliError::Config(format!("Failed to update lock file: {}", e)))?;

    println!(
        "Successfully added skill: {} (v{}) from registry",
        skill_def.name, version
    );
    println!(
        "{}",
        crate::cli::utils::messages::ok("Updated skills.toml and skills.lock")
    );
    Ok(())
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
pub fn create_skill_from_path(skill_path: &Path) -> CliResult<SkillDefinition> {
    let skill_file = skill_path.join("SKILL.md");
    let content = fs::read_to_string(&skill_file).map_err(CliError::Io)?;

    // Parse YAML frontmatter using proper YAML parser
    let frontmatter = fastskill::core::metadata::parse_yaml_frontmatter(&content)
        .map_err(|e| CliError::Validation(format!("Failed to parse SKILL.md: {}", e)))?;

    // Use directory name as skill ID
    let skill_id_str = skill_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| CliError::Validation("Invalid skill directory name".to_string()))?;
    let skill_id = fastskill::SkillId::new(skill_id_str.to_string())
        .map_err(|_| CliError::Validation(format!("Invalid skill ID format: {}", skill_id_str)))?;

    // Create skill definition from parsed frontmatter
    let mut skill = SkillDefinition::new(
        skill_id,
        frontmatter.name,
        frontmatter.description,
        frontmatter.version,
    );

    // Set skill file path
    skill.skill_file = skill_file.clone();

    // Set optional fields from frontmatter
    skill.tags = frontmatter.tags;
    skill.capabilities = frontmatter.capabilities;
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
mod tests {
    use super::*;
    use fastskill::{FastSkillService, ServiceConfig};
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
        };

        let result = execute_add(&service, args).await;
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
        };

        let result = execute_add(&service, args).await;
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
        };

        // Should still fail because source doesn't exist, but force flag is accepted
        let result = execute_add(&service, args).await;
        assert!(result.is_err());
    }
}
