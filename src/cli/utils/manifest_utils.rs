//! Utilities for managing skills.toml and skills.lock

use chrono::Utc;
use fastskill::core::{
    lock::SkillsLock,
    manifest::{SkillEntry, SkillSource, SkillsManifest},
    skill_manager::SkillDefinition,
};
use std::path::Path;

/// Update skills.toml with a new skill entry
pub fn add_skill_to_manifest(
    manifest_path: &Path,
    skill: &SkillDefinition,
    groups: Vec<String>,
    editable: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Ensure .claude directory exists
    if let Some(parent) = manifest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Load existing manifest or create new
    let mut manifest = if manifest_path.exists() {
        SkillsManifest::load_from_file(manifest_path)
            .map_err(|e| format!("Failed to load manifest: {}", e))?
    } else {
        SkillsManifest {
            metadata: fastskill::core::manifest::ManifestMetadata {
                version: "1.0.0".to_string(),
            },
            skills: Vec::new(),
            proxy: None,
        }
    };

    // Convert SkillDefinition to SkillSource
    let source = if let Some(source_type) = &skill.source_type {
        match source_type {
            fastskill::core::skill_manager::SourceType::GitUrl => SkillSource::Git {
                url: skill.source_url.clone().unwrap_or_default(),
                branch: skill.source_branch.clone(),
                tag: skill.source_tag.clone(),
                subdir: skill.source_subdir.clone(),
            },
            fastskill::core::skill_manager::SourceType::LocalPath => SkillSource::Local {
                path: skill.source_subdir.clone().unwrap_or_else(|| {
                    std::path::PathBuf::from(skill.source_url.clone().unwrap_or_default())
                }),
                editable,
            },
            fastskill::core::skill_manager::SourceType::ZipFile => SkillSource::ZipUrl {
                base_url: skill.source_url.clone().unwrap_or_default(),
                version: Some(skill.version.clone()),
            },
            fastskill::core::skill_manager::SourceType::Source => SkillSource::Source {
                name: skill.installed_from.clone().unwrap_or_default(),
                skill: skill.id.to_string(),
                version: Some(skill.version.clone()),
            },
        }
    } else {
        // Default to local path if source type unknown
        SkillSource::Local {
            path: std::path::PathBuf::from(skill.id.as_str()),
            editable,
        }
    };

    let entry = SkillEntry {
        id: skill.id.to_string(),
        source,
        version: Some(skill.version.clone()),
        groups,
        editable,
    };

    // Remove existing entry if present
    manifest.remove_skill(skill.id.as_str());
    manifest.add_skill(entry);

    // Save manifest
    manifest
        .save_to_file(manifest_path)
        .map_err(|e| format!("Failed to save manifest: {}", e))?;

    Ok(())
}

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

/// Remove skill from both manifest and lock files
pub fn remove_skill_from_manifest(
    manifest_path: &Path,
    lock_path: &Path,
    skill_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Remove from manifest
    if manifest_path.exists() {
        let mut manifest = SkillsManifest::load_from_file(manifest_path)
            .map_err(|e| format!("Failed to load manifest: {}", e))?;
        manifest.remove_skill(skill_id);
        manifest
            .save_to_file(manifest_path)
            .map_err(|e| format!("Failed to save manifest: {}", e))?;
    }

    // Remove from lock
    if lock_path.exists() {
        let mut lock = SkillsLock::load_from_file(lock_path)
            .map_err(|e| format!("Failed to load lock file: {}", e))?;
        lock.remove_skill(skill_id);
        lock.save_to_file(lock_path)
            .map_err(|e| format!("Failed to save lock file: {}", e))?;
    }

    Ok(())
}
