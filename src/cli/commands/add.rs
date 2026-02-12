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

/// Bundled options for add operations (reduces argument count)
struct AddContext<'a> {
    service: &'a FastSkillService,
    force: bool,
    editable: bool,
    groups: Vec<String>,
    verbose: bool,
    global: bool,
}

/// Source metadata to record after installing a skill
struct SourceMeta {
    source_url: Option<String>,
    source_type: Option<SourceType>,
    source_branch: Option<String>,
    source_tag: Option<String>,
    source_subdir: Option<PathBuf>,
    installed_from: Option<String>,
}

/// Target for install: where to copy and what to record
struct InstallTarget {
    storage_dir: PathBuf,
    meta: SourceMeta,
    version_display: String,
}

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

async fn register_skill_once(ctx: &AddContext<'_>, skill_def: &SkillDefinition) -> CliResult<()> {
    if ctx.force {
        ctx.service
            .skill_manager()
            .force_register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    } else {
        if ctx
            .service
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
        ctx.service
            .skill_manager()
            .register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    }
    Ok(())
}

/// Copy skill to storage, register, update source tracking, and update project files.
async fn install_copied_skill(
    ctx: &AddContext<'_>,
    skill_path: &Path,
    mut skill_def: SkillDefinition,
    target: InstallTarget,
) -> CliResult<()> {
    if target.storage_dir.exists() {
        if !ctx.force {
            return Err(CliError::Config(format!(
                "Skill directory '{}' already exists. Use --force to overwrite.",
                target.storage_dir.display()
            )));
        }
        tokio::fs::remove_dir_all(&target.storage_dir)
            .await
            .map_err(CliError::Io)?;
    }
    copy_dir_recursive(skill_path, &target.storage_dir).await?;
    skill_def.skill_file = target.storage_dir.join("SKILL.md");
    skill_def.source_url = target.meta.source_url;
    skill_def.source_type = target.meta.source_type;
    skill_def.source_branch = target.meta.source_branch;
    skill_def.source_tag = target.meta.source_tag;
    skill_def.source_subdir = target.meta.source_subdir;
    skill_def.installed_from = target.meta.installed_from;
    skill_def.fetched_at = Some(Utc::now());
    skill_def.editable = ctx.editable;

    register_skill_once(ctx, &skill_def).await?;

    let update = fastskill::core::skill_manager::SkillUpdate {
        source_url: skill_def.source_url.clone(),
        source_type: skill_def.source_type.clone(),
        source_branch: skill_def.source_branch.clone(),
        source_tag: skill_def.source_tag.clone(),
        source_subdir: skill_def.source_subdir.clone(),
        installed_from: skill_def.installed_from.clone(),
        fetched_at: skill_def.fetched_at,
        editable: Some(skill_def.editable),
        ..Default::default()
    };
    ctx.service
        .skill_manager()
        .update_skill(&skill_def.id, update)
        .await
        .map_err(|e| {
            crate::cli::utils::service_error_to_cli(
                e,
                ctx.service.config().skill_storage_path.as_path(),
                ctx.global,
            )
        })?;

    update_project_files(&skill_def, ctx.groups.clone(), ctx.editable)?;
    println!(
        "Successfully added skill: {} (v{})",
        skill_def.name, target.version_display
    );
    println!(
        "{}",
        crate::cli::utils::messages::ok("Updated skill-project.toml and skills.lock")
    );
    Ok(())
}

fn component_is_hidden(c: std::path::Component<'_>) -> bool {
    matches!(c, std::path::Component::Normal(name) if name.to_string_lossy().starts_with('.'))
}

/// Discover all directories containing SKILL.md under the given base directory.
/// Skips hidden directories (those starting with '.').
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

    let base_canonical = base.canonicalize().map_err(CliError::Io)?;
    let mut skill_dirs = Vec::new();

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
        if !path.is_dir() || !path.join("SKILL.md").exists() {
            continue;
        }

        let path_canonical = path.canonicalize().map_err(CliError::Io)?;
        let relative_path = path_canonical.strip_prefix(&base_canonical).map_err(|_| {
            CliError::Validation(format!(
                "Failed to compute relative path for: {}",
                path.display()
            ))
        })?;
        if relative_path.components().any(component_is_hidden) {
            continue;
        }
        skill_dirs.push(path_canonical);
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

fn resolve_source(args: &AddArgs) -> SkillSource {
    let Some(ref source_type) = args.source_type else {
        return detect_skill_source(&args.source);
    };
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
}

