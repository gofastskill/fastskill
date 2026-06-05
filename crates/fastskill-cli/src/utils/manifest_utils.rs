//! Utilities for managing skill-project.toml and skills.lock

use fastskill_core::core::{
    lock::{global_lock_path, GlobalSkillsLock, LockError, ProjectSkillsLock},
    manifest::{
        DependenciesSection, DependencySource, DependencySpec, SkillProjectToml,
        SourceSpecificFields,
    },
    project::resolve_project_file,
    skill_manager::SkillDefinition,
};
use fs2::FileExt;
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::time::Duration;

/// Acquire an exclusive advisory file lock on a sidecar file.
/// Retries up to 3 times with 200 ms sleep. Returns error if still contended.
fn acquire_advisory_lock(sidecar_path: &std::path::Path) -> Result<std::fs::File, LockError> {
    if let Some(parent) = sidecar_path.parent() {
        std::fs::create_dir_all(parent).map_err(LockError::Io)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(sidecar_path)
        .map_err(LockError::Io)?;

    for _ in 0..3 {
        if file.try_lock_exclusive().is_ok() {
            return Ok(file);
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    Err(LockError::FileLocked(sidecar_path.to_path_buf()))
}

/// Build the sidecar path for advisory locking: `<lock_path>.lock`
fn sidecar_path(lock_path: &std::path::Path) -> std::path::PathBuf {
    let mut s = lock_path.as_os_str().to_owned();
    s.push(".lock");
    std::path::PathBuf::from(s)
}

/// Update skills.lock with installed skill state
pub fn update_lock_file(
    lock_path: &Path,
    skill: &SkillDefinition,
    groups: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    update_lock_file_with_depth(lock_path, skill, groups, 0, None)
}

/// Update skills.lock with installed skill state including depth and parent info
pub fn update_lock_file_with_depth(
    lock_path: &Path,
    skill: &SkillDefinition,
    groups: Vec<String>,
    depth: u32,
    parent_skill: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let sidecar = sidecar_path(lock_path);
    let _guard = acquire_advisory_lock(&sidecar)
        .map_err(|e| format!("Failed to acquire lock on skills.lock: {}", e))?;

    let mut lock = if lock_path.exists() {
        ProjectSkillsLock::load_from_file(lock_path)
            .map_err(|e| format!("Failed to load lock file: {}", e))?
    } else {
        ProjectSkillsLock::new_empty()
    };

    lock.update_skill_with_depth(skill, depth, parent_skill);

    if let Some(locked_entry) = lock.skills.iter_mut().find(|s| s.id == skill.id.as_str()) {
        locked_entry.groups = groups;
    }

    lock.save_to_file(lock_path)
        .map_err(|e| format!("Failed to save lock file: {}", e))?;

    // Sidecar file lock is released when _guard (the File) is dropped
    let _ = std::fs::remove_file(&sidecar);

    Ok(())
}

/// Update the global `global-skills.lock` for a globally installed skill.
pub fn update_global_lock_file(
    skill: &SkillDefinition,
    installed_at: chrono::DateTime<chrono::Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let lock_path =
        global_lock_path().map_err(|e| format!("Failed to resolve global lock path: {}", e))?;

    let sidecar = sidecar_path(&lock_path);
    let _guard = acquire_advisory_lock(&sidecar)
        .map_err(|e| format!("Failed to acquire lock on global-skills.lock: {}", e))?;

    let mut lock = if lock_path.exists() {
        GlobalSkillsLock::load_from_file(&lock_path)
            .map_err(|e| format!("Failed to load global lock file: {}", e))?
    } else {
        GlobalSkillsLock::new_empty()
    };

    lock.upsert_skill(skill, installed_at);

    lock.save_to_file(&lock_path)
        .map_err(|e| format!("Failed to save global lock file: {}", e))?;

    let _ = std::fs::remove_file(&sidecar);

    Ok(())
}

/// Remove a skill from the global `global-skills.lock`.
pub fn remove_from_global_lock_file(skill_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let lock_path =
        global_lock_path().map_err(|e| format!("Failed to resolve global lock path: {}", e))?;

    if !lock_path.exists() {
        return Ok(());
    }

    let sidecar = sidecar_path(&lock_path);
    let _guard = acquire_advisory_lock(&sidecar)
        .map_err(|e| format!("Failed to acquire lock on global-skills.lock: {}", e))?;

    let mut lock = GlobalSkillsLock::load_from_file(&lock_path)
        .map_err(|e| format!("Failed to load global lock file: {}", e))?;

    lock.remove_skill(skill_id);

    lock.save_to_file(&lock_path)
        .map_err(|e| format!("Failed to save global lock file: {}", e))?;

    let _ = std::fs::remove_file(&sidecar);

    Ok(())
}

/// T028: Add skill to skill-project.toml [dependencies] section.
/// Fails if skill-project.toml is not found in the hierarchy (we never auto-create it).
///
/// Local paths are canonicalized to absolute paths before being written to the TOML file.
/// This ensures consistent behavior regardless of the current working directory when
/// the skill is added.
pub fn add_skill_to_project_toml(
    skill: &SkillDefinition,
    groups: Vec<String>,
    editable: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Resolve project file from current directory
    let current_dir =
        env::current_dir().map_err(|e| format!("Failed to get current directory: {}", e))?;
    let project_file_result = resolve_project_file(&current_dir);
    let project_file_path = project_file_result.path;

    if !project_file_result.found {
        return Err(
            "skill-project.toml not found in this directory or any parent. \
             Create it at the top level of your workspace (e.g. run 'fastskill init' there), \
             then run 'fastskill add <skill>' from your project."
                .to_string()
                .into(),
        );
    }

    let mut project = SkillProjectToml::load_from_file(&project_file_path)
        .map_err(|e| format!("Failed to load skill-project.toml: {}", e))?;

    // T052: Validate context - add command requires project-level context
    let mut context = project_file_result.context;
    if context == fastskill_core::core::manifest::ProjectContext::Ambiguous {
        context = fastskill_core::core::project::detect_context_from_content(&project);
    }

    if context == fastskill_core::core::manifest::ProjectContext::Skill {
        return Err(
            "Cannot add dependencies to skill-level skill-project.toml. \
            This file is in a skill directory (contains SKILL.md). \
            Use 'fastskill add' from the project root instead."
                .to_string()
                .into(),
        );
    }

    project
        .validate_for_context(context)
        .map_err(|e| format!("skill-project.toml validation failed: {}", e))?;

    // Ensure dependencies section exists
    if project.dependencies.is_none() {
        project.dependencies = Some(DependenciesSection {
            dependencies: HashMap::new(),
        });
    }

    let deps = project
        .dependencies
        .as_mut()
        .ok_or_else(|| "Failed to initialize dependencies section".to_string())?;

    // Convert SkillDefinition to DependencySpec
    let dep_spec = if let Some(source_type) = &skill.source_type {
        match source_type {
            fastskill_core::core::skill_manager::SourceType::GitUrl => {
                let source_specific = SourceSpecificFields {
                    url: skill.source_url.clone(),
                    branch: skill.source_branch.clone(),
                    path: None,
                    name: None,
                    skill: None,
                    zip_url: None,
                    version: None,
                };
                DependencySpec::Inline {
                    source: DependencySource::Git,
                    source_specific,
                    groups: if groups.is_empty() {
                        None
                    } else {
                        Some(groups)
                    },
                    editable: if editable { Some(true) } else { None },
                }
            }
            fastskill_core::core::skill_manager::SourceType::LocalPath => {
                let path_buf = skill
                    .source_subdir
                    .clone()
                    .or_else(|| skill.source_url.clone().map(std::path::PathBuf::from))
                    .unwrap_or_else(|| std::path::PathBuf::from(skill.id.as_str()));

                // Canonicalize to ensure absolute path (safety net)
                let canonical_path = path_buf.canonicalize().map_err(|e| {
                    format!(
                        "Failed to canonicalize path '{}': {}",
                        path_buf.display(),
                        e
                    )
                })?;

                let path_str = canonical_path.to_string_lossy().to_string();

                let source_specific = SourceSpecificFields {
                    url: None,
                    branch: None,
                    path: Some(path_str),
                    name: None,
                    skill: None,
                    zip_url: None,
                    version: None,
                };
                DependencySpec::Inline {
                    source: DependencySource::Local,
                    source_specific,
                    groups: if groups.is_empty() {
                        None
                    } else {
                        Some(groups)
                    },
                    editable: if editable { Some(true) } else { None },
                }
            }
            fastskill_core::core::skill_manager::SourceType::ZipFile => {
                let source_specific = SourceSpecificFields {
                    url: None,
                    branch: None,
                    path: None,
                    name: None,
                    skill: None,
                    zip_url: skill.source_url.clone(),
                    version: Some(skill.version.clone()),
                };
                DependencySpec::Inline {
                    source: DependencySource::ZipUrl,
                    source_specific,
                    groups: if groups.is_empty() {
                        None
                    } else {
                        Some(groups)
                    },
                    editable: None,
                }
            }
            fastskill_core::core::skill_manager::SourceType::Source => {
                let source_specific = SourceSpecificFields {
                    url: None,
                    branch: None,
                    path: None,
                    name: skill.installed_from.clone(),
                    skill: Some(skill.id.to_string()),
                    zip_url: None,
                    version: Some(skill.version.clone()),
                };
                DependencySpec::Inline {
                    source: DependencySource::Source,
                    source_specific,
                    groups: if groups.is_empty() {
                        None
                    } else {
                        Some(groups)
                    },
                    editable: None,
                }
            }
        }
    } else {
        // Default: version string (source-based)
        DependencySpec::Version(skill.version.clone())
    };

    // Add or update dependency
    deps.dependencies.insert(skill.id.to_string(), dep_spec);

    // Save project file
    project
        .save_to_file(&project_file_path)
        .map_err(|e| format!("Failed to save skill-project.toml: {}", e))?;

    Ok(())
}

/// T029: Remove skill from skill-project.toml [dependencies] section
pub fn remove_skill_from_project_toml(skill_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Resolve project file from current directory
    let current_dir =
        env::current_dir().map_err(|e| format!("Failed to get current directory: {}", e))?;
    let project_file_result = resolve_project_file(&current_dir);
    let project_file_path = project_file_result.path;

    // If file doesn't exist, nothing to remove
    if !project_file_result.found {
        return Ok(());
    }

    // Load project file
    let mut project = SkillProjectToml::load_from_file(&project_file_path)
        .map_err(|e| format!("Failed to load skill-project.toml: {}", e))?;

    // T054: Validate context - remove command requires project-level context
    let mut context = project_file_result.context;
    if context == fastskill_core::core::manifest::ProjectContext::Ambiguous {
        // Use content-based detection for ambiguous cases
        context = fastskill_core::core::project::detect_context_from_content(&project);
    }

    // Validate that we're in project context (not skill context)
    if context == fastskill_core::core::manifest::ProjectContext::Skill {
        return Err(
            "Cannot remove dependencies from skill-level skill-project.toml. \
            This file is in a skill directory (contains SKILL.md). \
            Use 'fastskill remove' from the project root instead."
                .to_string()
                .into(),
        );
    }

    project
        .validate_for_context(context)
        .map_err(|e| format!("skill-project.toml validation failed: {}", e))?;

    // Remove from dependencies if present
    if let Some(ref mut deps) = project.dependencies {
        deps.dependencies.remove(skill_id);
    }

    // Save project file
    project
        .save_to_file(&project_file_path)
        .map_err(|e| format!("Failed to save skill-project.toml: {}", e))?;

    Ok(())
}
