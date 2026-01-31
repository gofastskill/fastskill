//! Utilities for managing skill-project.toml and skills.lock

use chrono::Utc;
use fastskill::core::{
    lock::SkillsLock,
    manifest::{
        DependenciesSection, DependencySource, DependencySpec, SkillProjectToml,
        SourceSpecificFields,
    },
    project::resolve_project_file,
    skill_manager::SkillDefinition,
};
use std::collections::HashMap;
use std::env;
use std::path::Path;

/// Update skills.lock with installed skill state
pub fn update_lock_file(
    lock_path: &Path,
    skill: &SkillDefinition,
    groups: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Ensure .claude directory exists
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Load existing lock or create new
    let mut lock = if lock_path.exists() {
        SkillsLock::load_from_file(lock_path)
            .map_err(|e| format!("Failed to load lock file: {}", e))?
    } else {
        SkillsLock {
            metadata: fastskill::core::lock::LockMetadata {
                version: "1.0.0".to_string(),
                generated_at: Utc::now(),
                fastskill_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            },
            skills: Vec::new(),
        }
    };

    // Update lock with skill
    lock.update_skill(skill);

    // Update groups (we need to modify the locked entry)
    if let Some(locked_entry) = lock.skills.iter_mut().find(|s| s.id == skill.id.as_str()) {
        locked_entry.groups = groups;
    }

    // Save lock
    lock.save_to_file(lock_path)
        .map_err(|e| format!("Failed to save lock file: {}", e))?;

    Ok(())
}

/// T028: Add skill to skill-project.toml [dependencies] section
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

    // T032: Create file if it doesn't exist
    let mut project = if project_file_result.found {
        let loaded_project = SkillProjectToml::load_from_file(&project_file_path)
            .map_err(|e| format!("Failed to load skill-project.toml: {}", e))?;

        // T052: Validate context - add command requires project-level context
        let mut context = project_file_result.context;
        if context == fastskill::core::manifest::ProjectContext::Ambiguous {
            // Use content-based detection for ambiguous cases
            context = fastskill::core::project::detect_context_from_content(&loaded_project);
        }

        // Validate that we're in project context (not skill context)
        if context == fastskill::core::manifest::ProjectContext::Skill {
            return Err(
                "Cannot add dependencies to skill-level skill-project.toml. \
                This file is in a skill directory (contains SKILL.md). \
                Use 'fastskill add' from the project root instead."
                    .to_string()
                    .into(),
            );
        }

        loaded_project
            .validate_for_context(context)
            .map_err(|e| format!("skill-project.toml validation failed: {}", e))?;

        loaded_project
    } else {
        // Create new project file (defaults to project context)
        SkillProjectToml {
            metadata: None,
            dependencies: Some(DependenciesSection {
                dependencies: HashMap::new(),
            }),
            tool: None,
        }
    };

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
            fastskill::core::skill_manager::SourceType::GitUrl => {
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
            fastskill::core::skill_manager::SourceType::LocalPath => {
                let path_str = skill
                    .source_subdir
                    .clone()
                    .or_else(|| skill.source_url.clone().map(std::path::PathBuf::from))
                    .unwrap_or_else(|| std::path::PathBuf::from(skill.id.as_str()))
                    .to_string_lossy()
                    .to_string();
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
            fastskill::core::skill_manager::SourceType::ZipFile => {
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
            fastskill::core::skill_manager::SourceType::Source => {
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
    if context == fastskill::core::manifest::ProjectContext::Ambiguous {
        // Use content-based detection for ambiguous cases
        context = fastskill::core::project::detect_context_from_content(&project);
    }

    // Validate that we're in project context (not skill context)
    if context == fastskill::core::manifest::ProjectContext::Skill {
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