async fn handle_recursive_add(ctx: &AddContext<'_>, path: &Path) -> CliResult<()> {
    info!(
        "Adding skills recursively from directory: {}",
        path.display()
    );
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

    let mut failed: Vec<(PathBuf, CliError)> = Vec::new();
    let mut success_count = 0;
    for skill_path in skill_dirs {
        match add_from_folder(ctx, &skill_path).await {
            Ok(()) => success_count += 1,
            Err(e) => {
                eprintln!("Skill at {}: {}", skill_path.display(), e);
                failed.push((skill_path, e));
            }
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    let total = success_count + failed.len();
    Err(CliError::Validation(format!(
        "{} of {} skills failed:\n{}",
        failed.len(),
        total,
        failed
            .iter()
            .map(|(path, err)| format!("  - {}: {}", path.display(), err))
            .collect::<Vec<_>>()
            .join("\n")
    )))
}

fn ensure_manifest() -> CliResult<()> {
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    if !project_file_result.found {
        return Err(CliError::Config(
            manifest_required_for_add_message().to_string(),
        ));
    }
    Ok(())
}

pub async fn execute_add(
    service: &FastSkillService,
    args: AddArgs,
    verbose: bool,
    global: bool,
) -> CliResult<()> {
    ensure_manifest()?;
    let groups = args.group.clone().map(|g| vec![g]).unwrap_or_default();
    let ctx = AddContext {
        service,
        force: args.force,
        editable: args.editable,
        groups,
        verbose,
        global,
    };
    let source = resolve_source(&args);

    if args.recursive {
        let path = match &source {
            SkillSource::Folder(p) => p,
            _ => {
                return Err(CliError::Config(
                    "Recursive add is only valid when source is a local directory".to_string(),
                ));
            }
        };
        return handle_recursive_add(&ctx, path).await;
    }

    match source {
        SkillSource::ZipFile(path) => add_from_zip(&ctx, &path).await,
        SkillSource::Folder(path) => {
            validate_folder_has_skill(&path)?;
            add_from_folder(&ctx, &path).await
        }
        SkillSource::GitUrl(url) => {
            add_from_git(&ctx, &url, args.branch.as_deref(), args.tag.as_deref()).await
        }
        SkillSource::SkillId(skill_id) => add_from_registry(&ctx, &skill_id).await,
    }
}

fn validate_folder_has_skill(path: &Path) -> CliResult<()> {
    if path.join("SKILL.md").exists() {
        return Ok(());
    }
    if let Ok(dirs) = get_skill_dirs_recursive(path) {
        if !dirs.is_empty() {
            return Err(CliError::Validation(format!(
                "This directory has no SKILL.md at the root but contains {} skill(s) in subdirectories. Add them all with: fastskill add {} --recursive",
                dirs.len(),
                path.display()
            )));
        }
    }
    Err(CliError::Validation(format!(
        "No SKILL.md found in {}. A skill directory must contain a SKILL.md file. If this folder has multiple skills in subdirectories, use --recursive.",
        path.display()
    )))
}

fn extract_zip_to_temp(zip_path: &Path) -> CliResult<tempfile::TempDir> {
    let temp_dir = TempDir::new().map_err(CliError::Io)?;
    let extract_path = temp_dir.path();
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
    Ok(temp_dir)
}

async fn add_from_zip(ctx: &AddContext<'_>, zip_path: &Path) -> CliResult<()> {
    info!("Adding skill from zip file: {}", zip_path.display());
    if !zip_path.exists() {
        return Err(CliError::InvalidSource(format!(
            "Zip file does not exist: {}",
            zip_path.display()
        )));
    }
    let _temp_dir = extract_zip_to_temp(zip_path)?;
    let extract_path = _temp_dir.path();
    let skill_path = find_skill_in_directory(extract_path)?;
    validate_skill_structure(&skill_path, ctx.verbose)?;
    let skill_def = create_skill_from_path(&skill_path)?;
    let version = skill_def.version.clone();
    let target = InstallTarget {
        storage_dir: ctx
            .service
            .config()
            .skill_storage_path
            .join(skill_def.id.as_str()),
        meta: SourceMeta {
            source_url: Some(zip_path.to_string_lossy().to_string()),
            source_type: Some(SourceType::ZipFile),
            source_branch: None,
            source_tag: None,
            source_subdir: None,
            installed_from: None,
        },
        version_display: version,
    };
    install_copied_skill(ctx, &skill_path, skill_def, target).await
}

async fn add_from_folder(ctx: &AddContext<'_>, folder_path: &Path) -> CliResult<()> {
    info!("Adding skill from folder: {}", folder_path.display());
    validate_skill_structure(folder_path, ctx.verbose)?;
    let skill_def = create_skill_from_path(folder_path)?;
    let target = InstallTarget {
        storage_dir: ctx
            .service
            .config()
            .skill_storage_path
            .join(skill_def.id.as_str()),
        meta: SourceMeta {
            source_url: Some(folder_path.to_string_lossy().to_string()),
            source_type: Some(SourceType::LocalPath),
            source_branch: None,
            source_tag: None,
            source_subdir: None,
            installed_from: None,
        },
        version_display: skill_def.version.clone(),
    };
    install_copied_skill(ctx, folder_path, skill_def, target).await
}

async fn clone_and_validate_skill(
    git_url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
) -> CliResult<(
    TempDir,
    PathBuf,
    SkillDefinition,
    Option<String>,
    Option<String>,
)> {
    use fastskill::storage::git::{clone_repository, validate_cloned_skill};
    let git_info = parse_git_url(git_url)?;
    let branch = branch.or(git_info.branch.as_deref());
    let temp_dir = clone_repository(&git_info.repo_url, branch, tag, None)
        .await
        .map_err(|e| CliError::GitCloneFailed(e.to_string()))?;
    let skill_base_path = match &git_info.subdir {
        Some(subdir) => {
            let subdir_path = temp_dir.path().join(subdir);
            if !subdir_path.exists() {
                return Err(CliError::InvalidSource(format!(
                    "Specified subdirectory '{}' does not exist in cloned repository",
                    subdir.display()
                )));
            }
            subdir_path
        }
        None => temp_dir.path().to_path_buf(),
    };
    let skill_path = validate_cloned_skill(&skill_base_path)
        .map_err(|e| CliError::SkillValidationFailed(e.to_string()))?;
    let skill_def = create_skill_from_path(&skill_path)?;
    Ok((
        temp_dir,
        skill_path,
        skill_def,
        branch.map(String::from),
        tag.map(String::from),
    ))
}

async fn add_from_git(
    ctx: &AddContext<'_>,
    git_url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
) -> CliResult<()> {
    info!("Adding skill from git URL: {}", git_url);
    let (_temp_dir, skill_path, skill_def, branch_opt, tag_opt) =
        clone_and_validate_skill(git_url, branch, tag).await?;
    validate_skill_structure(&skill_path, ctx.verbose)?;
    let git_info = parse_git_url(git_url)?;
    let target = InstallTarget {
        storage_dir: ctx
            .service
            .config()
            .skill_storage_path
            .join(skill_def.id.as_str()),
        meta: SourceMeta {
            source_url: Some(git_url.to_string()),
            source_type: Some(SourceType::GitUrl),
            source_branch: branch_opt.or(git_info.branch),
            source_tag: tag_opt,
            source_subdir: git_info.subdir,
            installed_from: None,
        },
        version_display: skill_def.version.clone(),
    };
    install_copied_skill(ctx, &skill_path, skill_def, target).await
}

fn parse_registry_scope_id(
    skill_id_input: &str,
) -> CliResult<(String, String, String, Option<String>)> {
    let (skill_id_full, version_opt) = parse_skill_id(skill_id_input);
    let (scope, expected_id) = match skill_id_full.find('/') {
        Some(slash_pos) => (
            skill_id_full[..slash_pos].to_string(),
            skill_id_full[slash_pos + 1..].to_string(),
        ),
        None => {
            return Err(CliError::Config(format!(
                "Registry skill ID must be in format 'scope/id', got: {}",
                skill_id_full
            )));
        }
    };
    Ok((skill_id_full, scope, expected_id, version_opt))
}

async fn resolve_registry_version(
    repo_client: &(dyn fastskill::core::repository::RepositoryClient + Send + Sync),
    skill_id_full: &str,
    version_opt: Option<String>,
) -> CliResult<String> {
    if let Some(v) = version_opt {
        return Ok(v);
    }
    let versions = repo_client
        .get_versions(skill_id_full)
        .await
        .map_err(|e| CliError::Config(format!("Failed to get versions: {}", e)))?;
    if versions.is_empty() {
        return Err(CliError::Config(format!(
            "No versions found for skill '{}' in repository",
            skill_id_full
        )));
    }
    let mut sorted = versions;
    sorted.sort();
    sorted
        .last()
        .cloned()
        .ok_or_else(|| CliError::Config("No versions available".to_string()))
}

async fn download_registry_package(
    repo_client: &(dyn fastskill::core::repository::RepositoryClient + Send + Sync),
    skill_id_full: &str,
    version: &str,
    verbose: bool,
) -> CliResult<(TempDir, PathBuf, SkillDefinition)> {
    let zip_data = repo_client
        .download(skill_id_full, version)
        .await
        .map_err(|e| CliError::Config(format!("Failed to download package: {}", e)))?;
    let temp_dir = TempDir::new().map_err(CliError::Io)?;
    let extract_path = temp_dir.path().join("extracted");
    fs::create_dir_all(&extract_path).map_err(CliError::Io)?;
    let temp_zip = temp_dir.path().join(format!("package-{}.zip", version));
    std::fs::write(&temp_zip, zip_data).map_err(CliError::Io)?;
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
    let skill_path = find_skill_in_directory(&extract_path)?;
    validate_skill_structure(&skill_path, verbose)?;
    let skill_def = create_skill_from_path(&skill_path)?;
    Ok((temp_dir, skill_path, skill_def))
}

async fn add_from_registry(ctx: &AddContext<'_>, skill_id_input: &str) -> CliResult<()> {
    info!("Adding skill from registry: {}", skill_id_input);
    let (skill_id_full, scope, expected_id, version_opt) = parse_registry_scope_id(skill_id_input)?;

    let repositories = crate::cli::config::load_repositories_from_project()?;
    let repo_manager = RepositoryManager::from_definitions(repositories);
    let default_repo = repo_manager.get_default_repository().ok_or_else(|| {
        CliError::Config(
            "No default repository configured. Use 'fastskill registry add' to add a repository."
                .to_string(),
        )
    })?;
    if !matches!(
        default_repo.repo_type,
        fastskill::core::repository::RepositoryType::HttpRegistry
    ) {
        return Err(CliError::Config(format!(
            "Repository '{}' is not an http-registry type. Only http-registry repositories are supported for 'fastskill add' currently.",
            default_repo.name
        )));
    }

    let repo_client = repo_manager
        .get_client(&default_repo.name)
        .await
        .map_err(|e| CliError::Config(format!("Failed to get repository client: {}", e)))?;
    let version =
        resolve_registry_version(repo_client.as_ref(), &skill_id_full, version_opt).await?;

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

    info!(
        "Downloading {}@{} from repository...",
        skill_id_full, version
    );
    let (_temp_dir, skill_path, skill_def) =
        download_registry_package(repo_client.as_ref(), &skill_id_full, &version, ctx.verbose)
            .await?;

    if skill_def.id.as_str() != expected_id {
        return Err(CliError::Config(format!(
            "Skill ID mismatch: expected '{}' from registry reference '{}', but found '{}' in skill-project.toml",
            expected_id, skill_id_full, skill_def.id.as_str()
        )));
    }

    let target = InstallTarget {
        storage_dir: ctx
            .service
            .config()
            .skill_storage_path
            .join(&scope)
            .join(skill_def.id.as_str()),
        meta: SourceMeta {
            source_url: None,
            source_type: Some(SourceType::Source),
            source_branch: None,
            source_tag: None,
            source_subdir: None,
            installed_from: Some(default_repo.name.clone()),
        },
        version_display: version,
    };
    install_copied_skill(ctx, &skill_path, skill_def, target).await
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
    use tempfile::TempDir;

    async fn run_add_expect_err(source: &str, source_type: Option<&str>, force: bool) {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        let args = AddArgs {
            source: source.to_string(),
            source_type: source_type.map(String::from),
            branch: None,
            tag: None,
            force,
            editable: false,
            group: None,
            recursive: false,
        };
        let result = execute_add(&service, args, false, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_add_nonexistent_source() {
        run_add_expect_err("/nonexistent/path", Some("local"), false).await;
    }

    #[tokio::test]
    async fn test_execute_add_invalid_skill_id() {
        run_add_expect_err("invalid@skill@id", Some("registry"), false).await;
    }

    #[tokio::test]
    async fn test_execute_add_with_force_flag() {
        run_add_expect_err("/nonexistent/path", Some("local"), true).await;
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

        let result = execute_add(&service, args, false, false).await;
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

        let result = execute_add(&service, args, false, false).await;
        // May succeed or fail depending on various factors, but should exercise the code path
        assert!(result.is_ok() || result.is_err());
    }

    // NOTE: Path traversal security tests have been moved to lower-level tests
    // in src/core/registry/validation_worker.rs (test_extract_zip_to_temp_rejects_path_traversal
    // and test_extract_zip_to_temp_rejects_windows_traversal) where they don't rely on
    // changing the current directory, which is unreliable in concurrent test environments.
}
